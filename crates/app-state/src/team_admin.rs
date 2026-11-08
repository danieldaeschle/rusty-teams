use gpui_kit::*;
use store::TeamRights;
use teams_core::{ChannelPrivacy, MAX_CHANNEL_NAME_LENGTH};

use crate::app_state::{AppState, Selection};

pub const CREATE_CHANNEL_FAILURE: &str = "We couldn't create the channel.";
pub const EDIT_CHANNEL_FAILURE: &str = "Channel wasn't renamed.";
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

pub fn delete_channel_failure(channel_name: &str) -> String {
    format!("We couldn't delete the {channel_name} channel. Try deleting it again.")
}

pub fn channel_deleted_notice(channel_name: &str) -> String {
    format!("{channel_name} has been deleted")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelKind {
    General,
    Standard,
    Private,
    Shared,
}

impl ChannelKind {
    pub fn of(general: bool, membership_type: Option<&str>) -> Self {
        match membership_type.map(str::to_ascii_lowercase).as_deref() {
            _ if general => ChannelKind::General,
            Some("private") => ChannelKind::Private,
            Some("shared") => ChannelKind::Shared,
            _ => ChannelKind::Standard,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChannelAdmin {
    pub can_edit: bool,
    pub can_delete: bool,
}

/// Private channels are managed by team owners here, the channel's own owner role is not loaded.
pub fn channel_admin(rights: TeamRights, kind: ChannelKind) -> ChannelAdmin {
    match kind {
        ChannelKind::General | ChannelKind::Shared => ChannelAdmin::default(),
        ChannelKind::Private => ChannelAdmin {
            can_edit: rights.owner,
            can_delete: rights.owner,
        },
        ChannelKind::Standard => ChannelAdmin {
            can_edit: rights.edit_channel,
            can_delete: rights.delete_channel,
        },
    }
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

    pub fn channel_admin(&self, channel_id: &str) -> ChannelAdmin {
        let Some((team, channel)) = self.sidebar.teams.iter().find_map(|team| {
            team.channels
                .iter()
                .find(|channel| channel.id == channel_id)
                .map(|channel| (team, channel))
        }) else {
            return ChannelAdmin::default();
        };
        let general = self
            .store
            .team_thread_id(&team.team.id)
            .ok()
            .flatten()
            .is_some_and(|thread_id| thread_id == channel_id);
        let kind = ChannelKind::of(general, channel.membership_type.as_deref());
        channel_admin(team.rights, kind)
    }

    pub fn channel_is_private(&self, channel_id: &str) -> bool {
        self.sidebar
            .teams
            .iter()
            .flat_map(|team| &team.channels)
            .find(|channel| channel.id == channel_id)
            .is_some_and(|channel| {
                ChannelKind::of(false, channel.membership_type.as_deref()) == ChannelKind::Private
            })
    }

    pub fn load_channel_description(
        &self,
        channel_id: &str,
        cx: &mut Context<Self>,
    ) -> Task<Option<String>> {
        let Some(engine) = self.engine.clone() else {
            return Task::ready(None);
        };
        let channel_id = channel_id.to_owned();
        let receiver =
            crate::runtime::spawn(async move { engine.channel_description(&channel_id).await });
        cx.background_spawn(async move { receiver.await.ok().and_then(Result::ok) })
    }

    pub fn edit_channel(
        &mut self,
        channel_id: &str,
        name: &str,
        description: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let Some(Some(engine)) = self.chat_action_engine(cx) else {
            return;
        };
        let (channel_id, name, description) = (
            channel_id.to_owned(),
            name.to_owned(),
            description.map(str::to_owned),
        );
        self.run_channel_action(
            EDIT_CHANNEL_FAILURE,
            async move {
                engine
                    .edit_channel(&channel_id, &name, description.as_deref())
                    .await
            },
            |_, _| {},
            None,
            cx,
        );
    }

    pub fn delete_channel(&mut self, channel_id: &str, cx: &mut Context<Self>) {
        let Some(Some(engine)) = self.chat_action_engine(cx) else {
            return;
        };
        let channel_name = self.channel_name(channel_id).unwrap_or_default();
        let general_id = self
            .channel_team_id(channel_id)
            .and_then(|team_id| self.store.team_thread_id(&team_id).ok().flatten());
        let owned_id = channel_id.to_owned();
        let receiver = crate::runtime::spawn(async move { engine.delete_channel(&owned_id).await });
        let channel_id = channel_id.to_owned();
        cx.spawn(async move |this, cx| {
            let deleted = matches!(receiver.await, Ok(Ok(())));
            this.update(cx, |state, cx| {
                if !deleted {
                    state.raise_notice(delete_channel_failure(&channel_name), None, cx);
                    return;
                }
                if state.selection == Some(Selection::Channel(channel_id.clone()))
                    && let Some(general_id) = general_id
                {
                    state.select(Selection::Channel(general_id), cx);
                }
                state.raise_notice(channel_deleted_notice(&channel_name), None, cx);
            })
            .ok();
        })
        .detach();
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

    use store::TeamRights;

    use super::{
        CHANNEL_NAME_CHARACTERS, CHANNEL_NAME_EDGES, CHANNEL_NAME_LENGTH, ChannelAdmin,
        ChannelKind, can_create_channel, channel_admin, channel_name_problem,
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

    #[test]
    fn channel_admin_follows_the_team_rights_and_channel_kind() {
        let member = TeamRights {
            edit_channel: true,
            delete_channel: false,
            ..TeamRights::default()
        };
        assert_eq!(
            channel_admin(member, ChannelKind::Standard),
            ChannelAdmin {
                can_edit: true,
                can_delete: false
            }
        );
        assert_eq!(
            channel_admin(member, ChannelKind::Private),
            ChannelAdmin::default()
        );
        let owner = TeamRights {
            owner: true,
            edit_channel: true,
            delete_channel: true,
            ..TeamRights::default()
        };
        assert_eq!(
            channel_admin(owner, ChannelKind::General),
            ChannelAdmin::default()
        );
        assert_eq!(
            channel_admin(owner, ChannelKind::Private),
            ChannelAdmin {
                can_edit: true,
                can_delete: true
            }
        );
        assert_eq!(
            ChannelKind::of(false, Some("Private")),
            ChannelKind::Private
        );
        assert_eq!(
            ChannelKind::of(true, Some("standard")),
            ChannelKind::General
        );
        assert_eq!(ChannelKind::of(false, None), ChannelKind::Standard);
    }
}
