use std::rc::Rc;

use gpui_kit::base::POPUP_PRIORITY;
use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::widgets::{Corner, corner_badge};
use crate::emoticons::{SKIN_TONE_COUNT, with_tone};
use crate::theme;

const CHOICE_SIZE: f32 = 32.;
const CHOICE_GLYPH_SIZE: f32 = 20.;
const PANEL_PADDING: f32 = 4.;
const PANEL_GAP: f32 = 4.;
const BADGE_SIZE: f32 = 4.;
const BADGE_INSET: f32 = 3.;
const MENU_GAP: f32 = 12.;

pub type TonePick = Rc<dyn Fn(u8, &mut Window, &mut App)>;
pub type ToneDismiss = Rc<dyn Fn(&mut Window, &mut App)>;

pub fn tone_badge() -> Div {
    corner_badge(Corner::BottomRight, -BADGE_INSET).child(
        div()
            .size(px(BADGE_SIZE))
            .rounded_full()
            .bg(theme::text_muted()),
    )
}

pub fn tone_choices(
    id: impl Into<SharedString>,
    position: Point<Pixels>,
    glyph: &str,
    current: u8,
    pick: TonePick,
    dismiss: ToneDismiss,
) -> AnyElement {
    let id: SharedString = id.into();
    let choices: Vec<_> = (0..=SKIN_TONE_COUNT)
        .map(|tone| {
            let pick = pick.clone();
            div()
                .id(ElementId::Name(format!("{id}-{tone}").into()))
                .size(px(CHOICE_SIZE))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .text_size(px(CHOICE_GLYPH_SIZE))
                .cursor_pointer()
                .when(tone == current, |choice| choice.bg(theme::popup_hover()))
                .hover(|choice| choice.bg(theme::popup_hover()))
                .child(with_tone(glyph, tone))
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    pick(tone, window, cx);
                })
        })
        .collect();
    deferred(
        anchored()
            .position(position - point(px(0.), px(MENU_GAP)))
            .anchor(Anchor::BottomLeft)
            .snap_to_window_with_margin(px(8.))
            .child(
                h_flex()
                    .id(ElementId::Name(id.clone()))
                    .gap(px(PANEL_GAP))
                    .p(px(PANEL_PADDING))
                    .rounded(px(8.))
                    .bg(theme::popup())
                    .border_1()
                    .border_color(theme::border_strong())
                    .shadow_md()
                    .occlude()
                    .on_mouse_down_out(move |_, window, cx| dismiss(window, cx))
                    .children(choices),
            ),
    )
    .with_priority(POPUP_PRIORITY + 1)
    .into_any_element()
}
