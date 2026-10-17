use gpui_kit::component::v_flex;
use gpui_kit::*;
use teams_core::{ChatSection, ChatSettingChange};

use super::account::{DENSITY, ENTER_KEY, account_select};
use super::rows::{caption, toggle_row};
use crate::app_state::AppState;
use crate::theme;

const MARKDOWN_DETAIL: &str = "Use mark-down language to define the structure and appearance of messages. Your formatting will take effect once you send your message. When this is on, the formatting options menu won't be visible.";
const RECEIPTS_DETAIL: &str = "Let people know when you've seen their messages and know when they've seen yours. Close and reopen any chat windows for the setting to take effect in those chats.";

pub fn render(app: &Entity<AppState>, cx: &App) -> Div {
    let state = app.read(cx);
    let muted_section = state.directory.section_settings.enabled(ChatSection::Muted);
    let settings = state.account_chat_settings();
    let markdown_mode = state.markdown_mode;
    let (section_app, markdown_app, receipts_app) = (app.clone(), app.clone(), app.clone());
    v_flex()
        .w_full()
        .child(caption("CHAT LIST"))
        .child(
            toggle_row(
                "settings-muted-section",
                "Show muted items in their own section",
                "Read from your Teams account",
                theme::text_muted(),
                muted_section,
            )
            .on_click(move |_, _, cx| {
                section_app.update(cx, |state, cx| {
                    state.set_chat_section(ChatSection::Muted, !muted_section, cx)
                })
            }),
        )
        .child(caption("CHATS"))
        .child(account_select(
            app,
            (
                "settings-message-density",
                "Message density",
                "Compact your chats and see more messages",
            ),
            &DENSITY,
            settings.message_density,
            ChatSettingChange::MessageDensity,
        ))
        .child(account_select(
            app,
            (
                "settings-enter-key",
                "When writing a message, press Enter to",
                "This setting doesn't apply when message formatting is active.",
            ),
            &ENTER_KEY,
            settings.enter_key,
            ChatSettingChange::EnterKey,
        ))
        .child(
            toggle_row(
                "settings-markdown",
                "Format messages using mark-down",
                MARKDOWN_DETAIL,
                theme::text_muted(),
                markdown_mode,
            )
            .on_click(move |_, _, cx| {
                markdown_app.update(cx, |state, cx| state.set_markdown_mode(!markdown_mode, cx))
            }),
        )
        .child(
            toggle_row(
                "settings-read-receipts",
                "Read receipts",
                RECEIPTS_DETAIL,
                theme::text_muted(),
                settings.read_receipts,
            )
            .on_click(move |_, _, cx| {
                receipts_app.update(cx, |state, cx| {
                    state.save_chat_setting(
                        ChatSettingChange::ReadReceipts(!settings.read_receipts),
                        cx,
                    )
                })
            }),
        )
}
