use crate::activity_feed::{ActivityItem, ActivitySnapshot};

const MENTION_TYPES: [&str; 2] = ["mention", "mentionInCommunity"];

/// Who an @mention addressed, as Teams splits them in `ChannelActivitiesPublisher`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MentionKind {
    Personal,
    Channel,
    Team,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MentionActivity {
    pub activity_id: String,
    pub kind: MentionKind,
    pub source_thread_id: String,
    pub source_message_id: Option<String>,
    pub timestamp_ms: i64,
    pub is_read: bool,
}

pub fn mentions_of(snapshot: &ActivitySnapshot) -> Vec<MentionActivity> {
    snapshot
        .items
        .iter()
        .filter_map(|item| mention_activity(item, snapshot.is_read(item)))
        .collect()
}

fn mention_kind(subtype: &str) -> Option<MentionKind> {
    match subtype.to_ascii_lowercase().as_str() {
        "person" | "tag" => Some(MentionKind::Personal),
        "channel" => Some(MentionKind::Channel),
        "team" => Some(MentionKind::Team),
        _ => None,
    }
}

fn mention_activity(item: &ActivityItem, is_read: bool) -> Option<MentionActivity> {
    if !MENTION_TYPES.contains(&item.activity_type.as_str()) || item.source_thread_id.is_empty() {
        return None;
    }
    Some(MentionActivity {
        activity_id: item.activity_id.clone(),
        kind: mention_kind(&item.activity_subtype)?,
        source_thread_id: item.source_thread_id.clone(),
        source_message_id: item.source_message_id.clone(),
        timestamp_ms: item.timestamp_ms,
        is_read,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity_feed::ActivityHorizons;

    fn item(
        id: &str,
        activity_type: &str,
        subtype: &str,
        version_ms: i64,
        read_flag: bool,
    ) -> ActivityItem {
        ActivityItem {
            message_id: id.into(),
            activity_id: id.into(),
            activity_type: activity_type.into(),
            activity_subtype: subtype.into(),
            source_thread_id: "19:a@thread.tacv2".into(),
            source_message_id: Some("1791368000000".into()),
            timestamp_ms: version_ms,
            version_ms,
            read_flag,
            ..ActivityItem::default()
        }
    }

    #[test]
    fn keeps_mentions_and_maps_the_subtypes() {
        let snapshot = ActivitySnapshot {
            items: vec![
                item("1", "mention", "person", 100, true),
                item("2", "mention", "tag", 100, false),
                item("3", "mention", "channel", 100, false),
                item("4", "mentionInCommunity", "team", 100, false),
                item("5", "reply", "person", 100, false),
                item("6", "mention", "everyone", 100, false),
                item("7", "mentionInChat", "person", 100, false),
                item("8", "mention", "person", 10, false),
            ],
            horizons: ActivityHorizons {
                seen_ms: 0,
                bookmark_ms: 50,
            },
        };
        let mentions = mentions_of(&snapshot);
        let kinds: Vec<(&str, MentionKind, bool)> = mentions
            .iter()
            .map(|mention| (mention.activity_id.as_str(), mention.kind, mention.is_read))
            .collect();
        assert_eq!(
            kinds,
            [
                ("1", MentionKind::Personal, true),
                ("2", MentionKind::Personal, false),
                ("3", MentionKind::Channel, false),
                ("4", MentionKind::Team, false),
                ("8", MentionKind::Personal, true),
            ]
        );
        assert_eq!(
            mentions[0].source_message_id.as_deref(),
            Some("1791368000000")
        );
    }
}
