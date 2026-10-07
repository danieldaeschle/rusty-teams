use std::rc::Rc;

use gpui_kit::base::PopoverState;
use gpui_kit::component::{
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use gpui_kit::*;

use crate::app_state::AppState;
use crate::emoji;
use crate::theme;

pub const QUICK_REACTIONS: [&str; 4] = ["\u{1F44D}", "\u{2764}", "\u{1F602}", "\u{1F62E}"];
const PICKER_QUICK: [&str; 6] = [
    "\u{1F44D}",
    "\u{2764}",
    "\u{1F602}",
    "\u{1F62E}",
    "\u{1F622}",
    "\u{1F64F}",
];
const FALLBACK_RECENT: [&str; 16] = [
    "\u{1F389}",
    "\u{1F525}",
    "\u{1F440}",
    "\u{2705}",
    "\u{1F4AF}",
    "\u{1F680}",
    "\u{1F605}",
    "\u{1F914}",
    "\u{1F44F}",
    "\u{1F64C}",
    "\u{1F60A}",
    "\u{1F4A1}",
    "\u{1F4CC}",
    "\u{1F41E}",
    "\u{1F9EA}",
    "\u{2615}",
];
const PICKER_WIDTH: f32 = 296.;
const CELL_SIZE: f32 = 32.;
const GRID_COLUMNS: usize = 8;
const GRID_LIMIT: usize = 32;

pub type PickHandler = Rc<dyn Fn(&str, &mut Window, &mut App)>;

pub struct ReactionPicker {
    app: Entity<AppState>,
    input: Entity<InputState>,
    query: String,
    recent: emoji::Recent,
    target: Option<(PickHandler, WeakEntity<PopoverState>)>,
    _subscription: Subscription,
}

impl ReactionPicker {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search emoji"));
        let subscription = cx.subscribe_in(&input, window, Self::on_input_event);
        let recent = emoji::Recent::load(&app.read(cx).store);
        ReactionPicker {
            app,
            input,
            query: String::new(),
            recent,
            target: None,
            _subscription: subscription,
        }
    }

    pub fn set_target(&mut self, on_pick: PickHandler, popover: WeakEntity<PopoverState>) {
        self.target = Some((on_pick, popover));
    }

    pub fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.query.clear();
        self.input.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.focus(window, cx);
        });
        cx.notify();
    }

    fn on_input_event(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                self.query = input.read(cx).value().trim().to_owned();
                cx.notify();
            }
            InputEvent::PressEnter { .. } => {
                if let Some(first) = self.results().first().cloned() {
                    self.pick(&first, window, cx);
                }
            }
            _ => {}
        }
    }

    fn results(&self) -> Vec<String> {
        if self.query.is_empty() {
            let recent = self.recent.glyphs();
            let fill = FALLBACK_RECENT
                .iter()
                .map(|glyph| (*glyph).to_owned())
                .filter(|glyph| !recent.contains(glyph));
            return recent
                .iter()
                .cloned()
                .chain(fill)
                .take(GRID_LIMIT)
                .collect();
        }
        emoji::search(&self.query, self.recent.glyphs(), GRID_LIMIT)
            .into_iter()
            .map(|found| found.glyph.to_owned())
            .collect()
    }

    fn pick(&mut self, glyph: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.recent.push(glyph);
        self.recent.save(&self.app.read(cx).store);
        if let Some((on_pick, popover)) = self.target.take() {
            on_pick(glyph, window, cx);
            popover
                .update(cx, |state, cx| state.dismiss(window, cx))
                .ok();
        }
    }

    fn cell(&self, glyph: String, index: usize, cx: &mut Context<Self>) -> Stateful<Div> {
        let picked = glyph.clone();
        div()
            .id(("reaction-picker-cell", index))
            .size(px(CELL_SIZE))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .text_size(px(20.))
            .cursor_pointer()
            .hover(|cell| cell.bg(theme::border_strong()))
            .child(glyph)
            .on_click(cx.listener(move |this, _, window, cx| this.pick(&picked, window, cx)))
    }
}

impl Render for ReactionPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let results = self.results();
        let label = if self.query.is_empty() {
            "Recently used"
        } else if results.is_empty() {
            "No emoji found"
        } else {
            "Results"
        };
        let quick = PICKER_QUICK
            .iter()
            .enumerate()
            .map(|(index, glyph)| self.cell((*glyph).to_owned(), 1000 + index, cx))
            .collect::<Vec<_>>();
        let mut rows = Vec::new();
        let mut row = Vec::new();
        for (index, glyph) in results.into_iter().enumerate() {
            row.push(self.cell(glyph, index, cx));
            if row.len() == GRID_COLUMNS {
                rows.push(h_flex().gap(px(2.)).children(std::mem::take(&mut row)));
            }
        }
        if !row.is_empty() {
            rows.push(h_flex().gap(px(2.)).children(row));
        }
        v_flex()
            .w(px(PICKER_WIDTH))
            .gap(px(8.))
            .p(px(8.))
            .child(h_flex().gap(px(2.)).children(quick))
            .child(
                div()
                    .h(px(32.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .rounded(px(8.))
                    .bg(theme::surface())
                    .border_1()
                    .border_color(theme::border_strong())
                    .child(Input::new(&self.input).appearance(false).bordered(false)),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme::text_muted())
                    .child(label),
            )
            .child(v_flex().gap(px(2.)).children(rows))
    }
}
