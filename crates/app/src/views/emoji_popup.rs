use std::ops::Range;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::widgets::symbol;
use crate::emoji::Match;
use crate::theme;

pub const LIMIT: usize = 40;
const WIDTH: f32 = 300.;
const ROW_HEIGHT: f32 = 32.;
const VISIBLE_ROWS: usize = 7;
const HINT_HEIGHT: f32 = 28.;
const ALIAS_MAX_WIDTH: f32 = 120.;

pub struct EmojiPopup {
    pub range: Range<usize>,
    pub query: String,
    pub matches: Vec<Match>,
    pub highlighted: usize,
    scroll: ScrollHandle,
}

impl EmojiPopup {
    pub fn new(range: Range<usize>, query: String, matches: Vec<Match>) -> Self {
        EmojiPopup {
            range,
            query,
            matches,
            highlighted: 0,
            scroll: ScrollHandle::new(),
        }
    }

    pub fn move_highlight(&mut self, delta: isize) {
        let count = self.matches.len() as isize;
        if count > 0 {
            self.highlighted = (self.highlighted as isize + delta).rem_euclid(count) as usize;
            self.scroll.scroll_to_item(self.highlighted);
        }
    }

    pub fn selected(&self) -> Option<&Match> {
        self.matches.get(self.highlighted)
    }
}

fn highlighted_text(text: String, highlight: Range<usize>, size: f32, color: Hsla) -> Div {
    let marks = text
        .get(highlight.clone())
        .filter(|marked| !marked.is_empty())
        .map(|_| {
            (
                highlight,
                HighlightStyle {
                    color: Some(theme::accent_text()),
                    font_weight: Some(FontWeight::BOLD),
                    ..Default::default()
                },
            )
        });
    div()
        .min_w_0()
        .truncate()
        .text_size(px(size))
        .text_color(color)
        .child(StyledText::new(text).with_highlights(marks))
}

fn row(index: usize, found: &Match, selected: bool) -> Stateful<Div> {
    let code = format!(":{}:", found.code);
    let code_highlight = if found.alias.is_none() {
        found.highlight.start + 1..found.highlight.end + 1
    } else {
        0..0
    };
    h_flex()
        .id(ElementId::Name(format!("emoji-row-{index}").into()))
        .relative()
        .flex_none()
        .h(px(ROW_HEIGHT))
        .pl(px(8.))
        .pr(px(10.))
        .gap(px(8.))
        .items_center()
        .rounded(px(6.))
        .cursor_pointer()
        .when(selected, |row| {
            row.bg(theme::surface_raised()).child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .bottom_0()
                    .w(px(3.))
                    .rounded_l(px(6.))
                    .bg(theme::accent()),
            )
        })
        .when(!selected, |row| row.hover(|row| row.bg(theme::row_hover())))
        .child(
            div()
                .w(px(28.))
                .flex_none()
                .text_center()
                .text_size(px(20.))
                .line_height(px(ROW_HEIGHT))
                .child(found.glyph),
        )
        .child(highlighted_text(code, code_highlight, 13., theme::text()))
        .when(found.recent, |row| {
            row.child(symbol("schedule", 12., theme::text_muted()))
        })
        .child(div().flex_1())
        .when_some(found.alias, |row, alias| {
            row.child(
                highlighted_text(
                    alias.to_owned(),
                    found.highlight.clone(),
                    11.5,
                    theme::text_muted(),
                )
                .flex_none()
                .max_w(px(ALIAS_MAX_WIDTH)),
            )
        })
}

fn key(label: &'static str) -> Div {
    div()
        .px(px(5.))
        .py(px(1.))
        .rounded(px(4.))
        .border_1()
        .border_color(theme::border_strong())
        .bg(theme::surface_raised())
        .text_size(px(10.))
        .text_color(theme::text())
        .child(label)
}

fn hint() -> Div {
    h_flex()
        .h(px(HINT_HEIGHT))
        .mt(px(4.))
        .px(px(8.))
        .gap(px(6.))
        .items_center()
        .border_t_1()
        .border_color(theme::border())
        .text_size(px(11.))
        .text_color(theme::text_muted())
        .child(key("↑↓"))
        .child("wählen")
        .child(key("Tab"))
        .child(key("Enter"))
        .child("einfügen")
        .child(key("Esc"))
        .child("zu")
}

/// Bottom-left corner sits above `anchor`, the colon's top-left in window coordinates.
pub fn render(
    popup: &EmojiPopup,
    anchor: Option<Point<Pixels>>,
    on_pick: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let on_pick = std::rc::Rc::new(on_pick);
    let rows = popup.matches.iter().enumerate().map(|(index, found)| {
        let on_pick = on_pick.clone();
        row(index, found, index == popup.highlighted)
            .on_click(move |_, window, cx| on_pick(index, window, cx))
    });
    let visible = popup.matches.len().min(VISIBLE_ROWS) as f32;
    let panel = v_flex()
        .id("emoji-popup")
        .w(px(WIDTH))
        .p(px(4.))
        .rounded(px(10.))
        .border_1()
        .border_color(theme::border_strong())
        .bg(theme::surface())
        .shadow_lg()
        .occlude()
        .child(
            v_flex()
                .id("emoji-rows")
                .max_h(px(ROW_HEIGHT * visible))
                .overflow_y_scroll()
                .track_scroll(&popup.scroll)
                .children(rows),
        )
        .child(hint());
    match anchor {
        Some(anchor) => deferred(
            anchored()
                .position(anchor - point(px(12.), px(6.)))
                .anchor(Anchor::BottomLeft)
                .snap_to_window_with_margin(px(8.))
                .child(panel),
        )
        .into_any_element(),
        None => div()
            .absolute()
            .bottom(relative(1.))
            .left_0()
            .mb(px(6.))
            .child(panel)
            .into_any_element(),
    }
}
