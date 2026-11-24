//! Process restart helpers. A newer release is announced elsewhere; this module
//! does not download or replace the running binary. `RELAUNCH_ENV` is how a
//! restart (install, org switch, sign-in browser) tells the new process to wait
//! out the previous one. `exit_now` skips a slow DLL unload on Windows.

use std::time::Duration;

pub const RELAUNCH_ENV: &str = "RUSTY_TEAMS_RELAUNCH";
const QUIT_GRACE: Duration = Duration::from_secs(3);
const PREVIOUS_EXIT_TIMEOUT: Duration = Duration::from_secs(30);

pub fn running_version() -> &'static str {
    crate::version::build_version()
}

pub fn previous_process_id(relaunch_value: &str) -> Option<u32> {
    relaunch_value.trim().parse().ok()
}

pub fn wait_for_previous_exit(relaunch_value: &str) {
    if let Some(process_id) = previous_process_id(relaunch_value) {
        platform::wait_for_exit(process_id, PREVIOUS_EXIT_TIMEOUT);
    }
}

pub fn exit_after_grace() {
    std::thread::spawn(|| {
        std::thread::sleep(QUIT_GRACE);
        exit_now();
    });
}

/// Skips DLL unload, which takes seconds with WebRTC and WebView2 loaded and keeps the database locked.
pub fn exit_now() -> ! {
    platform::terminate_current();
    std::process::exit(0)
}

#[cfg(windows)]
mod platform {
    use std::time::Duration;

    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, PROCESS_SYNCHRONIZE, TerminateProcess, WaitForSingleObject,
    };

    pub fn terminate_current() {
        let _ = unsafe { TerminateProcess(GetCurrentProcess(), 0) };
    }

    pub fn wait_for_exit(process_id: u32, timeout: Duration) {
        let Ok(handle) = (unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, process_id) }) else {
            return;
        };
        unsafe {
            WaitForSingleObject(handle, timeout.as_millis() as u32);
            let _ = CloseHandle(handle);
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use std::time::Duration;

    pub fn wait_for_exit(_process_id: u32, _timeout: Duration) {}

    pub fn terminate_current() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relaunch_value_carries_the_previous_process_id() {
        assert_eq!(previous_process_id("4242"), Some(4242));
        assert_eq!(previous_process_id(""), None);
    }

    #[test]
    fn running_version_is_set() {
        assert!(!running_version().is_empty());
    }
}
