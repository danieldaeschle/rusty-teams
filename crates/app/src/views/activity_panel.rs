use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Local, NaiveDate, Offset};
use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, tooltip::Tooltip, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use store::Sidebar;

use super::avatar::person_avatar;
use super::title_bar::TITLE_BAR_HEIGHT;
use super::widgets::{dot, icon};
use crate::activity::{ActivityCenter, Entry, Filter, Kind};
use crate::app_state::{AppState, chat_title};
use crate::data::is_one_on_one;
use crate::format;
use crate::people::resolve_names;
use crate::theme;

const PANEL_WIDTH: f32 = 420.;
const PANEL_MAX_HEIGHT_RATIO: f32 = 0.7;
const PANEL_MARGIN: f32 = 8.;
const AVATAR_SIZE: f32 = 32.;
const KIND_MARKER_SIZE: f32 = 16.;
const BUTTON_SIZE: f32 = 28.;
const MARK_READ_SIZE: f32 = 24.;
const FILTER_KEY: &str = "activity_filter";

pub enum ActivityPanelEvent {
    Close,
    OpenSettings,
    Open {
        conversation_id: String,
        message_id: String,
    },
}

pub struct ActivityPanel {
    app: Entity<AppState>,
    activity: Entity<ActivityCenter>,
    filter: Filter,
    focus_handle: FocusHandle,
    scroll: ScrollHandle,
    resolved_names: HashMap<String, String>,
    looked_up: HashSet<String>,
    _observations: [Subscription; 2],
}

impl EventEmitter<ActivityPanelEvent> for ActivityPanel {}

impl ActivityPanel {
    pub fn new(
        app: Entity<AppState>,
        activity: Entity<ActivityCenter>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        let observations = [
            cx.observe(&app, |_, _, cx| cx.notify()),
            cx.observe(&activity, |_, _, cx| cx.notify()),
        ];
        let filter = app
            .read(cx)
            .store
            .meta(FILTER_KEY)
            .ok()
            .flatten()
            .and_then(|key| Filter::from_key(&key))
            .unwrap_or_default();
        ActivityPanel {
            app,
            activity,
            filter,
            focus_handle,
            scroll: ScrollHandle::new(),
            resolved_names: HashMap::new(),
            looked_up: HashSet::new(),
            _observations: observations,
        }
    }

    fn open(&mut self, id: i64, cx: &mut Context<Self>) {
        let Some(entry) = self.activity.read(cx).feed().get(id) else {
            return;
        };
        let event = ActivityPanelEvent::Open {
            conversation_id: entry.conversation_id.clone(),
            message_id: entry.message_id.clone(),
        };
        self.activity
            .update(cx, |activity, cx| activity.mark_read(id, cx));
        cx.emit(event);
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            cx.emit(ActivityPanelEvent::Close);
        }
    }

    fn header(&self, cx: &mut Context<Self>) -> Div {
        let pills = Filter::ALL.into_iter().map(|filter| {
            let selected = filter == self.filter;
            div()
                .id(ElementId::Name(
                    format!("activity-filter-{}", filter.label()).into(),
                ))
                .h(px(24.))
                .px(px(10.))
                .flex()
                .items_center()
                .rounded_full()
                .cursor_pointer()
                .text_size(px(12.))
                .when(selected, |pill| {
                    pill.bg(theme::accent())
                        .text_color(theme::on_accent())
                        .font_weight(FontWeight::SEMIBOLD)
                })
                .when(!selected, |pill| {
                    pill.bg(theme::surface_raised())
                        .text_color(theme::text_soft())
                        .hover(|pill| pill.bg(theme::border_strong()))
                })
                .child(filter.label())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.filter = filter;
                    let _ = this.app.read(cx).store.set_meta(FILTER_KEY, filter.key());
                    cx.notify();
                }))
        });
        v_flex()
            .px(px(12.))
            .pt(px(10.))
            .pb(px(8.))
            .gap(px(8.))
            .border_b_1()
            .border_color(theme::border())
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text())
                            .child("Activity"),
                    )
                    .child(
                        div()
                            .id("activity-mark-all-read")
                            .px(px(8.))
                            .h(px(BUTTON_SIZE))
                            .flex()
                            .items_center()
                            .rounded(px(6.))
                            .cursor_pointer()
                            .text_size(px(12.))
                            .text_color(theme::accent_text())
                            .hover(|button| button.bg(theme::surface_raised()))
                            .child("Mark all as read")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.activity
                                    .update(cx, |activity, cx| activity.mark_all_read(cx));
                            })),
                    )
                    .child(
                        div()
                            .id("activity-settings")
                            .size(px(BUTTON_SIZE))
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .rounded(px(6.))
                            .cursor_pointer()
                            .hover(|button| button.bg(theme::surface_raised()))
                            .child(icon(IconName::Settings, 16., theme::text_muted()))
                            .on_click(cx.listener(|_, _, _, cx| {
                                cx.emit(ActivityPanelEvent::OpenSettings);
                            })),
                    ),
            )
            .child(h_flex().gap(px(6.)).children(pills))
    }

    fn row(
        &self,
        entry: &Entry,
        sidebar: &Sidebar,
        directory: &crate::data::Directory,
        today: NaiveDate,
        offset: chrono::FixedOffset,
        panel: Entity<ActivityPanel>,
    ) -> AnyElement {
        let activity = self.activity.clone();
        let id = entry.id;
        let resolved_names = &self.resolved_names;
        let unread = !entry.read;
        let actor = entry.latest_actor();
        let avatar = person_avatar(
            directory,
            actor.and_then(|actor| actor.user_id.as_deref()),
            actor.map_or("", |actor| actor.display_name(resolved_names)),
            AVATAR_SIZE,
        );
        let group = SharedString::from(format!("activity-row-group-{id}"));
        h_flex()
            .id(ElementId::Name(format!("activity-row-{id}").into()))
            .group(group.clone())
            .mx(px(6.))
            .px(px(6.))
            .py(px(8.))
            .gap(px(10.))
            .items_start()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|row| row.bg(theme::row_hover()))
            .child(
                div()
                    .relative()
                    .flex_none()
                    .size(px(AVATAR_SIZE))
                    .child(avatar)
                    .child(kind_marker(entry)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(13.))
                            .font_weight(if unread {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(if unread {
                                theme::text()
                            } else {
                                theme::text_soft()
                            })
                            .child(headline(entry, sidebar, resolved_names)),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(if unread {
                                theme::text_soft()
                            } else {
                                theme::text_muted()
                            })
                            .child(detail(entry)),
                    ),
            )
            .child(
                v_flex()
                    .flex_none()
                    .items_end()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(theme::text_muted())
                            .child(format::list_time_label(entry.updated_at, today, offset)),
                    )
                    .child(status_slot(id, unread, group, activity)),
            )
            .on_click(move |_, _, cx| panel.update(cx, |this, cx| this.open(id, cx)))
            .into_any_element()
    }
}

fn status_slot(
    id: i64,
    unread: bool,
    group: SharedString,
    activity: Entity<ActivityCenter>,
) -> Div {
    let slot = div()
        .h(px(MARK_READ_SIZE))
        .flex()
        .flex_none()
        .items_center()
        .justify_end();
    if !unread {
        return slot;
    }
    slot.child(
        div()
            .flex()
            .group_hover(group.clone(), |wrapper| wrapper.hidden())
            .child(dot(8.)),
    )
    .child(
        div()
            .id(ElementId::Name(format!("activity-mark-read-{id}").into()))
            .hidden()
            .group_hover(group, |button| button.flex())
            .size(px(MARK_READ_SIZE))
            .flex_none()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .border_1()
            .border_color(theme::border_strong())
            .bg(theme::surface_raised())
            .cursor_pointer()
            .child(icon(IconName::Check, 14., theme::text()))
            .tooltip(|window, cx| Tooltip::new("Mark as read").build(window, cx))
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                activity.update(cx, |activity, cx| activity.mark_read(id, cx));
            }),
    )
}

fn kind_marker(entry: &Entry) -> Div {
    let marker = div()
        .absolute()
        .right(px(-4.))
        .bottom(px(-4.))
        .size(px(KIND_MARKER_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .border_1()
        .border_color(theme::surface())
        .bg(theme::surface_raised())
        .text_size(px(10.));
    match entry.kind {
        Kind::Messages => marker.child(icon(IconName::MessageCircle, 10., theme::text_muted())),
        Kind::Mention => marker
            .text_color(theme::accent_text())
            .font_weight(FontWeight::BOLD)
            .child("@"),
        Kind::Reaction => marker.child(entry.glyphs.first().cloned().unwrap_or_default()),
    }
}

fn headline(entry: &Entry, sidebar: &Sidebar, resolved_names: &HashMap<String, String>) -> String {
    let names = entry.names(resolved_names);
    let verb = match entry.kind {
        Kind::Messages => names,
        Kind::Mention => format!("{names} mentioned you"),
        Kind::Reaction => format!("{names} reacted"),
    };
    match conversation_label(sidebar, &entry.conversation_id) {
        Some(label) => format!("{verb} in {label}"),
        None => verb,
    }
}

fn detail(entry: &Entry) -> String {
    match entry.kind {
        Kind::Messages if entry.count > 1 => {
            format!("{} new messages: {}", entry.count, entry.preview)
        }
        Kind::Reaction => format!("{} to: {}", entry.glyphs.join(" "), entry.preview),
        _ => entry.preview.clone(),
    }
}

fn conversation_label(sidebar: &Sidebar, conversation_id: &str) -> Option<String> {
    if let Some(chat) = sidebar.chats.iter().find(|chat| chat.id == conversation_id) {
        return (!is_one_on_one(chat)).then(|| chat_title(chat));
    }
    sidebar.teams.iter().find_map(|team| {
        team.channels
            .iter()
            .find(|channel| channel.id == conversation_id)
            .map(|channel| format!("{} > {}", team.team.name, channel.name))
    })
}

fn date_header(label: &'static str) -> AnyElement {
    div()
        .px(px(12.))
        .pt(px(8.))
        .pb(px(4.))
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text_muted())
        .child(label)
        .into_any_element()
}

impl Render for ActivityPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now: DateTime<Local> = Local::now();
        let (today, offset) = (now.date_naive(), now.offset().fix());
        let panel = cx.entity();
        let app = self.app.clone();
        let activity = self.activity.clone();
        let state = app.read(cx);
        let sections = activity
            .read(cx)
            .feed()
            .sections(self.filter, today, offset);
        let unresolved: Vec<String> = sections
            .iter()
            .flat_map(|section| section.entries.iter())
            .flat_map(|entry| entry.unresolved_user_ids())
            .filter(|user_id| !self.looked_up.contains(*user_id))
            .cloned()
            .collect();
        if !unresolved.is_empty() {
            self.looked_up.extend(unresolved.iter().cloned());
            self.resolved_names
                .extend(resolve_names(state, &unresolved));
        }
        let mut rows: Vec<AnyElement> = Vec::new();
        for section in sections {
            rows.push(date_header(section.bucket.label()));
            for entry in section.entries {
                rows.push(self.row(
                    entry,
                    &state.sidebar,
                    &state.directory,
                    today,
                    offset,
                    panel.clone(),
                ));
            }
        }
        if rows.is_empty() {
            rows.push(
                div()
                    .py(px(32.))
                    .flex()
                    .justify_center()
                    .text_size(px(13.))
                    .text_color(theme::text_muted())
                    .child(self.filter.empty_label())
                    .into_any_element(),
            );
        }
        let max_height = f32::from(window.viewport_size().height) * PANEL_MAX_HEIGHT_RATIO;
        div()
            .id("activity-layer")
            .absolute()
            .top(px(TITLE_BAR_HEIGHT))
            .left_0()
            .right_0()
            .bottom_0()
            .occlude()
            .flex()
            .justify_end()
            .items_start()
            .p(px(PANEL_MARGIN))
            .on_click(cx.listener(|_, _, _, cx| cx.emit(ActivityPanelEvent::Close)))
            .child(
                v_flex()
                    .id("activity-card")
                    .track_focus(&self.focus_handle)
                    .w(px(PANEL_WIDTH))
                    .max_h(px(max_height))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(theme::border_strong())
                    .bg(theme::surface())
                    .shadow_lg()
                    .overflow_hidden()
                    .occlude()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .on_key_down(cx.listener(Self::on_key_down))
                    .child(self.header(cx))
                    .child(
                        v_flex()
                            .id("activity-list")
                            .flex_1()
                            .min_h_0()
                            .py(px(4.))
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .children(rows),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use store::{ChatRecord, Sidebar};

    use super::{detail, headline};
    use crate::activity::{Actor, Entry, Kind};

    fn entry(kind: Kind, count: u32) -> Entry {
        Entry {
            id: 1,
            conversation_id: "chat".into(),
            kind,
            message_id: "m1".into(),
            actors: vec![Actor {
                user_id: Some("u1".into()),
                name: "Anna".into(),
            }],
            preview: "hello".into(),
            glyphs: vec!["A".into(), "B".into()],
            count,
            updated_at: Utc.with_ymd_and_hms(2026, 10, 8, 9, 0, 0).unwrap(),
            read: false,
        }
    }

    #[test]
    fn detail_follows_the_kind() {
        assert_eq!(detail(&entry(Kind::Messages, 1)), "hello");
        assert_eq!(detail(&entry(Kind::Messages, 3)), "3 new messages: hello");
        assert_eq!(detail(&entry(Kind::Mention, 1)), "hello");
        assert_eq!(detail(&entry(Kind::Reaction, 1)), "A B to: hello");
    }

    #[test]
    fn headline_names_the_actor_and_skips_direct_chat_context() {
        let sidebar = Sidebar {
            chats: vec![ChatRecord {
                id: "chat".into(),
                kind: "oneOnOne".into(),
                ..Default::default()
            }],
            teams: Vec::new(),
        };
        assert_eq!(
            headline(&entry(Kind::Messages, 1), &sidebar, &HashMap::new()),
            "Anna"
        );
        assert_eq!(
            headline(&entry(Kind::Reaction, 1), &sidebar, &HashMap::new()),
            "Anna reacted"
        );
        assert_eq!(
            headline(&entry(Kind::Mention, 1), &sidebar, &HashMap::new()),
            "Anna mentioned you"
        );
    }

    #[test]
    fn headline_resolves_a_persisted_unknown_actor() {
        let mut stale = entry(Kind::Reaction, 1);
        stale.actors[0].name = "Unknown".into();
        let sidebar = Sidebar::default();
        let resolved = HashMap::from([("u1".to_owned(), "Anna".to_owned())]);
        assert_eq!(headline(&stale, &sidebar, &resolved), "Anna reacted");
        assert_eq!(
            headline(&stale, &sidebar, &HashMap::new()),
            "Unknown reacted"
        );
    }
}
