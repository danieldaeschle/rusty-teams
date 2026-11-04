use gpui_kit::component::v_flex;
use gpui_kit::*;

use crate::data::Directory;
use crate::rows::StartInfo;
use crate::theme;
use crate::views::avatar::person_avatar;

const NOTES_SPACE_TITLE: &str = "This is your space";
const NOTES_SPACE_TEXT: &str = "This chat is just for you...with you. Use it for drafts, send files to yourself, or get to know chat features a little better.";
const NOTES_SPACE_AVATAR_SIZE: f32 = 72.;
const NOTES_SPACE_TEXT_WIDTH: f32 = 360.;

pub(super) fn render_notes_space(directory: &Directory, start: &StartInfo) -> AnyElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap(px(12.))
        .px(px(16.))
        .child(person_avatar(
            directory,
            start.user_id.as_deref(),
            &start.avatar_name,
            NOTES_SPACE_AVATAR_SIZE,
        ))
        .child(
            v_flex()
                .items_center()
                .gap(px(4.))
                .max_w(px(NOTES_SPACE_TEXT_WIDTH))
                .child(
                    div()
                        .text_size(px(16.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::text())
                        .child(NOTES_SPACE_TITLE),
                )
                .child(
                    div()
                        .text_center()
                        .text_size(px(13.))
                        .text_color(theme::text_muted())
                        .child(NOTES_SPACE_TEXT),
                ),
        )
        .into_any_element()
}
