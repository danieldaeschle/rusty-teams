use super::week_grid::CalendarSpan;

/// Fraction of the hour-grid viewport used to keep «now» below the top edge.
pub const NOW_BIAS_FRACTION: f32 = 1. / 3.;

/// Fallback grid height before layout reports a real viewport.
pub const DEFAULT_GRID_VIEWPORT: f32 = 480.;

/// Pixels of scroll-offset slack before a change counts as manual scroll.
pub const MANUAL_SCROLL_EPSILON: f32 = 2.;

/// Vertical scroll offset (negative Y) so local wall time sits near `bias_fraction`
/// down the visible hour grid. Clamped to the day extent.
pub fn now_scroll_offset_y(
    hour: u32,
    minute: u32,
    hour_height: f32,
    viewport_height: f32,
    bias_fraction: f32,
) -> f32 {
    let hour = hour.min(23);
    let minute = minute.min(59);
    let day_height = 24. * hour_height;
    let now_y = (hour as f32 + minute as f32 / 60.) * hour_height;
    let viewport = viewport_height.max(0.);
    let bias = (viewport * bias_fraction.clamp(0., 1.)).clamp(0., viewport);
    let min_offset = -(day_height - viewport).max(0.);
    (bias - now_y).clamp(min_offset, 0.)
}

pub fn shows_hour_grid(span: CalendarSpan) -> bool {
    !matches!(span, CalendarSpan::Month)
}

/// Whether auto-follow should keep aligning while today is on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FollowNow {
    pub active: bool,
}

impl FollowNow {
    pub fn request_align(&mut self) {
        self.active = true;
    }

    pub fn note_manual_scroll(&mut self) {
        self.active = false;
    }

    pub fn should_follow(self, today_visible: bool) -> bool {
        self.active && today_visible
    }
}

pub fn offset_differs(actual: f32, expected: f32, epsilon: f32) -> bool {
    (actual - expected).abs() > epsilon
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noon_centers_near_bias_without_clamp() {
        let hour_height = 120.;
        let viewport = 480.;
        let offset = now_scroll_offset_y(12, 0, hour_height, viewport, NOW_BIAS_FRACTION);
        let now_y = 12. * hour_height;
        let bias = viewport * NOW_BIAS_FRACTION;
        assert!((offset - (bias - now_y)).abs() < 0.01);
    }

    #[test]
    fn midnight_stays_at_top() {
        let offset = now_scroll_offset_y(0, 0, 120., 480., NOW_BIAS_FRACTION);
        assert_eq!(offset, 0.);
    }

    #[test]
    fn late_evening_clamps_to_day_end() {
        let hour_height = 120.;
        let viewport = 480.;
        let day_height = 24. * hour_height;
        let min_offset = -(day_height - viewport);
        let offset = now_scroll_offset_y(23, 50, hour_height, viewport, NOW_BIAS_FRACTION);
        assert!((offset - min_offset).abs() < 0.01);
    }

    #[test]
    fn month_has_no_hour_grid() {
        assert!(!shows_hour_grid(CalendarSpan::Month));
        assert!(shows_hour_grid(CalendarSpan::Day));
        assert!(shows_hour_grid(CalendarSpan::WorkWeek));
    }

    #[test]
    fn follow_turns_off_on_manual_scroll_and_back_on_align() {
        let mut follow = FollowNow { active: true };
        assert!(follow.should_follow(true));
        follow.note_manual_scroll();
        assert!(!follow.should_follow(true));
        follow.request_align();
        assert!(follow.should_follow(true));
        assert!(!follow.should_follow(false));
    }

    #[test]
    fn offset_diff_uses_epsilon() {
        assert!(!offset_differs(-100., -100.5, MANUAL_SCROLL_EPSILON));
        assert!(offset_differs(-100., -110., MANUAL_SCROLL_EPSILON));
    }
}
