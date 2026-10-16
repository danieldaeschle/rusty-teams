use gpui_kit::*;

pub const COMPOSER_KEY_CONTEXT: &str = "Composer";

actions!(
    composer,
    [
        ToggleBold,
        ToggleItalic,
        ToggleUnderline,
        ToggleStrike,
        ToggleSuperscript,
        ToggleSubscript,
        ToggleCode,
        EditLink,
        ScheduleSend
    ]
);

actions!(teams, [ReplyToHovered, OpenSwitcher, NewChat, ToggleCallMute]);
