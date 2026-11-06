use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Sizable as _, Size, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use teams_core::{AnnouncementBanner, BannerBackground};

use super::{COMPOSER_BUTTON_SIZE, COMPOSER_ICON_SIZE, Composer, ComposerEvent};
use crate::theme;
use crate::views::announcement_banner::{
    BANNER_COLORS, banner_fill, banner_frame, color_label, headline_style,
};
use crate::views::avatar::person_avatar;
use crate::views::widgets::symbol;

pub const POST_CARD_HEADER_TAG: &str = "composer/post/header";
pub const POST_CARD_SUBJECT_TAG: &str = "composer/post/subject";
pub const POST_CARD_DIVIDER_TAG: &str = "composer/post/divider";
pub const POST_CARD_FOOTER_TAG: &str = "composer/post/footer";
pub const POST_CARD_SEND_TAG: &str = "composer/post/send";
pub const POST_ANNOUNCEMENT_TAG: &str = "composer/post/announcement";
pub const POST_ANNOUNCEMENT_TOGGLE_TAG: &str = "composer/post/announcement-toggle";
const ANNOUNCEMENT_TOOLTIP: &str = "Announcement";
const HEADLINE_PLACEHOLDER: &str = "Add a headline";
pub const SUBJECT_PLACEHOLDER: &str = "Add a subject";
const SUBHEADING_PLACEHOLDER: &str = "Add a subheading";
const HEADLINE_SIZE: f32 = 24.;
const SWATCH_SIZE: f32 = 18.;
const SWATCH_RING: f32 = 2.;
const SWATCH_GAP: f32 = 6.;
const SWATCH_INSET: f32 = 8.;
const POST_LABEL: &str = "Post";
const CLOSE_TOOLTIP: &str = "Close";
const AUTHOR_AVATAR: f32 = 24.;
const CARD_INSET: f32 = 12.;
const SUBJECT_SIZE: f32 = 18.;
const INPUT_TEXT_SCALE: f32 = 0.875;
const SUBJECT_INSET: f32 = 8.;
const TEXT_INSET: f32 = 16.;

pub(super) struct AnnouncementDraft {
    headline: Entity<InputState>,
    color: &'static str,
}

impl Composer {
    /// Shows the channel post layout with `subject` above the body, or the chat layout when `None`.
    pub fn set_post_layout(&mut self, subject: Option<Entity<InputState>>, cx: &mut Context<Self>) {
        let changed = self.post_subject.as_ref().map(Entity::entity_id)
            != subject.as_ref().map(Entity::entity_id);
        self.post_subject = subject;
        if self.post_subject.is_none() {
            self.announcement = None;
        }
        if changed {
            cx.notify();
        }
    }

    pub(super) fn post_layout(&self) -> bool {
        self.post_subject.is_some() && self.editing.is_none()
    }

    pub(super) fn announcement_banner(&self, cx: &App) -> Option<AnnouncementBanner> {
        let draft = self.announcement.as_ref().filter(|_| self.post_layout())?;
        let title = draft.headline.read(cx).value().trim().to_owned();
        (!title.is_empty()).then(|| AnnouncementBanner {
            title,
            background: BannerBackground::Color(draft.color.to_owned()),
        })
    }

    pub(super) fn restore_announcement(
        &mut self,
        banner: Option<&AnnouncementBanner>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match banner {
            Some(banner) => {
                let color = match &banner.background {
                    BannerBackground::Color(color) => BANNER_COLORS
                        .into_iter()
                        .find(|known| known == color)
                        .unwrap_or(BANNER_COLORS[0]),
                    BannerBackground::Image(_) => BANNER_COLORS[0],
                };
                self.open_announcement(color, window, cx);
                if let Some(draft) = &self.announcement {
                    let title = banner.title.clone();
                    draft
                        .headline
                        .update(cx, |input, cx| input.set_value(title, window, cx));
                }
            }
            None => self.close_announcement(window, cx),
        }
    }

    fn toggle_announcement(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.announcement.is_some() {
            self.close_announcement(window, cx);
            self.focus(window, cx);
        } else {
            self.open_announcement(BANNER_COLORS[0], window, cx);
            if let Some(draft) = &self.announcement {
                draft
                    .headline
                    .update(cx, |input, cx| input.focus(window, cx));
            }
        }
    }

    fn open_announcement(
        &mut self,
        color: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let headline = cx.new(|cx| InputState::new(window, cx).placeholder(HEADLINE_PLACEHOLDER));
        self.announcement = Some(AnnouncementDraft { headline, color });
        self.set_subject_placeholder(SUBHEADING_PLACEHOLDER, window, cx);
        cx.notify();
    }

    pub(super) fn close_announcement(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.announcement.take().is_some() {
            self.set_subject_placeholder(SUBJECT_PLACEHOLDER, window, cx);
            cx.notify();
        }
    }

    fn set_subject_placeholder(
        &self,
        placeholder: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(subject) = &self.post_subject {
            subject.update(cx, |input, cx| {
                input.set_placeholder(placeholder, window, cx)
            });
        }
    }

    pub(super) fn render_announcement_toggle(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        if !self.post_layout() {
            return None;
        }
        let active = self.announcement.is_some();
        Some(
            div()
                .id("composer-post-announcement")
                .debug_selector(|| POST_ANNOUNCEMENT_TOGGLE_TAG.into())
                .size(px(COMPOSER_BUTTON_SIZE))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .cursor_pointer()
                .when(active, |button| button.bg(theme::popup_hover()))
                .hover(|button| button.bg(theme::popup_hover()))
                .tooltip(|window, cx| Tooltip::new(ANNOUNCEMENT_TOOLTIP).build(window, cx))
                .child(symbol(
                    "campaign",
                    COMPOSER_ICON_SIZE,
                    if active {
                        theme::accent_soft()
                    } else {
                        theme::text_muted()
                    },
                ))
                .on_mouse_down(MouseButton::Left, |_, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                })
                .on_click(cx.listener(|this, _, window, cx| this.toggle_announcement(window, cx))),
        )
    }

    fn render_announcement(&self, cx: &mut Context<Self>) -> Option<Div> {
        let draft = self.announcement.as_ref()?;
        let directory = &self.app.read(cx).directory;
        let swatches = h_flex()
            .absolute()
            .right(px(SWATCH_INSET))
            .bottom(px(SWATCH_INSET))
            .gap(px(SWATCH_GAP))
            .children(BANNER_COLORS.into_iter().map(|color| {
                let selected = color == draft.color;
                div()
                    .id(ElementId::Name(format!("composer-banner-{color}").into()))
                    .size(px(SWATCH_SIZE))
                    .rounded_full()
                    .border(px(SWATCH_RING))
                    .border_color(if selected {
                        white()
                    } else {
                        white().opacity(0.4)
                    })
                    .bg(banner_fill(color))
                    .cursor_pointer()
                    .tooltip(move |window, cx| Tooltip::new(color_label(color)).build(window, cx))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(draft) = this.announcement.as_mut() {
                            draft.color = color;
                            cx.notify();
                        }
                    }))
            }));
        Some(
            div()
                .debug_selector(|| POST_ANNOUNCEMENT_TAG.into())
                .w_full()
                .px(px(CARD_INSET))
                .pt(px(CARD_INSET))
                .child(
                    banner_frame(&BannerBackground::Color(draft.color.to_owned()), directory)
                        .child(
                            headline_style(div().w_full()).child(
                                Input::new(&draft.headline)
                                    .appearance(false)
                                    .bordered(false)
                                    .with_size(Size::Size(px(HEADLINE_SIZE / INPUT_TEXT_SCALE))),
                            ),
                        )
                        .child(swatches),
                ),
        )
    }

    pub(super) fn render_post_header(&self, cx: &mut Context<Self>) -> Option<Div> {
        let subject = self
            .post_subject
            .clone()
            .filter(|_| self.editing.is_none())?;
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
                .children(self.render_announcement(cx))
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
