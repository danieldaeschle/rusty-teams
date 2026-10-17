use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use mcp::ReactionProposal;
use store::{Sidebar, Store};

use super::{ProposalEntry, ProposalStatus, agent_label};
use crate::activity::preview_label;
use crate::app_state::chat_title;
use crate::notify::preview_of;
use crate::rows::reaction_glyph;

const EXCERPT_CHARACTERS: usize = 120;

#[derive(Debug, Clone, PartialEq)]
pub enum PendingKind {
    Reply {
        text: String,
    },
    Reaction {
        glyph: String,
        message_id: String,
        excerpt: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingProposal {
    pub id: String,
    pub conversation_id: String,
    pub agent: &'static str,
    pub kind: PendingKind,
    pub created_at: DateTime<Utc>,
    pub seen: bool,
}

impl PendingProposal {
    pub fn title(&self) -> String {
        let what = match self.kind {
            PendingKind::Reply { .. } => "a reply",
            PendingKind::Reaction { .. } => "a reaction",
        };
        format!("{} proposes {what}", self.agent)
    }

    pub fn headline(&self, conversation: Option<&str>) -> String {
        match conversation {
            Some(conversation) => format!("{} in {conversation}", self.title()),
            None => self.title(),
        }
    }

    pub fn detail(&self) -> String {
        match &self.kind {
            PendingKind::Reply { text } => single_line(text),
            PendingKind::Reaction {
                glyph,
                excerpt: Some(excerpt),
                ..
            } => format!("{glyph} to: {excerpt}"),
            PendingKind::Reaction { glyph, .. } => glyph.clone(),
        }
    }

    pub fn jump_message_id(&self) -> &str {
        match &self.kind {
            PendingKind::Reply { .. } => "",
            PendingKind::Reaction { message_id, .. } => message_id,
        }
    }
}

fn single_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn collect(
    proposals: &HashMap<String, ProposalEntry>,
    reactions: &HashMap<(String, String), ReactionProposal>,
    excerpts: &HashMap<String, String>,
    seen: &HashSet<String>,
) -> Vec<PendingProposal> {
    let replies = proposals
        .values()
        .filter(|entry| !matches!(entry.status, ProposalStatus::Stale { .. }))
        .map(|entry| PendingProposal {
            id: entry.proposal.id.clone(),
            conversation_id: entry.proposal.conversation_id.clone(),
            agent: agent_label(entry.proposal.agent.as_deref()),
            kind: PendingKind::Reply {
                text: entry.proposal.text.clone(),
            },
            created_at: entry.proposal.created_at,
            seen: seen.contains(&entry.proposal.id),
        });
    let reacts = reactions.values().map(|reaction| PendingProposal {
        id: reaction.id.clone(),
        conversation_id: reaction.conversation_id.clone(),
        agent: agent_label(reaction.agent.as_deref()),
        kind: PendingKind::Reaction {
            glyph: reaction_glyph(&reaction.emoji),
            message_id: reaction.message_id.clone(),
            excerpt: excerpts.get(&reaction.id).cloned(),
        },
        created_at: reaction.created_at,
        seen: seen.contains(&reaction.id),
    });
    let mut pending: Vec<PendingProposal> = replies.chain(reacts).collect();
    pending.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    pending
}

pub fn conversation_label(sidebar: &Sidebar, conversation_id: &str) -> Option<String> {
    if let Some(chat) = sidebar.chats.iter().find(|chat| chat.id == conversation_id) {
        return Some(chat_title(chat));
    }
    sidebar.teams.iter().find_map(|team| {
        team.channels
            .iter()
            .find(|channel| channel.id == conversation_id)
            .map(|channel| format!("{} > {}", team.team.name, channel.name))
    })
}

pub fn reacted_excerpt(store: &Store, conversation_id: &str, message_id: &str) -> Option<String> {
    let records = store
        .messages_by_id(conversation_id, &[message_id.to_owned()])
        .ok()?;
    let record = records.get(message_id)?;
    let text = single_line(&preview_label(&preview_of(record)));
    Some(text.chars().take(EXCERPT_CHARACTERS).collect())
}

pub fn decode_seen(saved: Option<String>) -> HashSet<String> {
    saved
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

pub fn encode_seen(seen: &HashSet<String>) -> String {
    let mut ids: Vec<&str> = seen.iter().map(String::as_str).collect();
    ids.sort_unstable();
    ids.join("\n")
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;

    use chrono::{Local, TimeZone, Utc};
    use mcp::{Proposal, ReactionProposal};

    use super::*;

    fn reply(
        conversation_id: &str,
        id: &str,
        minute: u32,
        status: ProposalStatus,
    ) -> ProposalEntry {
        ProposalEntry {
            proposal: Proposal {
                id: id.into(),
                conversation_id: conversation_id.into(),
                reply_to_message_id: None,
                text: "Looks\n good  to me".into(),
                mentions: Vec::new(),
                images: Vec::new(),
                agent: Some("claude".into()),
                session_id: None,
                created_at: Utc.with_ymd_and_hms(2026, 10, 18, 9, minute, 0).unwrap(),
            },
            body: Arc::new(Vec::new()),
            mention_labels: Vec::new(),
            images: Vec::new(),
            status,
        }
    }

    fn reaction(id: &str, minute: u32) -> ReactionProposal {
        ReactionProposal {
            id: id.into(),
            conversation_id: "chat-b".into(),
            message_id: "m7".into(),
            emoji: "like".into(),
            agent: Some("copilot".into()),
            session_id: None,
            created_at: Utc.with_ymd_and_hms(2026, 10, 18, 9, minute, 0).unwrap(),
        }
    }

    #[test]
    fn collect_lists_pending_replies_and_reactions_newest_first() {
        let proposals = HashMap::from([
            (
                "chat-a".to_owned(),
                reply("chat-a", "p1", 5, ProposalStatus::Pending),
            ),
            (
                "chat-c".to_owned(),
                reply(
                    "chat-c",
                    "p2",
                    9,
                    ProposalStatus::Stale {
                        answered_at: Local::now(),
                    },
                ),
            ),
        ]);
        let reactions =
            HashMap::from([(("chat-b".to_owned(), "m7".to_owned()), reaction("r1", 7))]);
        let excerpts = HashMap::from([("r1".to_owned(), "Ship it?".to_owned())]);
        let seen = HashSet::from(["p1".to_owned()]);
        let pending = collect(&proposals, &reactions, &excerpts, &seen);
        let ids: Vec<&str> = pending.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, ["r1", "p1"]);
        assert!(!pending[0].seen);
        assert!(pending[1].seen);
        assert_eq!(pending[0].title(), "Copilot proposes a reaction");
        assert_eq!(pending[0].jump_message_id(), "m7");
        assert_eq!(pending[1].jump_message_id(), "");
        assert_eq!(
            pending[1].headline(Some("Release planning")),
            "Claude proposes a reply in Release planning"
        );
        assert_eq!(pending[1].detail(), "Looks good to me");
        assert!(pending[0].detail().ends_with(" to: Ship it?"));
    }

    #[test]
    fn seen_ids_round_trip() {
        let seen = HashSet::from(["b".to_owned(), "a".to_owned()]);
        assert_eq!(encode_seen(&seen), "a\nb");
        assert_eq!(decode_seen(Some(encode_seen(&seen))), seen);
        assert!(decode_seen(None).is_empty());
    }
}
