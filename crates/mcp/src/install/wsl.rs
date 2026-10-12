#![cfg_attr(not(windows), allow(dead_code))]

use std::path::PathBuf;

const DISTRO_SKIP_PREFIX: &str = "docker-desktop";

pub(super) fn decode_output(bytes: &[u8]) -> String {
    let looks_utf16 = bytes.starts_with(&[0xff, 0xfe])
        || bytes
            .iter()
            .skip(1)
            .step_by(2)
            .take(8)
            .all(|byte| *byte == 0)
            && bytes.len() >= 2;
    let text = if looks_utf16 {
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    text.trim_start_matches('\u{feff}').replace('\0', "")
}

pub(super) fn default_distro(list_output: &str) -> Option<String> {
    list_output
        .lines()
        .map(|line| line.trim().trim_end_matches("(Default)").trim())
        .find(|name| !name.is_empty() && !name.starts_with(DISTRO_SKIP_PREFIX))
        .map(str::to_string)
}

pub(super) fn unc_home(distro: &str, linux_home: &str) -> PathBuf {
    let relative = linux_home.trim().trim_matches('/').replace('/', "\\");
    PathBuf::from(format!("\\\\wsl.localhost\\{distro}\\{relative}"))
}

pub(super) fn wslconfig_is_mirrored(text: &str) -> bool {
    text.lines()
        .filter_map(|line| line.split('#').next()?.split_once('='))
        .any(|(key, value)| {
            key.trim().eq_ignore_ascii_case("networkingMode")
                && value.trim().eq_ignore_ascii_case("mirrored")
        })
}

#[cfg(windows)]
mod spawn {
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;
    use std::process::Command;

    use super::{decode_output, default_distro, unc_home, wslconfig_is_mirrored};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn run(arguments: &[&str]) -> Option<String> {
        let output = Command::new("wsl.exe")
            .args(arguments)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| decode_output(&output.stdout))
    }

    pub fn default_home() -> Option<(String, PathBuf)> {
        let distro = default_distro(&run(&["-l", "-q"])?)?;
        let linux_home = run(&["-d", &distro, "--cd", "~", "-e", "pwd"])?;
        let home = unc_home(&distro, linux_home.trim());
        Some((distro, home))
    }

    pub fn reachable(port: u16) -> Option<bool> {
        let probe = format!(
            "curl -s -m 2 http://127.0.0.1:{port}/healthz || wget -qO- -T 2 http://127.0.0.1:{port}/healthz"
        );
        match run(&["-e", "sh", "-c", &probe]) {
            Some(output) => Some(output.trim() == "ok"),
            None => mirrored_networking().then_some(true),
        }
    }

    fn mirrored_networking() -> bool {
        std::env::var_os("USERPROFILE")
            .map(|profile| PathBuf::from(profile).join(".wslconfig"))
            .and_then(|path| std::fs::read_to_string(path).ok())
            .is_some_and(|text| wslconfig_is_mirrored(&text))
    }
}

#[cfg(windows)]
pub(super) use spawn::{default_home, reachable};
