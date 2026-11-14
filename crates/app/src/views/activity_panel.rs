use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;

use app_ui_kit::layout::SIDEBAR_WIDTH;
use chrono::{DateTime, Local, NaiveDate, TimeZone, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{h_flex, tooltip::Tooltip, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use store::Sidebar;

use super::avatar::person_avatar;
use super::scroll_edge::{scroll_offset, with_top_edge};
use super::sidebar::SIDEBAR_SCROLL_DIVIDER;
use super::widgets::{Corner, corner_badge, dot, icon, symbol};
use crate::activity::{ActivityCenter, Entry, Filter, Kind};
use crate::agent_api::{
    self,
    pending::{PendingKind, PendingProposal, conversation_label as proposal_conversation_label},
};
use crate::app_state::{AppState, chat_title};
use crate::chat_filter::{FEWER_FILTERS, MORE_FILTERS};
use crate::data::is_one_on_one;
use crate::format;
use crate::people::resolve_names;
use crate::theme;

const AVATAR_SIZE: f32 = 32.;
const KIND_MARKER_SIZE: f32 = 16.;
const BUTTON_SIZE: f32 = 32.;
const MARK_READ_SIZE: f32 = 20.;
const ROW_HEIGHT: f32 = 58.;
const EVENT_LINE_HEIGHT: f32 = 16.;
const HEADLINE_LINE_HEIGHT: f32 = 18.;
const ROW_CHROME_WIDTH: f32 = 12. + 20. + AVATAR_SIZE + 2. * 12. + 56.;
const RECAP_TILE_RADIUS: f32 = 6.;
const RECAP_GLYPH_SIZE: f32 = 20.;
const HEADER_HEIGHT: f32 = 30.;
const EMPTY_HEIGHT: f32 = 84.;
const FALLBACK_VIEWPORT: f32 = 900.;
const VIEWPORT_MARGIN: f32 = 300.;
const FILTER_KEY: &str = "activity_filter";
const PROPOSALS_LABEL: &str = "Agent proposals";
pub const ACTIVITY_PROPOSALS_TAG: &str = "shell/islands/sidebar/activity-proposals";

pub enum ActivityPanelEvent {
    OpenSettings,
    Open {
        conversation_id: String,
        message_id: String,
        link: Option<String>,
    },
}

pub struct ActivityPanel {
    app: Entity<AppState>,
    activity: Entity<ActivityCenter>,
    filter: Filter,
    more_filters_open: bool,
    scroll: ScrollHandle,
    resolved_names: HashMap<String, String>,
    looked_up: HashSet<String>,
    _observations: [Subscription; 2],
}

impl EventEmitter<ActivityPanelEvent> for ActivityPanel {}

impl ActivityPanel {
    pub fn activity(&self) -> Entity<ActivityCenter> {
        self.activity.clone()
    }

    pub fn new(
        app: Entity<AppState>,
        activity: Entity<ActivityCenter>,
        cx: &mut Context<Self>,
    ) -> Self {
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
            more_filters_open: false,
            scroll: ScrollHandle::new(),
            resolved_names: HashMap::new(),
            looked_up: HashSet::new(),
            _observations: observations,
        }
    }

    fn open(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(entry) = self.activity.read(cx).feed().get(id) else {
            return;
        };
        let stored_reply = entry.reply_id.clone().filter(|reply_id| {
            self.app
                .read(cx)
                .store
                .messages_by_id(&entry.conversation_id, std::slice::from_ref(reply_id))
                .is_ok_and(|found| !found.is_empty())
        });
        let event = ActivityPanelEvent::Open {
            conversation_id: entry.conversation_id.clone(),
            message_id: stored_reply.unwrap_or_else(|| entry.message_id.clone()),
            link: entry.link.clone(),
        };
        self.activity
            .update(cx, |activity, cx| activity.mark_read(id, cx));
        cx.emit(event);
    }

    fn filter_pill(&self, filter: Filter, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = filter == self.filter;
        div()
            .id(ElementId::Name(
                format!("activity-filter-{}", filter.label()).into(),
            ))
            .h(px(24.))
            .px(px(12.))
            .flex()
            .flex_none()
            .items_center()
            .rounded_full()
            .cursor_pointer()
            .text_size(px(12.))
            .when(selected, |pill| {
                pill.bg(theme::accent()).text_color(theme::on_accent())
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
    }

    fn more_filters_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.more_filters_open;
        let label = if open { FEWER_FILTERS } else { MORE_FILTERS };
        div()
            .id("activity-filter-more")
            .size(px(24.))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .bg(theme::surface_raised())
            .hover(|pill| pill.bg(theme::border_strong()))
            .tooltip(move |window, cx| Tooltip::new(label).build(window, cx))
            .child(icon(
                if open {
                    IconName::ChevronUp
                } else {
                    IconName::ChevronDown
                },
                14.,
                theme::text_soft(),
            ))
            .on_click(cx.listener(|this, _, _, cx| {
                this.more_filters_open = !this.more_filters_open;
                cx.notify();
            }))
    }

    fn header(&self, cx: &mut Context<Self>) -> Div {
        let overflow_active = Filter::OVERFLOW.contains(&self.filter);
        let mut pills: Vec<AnyElement> = Filter::PRIMARY
            .into_iter()
            .map(|filter| self.filter_pill(filter, cx).into_any_element())
            .collect();
        if self.more_filters_open || overflow_active {
            pills.extend(
                Filter::OVERFLOW
                    .into_iter()
                    .map(|filter| self.filter_pill(filter, cx).into_any_element()),
            );
        }
        if !overflow_active {
            pills.push(self.more_filters_button(cx).into_any_element());
        }
        v_flex()
            .flex_none()
            .child(
                h_flex()
                    .h(px(52.))
                    .pl(px(16.))
                    .pr(px(12.))
                    .gap(px(4.))
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(16.))
                            .font_weight(FontWeight::BOLD)
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
                            .rounded(px(8.))
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
                            .rounded(px(8.))
                            .cursor_pointer()
                            .hover(|button| button.bg(theme::surface_raised()))
                            .child(icon(IconName::Settings, 16., theme::text_muted()))
                            .on_click(cx.listener(|_, _, _, cx| {
                                cx.emit(ActivityPanelEvent::OpenSettings);
                            })),
                    ),
            )
            .child(
                h_flex()
                    .px(px(16.))
                    .pb(px(12.))
                    .flex_wrap()
                    .gap(px(6.))
                    .children(pills),
            )
    }

    fn row(
        &self,
        entry: &Entry,
        text: &RowText,
        sidebar: &Sidebar,
        directory: &crate::data::Directory,
        today: NaiveDate,
        offset: app_base::format::Zone,
        panel: Entity<ActivityPanel>,
    ) -> AnyElement {
        let activity = self.activity.clone();
        let id = entry.id.clone();
        let actor = entry.latest_actor();
        let avatar = if is_recap(entry) {
            recap_tile().into_any_element()
        } else {
            person_avatar(
                directory,
                actor.and_then(|actor| actor.user_id.as_deref()),
                actor.map_or("", |actor| actor.display_name(&self.resolved_names)),
                AVATAR_SIZE,
            )
        };
        let group = SharedString::from(format!("activity-row-group-{id}"));
        row_frame(RowParts {
            element_id: format!("activity-row-{id}"),
            avatar: avatar.into_any_element(),
            marker: kind_marker(entry, group.clone()),
            group,
            headline: headline(entry, sidebar, &self.resolved_names),
            headline_wraps: text.headline_wraps,
            detail: detail(entry),
            lines: text.lines.clone(),
            unread: !entry.read,
            time: format::list_time_label(entry.updated_at, today, offset),
            mark_read: Rc::new({
                let id = id.clone();
                move |cx: &mut App| activity.update(cx, |activity, cx| activity.mark_read(&id, cx))
            }),
        })
        .on_click(move |_, _, cx| panel.update(cx, |this, cx| this.open(&id, cx)))
        .into_any_element()
    }

    fn proposal_row(
        &self,
        pending: &PendingProposal,
        sidebar: &Sidebar,
        today: NaiveDate,
        offset: app_base::format::Zone,
        panel: Entity<ActivityPanel>,
    ) -> AnyElement {
        let group = SharedString::from(format!("activity-proposal-group-{}", pending.id));
        let label = proposal_conversation_label(sidebar, &pending.conversation_id);
        let proposal_id = pending.id.clone();
        let opened = pending.clone();
        row_frame(RowParts {
            element_id: format!("activity-proposal-{}", pending.id),
            avatar: agent_avatar().into_any_element(),
            marker: proposal_marker(pending, group.clone()),
            group,
            headline: pending.headline(label.as_deref()),
            headline_wraps: false,
            detail: pending.detail(),
            lines: Vec::new(),
            unread: !pending.seen,
            time: format::list_time_label(pending.created_at, today, offset),
            mark_read: Rc::new(move |cx: &mut App| agent_api::mark_proposal_seen(&proposal_id, cx)),
        })
        .on_click(move |_, _, cx| panel.update(cx, |this, cx| this.open_proposal(&opened, cx)))
        .into_any_element()
    }

    fn open_proposal(&mut self, pending: &PendingProposal, cx: &mut Context<Self>) {
        agent_api::mark_proposal_seen(&pending.id, cx);
        cx.emit(ActivityPanelEvent::Open {
            conversation_id: pending.conversation_id.clone(),
            message_id: pending.jump_message_id().to_owned(),
            link: None,
        });
    }
}

struct RowParts {
    element_id: String,
    group: SharedString,
    avatar: AnyElement,
    marker: Div,
    headline: String,
    headline_wraps: bool,
    detail: String,
    lines: Vec<String>,
    unread: bool,
    time: String,
    mark_read: MarkRead,
}

type MarkRead = Rc<dyn Fn(&mut App)>;

fn row_frame(parts: RowParts) -> Stateful<Div> {
    let RowParts {
        element_id,
        group,
        avatar,
        marker,
        headline,
        headline_wraps,
        detail,
        lines,
        unread,
        time,
        mark_read,
    } = parts;
    let height = row_height(headline_wraps, lines.len());
    let selector = element_id.clone();
    h_flex()
        .id(ElementId::Name(element_id.clone().into()))
        .debug_selector(move || selector)
        .group(group.clone())
        .h(px(height))
        .flex_none()
        .mx(px(6.))
        .px(px(10.))
        .py(px(8.))
        .gap(px(12.))
        .items_start()
        .rounded(px(8.))
        .cursor_pointer()
        .hover(|row| row.bg(theme::row_hover()))
        .child(
            div()
                .relative()
                .flex_none()
                .size(px(AVATAR_SIZE))
                .child(avatar)
                .child(marker),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(2.))
                .child(
                    div()
                        .when(headline_wraps, |headline| {
                            headline.line_clamp(2).text_ellipsis()
                        })
                        .when(!headline_wraps, |headline| headline.truncate())
                        .text_size(px(13.))
                        .line_height(px(HEADLINE_LINE_HEIGHT))
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
                        .child(headline),
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
                        .child(detail),
                )
                .children(lines.into_iter().map(|line| {
                    div()
                        .truncate()
                        .text_size(px(12.))
                        .text_color(theme::text_muted())
                        .child(line)
                })),
        )
        .child(
            v_flex()
                .flex_none()
                .items_end()
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(theme::text_muted())
                        .child(time),
                )
                .child(status_slot(element_id, unread, group, mark_read)),
        )
}

fn agent_avatar() -> Div {
    div()
        .size(px(AVATAR_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(theme::accent_soft())
        .child(icon(IconName::Bot, 16., theme::accent_text()))
}

fn admits_proposal(filter: Filter, pending: &PendingProposal) -> bool {
    match filter {
        Filter::All => true,
        Filter::Mentions => false,
        Filter::Unread => !pending.seen,
        Filter::Replies | Filter::Reactions | Filter::MissedCalls => false,
    }
}

fn status_slot(element_id: String, unread: bool, group: SharedString, mark_read: MarkRead) -> Div {
    let slot = div()
        .h(px(MARK_READ_SIZE))
        .flex()
        .flex_none()
        .items_center()
        .justify_end();
    if !unread {
        return slot;
    }
    // Toggle visibility, never display: gpui reads hover in prepaint and paint from different frames.
    slot.relative()
        .w(px(MARK_READ_SIZE))
        .child(
            div()
                .flex()
                .group_hover(group.clone(), |wrapper| wrapper.invisible())
                .mr(px(8.))
                .child(dot(8.)),
        )
        .child(
            div()
                .id(ElementId::Name(format!("{element_id}-mark-read").into()))
                .absolute()
                .top_0()
                .right_0()
                .invisible()
                .group_hover(group, |button| button.visible())
                .size(px(MARK_READ_SIZE))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .bg(theme::surface_raised())
                .cursor_pointer()
                .child(icon(IconName::Check, 12., theme::text()))
                .tooltip(|window, cx| Tooltip::new("Mark as read").build(window, cx))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    mark_read(cx);
                }),
        )
}

fn marker_frame(row_group: SharedString) -> Div {
    corner_badge(Corner::BottomRight, 4.)
        .size(px(KIND_MARKER_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .border_2()
        .border_color(theme::surface())
        .group_hover(row_group, |marker| marker.border_color(theme::row_hover()))
        .bg(theme::surface_raised())
        .text_size(px(10.))
}

fn proposal_marker(pending: &PendingProposal, row_group: SharedString) -> Div {
    let marker = marker_frame(row_group);
    match &pending.kind {
        PendingKind::Reply { .. } => {
            marker.child(icon(IconName::MessageCircle, 10., theme::text_muted()))
        }
        PendingKind::Reaction { glyph, .. } => marker
            .font_family(theme::EMOJI_FONT_FAMILY)
            .child(glyph.clone()),
    }
}

fn kind_marker(entry: &Entry, row_group: SharedString) -> Div {
    let marker = marker_frame(row_group);
    let muted_icon = |name: IconName| icon(name, 10., theme::text_muted());
    match entry.kind {
        Kind::Mention => marker
            .text_color(theme::accent_text())
            .font_weight(FontWeight::BOLD)
            .child("@"),
        Kind::Reaction => match entry.glyphs.first() {
            Some(glyph) => marker
                .font_family(theme::EMOJI_FONT_FAMILY)
                .child(glyph.clone()),
            None => marker.child(muted_icon(IconName::Heart)),
        },
        Kind::MissedCall => marker.child(icon(IconName::PhoneOff, 10., theme::red())),
        Kind::Reply => marker.child(muted_icon(IconName::Reply)),
        Kind::Call => marker.child(muted_icon(IconName::Phone)),
        Kind::Posted => marker.child(muted_icon(IconName::MessageSquare)),
        Kind::Team => marker.child(muted_icon(IconName::Users)),
        Kind::App => marker.child(muted_icon(IconName::AppWindow)),
        Kind::Other => marker.child(muted_icon(IconName::Bell)),
    }
}

fn headline(entry: &Entry, sidebar: &Sidebar, resolved_names: &HashMap<String, String>) -> String {
    let actor = entry
        .latest_actor()
        .map(|actor| actor.display_name(resolved_names).to_owned())
        .unwrap_or_default();
    let names = if entry.count > 1 && matches!(entry.kind, Kind::Reaction | Kind::Reply) {
        format!("{actor} +{}", entry.count - 1)
    } else {
        actor.clone()
    };
    let channel = channel_of(sidebar, &entry.conversation_id);
    let verb = match (entry.kind, entry.subtype.as_str()) {
        (Kind::Mention, "everyone") => format!("{names} mentioned Everyone"),
        (Kind::Mention, "channel") => match channel {
            Some((_, channel_name)) => format!("{names} mentioned {channel_name}"),
            None => format!("{names} mentioned the channel"),
        },
        (Kind::Mention, "team") => match channel {
            Some((team_name, _)) => format!("{names} mentioned {team_name}"),
            None => format!("{names} mentioned the team"),
        },
        (Kind::Mention, _) => format!("{names} mentioned you"),
        (Kind::Reply, _) => reply_headline(entry, &names),
        (Kind::Reaction, _) => format!("{names} reacted to your message"),
        (Kind::MissedCall, _) => format!("Missed call from {actor}"),
        (Kind::Call, "voicemail") => format!("Voicemail from {actor}"),
        (Kind::Call, "missedMeetingNudge") => format!("{actor} asked you to join a meeting"),
        (Kind::Call, _) => actor,
        (Kind::Posted, "channelReplyMessage") => format!("{names} replied"),
        (Kind::Posted, _) => format!("{names} posted a new message"),
        (Kind::Team, "promotedToTeamAdmin") => {
            return format!(
                "{actor} made you an owner of {}",
                team_label(entry, channel)
            );
        }
        (Kind::Team, _) => {
            return format!("{actor} added you to {}", team_label(entry, channel));
        }
        (Kind::App, _) => match &entry.title {
            Some(title) => return title.clone(),
            None => entry.source_label.clone().unwrap_or(actor),
        },
        (Kind::Other, _) => actor,
    };
    verb
}

fn event_line<Zone: TimeZone>(start: DateTime<Utc>, end: DateTime<Utc>, zone: &Zone) -> String
where
    Zone::Offset: std::fmt::Display,
{
    let (start, end) = (start.with_timezone(zone), end.with_timezone(zone));
    let clock = crate::format::clock_format();
    let dated = format!("%b %-d, {clock}");
    let end_format = if end.date_naive() == start.date_naive() {
        clock
    } else {
        dated.as_str()
    };
    format!("{} - {}", start.format(&dated), end.format(end_format))
}

fn reply_headline(entry: &Entry, names: &str) -> String {
    match entry.activity_type.as_str() {
        "reply" => format!("{names} replied to your post"),
        "replyInChat" => format!("{names} replied to message"),
        _ => format!("{names} replied"),
    }
}

fn team_label(entry: &Entry, channel: Option<(&str, &str)>) -> String {
    channel
        .map(|(team_name, _)| team_name.to_owned())
        .or_else(|| entry.source_label.clone())
        .unwrap_or_else(|| "a team".to_owned())
}

fn channel_of<'a>(sidebar: &'a Sidebar, conversation_id: &str) -> Option<(&'a str, &'a str)> {
    sidebar.teams.iter().find_map(|team| {
        team.channels
            .iter()
            .find(|channel| channel.id == conversation_id)
            .map(|channel| (team.team.name.as_str(), channel.name.as_str()))
    })
}

fn detail(entry: &Entry) -> String {
    match entry.kind {
        Kind::Reaction if !entry.glyphs.is_empty() => {
            format!("{} to: {}", entry.glyphs.join(" "), entry.preview)
        }
        Kind::MissedCall if entry.count > 1 => format!("{} missed calls", entry.count),
        _ => entry.preview.clone(),
    }
}

fn conversation_label(sidebar: &Sidebar, conversation_id: &str) -> Option<String> {
    if let Some(chat) = sidebar.chats.iter().find(|chat| chat.id == conversation_id) {
        return match (is_one_on_one(chat), chat.title.trim().is_empty()) {
            (true, _) => None,
            (false, true) => Some(format!("Chat with {}", chat_title(chat))),
            (false, false) => Some(chat_title(chat)),
        };
    }
    sidebar.teams.iter().find_map(|team| {
        team.channels
            .iter()
            .find(|channel| channel.id == conversation_id)
            .map(|channel| format!("{} > {}", team.team.name, channel.name))
    })
}

fn group_header(label: &'static str) -> Div {
    div()
        .h(px(HEADER_HEIGHT - 4.))
        .mt(px(4.))
        .px(px(16.))
        .flex()
        .flex_none()
        .items_center()
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text_muted())
        .child(label)
}

struct RowText {
    headline_wraps: bool,
    lines: Vec<String>,
}

impl RowText {
    fn new(entry: &Entry, sidebar: &Sidebar, headline_wraps: bool) -> Self {
        let lines = entry
            .event_range
            .map(|(start, end)| event_line(start, end, &Local))
            .into_iter()
            .chain(location_line(entry, sidebar))
            .collect();
        Self {
            headline_wraps,
            lines,
        }
    }

    fn height(&self) -> f32 {
        row_height(self.headline_wraps, self.lines.len())
    }
}

fn row_height(headline_wraps: bool, extra_lines: usize) -> f32 {
    let headline_extra = if headline_wraps {
        HEADLINE_LINE_HEIGHT
    } else {
        0.
    };
    ROW_HEIGHT + headline_extra + extra_lines as f32 * EVENT_LINE_HEIGHT
}

fn is_recap(entry: &Entry) -> bool {
    entry.kind == Kind::App && entry.subtype.starts_with("recapIsReady")
}

fn recap_tile() -> Div {
    div()
        .size(px(AVATAR_SIZE))
        .flex_none()
        .rounded(px(RECAP_TILE_RADIUS))
        .bg(theme::accent())
        .flex()
        .items_center()
        .justify_center()
        .child(symbol("recap", RECAP_GLYPH_SIZE, theme::white()))
}

fn headline_wraps(headline: String, text_width: f32, window: &mut Window) -> bool {
    if text_width <= 0. {
        return false;
    }
    let mut run = window.text_style().to_run(headline.len());
    run.font.weight = FontWeight::SEMIBOLD;
    let width = window
        .text_system()
        .shape_line(headline.into(), px(13.), &[run], None)
        .width;
    f32::from(width) > text_width
}

fn location_line(entry: &Entry, sidebar: &Sidebar) -> Option<String> {
    match entry.kind {
        Kind::Mention | Kind::Reply | Kind::Reaction | Kind::Posted => {
            conversation_label(sidebar, &entry.conversation_id)
        }
        _ => None,
    }
}

enum Slot<'a> {
    Proposals(Vec<PendingProposal>),
    Header(&'static str),
    Row(&'a Entry, RowText),
    Empty,
}

impl Slot<'_> {
    fn height(&self) -> f32 {
        match self {
            Slot::Proposals(proposals) => HEADER_HEIGHT + proposals.len() as f32 * ROW_HEIGHT,
            Slot::Header(_) => HEADER_HEIGHT,
            Slot::Row(_, text) => text.height(),
            Slot::Empty => EMPTY_HEIGHT,
        }
    }
}

#[derive(Debug, PartialEq)]
struct VisibleSlots {
    range: Range<usize>,
    before: f32,
    after: f32,
}

fn visible_slots(heights: &[f32], from: f32, to: f32) -> VisibleSlots {
    let mut position = 0.;
    let mut first = heights.len();
    let mut last = heights.len();
    let mut before = 0.;
    for (index, height) in heights.iter().enumerate() {
        let end = position + height;
        if first == heights.len() && end >= from {
            first = index;
            before = position;
        }
        if position > to {
            last = index;
            break;
        }
        position = end;
    }
    if first == heights.len() {
        before = position;
    }
    let last = last.max(first);
    let after = heights[last..].iter().sum();
    VisibleSlots {
        range: first..last,
        before,
        after,
    }
}

impl Render for ActivityPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now: DateTime<Local> = Local::now();
        let (today, offset) = (now.date_naive(), app_base::format::Zone::Local);
        let panel = cx.entity();
        let app = self.app.clone();
        let activity = self.activity.clone();
        let state = app.read(cx);
        let sections = activity
            .read(cx)
            .feed()
            .sections(self.filter, today, offset);
        let measured_width = f32::from(self.scroll.bounds().size.width);
        let list_width = if measured_width > 1. {
            measured_width
        } else {
            SIDEBAR_WIDTH
        };
        let text_width = list_width - ROW_CHROME_WIDTH;
        let mut slots: Vec<Slot> = Vec::new();
        let proposals: Vec<PendingProposal> = agent_api::pending_proposals(cx)
            .into_iter()
            .filter(|pending| admits_proposal(self.filter, pending))
            .collect();
        if !proposals.is_empty() {
            slots.push(Slot::Proposals(proposals));
        }
        for section in &sections {
            slots.push(Slot::Header(section.bucket.label()));
            slots.extend(section.entries.iter().map(|entry| {
                let text = headline(entry, &state.sidebar, &self.resolved_names);
                let wraps = headline_wraps(text, text_width, window);
                Slot::Row(entry, RowText::new(entry, &state.sidebar, wraps))
            }));
        }
        if slots.is_empty() {
            slots.push(Slot::Empty);
        }
        let heights: Vec<f32> = slots.iter().map(Slot::height).collect();
        let top = -f32::from(scroll_offset(&self.scroll).y);
        let measured = f32::from(self.scroll.bounds().size.height);
        let height = if measured > 1. {
            measured
        } else {
            f32::from(window.viewport_size().height).max(FALLBACK_VIEWPORT)
        };
        let visible = visible_slots(
            &heights,
            top - VIEWPORT_MARGIN,
            top + height + VIEWPORT_MARGIN,
        );
        let visible_slots = &slots[visible.range.clone()];
        let unresolved: Vec<String> = visible_slots
            .iter()
            .filter_map(|slot| match slot {
                Slot::Row(entry, _) => Some(entry.unresolved_user_ids()),
                _ => None,
            })
            .flatten()
            .filter(|user_id| !self.looked_up.contains(*user_id))
            .cloned()
            .collect();
        if !unresolved.is_empty() {
            self.looked_up.extend(unresolved.iter().cloned());
            self.resolved_names
                .extend(resolve_names(state, &unresolved));
        }
        let mut rows: Vec<AnyElement> = Vec::new();
        if visible.before > 0. {
            rows.push(div().h(px(visible.before)).flex_none().into_any_element());
        }
        for slot in visible_slots {
            rows.push(match slot {
                Slot::Proposals(proposals) => v_flex()
                    .debug_selector(|| ACTIVITY_PROPOSALS_TAG.into())
                    .child(group_header(PROPOSALS_LABEL))
                    .children(proposals.iter().map(|pending| {
                        self.proposal_row(pending, &state.sidebar, today, offset, panel.clone())
                    }))
                    .into_any_element(),
                Slot::Header(label) => group_header(label).into_any_element(),
                Slot::Row(entry, text) => self.row(
                    entry,
                    text,
                    &state.sidebar,
                    &state.directory,
                    today,
                    offset,
                    panel.clone(),
                ),
                Slot::Empty => div()
                    .h(px(EMPTY_HEIGHT))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .text_size(px(13.))
                    .text_color(theme::text_muted())
                    .child(self.filter.empty_label())
                    .into_any_element(),
            });
        }
        if visible.after > 0. {
            rows.push(div().h(px(visible.after)).flex_none().into_any_element());
        }
        let list = v_flex()
            .id("activity-list")
            .flex_1()
            .min_h_0()
            .pb(px(12.))
            .overflow_y_scrollbar()
            .track_scroll(&self.scroll)
            .children(rows);
        v_flex()
            .flex_1()
            .min_h_0()
            .child(self.header(cx))
            .child(with_top_edge(list, &self.scroll, SIDEBAR_SCROLL_DIVIDER))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use chrono::{Local, TimeZone, Utc};
    use gpui_kit::{AppContext as _, Modifiers, TestAppContext};
    use store::{ChatRecord, Sidebar, Store};

    use super::{
        ActivityPanel, ROW_HEIGHT, VIEWPORT_MARGIN, VisibleSlots, detail, headline, location_line,
        visible_slots,
    };
    use crate::activity::{ActivityCenter, Actor, Entry, Filter, Kind};
    use crate::app_state::{AppState, Mode};

    fn entry(kind: Kind, count: u32) -> Entry {
        Entry {
            id: "1".into(),
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
            ..Entry::default()
        }
    }

    fn with(kind: Kind, activity_type: &str, subtype: &str, count: u32) -> Entry {
        Entry {
            activity_type: activity_type.into(),
            subtype: subtype.into(),
            ..entry(kind, count)
        }
    }

    #[test]
    fn only_rows_near_the_viewport_are_built() {
        let heights = vec![ROW_HEIGHT; 300];
        let viewport = 900.;
        let at_top = visible_slots(&heights, -VIEWPORT_MARGIN, viewport + VIEWPORT_MARGIN);
        assert_eq!(
            at_top,
            VisibleSlots {
                range: 0..21,
                before: 0.,
                after: 279. * ROW_HEIGHT,
            }
        );
        let scrolled = visible_slots(
            &heights,
            5000. - VIEWPORT_MARGIN,
            5000. + viewport + VIEWPORT_MARGIN,
        );
        assert_eq!(scrolled.range, 81..107);
        assert_eq!(scrolled.before, 81. * ROW_HEIGHT);
        assert_eq!(scrolled.after, 193. * ROW_HEIGHT);
        let past_the_end = visible_slots(&heights, 90_000., 91_000.);
        assert_eq!(past_the_end.range, 300..300);
        assert_eq!(past_the_end.before, 300. * ROW_HEIGHT);
        assert_eq!(past_the_end.after, 0.);
    }

    #[test]
    fn detail_follows_the_kind() {
        assert_eq!(detail(&entry(Kind::Mention, 1)), "hello");
        assert_eq!(detail(&entry(Kind::Reaction, 1)), "A B to: hello");
        assert_eq!(detail(&entry(Kind::MissedCall, 3)), "3 missed calls");
        assert_eq!(detail(&entry(Kind::Reply, 2)), "hello");
    }

    #[test]
    fn event_line_shows_the_meeting_time_in_local_time() {
        let start = chrono::DateTime::from_timestamp_millis(1793865600000).unwrap();
        let end = chrono::DateTime::from_timestamp_millis(1793869200000).unwrap();
        let berlin = chrono::FixedOffset::east_opt(3600).unwrap();
        assert_eq!(
            super::event_line(start, end, &berlin),
            "Nov 5, 09:00 - 10:00"
        );
        let next_day = chrono::DateTime::from_timestamp_millis(1793955600000).unwrap();
        assert_eq!(
            super::event_line(start, next_day, &berlin),
            "Nov 5, 09:00 - Nov 6, 10:00"
        );
    }

    #[test]
    fn headline_uses_the_teams_activity_texts() {
        let sidebar = Sidebar {
            chats: vec![ChatRecord {
                id: "chat".into(),
                kind: "oneOnOne".into(),
                ..Default::default()
            }],
            teams: Vec::new(),
        };
        let cases = [
            (
                with(Kind::Mention, "mentionInChat", "person", 1),
                "Anna mentioned you",
            ),
            (
                with(Kind::Mention, "mentionInChat", "everyone", 1),
                "Anna mentioned Everyone",
            ),
            (
                with(Kind::Reaction, "reactionInChat", "like", 1),
                "Anna reacted to your message",
            ),
            (
                with(Kind::Reaction, "reactionInChat", "like", 3),
                "Anna +2 reacted to your message",
            ),
            (
                with(Kind::Reply, "reply", "", 1),
                "Anna replied to your post",
            ),
            (with(Kind::Reply, "replyToReply", "", 1), "Anna replied"),
            (
                with(Kind::MissedCall, "call", "missedCall", 1),
                "Missed call from Anna",
            ),
            (
                with(Kind::Call, "call", "missedMeetingNudge", 1),
                "Anna asked you to join a meeting",
            ),
            (
                with(Kind::Posted, "follow", "channelNewMessage", 1),
                "Anna posted a new message",
            ),
        ];
        for (entry, expected) in cases {
            assert_eq!(headline(&entry, &sidebar, &HashMap::new()), expected);
        }
    }

    #[test]
    fn headline_names_channel_and_team_mentions() {
        let sidebar = Sidebar {
            chats: Vec::new(),
            teams: vec![store::SidebarTeam {
                rights: Default::default(),
                team: store::TeamRecord {
                    id: "team".into(),
                    name: "Platform".into(),
                },
                channels: vec![store::ChannelRecord {
                    id: "chat".into(),
                    team_id: "team".into(),
                    name: "Releases".into(),
                    membership_type: None,
                    last_message_at: None,
                    unread: false,
                }],
                hidden: false,
                hidden_channel_ids: Vec::new(),
                notifications: Default::default(),
                mentions: Default::default(),
            }],
        };
        assert_eq!(
            headline(
                &with(Kind::Mention, "mention", "channel", 1),
                &sidebar,
                &HashMap::new()
            ),
            "Anna mentioned Releases"
        );
        assert_eq!(
            location_line(&with(Kind::Mention, "mention", "channel", 1), &sidebar).as_deref(),
            Some("Platform > Releases")
        );
        assert_eq!(
            headline(
                &with(Kind::Mention, "mention", "team", 1),
                &sidebar,
                &HashMap::new()
            ),
            "Anna mentioned Platform"
        );
        assert_eq!(
            headline(
                &with(Kind::Team, "teamMembershipChange", "addedToTeam", 1),
                &sidebar,
                &HashMap::new()
            ),
            "Anna added you to Platform"
        );
    }

    #[test]
    fn headline_resolves_an_unknown_actor() {
        let mut stale = with(Kind::Reaction, "reactionInChat", "like", 1);
        stale.actors[0].name = "Unknown".into();
        let sidebar = Sidebar::default();
        let resolved = HashMap::from([("u1".to_owned(), "Anna".to_owned())]);
        assert_eq!(
            headline(&stale, &sidebar, &resolved),
            "Anna reacted to your message"
        );
    }

    #[gpui_kit::test]
    fn row_sliding_under_a_resting_mouse_draws(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let store = Arc::new(Store::open_in_memory().unwrap());
        crate::demo::seed(&store);
        let mode = Mode {
            demo: true,
            read_only: false,
            demo_sync: None,
        };
        let app = cx.update(|cx| {
            cx.new(|_| {
                let mut state = AppState::new(store, mode);
                crate::demo::seed_directory(&mut state);
                state
            })
        });
        let (panel, cx) = cx.add_window_view(|window, cx| {
            let activity = cx.new(|cx| ActivityCenter::new(app.clone(), window, cx));
            let mut panel = ActivityPanel::new(app.clone(), activity, cx);
            panel.filter = Filter::Unread;
            panel
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let activity = cx.update(|_, cx| panel.read(cx).activity.clone());
        let first_unread = cx.update(|_, cx| {
            let now = Local::now();
            activity.read(cx).feed().sections(
                Filter::Unread,
                now.date_naive(),
                app_base::format::Zone::Local,
            )[0]
            .entries[0]
                .id
                .clone()
        });
        let selector: &'static str = format!("activity-row-{first_unread}").leak();
        let first_row = cx.debug_bounds(selector).unwrap();
        cx.simulate_mouse_move(first_row.center(), None, Modifiers::none());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.update(|_, cx| {
            activity.update(cx, |activity, cx| activity.mark_read(&first_unread, cx))
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}
