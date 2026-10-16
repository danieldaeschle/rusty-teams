use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

static ACTIVATION: AtomicBool = AtomicBool::new(false);

const RETRY_INTERVAL: Duration = Duration::from_millis(200);
const RETRY_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, PartialEq, Eq)]
pub enum Claim {
    Primary,
    Forwarded,
}

pub fn take_activation() -> bool {
    ACTIVATION.swap(false, Ordering::AcqRel)
}

pub fn claim(name: &str, wait_for_previous: bool) -> Claim {
    let started = Instant::now();
    loop {
        if platform::try_become_primary(name) {
            return Claim::Primary;
        }
        if !wait_for_previous {
            platform::notify_primary(name);
            return Claim::Forwarded;
        }
        if started.elapsed() >= RETRY_TIMEOUT {
            return Claim::Primary;
        }
        std::thread::sleep(RETRY_INTERVAL);
    }
}

fn user_name() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default()
}

#[cfg(windows)]
mod platform {
    use std::fs::OpenOptions;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    use windows::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_NO_DATA, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GetLastError,
        INVALID_HANDLE_VALUE,
    };
    use windows::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_INBOUND};
    use windows::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_REJECT_REMOTE_CLIENTS,
        PIPE_TYPE_BYTE, PIPE_WAIT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};
    use windows::core::PCWSTR;

    use super::{ACTIVATION, user_name};

    const CONNECT_ATTEMPTS: u32 = 20;
    const CONNECT_INTERVAL: Duration = Duration::from_millis(100);

    fn pipe_path(name: &str) -> String {
        format!(r"\\.\pipe\{name}-{}", user_name())
    }

    pub fn try_become_primary(name: &str) -> bool {
        let wide_path: Vec<u16> = pipe_path(name)
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let handle = unsafe {
            CreateNamedPipeW(
                PCWSTR(wide_path.as_ptr()),
                PIPE_ACCESS_INBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                0,
                0,
                0,
                None,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            let error = unsafe { GetLastError() };
            return error != ERROR_ACCESS_DENIED && error != ERROR_PIPE_BUSY;
        }
        let raw_handle = handle.0 as usize;
        std::thread::spawn(move || {
            let handle = windows::Win32::Foundation::HANDLE(raw_handle as *mut _);
            loop {
                let connected = unsafe { ConnectNamedPipe(handle, None) }.is_ok()
                    || matches!(
                        unsafe { GetLastError() },
                        ERROR_PIPE_CONNECTED | ERROR_NO_DATA
                    );
                if connected {
                    ACTIVATION.store(true, Ordering::Release);
                } else {
                    std::thread::sleep(CONNECT_INTERVAL);
                }
                let _ = unsafe { DisconnectNamedPipe(handle) };
            }
        });
        true
    }

    pub fn notify_primary(name: &str) {
        let path = pipe_path(name);
        let _ = unsafe { AllowSetForegroundWindow(ASFW_ANY) };
        for _ in 0..CONNECT_ATTEMPTS {
            if OpenOptions::new().write(true).open(&path).is_ok() {
                return;
            }
            std::thread::sleep(CONNECT_INTERVAL);
        }
    }
}

#[cfg(unix)]
mod platform {
    use std::io::ErrorKind;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::sync::atomic::Ordering;

    use super::{ACTIVATION, user_name};

    pub(super) fn socket_path(name: &str) -> PathBuf {
        let directory = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        directory.join(format!("{name}-{}.sock", user_name()))
    }

    enum Bound {
        Listening(UnixListener),
        Taken,
        Unavailable,
    }

    fn bind(path: &PathBuf) -> Bound {
        match UnixListener::bind(path) {
            Ok(listener) => Bound::Listening(listener),
            Err(error) if error.kind() == ErrorKind::AddrInUse => {
                if UnixStream::connect(path).is_ok() {
                    return Bound::Taken;
                }
                let _ = std::fs::remove_file(path);
                UnixListener::bind(path).map_or(Bound::Unavailable, Bound::Listening)
            }
            Err(_) => Bound::Unavailable,
        }
    }

    pub fn try_become_primary(name: &str) -> bool {
        let listener = match bind(&socket_path(name)) {
            Bound::Listening(listener) => listener,
            Bound::Taken => return false,
            Bound::Unavailable => return true,
        };
        std::thread::spawn(move || {
            for connection in listener.incoming() {
                if connection.is_ok() {
                    ACTIVATION.store(true, Ordering::Release);
                }
            }
        });
        true
    }

    pub fn notify_primary(name: &str) {
        let _ = UnixStream::connect(socket_path(name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_claim_forwards_and_stale_socket_is_reclaimed() {
        let name = format!("rusty-teams-test-{}", std::process::id());
        assert_eq!(claim(&name, false), Claim::Primary);
        assert_eq!(claim(&name, false), Claim::Forwarded);
        let started = Instant::now();
        while !take_activation() {
            assert!(started.elapsed() < Duration::from_secs(1));
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!take_activation());

        let stale_name = format!("{name}-stale");
        #[cfg(unix)]
        {
            let path = platform::socket_path(&stale_name);
            drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
            assert!(path.exists());
        }
        assert_eq!(claim(&stale_name, false), Claim::Primary);
        assert_eq!(
            claim("missing-directory/rusty-teams", false),
            Claim::Primary
        );

        #[cfg(unix)]
        for leftover in [&name, &stale_name] {
            let _ = std::fs::remove_file(platform::socket_path(leftover));
        }
    }
}
