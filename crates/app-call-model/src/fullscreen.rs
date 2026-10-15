use std::time::{Duration, Instant};

pub const CHROME_IDLE: Duration = Duration::from_secs(3);
pub const HINT_SHOWN: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy)]
pub struct FullscreenState {
    pub entered: Instant,
    pub active_at: Instant,
    pub hovered: bool,
    pub timer_running: bool,
}

impl FullscreenState {
    pub fn new(now: Instant) -> Self {
        FullscreenState {
            entered: now,
            active_at: now,
            hovered: false,
            timer_running: false,
        }
    }

    pub fn chrome_visible(&self, now: Instant) -> bool {
        self.hovered || now.saturating_duration_since(self.active_at) < CHROME_IDLE
    }

    pub fn hint_visible(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.entered) < HINT_SHOWN
    }

    pub fn next_change(&self, now: Instant) -> Option<Duration> {
        if self.hovered {
            return None;
        }
        let hide_at = self.active_at + CHROME_IDLE;
        (hide_at > now).then(|| hide_at - now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_hide_after_three_idle_seconds_and_return_on_movement() {
        let start = Instant::now();
        let mut state = FullscreenState::new(start);
        assert!(state.chrome_visible(start + Duration::from_millis(2900)));
        assert!(!state.chrome_visible(start + CHROME_IDLE));
        state.active_at = start + Duration::from_secs(10);
        assert!(state.chrome_visible(start + Duration::from_secs(11)));
    }

    #[test]
    fn controls_stay_while_the_pointer_is_over_them() {
        let start = Instant::now();
        let mut state = FullscreenState::new(start);
        state.hovered = true;
        assert!(state.chrome_visible(start + Duration::from_secs(60)));
        assert_eq!(state.next_change(start), None);
    }

    #[test]
    fn the_exit_hint_shows_for_two_seconds() {
        let start = Instant::now();
        let state = FullscreenState::new(start);
        assert!(state.hint_visible(start + Duration::from_millis(1900)));
        assert!(!state.hint_visible(start + HINT_SHOWN));
    }
}
