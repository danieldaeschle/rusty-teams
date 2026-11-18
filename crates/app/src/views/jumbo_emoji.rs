use std::collections::HashMap;
use std::time::Instant;

use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::JumboEmoji;

use crate::emoticons::{self, StripLoader};
use crate::theme;

const EMOTICON_SIZE: f32 = 50.;
const GLYPH_SIZE: f32 = 40.;
const EMOTICON_GAP: f32 = 4.;
/// When each big emoji started its one play. Kept for the app's lifetime, so rows that
/// are laid out off screen or rebuilt keep their start and a reopened chat does not replay.
#[derive(Default)]
struct PlayedEmoticons(HashMap<ElementId, Instant>);

impl Global for PlayedEmoticons {}

/// Server message id of a sent message to the key its pending row had, so the big emoji
/// keeps playing across the swap from the pending row to the server copy.
#[derive(Default)]
struct SentEmoticonKeys(HashMap<String, String>);

impl Global for SentEmoticonKeys {}

pub fn hand_over_sent(pending_key: &str, message_id: &str, cx: &mut App) {
    cx.default_global::<SentEmoticonKeys>()
        .0
        .insert(message_id.to_owned(), pending_key.to_owned());
}

fn play_start(id: &ElementId, active: bool, now: Instant, cx: &mut App) -> Option<Instant> {
    let played = cx.default_global::<PlayedEmoticons>();
    match played.0.get(id) {
        Some(started) => Some(*started),
        None if active => {
            played.0.insert(id.clone(), now);
            Some(now)
        }
        None => None,
    }
}

#[derive(IntoElement)]
struct JumboEmoticon {
    id: ElementId,
    emoji: JumboEmoji,
}

fn glyph(emoji: &JumboEmoji) -> AnyElement {
    div()
        .h(px(EMOTICON_SIZE))
        .flex()
        .items_center()
        .text_size(px(GLYPH_SIZE))
        .font_family(theme::EMOJI_FONT_FAMILY)
        .text_color(theme::text())
        .child(emoji.glyph.clone())
        .into_any_element()
}

impl RenderOnce for JumboEmoticon {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(resolved) = emoticons::resolve(self.emoji.id.as_deref(), &self.emoji.glyph) else {
            return glyph(&self.emoji);
        };
        let images = match window.use_asset::<StripLoader>(&resolved.strip_source(), cx) {
            Some(Ok(images)) => images,
            Some(Err(_)) => return glyph(&self.emoji),
            None => return div().size(px(EMOTICON_SIZE)).into_any_element(),
        };
        let now = Instant::now();
        let started = play_start(&self.id, window.is_window_active(), now, cx);
        let playing =
            started.is_some_and(|started| now.duration_since(started) < resolved.loop_duration());
        let image = if playing {
            images.animated.clone()
        } else {
            images.still.clone()
        };
        let image_id = ElementId::Name(format!("{}-image", self.id).into());
        div()
            .id(self.id.clone())
            .size(px(EMOTICON_SIZE))
            .flex_none()
            .on_hover({
                let id = self.id;
                move |hovered, window, cx| {
                    if *hovered {
                        cx.default_global::<PlayedEmoticons>()
                            .0
                            .insert(id.clone(), Instant::now());
                        window.refresh();
                    }
                }
            })
            .child(img(ImageSource::Render(image)).id(image_id).size_full())
            .into_any_element()
    }
}

pub fn jumbo_row(message_key: &str, jumbo: &[JumboEmoji], own: bool, cx: &App) -> Div {
    let message_key = cx
        .try_global::<SentEmoticonKeys>()
        .and_then(|sent| sent.0.get(message_key))
        .map_or(message_key, String::as_str);
    h_flex()
        .gap(px(EMOTICON_GAP))
        .when(own, |row| row.justify_end())
        .children(
            jumbo
                .iter()
                .enumerate()
                .map(|(index, emoji)| JumboEmoticon {
                    id: ElementId::Name(format!("jumbo-{message_key}-{index}").into()),
                    emoji: emoji.clone(),
                }),
        )
}
