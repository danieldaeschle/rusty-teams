use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{LinkPreview, first_public_link};

use super::widgets::icon;
use crate::app_state::AppHandle;
use crate::data::Directory;
use crate::theme;

const CARD_WIDTH: f32 = 360.;
const THUMB_WIDTH: f32 = 112.;
const THUMB_WIDTH_COMPACT: f32 = 72.;
const THUMB_MIN_HEIGHT: f32 = 48.;
const THUMB_MAX_HEIGHT: f32 = 112.;
const THUMB_RADIUS: f32 = 6.;
const CLOSE_SIZE: f32 = 22.;
const JOIN_CHIP_HEIGHT: f32 = 24.;

pub type CloseHandler = Rc<dyn Fn(&mut Window, &mut App)>;

pub fn thumbnail_size(
    declared: Option<(u32, u32)>,
    loaded: Option<(u32, u32)>,
    width: f32,
) -> (f32, f32) {
    let aspect = declared
        .or(loaded)
        .filter(|(width, height)| *width > 0 && *height > 0)
        .map_or(0.6, |(image_width, image_height)| {
            image_height as f32 / image_width as f32
        });
    (
        width,
        (width * aspect)
            .clamp(THUMB_MIN_HEIGHT, THUMB_MAX_HEIGHT)
            .round(),
    )
}

fn thumbnail(
    preview: &LinkPreview,
    id: &str,
    directory: &Directory,
    width: f32,
) -> Option<AnyElement> {
    let image = preview.image()?;
    let entry = directory.image(&image.url);
    let (width, height) = thumbnail_size(
        image.width.zip(image.height),
        entry.and_then(|entry| entry.size),
        width,
    );
    let frame = div()
        .id(ElementId::Name(format!("{id}-thumbnail").into()))
        .w(px(width))
        .h(px(height))
        .flex_none()
        .rounded(px(THUMB_RADIUS))
        .overflow_hidden()
        .bg(theme::background());
    Some(match entry {
        Some(entry) => frame
            .child(
                img(entry.path.clone())
                    .size_full()
                    .object_fit(ObjectFit::Cover),
            )
            .into_any_element(),
        None => frame
            .flex()
            .items_center()
            .justify_center()
            .child(icon(IconName::Image, 18., theme::text_muted()))
            .into_any_element(),
    })
}

pub fn link_preview_card(
    preview: &LinkPreview,
    id: String,
    directory: &Directory,
    compact: bool,
    close: Option<CloseHandler>,
) -> AnyElement {
    let thumb_width = if compact {
        THUMB_WIDTH_COMPACT
    } else {
        THUMB_WIDTH
    };
    let url = preview.url.clone();
    let title = preview.title.clone().unwrap_or_else(|| preview.domain());
    let close_button = close.map(|close| {
        div()
            .id(ElementId::Name(format!("{id}-close").into()))
            .absolute()
            .top(px(4.))
            .right(px(4.))
            .size(px(CLOSE_SIZE))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|button| button.bg(theme::row_hover()))
            .child(icon(IconName::Close, 12., theme::text_muted()))
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                close(window, cx);
            })
    });
    h_flex()
        .id(ElementId::Name(id.clone().into()))
        .relative()
        .overflow_hidden()
        .when(compact, |card| card.w_full())
        .when(!compact, |card| card.w(px(CARD_WIDTH)))
        .max_w(relative(1.))
        .gap(px(10.))
        .p(px(8.))
        .items_start()
        .rounded(px(9.))
        .bg(theme::surface())
        .border_1()
        .border_color(theme::border_strong())
        .cursor_pointer()
        .hover(|card| card.bg(theme::row_hover()).border_color(theme::accent()))
        .children(thumbnail(preview, &id, directory, thumb_width))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(2.))
                .when(close_button.is_some(), |column| column.pr(px(CLOSE_SIZE)))
                .child(
                    div()
                        .line_clamp(2)
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::text_strong())
                        .child(title),
                )
                .children(preview.description.clone().map(|description| {
                    div()
                        .line_clamp(2)
                        .text_size(px(12.))
                        .text_color(theme::text_muted())
                        .child(description)
                }))
                .child(
                    div()
                        .truncate()
                        .text_size(px(11.))
                        .text_color(theme::text_muted())
                        .child(preview.domain()),
                ),
        )
        .children(close_button)
        .on_click(move |_, _, cx| cx.open_url(&url))
        .into_any_element()
}

pub fn meeting_join_chip(id: String, url: String) -> AnyElement {
    h_flex()
        .id(ElementId::Name(id.into()))
        .flex_none()
        .self_start()
        .h(px(JOIN_CHIP_HEIGHT))
        .px(px(10.))
        .gap(px(5.))
        .items_center()
        .rounded_full()
        .bg(theme::accent())
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::on_accent())
        .cursor_pointer()
        .hover(|chip| chip.opacity(0.85))
        .child(icon(IconName::Video, 13., theme::on_accent()))
        .child("Join")
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            let Some(handle) = cx.try_global::<AppHandle>() else {
                return;
            };
            let app = handle.0.clone();
            app.update(cx, |state, cx| state.join_meeting_link(&url, cx));
        })
        .into_any_element()
}

#[derive(Default)]
pub struct ComposeLink {
    url: Option<String>,
    preview: Option<LinkPreview>,
    dismissed: Option<String>,
    pub meeting: Option<String>,
    pub lookup: Option<Task<()>>,
}

impl ComposeLink {
    pub fn shown(&self) -> Option<&LinkPreview> {
        self.preview.as_ref()
    }

    /// The link to look up when the draft's link changed.
    pub fn observe(&mut self, link: Option<String>, draft_blank: bool) -> Option<String> {
        if draft_blank {
            self.reset();
            return None;
        }
        let Some(link) = link else {
            self.url = None;
            self.preview = None;
            self.lookup = None;
            return None;
        };
        if self.url.as_deref() == Some(link.as_str()) {
            return None;
        }
        self.preview = None;
        self.lookup = None;
        self.url = Some(link.clone());
        (self.dismissed.as_deref() != Some(link.as_str())).then_some(link)
    }

    pub fn apply(&mut self, url: &str, preview: Option<LinkPreview>) -> bool {
        let current = self.url.as_deref() == Some(url) && self.dismissed.as_deref() != Some(url);
        if current {
            self.preview = preview;
        }
        current
    }

    pub fn dismiss(&mut self) {
        self.dismissed = self.url.clone();
        self.preview = None;
        self.lookup = None;
    }

    pub fn restore(&mut self, preview: Option<LinkPreview>) {
        self.dismissed = None;
        self.lookup = None;
        self.url = preview.as_ref().map(|preview| preview.url.clone());
        self.preview = preview;
    }

    pub fn reset(&mut self) {
        *self = ComposeLink::default();
    }
}

pub fn draft_link(draft: &teams_core::Draft) -> Option<String> {
    first_public_link(&draft.to_html()).or_else(|| {
        draft
            .plain_text()
            .split_whitespace()
            .map(|word| word.trim_end_matches(['.', ',', ';', ':', '!', '?', ')']))
            .find(|word| {
                let lower = word.to_ascii_lowercase();
                (lower.starts_with("http://") || lower.starts_with("https://"))
                    && teams_core::is_public_link(word)
            })
            .map(str::to_owned)
    })
}

#[cfg(test)]
mod tests {
    use teams_core::LinkPreview;

    use super::{ComposeLink, THUMB_MAX_HEIGHT, THUMB_MIN_HEIGHT, draft_link, thumbnail_size};

    fn preview(url: &str) -> LinkPreview {
        LinkPreview {
            url: url.to_owned(),
            title: Some("Title".to_owned()),
            description: None,
            image_url: None,
            image_width: None,
            image_height: None,
        }
    }

    #[test]
    fn thumbnails_keep_the_aspect_within_bounds() {
        assert_eq!(thumbnail_size(Some((320, 160)), None, 112.), (112., 56.));
        assert_eq!(
            thumbnail_size(Some((100, 400)), None, 112.),
            (112., THUMB_MAX_HEIGHT)
        );
        assert_eq!(
            thumbnail_size(Some((400, 20)), None, 112.),
            (112., THUMB_MIN_HEIGHT)
        );
        assert_eq!(thumbnail_size(None, Some((200, 100)), 100.), (100., 50.));
        assert_eq!(thumbnail_size(None, None, 100.), (100., 60.));
    }

    #[test]
    fn a_new_link_is_looked_up_once() {
        let mut link = ComposeLink::default();
        assert_eq!(
            link.observe(Some("https://a.example".into()), false),
            Some("https://a.example".into())
        );
        assert_eq!(link.observe(Some("https://a.example".into()), false), None);
        assert!(link.apply("https://a.example", Some(preview("https://a.example"))));
        assert!(link.shown().is_some());
        assert_eq!(
            link.observe(Some("https://b.example".into()), false),
            Some("https://b.example".into())
        );
        assert!(link.shown().is_none());
    }

    #[test]
    fn a_late_answer_for_an_old_link_is_ignored() {
        let mut link = ComposeLink::default();
        link.observe(Some("https://a.example".into()), false);
        link.observe(Some("https://b.example".into()), false);
        assert!(!link.apply("https://a.example", Some(preview("https://a.example"))));
        assert!(link.shown().is_none());
    }

    #[test]
    fn a_dismissed_link_stays_hidden_until_the_link_changes_or_the_draft_clears() {
        let mut link = ComposeLink::default();
        link.observe(Some("https://a.example".into()), false);
        link.apply("https://a.example", Some(preview("https://a.example")));
        link.dismiss();
        assert!(link.shown().is_none());
        assert_eq!(link.observe(Some("https://a.example".into()), false), None);
        assert_eq!(link.observe(None, false), None);
        assert_eq!(link.observe(Some("https://a.example".into()), false), None);
        assert!(!link.apply("https://a.example", Some(preview("https://a.example"))));
        assert_eq!(
            link.observe(Some("https://b.example".into()), false),
            Some("https://b.example".into())
        );
        link.dismiss();
        assert_eq!(link.observe(None, true), None);
        assert_eq!(
            link.observe(Some("https://b.example".into()), false),
            Some("https://b.example".into())
        );
    }

    #[test]
    fn removing_the_link_removes_the_card() {
        let mut link = ComposeLink::default();
        link.observe(Some("https://a.example".into()), false);
        link.apply("https://a.example", Some(preview("https://a.example")));
        assert_eq!(link.observe(None, false), None);
        assert!(link.shown().is_none());
    }

    #[test]
    fn restoring_a_sent_preview_shows_it_without_a_new_lookup() {
        let mut link = ComposeLink::default();
        link.restore(Some(preview("https://a.example")));
        assert!(link.shown().is_some());
        assert_eq!(link.observe(Some("https://a.example".into()), false), None);
        assert!(link.shown().is_some());
    }

    #[test]
    fn finds_anchors_and_bare_urls_in_a_draft() {
        let marked = teams_core::Draft::from_markdown("see [docs](https://docs.example/a) now");
        assert_eq!(
            draft_link(&marked).as_deref(),
            Some("https://docs.example/a")
        );
        let bare = teams_core::Draft::plain("go to https://b.example/x, ok");
        assert_eq!(draft_link(&bare).as_deref(), Some("https://b.example/x"));
        let internal = teams_core::Draft::plain("https://contoso.sharepoint.com/x");
        assert_eq!(draft_link(&internal), None);
    }
}
