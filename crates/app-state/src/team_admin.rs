use gpui_kit::*;
use store::TeamRights;
use teams_core::{ChannelPrivacy, MAX_CHANNEL_NAME_LENGTH};

use crate::app_state::AppState;

pub const CREATE_CHANNEL_FAILURE: &str = "We couldn't create the channel.";
pub const CHANNEL_NAME_CHARACTERS: &str =
    "Channel names can't contain ~ # % & * { } + / \\ : < > ? | ' \" , or ..";
pub const CHANNEL_NAME_EDGES: &str = "Channel names can't start with _ or . and can't end with .";
pub const CHANNEL_NAME_LENGTH: &str = "Channel names can have up to 50 characters.";
const FORBIDDEN_CHARACTERS: &[char] = &[
    '~', '#', '%', '&', '*', '{', '}', '+', '/', '\\', ':', '<', '>', '?', '|', '\'', '"', ',',
];

/// Why Teams would refuse this channel name, `None` when it may be created.
pub fn channel_name_problem(raw: &str) -> Option<&'static str> {
    let name = raw.trim();
    if name.chars().count() > MAX_CHANNEL_NAME_LENGTH {
        return Some(CHANNEL_NAME_LENGTH);
    }
    if name.contains(FORBIDDEN_CHARACTERS) || name.contains("..") {
        return Some(CHANNEL_NAME_CHARACTERS);
    }
    if name.starts_with(['_', '.']) || name.ends_with('.') {
        return Some(CHANNEL_NAME_EDGES);
    }
    None
}

pub fn can_create_channel(raw: &str) -> bool {
    !raw.trim().is_empty() && channel_name_problem(raw).is_none()
}

impl AppState {
    pub fn team_rights(&self, team_id: &str) -> TeamRights {
        self.sidebar
            .teams
            .iter()
            .find(|team| team.team.id == team_id)
            .map(|team| team.rights)
            .unwrap_or_default()
    }

    pub fn team_name(&self, team_id: &str) -> Option<String> {
        self.sidebar
            .teams
            .iter()
            .find(|team| team.team.id == team_id)
            .map(|team| team.team.name.clone())
    }

    pub fn create_channel(
        &mut self,
        team_id: &str,
        name: &str,
        description: &str,
        privacy: ChannelPrivacy,
        cx: &mut Context<Self>,
    ) {
        let Some(Some(engine)) = self.chat_action_engine(cx) else {
            return;
        };
        let (team_id, name, description) =
            (team_id.to_owned(), name.to_owned(), description.to_owned());
        self.run_channel_action(
            CREATE_CHANNEL_FAILURE,
            async move {
                engine
                    .create_channel(&team_id, &name, &description, privacy)
                    .await
            },
            |_, _| {},
            None,
            cx,
        );
    }
}

#[cfg(test)]
mod tests {
    use teams_core::MAX_CHANNEL_NAME_LENGTH;

    use super::{
        CHANNEL_NAME_CHARACTERS, CHANNEL_NAME_EDGES, CHANNEL_NAME_LENGTH, can_create_channel,
        channel_name_problem,
    };

    #[test]
    fn channel_names_follow_the_teams_rules() {
        assert!(can_create_channel(" Planning 2027 "));
        assert!(!can_create_channel("   "));
        assert!(!can_create_channel(
            &"a".repeat(MAX_CHANNEL_NAME_LENGTH + 1)
        ));
        assert_eq!(
            channel_name_problem(&"a".repeat(MAX_CHANNEL_NAME_LENGTH + 1)),
            Some(CHANNEL_NAME_LENGTH)
        );
        assert_eq!(channel_name_problem("Q&A"), Some(CHANNEL_NAME_CHARACTERS));
        assert_eq!(channel_name_problem("a..b"), Some(CHANNEL_NAME_CHARACTERS));
        assert_eq!(channel_name_problem("_drafts"), Some(CHANNEL_NAME_EDGES));
        assert_eq!(channel_name_problem("notes."), Some(CHANNEL_NAME_EDGES));
        assert_eq!(channel_name_problem("Release notes"), None);
    }
}
