use std::time::{Duration, Instant};

/// High-resolution frame pacer.
///
/// On Windows, `std::thread::sleep` is quantized to the 15.6 ms system timer tick,
/// causing audio underruns at 10 ms and uneven 30 fps screen capture. A high-resolution
/// waitable timer (`CREATE_WAITABLE_TIMER_HIGH_RESOLUTION`, Windows 10 1803+) wakes
/// within ~0.5 ms without changing the global multimedia timer period.
pub struct HighResSleep {
    #[cfg(windows)]
    handle: *mut std::ffi::c_void,
}

#[cfg(windows)]
unsafe impl Send for HighResSleep {}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    pub const CREATE_WAITABLE_TIMER_HIGH_RESOLUTION: u32 = 0x0000_0002;
    pub const TIMER_ALL_ACCESS: u32 = 0x001F_0003;
    pub const INFINITE: u32 = 0xFFFF_FFFF;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn CreateWaitableTimerExW(
            lp_timer_attributes: *const c_void,
            lp_timer_name: *const u16,
            dw_flags: u32,
            dw_desired_access: u32,
        ) -> *mut c_void;
        pub fn SetWaitableTimer(
            h_timer: *mut c_void,
            lp_due_time: *const i64,
            l_period: i32,
            pfn_completion_routine: *const c_void,
            lp_arg_to_completion_routine: *const c_void,
            f_resume: i32,
        ) -> i32;
        pub fn WaitForSingleObject(h_handle: *mut c_void, dw_milliseconds: u32) -> u32;
        pub fn CloseHandle(h_object: *mut c_void) -> i32;
    }
}

impl HighResSleep {
    pub fn new() -> Self {
        #[cfg(windows)]
        {
            let mut handle = unsafe {
                win::CreateWaitableTimerExW(
                    std::ptr::null(),
                    std::ptr::null(),
                    win::CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                    win::TIMER_ALL_ACCESS,
                )
            };
            if handle.is_null() {
                handle = unsafe {
                    win::CreateWaitableTimerExW(
                        std::ptr::null(),
                        std::ptr::null(),
                        0,
                        win::TIMER_ALL_ACCESS,
                    )
                };
            }
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
            if !self.handle.is_null() {
                // Negative relative due time in 100-nanosecond intervals.
                let ticks_100ns = (duration.as_nanos() / 100).max(1) as i64;
                let due_time = -ticks_100ns;
                let ok = unsafe {
                    win::SetWaitableTimer(
                        self.handle,
                        &due_time,
                        0,
                        std::ptr::null(),
                        std::ptr::null(),
                        0,
                    )
                };
                if ok != 0 {
                    unsafe {
                        win::WaitForSingleObject(self.handle, win::INFINITE);
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
        if !self.handle.is_null() {
            unsafe {
                win::CloseHandle(self.handle);
            }
            self.handle = std::ptr::null_mut();
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
