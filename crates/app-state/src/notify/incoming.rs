use std::collections::{HashMap, HashSet, VecDeque};

use chrono::{DateTime, Utc};
use store::{ChannelNotifications, MessageRecord, Sidebar, Store};

use super::rules::{ChannelSignals, ChatKind, Incoming, Preview};
use crate::app_state::chat_title;

const RECENT_WINDOW: usize = 20;
const FILE_FALLBACK: &str = "File";
const LATE_ARRIVALS_KEPT: usize = 500;

#[derive(Default)]
pub struct LateArrivals {
    keys: HashSet<(String, String)>,
    order: VecDeque<(String, String)>,
}

impl LateArrivals {
    pub fn insert(&mut self, conversation_id: &str, message_ids: Vec<String>) {
        for message_id in message_ids {
            let key = (conversation_id.to_owned(), message_id);
            if self.keys.insert(key.clone()) {
                self.order.push_back(key);
            }
        }
        while self.order.len() > LATE_ARRIVALS_KEPT {
            if let Some(oldest) = self.order.pop_front() {
                self.keys.remove(&oldest);
            }
        }
    }

    pub fn contains(&self, conversation_id: &str, message_id: &str) -> bool {
        self.keys
            .contains(&(conversation_id.to_owned(), message_id.to_owned()))
    }
}

pub struct IncomingTracker {
    started: DateTime<Utc>,
    watermarks: HashMap<String, DateTime<Utc>>,
}

impl IncomingTracker {
    pub fn new(started: DateTime<Utc>) -> Self {
        IncomingTracker {
            started,
            watermarks: HashMap::new(),
        }
    }

    pub fn collect(
        &mut self,
        store: &Store,
        sidebar: &Sidebar,
        my_user_id: Option<&str>,
        late_arrivals: &LateArrivals,
        conversation_id: &str,
    ) -> Vec<Incoming> {
        let Ok(records) = store.messages(conversation_id, None, RECENT_WINDOW) else {
            return Vec::new();
        };
        let records = records
            .into_iter()
            .filter(|record| !late_arrivals.contains(&record.conversation_id, &record.message_id))
            .collect();
        self.fresh(records, store, sidebar, my_user_id)
    }

    fn fresh(
        &mut self,
        records: Vec<MessageRecord>,
        store: &Store,
        sidebar: &Sidebar,
        my_user_id: Option<&str>,
    ) -> Vec<Incoming> {
        let mut incoming = Vec::new();
        let newest_own = newest_own_message(&records, my_user_id);
        for record in records {
            let watermark = self
                .watermarks
                .get(&record.conversation_id)
                .copied()
                .unwrap_or(self.started)
                .max(self.started);
            if record.created_at <= watermark {
                continue;
            }
            self.watermarks
                .insert(record.conversation_id.clone(), record.created_at);
            if record.deleted
                || record.call_event_json.is_some()
                || is_own(&record, my_user_id)
                || seen_elsewhere(&record, sidebar, my_user_id, &newest_own)
            {
                continue;
            }
            if let Some(entry) = build_incoming(&record, store, sidebar, my_user_id) {
                incoming.push(entry);
            }
        }
        incoming
    }
}

fn is_own(record: &MessageRecord, my_user_id: Option<&str>) -> bool {
    matches!((record.sender_id.as_deref(), my_user_id), (Some(sender), Some(me)) if sender == me)
}

fn newest_own_message(
    records: &[MessageRecord],
    my_user_id: Option<&str>,
) -> HashMap<String, DateTime<Utc>> {
    let mut newest = HashMap::new();
    for record in records.iter().filter(|record| is_own(record, my_user_id)) {
        let entry = newest
            .entry(record.conversation_id.clone())
            .or_insert(record.created_at);
        *entry = (*entry).max(record.created_at);
    }
    newest
}

fn seen_elsewhere(
    record: &MessageRecord,
    sidebar: &Sidebar,
    my_user_id: Option<&str>,
    newest_own: &HashMap<String, DateTime<Utc>>,
) -> bool {
    let read_on_another_device = sidebar
        .chats
        .iter()
        .find(|chat| chat.id == record.conversation_id)
        .and_then(|chat| chat.last_read_at)
        .is_some_and(|read_at| read_at >= record.created_at);
    let answered_later = newest_own
        .get(&record.conversation_id)
        .is_some_and(|own_at| *own_at >= record.created_at);
    read_on_another_device || answered_later || reacted_by_me(record, my_user_id)
}

fn reacted_by_me(record: &MessageRecord, my_user_id: Option<&str>) -> bool {
    let Some(me) = my_user_id else {
        return false;
    };
    teams_core::reactions(record)
        .iter()
        .any(|reaction| reaction.user_id.as_deref() == Some(me))
}

pub fn build_incoming(
    record: &MessageRecord,
    store: &Store,
    sidebar: &Sidebar,
    my_user_id: Option<&str>,
) -> Option<Incoming> {
    let (kind, chat_name) = chat_context(sidebar, &record.conversation_id)?;
    let signals = match kind {
        ChatKind::Channel { .. } => channel_signals(record, store, sidebar, my_user_id),
        _ => ChannelSignals::default(),
    };
    Some(Incoming {
        conversation_id: record.conversation_id.clone(),
        message_id: record.message_id.clone(),
        kind,
        chat_title: chat_name,
        sender_id: record.sender_id.clone(),
        sender_name: record.sender_name.clone().unwrap_or_default(),
        preview: preview_of(record),
        mentions_me: mentions_me(record, my_user_id),
        muted: is_muted_chat(sidebar, &record.conversation_id),
        signals,
        created_at: record.created_at,
    })
}

fn is_muted_chat(sidebar: &Sidebar, conversation_id: &str) -> bool {
    sidebar
        .chats
        .iter()
        .any(|chat| chat.id == conversation_id && chat.muted)
}

fn chat_context(sidebar: &Sidebar, conversation_id: &str) -> Option<(ChatKind, String)> {
    if let Some(chat) = sidebar.chats.iter().find(|chat| chat.id == conversation_id) {
        let kind = if chat.kind.eq_ignore_ascii_case("oneOnOne") {
            ChatKind::Direct
        } else {
            ChatKind::Group {
                member_count: chat.members.len().max(2),
            }
        };
        return Some((kind, chat_title(chat)));
    }
    sidebar.teams.iter().find_map(|team| {
        team.channels
            .iter()
            .find(|channel| channel.id == conversation_id)
            .map(|channel| {
                (
                    ChatKind::Channel {
                        team: team.team.name.clone(),
                        channel: channel.name.clone(),
                    },
                    channel.name.clone(),
                )
            })
    })
}

fn channel_signals(
    record: &MessageRecord,
    store: &Store,
    sidebar: &Sidebar,
    my_user_id: Option<&str>,
) -> ChannelSignals {
    let in_my_thread = match (record.reply_to_id.as_deref(), my_user_id) {
        (Some(root_id), Some(me)) => store
            .thread_has_sender(&record.conversation_id, root_id, me)
            .unwrap_or(false),
        _ => false,
    };
    ChannelSignals {
        mentions_channel: teams_core::mentions(record)
            .iter()
            .any(|mention| mention.group),
        in_my_thread,
        hidden: channel_hidden(sidebar, &record.conversation_id),
        is_reply: record.reply_to_id.is_some(),
        notifications: channel_notifications(sidebar, &record.conversation_id),
    }
}

fn channel_notifications(sidebar: &Sidebar, conversation_id: &str) -> ChannelNotifications {
    sidebar
        .teams
        .iter()
        .find(|team| {
            team.channels
                .iter()
                .any(|channel| channel.id == conversation_id)
        })
        .map(|team| team.channel_notifications(conversation_id))
        .unwrap_or_default()
}

fn channel_hidden(sidebar: &Sidebar, conversation_id: &str) -> bool {
    sidebar.teams.iter().any(|team| {
        team.channels
            .iter()
            .any(|channel| channel.id == conversation_id)
            && (team.hidden
                || team
                    .hidden_channel_ids
                    .iter()
                    .any(|channel_id| channel_id == conversation_id))
    })
}

fn mentions_me(record: &MessageRecord, my_user_id: Option<&str>) -> bool {
    let Some(me) = my_user_id else {
        return false;
    };
    teams_core::mentions(record)
        .iter()
        .any(|mention| mention.user_id.as_deref() == Some(me))
}

pub fn preview_of(record: &MessageRecord) -> Preview {
    if let Some(text) = teams_core::preview_text(&record.body_html) {
        return Preview::Text(text);
    }
    if !teams_core::images(record).is_empty() {
        return Preview::Image;
    }
    let file_name = teams_core::attachments(record)
        .into_iter()
        .find_map(|attachment| attachment.name);
    Preview::Text(file_name.unwrap_or_else(|| FILE_FALLBACK.to_owned()))
}

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use store::{
        ChannelNotificationLevel, ChannelRecord, ChatRecord, MemberRecord, SidebarTeam, TeamRecord,
    };

    use super::*;

    fn sidebar() -> Sidebar {
        Sidebar {
            chats: vec![
                ChatRecord {
                    id: "direct".into(),
                    kind: "oneOnOne".into(),
                    title: "Mara".into(),
                    ..Default::default()
                },
                ChatRecord {
                    id: "group".into(),
                    kind: "group".into(),
                    title: "Team".into(),
                    members: vec![MemberRecord::default(); 5],
                    ..Default::default()
                },
            ],
            teams: vec![
                team("team-open", false, &["channel-open"], &[]),
                team("team-hidden", true, &["channel-in-hidden-team"], &[]),
                team(
                    "team-partial",
                    false,
                    &["channel-hidden", "channel-shown"],
                    &["channel-hidden"],
                ),
            ],
        }
    }

    fn team(
        team_id: &str,
        hidden: bool,
        channel_ids: &[&str],
        hidden_channel_ids: &[&str],
    ) -> SidebarTeam {
        SidebarTeam {
            team: TeamRecord {
                id: team_id.into(),
                name: "Team".into(),
            },
            channels: channel_ids
                .iter()
                .map(|id| ChannelRecord {
                    id: (*id).into(),
                    team_id: team_id.into(),
                    name: "General".into(),
                    membership_type: None,
                    last_message_at: None,
                    unread: false,
                })
                .collect(),
            hidden,
            hidden_channel_ids: hidden_channel_ids
                .iter()
                .map(|id| (*id).to_owned())
                .collect(),
            notifications: HashMap::new(),
        }
    }

    fn collect_one(message: MessageRecord, store: &Store) -> Incoming {
        build_incoming(&message, store, &sidebar(), Some("me")).unwrap()
    }

    fn record(conversation: &str, sender: &str, at: DateTime<Utc>, html: &str) -> MessageRecord {
        MessageRecord {
            subject: None,
            conversation_id: conversation.into(),
            message_id: format!("{conversation}-{}", at.timestamp_millis()),
            reply_to_id: None,
            sender_id: Some(sender.into()),
            sender_name: Some("Sender".into()),
            sender_application_id: None,
            links_json: "[]".to_owned(),
            created_at: at,
            edited_at: None,
            deleted: false,
            body_html: html.into(),
            attachments_json: "[]".into(),
            reactions_json: "[]".into(),
            mentions_json: "[]".into(),
            call_event_json: None,
        }
    }

    #[test]
    fn incoming_in_a_muted_chat_is_marked_muted() {
        let mut quiet = sidebar();
        quiet.chats[1].muted = true;
        let message = record("group", "u1", Utc::now(), "hi");
        let store = Store::open_in_memory().unwrap();
        let build =
            |sidebar: &Sidebar| build_incoming(&message, &store, sidebar, Some("me")).unwrap();
        assert!(build(&quiet).muted);
        assert!(!build(&sidebar()).muted);
    }

    #[test]
    fn history_before_start_is_ignored() {
        let start = Utc::now();
        let mut tracker = IncomingTracker::new(start);
        let old = record("direct", "u1", start - Duration::minutes(5), "alt");
        let new = record("direct", "u1", start + Duration::seconds(5), "neu");
        let found = tracker.fresh(
            vec![old, new],
            &Store::open_in_memory().unwrap(),
            &sidebar(),
            Some("me"),
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].preview, Preview::Text("neu".into()));
    }

    #[test]
    fn same_message_is_not_reported_twice() {
        let start = Utc::now();
        let mut tracker = IncomingTracker::new(start);
        let message = record("direct", "u1", start + Duration::seconds(5), "neu");
        let store = Store::open_in_memory().unwrap();
        let sidebar = sidebar();
        let first = tracker.fresh(vec![message.clone()], &store, &sidebar, None);
        assert_eq!(first.len(), 1);
        assert!(
            tracker
                .fresh(vec![message], &store, &sidebar, None)
                .is_empty()
        );
    }

    #[test]
    fn own_messages_from_other_devices_are_skipped() {
        let start = Utc::now();
        let mut tracker = IncomingTracker::new(start);
        let own = record("direct", "me", start + Duration::seconds(5), "ich");
        let store = Store::open_in_memory().unwrap();
        let found = tracker.fresh(vec![own], &store, &sidebar(), Some("me"));
        assert!(found.is_empty());
    }

    #[test]
    fn message_first_stored_by_my_reaction_from_the_phone_is_skipped() {
        let start = Utc::now();
        let mut tracker = IncomingTracker::new(start);
        let mut reacted = record("direct", "u1", start + Duration::seconds(5), "PR link");
        reacted.reactions_json =
            r#"[{"reaction_type":"👍🏼","user_id":"me","user_name":null,"created_at":null}]"#.into();
        let mut liked_by_other = record("group", "u1", start + Duration::seconds(6), "hi");
        liked_by_other.reactions_json =
            r#"[{"reaction_type":"👍","user_id":"u2","user_name":null,"created_at":null}]"#.into();
        let store = Store::open_in_memory().unwrap();
        let found = tracker.fresh(
            vec![reacted, liked_by_other],
            &store,
            &sidebar(),
            Some("me"),
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].conversation_id, "group");
    }

    #[test]
    fn message_stored_late_by_an_update_is_not_an_arrival() {
        let start = Utc::now();
        let store = Store::open_in_memory().unwrap();
        let late = record("direct", "u1", start + Duration::seconds(5), "PR link");
        let arrived = record("direct", "u1", start + Duration::seconds(6), "neu");
        store.upsert_messages(&[late.clone(), arrived]).unwrap();
        let mut late_arrivals = LateArrivals::default();
        late_arrivals.insert("direct", vec![late.message_id.clone()]);
        let found = IncomingTracker::new(start).collect(
            &store,
            &sidebar(),
            Some("me"),
            &late_arrivals,
            "direct",
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].preview, Preview::Text("neu".into()));
    }

    #[test]
    fn late_arrivals_forget_the_oldest_beyond_the_limit() {
        let mut late_arrivals = LateArrivals::default();
        late_arrivals.insert(
            "c",
            (0..=LATE_ARRIVALS_KEPT).map(|n| n.to_string()).collect(),
        );
        assert!(!late_arrivals.contains("c", "0"));
        assert!(late_arrivals.contains("c", "1"));
        assert!(late_arrivals.contains("c", &LATE_ARRIVALS_KEPT.to_string()));
        assert!(!late_arrivals.contains("other", "1"));
    }

    #[test]
    fn message_read_on_another_device_is_skipped() {
        let start = Utc::now();
        let at = start + Duration::seconds(5);
        let store = Store::open_in_memory().unwrap();
        let mut read = sidebar();
        read.chats[0].last_read_at = Some(at);
        let found = IncomingTracker::new(start).fresh(
            vec![record("direct", "u1", at, "x")],
            &store,
            &read,
            Some("me"),
        );
        assert!(found.is_empty());
        read.chats[0].last_read_at = Some(at - Duration::milliseconds(1));
        let found = IncomingTracker::new(start).fresh(
            vec![record("direct", "u1", at, "x")],
            &store,
            &read,
            Some("me"),
        );
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn message_answered_from_another_device_is_skipped() {
        let start = Utc::now();
        let theirs = record("direct", "u1", start + Duration::seconds(5), "frage");
        let mine = record("direct", "me", start + Duration::seconds(9), "antwort");
        let store = Store::open_in_memory().unwrap();
        let found = IncomingTracker::new(start).fresh(
            vec![theirs.clone(), mine],
            &store,
            &sidebar(),
            Some("me"),
        );
        assert!(found.is_empty());
        let found = IncomingTracker::new(start).fresh(vec![theirs], &store, &sidebar(), Some("me"));
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn detects_mention_of_me_and_group_size() {
        let start = Utc::now();
        let mut message = record(
            "group",
            "u1",
            start + Duration::seconds(5),
            "<at>Me</at> hi",
        );
        message.mentions_json = r#"[{"user_id":"me","name":"Me"}]"#.into();
        let found = collect_one(message.clone(), &Store::open_in_memory().unwrap());
        assert!(found.mentions_me);
        assert_eq!(found.kind, ChatKind::Group { member_count: 5 });
        let other = build_incoming(
            &message,
            &Store::open_in_memory().unwrap(),
            &sidebar(),
            Some("other"),
        );
        assert!(!other.unwrap().mentions_me);
    }

    #[test]
    fn unknown_conversation_yields_nothing() {
        let message = record("nowhere", "u1", Utc::now(), "x");
        assert!(
            build_incoming(
                &message,
                &Store::open_in_memory().unwrap(),
                &sidebar(),
                None
            )
            .is_none()
        );
    }

    #[test]
    fn empty_body_without_image_falls_back_to_file_label() {
        let message = record("direct", "u1", Utc::now(), "");
        let found = build_incoming(
            &message,
            &Store::open_in_memory().unwrap(),
            &sidebar(),
            None,
        )
        .unwrap();
        assert_eq!(found.preview, Preview::Text(FILE_FALLBACK.into()));
    }

    #[test]
    fn group_mention_sets_channel_signal() {
        let store = Store::open_in_memory().unwrap();
        let mut message = record("channel-open", "u1", Utc::now(), "<at>channel</at>");
        message.mentions_json =
            r#"[{"user_id":null,"name":"channel","target_id":"channel-open","group":true}]"#.into();
        assert!(
            collect_one(message.clone(), &store)
                .signals
                .mentions_channel
        );
        message.mentions_json = r#"[{"user_id":null,"name":"bot","target_id":"bot-1"}]"#.into();
        assert!(!collect_one(message, &store).signals.mentions_channel);
    }

    #[test]
    fn reply_in_my_thread_sets_signal() {
        let store = Store::open_in_memory().unwrap();
        let start = Utc::now();
        let mut root = record("channel-open", "me", start, "root");
        root.message_id = "root".into();
        store.upsert_messages(&[root]).unwrap();
        let mut reply = record("channel-open", "u1", start + Duration::seconds(1), "reply");
        reply.reply_to_id = Some("root".into());
        assert!(collect_one(reply.clone(), &store).signals.in_my_thread);
        reply.reply_to_id = Some("unknown-root".into());
        assert!(!collect_one(reply, &store).signals.in_my_thread);
        let top_level = record("channel-open", "u1", start, "post");
        assert!(!collect_one(top_level, &store).signals.in_my_thread);
    }

    #[test]
    fn hidden_channels_are_flagged() {
        let store = Store::open_in_memory().unwrap();
        let signals =
            |channel: &str| collect_one(record(channel, "u1", Utc::now(), "x"), &store).signals;
        assert!(!signals("channel-open").hidden);
        assert!(signals("channel-in-hidden-team").hidden);
        assert!(signals("channel-hidden").hidden);
        assert!(!signals("channel-shown").hidden);
    }

    #[test]
    fn channel_notifications_and_reply_flag_come_from_the_sidebar() {
        let store = Store::open_in_memory().unwrap();
        let banner = ChannelNotifications {
            level: ChannelNotificationLevel::BannerAndFeed,
            include_replies: true,
        };
        let mut configured = sidebar();
        configured.teams[0]
            .notifications
            .insert("channel-open".into(), banner);
        let mut reply = record("channel-open", "u1", Utc::now(), "x");
        let build = |message: &MessageRecord| {
            build_incoming(message, &store, &configured, Some("me"))
                .unwrap()
                .signals
        };
        assert_eq!(build(&reply).notifications, banner);
        assert!(!build(&reply).is_reply);
        reply.reply_to_id = Some("root".into());
        assert!(build(&reply).is_reply);
        let other = record("channel-shown", "u1", Utc::now(), "x");
        assert_eq!(build(&other).notifications, ChannelNotifications::default());
    }

    #[test]
    fn chats_carry_no_channel_signals() {
        let store = Store::open_in_memory().unwrap();
        let found = collect_one(record("group", "u1", Utc::now(), "x"), &store);
        assert_eq!(found.signals, ChannelSignals::default());
    }
}
