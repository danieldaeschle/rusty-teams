use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    switch::Switch,
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::avatar::person_avatar;
use super::call_extras::open_invite_picker;
use super::call_organizer::{pill, tile_menu};
use super::call_view::{BANNER_HEIGHT, CHAT_PANEL_WIDTH, MENU_WIDTH, round_control};
use super::widgets::{Corner, corner_badge, count_badge, icon};
use crate::app_state::AppState;
use crate::call::{
    ActiveCall, CallModel, OptionsDialog, PeopleSections, PersonRow, people_sections,
};
use crate::notice::truncated;
use crate::theme;
use calling::MeetingOptions;

const HEADER_HEIGHT: f32 = 48.;
const ROW_HEIGHT: f32 = 44.;
const AVATAR_SIZE: f32 = 28.;
const MORE_SIZE: f32 = 24.;
const SECTION_HEIGHT: f32 = 32.;
const NAME_LIMIT: usize = 26;
const STATE_ICON: f32 = 15.;
const DIALOG_WIDTH: f32 = 460.;
const DIALOG_BACKDROP: f32 = 0.55;
const FIELD_HEIGHT: f32 = 36.;
const DROPDOWN_WIDTH: f32 = 210.;

fn dark_text() -> Hsla {
    hsla(0., 0., 0.08, 1.)
}

pub fn people_button(app: &Entity<AppState>, open: bool, raised_hands: usize) -> Stateful<Div> {
    let app = app.clone();
    let (background, foreground) = if open {
        (theme::accent(), theme::on_accent())
    } else {
        (theme::surface_raised(), theme::text())
    };
    round_control("call-people", IconName::Users, background, foreground)
        .relative()
        .tooltip(move |window, cx| {
            Tooltip::new(if open { "Close the people list" } else { "Show people" }).build(window, cx)
        })
        .when(raised_hands > 0, |button| {
            button.child(
                corner_badge(Corner::TopRight, 4.).child(count_badge(raised_hands as u32, false)),
            )
        })
        .on_click(move |_, _, cx| app.update(cx, |state, cx| state.toggle_call_people(cx)))
}

pub fn lock_chip(model: &CallModel) -> Option<AnyElement> {
    model.capability.locked.then(|| {
        h_flex()
            .id("call-lock-chip")
            .h(px(24.))
            .px(px(8.))
            .gap(px(4.))
            .flex_none()
            .items_center()
            .rounded_full()
            .bg(theme::amber())
            .text_color(dark_text())
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(icon(IconName::Lock, 13., dark_text()))
            .child("Locked")
            .into_any_element()
    })
}

pub fn lock_banner(app: &Entity<AppState>, model: &CallModel) -> Option<AnyElement> {
    if !model.capability.locked || !model.organizes_meeting() {
        return None;
    }
    let app = app.clone();
    Some(
        h_flex()
            .id("call-lock-banner")
            .w_full()
            .h(px(BANNER_HEIGHT))
            .flex_none()
            .px(px(20.))
            .gap(px(12.))
            .items_center()
            .justify_center()
            .bg(theme::surface_raised())
            .text_color(theme::text())
            .text_size(px(13.))
            .font_weight(FontWeight::SEMIBOLD)
            .child("The meeting is locked. Nobody new can join.")
            .child(
                pill("call-lock-unlock", "Unlock", false)
                    .on_click(move |_, _, cx| app.update(cx, |state, cx| state.set_call_lock(false, cx))),
            )
            .into_any_element(),
    )
}

fn people_menu(popup: PopupMenu, app: &Entity<AppState>, model: &CallModel) -> PopupMenu {
    let popup = popup.min_w(px(MENU_WIDTH));
    if !model.organizes_meeting() {
        return popup;
    }
    let capability = model.capability;
    let (mute_app, lock_app, mic_app, camera_app, options_app) =
        (app.clone(), app.clone(), app.clone(), app.clone(), app.clone());
    popup
        .item(
            PopupMenuItem::new("Mute all")
                .icon(IconName::MicOff)
                .on_click(move |_, _, cx| mute_app.update(cx, |state, cx| state.mute_all_call(cx))),
        )
        .item(
            PopupMenuItem::new(if capability.locked { "Unlock meeting" } else { "Lock meeting" })
                .icon(if capability.locked { IconName::LockOpen } else { IconName::Lock })
                .on_click(move |_, _, cx| {
                    lock_app.update(cx, |state, cx| state.set_call_lock(!capability.locked, cx))
                }),
        )
        .item(
            PopupMenuItem::new(if capability.attendee_mic_off {
                "Allow mic for attendees"
            } else {
                "Disable mic for attendees"
            })
            .icon(IconName::MicOff)
            .on_click(move |_, _, cx| {
                mic_app.update(cx, |state, cx| {
                    state.set_call_attendee_restrictions(
                        !capability.attendee_mic_off,
                        capability.attendee_camera_off,
                        cx,
                    )
                })
            }),
        )
        .item(
            PopupMenuItem::new(if capability.attendee_camera_off {
                "Allow camera for attendees"
            } else {
                "Disable camera for attendees"
            })
            .icon(IconName::VideoOff)
            .on_click(move |_, _, cx| {
                camera_app.update(cx, |state, cx| {
                    state.set_call_attendee_restrictions(
                        capability.attendee_mic_off,
                        !capability.attendee_camera_off,
                        cx,
                    )
                })
            }),
        )
        .separator()
        .item(
            PopupMenuItem::new("Meeting options...")
                .icon(IconName::Settings)
                .on_click(move |_, _, cx| {
                    options_app.update(cx, |state, cx| state.open_meeting_options(cx))
                }),
        )
}

fn has_people_menu(model: &CallModel) -> bool {
    model.organizes_meeting()
}

fn panel_header(app: &Entity<AppState>, model: &CallModel, count: usize) -> Div {
    let (invite_app, menu_app, close_app) = (app.clone(), app.clone(), app.clone());
    let menu_model = model.clone();
    h_flex()
        .w_full()
        .h(px(HEADER_HEIGHT))
        .flex_none()
        .pl(px(20.))
        .pr(px(12.))
        .gap(px(8.))
        .items_center()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(15.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(format!("People ({count})")),
        )
        .when(model.can_invite(), |header| {
            header.child(
                Button::new("call-people-invite")
                    .label("Invite")
                    .icon(IconName::UserPlus)
                    .xsmall()
                    .on_click(move |_, window, cx| open_invite_picker(&invite_app, window, cx)),
            )
        })
        .when(has_people_menu(model), |header| {
            header.child(
                Button::new("call-people-more")
                    .icon(IconName::Ellipsis)
                    .ghost()
                    .xsmall()
                    .tooltip("More")
                    .dropdown_menu_with_anchor(Anchor::TopRight, move |popup, _, _| {
                        people_menu(popup, &menu_app, &menu_model)
                    }),
            )
        })
        .child(
            div()
                .id("call-people-close")
                .size(px(28.))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .rounded(px(7.))
                .cursor_pointer()
                .hover(|button| button.bg(theme::row_hover()))
                .tooltip(|window, cx| Tooltip::new("Close the people list").build(window, cx))
                .child(icon(IconName::Close, 14., theme::text_muted()))
                .on_click(move |_, _, cx| close_app.update(cx, |state, cx| state.toggle_call_people(cx))),
        )
}

fn section_header(title: String, action: Option<AnyElement>) -> Div {
    h_flex()
        .w_full()
        .h(px(SECTION_HEIGHT))
        .flex_none()
        .px(px(20.))
        .items_center()
        .justify_between()
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text_muted())
        .child(title)
        .children(action)
}

fn hand_badge(position: usize) -> Div {
    h_flex()
        .h(px(20.))
        .px(px(6.))
        .gap(px(3.))
        .flex_none()
        .items_center()
        .rounded_full()
        .bg(theme::amber())
        .text_color(dark_text())
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(icon(IconName::Hand, 11., dark_text()))
        .child(position.to_string())
}

fn mic_icon(row: &PersonRow) -> impl IntoElement {
    if row.mic_blocked {
        icon(IconName::MicOff, STATE_ICON, theme::red())
    } else if row.muted {
        icon(IconName::MicOff, STATE_ICON, theme::text_muted())
    } else {
        icon(IconName::Mic, STATE_ICON, theme::text())
    }
}

fn camera_icon(row: &PersonRow) -> impl IntoElement {
    if row.camera_blocked {
        icon(IconName::VideoOff, STATE_ICON, theme::red())
    } else if row.camera_on {
        icon(IconName::Video, STATE_ICON, theme::text())
    } else {
        icon(IconName::VideoOff, STATE_ICON, theme::text_muted())
    }
}

fn person_row(
    app: &Entity<AppState>,
    state: &AppState,
    model: &CallModel,
    section: &'static str,
    row: &PersonRow,
) -> AnyElement {
    let group = SharedString::from(format!("call-people-{section}-{}", row.mri));
    let user_id = row
        .user_id
        .clone()
        .or_else(|| {
            row.own
                .then(|| state.directory.me.as_ref().map(|me| me.user_id.clone()))
                .flatten()
        });
    let subtitle = if row.ringing { Some("Ringing") } else { row.label };
    let (more_app, more_model, more_mri, more_name) =
        (app.clone(), model.clone(), row.mri.clone(), row.name.clone());
    h_flex()
        .id(SharedString::from(format!("call-people-row-{section}-{}", row.mri)))
        .group(group.clone())
        .w_full()
        .h(px(ROW_HEIGHT))
        .flex_none()
        .px(px(20.))
        .gap(px(8.))
        .items_center()
        .hover(|row| row.bg(theme::row_hover()))
        .child(person_avatar(&state.directory, user_id.as_deref(), &row.name, AVATAR_SIZE))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(truncated(&row.name, NAME_LIMIT)),
                )
                .children(subtitle.map(|text| {
                    div()
                        .truncate()
                        .text_size(px(11.))
                        .text_color(theme::text_muted())
                        .child(text)
                })),
        )
        .children(row.hand.map(hand_badge))
        .child(mic_icon(row))
        .child(camera_icon(row))
        .child(
            div()
                .size(px(MORE_SIZE))
                .flex_none()
                .when(!row.own, |slot| {
                    slot.opacity(0.)
                        .group_hover(group, |slot| slot.opacity(1.))
                        .child(
                            Button::new(SharedString::from(format!(
                                "call-people-more-{section}-{}",
                                row.mri
                            )))
                            .icon(IconName::Ellipsis)
                            .ghost()
                            .xsmall()
                            .size(px(MORE_SIZE))
                            .tooltip("More")
                            .dropdown_menu_with_anchor(Anchor::TopRight, move |popup, _, _| {
                                tile_menu(popup, &more_app, &more_model, &more_mri, &more_name, false)
                            }),
                        )
                }),
        )
        .into_any_element()
}

fn lobby_section(app: &Entity<AppState>, state: &AppState, sections: &PeopleSections) -> Option<Div> {
    if sections.lobby.is_empty() {
        return None;
    }
    let admit_all_app = app.clone();
    let action = Button::new("call-people-admit-all")
        .label("Admit all")
        .ghost()
        .xsmall()
        .on_click(move |_, _, cx| admit_all_app.update(cx, |state, cx| state.admit_all_call_guests(cx)))
        .into_any_element();
    Some(
        v_flex()
            .w_full()
            .child(section_header(format!("Waiting in lobby ({})", sections.lobby.len()), Some(action)))
            .children(sections.lobby.iter().map(|guest| {
                let (admit_app, deny_app) = (app.clone(), app.clone());
                let (admit_mri, deny_mri) = (guest.mri.clone(), guest.mri.clone());
                h_flex()
                    .id(SharedString::from(format!("call-people-lobby-{}", guest.mri)))
                    .w_full()
                    .h(px(ROW_HEIGHT))
                    .flex_none()
                    .px(px(20.))
                    .gap(px(8.))
                    .items_center()
                    .child(person_avatar(&state.directory, guest.user_id.as_deref(), &guest.name, AVATAR_SIZE))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(truncated(&guest.name, NAME_LIMIT)),
                    )
                    .child(
                        Button::new(SharedString::from(format!("call-people-admit-{}", guest.mri)))
                            .label("Admit")
                            .xsmall()
                            .on_click(move |_, _, cx| {
                                admit_app.update(cx, |state, cx| state.admit_call_guest(admit_mri.clone(), cx))
                            }),
                    )
                    .child(
                        Button::new(SharedString::from(format!("call-people-deny-{}", guest.mri)))
                            .label("Deny")
                            .ghost()
                            .xsmall()
                            .on_click(move |_, _, cx| {
                                deny_app.update(cx, |state, cx| state.deny_call_guest(deny_mri.clone(), cx))
                            }),
                    )
            })),
    )
}

pub fn people_panel(app: &Entity<AppState>, state: &AppState, call: &ActiveCall) -> Option<AnyElement> {
    call.people_open.then_some(())?;
    let model = &call.model;
    let sections = people_sections(model);
    let lower_all = model.can_lower_hands().then(|| {
        let app = app.clone();
        Button::new("call-people-lower-all")
            .label("Lower all")
            .ghost()
            .xsmall()
            .on_click(move |_, _, cx| app.update(cx, |state, cx| state.lower_all_call_hands(cx)))
            .into_any_element()
    });
    let body = v_flex()
        .id("call-people-list")
        .flex_1()
        .min_h_0()
        .w_full()
        .overflow_y_scroll()
        .pb(px(12.))
        .children(model.can_admit().then(|| lobby_section(app, state, &sections)).flatten())
        .when(!sections.hands.is_empty(), |list| {
            list.child(section_header(format!("Raised hands ({})", sections.hands.len()), lower_all))
                .children(
                    sections
                        .hands
                        .iter()
                        .map(|row| person_row(app, state, model, "hands", row)),
                )
        })
        .children(sections.groups.iter().map(|group| {
            v_flex()
                .w_full()
                .child(section_header(format!("{} ({})", group.title, group.rows.len()), None))
                .children(group.rows.iter().map(|row| person_row(app, state, model, "group", row)))
        }));
    Some(
        v_flex()
            .id("call-people-panel")
            .debug_selector(|| "shell/islands/main/call-people".into())
            .w(px(CHAT_PANEL_WIDTH))
            .h_full()
            .flex_shrink(1.)
            .min_w_0()
            .rounded(px(10.))
            .overflow_hidden()
            .bg(theme::surface())
            .child(panel_header(app, model, sections.count))
            .child(body)
            .into_any_element(),
    )
}

fn choice_label(choices: &[(&str, &str)], value: &str) -> String {
    choices
        .iter()
        .find(|(wire, _)| *wire == value)
        .map_or_else(|| value.to_owned(), |(_, label)| (*label).to_owned())
}

fn choice_dropdown(
    id: &'static str,
    app: &Entity<AppState>,
    choices: &'static [(&'static str, &'static str)],
    current: &str,
    edit: fn(&mut MeetingOptions, &str),
) -> AnyElement {
    let app = app.clone();
    let current = current.to_owned();
    Button::new(id)
        .label(choice_label(choices, &current))
        .icon(IconName::ChevronDown)
        .ghost()
        .h(px(FIELD_HEIGHT))
        .dropdown_menu_with_anchor(Anchor::TopRight, move |popup, _, _| {
            let mut popup = popup.min_w(px(DROPDOWN_WIDTH));
            if !choices.iter().any(|(wire, _)| *wire == current) {
                popup = popup.item(PopupMenuItem::new(current.clone()).checked(true));
            }
            for (wire, label) in choices {
                let app = app.clone();
                popup = popup.item(PopupMenuItem::new(*label).checked(*wire == current).on_click(
                    move |_, _, cx| {
                        app.update(cx, |state, cx| {
                            state.edit_meeting_options(|options| edit(options, wire), cx)
                        })
                    },
                ));
            }
            popup
        })
        .into_any_element()
}

fn switch_field(
    id: &'static str,
    app: &Entity<AppState>,
    on: bool,
    edit: fn(&mut MeetingOptions, bool),
) -> AnyElement {
    let app = app.clone();
    Switch::new(id)
        .checked(on)
        .on_click(move |checked, _, cx| {
            let checked = *checked;
            app.update(cx, |state, cx| {
                state.edit_meeting_options(|options| edit(options, checked), cx)
            })
        })
        .into_any_element()
}

fn option_row(label: &'static str, control: AnyElement) -> Div {
    h_flex()
        .w_full()
        .min_h(px(FIELD_HEIGHT))
        .gap(px(12.))
        .items_center()
        .justify_between()
        .child(div().flex_1().min_w_0().text_size(px(14.)).child(label))
        .child(control)
}

fn options_fields(app: &Entity<AppState>, options: &MeetingOptions) -> Div {
    use calling::meeting_options::{BYPASS_CHOICES, CHAT_CHOICES, PRESENTER_CHOICES};
    v_flex()
        .w_full()
        .gap(px(6.))
        .child(option_row(
            "Who can bypass the lobby",
            choice_dropdown("options-bypass", app, &BYPASS_CHOICES, &options.bypass_lobby, |options, value| {
                options.bypass_lobby = value.to_owned()
            }),
        ))
        .child(option_row(
            "Who can present",
            choice_dropdown("options-present", app, &PRESENTER_CHOICES, &options.presenters, |options, value| {
                options.presenters = value.to_owned()
            }),
        ))
        .child(option_row(
            "Allow mic for attendees",
            switch_field("options-mic", app, options.attendee_mic, |options, on| options.attendee_mic = on),
        ))
        .child(option_row(
            "Allow camera for attendees",
            switch_field("options-camera", app, options.attendee_camera, |options, on| {
                options.attendee_camera = on
            }),
        ))
        .child(option_row(
            "Meeting chat",
            choice_dropdown("options-chat", app, &CHAT_CHOICES, &options.meeting_chat, |options, value| {
                options.meeting_chat = value.to_owned()
            }),
        ))
        .child(option_row(
            "Allow reactions",
            switch_field("options-reactions", app, options.reactions, |options, on| options.reactions = on),
        ))
        .child(option_row(
            "Record automatically",
            switch_field("options-record", app, options.record_automatically, |options, on| {
                options.record_automatically = on
            }),
        ))
}

fn dialog_body(app: &Entity<AppState>, dialog: &OptionsDialog) -> Div {
    let body = v_flex().w_full().gap(px(12.));
    let body = match &dialog.draft {
        Some(draft) => body.child(options_fields(app, draft)),
        None if dialog.is_loading() => body.child(
            div()
                .text_size(px(13.))
                .text_color(theme::text_muted())
                .child("Loading the meeting options..."),
        ),
        None => body,
    };
    body.children(dialog.error.as_ref().map(|error| {
        div()
            .id("options-error")
            .w_full()
            .text_size(px(13.))
            .text_color(theme::red())
            .child(error.clone())
    }))
}

pub fn options_dialog(app: &Entity<AppState>, call: &ActiveCall) -> Option<AnyElement> {
    let dialog = call.options_dialog.as_ref()?;
    let (close_app, cancel_app, save_app) = (app.clone(), app.clone(), app.clone());
    let can_save = dialog.can_save();
    let save = Button::new("options-save")
        .label(if dialog.saving { "Saving..." } else { "Save" })
        .when(!can_save, |button| button.opacity(0.4))
        .on_click(move |_, _, cx| {
            if can_save {
                save_app.update(cx, |state, cx| state.save_meeting_options(cx))
            }
        });
    Some(
        div()
            .id("options-layer")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("options-backdrop")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(black().opacity(DIALOG_BACKDROP))
                    .occlude()
                    .on_click(move |_, _, cx| close_app.update(cx, |state, cx| state.close_meeting_options(cx))),
            )
            .child(
                v_flex()
                    .id("options-card")
                    .w(px(DIALOG_WIDTH))
                    .max_w(relative(1.))
                    .p(px(16.))
                    .gap(px(12.))
                    .rounded(px(12.))
                    .bg(theme::background())
                    .border_1()
                    .border_color(theme::border_strong())
                    .text_color(theme::text())
                    .shadow_lg()
                    .occlude()
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Meeting options"),
                    )
                    .child(dialog_body(app, dialog))
                    .child(
                        h_flex()
                            .w_full()
                            .justify_end()
                            .gap(px(8.))
                            .child(
                                Button::new("options-cancel")
                                    .label("Cancel")
                                    .ghost()
                                    .on_click(move |_, _, cx| {
                                        cancel_app.update(cx, |state, cx| state.close_meeting_options(cx))
                                    }),
                            )
                            .child(save),
                    ),
            )
            .into_any_element(),
    )
}
