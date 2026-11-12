use chatsvc::{CalendarEvent, ResponseType, join_info_text};
use chrono::{Local, Offset};
use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::{Dialog, DialogFooter};
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::*;

use super::rsvp_menu::rsvp_menu;
use crate::i18n::tr;
use crate::app_state::AppState;
use crate::call::week_grid::is_effective_all_day;
use crate::theme;

const DIALOG_WIDTH: f32 = 440.;
const FIELD_GAP: f32 = 12.;
const LABEL_SIZE: f32 = 12.;
const COPY_JOIN_INFO: &str = "Copy join info";

fn when_label(event: &CalendarEvent) -> String {
    let offset = Local::now().offset().fix();
    if is_effective_all_day(event, offset) {
        let start = event.start.with_timezone(&offset).date_naive();
        let end = event.end.with_timezone(&offset).date_naive();
        if end > start + chrono::Duration::days(1) {
            return format!(
                "All day · {} – {}",
                start.format("%a, %b %-d"),
                (end - chrono::Duration::days(1)).format("%a, %b %-d, %Y")
            );
        }
        return format!("All day · {}", start.format("%a, %b %-d, %Y"));
    }
    let start = event.start.with_timezone(&offset);
    let end = event.end.with_timezone(&offset);
    if start.date_naive() == end.date_naive() {
        format!(
            "{} · {} – {}",
            start.format("%a, %b %-d, %Y"),
            start.format(app_base::format::clock_format()),
            end.format(app_base::format::clock_format())
        )
    } else {
        format!(
            "{} {} – {} {}",
            start.format("%a, %b %-d"),
            start.format(app_base::format::clock_format()),
            end.format("%a, %b %-d, %Y"),
            end.format(app_base::format::clock_format())
        )
    }
}

fn source_label(event: &CalendarEvent) -> String {
    let mut sources = event.sources.clone();
    if sources.is_empty() {
        sources.push(event.source);
    }
    sources.sort_by_key(|source| source.label());
    sources.dedup();
    sources
        .iter()
        .map(|source| source.label())
        .collect::<Vec<_>>()
        .join(" + ")
}

fn detail_row(label: &str, value: impl Into<SharedString>) -> Div {
    v_flex()
        .gap(px(2.))
        .child(
            div()
                .text_size(px(LABEL_SIZE))
                .text_color(theme::text_muted())
                .child(label.to_owned()),
        )
        .child(
            div()
                .text_size(px(14.))
                .text_color(theme::text())
                .child(value.into()),
        )
}

pub fn open_calendar_event(
    state: Entity<AppState>,
    event: CalendarEvent,
    window: &mut Window,
    cx: &mut App,
) {
    let send_response = state.read(cx).rsvp_send_response;
    // Show Join whenever a Teams link exists — not only inside the join window.
    let can_join = event.online && !event.cancelled && event.join_url.is_some();
    let subject = if event.cancelled {
        format!("Canceled: {}", event.subject)
    } else if event.subject.is_empty() {
        "(No title)".to_owned()
    } else {
        event.subject.clone()
    };
    let when = when_label(&event);
    let source = source_label(&event);
    let organizer = event
        .organizer_name
        .clone()
        .or_else(|| event.organizer_address.clone());
    let join_url = event.join_url.clone();
    let dial_in = event.dial_in.clone();
    let thread_id = event.thread_id.clone();
    let can_rsvp = event.response != ResponseType::Organizer && event.organizer_name.is_some();
    let response = event.response;
    let event_key = event.object_id.clone();
    let meeting_title = event.subject.clone();
    let online = event.online;

    window.open_dialog(cx, move |dialog: Dialog, _, _| {
        let delete_state = state.clone();
        let delete_key = event_key.clone();
        let mut footer = DialogFooter::new()
            .child(
                Button::new("calendar-event-delete")
                    .danger()
                    .label(tr("common.delete"))
                    .on_click(move |_, window, cx| {
                        delete_state
                            .update(cx, |state, cx| state.delete_calendar_event(&delete_key, cx));
                        window.close_dialog(cx);
                    }),
            )
            .child(
                Button::new("calendar-event-close")
                    .label(tr("common.close"))
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            );
        if can_rsvp {
            let menu_state = state.clone();
            let menu_key = event_key.clone();
            footer = footer.child(
                Button::new("calendar-event-rsvp")
                    .label("RSVP")
                    .dropdown_menu(move |menu, _, _| {
                        rsvp_menu(menu, &menu_state, &menu_key, response, send_response)
                    }),
            );
        }
        if thread_id.is_some() {
            let chat_state = state.clone();
            let chat_thread = thread_id.clone();
            footer = footer.child(Button::new("calendar-event-chat").label(tr("common.chat")).on_click(
                move |_, window, cx| {
                    chat_state.update(cx, |state, cx| {
                        state.open_meeting_chat(chat_thread.as_deref(), cx)
                    });
                    window.close_dialog(cx);
                },
            ));
        }
        if can_join {
            let join_state = state.clone();
            let url = join_url.clone();
            let title = meeting_title.clone();
            footer = footer.child(
                Button::new("calendar-event-join")
                    .primary()
                    .label(tr("common.join"))
                    .on_click(move |_, window, cx| {
                        join_state.update(cx, |state, cx| {
                            state.join_calendar_meeting(url.as_deref(), &title, cx)
                        });
                        window.close_dialog(cx);
                    }),
            );
        }

        let mut body = v_flex()
            .gap(px(FIELD_GAP))
            .child(detail_row("When", when.clone()))
            .child(detail_row("Calendar", source.clone()));
        if let Some(name) = organizer.clone() {
            body = body.child(detail_row("Organizer", name));
        }
        if let Some(url) = join_url.clone().filter(|_| online) {
            let info = join_info_text(&url, dial_in.as_ref());
            body = body.child(
                v_flex()
                    .gap(px(6.))
                    .child(detail_row("Join link", url))
                    .child(
                        h_flex().child(
                            Button::new("calendar-event-copy-join")
                                .small()
                                .label(COPY_JOIN_INFO)
                                .icon(IconName::Copy)
                                .on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(info.clone()))
                                }),
                        ),
                    ),
            );
        }

        dialog
            .title(subject.clone())
            .w(px(DIALOG_WIDTH))
            .child(body)
            .footer(footer)
    });
}
