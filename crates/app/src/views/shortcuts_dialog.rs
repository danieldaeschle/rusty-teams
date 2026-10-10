use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::shortcuts::{Group, shortcuts_in};
use super::widgets::symbol;
use crate::theme;

const CARD_WIDTH: f32 = 520.;
const CARD_RADIUS: f32 = 12.;
const CARD_PADDING: f32 = 16.;
const CARD_MARGIN: f32 = 32.;
const SECTION_GAP: f32 = 12.;
const ROW_GAP: f32 = 6.;
const BACKDROP_OPACITY: f32 = 0.55;
const CLOSE_SIZE: f32 = 28.;

pub enum ShortcutsDialogEvent {
    Close,
}

pub struct ShortcutsDialog {
    focus_handle: FocusHandle,
}

impl EventEmitter<ShortcutsDialogEvent> for ShortcutsDialog {}

impl ShortcutsDialog {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        ShortcutsDialog { focus_handle }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key != "escape" {
            return;
        }
        cx.emit(ShortcutsDialogEvent::Close);
        cx.stop_propagation();
    }

    fn header(&self, cx: &mut Context<Self>) -> Div {
        h_flex()
            .w_full()
            .items_center()
            .gap(px(SECTION_GAP))
            .child(
                div()
                    .flex_1()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Keyboard shortcuts"),
            )
            .child(
                div()
                    .id("shortcuts-close")
                    .size(px(CLOSE_SIZE))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .hover(|button| button.bg(theme::row_hover()))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ShortcutsDialogEvent::Close)))
                    .child(symbol("close", 16., theme::text_muted())),
            )
    }
}

fn key_chip(keys: String) -> Div {
    div()
        .flex_none()
        .px(px(8.))
        .py(px(2.))
        .rounded(px(6.))
        .bg(theme::surface())
        .border_1()
        .border_color(theme::border())
        .text_size(px(12.))
        .text_color(theme::text_muted())
        .child(keys)
}

fn group_block(group: Group) -> Div {
    let rows = shortcuts_in(group).map(|shortcut| {
        h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .gap(px(SECTION_GAP))
            .text_size(px(13.))
            .child(div().flex_1().min_w_0().child(shortcut.label))
            .child(key_chip(shortcut.display_keys()))
    });
    v_flex()
        .w_full()
        .gap(px(ROW_GAP))
        .child(
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text_muted())
                .child(group.title()),
        )
        .children(rows)
}

impl Render for ShortcutsDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("shortcuts-layer")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("shortcuts-backdrop")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(black().opacity(BACKDROP_OPACITY))
                    .occlude()
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ShortcutsDialogEvent::Close))),
            )
            .child(
                v_flex()
                    .id("shortcuts-card")
                    .track_focus(&self.focus_handle)
                    .w(px(CARD_WIDTH))
                    .max_w(relative(1.))
                    .max_h(relative(1.))
                    .m(px(CARD_MARGIN))
                    .p(px(CARD_PADDING))
                    .gap(px(SECTION_GAP))
                    .rounded(px(CARD_RADIUS))
                    .bg(theme::background())
                    .border_1()
                    .border_color(theme::border_strong())
                    .text_color(theme::text())
                    .shadow_lg()
                    .occlude()
                    .capture_key_down(cx.listener(Self::on_key_down))
                    .child(self.header(cx))
                    .child(
                        v_flex()
                            .id("shortcuts-body")
                            .min_h_0()
                            .gap(px(SECTION_GAP))
                            .overflow_y_scroll()
                            .children(Group::ALL.into_iter().map(group_block)),
                    ),
            )
    }
}
