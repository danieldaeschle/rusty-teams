use chrono::{Local, Offset, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use store::MessageRecord;
use teams_core::FileKind;

use super::{ChannelPane, ConversationView};
use crate::app_state::Selection;
use crate::channel_files::{SharedContent, SharedItem};
use crate::format;
use crate::rows::reply_excerpt;
use crate::theme;
use crate::views::attachments::file_badge;
use crate::views::avatar::person_avatar;

pub const CHANNEL_DETAILS_TAG: &str = "shell/islands/main/channel-details";
pub const CHANNEL_DETAILS_BUTTON: &str = "shell/islands/main/channel-details-button";
pub const CHANNEL_DETAILS_PINNED_TAG: &str = "shell/islands/main/channel-details/pinned";
pub const CHANNEL_DETAILS_SHARED_TAG: &str = "shell/islands/main/channel-details/shared";
const OPEN_LABEL: &str = "Open channel details";
const CLOSE_LABEL: &str = "Close channel details";
const PINNED_TITLE: &str = "Pinned";
const SHARED_TITLE: &str = "Shared";
const SEE_ALL_SHARED_ARIA: &str = "See all shared files";
const SEE_ALL_LABEL: &str = "See all";
const SEE_ALL_ARIA: &str = "See all pinned posts";
const BACK_LABEL: &str = "Back";
const FALLBACK_PREVIEW: &str = "Pinned post";
const PANE_WIDTH: f32 = 320.;
const PANE_PADDING: f32 = 16.;
const HEADER_HEIGHT: f32 = 48.;
const BUTTON_SIZE: f32 = 32.;
const ITEM_AVATAR: f32 = 24.;
const COLLAPSED_PIN_COUNT: usize = 3;
const SHARED_FILE_COUNT: usize = 3;

pub(super) struct PinnedPostItem {
    pub message_id: String,
    pub author_id: Option<String>,
    pub title: String,
    pub byline: String,
}

/// Teams' info pane item: post subject or text, then "author, date".
pub(super) fn pinned_post_item(message_id: &str, record: Option<&MessageRecord>) -> PinnedPostItem {
    let Some(record) = record else {
        return PinnedPostItem {
            message_id: message_id.to_owned(),
            author_id: None,
            title: FALLBACK_PREVIEW.to_owned(),
            byline: String::new(),
        };
    };
    let offset = Local::now().offset().fix();
    let today = Utc::now().with_timezone(&offset).date_naive();
    let date = format::list_time_label(record.created_at, today, offset);
    let author = record.sender_name.clone().unwrap_or_default();
    let byline = match (author.is_empty(), date.is_empty()) {
        (false, false) => format!("{author}, {date}"),
        (false, true) => author,
        _ => date,
    };
    PinnedPostItem {
        message_id: message_id.to_owned(),
        author_id: record.sender_id.clone(),
        title: record
            .subject
            .clone()
            .filter(|subject| !subject.trim().is_empty())
            .unwrap_or_else(|| reply_excerpt(record)),
        byline,
    }
}

pub(super) struct SharedFileRow {
    pub message_id: String,
    pub name: String,
    pub kind: FileKind,
    pub byline: String,
}

pub(super) fn shared_file_rows(items: &[SharedItem]) -> (Vec<SharedFileRow>, bool) {
    let offset = Local::now().offset().fix();
    let today = Utc::now().with_timezone(&offset).date_naive();
    let mut files = items.iter().filter_map(|item| match &item.content {
        SharedContent::File(card) => Some((item, card)),
        SharedContent::Link(_) => None,
    });
    let rows = files
        .by_ref()
        .take(SHARED_FILE_COUNT)
        .map(|(item, card)| {
            let date = format::list_time_label(item.created_at, today, offset);
            SharedFileRow {
                message_id: item.message_id.clone(),
                name: card.name.clone(),
                kind: card.kind,
                byline: [item.author.as_str(), date.as_str()]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join(", "),
            }
        })
        .collect();
    (rows, files.next().is_some())
}

pub(super) fn shown_pin_count(total: usize, expanded: bool) -> usize {
    if expanded {
        total
    } else {
        total.min(COLLAPSED_PIN_COUNT)
    }
}

impl ConversationView {
    fn details_channel_id(&self) -> Option<String> {
        match &self.current.as_ref()?.selection {
            Selection::Channel(channel_id) => Some(channel_id.clone()),
            Selection::Chat(_) => None,
        }
    }

    pub(super) fn channel_details_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.details_channel_id()?;
        let open = self.channel_details_open;
        Some(
            Button::new("channel-details-toggle")
                .debug_selector(|| CHANNEL_DETAILS_BUTTON.into())
                .icon(if open {
                    IconName::PanelRightClose
                } else {
                    IconName::PanelRight
                })
                .ghost()
                .size(px(BUTTON_SIZE))
                .rounded(px(8.))
                .tooltip(if open { CLOSE_LABEL } else { OPEN_LABEL })
                .on_click(cx.listener(|this, _, _, cx| {
                    this.channel_details_open = !this.channel_details_open;
                    this.pinned_posts_expanded = false;
                    if this.channel_details_open {
                        this.refresh_shared_items(cx);
                    }
                    cx.notify();
                }))
                .into_any_element(),
        )
    }

    pub(super) fn render_channel_details(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.channel_details_open {
            return None;
        }
        let channel_id = self.details_channel_id()?;
        let state = self.app.read(cx);
        let ids: Vec<String> = state
            .pinned_messages(&channel_id)
            .iter()
            .map(|pin| pin.message_id.clone())
            .collect();
        let records = state
            .store
            .messages_by_id(&channel_id, &ids)
            .unwrap_or_default();
        let items: Vec<PinnedPostItem> = ids
            .iter()
            .map(|id| pinned_post_item(id, records.get(id)))
            .collect();
        let expanded = self.pinned_posts_expanded;
        let shown = shown_pin_count(items.len(), expanded);
        let has_more = items.len() > shown;
        let title = self
            .current
            .as_ref()
            .map(|current| current.title.clone())
            .unwrap_or_default();
        let directory = &state.directory;
        let mut body = v_flex()
            .w_full()
            .px(px(PANE_PADDING))
            .pb(px(PANE_PADDING))
            .gap(px(4.));
        if !items.is_empty() {
            let mut section = v_flex()
                .w_full()
                .gap(px(4.))
                .debug_selector(|| CHANNEL_DETAILS_PINNED_TAG.into());
            if !expanded {
                section = section.child(section_title(PINNED_TITLE));
            }
            for item in items.iter().take(shown) {
                section = section.child(self.pinned_post_row(&channel_id, item, directory, cx));
            }
            if has_more {
                section = section.child(
                    Button::new("channel-details-see-all-pins")
                        .ghost()
                        .label(SEE_ALL_LABEL)
                        .tooltip(SEE_ALL_ARIA)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.pinned_posts_expanded = true;
                            cx.notify();
                        })),
                );
            }
            body = body.child(section);
        }
        let (shared_rows, more_shared) = shared_file_rows(&self.shared.items);
        if !expanded && !shared_rows.is_empty() {
            let mut section = v_flex()
                .w_full()
                .gap(px(4.))
                .debug_selector(|| CHANNEL_DETAILS_SHARED_TAG.into())
                .child(section_title(SHARED_TITLE));
            for row in &shared_rows {
                section = section.child(self.shared_file_row(&channel_id, row, cx));
            }
            if more_shared {
                section = section.child(
                    Button::new("channel-details-see-all-shared")
                        .ghost()
                        .label(SEE_ALL_LABEL)
                        .tooltip(SEE_ALL_SHARED_ARIA)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.select_pane(ChannelPane::Shared, cx);
                        })),
                );
            }
            body = body.child(section);
        }
        let header = if expanded {
            h_flex()
                .gap(px(8.))
                .child(
                    Button::new("channel-details-back")
                        .icon(IconName::ChevronLeft)
                        .ghost()
                        .size(px(BUTTON_SIZE))
                        .rounded(px(8.))
                        .tooltip(BACK_LABEL)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.pinned_posts_expanded = false;
                            cx.notify();
                        })),
                )
                .child(header_title(PINNED_TITLE.to_owned()))
        } else {
            h_flex().child(header_title(title))
        };
        Some(
            h_flex()
                .h_full()
                .flex_none()
                .child(div().w(px(1.)).h_full().flex_none().bg(theme::border()))
                .child(
                    v_flex()
                        .debug_selector(|| CHANNEL_DETAILS_TAG.into())
                        .w(px(PANE_WIDTH))
                        .h_full()
                        .bg(theme::surface())
                        .child(
                            h_flex()
                                .h(px(HEADER_HEIGHT))
                                .flex_none()
                                .px(px(PANE_PADDING))
                                .items_center()
                                .justify_between()
                                .child(header.flex_1().min_w_0().items_center())
                                .child(
                                    Button::new("channel-details-close")
                                        .icon(IconName::X)
                                        .ghost()
                                        .size(px(BUTTON_SIZE))
                                        .rounded(px(8.))
                                        .tooltip(CLOSE_LABEL)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.channel_details_open = false;
                                            this.pinned_posts_expanded = false;
                                            cx.notify();
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .id("channel-details-body")
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scroll()
                                .child(body),
                        ),
                )
                .into_any_element(),
        )
    }

    fn pinned_post_row(
        &self,
        channel_id: &str,
        item: &PinnedPostItem,
        directory: &crate::data::Directory,
        cx: &Context<Self>,
    ) -> AnyElement {
        let (channel_id, message_id) = (channel_id.to_owned(), item.message_id.clone());
        let author_name = item.byline.split(',').next().unwrap_or_default().to_owned();
        h_flex()
            .id(SharedString::from(format!(
                "pinned-post-{}",
                item.message_id
            )))
            .w_full()
            .p(px(8.))
            .gap(px(10.))
            .items_start()
            .rounded(px(8.))
            .cursor_pointer()
            .hover(|row| row.bg(theme::row_hover()))
            .child(person_avatar(
                directory,
                item.author_id.as_deref(),
                &author_name,
                ITEM_AVATAR,
            ))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(13.))
                            .text_color(theme::text())
                            .child(item.title.clone()),
                    )
                    .when(!item.byline.is_empty(), |column| {
                        column.child(
                            div()
                                .truncate()
                                .text_size(px(12.))
                                .text_color(theme::text_muted())
                                .child(item.byline.clone()),
                        )
                    }),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.jump_to_message(&channel_id, &message_id, cx);
            }))
            .into_any_element()
    }
}

impl ConversationView {
    fn shared_file_row(
        &self,
        channel_id: &str,
        row: &SharedFileRow,
        cx: &Context<Self>,
    ) -> AnyElement {
        let (channel_id, message_id) = (channel_id.to_owned(), row.message_id.clone());
        h_flex()
            .id(SharedString::from(format!(
                "shared-file-{}-{}",
                row.message_id, row.name
            )))
            .w_full()
            .p(px(8.))
            .gap(px(10.))
            .items_start()
            .rounded(px(8.))
            .cursor_pointer()
            .hover(|item| item.bg(theme::row_hover()))
            .child(file_badge(row.kind, ITEM_AVATAR))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(13.))
                            .text_color(theme::text())
                            .child(row.name.clone()),
                    )
                    .when(!row.byline.is_empty(), |column| {
                        column.child(
                            div()
                                .truncate()
                                .text_size(px(12.))
                                .text_color(theme::text_muted())
                                .child(row.byline.clone()),
                        )
                    }),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.jump_to_message(&channel_id, &message_id, cx);
            }))
            .into_any_element()
    }
}

fn section_title(text: &'static str) -> Div {
    div()
        .py(px(8.))
        .text_size(px(14.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text())
        .child(text)
}

fn header_title(text: String) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .truncate()
        .text_size(px(15.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text())
        .child(text)
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use store::MessageRecord;

    use teams_core::{FileCard, FileKind};

    use super::{FALLBACK_PREVIEW, pinned_post_item, shared_file_rows, shown_pin_count};
    use crate::channel_files::{SharedContent, SharedItem};

    fn record(subject: Option<&str>) -> MessageRecord {
        MessageRecord {
            conversation_id: "19:channel".into(),
            message_id: "1".into(),
            sender_id: Some("user-ada".into()),
            sender_name: Some("Ada Lovelace".into()),
            created_at: Utc.with_ymd_and_hms(2024, 3, 4, 9, 30, 0).unwrap(),
            body_html: "<p>Release notes are up</p>".into(),
            subject: subject.map(str::to_owned),
            attachments_json: "[]".into(),
            reactions_json: "[]".into(),
            mentions_json: "[]".into(),
            links_json: "[]".into(),
            ..MessageRecord::default()
        }
    }

    #[test]
    fn the_item_prefers_the_subject_and_names_author_and_date() {
        let item = pinned_post_item("1", Some(&record(Some("Release 4.2"))));
        assert_eq!(item.title, "Release 4.2");
        assert!(item.byline.starts_with("Ada Lovelace, "), "{}", item.byline);
        assert_eq!(item.author_id.as_deref(), Some("user-ada"));
        let plain = pinned_post_item("1", Some(&record(None)));
        assert_eq!(plain.title, "Release notes are up");
    }

    #[test]
    fn an_uncached_post_keeps_its_place_with_a_fallback() {
        let item = pinned_post_item("9", None);
        assert_eq!(item.message_id, "9");
        assert_eq!(item.title, FALLBACK_PREVIEW);
        assert!(item.byline.is_empty());
    }

    #[test]
    fn three_pins_show_until_see_all() {
        assert_eq!(shown_pin_count(5, false), 3);
        assert_eq!(shown_pin_count(2, false), 2);
        assert_eq!(shown_pin_count(5, true), 5);
    }

    fn shared(content: SharedContent, message_id: &str) -> SharedItem {
        SharedItem {
            content,
            author: "Ada Lovelace".into(),
            post_label: String::new(),
            message_id: message_id.into(),
            created_at: Utc.with_ymd_and_hms(2024, 3, 4, 9, 30, 0).unwrap(),
        }
    }

    fn file(name: &str) -> SharedContent {
        SharedContent::File(FileCard {
            name: name.into(),
            kind: FileKind::from_name(name),
            content_type: None,
            size: None,
            open_url: format!("https://example.invalid/{name}"),
        })
    }

    #[test]
    fn shared_section_lists_the_newest_files_and_skips_links() {
        let items = vec![
            shared(SharedContent::Link("https://example.invalid".into()), "1"),
            shared(file("plan.docx"), "2"),
            shared(file("budget.xlsx"), "3"),
        ];
        let (rows, more) = shared_file_rows(&items);
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["plan.docx", "budget.xlsx"]);
        assert!(!more);
        assert_eq!(rows[0].message_id, "2");
        assert!(
            rows[0].byline.starts_with("Ada Lovelace, "),
            "{}",
            rows[0].byline
        );
    }

    #[test]
    fn shared_section_offers_see_all_after_three_files() {
        let items: Vec<SharedItem> = (0..4)
            .map(|index| shared(file(&format!("file-{index}.pdf")), &index.to_string()))
            .collect();
        let (rows, more) = shared_file_rows(&items);
        assert_eq!(rows.len(), 3);
        assert!(more);
    }
}
