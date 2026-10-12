use std::collections::BTreeSet;

pub const DEFAULT_PORT: u16 = 47615;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Access {
    #[default]
    None,
    Selected(BTreeSet<String>),
    All,
}

impl Access {
    pub fn allows(&self, conversation_id: &str) -> bool {
        match self {
            Access::None => false,
            Access::Selected(ids) => ids.contains(conversation_id),
            Access::All => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationKind {
    Chat,
    Meeting,
    Channel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPolicy {
    pub enabled: bool,
    pub port: u16,
    pub chats: Access,
    pub channels: Access,
    pub propose: bool,
    pub calendar: bool,
    pub transcripts: bool,
}

impl Default for AgentPolicy {
    fn default() -> Self {
        AgentPolicy {
            enabled: false,
            port: DEFAULT_PORT,
            chats: Access::None,
            channels: Access::None,
            propose: false,
            calendar: false,
            transcripts: false,
        }
    }
}

impl AgentPolicy {
    pub fn access(&self, kind: ConversationKind) -> &Access {
        match kind {
            ConversationKind::Chat | ConversationKind::Meeting => &self.chats,
            ConversationKind::Channel => &self.channels,
        }
    }

    pub fn can_read(&self, kind: ConversationKind, conversation_id: &str) -> bool {
        self.enabled && self.access(kind).allows(conversation_id)
    }

    pub fn can_propose(&self, kind: ConversationKind, conversation_id: &str) -> bool {
        self.can_read(kind, conversation_id) && self.propose
    }

    pub fn can_read_any_kind(&self, conversation_id: &str) -> bool {
        self.enabled
            && (self.chats.allows(conversation_id) || self.channels.allows(conversation_id))
    }

    pub fn readable_ids<'a>(
        &self,
        kind: ConversationKind,
        conversation_ids: impl IntoIterator<Item = &'a str>,
    ) -> Vec<String> {
        conversation_ids
            .into_iter()
            .filter(|conversation_id| self.can_read(kind, conversation_id))
            .map(str::to_owned)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selected(ids: &[&str]) -> Access {
        Access::Selected(ids.iter().map(|id| (*id).to_owned()).collect())
    }

    fn policy(chats: Access, channels: Access, propose: bool) -> AgentPolicy {
        AgentPolicy {
            enabled: true,
            chats,
            channels,
            propose,
            ..AgentPolicy::default()
        }
    }

    #[test]
    fn read_and_propose_follow_scope_and_flags() {
        use ConversationKind::{Channel, Chat, Meeting};
        let cases = [
            (
                policy(Access::None, Access::None, true),
                Chat,
                "a",
                false,
                false,
            ),
            (
                policy(Access::All, Access::None, false),
                Chat,
                "a",
                true,
                false,
            ),
            (
                policy(Access::All, Access::None, true),
                Chat,
                "a",
                true,
                true,
            ),
            (
                policy(selected(&["a"]), Access::None, true),
                Chat,
                "a",
                true,
                true,
            ),
            (
                policy(selected(&["a"]), Access::None, true),
                Chat,
                "b",
                false,
                false,
            ),
            (
                policy(selected(&["a"]), Access::None, true),
                Meeting,
                "a",
                true,
                true,
            ),
            (
                policy(selected(&["a"]), Access::None, true),
                Channel,
                "a",
                false,
                false,
            ),
            (
                policy(Access::None, Access::All, true),
                Channel,
                "a",
                true,
                true,
            ),
            (
                policy(Access::All, selected(&["c"]), false),
                Channel,
                "d",
                false,
                false,
            ),
        ];
        for (policy, kind, id, read, propose) in cases {
            assert_eq!(policy.can_read(kind, id), read, "{kind:?} {id}");
            assert_eq!(policy.can_propose(kind, id), propose, "{kind:?} {id}");
        }
    }

    #[test]
    fn disabled_policy_allows_nothing() {
        let policy = AgentPolicy {
            enabled: false,
            chats: Access::All,
            channels: Access::All,
            propose: true,
            ..AgentPolicy::default()
        };
        assert!(!policy.can_read(ConversationKind::Chat, "a"));
        assert!(!policy.can_propose(ConversationKind::Channel, "a"));
    }

    #[test]
    fn default_is_closed() {
        let policy = AgentPolicy::default();
        assert_eq!(policy.port, DEFAULT_PORT);
        assert!(!policy.enabled && !policy.propose && !policy.calendar && !policy.transcripts);
        assert_eq!(policy.chats, Access::None);
        assert_eq!(policy.channels, Access::None);
    }

    #[test]
    fn readable_ids_keeps_only_allowed() {
        let policy = policy(selected(&["a", "c"]), Access::None, false);
        assert_eq!(
            policy.readable_ids(ConversationKind::Chat, ["a", "b", "c"]),
            ["a", "c"]
        );
    }
}
