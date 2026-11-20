use std::time::{Duration, Instant};

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::active_appearance;
use crate::app_state::AppState;
use crate::fuzzy;
use crate::i18n::tr;
use crate::theme::{self, ElevatedSurface as _};
use crate::views::widgets::symbol;

const PANEL_WIDTH: f32 = 360.;
const PANEL_GAP: f32 = 4.;
const PANEL_RADIUS: f32 = 10.;
const WINDOW_MARGIN: f32 = 8.;
/// A click on the button that opened the list closes it first, then arrives as a toggle.
const REOPEN_GUARD: Duration = Duration::from_millis(300);
const LIST_MAX_HEIGHT: f32 = 240.;
const ROW_HEIGHT: f32 = 32.;
const ROW_RADIUS: f32 = 6.;
const CHECK_SIZE: f32 = 16.;
const MAX_ROWS: usize = 200;

/// Which font the list is choosing.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FontTarget {
    Interface,
    Code,
}

#[derive(Clone)]
struct Item {
    /// The font family. Empty for the default font.
    family: SharedString,
    label: SharedString,
}

/// Searchable list of the fonts installed on this device.
pub struct FontPicker {
    app: Entity<AppState>,
    target: Option<FontTarget>,
    dismissed: Option<(FontTarget, Instant)>,
    items: Vec<Item>,
    highlighted: usize,
    search: Entity<InputState>,
    list_scroll: ScrollHandle,
    _subscription: Subscription,
}

impl FontPicker {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder(tr("settings.appearance.search")));
        let subscription = cx.subscribe_in(&search, window, Self::on_search_event);
        FontPicker {
            app,
            target: None,
            dismissed: None,
            items: Vec::new(),
            highlighted: 0,
            search,
            list_scroll: ScrollHandle::new(),
            _subscription: subscription,
        }
    }

    pub fn target(&self) -> Option<FontTarget> {
        self.target
    }

    fn font_items(cx: &App) -> Vec<Item> {
        let default = Item {
            family: SharedString::default(),
            label: tr("settings.appearance.default_font").into(),
        };
        let fonts = theme::installed_fonts(cx).into_iter().map(|family| Item {
            label: family.clone(),
            family,
        });
        std::iter::once(default).chain(fonts).collect()
    }

    pub fn toggle(&mut self, target: FontTarget, window: &mut Window, cx: &mut Context<Self>) {
        if self.target == Some(target) {
            self.close(cx);
            return;
        }
        let just_dismissed = self
            .dismissed
            .take()
            .is_some_and(|(dismissed, at)| dismissed == target && at.elapsed() < REOPEN_GUARD);
        if just_dismissed {
            return;
        }
        self.items = Self::font_items(cx);
        self.target = Some(target);
        self.highlighted = 0;
        self.search.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.target = None;
        cx.notify();
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.dismissed = self.target.map(|target| (target, Instant::now()));
        self.close(cx);
    }

    fn visible(&self, cx: &App) -> Vec<Item> {
        let query = self.search.read(cx).value();
        let items: Vec<(String, Item)> = self
            .items
            .iter()
            .map(|item| (item.label.to_string(), item.clone()))
            .collect();
        fuzzy::rank(&query, &items, MAX_ROWS)
    }

    fn on_search_event(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                self.highlighted = 0;
                cx.notify();
            }
            InputEvent::PressEnter { .. } => {
                if let Some(item) = self.visible(cx).get(self.highlighted) {
                    self.choose(item.family.clone(), cx);
                }
            }
            _ => {}
        }
    }

    fn choose(&mut self, family: SharedString, cx: &mut Context<Self>) {
        let Some(target) = self.target.take() else {
            return;
        };
        let store = self.app.read(cx).store.clone();
        let font = (!family.is_empty()).then_some(family);
        match target {
            FontTarget::Interface => active_appearance::set_ui_font(font, &store, cx),
            FontTarget::Code => active_appearance::set_code_font(font, &store, cx),
        }
        cx.notify();
    }

    fn move_highlight(&mut self, delta: isize, cx: &mut Context<Self>) {
        let count = self.visible(cx).len();
        if count == 0 {
            return;
        }
        let last = count as isize - 1;
        self.highlighted = (self.highlighted as isize + delta).clamp(0, last) as usize;
        self.list_scroll.scroll_to_item(self.highlighted);
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => {
                self.close(cx);
                cx.stop_propagation();
            }
            "down" => self.move_highlight(1, cx),
            "up" => self.move_highlight(-1, cx),
            _ => {}
        }
    }

    fn row(
        &self,
        index: usize,
        item: &Item,
        chosen: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let family = item.family.clone();
        h_flex()
            .id(("font-picker-row", index))
            .w_full()
            .h(px(ROW_HEIGHT))
            .flex_none()
            .px(px(8.))
            .gap(px(8.))
            .items_center()
            .rounded(px(ROW_RADIUS))
            .cursor_pointer()
            .when(index == self.highlighted, |row| row.bg(theme::row_hover()))
            .hover(|row| row.bg(theme::popup_hover()))
            .on_click(cx.listener(move |picker, _, _, cx| picker.choose(family.clone(), cx)))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(theme::text_label())
                    .text_color(theme::text())
                    .child(item.label.clone()),
            )
            .when(chosen, |row| {
                row.child(symbol("check", CHECK_SIZE, theme::accent_text()))
            })
    }
}

impl Render for FontPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(target) = self.target else {
            return div().into_any_element();
        };
        let current = active_appearance::current(cx);
        let chosen = match target {
            FontTarget::Interface => current.ui_font,
            FontTarget::Code => current.code_font,
        }
        .unwrap_or_default();
        let visible = self.visible(cx);
        let rows: Vec<_> = visible
            .iter()
            .enumerate()
            .map(|(index, item)| self.row(index, item, item.family == chosen, cx))
            .collect();
        let empty = rows
            .is_empty()
            .then(|| tr("settings.appearance.no_matching_fonts"));
        let panel = v_flex()
            .id("font-picker")
            .w(px(PANEL_WIDTH))
            .p(px(8.))
            .gap(px(6.))
            .rounded(px(PANEL_RADIUS))
            .flyout_surface()
            .occlude()
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down_out(cx.listener(|picker, _, _, cx| picker.dismiss(cx)))
            .child(Input::new(&self.search))
            .child(
                v_flex()
                    .id("font-picker-list")
                    .w_full()
                    .max_h(px(LIST_MAX_HEIGHT))
                    .overflow_y_scroll()
                    .track_scroll(&self.list_scroll)
                    .children(rows),
            )
            .children(empty.map(|text| {
                div()
                    .px(px(8.))
                    .py(px(4.))
                    .text_size(theme::text_label())
                    .text_color(theme::text_muted())
                    .child(text)
            }));
        deferred(
            anchored()
                .anchor(Anchor::TopLeft)
                .offset(point(px(0.), px(PANEL_GAP)))
                .snap_to_window_with_margin(px(WINDOW_MARGIN))
                .child(panel),
        )
        .with_priority(2)
        .into_any_element()
    }
}
