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
const APPEAR_LOOPS: u32 = 2;
const HOVER_LOOPS: u32 = 5;

struct Playback {
    started: Option<Instant>,
    loops: u32,
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
        let playback = window.use_keyed_state(self.id.clone(), cx, |_, _| Playback {
            started: None,
            loops: APPEAR_LOOPS,
        });
        let now = Instant::now();
        let (started, loops) = playback.update(cx, |playback, _| {
            (*playback.started.get_or_insert(now), playback.loops)
        });
        let playing = now.duration_since(started) < resolved.loop_duration() * loops;
        let image = if playing {
            images.animated.clone()
        } else {
            images.still.clone()
        };
        div()
            .id(self.id)
            .size(px(EMOTICON_SIZE))
            .flex_none()
            .on_hover(move |hovered, _, cx| {
                if *hovered {
                    playback.update(cx, |playback, cx| {
                        playback.started = Some(Instant::now());
                        playback.loops = HOVER_LOOPS;
                        cx.notify();
                    });
                }
            })
            .child(img(ImageSource::Render(image)).size_full())
            .into_any_element()
    }
}

pub fn jumbo_row(message_key: &str, jumbo: &[JumboEmoji], own: bool) -> Div {
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
