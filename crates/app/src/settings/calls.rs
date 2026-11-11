use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::rows::{Choice, caption, control_row, select, text_button};
use crate::notify::NotificationCenter;
use crate::notify::sounds::{Ringtone, RingtoneCallType};

pub fn render(center: &Entity<NotificationCenter>, cx: &App) -> Div {
    let ringtones = center.read(cx).settings().sounds.ringtones;
    let rows = RingtoneCallType::ALL
        .into_iter()
        .filter(|call_type| {
            *call_type == RingtoneCallType::Regular || ringtones.get(*call_type) != Ringtone::Ring
        })
        .map(|call_type| ringtone_row(center, call_type, ringtones.get(call_type)));
    v_flex().w_full().child(caption("RINGTONES")).children(rows)
}

fn ringtone_row(
    center: &Entity<NotificationCenter>,
    call_type: RingtoneCallType,
    current: Ringtone,
) -> Div {
    let choices = Ringtone::ALL
        .iter()
        .map(|ringtone| {
            let (center, ringtone) = (center.clone(), *ringtone);
            Choice::new(ringtone.label(), ringtone == current, move |cx| {
                center.update(cx, |center, cx| {
                    center.update_settings(cx, |settings| {
                        settings.sounds.ringtones.set(call_type, ringtone)
                    });
                    center.preview_sound(ringtone.file_name(), cx);
                })
            })
        })
        .collect();
    let (id, preview_id) = ringtone_ids(call_type);
    let preview_center = center.clone();
    control_row(
        call_type.label(),
        "",
        h_flex()
            .gap(px(8.))
            .items_center()
            .child(select(id, current.label(), false, choices))
            .child(
                text_button(preview_id, "Preview").on_click(move |_, _, cx| {
                    preview_center.update(cx, |center, cx| {
                        center.preview_sound(current.file_name(), cx)
                    })
                }),
            ),
    )
}

fn ringtone_ids(call_type: RingtoneCallType) -> (&'static str, &'static str) {
    match call_type {
        RingtoneCallType::Regular => ("ringtone-regular", "ringtone-regular-preview"),
        RingtoneCallType::CallGroup => ("ringtone-call-group", "ringtone-call-group-preview"),
        RingtoneCallType::CallQueue => ("ringtone-call-queue", "ringtone-call-queue-preview"),
        RingtoneCallType::Delegate => ("ringtone-delegate", "ringtone-delegate-preview"),
    }
}
