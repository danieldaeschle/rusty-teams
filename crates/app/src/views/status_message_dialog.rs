use chrono::Local;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    input::{InputEvent, Textarea, TextareaState},
    menu::{DropdownMenu as _, PopupMenuItem},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::StatusNote;

use super::widgets::symbol;
use crate::app_state::AppState;
use crate::own_status::{NOTE_DURATIONS, NOTE_LIMIT, NoteDuration, clamp_note};
use crate::theme;

const CARD_WIDTH: f32 = 440.;
const CARD_RADIUS: f32 = 12.;
const CARD_PADDING: f32 = 16.;
const CARD_MARGIN: f32 = 32.;
const SECTION_GAP: f32 = 12.;
const BACKDROP_OPACITY: f32 = 0.55;
const CLOSE_SIZE: f32 = 28.;
const BUTTON_HEIGHT: f32 = 32.;
const DISABLED_OPACITY: f32 = 0.4;
const FIELD_MIN_ROWS: usize = 3;
const FIELD_MAX_ROWS: usize = 6;
const EXPIRY_WIDTH: f32 = 160.;

pub enum StatusMessageDialogEvent {
    Close,
}

pub struct StatusMessageDialog {
    app: Entity<AppState>,
    input: Entity<TextareaState>,
    show_when_messaged: bool,
    expiry: NoteDuration,
    has_message: bool,
    length: usize,
    _subscription: Subscription,
}

impl EventEmitter<StatusMessageDialogEvent> for StatusMessageDialog {}

pub fn can_finish(length: usize) -> bool {
    length > 0
}

impl StatusMessageDialog {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let note = app.read(cx).own_status.note.clone();
        let text = note
            .as_ref()
            .map(|note| note.text.clone())
            .unwrap_or_default();
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(FIELD_MIN_ROWS, FIELD_MAX_ROWS)
                .placeholder("What's your status message?")
                .default_value(text.clone())
        });
        let subscription = cx.subscribe_in(&input, window, Self::on_input_event);
        input.update(cx, |input, cx| input.focus(window, cx));
        StatusMessageDialog {
            app,
            input,
            show_when_messaged: note.as_ref().is_some_and(|note| note.show_when_messaged),
            expiry: NoteDuration::DEFAULT,
            has_message: note.is_some(),
            length: text.trim().chars().count(),
            _subscription: subscription,
        }
    }

    fn on_input_event(
        &mut self,
        input: &Entity<TextareaState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        let value = input.read(cx).value().to_string();
        let clamped = clamp_note(&value);
        if clamped != value {
            input.update(cx, |input, cx| input.set_value(clamped.clone(), window, cx));
        }
        self.length = clamped.trim().chars().count();
        cx.notify();
    }

    fn finish(&mut self, cx: &mut Context<Self>) {
        let text = clamp_note(self.input.read(cx).value().trim());
        if !can_finish(text.chars().count()) {
            return;
        }
        let note = StatusNote {
            text,
            show_when_messaged: self.show_when_messaged,
            expires_at: self.expiry.expires_at(Local::now()),
        };
        self.app
            .update(cx, |state, cx| state.set_own_status_note(Some(note), cx));
        cx.emit(StatusMessageDialogEvent::Close);
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        self.app
            .update(cx, |state, cx| state.set_own_status_note(None, cx));
        cx.emit(StatusMessageDialogEvent::Close);
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => cx.emit(StatusMessageDialogEvent::Close),
            "enter" if event.keystroke.modifiers.control => self.finish(cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    fn header(&self, cx: &mut Context<Self>) -> Div {
        h_flex()
            .w_full()
            .items_center()
            .gap(px(SECTION_GAP))
            .child(
                div()
                    .flex_1()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Set status message"),
            )
            .child(
                div()
                    .id("status-message-close")
                    .size(px(CLOSE_SIZE))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .hover(|button| button.bg(theme::row_hover()))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(StatusMessageDialogEvent::Close)))
                    .child(symbol("close", 16., theme::text_muted())),
            )
    }

    fn field(&self) -> Div {
        v_flex()
            .w_full()
            .gap(px(4.))
            .child(
                div()
                    .w_full()
                    .px(px(10.))
                    .py(px(6.))
                    .rounded(px(6.))
                    .bg(theme::surface())
                    .border_1()
                    .border_color(theme::border())
                    .child(Textarea::new(&self.input).appearance(false).bordered(false)),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .justify_end()
                    .text_size(px(11.))
                    .text_color(theme::text_muted())
                    .child(format!("{}/{NOTE_LIMIT}", self.length)),
            )
    }

    fn expiry_row(&self, cx: &mut Context<Self>) -> Div {
        let dialog = cx.entity();
        let current = self.expiry;
        h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .gap(px(SECTION_GAP))
            .child(div().text_size(px(13.)).child("Clear status message after"))
            .child(
                Button::new("status-message-expiry")
                    .outline()
                    .w(px(EXPIRY_WIDTH))
                    .label(current.label())
                    .icon(IconName::ChevronDown)
                    .dropdown_menu(move |menu, _, _| {
                        NOTE_DURATIONS.iter().fold(menu, |menu, duration| {
                            let (dialog, duration) = (dialog.clone(), *duration);
                            menu.item(
                                PopupMenuItem::new(duration.label())
                                    .checked(duration == current)
                                    .on_click(move |_, _, cx| {
                                        dialog.update(cx, |this, cx| {
                                            this.expiry = duration;
                                            cx.notify();
                                        });
                                    }),
                            )
                        })
                    }),
            )
    }

    fn done_button(&self, enabled: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let button = h_flex()
            .id("status-message-done")
            .h(px(BUTTON_HEIGHT))
            .px(px(16.))
            .items_center()
            .rounded(px(6.))
            .bg(theme::accent())
            .text_color(theme::on_accent())
            .text_size(px(13.))
            .font_weight(FontWeight::SEMIBOLD)
            .child("Done");
        if enabled {
            button
                .cursor_pointer()
                .hover(|button| button.bg(theme::accent_text()))
                .on_click(cx.listener(|this, _, _, cx| this.finish(cx)))
        } else {
            button.opacity(DISABLED_OPACITY)
        }
    }
}

impl Render for StatusMessageDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let enabled = can_finish(self.length);
        div()
            .id("status-message-layer")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("status-message-backdrop")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(black().opacity(BACKDROP_OPACITY))
                    .occlude()
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(StatusMessageDialogEvent::Close))),
            )
            .child(
                v_flex()
                    .id("status-message-card")
                    .w(px(CARD_WIDTH))
                    .max_w(relative(1.))
                    .max_h(relative(1.))
                    .m(px(CARD_MARGIN))
                    .p(px(CARD_PADDING))
                    .gap(px(SECTION_GAP))
                    .rounded(px(CARD_RADIUS))
                    .bg(theme::background())
                    .border_1()
                    .border_color(theme::border_strong())
                    .text_color(theme::text())
                    .shadow_lg()
                    .occlude()
                    .capture_key_down(cx.listener(Self::on_key_down))
                    .child(self.header(cx))
                    .child(self.field())
                    .child(
                        Checkbox::new("status-message-pinned")
                            .label("Show when people message me")
                            .checked(self.show_when_messaged)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.show_when_messaged = *checked;
                                cx.notify();
                            })),
                    )
                    .child(self.expiry_row(cx))
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap(px(8.))
                            .when(self.has_message, |row| {
                                row.child(
                                    Button::new("status-message-clear")
                                        .ghost()
                                        .label("Clear message")
                                        .on_click(cx.listener(|this, _, _, cx| this.clear(cx))),
                                )
                            })
                            .child(div().flex_1())
                            .child(
                                Button::new("status-message-cancel")
                                    .ghost()
                                    .label("Cancel")
                                    .on_click(cx.listener(|_, _, _, cx| {
                                        cx.emit(StatusMessageDialogEvent::Close)
                                    })),
                            )
                            .child(self.done_button(enabled, cx)),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::can_finish;

    #[test]
    fn done_needs_at_least_one_character() {
        assert!(!can_finish(0));
        assert!(can_finish(1));
    }
}
