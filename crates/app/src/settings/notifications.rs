use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::rows::{Choice, caption, control_row, select, text_button, toggle_row};
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

const APPEARANCE: [Toggle; 2] = [
    Toggle {
        id: "settings-preview",
        title: "Show message preview",
        detail: "Off: only \"New message\"",
        field: |settings| &mut settings.preview,
    },
    Toggle {
        id: "settings-flash",
        title: "Flash taskbar when notifications are received",
        detail: "On new messages, until you open the window",
        field: |settings| &mut settings.flash,
    },
];

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

const MENTIONS_ONLY: Toggle = Toggle {
    id: "settings-mentions-only",
    title: "Mentions only",
    detail: "Direct messages and @mentions",
    field: |settings| &mut settings.mentions_only,
};

const HIDE_IN_CALLS: Toggle = Toggle {
    id: "settings-hide-in-calls",
    title: "Hide notifications during calls and meetings",
    detail: "No message toasts while a call is active",
    field: |settings| &mut settings.mute_in_calls,
};

pub fn render(center: &Entity<NotificationCenter>, cx: &App) -> Div {
    let mut current = center.read(cx).settings().clone();
    let corner = current.corner;
    let sounds = current.sounds.clone();
    let mut toggle = |toggle: &Toggle| {
        let on = *(toggle.field)(&mut current);
        toggle_element(center, toggle, on)
    };
    let appearance: Vec<Stateful<Div>> = APPEARANCE.iter().map(&mut toggle).collect();
    let notification_sound = toggle(&NOTIFICATION_SOUND);
    let urgent_sound = toggle(&URGENT_SOUND);
    let call_sounds: Vec<Stateful<Div>> = CALL_SOUNDS.iter().map(&mut toggle).collect();
    let mentions_only = toggle(&MENTIONS_ONLY);
    let hide_in_calls = toggle(&HIDE_IN_CALLS);
    let test_center = center.clone();
    v_flex()
        .w_full()
        .child(caption("APPEARANCE AND SOUND"))
        .children(appearance)
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
        .child(caption("WHAT YOU GET NOTIFIED FOR"))
        .child(mentions_only)
        .child(caption("MEETINGS AND CALLS"))
        .child(hide_in_calls)
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
