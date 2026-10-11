use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::watch;

pub const IDLE_AFTER: Duration = Duration::from_secs(5 * 60);
const SAMPLE_INTERVAL: Duration = Duration::from_secs(15);

pub struct UserInput {
    origin: Instant,
    last_input_millis: AtomicU64,
}

impl UserInput {
    pub fn new() -> Arc<Self> {
        Arc::new(UserInput {
            origin: Instant::now(),
            last_input_millis: AtomicU64::new(0),
        })
    }

    pub fn touch(&self) {
        let elapsed = self.origin.elapsed().as_millis() as u64;
        self.last_input_millis.store(elapsed, Ordering::Relaxed);
    }

    fn last_input(&self) -> Instant {
        self.origin + Duration::from_millis(self.last_input_millis.load(Ordering::Relaxed))
    }
}

pub fn latest_input(app_input: Instant, system_idle: Option<Duration>, now: Instant) -> Instant {
    match system_idle.and_then(|idle| now.checked_sub(idle)) {
        Some(system_input) => app_input.max(system_input),
        None => app_input,
    }
}

pub fn is_active(last_input: Instant, now: Instant, idle_after: Duration) -> bool {
    now.saturating_duration_since(last_input) < idle_after
}

pub fn activity_change(previous: bool, last_input: Instant, now: Instant, idle_after: Duration) -> Option<bool> {
    let current = is_active(last_input, now, idle_after);
    (current != previous).then_some(current)
}

pub fn track(input: Arc<UserInput>) -> watch::Receiver<bool> {
    let (sender, receiver) = watch::channel(true);
    crate::runtime::handle().spawn(async move {
        let mut interval = tokio::time::interval(SAMPLE_INTERVAL);
        loop {
            interval.tick().await;
            let now = Instant::now();
            let last_input = latest_input(input.last_input(), system_idle(), now);
            let previous = *sender.borrow();
            if let Some(current) = activity_change(previous, last_input, now, IDLE_AFTER) {
                sender.send_replace(current);
            }
        }
    });
    receiver
}

#[cfg(windows)]
fn system_idle() -> Option<Duration> {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    unsafe { GetLastInputInfo(&mut info) }.as_bool().then(|| {
        let idle_millis = unsafe { GetTickCount() }.wrapping_sub(info.dwTime);
        Duration::from_millis(u64::from(idle_millis))
    })
}

#[cfg(not(windows))]
fn system_idle() -> Option<Duration> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seconds(count: u64) -> Duration {
        Duration::from_secs(count)
    }

    #[test]
    fn active_until_the_idle_threshold() {
        let last_input = Instant::now();
        assert!(is_active(last_input, last_input + seconds(299), IDLE_AFTER));
        assert!(!is_active(last_input, last_input + seconds(300), IDLE_AFTER));
        assert!(is_active(last_input + seconds(10), last_input, IDLE_AFTER));
    }

    #[test]
    fn only_a_change_of_state_is_reported() {
        let last_input = Instant::now();
        assert_eq!(activity_change(true, last_input, last_input + seconds(10), IDLE_AFTER), None);
        assert_eq!(activity_change(true, last_input, last_input + seconds(301), IDLE_AFTER), Some(false));
        assert_eq!(activity_change(false, last_input, last_input + seconds(301), IDLE_AFTER), None);
        assert_eq!(activity_change(false, last_input + seconds(400), last_input + seconds(401), IDLE_AFTER), Some(true));
    }

    #[test]
    fn system_input_counts_when_newer_than_app_input() {
        let now = Instant::now() + seconds(1000);
        let app_input = now - seconds(900);
        assert_eq!(latest_input(app_input, Some(seconds(20)), now), now - seconds(20));
        assert_eq!(latest_input(app_input, Some(seconds(2000)), now), app_input);
        assert_eq!(latest_input(app_input, None, now), app_input);
    }

    #[test]
    fn touch_moves_the_last_input_forward() {
        let input = UserInput::new();
        let before = input.last_input();
        std::thread::sleep(Duration::from_millis(5));
        input.touch();
        assert!(input.last_input() > before);
    }
}
