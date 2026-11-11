use std::time::Duration;

use gpui_kit::*;

pub const OWN_APPEAR: Duration = Duration::from_millis(320);
pub const INCOMING_APPEAR: Duration = Duration::from_millis(240);

const OWN_RISE: f32 = 56.;
const OWN_START_OPACITY: f32 = 0.5;
const OWN_FADE_SHARE: f32 = 0.4;

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
            let fade = ease_out_cubic(progress / OWN_FADE_SHARE);
            AppearFrame {
                offset_x: 0.,
                offset_y: (1. - ease_out_quart(progress)) * OWN_RISE,
                opacity: OWN_START_OPACITY + (1. - OWN_START_OPACITY) * fade,
            }
        }
        AppearKind::Incoming => AppearFrame {
            offset_x: 0.,
            offset_y: 0.,
            opacity: ease_out_cubic(progress),
        },
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

fn ease_out_cubic(progress: f32) -> f32 {
    1. - (1. - progress.clamp(0., 1.)).powi(3)
}

fn ease_out_quart(progress: f32) -> f32 {
    1. - (1. - progress.clamp(0., 1.)).powi(4)
}

#[cfg(test)]
mod tests {
    use super::{AppearFrame, AppearKind, OWN_RISE, appear_frame};

    #[test]
    fn both_kinds_settle_on_the_resting_place() {
        for kind in [AppearKind::Own, AppearKind::Incoming] {
            assert_eq!(
                appear_frame(kind, 1.),
                AppearFrame {
                    offset_x: 0.,
                    offset_y: 0.,
                    opacity: 1.
                }
            );
        }
    }

    #[test]
    fn own_messages_glide_up_from_behind_the_composer_without_overshoot() {
        let start = appear_frame(AppearKind::Own, 0.);
        assert_eq!(start.offset_y, OWN_RISE);
        assert!((1..100).all(|step| {
            let frame = appear_frame(AppearKind::Own, step as f32 / 100.);
            frame.offset_y >= 0. && frame.offset_x == 0.
        }));
    }

    #[test]
    fn incoming_messages_only_fade_in() {
        assert_eq!(appear_frame(AppearKind::Incoming, 0.).opacity, 0.);
        assert!((0..=100).all(|step| {
            let frame = appear_frame(AppearKind::Incoming, step as f32 / 100.);
            frame.offset_x == 0. && frame.offset_y == 0.
        }));
    }
}
