use std::time::Duration;

use crate::notify::platform::{ScreenRect, WorkArea};

pub const BAR_WIDTH: f32 = 380.;
pub const BAR_HEIGHT: f32 = 40.;
pub const HANDLE_WIDTH: f32 = 120.;
pub const HANDLE_HEIGHT: f32 = 6.;
pub const BORDER_THICKNESS: f32 = 4.;
pub const MINI_WIDTH: f32 = 320.;
pub const MINI_HEIGHT: f32 = 180.;
pub const MINI_MARGIN: f32 = 16.;
pub const BAR_IDLE_BEFORE_COLLAPSE: Duration = Duration::from_secs(3);
pub const MINI_POSITION_KEY: &str = "call_mini_position";

fn scaled(logical: f32, scale: f32) -> i32 {
    (logical * scale).round() as i32
}

pub fn border_thickness(scale: f32) -> i32 {
    scaled(BORDER_THICKNESS, scale).max(1)
}

pub fn bar_rect(work: WorkArea, collapsed: bool) -> ScreenRect {
    let (width, height) = if collapsed {
        (HANDLE_WIDTH, HANDLE_HEIGHT)
    } else {
        (BAR_WIDTH, BAR_HEIGHT)
    };
    let width = scaled(width, work.scale);
    let left = work.left + ((work.right - work.left) - width) / 2;
    ScreenRect {
        left,
        top: work.top,
        right: left + width,
        bottom: work.top + scaled(height, work.scale),
    }
}

pub fn bar_collapsed(pinned: bool, idle: Duration) -> bool {
    !pinned && idle >= BAR_IDLE_BEFORE_COLLAPSE
}

pub fn border_rects(target: ScreenRect, thickness: i32) -> [ScreenRect; 4] {
    let thickness = thickness.clamp(1, (target.width().min(target.height()) / 2).max(1));
    let ScreenRect { left, top, right, bottom } = target;
    [
        ScreenRect { left, top, right, bottom: top + thickness },
        ScreenRect { left, top: bottom - thickness, right, bottom },
        ScreenRect { left, top: top + thickness, right: left + thickness, bottom: bottom - thickness },
        ScreenRect { left: right - thickness, top: top + thickness, right, bottom: bottom - thickness },
    ]
}

pub fn mini_rect(work: WorkArea, saved: Option<(i32, i32)>) -> ScreenRect {
    let width = scaled(MINI_WIDTH, work.scale);
    let height = scaled(MINI_HEIGHT, work.scale);
    let margin = scaled(MINI_MARGIN, work.scale);
    let (left, top) = saved.unwrap_or((work.right - width - margin, work.bottom - height - margin));
    let left = left.clamp(work.left, (work.right - width).max(work.left));
    let top = top.clamp(work.top, (work.bottom - height).max(work.top));
    ScreenRect {
        left,
        top,
        right: left + width,
        bottom: top + height,
    }
}

pub fn mini_work_area(main: WorkArea, saved: Option<(i32, i32)>, area_at: impl Fn(i32, i32) -> Option<WorkArea>) -> WorkArea {
    saved.and_then(|(left, top)| area_at(left, top)).unwrap_or(main)
}

pub fn mini_wanted(call_active: bool, main_minimized: bool) -> bool {
    call_active && main_minimized
}

pub fn parse_position(text: &str) -> Option<(i32, i32)> {
    let (left, top) = text.split_once(',')?;
    Some((left.trim().parse().ok()?, top.trim().parse().ok()?))
}

pub fn format_position(left: i32, top: i32) -> String {
    format!("{left},{top}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work(scale: f32) -> WorkArea {
        WorkArea { left: 0, top: 0, right: 1920, bottom: 1040, scale }
    }

    #[test]
    fn bar_is_centred_at_the_top() {
        let rect = bar_rect(work(1.), false);
        assert_eq!((rect.left, rect.top, rect.right, rect.bottom), (770, 0, 1150, 40));
    }

    #[test]
    fn bar_follows_a_secondary_monitor_and_scale() {
        let second = WorkArea { left: 1920, top: 100, right: 3840, bottom: 1140, scale: 1.5 };
        let rect = bar_rect(second, false);
        assert_eq!(rect.width(), 570);
        assert_eq!(rect.height(), 60);
        assert_eq!(rect.top, 100);
        assert_eq!(rect.left, 1920 + (1920 - 570) / 2);
    }

    #[test]
    fn collapsed_bar_is_a_thin_handle() {
        let rect = bar_rect(work(1.), true);
        assert_eq!((rect.width(), rect.height(), rect.top), (120, 6, 0));
    }

    #[test]
    fn bar_collapses_only_when_unpinned_and_idle() {
        assert!(!bar_collapsed(false, Duration::from_secs(2)));
        assert!(bar_collapsed(false, Duration::from_secs(3)));
        assert!(!bar_collapsed(true, Duration::from_secs(60)));
    }

    #[test]
    fn border_thickness_scales_but_stays_visible() {
        assert_eq!(border_thickness(1.), 4);
        assert_eq!(border_thickness(2.), 8);
        assert_eq!(border_thickness(0.01), 1);
    }

    #[test]
    fn border_rects_frame_the_target_without_overlap() {
        let target = ScreenRect { left: 100, top: 50, right: 500, bottom: 350 };
        let [top, bottom, left, right] = border_rects(target, 4);
        assert_eq!((top.left, top.top, top.right, top.bottom), (100, 50, 500, 54));
        assert_eq!((bottom.top, bottom.bottom), (346, 350));
        assert_eq!((left.left, left.right, left.top, left.bottom), (100, 104, 54, 346));
        assert_eq!((right.left, right.right), (496, 500));
    }

    #[test]
    fn border_thickness_never_exceeds_half_the_target() {
        let target = ScreenRect { left: 0, top: 0, right: 6, bottom: 100 };
        let [top, _, left, right] = border_rects(target, 4);
        assert_eq!(top.bottom, 3);
        assert_eq!(left.right, 3);
        assert_eq!(right.left, 3);
    }

    #[test]
    fn mini_defaults_to_the_bottom_right_corner() {
        let rect = mini_rect(work(1.), None);
        assert_eq!((rect.left, rect.top, rect.right, rect.bottom), (1584, 844, 1904, 1024));
    }

    #[test]
    fn mini_keeps_a_saved_position_inside_the_work_area() {
        assert_eq!(mini_rect(work(1.), Some((300, 200))).left, 300);
        let pushed = mini_rect(work(1.), Some((5000, -40)));
        assert_eq!((pushed.right, pushed.top), (1920, 0));
        let pulled = mini_rect(work(1.), Some((-500, 5000)));
        assert_eq!((pulled.left, pulled.bottom), (0, 1040));
    }

    #[test]
    fn mini_stays_on_the_monitor_that_holds_the_saved_point() {
        let second = WorkArea { left: 1920, top: 0, right: 3840, bottom: 1040, scale: 1. };
        let area_at = |left: i32, _: i32| (left >= 1920).then_some(second);
        let saved = Some((3700, 900));
        let rect = mini_rect(mini_work_area(work(1.), saved, area_at), saved);
        assert_eq!((rect.right, rect.bottom), (3840, 1040));
        assert_eq!(mini_work_area(work(1.), Some((100, 100)), area_at), work(1.));
        assert_eq!(mini_work_area(work(1.), None, area_at), work(1.));
    }

    #[test]
    fn mini_scales_with_the_monitor() {
        let rect = mini_rect(work(2.), None);
        assert_eq!((rect.width(), rect.height()), (640, 360));
    }

    #[test]
    fn mini_shows_only_in_a_call_with_the_main_window_minimized() {
        assert!(mini_wanted(true, true));
        assert!(!mini_wanted(true, false));
        assert!(!mini_wanted(false, true));
    }

    #[test]
    fn position_round_trips() {
        assert_eq!(parse_position(&format_position(-1200, 80)), Some((-1200, 80)));
        assert_eq!(parse_position("garbage"), None);
    }
}
