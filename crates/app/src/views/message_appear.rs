use std::time::Duration;

use gpui_kit::*;

pub const OWN_APPEAR: Duration = Duration::from_millis(420);
pub const INCOMING_APPEAR: Duration = Duration::from_millis(340);

const OWN_RISE: f32 = 16.;
const OWN_DRIFT: f32 = 8.;
const OWN_FADE_SHARE: f32 = 0.35;
const INCOMING_SLIDE: f32 = 18.;
const INCOMING_RISE: f32 = 4.;
const INCOMING_FADE_SHARE: f32 = 0.6;
const SPRING_DECAY: f32 = 6.;
const SPRING_FREQUENCY: f32 = 8.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppearKind {
    Own,
    Incoming,
}

impl AppearKind {
    pub fn duration(self) -> Duration {
        match self {
            Self::Own => OWN_APPEAR,
            Self::Incoming => INCOMING_APPEAR,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AppearFrame {
    pub offset_x: f32,
    pub offset_y: f32,
    pub opacity: f32,
}

pub fn appear_frame(kind: AppearKind, progress: f32) -> AppearFrame {
    let progress = progress.clamp(0., 1.);
    match kind {
        AppearKind::Own => {
            let remaining = 1. - spring_settle(progress);
            AppearFrame {
                offset_x: remaining * OWN_DRIFT,
                offset_y: remaining * OWN_RISE,
                opacity: ease_out_cubic(progress / OWN_FADE_SHARE),
            }
        }
        AppearKind::Incoming => {
            let remaining = 1. - ease_out_quint(progress);
            AppearFrame {
                offset_x: -remaining * INCOMING_SLIDE,
                offset_y: remaining * INCOMING_RISE,
                opacity: ease_out_cubic(progress / INCOMING_FADE_SHARE),
            }
        }
    }
}

pub fn appear(body: Div, message_key: &str, kind: AppearKind) -> AnimationElement<Div> {
    body.relative().with_animation(
        SharedString::from(format!("message-appear/{message_key}")),
        Animation::new(kind.duration()),
        move |body, progress| {
            let frame = appear_frame(kind, progress);
            body.left(px(frame.offset_x))
                .top(px(frame.offset_y))
                .opacity(frame.opacity)
        },
    )
}

fn spring_settle(progress: f32) -> f32 {
    if progress >= 1. {
        return 1.;
    }
    let angle = SPRING_FREQUENCY * progress;
    let ratio = SPRING_DECAY / SPRING_FREQUENCY;
    1. - (-SPRING_DECAY * progress).exp() * (angle.cos() + ratio * angle.sin())
}

fn ease_out_cubic(progress: f32) -> f32 {
    1. - (1. - progress.clamp(0., 1.)).powi(3)
}

fn ease_out_quint(progress: f32) -> f32 {
    1. - (1. - progress.clamp(0., 1.)).powi(5)
}

#[cfg(test)]
mod tests {
    use super::{AppearFrame, AppearKind, OWN_RISE, appear_frame};

    #[test]
    fn both_kinds_start_hidden_and_settle_on_the_resting_place() {
        for kind in [AppearKind::Own, AppearKind::Incoming] {
            let start = appear_frame(kind, 0.);
            assert_eq!(start.opacity, 0.);
            let end = appear_frame(kind, 1.);
            assert_eq!(
                end,
                AppearFrame {
                    offset_x: 0.,
                    offset_y: 0.,
                    opacity: 1.
                }
            );
        }
    }

    #[test]
    fn own_messages_rise_from_the_bottom_right_and_overshoot_slightly() {
        let start = appear_frame(AppearKind::Own, 0.);
        assert!(start.offset_x > 0. && start.offset_y > 0.);
        let deepest = (1..100)
            .map(|step| appear_frame(AppearKind::Own, step as f32 / 100.).offset_y)
            .fold(f32::MAX, f32::min);
        assert!(deepest < 0. && deepest > -OWN_RISE * 0.15);
    }

    #[test]
    fn incoming_messages_slide_in_from_the_left_without_overshoot() {
        let start = appear_frame(AppearKind::Incoming, 0.);
        assert!(start.offset_x < 0.);
        assert!((1..100).all(|step| appear_frame(AppearKind::Incoming, step as f32 / 100.).offset_x <= 0.));
    }
}
