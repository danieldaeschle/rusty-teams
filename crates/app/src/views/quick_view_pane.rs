use std::collections::HashMap;

use chrono::{DateTime, FixedOffset, Local, NaiveDate, Offset, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{h_flex, tooltip::Tooltip, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::SavedMessage;

use super::avatar::person_avatar;
use super::scroll_edge::with_top_edge;
use super::widgets::icon;
use crate::activity::{ActivityCenter, Entry, Kind};
use store::Sidebar;

use crate::app_state::{AppState, QuickView, selection_for, selection_title};
use crate::format;
use crate::local_previews::LocalPreview;
use crate::theme;

const QUICK_VIEW_SCROLL_DIVIDER: &str = "shell/islands/main/quick-view-divider-scroll";
const HEADER_HEIGHT: f32 = 60.;
const HEADER_PADDING: f32 = 20.;
const AVATAR_SIZE: f32 = 32.;
const BUTTON_SIZE: f32 = 24.;
const PREVIEW_LINES: usize = 2;
const LIST_MAX_WIDTH: f32 = 860.;
const UNKNOWN_AUTHOR: &str = "Unknown";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickRow {
    pub conversation_id: String,
    pub message_id: Option<String>,
    pub author_id: Option<String>,
    pub author: String,
    pub place: String,
    pub time: String,
    pub preview: String,
    pub saved: Option<SavedMessage>,
}

fn place_of(sidebar: &Sidebar, conversation_id: &str) -> Option<String> {
    selection_for(sidebar, conversation_id).map(|selection| selection_title(sidebar, &selection))
}

fn time_label(time: DateTime<Utc>, today: NaiveDate, offset: FixedOffset) -> String {
    format::list_time_label(time, today, offset)
}

pub fn mention_rows(
    entries: &[Entry],
    sidebar: &Sidebar,
    today: NaiveDate,
    offset: FixedOffset,
) -> Vec<QuickRow> {
    let mut mentions: Vec<&Entry> = entries
        .iter()
        .filter(|entry| entry.kind == Kind::Mention)
        .collect();
    mentions.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    mentions
        .into_iter()
        .map(|entry| {
            let actor = entry.actors.first();
            QuickRow {
                conversation_id: entry.conversation_id.clone(),
                message_id: Some(entry.message_id.clone()).filter(|id| !id.is_empty()),
                author_id: actor.and_then(|actor| actor.user_id.clone()),
                author: actor
                    .map(|actor| actor.name.clone())
                    .unwrap_or_else(|| UNKNOWN_AUTHOR.to_owned()),
                place: place_of(sidebar, &entry.conversation_id)
                    .or_else(|| entry.source_label.clone())
                    .unwrap_or_default(),
                time: time_label(entry.updated_at, today, offset),
                preview: entry.preview.clone(),
                saved: None,
            }
        })
        .collect()
}

pub fn draft_rows(previews: &HashMap<String, LocalPreview>, sidebar: &Sidebar) -> Vec<QuickRow> {
    let mut rows: Vec<QuickRow> = previews
        .iter()
        .filter_map(|(conversation_id, preview)| {
            let LocalPreview::Draft(text) = preview else {
                return None;
            };
            let place = place_of(sidebar, conversation_id)?;
            Some(QuickRow {
                conversation_id: conversation_id.clone(),
                message_id: None,
                author_id: None,
                author: place.clone(),
                place: String::new(),
                time: String::new(),
                preview: text.clone(),
                saved: None,
            })
        })
        .collect();
    rows.sort_by(|left, right| left.author.to_lowercase().cmp(&right.author.to_lowercase()));
    rows
}

pub fn saved_rows(
    items: &[SavedMessage],
    sidebar: &Sidebar,
    today: NaiveDate,
    offset: FixedOffset,
) -> Vec<QuickRow> {
    items
        .iter()
        .map(|item| QuickRow {
            conversation_id: item.conversation_id.clone(),
            message_id: Some(item.message_id.clone()),
            author_id: item.author_id.clone(),
            author: item
                .author_name
                .clone()
                .unwrap_or_else(|| UNKNOWN_AUTHOR.to_owned()),
            place: place_of(sidebar, &item.conversation_id)
                .or_else(|| item.topic.clone())
                .unwrap_or_default(),
            time: time_label(item.saved_at, today, offset),
            preview: item.preview.clone(),
            saved: Some(item.clone()),
        })
        .collect()
}

pub enum QuickViewEvent {
    Open {
        conversation_id: String,
        message_id: Option<String>,
    },
}

pub struct QuickViewPane {
    app: Entity<AppState>,
    activity: Entity<ActivityCenter>,
    scroll: ScrollHandle,
    _observations: [Subscription; 2],
}

impl EventEmitter<QuickViewEvent> for QuickViewPane {}

impl QuickViewPane {
    pub fn new(
        app: Entity<AppState>,
        activity: Entity<ActivityCenter>,
        cx: &mut Context<Self>,
    ) -> Self {
        let observations = [
            cx.observe(&app, |_, _, cx| cx.notify()),
            cx.observe(&activity, |_, _, cx| cx.notify()),
        ];
        QuickViewPane {
            app,
            activity,
            scroll: ScrollHandle::new(),
            _observations: observations,
        }
    }

    fn rows(&self, view: QuickView, cx: &App) -> Vec<QuickRow> {
        let state = self.app.read(cx);
        let offset = Local::now().offset().fix();
        let today = Utc::now().with_timezone(&offset).date_naive();
        match view {
            QuickView::Mentions => mention_rows(
                self.activity.read(cx).feed().entries(),
                &state.sidebar,
                today,
                offset,
            ),
            QuickView::Drafts => draft_rows(&state.local_previews, &state.sidebar),
            QuickView::Saved => saved_rows(state.saved.items(), &state.sidebar, today, offset),
        }
    }

    fn header(&self, view: QuickView, count: usize) -> Div {
        h_flex()
            .h(px(HEADER_HEIGHT))
            .flex_none()
            .px(px(HEADER_PADDING))
            .gap(px(8.))
            .items_center()
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme::text())
                    .child(view.title()),
            )
            .when(count > 0, |header| {
                header.child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::text_muted())
                        .child(count.to_string()),
                )
            })
    }

    fn row(&self, row: &QuickRow, index: usize, cx: &Context<Self>) -> AnyElement {
        let app = self.app.clone();
        let group = SharedString::from(format!("quick-row-group-{index}"));
        let (conversation_id, message_id) = (row.conversation_id.clone(), row.message_id.clone());
        let unsave = row.saved.clone().map(|item| {
            div()
                .id(ElementId::Name(format!("quick-unsave-{index}").into()))
                .absolute()
                .right(px(8.))
                .top(px(8.))
                .size(px(BUTTON_SIZE))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .cursor_pointer()
                .invisible()
                .group_hover(group.clone(), |button| button.visible())
                .hover(|button| button.bg(theme::popup_hover()))
                .child(icon(IconName::BookmarkOff, 14., theme::text_muted()))
                .tooltip(|window, cx| Tooltip::new("Unsave").build(window, cx))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    let entry = item.clone();
                    app.update(cx, |state, cx| state.set_saved(entry, false, cx));
                })
        });
        let directory = &self.app.read(cx).directory;
        h_flex()
            .id(ElementId::Name(format!("quick-row-{index}").into()))
            .group(group.clone())
            .relative()
            .w_full()
            .p(px(12.))
            .gap(px(12.))
            .items_start()
            .rounded(px(8.))
            .cursor_pointer()
            .hover(|row| row.bg(theme::row_hover()))
            .child(div().flex_none().size(px(AVATAR_SIZE)).child(person_avatar(
                directory,
                row.author_id.as_deref(),
                &row.author,
                AVATAR_SIZE,
            )))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.))
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .items_baseline()
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::text())
                                    .child(row.author.clone()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(12.))
                                    .text_color(theme::text_muted())
                                    .child(row.place.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .pr(px(BUTTON_SIZE + 8.))
                                    .text_size(px(12.))
                                    .text_color(theme::text_muted())
                                    .child(row.time.clone()),
                            ),
                    )
                    .child(
                        div()
                            .line_clamp(PREVIEW_LINES)
                            .text_ellipsis()
                            .text_size(px(14.))
                            .line_height(relative(1.4))
                            .text_color(theme::text_soft())
                            .child(row.preview.clone()),
                    ),
            )
            .children(unsave)
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.emit(QuickViewEvent::Open {
                    conversation_id: conversation_id.clone(),
                    message_id: message_id.clone(),
                });
            }))
            .into_any_element()
    }
}

impl Render for QuickViewPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.app.read(cx).quick_view.unwrap_or(QuickView::Saved);
        let rows = self.rows(view, cx);
        let mut list: Vec<AnyElement> = rows
            .iter()
            .enumerate()
            .map(|(index, row)| self.row(row, index, cx))
            .collect();
        if list.is_empty() {
            list.push(
                div()
                    .py(px(32.))
                    .px(px(24.))
                    .flex()
                    .justify_center()
                    .text_center()
                    .text_size(px(13.))
                    .text_color(theme::text_muted())
                    .child(view.empty_text())
                    .into_any_element(),
            );
        }
        v_flex()
            .debug_selector(|| "shell/islands/main".into())
            .flex_1()
            .w_full()
            .min_w_0()
            .h_full()
            .rounded(px(10.))
            .bg(theme::surface())
            .overflow_hidden()
            .child(self.header(view, rows.len()))
            .child(div().w_full().h(px(1.)).flex_none().bg(theme::border()))
            .child(with_top_edge(
                v_flex()
                    .id("quick-view-list")
                    .flex_1()
                    .min_h_0()
                    .px(px(HEADER_PADDING - 12.))
                    .py(px(8.))
                    .overflow_y_scrollbar()
                    .track_scroll(&self.scroll)
                    .child(v_flex().w_full().max_w(px(LIST_MAX_WIDTH)).children(list)),
                &self.scroll,
                QUICK_VIEW_SCROLL_DIVIDER,
            ))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{FixedOffset, NaiveDate, TimeZone, Utc};
    use store::Sidebar;

    use super::{draft_rows, mention_rows};
    use crate::activity::{Actor, Entry, Kind};
    use crate::local_previews::LocalPreview;

    fn entry(id: &str, kind: Kind, minute: u32) -> Entry {
        Entry {
            id: id.to_owned(),
            conversation_id: "19:chat".to_owned(),
            kind,
            message_id: format!("m-{id}"),
            actors: vec![Actor {
                user_id: Some("ada".to_owned()),
                name: "Ada".to_owned(),
            }],
            preview: "@Dana look".to_owned(),
            updated_at: Utc.with_ymd_and_hms(2026, 11, 8, 9, minute, 0).unwrap(),
            ..Entry::default()
        }
    }

    #[test]
    fn mentions_list_only_mentions_newest_first() {
        let entries = [
            entry("a", Kind::Mention, 1),
            entry("b", Kind::Reaction, 2),
            entry("c", Kind::Mention, 3),
        ];
        let offset = FixedOffset::east_opt(0).unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 11, 8).unwrap();
        let rows = mention_rows(&entries, &Sidebar::default(), today, offset);
        let ids: Vec<_> = rows
            .iter()
            .map(|row| row.message_id.clone().unwrap())
            .collect();
        assert_eq!(ids, ["m-c", "m-a"]);
        assert_eq!(rows[0].author, "Ada");
    }

    #[test]
    fn drafts_skip_unsent_and_unknown_conversations() {
        let previews = HashMap::from([
            ("19:gone".to_owned(), LocalPreview::Draft("lost".to_owned())),
            ("19:chat".to_owned(), LocalPreview::NotSent),
        ]);
        assert!(draft_rows(&previews, &Sidebar::default()).is_empty());
    }
}
