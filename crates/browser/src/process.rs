use std::process::Command;

use crate::config::Platform;
use crate::error::BrowserError;

pub(crate) fn parse_ss_pid(output: &str) -> Option<u32> {
    let after = output.split("pid=").nth(1)?;
    after.chars().take_while(char::is_ascii_digit).collect::<String>().parse().ok()
}

pub(crate) fn parse_netstat_pid(output: &str, port: u16) -> Option<u32> {
    let suffix = format!(":{port}");
    output.lines().find_map(|line| {
        let columns: Vec<&str> = line.split_whitespace().collect();
        let listening = columns.iter().any(|column| column.eq_ignore_ascii_case("LISTENING"));
        let local = columns.get(1)?;
        if !listening || !local.ends_with(&suffix) {
            return None;
        }
        columns.last()?.parse().ok()
    })
}

fn run(program: &str, arguments: &[&str]) -> Result<String, BrowserError> {
    let output = Command::new(program).args(arguments).output()?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn listener_pid(platform: Platform, port: u16) -> Result<u32, BrowserError> {
    let pid = match platform {
        Platform::Linux => parse_ss_pid(&run("ss", &["-ltnpH", &format!("sport = :{port}")])?),
        Platform::Windows => parse_netstat_pid(&run("netstat", &["-ano", "-p", "TCP"])?, port),
        Platform::Wsl => parse_netstat_pid(&run("netstat.exe", &["-ano", "-p", "TCP"])?, port),
    };
    pid.ok_or_else(|| BrowserError::Cdp(format!("no process listens on port {port}")))
}

pub(crate) fn is_chrome_image(image_name: &str) -> bool {
    let lowered = image_name.trim().trim_matches('"').to_ascii_lowercase();
    lowered.starts_with("chrome")
}

fn process_image_name(platform: Platform, pid: &str) -> Result<String, BrowserError> {
    let filter = format!("PID eq {pid}");
    let csv_arguments = ["/FI", filter.as_str(), "/FO", "CSV", "/NH"];
    let output = match platform {
        Platform::Linux => std::fs::read_to_string(format!("/proc/{pid}/comm"))?,
        Platform::Windows => run("tasklist", &csv_arguments)?,
        Platform::Wsl => run("tasklist.exe", &csv_arguments)?,
    };
    Ok(output.split(',').next().unwrap_or_default().to_owned())
}

pub(crate) fn kill_listener(platform: Platform, port: u16) -> Result<(), BrowserError> {
    let pid = listener_pid(platform, port)?.to_string();
    let image_name = process_image_name(platform, &pid)?;
    if !is_chrome_image(&image_name) {
        return Err(BrowserError::Cdp(format!("port {port} is held by a non-Chrome process, not killing it")));
    }
    let (program, arguments): (&str, Vec<&str>) = match platform {
        Platform::Linux => ("kill", vec!["-KILL", &pid]),
        Platform::Windows => ("taskkill", vec!["/PID", &pid, "/T", "/F"]),
        Platform::Wsl => ("taskkill.exe", vec!["/PID", &pid, "/T", "/F"]),
    };
    let status = Command::new(program).args(arguments).output()?.status;
    if status.success() { Ok(()) } else { Err(BrowserError::Cdp(format!("{program} failed for pid {pid}"))) }
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
        assert!(!is_chrome_image("\"wslrelay.exe\""));
        assert!(!is_chrome_image(""));
    }

    #[test]
    fn reads_pid_from_netstat() {
        let table = "  TCP    127.0.0.1:19222        0.0.0.0:0              LISTENING       111\r\n  TCP    127.0.0.1:9222         0.0.0.0:0              LISTENING       5150\r\n  TCP    127.0.0.1:9222         127.0.0.1:50000        ESTABLISHED     5150\r\n";
        assert_eq!(parse_netstat_pid(table, 9222), Some(5150));
        assert_eq!(parse_netstat_pid(table, 9333), None);
    }
}
