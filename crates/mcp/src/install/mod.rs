mod deeplink;
mod entry;
mod error;
mod file;
mod json_file;
mod toml_file;
mod wsl;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use crate::McpEndpoint;
use entry::ServerEntry;
pub use error::InstallError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Target {
    ClaudeCode,
    Codex,
    CopilotCli,
    VsCode,
    Cursor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    Windows,
    Wsl { distro: String },
    Native,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallState {
    pub target: Target,
    pub available: bool,
    pub installed_at: Vec<Location>,
    pub stale: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallReport {
    Written(Vec<Location>),
    OpenUrl(String),
}

#[derive(Debug, Clone, Default)]
pub struct Homes {
    pub entries: Vec<(Location, PathBuf)>,
}

impl Homes {
    pub fn detect() -> Homes {
        let mut entries = Vec::new();
        #[cfg(windows)]
        {
            if let Some(profile) = std::env::var_os("USERPROFILE") {
                entries.push((Location::Windows, PathBuf::from(profile)));
            }
            if let Some((distro, home)) = wsl::default_home() {
                entries.push((Location::Wsl { distro }, home));
            }
        }
        #[cfg(not(windows))]
        if let Some(home) = std::env::var_os("HOME") {
            entries.push((Location::Native, PathBuf::from(home)));
        }
        Homes { entries }
    }
}

#[cfg(windows)]
pub fn wsl_reachable(port: u16) -> Option<bool> {
    wsl::reachable(port)
}

#[cfg(not(windows))]
pub fn wsl_reachable(_port: u16) -> Option<bool> {
    None
}

enum Format {
    Json { container: &'static str },
    Toml,
}

impl Target {
    pub const ALL: [Target; 5] = [
        Target::ClaudeCode,
        Target::Codex,
        Target::CopilotCli,
        Target::VsCode,
        Target::Cursor,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Target::ClaudeCode => "Claude Code",
            Target::Codex => "Codex CLI",
            Target::CopilotCli => "Copilot CLI",
            Target::VsCode => "VS Code",
            Target::Cursor => "Cursor",
        }
    }

    pub fn config_hint(self) -> &'static str {
        match self {
            Target::ClaudeCode => "~/.claude.json",
            Target::Codex => "~/.codex/config.toml",
            Target::CopilotCli => "~/.copilot/mcp-config.json",
            Target::VsCode => "Code/User/mcp.json",
            Target::Cursor => "~/.cursor/mcp.json",
        }
    }

    fn format(self) -> Format {
        match self {
            Target::Codex => Format::Toml,
            Target::VsCode => Format::Json {
                container: "servers",
            },
            _ => Format::Json {
                container: "mcpServers",
            },
        }
    }

    fn config_path(self, location: &Location, home: &Path) -> PathBuf {
        match self {
            Target::ClaudeCode => home.join(".claude.json"),
            Target::Codex => home.join(".codex").join("config.toml"),
            Target::CopilotCli => home.join(".copilot").join("mcp-config.json"),
            Target::Cursor => home.join(".cursor").join("mcp.json"),
            Target::VsCode => {
                let base = match location {
                    Location::Windows => home.join("AppData").join("Roaming"),
                    _ => home.join(".config"),
                };
                base.join("Code").join("User").join("mcp.json")
            }
        }
    }

    fn marker_paths(self, location: &Location, home: &Path) -> Vec<PathBuf> {
        let config = self.config_path(location, home);
        let mut markers = vec![config.clone()];
        match self {
            Target::ClaudeCode => markers.push(home.join(".claude")),
            _ => markers.extend(config.parent().map(Path::to_path_buf)),
        }
        markers
    }

    fn binary_names(self) -> &'static [&'static str] {
        match self {
            Target::ClaudeCode => &["claude"],
            Target::Codex => &["codex"],
            Target::CopilotCli => &["copilot"],
            Target::VsCode => &["code"],
            Target::Cursor => &["cursor"],
        }
    }

    fn is_available_in(self, location: &Location, home: &Path) -> bool {
        self.marker_paths(location, home)
            .iter()
            .any(|path| path.exists())
            || matches!(location, Location::Windows | Location::Native)
                && self.binary_names().iter().any(|name| binary_on_path(name))
    }

    fn json_entry(self, entry: &ServerEntry) -> serde_json::Value {
        match self {
            Target::ClaudeCode | Target::VsCode => entry.to_json(true, false),
            Target::CopilotCli => entry.to_json(true, true),
            _ => entry.to_json(false, false),
        }
    }

    fn read_entry(self, path: &Path) -> Result<Option<ServerEntry>, InstallError> {
        match self.format() {
            Format::Json { container } => json_file::read_entry(path, container),
            Format::Toml => toml_file::read_entry(path),
        }
    }

    fn write_entry(self, path: &Path, entry: &ServerEntry) -> Result<(), InstallError> {
        match self.format() {
            Format::Json { container } => {
                json_file::upsert(path, container, &self.json_entry(entry))
            }
            Format::Toml => toml_file::upsert(path, entry),
        }
    }
}

fn binary_on_path(name: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|directory| {
        ["", ".exe", ".cmd"]
            .iter()
            .any(|extension| directory.join(format!("{name}{extension}")).is_file())
    })
}

pub fn status(target: Target, endpoint: &McpEndpoint, homes: &Homes) -> InstallState {
    let expected = ServerEntry::expected(endpoint);
    let mut state = InstallState {
        target,
        available: false,
        installed_at: Vec::new(),
        stale: false,
    };
    for (location, home) in &homes.entries {
        state.available |= target.is_available_in(location, home);
        let path = target.config_path(location, home);
        if let Ok(Some(found)) = target.read_entry(&path) {
            state.stale |= found != expected;
            state.installed_at.push(location.clone());
        }
    }
    state
}

pub fn install(
    target: Target,
    endpoint: &McpEndpoint,
    homes: &Homes,
) -> Result<InstallReport, InstallError> {
    let entry = ServerEntry::expected(endpoint);
    if target == Target::VsCode {
        return Ok(InstallReport::OpenUrl(deeplink::vscode(&entry)));
    }
    let mut written = Vec::new();
    for (location, home) in &homes.entries {
        if !target.is_available_in(location, home) {
            continue;
        }
        target.write_entry(&target.config_path(location, home), &entry)?;
        written.push(location.clone());
    }
    if written.is_empty() {
        return Err(InstallError::NotAvailable(target));
    }
    Ok(InstallReport::Written(written))
}

pub fn uninstall(target: Target, homes: &Homes) -> Result<(), InstallError> {
    for (location, home) in &homes.entries {
        let path = target.config_path(location, home);
        match target.format() {
            Format::Json { container } => json_file::remove(&path, container)?,
            Format::Toml => toml_file::remove(&path)?,
        }
    }
    Ok(())
}

pub fn reinstall_all(
    endpoint: &McpEndpoint,
    homes: &Homes,
) -> Vec<(Target, Result<(), InstallError>)> {
    let entry = ServerEntry::expected(endpoint);
    let mut results = Vec::new();
    for target in Target::ALL {
        for (location, home) in &homes.entries {
            let path = target.config_path(location, home);
            if !matches!(target.read_entry(&path), Ok(Some(_))) {
                continue;
            }
            results.push((target, target.write_entry(&path, &entry)));
        }
    }
    results
}
