use super::platform::WorkArea;
use super::rules::{Corner, Preview};
use super::stack::{ReplyState, ToastModel};

pub const TOAST_WIDTH: f32 = 360.;
pub const PILL_WIDTH: f32 = 132.;
pub const PILL_HEIGHT: f32 = 32.;
pub const SIDE_MARGIN: f32 = 16.;
pub const EDGE_MARGIN: f32 = 12.;
pub const STACK_GAP: f32 = 8.;
pub const ACTION_BUTTON: f32 = 28.;
pub const SEND_BUTTON: f32 = 32.;
const BASE_HEIGHT: f32 = 62.;
const PREVIEW_LINE: f32 = 18.;
const CHARS_PER_LINE: usize = 40;
const ACTION_ROW_SPACING: f32 = 4.;
const REPLY_FIELD_FRAME: f32 = 14.;
const REPLY_HINT_ROW: f32 = 20.;
const REPLY_SPACING: f32 = 8.;
const CONFIRM_HEIGHT: f32 = 56.;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slot {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

fn preview_lines(model: &ToastModel, preview_on: bool) -> usize {
    match (&model.preview, preview_on) {
        (Preview::Text(text), true) => text.chars().count().div_ceil(CHARS_PER_LINE).clamp(1, 2),
        _ => 1,
    }
}

pub fn toast_height(model: &ToastModel, preview_on: bool) -> f32 {
    let base = BASE_HEIGHT + preview_lines(model, preview_on) as f32 * PREVIEW_LINE;
    match model.reply {
        ReplyState::Sent | ReplyState::Failed => CONFIRM_HEIGHT,
        ReplyState::Open => {
            base + REPLY_SPACING + SEND_BUTTON + REPLY_FIELD_FRAME + REPLY_HINT_ROW
        }
        ReplyState::Closed => base + ACTION_ROW_SPACING + ACTION_BUTTON,
    }
}

pub struct StackSlots {
    pub toasts: Vec<Slot>,
    pub pill: Option<Slot>,
}

pub fn stack_slots(
    area: WorkArea,
    corner: Corner,
    heights: &[f32],
    with_pill: bool,
) -> StackSlots {
    let scale = area.scale;
    let physical = |value: f32| (value * scale).round() as i32;
    let width = physical(TOAST_WIDTH);
    let x = if corner.is_right() {
        area.right - physical(SIDE_MARGIN) - width
    } else {
        area.left + physical(SIDE_MARGIN)
    };
    let gap = physical(STACK_GAP);
    let mut rows: Vec<(i32, i32)> = Vec::new();
    if with_pill {
        rows.push((physical(PILL_WIDTH), physical(PILL_HEIGHT)));
    }
    rows.extend(heights.iter().map(|height| (width, physical(*height))));
    let total: i32 = rows.iter().map(|(_, height)| height).sum::<i32>()
        + gap * rows.len().saturating_sub(1) as i32;
    let mut y = if corner.is_bottom() {
        area.bottom - physical(EDGE_MARGIN) - total
    } else {
        area.top + physical(EDGE_MARGIN)
    };
    let mut slots = Vec::new();
    for (row_width, height) in rows {
        let row_x = if corner.is_right() {
            x + width - row_width
        } else {
            x
        };
        slots.push(Slot {
            x: row_x,
            y,
            width: row_width,
            height,
        });
        y += height + gap;
    }
    let pill = if with_pill && !slots.is_empty() {
        Some(slots.remove(0))
    } else {
        None
    };
    StackSlots {
        toasts: slots,
        pill,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(scale: f32) -> WorkArea {
        WorkArea {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1032,
            scale,
        }
    }

    #[test]
    fn bottom_right_stack_hugs_corner_newest_at_bottom() {
        let slots = stack_slots(area(1.), Corner::BottomRight, &[80., 98.], false);
        assert_eq!(slots.toasts.len(), 2);
        let newest = slots.toasts[1];
        assert_eq!(newest.x, 1920 - 16 - 360);
        assert_eq!(newest.y + newest.height, 1032 - 12);
        let older = slots.toasts[0];
        assert_eq!(older.y + older.height + 8, newest.y);
    }

    #[test]
    fn pill_sits_above_oldest_toast() {
        let slots = stack_slots(area(1.), Corner::BottomRight, &[80., 80., 80.], true);
        let pill = slots.pill.unwrap();
        assert_eq!(pill.height, 32);
        assert_eq!(pill.y + pill.height + 8, slots.toasts[0].y);
        assert_eq!(pill.x + pill.width, slots.toasts[0].x + slots.toasts[0].width);
    }

    #[test]
    fn top_left_anchors_to_top_edge() {
        let slots = stack_slots(area(1.), Corner::TopLeft, &[80.], false);
        assert_eq!((slots.toasts[0].x, slots.toasts[0].y), (16, 12));
    }

    #[test]
    fn scale_applies_to_all_distances() {
        let slots = stack_slots(area(1.5), Corner::BottomRight, &[80.], false);
        let toast = slots.toasts[0];
        assert_eq!(toast.width, 540);
        assert_eq!(toast.height, 120);
        assert_eq!(toast.y + toast.height, 1032 - 18);
    }

    #[test]
    fn heights_stay_in_board_range() {
        use super::super::rules::ChatKind;
        use super::super::stack::{ReplyState, ToastTimer};
        let model = |text: &str| ToastModel {
            id: 1,
            conversation_id: "a".into(),
            message_id: "m".into(),
            kind: ChatKind::Direct,
            chat_title: "T".into(),
            sender_id: None,
            sender_name: "S".into(),
            preview: Preview::Text(text.into()),
            mentions_me: false,
            count: 1,
            timer: ToastTimer::new(std::time::Instant::now(), false),
            reply: ReplyState::Closed,
            reply_text: String::new(),
            hovered: false,
            time: String::new(),
        };
        let short = toast_height(&model("Hi"), true);
        let long = toast_height(&model(&"x".repeat(200)), true);
        assert_eq!(short, 112.);
        assert_eq!(long, 130.);
        assert_eq!(toast_height(&model(&"x".repeat(200)), false), short);
    }
}
