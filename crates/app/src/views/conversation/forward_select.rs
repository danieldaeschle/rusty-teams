use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::h_flex;
use gpui_kit::*;

use super::ConversationView;
use crate::forward_selection::{CANCEL_LABEL, ForwardSelection, NEXT_LABEL, selected_count_label};
use crate::rows::MessageRow;
use crate::theme;
use crate::views::widgets::icon;
use app_ui_kit::layout::ROW_PADDING;

pub const FORWARD_FOOTER_TAG: &str = "forward-footer";
pub const FORWARD_FOOTER_BAR_TAG: &str = "forward-footer/bar";
pub const FORWARD_FOOTER_COUNT_TAG: &str = "forward-footer/bar/count";
pub const FORWARD_FOOTER_NEXT_TAG: &str = "forward-footer/bar/next";
const FOOTER_TOP: f32 = 8.;
const FOOTER_BOTTOM: f32 = 16.;
const BAR_PADDING: f32 = 8.;
const BAR_RADIUS: f32 = 12.;
const COUNT_INSET: f32 = 4.;
const ICON_SIZE: f32 = 16.;
const BUTTON_HEIGHT: f32 = 32.;
const BUTTON_RADIUS: f32 = 6.;
const DISABLED_OPACITY: f32 = 0.4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SelectionSnapshot {
    pub selection: ForwardSelection,
    pub image_count: usize,
}

impl ConversationView {
    pub(super) fn active_forward_selection(&self, cx: &App) -> Option<SelectionSnapshot> {
        let chat_id = self
            .chat_id()
            .filter(|_| !self.draft_active && !self.in_feed())?;
        let selection = self
            .app
            .read(cx)
            .forward_selection
            .clone()
            .filter(|selection| selection.conversation_id() == chat_id)?;
        let image_count = self
            .rows
            .borrow()
            .iter()
            .flat_map(|row| row.messages())
            .filter(|message| selection.contains(&message.key))
            .map(|message| message.images.len())
            .sum();
        Some(SelectionSnapshot {
            selection,
            image_count,
        })
    }

    pub(super) fn start_forward_selection(&mut self, message_id: &str, cx: &mut Context<Self>) {
        if let Some(chat_id) = self.chat_id() {
            self.app.update(cx, |state, cx| {
                state.start_forward_selection(&chat_id, message_id, cx)
            });
        }
    }

    pub(super) fn on_forward_selection_changed(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let row_count = self.rows.borrow().len();
        self.scroller.update(cx, |scroller, cx| {
            scroller.remeasure_items(0..row_count, cx)
        });
        if self.active_forward_selection(cx).is_some() {
            window.focus(&self.focus_handle, cx);
        } else if !self.draft_active && !self.in_feed() {
            self.composer
                .update(cx, |composer, cx| composer.focus(window, cx));
        }
        cx.notify();
    }

    pub(super) fn cancel_forward_selection(&mut self, cx: &mut Context<Self>) -> bool {
        if self.active_forward_selection(cx).is_none() {
            return false;
        }
        self.app
            .update(cx, |state, cx| state.cancel_forward_selection(cx));
        true
    }

    fn forward_next(&mut self, cx: &mut Context<Self>) {
        let Some(snapshot) = self.active_forward_selection(cx) else {
            return;
        };
        let ordered_ids = {
            let rows = self.rows.borrow();
            let messages: Vec<&MessageRow> = rows.iter().flat_map(|row| row.messages()).collect();
            snapshot.selection.ordered_ids(messages)
        };
        self.app
            .update(cx, |state, cx| state.forward_selected(&ordered_ids, cx));
    }

    pub(super) fn render_forward_footer(
        &self,
        snapshot: &SelectionSnapshot,
        cx: &mut Context<Self>,
    ) -> Div {
        let count = snapshot.selection.len();
        div()
            .debug_selector(|| FORWARD_FOOTER_TAG.into())
            .w_full()
            .flex_none()
            .px(px(ROW_PADDING))
            .pt(px(FOOTER_TOP))
            .pb(px(FOOTER_BOTTOM))
            .child(
                h_flex()
                    .debug_selector(|| FORWARD_FOOTER_BAR_TAG.into())
                    .w_full()
                    .p(px(BAR_PADDING))
                    .gap(px(BAR_PADDING))
                    .items_center()
                    .rounded(px(BAR_RADIUS))
                    .bg(theme::surface_raised())
                    .child(
                        h_flex()
                            .debug_selector(|| FORWARD_FOOTER_COUNT_TAG.into())
                            .flex_1()
                            .min_w_0()
                            .pl(px(COUNT_INSET))
                            .gap(px(BAR_PADDING))
                            .items_center()
                            .child(icon(IconName::Forward, ICON_SIZE, theme::text_soft()))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(13.))
                                    .text_color(theme::text())
                                    .child(selected_count_label(count)),
                            ),
                    )
                    .child(
                        Button::new("forward-footer-cancel")
                            .ghost()
                            .label(CANCEL_LABEL)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cancel_forward_selection(cx);
                            })),
                    )
                    .child(next_button(count > 0, cx)),
            )
    }
}

fn next_button(enabled: bool, cx: &mut Context<ConversationView>) -> Stateful<Div> {
    let button = h_flex()
        .id("forward-footer-next")
        .debug_selector(|| FORWARD_FOOTER_NEXT_TAG.into())
        .h(px(BUTTON_HEIGHT))
        .flex_none()
        .px(px(16.))
        .items_center()
        .rounded(px(BUTTON_RADIUS))
        .bg(theme::accent())
        .text_color(theme::on_accent())
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(NEXT_LABEL);
    if enabled {
        button
            .cursor_pointer()
            .hover(|button| button.bg(theme::accent_text()))
            .on_click(cx.listener(|this, _, _, cx| this.forward_next(cx)))
    } else {
        button.opacity(DISABLED_OPACITY)
    }
}
