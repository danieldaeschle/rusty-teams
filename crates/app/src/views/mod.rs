pub mod activity_panel;
pub mod adaptive_card;
pub mod attachment_tray;
pub mod attachments;
pub mod avatar;
pub mod call_controls;
pub mod call_extras;
pub mod call_layouts;
pub mod call_mini;
pub mod call_mini_window;
pub mod call_organizer;
pub mod call_stage;
pub mod call_view;
pub mod call_view_menu;
pub mod calls_tab;
pub mod card_carousel;
pub mod card_chart;
pub mod card_code_block;
pub mod card_icon;
pub mod card_input;
pub mod card_media;
pub mod card_table;
pub mod card_widgets;
pub mod channel_tabs;
pub mod composer;
pub mod conversation;
pub mod dialog_overlay;
pub mod draft_style;
pub mod emoji_popup;
pub mod format_toolbar;
pub mod forward_dialog;
pub mod fun_picker;
pub mod image_viewer;
pub mod join_dialog;
pub mod link_preview;
pub mod message_actions;
pub mod message_row;
pub mod new_chat;
pub mod pin_banner;
pub mod post_card;
pub mod profile_card;
pub mod proposal_card;
pub mod rail;
pub mod reaction_picker;
pub mod reaction_pills;
pub mod saved_panel;
pub mod scheduled_toolbar;
pub mod section_dialog;
pub mod share_bar;
pub mod shared_tab;
pub mod shell;
pub mod shortcuts;
pub mod shortcuts_dialog;
pub mod sidebar;
pub mod status_bar;
pub mod status_menu;
pub mod status_message_dialog;
pub mod switcher;
pub mod title_bar;
pub mod toolbar_placement;
pub mod transfer_picker;
pub mod translation_line;
pub mod widgets;

#[cfg(test)]
mod tests {
    use std::path::Path;

    fn rust_files(directory: &Path, files: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                rust_files(&path, files);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }

    #[test]
    fn hover_styles_never_toggle_display() {
        let toggle =
            regex::Regex::new(r"hover\([^|]*\|\w+\|\s*\w+\s*\.\s*(hidden|flex|block|grid)\(\)")
                .unwrap();
        let mut files = Vec::new();
        rust_files(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut files,
        );
        let offenders: Vec<String> = files
            .iter()
            .flat_map(|path| {
                let source = std::fs::read_to_string(path).unwrap();
                source
                    .lines()
                    .enumerate()
                    .filter(|(_, line)| toggle.is_match(line))
                    .map(|(index, _)| format!("{}:{}", path.display(), index + 1))
                    .collect::<Vec<_>>()
            })
            .collect();
        assert!(
            offenders.is_empty(),
            "hover styles must toggle visible()/invisible(), not display (gpui panics \"must call prepaint before paint\"): {offenders:?}"
        );
    }
}
