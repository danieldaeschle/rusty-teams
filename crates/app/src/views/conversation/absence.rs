use gpui_kit::assets::IconName;
use gpui_kit::component::h_flex;
use gpui_kit::*;

use super::ConversationView;
use crate::theme;
use crate::views::widgets::icon;

pub const ABSENCE_NOTICE_TAG: &str = "conversation/absence-notice";
const NOTICE_MARGIN_X: f32 = 20.;
const NOTICE_MARGIN_BOTTOM: f32 = 8.;
const NOTICE_PADDING: f32 = 12.;
const NOTICE_HEIGHT: f32 = 36.;
const NOTICE_RADIUS: f32 = 10.;
const CLOSE_SIZE: f32 = 24.;
const NOTICE_TINT: f32 = 0.18;

impl ConversationView {
    pub(super) fn render_absence_notice(&self, cx: &App) -> Option<AnyElement> {
        if self.draft_active {
            return None;
        }
        let conversation_id = self.conversation_id()?;
        let notice = self.app.read(cx).absence_notice(&conversation_id)?;
        let app = self.app.clone();
        Some(
            h_flex()
                .flex_none()
                .mx(px(NOTICE_MARGIN_X))
                .mb(px(NOTICE_MARGIN_BOTTOM))
                .pl(px(NOTICE_PADDING))
                .pr(px(NOTICE_PADDING - (CLOSE_SIZE - 14.) / 2.))
                .h(px(NOTICE_HEIGHT))
                .gap(px(8.))
                .items_center()
                .rounded(px(NOTICE_RADIUS))
                .bg(theme::accent().opacity(NOTICE_TINT))
                .debug_selector(|| ABSENCE_NOTICE_TAG.into())
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(13.))
                        .text_color(theme::text())
                        .child(notice.text()),
                )
                .child(
                    div()
                        .id("absence-notice-close")
                        .size(px(CLOSE_SIZE))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(6.))
                        .cursor_pointer()
                        .hover(|button| button.bg(theme::row_hover()))
                        .child(icon(IconName::Close, 14., theme::text_muted()))
                        .on_click(move |_, _, cx| {
                            app.update(cx, |state, cx| state.dismiss_absence_notice(&conversation_id, cx));
                        }),
                )
                .into_any_element(),
        )
    }
}
