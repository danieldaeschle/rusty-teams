use std::time::{Duration, Instant};

use crate::config::Platform;
use crate::error::BrowserError;

pub(crate) fn parse_ss_pid(output: &str) -> Option<u32> {
    let after = output.split("pid=").nth(1)?;
    after
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()
}

pub(crate) fn parse_netstat_pid(output: &str, port: u16) -> Option<u32> {
    let suffix = format!(":{port}");
    output.lines().find_map(|line| {
        let columns: Vec<&str> = line.split_whitespace().collect();
        let listening = columns
            .iter()
            .any(|column| column.eq_ignore_ascii_case("LISTENING"));
        let local = columns.get(1)?;
        if !listening || !local.ends_with(&suffix) {
            return None;
        }
        columns.last()?.parse().ok()
    })
}

/// First LISTEN row's PID column from `lsof -nP -iTCP:<port> -sTCP:LISTEN`.
pub(crate) fn parse_lsof_pid(output: &str) -> Option<u32> {
    output.lines().skip(1).find_map(|line| {
        let columns: Vec<&str> = line.split_whitespace().collect();
        columns.get(1)?.parse().ok()
    })
}

fn run(program: &str, arguments: &[&str]) -> Result<String, BrowserError> {
    let output = crate::host::command(program).args(arguments).output()?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn listener_pid(platform: Platform, port: u16) -> Result<u32, BrowserError> {
    let pid = match platform {
        Platform::Linux => parse_ss_pid(&run("ss", &["-ltnpH", &format!("sport = :{port}")])?),
        Platform::Macos => parse_lsof_pid(&run(
            "lsof",
            &["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN"],
        )?),
        Platform::Windows => parse_netstat_pid(&run("netstat", &["-ano", "-p", "TCP"])?, port),
        Platform::Wsl => parse_netstat_pid(&run("netstat.exe", &["-ano", "-p", "TCP"])?, port),
    };
    pid.ok_or_else(|| BrowserError::Cdp(format!("no process listens on port {port}")))
}

/// Lowercased basename: macOS `ps -o comm=` may return a full path.
fn image_base(image_name: &str) -> String {
    let trimmed = image_name.trim().trim_matches('"');
    let base = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed).trim();
    base.to_ascii_lowercase()
}

pub(crate) fn is_chrome_image(image_name: &str) -> bool {
    let lowered = image_base(image_name);
    lowered.starts_with("chrome")
        || lowered.starts_with("google chrome")
        || lowered.starts_with("chromium")
        || lowered.starts_with("microsoft edge")
        || lowered.starts_with("msedge")
        || lowered.starts_with("brave")
        || lowered.starts_with("helium")
        || lowered.starts_with("vivaldi")
}

pub(crate) fn is_firefox_image(image_name: &str) -> bool {
    let lowered = image_base(image_name);
    ["firefox", "zen", "librewolf", "floorp"]
        .iter()
        .any(|name| lowered.starts_with(name))
}

fn process_image_name(platform: Platform, pid: &str) -> Result<String, BrowserError> {
    let filter = format!("PID eq {pid}");
    let csv_arguments = ["/FI", filter.as_str(), "/FO", "CSV", "/NH"];
    let output = match platform {
        Platform::Linux if crate::host::in_flatpak() => {
            run("cat", &[&format!("/proc/{pid}/comm")])?
        }
        Platform::Linux => std::fs::read_to_string(format!("/proc/{pid}/comm"))?,
        Platform::Macos => run("ps", &["-p", pid, "-o", "comm="])?,
        Platform::Windows => run("tasklist", &csv_arguments)?,
        Platform::Wsl => run("tasklist.exe", &csv_arguments)?,
    };
    Ok(output.split(',').next().unwrap_or_default().to_owned())
}

const EXIT_WAIT: Duration = Duration::from_secs(10);
const EXIT_POLL: Duration = Duration::from_millis(100);

/// Asks the browser holding `port` to quit with SIGTERM and waits until the process is gone, so
/// its profile is free again. Firefox on macOS keeps running after WebDriver BiDi `browser.close`.
pub(crate) fn terminate_listener(platform: Platform, port: u16) -> Result<(), BrowserError> {
    let pid = browser_listener_pid(platform, port)?;
    if !platform.uses_unix_paths() {
        return kill_pid(platform, &pid);
    }
    crate::host::command("kill")
        .args(["-TERM", &pid])
        .output()?;
    let deadline = Instant::now() + EXIT_WAIT;
    while Instant::now() < deadline {
        let alive = crate::host::command("kill")
            .args(["-0", &pid])
            .output()?
            .status
            .success();
        if !alive {
            return Ok(());
        }
        std::thread::sleep(EXIT_POLL);
    }
    Err(BrowserError::Cdp(format!(
        "pid {pid} did not exit after SIGTERM"
    )))
}

pub(crate) fn kill_listener(platform: Platform, port: u16) -> Result<(), BrowserError> {
    let pid = browser_listener_pid(platform, port)?;
    kill_pid(platform, &pid)
}

/// The PID listening on `port`, only when it is a browser: never signal anything else.
fn browser_listener_pid(platform: Platform, port: u16) -> Result<String, BrowserError> {
    let pid = listener_pid(platform, port)?.to_string();
    let image_name = process_image_name(platform, &pid)?;
    if !is_chrome_image(&image_name) && !is_firefox_image(&image_name) {
        return Err(BrowserError::Cdp(format!(
            "port {port} is held by a process that is no known browser, not killing it"
        )));
    }
    Ok(pid)
}

fn kill_pid(platform: Platform, pid: &str) -> Result<(), BrowserError> {
    let (program, arguments): (&str, Vec<&str>) = match platform {
        Platform::Linux | Platform::Macos => ("kill", vec!["-KILL", pid]),
        Platform::Windows => ("taskkill", vec!["/PID", pid, "/T", "/F"]),
        Platform::Wsl => ("taskkill.exe", vec!["/PID", pid, "/T", "/F"]),
    };
    let status = crate::host::command(program)
        .args(arguments)
        .output()?
        .status;
    if status.success() {
        Ok(())
    } else {
        Err(BrowserError::Cdp(format!("{program} failed for pid {pid}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_pid_from_ss() {
        let line = r#"LISTEN 0 511 127.0.0.1:9222 0.0.0.0:* users:(("chrome",pid=4242,fd=45))"#;
        assert_eq!(parse_ss_pid(line), Some(4242));
        assert_eq!(parse_ss_pid(""), None);
    }

    #[test]
    fn only_chrome_images_are_killable() {
        assert!(is_chrome_image("\"chrome.exe\""));
        assert!(is_chrome_image("chrome\n"));
        assert!(is_chrome_image("Google Chrome"));
        assert!(is_chrome_image("Chromium"));
        assert!(is_chrome_image(
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
        ));
        assert!(is_chrome_image("/usr/bin/chromium"));
        assert!(is_chrome_image(
            "/Applications/Helium.app/Contents/MacOS/Helium"
        ));
        assert!(is_chrome_image(
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"
        ));
        assert!(is_chrome_image(
            "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser"
        ));
        assert!(is_chrome_image("Vivaldi"));
        assert!(!is_chrome_image("\"wslrelay.exe\""));
        assert!(!is_chrome_image(
            "/Applications/Firefox.app/Contents/MacOS/firefox"
        ));
        assert!(!is_chrome_image(""));
    }

    #[test]
    fn firefox_family_images_are_recognised() {
        assert!(is_firefox_image(
            "/Applications/Firefox.app/Contents/MacOS/firefox"
        ));
        assert!(is_firefox_image("/Applications/Zen.app/Contents/MacOS/zen"));
        assert!(is_firefox_image("zen-browser\n"));
        assert!(is_firefox_image("librewolf"));
        assert!(!is_firefox_image("/usr/bin/chromium"));
        assert!(!is_firefox_image(""));
    }

    #[test]
    fn reads_pid_from_netstat() {
        let table = "  TCP    127.0.0.1:19222        0.0.0.0:0              LISTENING       111\r\n  TCP    127.0.0.1:9222         0.0.0.0:0              LISTENING       5150\r\n  TCP    127.0.0.1:9222         127.0.0.1:50000        ESTABLISHED     5150\r\n";
        assert_eq!(parse_netstat_pid(table, 9222), Some(5150));
        assert_eq!(parse_netstat_pid(table, 9333), None);
    }

    #[test]
    fn reads_pid_from_lsof() {
        let table = "COMMAND     PID USER   FD   TYPE DEVICE SIZE/OFF NODE NAME\nGoogle\\x20Ch 4242 joe   45u  IPv4 0xabc      0t0  TCP 127.0.0.1:9222 (LISTEN)\n";
        assert_eq!(parse_lsof_pid(table), Some(4242));
        assert_eq!(parse_lsof_pid("COMMAND PID\n"), None);
    }
}
