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
        ScheduleSend,
        PastePlainText,
        InsertTableRowAbove,
        InsertTableRowBelow,
        InsertTableColumnLeft,
        InsertTableColumnRight,
        DeleteTableRow,
        DeleteTableColumn,
        DeleteTable,
        ToggleFormatBar,
        InsertTableLine,
        RemoveTableLine
    ]
);

actions!(
    teams,
    [ReplyToHovered, OpenSwitcher, NewChat, ToggleCallMute]
);
