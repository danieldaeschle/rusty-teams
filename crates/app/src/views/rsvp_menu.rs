use chatsvc::{Reply, ResponseType};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::*;

use crate::app_state::AppState;

fn rsvp_menu_item(
    state: &Entity<AppState>,
    event_key: &str,
    label: &'static str,
    reply: Reply,
    chosen: bool,
) -> PopupMenuItem {
    let state = state.clone();
    let event_key = event_key.to_owned();
    PopupMenuItem::new(label)
        .checked(chosen)
        .on_click(move |_, _, cx| {
            state.update(cx, |state, cx| {
                state.respond_to_event(&event_key, reply, "", cx)
            });
        })
}

pub(crate) fn rsvp_menu(
    menu: PopupMenu,
    state: &Entity<AppState>,
    key: &str,
    response: ResponseType,
    send_response: bool,
) -> PopupMenu {
    let toggle_state = state.clone();
    menu.item(rsvp_menu_item(
        state,
        key,
        "Accept",
        Reply::Accept,
        response == ResponseType::Accepted,
    ))
    .item(rsvp_menu_item(
        state,
        key,
        "Tentative",
        Reply::Tentative,
        response == ResponseType::Tentative,
    ))
    .item(rsvp_menu_item(
        state,
        key,
        "Decline",
        Reply::Decline,
        response == ResponseType::Declined,
    ))
    .separator()
    .item(
        PopupMenuItem::new("Send a response")
            .checked(send_response)
            .on_click(move |_, _, cx| {
                toggle_state.update(cx, |state, cx| state.toggle_rsvp_send_response(cx));
            }),
    )
}
