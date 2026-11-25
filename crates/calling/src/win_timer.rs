use std::time::{Duration, Instant};

/// High-resolution frame pacer.
///
/// On Windows, `std::thread::sleep` is quantized to the 15.6 ms system timer tick,
/// causing audio underruns at 10 ms and uneven 30 fps screen capture. A high-resolution
/// waitable timer (`CREATE_WAITABLE_TIMER_HIGH_RESOLUTION`, Windows 10 1803+) wakes
/// within ~0.5 ms without changing the global multimedia timer period.
pub struct HighResSleep {
    #[cfg(windows)]
    handle: windows::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
unsafe impl Send for HighResSleep {}

impl HighResSleep {
    pub fn new() -> Self {
        #[cfg(windows)]
        {
            use windows::Win32::Foundation::HANDLE;
            use windows::Win32::System::Threading::{
                CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CreateWaitableTimerExW, TIMER_ALL_ACCESS,
            };
            let handle = unsafe {
                CreateWaitableTimerExW(
                    None,
                    None,
                    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                    TIMER_ALL_ACCESS.0,
                )
                .or_else(|_| CreateWaitableTimerExW(None, None, 0, TIMER_ALL_ACCESS.0))
                .unwrap_or_else(|_| HANDLE::default())
            };
            HighResSleep { handle }
        }
        #[cfg(not(windows))]
        {
            HighResSleep {}
        }
    }

    pub fn sleep(&self, duration: Duration) {
        if duration.is_zero() {
            return;
        }
        #[cfg(windows)]
        {
            use windows::Win32::System::Threading::{
                INFINITE, SetWaitableTimer, WaitForSingleObject,
            };
            if !self.handle.is_invalid() {
                // Negative relative due time in 100-nanosecond intervals.
                let ticks_100ns = (duration.as_nanos() / 100).max(1) as i64;
                let due_time = -ticks_100ns;
                let ok = unsafe { SetWaitableTimer(self.handle, &due_time, 0, None, None, false) };
                if ok.is_ok() {
                    unsafe {
                        let _ = WaitForSingleObject(self.handle, INFINITE);
                    }
                    return;
                }
            }
        }
        std::thread::sleep(duration);
    }

    pub fn sleep_until(&self, deadline: Instant) {
        let now = Instant::now();
        if deadline > now {
            self.sleep(deadline - now);
        }
    }
}

impl Default for HighResSleep {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(windows)]
impl Drop for HighResSleep {
    fn drop(&mut self) {
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        if !self.handle.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
            self.handle = HANDLE::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_res_sleep_waits_for_a_short_interval() {
        let sleeper = HighResSleep::new();
        let started = Instant::now();
        sleeper.sleep(Duration::from_millis(5));
        assert!(started.elapsed() >= Duration::from_millis(3));
    }
}
