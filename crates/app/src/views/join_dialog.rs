use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::{Dialog, DialogFooter};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{WindowExt as _, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app_state::AppState;
use crate::theme;

const DIALOG_WIDTH: f32 = 380.;
const FIELD_GAP: f32 = 12.;
const LABEL_SIZE: f32 = 12.;
const TITLE: &str = "Join with a meeting ID";

struct JoinForm {
    state: Entity<AppState>,
    meeting_id: Entity<InputState>,
    passcode: Entity<InputState>,
    error: Option<String>,
    _subscriptions: [Subscription; 2],
}

impl JoinForm {
    fn new(state: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let meeting_id = cx.new(|cx| InputState::new(window, cx).placeholder("123 456 789 012"));
        let passcode = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Passcode (if the invitation has one)")
        });
        let subscriptions = [
            cx.subscribe_in(&meeting_id, window, Self::on_enter),
            cx.subscribe_in(&passcode, window, Self::on_enter),
        ];
        JoinForm {
            state,
            meeting_id,
            passcode,
            error: None,
            _subscriptions: subscriptions,
        }
    }

    fn on_enter(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::PressEnter { .. }) && self.submit(cx) {
            window.close_dialog(cx);
        }
    }

    fn submit(&mut self, cx: &mut Context<Self>) -> bool {
        let meeting_id = self.meeting_id.read(cx).value().to_string();
        let passcode = self.passcode.read(cx).value().to_string();
        let joined = self.state.update(cx, |state, cx| {
            state.join_meeting_by_id(&meeting_id, &passcode, cx)
        });
        self.error = joined.as_ref().err().cloned();
        cx.notify();
        joined.is_ok()
    }
}

fn labeled(label: &str, field: impl IntoElement) -> Div {
    v_flex()
        .gap(px(4.))
        .child(
            div()
                .text_size(px(LABEL_SIZE))
                .text_color(theme::text_muted())
                .child(label.to_owned()),
        )
        .child(field)
}

impl Render for JoinForm {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap(px(FIELD_GAP))
            .child(labeled("Meeting ID", Input::new(&self.meeting_id)))
            .child(labeled("Passcode", Input::new(&self.passcode)))
            .when_some(self.error.clone(), |form, error| {
                form.child(
                    div()
                        .text_size(px(LABEL_SIZE))
                        .text_color(theme::red_soft())
                        .child(error),
                )
            })
    }
}

pub fn open_join_with_id(state: Entity<AppState>, window: &mut Window, cx: &mut App) {
    let form = cx.new(|cx| JoinForm::new(state, window, cx));
    let first_field = form.read(cx).meeting_id.clone();
    window.open_dialog(cx, move |dialog: Dialog, _, _| {
        let submit_form = form.clone();
        let footer = DialogFooter::new()
            .child(
                Button::new("join-cancel")
                    .label("Cancel")
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .child(
                Button::new("join-confirm")
                    .primary()
                    .label("Join")
                    .on_click(move |_, window, cx| {
                        if submit_form.update(cx, |form, cx| form.submit(cx)) {
                            window.close_dialog(cx);
                        }
                    }),
            );
        dialog
            .title(TITLE)
            .w(px(DIALOG_WIDTH))
            .child(form.clone())
            .footer(footer)
    });
    window.defer(cx, move |window, cx| {
        first_field.update(cx, |input, cx| input.focus(window, cx));
    });
}
