use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use teams_core::ChatSettingChange;

use super::account::{
    ACTIVITY_OR_OFF, CHAT_MESSAGES, MEETING_CHATS, MENTIONS, PRESETS, account_select,
    meeting_chats_detail, preset_detail,
};
use super::rows::{Choice, caption, control_row, select, text_button, toggle_row};
use crate::app_state::AppState;
use crate::notify::NotificationCenter;
use crate::notify::rules::{Corner, Settings};
use crate::notify::sounds::AlertSound;
use crate::theme;

type Accessor = fn(&mut Settings) -> &mut bool;
type SoundAccessor = fn(&mut Settings) -> &mut AlertSound;

struct Toggle {
    id: &'static str,
    title: &'static str,
    detail: &'static str,
    field: Accessor,
}

const FLASH: Toggle = Toggle {
    id: "settings-flash",
    title: "Flash taskbar when notifications are received",
    detail: "On new messages, until you open the window",
    field: |settings| &mut settings.flash,
};

const NOTIFICATION_SOUND: Toggle = Toggle {
    id: "settings-notification-sound",
    title: "Play sounds with notifications",
    detail: "",
    field: |settings| &mut settings.sounds.notification_sound,
};

const URGENT_SOUND: Toggle = Toggle {
    id: "settings-urgent-sound",
    title: "Play sounds with urgent and priority contact notifications",
    detail: "",
    field: |settings| &mut settings.sounds.urgent_sound,
};

const CALL_SOUNDS: [Toggle; 2] = [
    Toggle {
        id: "settings-call-sound",
        title: "Play sounds with incoming calls, requests to join, and meeting start notifications",
        detail: "",
        field: |settings| &mut settings.sounds.call_and_meeting_sound,
    },
    Toggle {
        id: "settings-busy-sound",
        title: "Play sounds when I'm busy or in a meeting",
        detail: "",
        field: |settings| &mut settings.sounds.sound_when_busy,
    },
];

const HIDE_IN_CALLS: Toggle = Toggle {
    id: "settings-hide-in-calls",
    title: "Hide notifications during calls and meetings",
    detail: "No message toasts while a call is active",
    field: |settings| &mut settings.mute_in_calls,
};

pub fn render(center: &Entity<NotificationCenter>, app: &Entity<AppState>, cx: &App) -> Div {
    let account = app.read(cx).account_chat_settings().notifications;
    let mut current = center.read(cx).settings().clone();
    let corner = current.corner;
    let sounds = current.sounds.clone();
    let mut toggle = |toggle: &Toggle| {
        let on = *(toggle.field)(&mut current);
        toggle_element(center, toggle, on)
    };
    let flash = toggle(&FLASH);
    let notification_sound = toggle(&NOTIFICATION_SOUND);
    let urgent_sound = toggle(&URGENT_SOUND);
    let call_sounds: Vec<Stateful<Div>> = CALL_SOUNDS.iter().map(&mut toggle).collect();
    let hide_in_calls = toggle(&HIDE_IN_CALLS);
    let test_center = center.clone();
    v_flex()
        .w_full()
        .child(caption("APPEARANCE AND SOUND"))
        .child(preview_toggle(app, account.show_preview))
        .child(flash)
        .child(control_row(
            "Position on the screen",
            "Corner of the main screen",
            select(
                "settings-position",
                corner.label(),
                false,
                corner_choices(center, corner),
            ),
        ))
        .child(control_row(
            "Test notification",
            "Shows a toast with the settings above",
            text_button("settings-test-notification", "Send test notification").on_click(
                move |_, _, cx| {
                    test_center.update(cx, |center, cx| center.send_test_notification(cx))
                },
            ),
        ))
        .child(caption("SOUND"))
        .child(notification_sound)
        .child(sound_row(
            center,
            (
                "settings-notification-sound-type",
                "settings-notification-sound-preview",
            ),
            "Notification sound",
            &AlertSound::NORMAL,
            sounds.notification_sound_type,
            sounds.notification_sound,
            |settings| &mut settings.sounds.notification_sound_type,
        ))
        .child(urgent_sound)
        .child(sound_row(
            center,
            (
                "settings-urgent-sound-type",
                "settings-urgent-sound-preview",
            ),
            "Urgent sound",
            &AlertSound::URGENT,
            sounds.urgent_sound_type,
            sounds.urgent_sound,
            |settings| &mut settings.sounds.urgent_sound_type,
        ))
        .children(call_sounds)
        .child(caption("TEAMS AND CHANNELS"))
        .child(account_select(
            app,
            (
                "settings-notification-preset",
                "Teams and channels",
                preset_detail(account.preset),
            ),
            &PRESETS,
            account.preset,
            ChatSettingChange::Preset,
        ))
        .child(caption("CHAT"))
        .child(account_select(
            app,
            ("settings-chat-messages", "Chat message notifications", ""),
            &CHAT_MESSAGES,
            account.chat_messages,
            ChatSettingChange::ChatMessages,
        ))
        .child(account_select(
            app,
            ("settings-personal-mentions", "Personal @mentions", ""),
            &MENTIONS,
            account.personal_mentions,
            ChatSettingChange::PersonalMentions,
        ))
        .child(account_select(
            app,
            (
                "settings-everyone-mentions",
                "Everyone in chat @mentions",
                "",
            ),
            &ACTIVITY_OR_OFF,
            account.everyone_mentions,
            ChatSettingChange::EveryoneMentions,
        ))
        .child(account_select(
            app,
            ("settings-likes", "Likes and reactions", ""),
            &ACTIVITY_OR_OFF,
            account.likes,
            ChatSettingChange::Likes,
        ))
        .child(caption("MEETINGS AND CALLS"))
        .child(account_select(
            app,
            (
                "settings-meeting-chats",
                "Meeting chat notifications",
                meeting_chats_detail(account.meeting_chats),
            ),
            &MEETING_CHATS,
            account.meeting_chats,
            ChatSettingChange::MeetingChats,
        ))
        .child(hide_in_calls)
}

fn preview_toggle(app: &Entity<AppState>, on: bool) -> Stateful<Div> {
    let app = app.clone();
    toggle_row(
        "settings-preview",
        "Show message preview",
        "Off: only \"New message\"",
        theme::text_muted(),
        on,
    )
    .on_click(move |_, _, cx| {
        app.update(cx, |state, cx| {
            state.save_chat_setting(ChatSettingChange::ShowPreview(!on), cx)
        })
    })
}

fn toggle_element(center: &Entity<NotificationCenter>, toggle: &Toggle, on: bool) -> Stateful<Div> {
    let center = center.clone();
    let field = toggle.field;
    toggle_row(
        toggle.id,
        toggle.title,
        toggle.detail,
        theme::text_muted(),
        on,
    )
    .on_click(move |_, _, cx| {
        center.update(cx, |center, cx| {
            center.update_settings(cx, |settings| {
                let value = field(settings);
                *value = !*value;
            })
        })
    })
}

fn corner_choices(center: &Entity<NotificationCenter>, current: Corner) -> Vec<Choice> {
    Corner::ALL
        .into_iter()
        .map(|corner| {
            let center = center.clone();
            Choice::new(corner.label(), corner == current, move |cx| {
                center.update(cx, |center, cx| {
                    center.update_settings(cx, |settings| settings.corner = corner)
                })
            })
        })
        .collect()
}

fn sound_row(
    center: &Entity<NotificationCenter>,
    (id, preview_id): (&'static str, &'static str),
    title: &'static str,
    options: &'static [AlertSound],
    current: AlertSound,
    enabled: bool,
    field: SoundAccessor,
) -> Div {
    let choices = options
        .iter()
        .map(|sound| {
            let (center, sound) = (center.clone(), *sound);
            Choice::new(sound.label(), sound == current, move |cx| {
                center.update(cx, |center, cx| {
                    center.update_settings(cx, |settings| *field(settings) = sound);
                    center.preview_sound(sound.file_name(), cx);
                })
            })
        })
        .collect();
    let preview_center = center.clone();
    control_row(
        title,
        "",
        h_flex()
            .gap(px(8.))
            .items_center()
            .child(select(id, current.label(), !enabled, choices))
            .child(
                text_button(preview_id, "Preview").on_click(move |_, _, cx| {
                    preview_center.update(cx, |center, cx| {
                        center.preview_sound(current.file_name(), cx)
                    })
                }),
            ),
    )
}
