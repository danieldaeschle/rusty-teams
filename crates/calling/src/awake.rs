/// Keeps the computer and its display awake while it lives; a call holds one from dial to hang up.
pub struct StayAwake {
    #[cfg(windows)]
    request: Option<windows::Win32::Foundation::HANDLE>,
    #[cfg(not(windows))]
    inhibitor: Option<std::process::Child>,
}

const REASON: &str = "Teams call in progress";

#[cfg(windows)]
mod platform {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Power::{
        POWER_REQUEST_TYPE, PowerClearRequest, PowerCreateRequest, PowerRequestDisplayRequired,
        PowerRequestSystemRequired, PowerSetRequest,
    };
    use windows::Win32::System::Threading::{
        POWER_REQUEST_CONTEXT_SIMPLE_STRING, REASON_CONTEXT, REASON_CONTEXT_0,
    };
    use windows_core::PWSTR;

    pub const REQUESTS: [POWER_REQUEST_TYPE; 2] =
        [PowerRequestSystemRequired, PowerRequestDisplayRequired];

    pub fn acquire(reason: &str) -> Option<HANDLE> {
        let mut wide: Vec<u16> = reason.encode_utf16().chain(Some(0)).collect();
        let context = REASON_CONTEXT {
            Version: 0,
            Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING,
            Reason: REASON_CONTEXT_0 {
                SimpleReasonString: PWSTR(wide.as_mut_ptr()),
            },
        };
        let request = unsafe { PowerCreateRequest(&context) }.ok()?;
        for kind in REQUESTS {
            let _ = unsafe { PowerSetRequest(request, kind) };
        }
        Some(request)
    }

    pub fn release(request: HANDLE) {
        for kind in REQUESTS {
            let _ = unsafe { PowerClearRequest(request, kind) };
        }
        let _ = unsafe { CloseHandle(request) };
    }
}

impl StayAwake {
    #[cfg(windows)]
    pub fn new() -> Self {
        StayAwake {
            request: platform::acquire(REASON),
        }
    }

    #[cfg(not(windows))]
    pub fn new() -> Self {
        let inhibitor = std::process::Command::new("systemd-inhibit")
            .args([
                "--what=idle:sleep",
                "--who=Rusty Teams",
                &format!("--why={REASON}"),
                "--mode=block",
                "sleep",
                "infinity",
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok();
        StayAwake { inhibitor }
    }

    pub fn holding(&self) -> bool {
        #[cfg(windows)]
        return self.request.is_some();
        #[cfg(not(windows))]
        return self.inhibitor.is_some();
    }
}

impl Drop for StayAwake {
    fn drop(&mut self) {
        #[cfg(windows)]
        if let Some(request) = self.request.take() {
            platform::release(request);
        }
        #[cfg(not(windows))]
        if let Some(mut inhibitor) = self.inhibitor.take() {
            let _ = inhibitor.kill();
            let _ = inhibitor.wait();
        }
    }
}

// SAFETY: a power request handle is a process-wide kernel object, usable and closable from any thread.
#[cfg(windows)]
unsafe impl Send for StayAwake {}
#[cfg(windows)]
unsafe impl Sync for StayAwake {}
