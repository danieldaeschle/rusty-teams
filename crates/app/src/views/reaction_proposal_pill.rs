use std::rc::Rc;

use gpui_kit::component::h_flex;
use gpui_kit::*;

use crate::theme;

const PILL_HEIGHT: f32 = 26.;
const PILL_PADDING: f32 = 10.;
const PILL_GAP: f32 = 8.;
const PILL_TEXT_SIZE: f32 = 12.;
pub const REACTION_PILL: &str = "shell/islands/main/reaction-pill";

pub type PillAction = Rc<dyn Fn(&mut App)>;

#[derive(Clone)]
pub struct ReactionPill {
    pub agent: &'static str,
    pub emoji: String,
    pub accept: PillAction,
    pub discard: PillAction,
}

fn pill_link(id: String, label: &'static str, color: Hsla, action: PillAction) -> Stateful<Div> {
    div()
        .id(ElementId::Name(id.into()))
        .flex_none()
        .cursor_pointer()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(color)
        .hover(|link| link.underline())
        .child(label)
        .on_click(move |_, _, cx| action(cx))
}

pub fn reaction_pill(pill: ReactionPill, key: &str) -> Div {
    h_flex()
        .h(px(PILL_HEIGHT))
        .flex_none()
        .items_center()
        .gap(px(PILL_GAP))
        .px(px(PILL_PADDING))
        .rounded_full()
        .bg(theme::row_hover())
        .text_size(px(PILL_TEXT_SIZE))
        .debug_selector(|| REACTION_PILL.into())
        .child(
            div()
                .flex_none()
                .text_color(theme::text_muted())
                .child(format!("{}: {}", pill.agent, pill.emoji)),
        )
        .child(pill_link(
            format!("reaction-accept-{key}"),
            "Accept",
            theme::accent_text(),
            pill.accept,
        ))
        .child(pill_link(
            format!("reaction-discard-{key}"),
            "Discard",
            theme::text_muted(),
            pill.discard,
        ))
}
