use chatsvc::Repeat;
use chrono::{Local, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::date_picker::DatePicker;
use gpui_kit::component::date_picker::DatePickerState;
use gpui_kit::component::dialog::{Dialog, DialogFooter};
use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::time_field::{TimeField, TimeFieldState};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::new_chat::{NewChatDraft, Pick};
use super::widgets::icon;
use crate::app_state::AppState;
use crate::call::{FormValues, Invitee, default_window, draft_from_form};
use crate::theme;

const DIALOG_WIDTH: f32 = 520.;
const FIELD_GAP: f32 = 12.;
const LABEL_SIZE: f32 = 12.;
const TITLE: &str = "New meeting";
const DATE_FORMAT: &str = "%a, %b %-d, %Y";
const DETAILS_MIN_ROWS: usize = 3;
const DETAILS_MAX_ROWS: usize = 6;
const TIME_FIELD_WIDTH: f32 = 96.;
const DATE_FIELD_WIDTH: f32 = 176.;
const REPEAT_WIDTH: f32 = 180.;

struct MeetingForm {
    state: Entity<AppState>,
    title: Entity<InputState>,
    people: Entity<NewChatDraft>,
    date: Entity<DatePickerState>,
    start: Entity<TimeFieldState>,
    end: Entity<TimeFieldState>,
    details: Entity<TextareaState>,
    repeat: Repeat,
    error: Option<String>,
    sending: bool,
}

impl MeetingForm {
    fn new(state: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (start_moment, end_moment) = default_window(Utc::now(), &Local);
        let title = cx.new(|cx| InputState::new(window, cx).placeholder("Add a title"));
        let people_state = state.clone();
        let people = cx.new(|cx| NewChatDraft::new(people_state, window, cx).compact());
        let date = cx.new(|cx| {
            let mut picker = DatePickerState::new(window, cx).date_format(DATE_FORMAT);
            picker.set_date(start_moment.date(), window, cx);
            picker
        });
        let start = cx.new(|cx| {
            let mut field = TimeFieldState::new(window, cx);
            field.set_time(start_moment.time(), window, cx);
            field
        });
        let end = cx.new(|cx| {
            let mut field = TimeFieldState::new(window, cx);
            field.set_time(end_moment.time(), window, cx);
            field
        });
        let details = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(DETAILS_MIN_ROWS, DETAILS_MAX_ROWS)
                .placeholder("Add details")
        });
        MeetingForm {
            state,
            title,
            people,
            date,
            start,
            end,
            details,
            repeat: Repeat::Never,
            error: None,
            sending: false,
        }
    }

    fn set_repeat(&mut self, repeat: Repeat, cx: &mut Context<Self>) {
        self.repeat = repeat;
        cx.notify();
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sending {
            return;
        }
        let people = self
            .people
            .read(cx)
            .chips()
            .iter()
            .map(|pick| Invitee {
                user_id: pick.user_id.clone(),
                name: pick.name.clone(),
                mail: pick.mail.clone(),
            })
            .collect();
        let values = FormValues {
            subject: self.title.read(cx).value().to_string(),
            date: self.date.read(cx).date().start(),
            start: self.start.read(cx).time(),
            end: self.end.read(cx).time(),
            repeat: self.repeat,
            details: self.details.read(cx).value().to_string(),
            people,
        };
        let time_zone_name = self.state.read(cx).own_time_zone.clone();
        let draft = match draft_from_form(values, &Local, time_zone_name) {
            Ok(draft) => draft,
            Err(message) => {
                self.error = Some(message);
                cx.notify();
                return;
            }
        };
        self.error = None;
        self.sending = true;
        let created = self
            .state
            .update(cx, |state, cx| state.create_meeting(draft, cx));
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let outcome = created
                .await
                .unwrap_or_else(|_| Err("The request was cancelled.".to_owned()));
            this.update_in(cx, |form, window, cx| match outcome {
                Ok(()) => window.close_dialog(cx),
                Err(message) => {
                    form.sending = false;
                    form.error = Some(message);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
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

impl Render for MeetingForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let form = cx.entity();
        let current = self.repeat;
        let repeat_button = Button::new("meeting-repeat")
            .outline()
            .w(px(REPEAT_WIDTH))
            .label(current.label())
            .icon(IconName::ChevronDown)
            .dropdown_menu(move |menu, _, _| {
                Repeat::ALL.into_iter().fold(menu, |menu, repeat| {
                    let form = form.clone();
                    menu.item(
                        PopupMenuItem::new(repeat.label())
                            .checked(repeat == current)
                            .on_click(move |_, _, cx| {
                                form.update(cx, |form, cx| form.set_repeat(repeat, cx))
                            }),
                    )
                })
            })
            .listbox();
        v_flex()
            .debug_selector(|| "new-meeting".into())
            .gap(px(FIELD_GAP))
            .child(labeled("Title", Input::new(&self.title)))
            .child(labeled(
                "People",
                div()
                    .debug_selector(|| "new-meeting/people".into())
                    .rounded(px(6.))
                    .border_1()
                    .border_color(theme::border())
                    .child(self.people.clone()),
            ))
            .child(
                h_flex()
                    .debug_selector(|| "new-meeting/when".into())
                    .gap(px(8.))
                    .items_end()
                    .child(div().w(px(DATE_FIELD_WIDTH)).child(labeled(
                        "Date",
                        DatePicker::new(&self.date).cleanable(false),
                    )))
                    .child(
                        div()
                            .w(px(TIME_FIELD_WIDTH))
                            .child(labeled("Start", TimeField::new(&self.start))),
                    )
                    .child(
                        div()
                            .w(px(TIME_FIELD_WIDTH))
                            .child(labeled("End", TimeField::new(&self.end))),
                    ),
            )
            .child(labeled("Repeat", repeat_button))
            .child(labeled("Details", Textarea::new(&self.details)))
            .child(
                h_flex()
                    .gap(px(6.))
                    .items_center()
                    .text_size(px(LABEL_SIZE))
                    .text_color(theme::text_muted())
                    .child(icon(IconName::Video, 14., theme::text_muted()))
                    .child("Teams meeting link is added"),
            )
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

pub fn open_new_meeting(state: Entity<AppState>, window: &mut Window, cx: &mut App) {
    open_new_meeting_with(state, Vec::new(), window, cx);
}

pub fn open_new_meeting_with(
    state: Entity<AppState>,
    people: Vec<Pick>,
    window: &mut Window,
    cx: &mut App,
) {
    let form = cx.new(|cx| MeetingForm::new(state, window, cx));
    if !people.is_empty() {
        let draft = form.read(cx).people.clone();
        draft.update(cx, |draft, cx| draft.set_chips(people, cx));
    }
    let first_field = form.read(cx).title.clone();
    window.open_dialog(cx, move |dialog: Dialog, _, cx| {
        let submit_form = form.clone();
        let sending = form.read(cx).sending;
        let footer = DialogFooter::new()
            .child(
                Button::new("meeting-cancel")
                    .label("Cancel")
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .child(
                Button::new("meeting-send")
                    .primary()
                    .label(if sending { "Sending..." } else { "Send" })
                    .on_click(move |_, window, cx| {
                        submit_form.update(cx, |form, cx| form.submit(window, cx));
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
