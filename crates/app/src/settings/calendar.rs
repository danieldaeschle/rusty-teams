use gpui_kit::component::v_flex;
use gpui_kit::*;
use store::{CalendarProvider, CalendarRef};

use super::rows::{caption, control_row, text_button, toggle_row};
use crate::app_state::AppState;
use crate::theme;

pub fn render(app: &Entity<AppState>, cx: &App) -> Div {
    let accounts = app.read(cx).calendar_accounts();
    let mut body = v_flex()
        .w_full()
        .child(caption("GOOGLE"))
        .child(connect_row(app, accounts.has_google()));
    for account in &accounts.google_accounts {
        body = body.child(account_block(app, account, &accounts));
    }
    body = body
        .child(caption("HOME CALENDAR"))
        .child(home_choices(app, &accounts))
        .child(caption("ALSO WRITE TO"));
    if accounts.home.provider != CalendarProvider::Outlook {
        let write_app = app.clone();
        let target = CalendarRef::outlook();
        let selected = accounts
            .write_targets
            .iter()
            .any(|existing| existing.key() == target.key());
        body = body.child(
            toggle_row(
                "settings-write-outlook",
                "Outlook (Microsoft mailbox)",
                "Dual-write when home is Google",
                theme::text_muted(),
                selected,
            )
            .on_click(move |_, _, cx| {
                write_app.update(cx, |state, cx| {
                    state.toggle_write_target(target.clone(), cx)
                })
            }),
        );
    } else if accounts.google_accounts.is_empty() {
        body = body.child(control_row(
            "No extra write targets",
            "Connect Google, then enable dual-write on a calendar above.",
            div(),
        ));
    }
    body
}

fn connect_row(app: &Entity<AppState>, connected: bool) -> Div {
    let app = app.clone();
    if connected {
        control_row(
            "Google Calendar",
            "Connected accounts appear below. Disconnect removes tokens from this device.",
            div(),
        )
    } else {
        control_row(
            "Google Calendar",
            "Requires TEAMS_GOOGLE_CLIENT_ID. Opens a browser for sign-in.",
            text_button("settings-google-connect", "Connect Google").on_click(move |_, _, cx| {
                app.update(cx, |state, cx| state.connect_google_calendar(cx))
            }),
        )
    }
}

fn account_block(
    app: &Entity<AppState>,
    account: &store::GoogleAccount,
    accounts: &store::CalendarAccounts,
) -> Div {
    let disconnect_app = app.clone();
    let account_id = account.id.clone();
    let mut block = v_flex().w_full().child(control_row(
        account.email.clone(),
        "Google account",
        text_button(
            format!("settings-google-disconnect-{}", account.id),
            "Disconnect",
        )
        .on_click(move |_, _, cx| {
            disconnect_app.update(cx, |state, cx| {
                state.disconnect_google_account(&account_id, cx)
            })
        }),
    ));
    for calendar in &account.calendars {
        let toggle_app = app.clone();
        let account_id = account.id.clone();
        let calendar_id = calendar.id.clone();
        let write_ref = CalendarRef::google(&account.id, &calendar.id);
        let is_home = accounts.home.key() == write_ref.key();
        let is_write = accounts
            .write_targets
            .iter()
            .any(|target| target.key() == write_ref.key());
        block = block.child(
            toggle_row(
                format!("settings-google-cal-{}", calendar.id),
                calendar.summary.clone(),
                if calendar.primary {
                    "Primary calendar"
                } else {
                    "Show in calendar grid"
                },
                theme::text_muted(),
                calendar.selected,
            )
            .on_click(move |_, _, cx| {
                toggle_app.update(cx, |state, cx| {
                    state.toggle_google_calendar_selected(&account_id, &calendar_id, cx)
                })
            }),
        );
        if !is_home {
            let write_app = app.clone();
            let target = write_ref;
            block = block.child(
                toggle_row(
                    format!("settings-google-write-{}", calendar.id),
                    format!("Also write new meetings to {}", calendar.summary),
                    "",
                    theme::text_muted(),
                    is_write,
                )
                .on_click(move |_, _, cx| {
                    write_app.update(cx, |state, cx| {
                        state.toggle_write_target(target.clone(), cx)
                    })
                }),
            );
        }
    }
    block
}

fn home_choices(app: &Entity<AppState>, accounts: &store::CalendarAccounts) -> Div {
    let mut choices = v_flex().w_full();
    let outlook_app = app.clone();
    let outlook_home = accounts.home.provider == CalendarProvider::Outlook;
    choices = choices.child(
        toggle_row(
            "settings-home-outlook",
            "Outlook (Microsoft mailbox)",
            "Default when no Google home is chosen",
            theme::text_muted(),
            outlook_home,
        )
        .on_click(move |_, _, cx| {
            outlook_app.update(cx, |state, cx| {
                state.set_home_calendar(CalendarRef::outlook(), cx)
            })
        }),
    );
    for account in &accounts.google_accounts {
        for calendar in &account.calendars {
            let home_app = app.clone();
            let target = CalendarRef::google(&account.id, &calendar.id);
            let selected = accounts.home.key() == target.key();
            choices = choices.child(
                toggle_row(
                    format!("settings-home-google-{}", calendar.id),
                    format!("Google · {}", calendar.summary),
                    account.email.clone(),
                    theme::text_muted(),
                    selected,
                )
                .on_click(move |_, _, cx| {
                    home_app.update(cx, |state, cx| state.set_home_calendar(target.clone(), cx))
                }),
            );
        }
    }
    choices
}
