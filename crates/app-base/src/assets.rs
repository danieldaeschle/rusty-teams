use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};

macro_rules! symbols {
    ($($name:literal),* $(,)?) => {
        [$((concat!("symbols/", $name, ".svg"), include_bytes!(concat!("../../app/assets/symbols/", $name, ".svg")) as &[u8])),*]
    };
}

const SYMBOLS: [(&str, &[u8]); 82] = symbols![
    "campaign",
    "lock",
    "bot",
    "copilot_color",
    "done",
    "table",
    "done_all",
    "keyboard_return",
    "schedule",
    "schedule_send",
    "event",
    "wb_sunny",
    "work",
    "close",
    "reply",
    "group",
    "tag",
    "image",
    "check_circle",
    "error",
    "notifications",
    "content_copy",
    "attach_file",
    "upload",
    "download",
    "folder_open",
    "refresh",
    "code",
    "link",
    "format_list_bulleted",
    "format_list_numbered",
    "format_quote",
    "mood",
    "status_dot",
    "status_dnd",
    "status_away",
    "status_ring",
    "status_available",
    "status_offline",
    "recap",
    "zoom_in",
    "zoom_out",
    "rotate_right",
    "forward",
    "open_in_new",
    "chevron_left",
    "chevron_right",
    "fullscreen",
    "spinner_track",
    "spinner_arc",
    "shield",
    "call",
    "call_missed",
    "videocam",
    "fluent_video",
    "fluent_record",
    "fluent_record_stop",
    "fluent_cloud_arrow_up",
    "fluent_share",
    "fluent_clock",
    "fluent_document_text",
    "fluent_text_edit_style",
    "fluent_text_edit_style_filled",
    "fluent_table_add",
    "fluent_table_dismiss",
    "fluent_important",
    "fluent_important_filled",
    "fluent_alert_urgent",
    "fluent_alert_urgent_filled",
    "fluent_chat",
    "fluent_text_color",
    "fluent_highlight",
    "fluent_text_indent_increase",
    "fluent_text_indent_decrease",
    "fluent_line_horizontal_1",
    "fluent_text_clear_formatting",
    "fluent_code_block",
    "fluent_text_header_1",
    "fluent_text_font_size",
    "fluent_send",
    "fluent_add",
    "fluent_calendar_add",
];

pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = SYMBOLS.iter().find(|(name, _)| *name == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        gpui_kit::assets::AllAssets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut names = gpui_kit::assets::AllAssets.list(path)?;
        names.extend(
            SYMBOLS
                .iter()
                .filter(|(name, _)| name.starts_with(path))
                .map(|(name, _)| SharedString::from(*name)),
        );
        Ok(names)
    }
}
