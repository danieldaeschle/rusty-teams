use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Sizable as _, Size, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{COMPOSER_BUTTON_SIZE, Composer, ComposerEvent};
use crate::theme;
use crate::views::avatar::person_avatar;
use crate::views::widgets::symbol;

pub const POST_CARD_HEADER_TAG: &str = "composer/post/header";
pub const POST_CARD_SUBJECT_TAG: &str = "composer/post/subject";
pub const POST_CARD_DIVIDER_TAG: &str = "composer/post/divider";
pub const POST_CARD_FOOTER_TAG: &str = "composer/post/footer";
pub const POST_CARD_SEND_TAG: &str = "composer/post/send";
const POST_LABEL: &str = "Post";
const CLOSE_TOOLTIP: &str = "Close";
const AUTHOR_AVATAR: f32 = 24.;
const CARD_INSET: f32 = 12.;
const SUBJECT_SIZE: f32 = 18.;
const INPUT_TEXT_SCALE: f32 = 0.875;
const SUBJECT_INSET: f32 = 8.;
const TEXT_INSET: f32 = 16.;

impl Composer {
    /// Shows the channel post layout with `subject` above the body, or the chat layout when `None`.
    pub fn set_post_layout(&mut self, subject: Option<Entity<InputState>>, cx: &mut Context<Self>) {
        let changed = self.post_subject.as_ref().map(Entity::entity_id)
            != subject.as_ref().map(Entity::entity_id);
        self.post_subject = subject;
        if changed {
            cx.notify();
        }
    }

    pub(super) fn post_layout(&self) -> bool {
        self.post_subject.is_some() && self.editing.is_none()
    }

    pub(super) fn render_post_header(&self, cx: &mut Context<Self>) -> Option<Div> {
        let subject = self.post_subject.clone().filter(|_| self.editing.is_none())?;
        let directory = &self.app.read(cx).directory;
        let author = directory.me.as_ref().map(|me| {
            h_flex()
                .gap(px(8.))
                .items_center()
                .child(person_avatar(
                    directory,
                    Some(&me.user_id),
                    &me.display_name,
                    AUTHOR_AVATAR,
                ))
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::text_strong())
                        .child(me.display_name.clone()),
                )
        });
        let close = div()
            .id("composer-post-close")
            .size(px(28.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|button| button.bg(theme::popup_hover()))
            .tooltip(|window, cx| Tooltip::new(CLOSE_TOOLTIP).build(window, cx))
            .child(symbol("close", 18., theme::text_muted()))
            .on_click(cx.listener(|_, _, _, cx| cx.emit(ComposerEvent::ClosePost)));
        Some(
            v_flex()
                .w_full()
                .child(
                    h_flex()
                        .debug_selector(|| POST_CARD_HEADER_TAG.into())
                        .w_full()
                        .pl(px(TEXT_INSET))
                        .pr(px(CARD_INSET))
                        .pt(px(CARD_INSET))
                        .items_center()
                        .children(author)
                        .child(div().flex_1())
                        .child(close),
                )
                .child(
                    div()
                        .debug_selector(|| POST_CARD_SUBJECT_TAG.into())
                        .w_full()
                        .px(px(SUBJECT_INSET))
                        .py(px(6.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(
                            Input::new(&subject)
                                .appearance(false)
                                .bordered(false)
                                .with_size(Size::Size(px(SUBJECT_SIZE / INPUT_TEXT_SCALE))),
                        ),
                )
                .child(
                    div()
                        .debug_selector(|| POST_CARD_DIVIDER_TAG.into())
                        .w_full()
                        .h(px(1.))
                        .bg(theme::border()),
                ),
        )
    }

    pub(super) fn render_post_footer(
        &self,
        actions: impl IntoElement,
        can_send: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        h_flex()
            .debug_selector(|| POST_CARD_FOOTER_TAG.into())
            .w_full()
            .px(px(CARD_INSET))
            .pb(px(CARD_INSET))
            .items_center()
            .child(actions)
            .child(div().flex_1())
            .child(
                div()
                    .id("composer-post-send")
                    .debug_selector(|| POST_CARD_SEND_TAG.into())
                    .h(px(COMPOSER_BUTTON_SIZE))
                    .px(px(14.))
                    .flex()
                    .items_center()
                    .rounded(px(6.))
                    .bg(theme::accent())
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::on_accent())
                    .child(POST_LABEL)
                    .when(!can_send, |button| button.opacity(0.4))
                    .when(can_send, |button| {
                        button.cursor_pointer().on_click(
                            cx.listener(|this, _, window, cx| this.submit_current(window, cx)),
                        )
                    }),
            )
    }
}
