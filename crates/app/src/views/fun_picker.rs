use std::time::Duration;

use gpui_kit::base::PopoverState;
use gpui_kit::component::{
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::Gif;

use crate::app_state::AppState;
use crate::emoji;
use crate::gifs::{self, GifError};
use crate::remote_image::{RemoteImage, image_source};
use crate::stickers::{self, Sticker};
use crate::theme;

const TAB_KEY: &str = "fun_picker_tab";
const WIDTH: f32 = 360.;
const HEIGHT: f32 = 380.;
const PADDING: f32 = 8.;
const CONTENT_WIDTH: f32 = WIDTH - 2. * PADDING;
const EMOJI_COLUMNS: usize = 8;
const EMOJI_CELL: f32 = 32.;
const EMOJI_GAP: f32 = 2.;
const EMOJI_SEARCH_LIMIT: usize = 240;
const GIF_COLUMNS: usize = 3;
const GIF_GAP: f32 = 6.;
const GIF_DEBOUNCE: Duration = Duration::from_millis(300);
const GIF_MIN_HEIGHT: f32 = 48.;
const GIF_MAX_HEIGHT: f32 = 200.;
const GIF_PLACEHOLDER_HEIGHTS: [f32; 9] = [90., 70., 110., 80., 100., 70., 100., 90., 80.];
const STICKER_COLUMNS: usize = 4;
const STICKER_GAP: f32 = 6.;
const POPULAR_LABEL: &str = "Beliebt";
const TILE_RADIUS: f32 = 6.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Emoji,
    Gif,
    Sticker,
}

impl Tab {
    const ALL: [Tab; 3] = [Tab::Emoji, Tab::Gif, Tab::Sticker];

    fn key(self) -> &'static str {
        match self {
            Tab::Emoji => "emoji",
            Tab::Gif => "gif",
            Tab::Sticker => "sticker",
        }
    }

    fn from_key(key: &str) -> Option<Tab> {
        Tab::ALL.into_iter().find(|tab| tab.key() == key)
    }

    fn label(self) -> &'static str {
        match self {
            Tab::Emoji => "Emoji",
            Tab::Gif => "GIF",
            Tab::Sticker => "Sticker",
        }
    }

    fn placeholder(self) -> &'static str {
        match self {
            Tab::Emoji => "Emoji suchen",
            Tab::Gif => "GIFs suchen",
            Tab::Sticker => "Sticker suchen",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StickerChip {
    Popular,
    Category(&'static str),
}

enum GifState {
    Loading,
    Ready(Vec<Gif>),
    Failed,
    Disabled,
}

enum EmojiRow {
    Header(&'static str),
    Cells(Vec<String>),
}

pub enum FunPickerEvent {
    Emoji(String),
    Image(RemoteImage),
}

pub struct FunPicker {
    app: Entity<AppState>,
    input: Entity<InputState>,
    tab: Tab,
    query: String,
    recent: emoji::Recent,
    popover: Option<WeakEntity<PopoverState>>,
    emoji_rows: Vec<EmojiRow>,
    emoji_scroll: UniformListScrollHandle,
    gifs: GifState,
    gif_generation: u64,
    gif_task: Option<Task<()>>,
    chip: StickerChip,
    sticker_rows: Vec<Vec<&'static Sticker>>,
    sticker_scroll: UniformListScrollHandle,
    _subscription: Subscription,
}

impl EventEmitter<FunPickerEvent> for FunPicker {}

fn emoji_rows(query: &str, recent: &[String]) -> Vec<EmojiRow> {
    let chunked = |glyphs: Vec<String>| -> Vec<EmojiRow> {
        glyphs
            .chunks(EMOJI_COLUMNS)
            .map(|chunk| EmojiRow::Cells(chunk.to_vec()))
            .collect()
    };
    if !query.is_empty() {
        let found = emoji::search(query, recent, EMOJI_SEARCH_LIMIT)
            .into_iter()
            .map(|found| found.glyph.to_owned())
            .collect();
        return chunked(found);
    }
    let mut rows = Vec::new();
    if !recent.is_empty() {
        rows.push(EmojiRow::Header("Zuletzt verwendet"));
        rows.extend(chunked(recent.to_vec()));
    }
    rows.push(EmojiRow::Header("Alle Emojis"));
    rows.extend(chunked(emoji::glyphs().map(str::to_owned).collect()));
    rows
}

fn sticker_rows(query: &str, chip: StickerChip) -> Vec<Vec<&'static Sticker>> {
    let stickers = if !query.is_empty() {
        stickers::search(query)
    } else {
        match chip {
            StickerChip::Popular => stickers::popular(),
            StickerChip::Category(category) => stickers::in_category(category),
        }
    };
    stickers
        .chunks(STICKER_COLUMNS)
        .map(<[&Sticker]>::to_vec)
        .collect()
}

/// Indices per column; each item goes to the currently shortest column.
fn masonry(heights: &[f32], columns: usize) -> Vec<Vec<usize>> {
    let mut layout: Vec<Vec<usize>> = vec![Vec::new(); columns];
    let mut totals = vec![0_f32; columns];
    for (index, height) in heights.iter().enumerate() {
        let shortest = totals
            .iter()
            .enumerate()
            .min_by(|left, right| left.1.total_cmp(right.1))
            .map_or(0, |(column, _)| column);
        layout[shortest].push(index);
        totals[shortest] += height + GIF_GAP;
    }
    layout
}

fn gif_tile_width() -> f32 {
    (CONTENT_WIDTH - GIF_GAP * (GIF_COLUMNS - 1) as f32) / GIF_COLUMNS as f32
}

fn gif_tile_height(gif: &Gif) -> f32 {
    let ratio = gif.preview_height as f32 / gif.preview_width.max(1) as f32;
    (gif_tile_width() * ratio).clamp(GIF_MIN_HEIGHT, GIF_MAX_HEIGHT)
}

fn sticker_tile_size() -> f32 {
    (CONTENT_WIDTH - STICKER_GAP * (STICKER_COLUMNS - 1) as f32) / STICKER_COLUMNS as f32
}

impl FunPicker {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(Tab::Emoji.placeholder()));
        let subscription = cx.subscribe_in(&input, window, Self::on_input_event);
        let recent = emoji::Recent::load(&app.read(cx).store);
        FunPicker {
            app,
            input,
            tab: Tab::Emoji,
            query: String::new(),
            emoji_rows: emoji_rows("", &recent.glyphs()),
            recent,
            popover: None,
            emoji_scroll: UniformListScrollHandle::new(),
            gifs: GifState::Loading,
            gif_generation: 0,
            gif_task: None,
            chip: StickerChip::Popular,
            sticker_rows: sticker_rows("", StickerChip::Popular),
            sticker_scroll: UniformListScrollHandle::new(),
            _subscription: subscription,
        }
    }

    pub fn set_popover(&mut self, popover: WeakEntity<PopoverState>) {
        self.popover = Some(popover);
    }

    pub fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let store = self.app.read(cx).store.clone();
        self.recent.reload(&store);
        self.tab = store
            .meta(TAB_KEY)
            .ok()
            .flatten()
            .and_then(|key| Tab::from_key(&key))
            .unwrap_or(Tab::Emoji);
        self.show_tab(window, cx);
    }

    fn set_tab(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        if tab == self.tab {
            return;
        }
        self.tab = tab;
        let _ = self.app.read(cx).store.set_meta(TAB_KEY, tab.key());
        self.show_tab(window, cx);
    }

    fn show_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.query.clear();
        let placeholder = self.tab.placeholder();
        self.input.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.set_placeholder(placeholder, window, cx);
            state.focus(window, cx);
        });
        self.refresh(false, cx);
    }

    fn refresh(&mut self, debounce: bool, cx: &mut Context<Self>) {
        match self.tab {
            Tab::Emoji => {
                self.emoji_rows = emoji_rows(&self.query, &self.recent.glyphs());
                self.emoji_scroll.scroll_to_item(0, ScrollStrategy::Top);
            }
            Tab::Gif => self.search_gifs(debounce, cx),
            Tab::Sticker => {
                self.sticker_rows = sticker_rows(&self.query, self.chip);
                self.sticker_scroll.scroll_to_item(0, ScrollStrategy::Top);
            }
        }
        cx.notify();
    }

    fn on_input_event(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                let query = input.read(cx).value().trim().to_owned();
                if query != self.query {
                    self.query = query;
                    self.refresh(true, cx);
                }
            }
            InputEvent::PressEnter { .. } if self.tab == Tab::Emoji => {
                let first = self.emoji_rows.iter().find_map(|row| match row {
                    EmojiRow::Cells(glyphs) => glyphs.first().cloned(),
                    EmojiRow::Header(_) => None,
                });
                if let Some(glyph) = first.filter(|_| !self.query.is_empty()) {
                    self.pick_emoji(&glyph, cx);
                }
            }
            _ => {}
        }
    }

    fn search_gifs(&mut self, debounce: bool, cx: &mut Context<Self>) {
        self.gif_generation += 1;
        let generation = self.gif_generation;
        self.gifs = GifState::Loading;
        let query = self.query.clone();
        let (demo, engine) = {
            let state = self.app.read(cx);
            (state.mode.demo, state.engine.clone())
        };
        self.gif_task = Some(cx.spawn(async move |this, cx| {
            if debounce {
                cx.background_executor().timer(GIF_DEBOUNCE).await;
            }
            let outcome = gifs::search(engine, demo, query)
                .await
                .unwrap_or(Err(GifError::Failed));
            this.update(cx, |this, cx| {
                if this.gif_generation != generation {
                    return;
                }
                this.gifs = match outcome {
                    Ok(found) => GifState::Ready(found),
                    Err(GifError::Disabled) => GifState::Disabled,
                    Err(GifError::Failed) => GifState::Failed,
                };
                cx.notify();
            })
            .ok();
        }));
    }

    fn pick_emoji(&mut self, glyph: &str, cx: &mut Context<Self>) {
        self.recent.push(glyph);
        self.refresh(false, cx);
        cx.emit(FunPickerEvent::Emoji(glyph.to_owned()));
    }

    fn pick_image(&mut self, image: RemoteImage, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(popover) = &self.popover {
            popover
                .update(cx, |state, cx| state.dismiss(window, cx))
                .ok();
        }
        cx.emit(FunPickerEvent::Image(image));
    }

    fn select_chip(&mut self, chip: StickerChip, cx: &mut Context<Self>) {
        self.chip = chip;
        self.refresh(false, cx);
    }

    fn tab_button(&self, tab: Tab, cx: &mut Context<Self>) -> Stateful<Div> {
        let selected = tab == self.tab;
        div()
            .id(("fun-picker-tab", tab as usize))
            .h(px(32.))
            .px(px(14.))
            .flex()
            .items_center()
            .cursor_pointer()
            .text_size(px(13.))
            .font_weight(FontWeight::SEMIBOLD)
            .border_b_2()
            .border_color(if selected {
                theme::accent()
            } else {
                transparent_black()
            })
            .text_color(if selected {
                theme::accent_text()
            } else {
                theme::text_muted()
            })
            .hover(|button| button.text_color(theme::text_strong()))
            .child(tab.label())
            .on_click(cx.listener(move |this, _, window, cx| this.set_tab(tab, window, cx)))
    }

    fn search_field(&self) -> Div {
        div()
            .h(px(32.))
            .px(px(8.))
            .flex()
            .flex_none()
            .items_center()
            .rounded(px(8.))
            .bg(theme::surface())
            .border_1()
            .border_color(theme::border_strong())
            .child(Input::new(&self.input).appearance(false).bordered(false))
    }

    fn message(text: String) -> Div {
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(8.))
            .items_center()
            .justify_center()
            .text_size(px(12.5))
            .text_color(theme::text_muted())
            .child(text)
    }

    fn emoji_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let row = match &self.emoji_rows[index] {
            EmojiRow::Header(title) => {
                return div()
                    .h(px(EMOJI_CELL + EMOJI_GAP))
                    .flex()
                    .items_end()
                    .pb(px(4.))
                    .text_size(px(11.))
                    .text_color(theme::text_muted())
                    .child(*title)
                    .into_any_element();
            }
            EmojiRow::Cells(glyphs) => glyphs,
        };
        let cells = row.iter().enumerate().map(|(column, glyph)| {
            let picked = glyph.clone();
            div()
                .id(("fun-picker-emoji", index * EMOJI_COLUMNS + column))
                .size(px(EMOJI_CELL))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .text_size(px(20.))
                .cursor_pointer()
                .hover(|cell| cell.bg(theme::border_strong()))
                .child(glyph.clone())
                .on_click(cx.listener(move |this, _, _, cx| this.pick_emoji(&picked, cx)))
        });
        h_flex()
            .h(px(EMOJI_CELL + EMOJI_GAP))
            .gap(px(EMOJI_GAP))
            .children(cells)
            .into_any_element()
    }

    fn render_emoji(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.emoji_rows.is_empty() {
            return Self::message(format!("Keine Emojis zu \"{}\"", self.query)).into_any_element();
        }
        uniform_list(
            "fun-picker-emoji-list",
            self.emoji_rows.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| this.emoji_row(index, cx))
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.emoji_scroll)
        .size_full()
        .into_any_element()
    }

    fn gif_tile(&self, gif: &Gif, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let picked = RemoteImage::gif(gif);
        div()
            .id(("fun-picker-gif", index))
            .w(px(gif_tile_width()))
            .h(px(gif_tile_height(gif)))
            .flex_none()
            .rounded(px(TILE_RADIUS))
            .overflow_hidden()
            .bg(theme::surface_raised())
            .cursor_pointer()
            .hover(|tile| tile.opacity(0.85))
            .child(
                img(image_source(&gif.preview_url))
                    .size_full()
                    .object_fit(ObjectFit::Cover),
            )
            .on_click(
                cx.listener(move |this, _, window, cx| this.pick_image(picked.clone(), window, cx)),
            )
            .into_any_element()
    }

    fn gif_columns(columns: Vec<Vec<AnyElement>>) -> AnyElement {
        h_flex()
            .w_full()
            .items_start()
            .gap(px(GIF_GAP))
            .children(
                columns
                    .into_iter()
                    .map(|column| v_flex().gap(px(GIF_GAP)).children(column)),
            )
            .into_any_element()
    }

    fn render_gifs(&self, cx: &mut Context<Self>) -> AnyElement {
        let body = match &self.gifs {
            GifState::Loading => {
                let layout = masonry(&GIF_PLACEHOLDER_HEIGHTS, GIF_COLUMNS);
                Self::gif_columns(
                    layout
                        .into_iter()
                        .map(|column| {
                            column
                                .into_iter()
                                .map(|index| {
                                    div()
                                        .w(px(gif_tile_width()))
                                        .h(px(GIF_PLACEHOLDER_HEIGHTS[index]))
                                        .flex_none()
                                        .rounded(px(TILE_RADIUS))
                                        .bg(theme::surface_raised())
                                        .into_any_element()
                                })
                                .collect()
                        })
                        .collect(),
                )
            }
            GifState::Ready(found) if found.is_empty() => {
                let text = if self.query.is_empty() {
                    "Keine GIFs gefunden".to_owned()
                } else {
                    format!("Keine GIFs zu \"{}\"", self.query)
                };
                return Self::message(text).into_any_element();
            }
            GifState::Ready(found) => {
                let heights: Vec<f32> = found.iter().map(gif_tile_height).collect();
                let layout = masonry(&heights, GIF_COLUMNS);
                Self::gif_columns(
                    layout
                        .into_iter()
                        .map(|column| {
                            column
                                .into_iter()
                                .map(|index| self.gif_tile(&found[index], index, cx))
                                .collect()
                        })
                        .collect(),
                )
            }
            GifState::Failed => {
                return Self::message("GIFs konnten nicht geladen werden.".to_owned())
                    .child(
                        div()
                            .id("fun-picker-gif-retry")
                            .h(px(28.))
                            .px(px(12.))
                            .flex()
                            .items_center()
                            .rounded(px(6.))
                            .border_1()
                            .border_color(theme::border_strong())
                            .text_color(theme::text())
                            .cursor_pointer()
                            .hover(|button| button.bg(theme::row_hover()))
                            .child("Erneut versuchen")
                            .on_click(cx.listener(|this, _, _, cx| this.refresh(false, cx))),
                    )
                    .into_any_element();
            }
            GifState::Disabled => {
                return Self::message("GIFs sind in deiner Organisation deaktiviert.".to_owned())
                    .into_any_element();
            }
        };
        div()
            .id("fun-picker-gifs")
            .size_full()
            .overflow_y_scroll()
            .child(body)
            .into_any_element()
    }

    fn sticker_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let size = sticker_tile_size();
        let tiles = self.sticker_rows[index]
            .iter()
            .enumerate()
            .map(|(column, sticker)| {
                let picked = RemoteImage::sticker(sticker);
                div()
                    .id(("fun-picker-sticker", index * STICKER_COLUMNS + column))
                    .size(px(size))
                    .flex_none()
                    .p(px(4.))
                    .rounded(px(TILE_RADIUS))
                    .bg(theme::surface_raised())
                    .cursor_pointer()
                    .hover(|tile| tile.bg(theme::border_strong()))
                    .child(
                        img(image_source(&sticker.url()))
                            .size_full()
                            .object_fit(ObjectFit::Contain),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.pick_image(picked.clone(), window, cx)
                    }))
            });
        h_flex()
            .h(px(size + STICKER_GAP))
            .gap(px(STICKER_GAP))
            .children(tiles)
            .into_any_element()
    }

    fn chip_button(&self, chip: StickerChip, cx: &mut Context<Self>) -> Stateful<Div> {
        let (label, id) = match chip {
            StickerChip::Popular => (POPULAR_LABEL.to_owned(), "popular"),
            StickerChip::Category(category) => (stickers::category_label(category), category),
        };
        let selected = self.query.is_empty() && chip == self.chip;
        div()
            .id(ElementId::Name(format!("fun-picker-chip-{id}").into()))
            .h(px(26.))
            .px(px(10.))
            .flex_none()
            .flex()
            .items_center()
            .rounded_full()
            .cursor_pointer()
            .text_size(px(12.))
            .when(selected, |button| {
                button
                    .bg(theme::accent_soft())
                    .text_color(theme::accent_text())
            })
            .when(!selected, |button| {
                button
                    .bg(theme::surface_raised())
                    .text_color(theme::text_muted())
                    .hover(|button| button.text_color(theme::text_strong()))
            })
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| this.select_chip(chip, cx)))
    }

    fn render_stickers(&self, cx: &mut Context<Self>) -> AnyElement {
        let chips = std::iter::once(StickerChip::Popular)
            .chain(
                stickers::categories()
                    .into_iter()
                    .map(StickerChip::Category),
            )
            .map(|chip| self.chip_button(chip, cx))
            .collect::<Vec<_>>();
        let grid = if self.sticker_rows.is_empty() {
            Self::message(format!("Keine Sticker zu \"{}\"", self.query)).into_any_element()
        } else {
            uniform_list(
                "fun-picker-sticker-list",
                self.sticker_rows.len(),
                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                    range
                        .map(|index| this.sticker_row(index, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&self.sticker_scroll)
            .size_full()
            .into_any_element()
        };
        v_flex()
            .size_full()
            .gap(px(8.))
            .child(
                h_flex()
                    .id("fun-picker-chips")
                    .w_full()
                    .flex_none()
                    .gap(px(6.))
                    .overflow_x_scroll()
                    .children(chips),
            )
            .child(div().flex_1().min_h_0().child(grid))
            .into_any_element()
    }
}

impl Render for FunPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs = Tab::ALL.map(|tab| self.tab_button(tab, cx));
        let body = match self.tab {
            Tab::Emoji => self.render_emoji(cx),
            Tab::Gif => self.render_gifs(cx),
            Tab::Sticker => self.render_stickers(cx),
        };
        v_flex()
            .w(px(WIDTH))
            .h(px(HEIGHT))
            .gap(px(8.))
            .p(px(PADDING))
            .child(h_flex().flex_none().gap(px(2.)).children(tabs))
            .child(self.search_field())
            .child(div().flex_1().min_h_0().child(body))
            .when(self.tab == Tab::Gif, |picker| {
                picker.child(
                    div()
                        .flex_none()
                        .flex()
                        .justify_end()
                        .text_size(px(10.5))
                        .text_color(theme::text_faint())
                        .child("Powered by GIPHY"),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{EmojiRow, STICKER_COLUMNS, StickerChip, Tab, emoji_rows, masonry, sticker_rows};

    #[test]
    fn tabs_round_trip_through_their_keys() {
        for tab in Tab::ALL {
            assert_eq!(Tab::from_key(tab.key()), Some(tab));
        }
        assert_eq!(Tab::from_key("unknown"), None);
    }

    #[test]
    fn masonry_fills_the_shortest_column() {
        let layout = masonry(&[100., 50., 50., 50.], 2);
        assert_eq!(layout, vec![vec![0, 3], vec![1, 2]]);
    }

    #[test]
    fn emoji_rows_lead_with_recent_and_filter_by_query() {
        let recent = vec!["\u{1F44D}".to_owned()];
        let rows = emoji_rows("", &recent);
        assert!(matches!(rows[0], EmojiRow::Header("Zuletzt verwendet")));
        assert!(matches!(&rows[1], EmojiRow::Cells(cells) if cells == &recent));
        let found = emoji_rows("thumbs", &recent);
        assert!(!found.is_empty());
        assert!(found.iter().all(|row| matches!(row, EmojiRow::Cells(_))));
    }

    #[test]
    fn sticker_rows_hold_four_and_search_spans_categories() {
        let popular = sticker_rows("", StickerChip::Popular);
        assert_eq!(popular.len(), 2);
        assert_eq!(popular[0].len(), STICKER_COLUMNS);
        let found = sticker_rows("coffee", StickerChip::Category("Clippy"));
        assert!(
            found
                .iter()
                .flatten()
                .any(|sticker| sticker.id == "Octocorn_Coffee")
        );
    }
}
