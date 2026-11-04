use std::io;
use std::path::Path;

pub const AUTOSTART_FLAG: &str = "--autostart";
const BACKGROUND_META_KEY: &str = "startup.open_in_background";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StartupSettings {
    pub auto_start: bool,
    pub open_in_background: bool,
}

impl StartupSettings {
    pub fn load(store: &store::Store, read_system: bool) -> StartupSettings {
        StartupSettings {
            auto_start: read_system && is_enabled(crate::APP_NAME),
            open_in_background: store.meta(BACKGROUND_META_KEY).ok().flatten().as_deref()
                == Some("1"),
        }
    }

    pub fn save_background(&self, store: &store::Store) {
        let _ = store.set_meta(
            BACKGROUND_META_KEY,
            if self.open_in_background { "1" } else { "0" },
        );
    }

    pub fn starts_hidden(&self, launched_by_autostart: bool) -> bool {
        launched_by_autostart && self.auto_start && self.open_in_background
    }
}

pub fn launch_command(executable: &Path) -> String {
    format!("\"{}\" {AUTOSTART_FLAG}", executable.display())
}

pub fn desktop_entry(name: &str, executable: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName={name}\nExec={}\nX-GNOME-Autostart-enabled=true\n",
        launch_command(executable)
    )
}

pub fn launched_by_autostart(arguments: impl IntoIterator<Item = String>) -> bool {
    arguments
        .into_iter()
        .any(|argument| argument == AUTOSTART_FLAG)
}

#[cfg(not(windows))]
fn entry_path(name: &str) -> Option<std::path::PathBuf> {
    let file_name = format!("{}.desktop", name.to_lowercase().replace(' ', "-"));
    directories::BaseDirs::new().map(|base| base.config_dir().join("autostart").join(file_name))
}

#[cfg(not(windows))]
pub fn is_enabled(name: &str) -> bool {
    entry_path(name).is_some_and(|path| path.is_file())
}

#[cfg(not(windows))]
pub fn set_enabled(name: &str, enabled: bool) -> io::Result<()> {
    let path = entry_path(name).ok_or_else(|| io::Error::other("no config directory"))?;
    if !enabled {
        return match std::fs::remove_file(&path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        };
    }
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory)?;
    }
    std::fs::write(&path, desktop_entry(name, &std::env::current_exe()?))
}

#[cfg(windows)]
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

#[cfg(windows)]
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
pub fn is_enabled(name: &str) -> bool {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
    use windows::core::PCWSTR;
    let (key, value) = (wide(RUN_KEY), wide(name));
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            None,
            None,
        )
        .is_ok()
    }
}

#[cfg(windows)]
pub fn set_enabled(name: &str, enabled: bool) -> io::Result<()> {
    use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    use windows::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RegDeleteKeyValueW, RegSetKeyValueW,
    };
    use windows::core::PCWSTR;
    let (key, value) = (wide(RUN_KEY), wide(name));
    let status = if enabled {
        let command = wide(&launch_command(&std::env::current_exe()?));
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                PCWSTR(key.as_ptr()),
                PCWSTR(value.as_ptr()),
                REG_SZ.0,
                Some(command.as_ptr().cast()),
                (command.len() * size_of::<u16>()) as u32,
            )
        }
    } else {
        match unsafe {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), PCWSTR(value.as_ptr()))
        } {
            ERROR_FILE_NOT_FOUND => return Ok(()),
            status => status,
        }
    };
    status
        .ok()
        .map_err(|error| io::Error::other(error.message()))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        AUTOSTART_FLAG, StartupSettings, desktop_entry, launch_command, launched_by_autostart,
    };

    #[test]
    fn background_choice_survives_a_restart_and_only_applies_to_autostart() {
        let store = store::Store::open_in_memory().unwrap();
        let loaded = StartupSettings::load(&store, false);
        assert_eq!(loaded, StartupSettings::default());
        let settings = StartupSettings {
            auto_start: true,
            open_in_background: true,
        };
        settings.save_background(&store);
        assert!(StartupSettings::load(&store, false).open_in_background);
        assert!(settings.starts_hidden(true));
        assert!(!settings.starts_hidden(false));
        assert!(
            !StartupSettings {
                auto_start: false,
                ..settings
            }
            .starts_hidden(true)
        );
    }

    #[test]
    fn the_start_command_quotes_the_path_and_marks_the_autostart() {
        let command = launch_command(Path::new("/opt/rusty teams/teams"));
        assert_eq!(command, format!("\"/opt/rusty teams/teams\" {AUTOSTART_FLAG}"));
        let entry = desktop_entry("Rusty Teams", Path::new("/opt/teams"));
        assert!(entry.starts_with("[Desktop Entry]\n"));
        assert!(entry.contains(&format!("Exec=\"/opt/teams\" {AUTOSTART_FLAG}\n")));
        assert!(entry.contains("Name=Rusty Teams\n"));
    }

    #[test]
    fn only_the_autostart_flag_marks_a_background_start() {
        assert!(launched_by_autostart(["--demo".into(), AUTOSTART_FLAG.into()]));
        assert!(!launched_by_autostart(["--demo".to_owned()]));
    }
}
