use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Sizable as _, Size, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::banner_image::prepare_banner_image;
use teams_core::{AnnouncementBanner, BannerBackground};

use super::{COMPOSER_BUTTON_SIZE, COMPOSER_ICON_SIZE, Composer, ComposerEvent};
use crate::runtime;
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
pub const POST_AUDIENCE_TAG: &str = "composer/post/audience";
const DEMO_AUDIENCE: usize = 24;
const AUDIENCE_HEIGHT: f32 = 32.;
const ANNOUNCEMENT_TOOLTIP: &str = "Announcement";
const BANNER_IMAGE_TOOLTIP: &str = "Upload an image";
const HEADLINE_PLACEHOLDER: &str = "Type a headline";
pub const SUBJECT_PLACEHOLDER: &str = "Add a subject";
const SUBHEADING_PLACEHOLDER: &str = "Add a subheading";
const HEADLINE_SIZE: f32 = 32.;
const COMPOSER_BANNER_HEIGHT: f32 = 240.;
const HEADLINE_LINE: f32 = 1.25;
const SWATCH_SIZE: f32 = 18.;
const SWATCH_RING: f32 = 2.;
const SWATCH_GAP: f32 = 6.;
const SWATCH_INSET: f32 = 8.;
const POST_LABEL: &str = "Post";
const CLOSE_TOOLTIP: &str = "Close";
const AUTHOR_AVATAR: f32 = 24.;
const CARD_INSET: f32 = 12.;
const SUBJECT_SIZE: f32 = 18.;
const SUBJECT_LINE: f32 = 1.4;
const INPUT_TEXT_SCALE: f32 = 0.875;
const SUBJECT_INSET: f32 = 8.;
const TEXT_INSET: f32 = 16.;
const SUBHEADING_GAP: f32 = 8.;

pub(super) struct AnnouncementDraft {
    pub(super) headline: Entity<InputState>,
    color: &'static str,
    image: Option<String>,
    _changes: Subscription,
}

impl AnnouncementDraft {
    fn background(&self) -> BannerBackground {
        match &self.image {
            Some(path) => BannerBackground::LocalImage(path.clone()),
            None => BannerBackground::Color(self.color.to_owned()),
        }
    }
}

pub fn audience_hint(count: usize) -> String {
    match count {
        1 => "1 person will see your message.".to_owned(),
        count => format!("{count} people will see your message."),
    }
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
            self.schedule_draft_save(cx);
            cx.notify();
        }
    }

    pub(super) fn post_layout(&self) -> bool {
        self.post_subject.is_some() && self.editing.is_none()
    }

    pub(super) fn post_subject_text(&self, cx: &App) -> Option<String> {
        let subject = self.post_subject.as_ref().filter(|_| self.post_layout())?;
        let text = subject.read(cx).value().trim().to_owned();
        (!text.is_empty()).then_some(text)
    }

    /// Saves the draft with its post subject and banner before the post card closes.
    pub fn leave_conversation(&mut self, next_conversation_id: &str, cx: &mut Context<Self>) {
        if self.conversation_id.as_deref() != Some(next_conversation_id) {
            self.save_draft_now(cx);
            self.conversation_id = None;
        }
    }

    pub(super) fn announcement_banner(&self, cx: &App) -> Option<AnnouncementBanner> {
        let draft = self.announcement.as_ref().filter(|_| self.post_layout())?;
        let title = draft.headline.read(cx).value().trim().to_owned();
        (!title.is_empty()).then(|| AnnouncementBanner {
            title,
            background: draft.background(),
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
                    BannerBackground::Image(_) | BannerBackground::LocalImage(_) => {
                        BANNER_COLORS[0]
                    }
                };
                self.open_announcement(color, window, cx);
                if let Some(draft) = self.announcement.as_mut() {
                    if let BannerBackground::LocalImage(path) = &banner.background {
                        draft.image = Some(path.clone());
                    }
                    let title = banner.title.clone();
                    draft
                        .headline
                        .update(cx, |input, cx| input.set_value(title, window, cx));
                }
            }
            None => self.close_announcement(window, cx),
        }
    }

    pub fn begin_announcement(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.announcement.is_none() {
            self.toggle_announcement(window, cx);
        }
    }

    pub(super) fn toggle_announcement(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
        let changes = cx.subscribe(&headline, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.schedule_draft_save(cx);
            }
        });
        self.announcement = Some(AnnouncementDraft {
            headline,
            color,
            image: None,
            _changes: changes,
        });
        self.set_subject_placeholder(SUBHEADING_PLACEHOLDER, window, cx);
        self.load_audience(cx);
        self.schedule_draft_save(cx);
        cx.notify();
    }

    fn pick_banner_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(source) = paths.into_iter().next() else {
                return;
            };
            let prepared = cx
                .background_executor()
                .spawn(async move { prepare_banner_image(&source) })
                .await;
            this.update(cx, |this, cx| match prepared {
                Ok(path) => {
                    if let Some(draft) = this.announcement.as_mut() {
                        draft.image = Some(path.to_string_lossy().into_owned());
                        this.schedule_draft_save(cx);
                        cx.notify();
                    }
                }
                Err(error) => log::warn!("banner image: {error}"),
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn close_announcement(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.announcement.take().is_some() {
            self.set_subject_placeholder(SUBJECT_PLACEHOLDER, window, cx);
            self.schedule_draft_save(cx);
            cx.notify();
        }
    }

    fn load_audience(&mut self, cx: &mut Context<Self>) {
        let Some(channel_id) = self.conversation_id.clone() else {
            return;
        };
        if self
            .audience
            .as_ref()
            .is_some_and(|(id, _)| *id == channel_id)
        {
            return;
        }
        let state = self.app.read(cx);
        if state.mode.demo {
            self.audience = Some((channel_id, DEMO_AUDIENCE));
            return;
        }
        let Some(engine) = state.engine.clone() else {
            return;
        };
        let lookup = channel_id.clone();
        let receiver = runtime::spawn(async move { engine.channel_member_count(&lookup).await });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(count)) = receiver.await else {
                return;
            };
            this.update(cx, |this, cx| {
                if this.conversation_id.as_deref() == Some(channel_id.as_str()) {
                    this.audience = Some((channel_id, count));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn render_audience_hint(&self) -> Option<Div> {
        self.announcement.as_ref().filter(|_| self.post_layout())?;
        let (channel_id, count) = self.audience.as_ref()?;
        (self.conversation_id.as_ref() == Some(channel_id)).then(|| {
            h_flex()
                .debug_selector(|| POST_AUDIENCE_TAG.into())
                .w_full()
                .h(px(AUDIENCE_HEIGHT))
                .mb(px(8.))
                .px(px(CARD_INSET))
                .items_center()
                .rounded(px(10.))
                .bg(theme::surface_raised())
                .text_size(px(12.))
                .text_color(theme::text_muted())
                .child(audience_hint(*count))
        })
    }

    pub fn sync_subject_placeholder(&self, window: &mut Window, cx: &mut Context<Self>) {
        let placeholder = if self.announcement.is_some() {
            SUBHEADING_PLACEHOLDER
        } else {
            SUBJECT_PLACEHOLDER
        };
        self.set_subject_placeholder(placeholder, window, cx);
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
                let selected = color == draft.color && draft.image.is_none();
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
                            draft.image = None;
                            this.schedule_draft_save(cx);
                            cx.notify();
                        }
                    }))
            }))
            .child({
                let selected = draft.image.is_some();
                div()
                    .id("composer-banner-image")
                    .size(px(SWATCH_SIZE))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .border(px(SWATCH_RING))
                    .border_color(if selected {
                        white()
                    } else {
                        white().opacity(0.4)
                    })
                    .bg(black().opacity(0.35))
                    .cursor_pointer()
                    .tooltip(|window, cx| Tooltip::new(BANNER_IMAGE_TOOLTIP).build(window, cx))
                    .child(symbol("image", SWATCH_SIZE - 6., white()))
                    .on_click(cx.listener(|this, _, window, cx| this.pick_banner_image(window, cx)))
            });
        Some(
            div()
                .debug_selector(|| POST_ANNOUNCEMENT_TAG.into())
                .w_full()
                .px(px(CARD_INSET))
                .pt(px(CARD_INSET))
                .child(
                    banner_frame(&draft.background(), directory)
                        .h(px(COMPOSER_BANNER_HEIGHT))
                        .child(
                            headline_style(div().w_full()).child(
                                Input::new(&draft.headline)
                                    .appearance(false)
                                    .bordered(false)
                                    .text_center()
                                    .h(px(HEADLINE_SIZE * HEADLINE_LINE))
                                    .line_height(px(HEADLINE_SIZE * HEADLINE_LINE))
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
        let announcement = self.render_announcement(cx);
        let has_banner = announcement.is_some();
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
                .children(announcement)
                .child(
                    div()
                        .debug_selector(|| POST_CARD_SUBJECT_TAG.into())
                        .w_full()
                        .when(has_banner, |subject| subject.mt(px(SUBHEADING_GAP)))
                        .px(px(SUBJECT_INSET))
                        .py(px(6.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(
                            Input::new(&subject)
                                .appearance(false)
                                .bordered(false)
                                .h(px(SUBJECT_SIZE * SUBJECT_LINE))
                                .line_height(px(SUBJECT_SIZE * SUBJECT_LINE))
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
