use std::collections::HashMap;

use chatsvc::{ChannelEdit, ChannelPrivacy, ExistingChannel, Mailbox, NewChannel};
use chrono::{Duration, Utc};

use crate::engine::SyncEngine;
use crate::error::{Error, Result};
use crate::events::CoreEvent;
use crate::remote::Remote;

pub const MAX_CHANNEL_NAME_LENGTH: usize = 50;
pub const MAX_CHANNEL_DESCRIPTION_LENGTH: usize = 1024;
const META_DELETED_CHANNELS: &str = "deleted_channels";
const DELETED_CHANNEL_GRACE_MINUTES: i64 = 60;

/// Channel ids deleted here with their deletion time in ms; Graph and CSA keep listing them for minutes.
pub(crate) fn parse_deleted_channels(text: &str) -> HashMap<String, i64> {
    serde_json::from_str(text).unwrap_or_default()
}

pub(crate) fn recent_deleted_channels(
    deleted: HashMap<String, i64>,
    now_ms: i64,
) -> HashMap<String, i64> {
    let grace_ms = Duration::minutes(DELETED_CHANNEL_GRACE_MINUTES).num_milliseconds();
    deleted
        .into_iter()
        .filter(|(_, deleted_at)| now_ms - deleted_at < grace_ms)
        .collect()
}

impl<R: Remote> SyncEngine<R> {
    pub async fn create_channel(
        &self,
        team_id: &str,
        name: &str,
        description: &str,
        privacy: ChannelPrivacy,
    ) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::Unsupported("a channel needs a name"));
        }
        let team_thread_id = self
            .store
            .team_thread_id(team_id)?
            .ok_or_else(|| Error::UnknownConversation(team_id.to_owned()))?;
        let object_id = self.my_user_id().await?;
        let tenant_id = self.my_tenant_id_from_any_chat().await?;
        let source = self.folder_source()?;
        let created = source
            .create_channel(
                NewChannel {
                    team_thread_id: &team_thread_id,
                    group_id: team_id,
                    name,
                    description: description.trim(),
                    privacy,
                },
                Mailbox {
                    object_id: &object_id,
                    tenant_id: &tenant_id,
                },
            )
            .await;
        self.refresh_teams(true).await;
        if let Err(error) = self.refresh_team_layout(source, true).await {
            self.report(format!("cannot refresh the team layout: {error}"));
        }
        let _ = self.events.send(CoreEvent::SidebarChanged);
        created
    }

    pub async fn channel_description(&self, channel_id: &str) -> Result<String> {
        let channel = self
            .store
            .channel(channel_id)?
            .ok_or_else(|| Error::UnknownConversation(channel_id.to_owned()))?;
        self.remote
            .channel_description(&channel.team_id, channel_id)
            .await
    }

    pub async fn edit_channel(
        &self,
        channel_id: &str,
        name: &str,
        description: Option<&str>,
    ) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::Unsupported("a channel needs a name"));
        }
        let address = self.existing_channel(channel_id)?;
        let (object_id, tenant_id) = self.mailbox_ids().await?;
        let source = self.folder_source()?;
        source
            .update_channel(
                address.existing(),
                ChannelEdit {
                    name,
                    description: description.map(str::trim),
                },
                Mailbox {
                    object_id: &object_id,
                    tenant_id: &tenant_id,
                },
            )
            .await?;
        self.store.rename_channel(channel_id, name)?;
        let _ = self.events.send(CoreEvent::SidebarChanged);
        Ok(())
    }

    pub async fn delete_channel(&self, channel_id: &str) -> Result<()> {
        let address = self.existing_channel(channel_id)?;
        let (object_id, tenant_id) = self.mailbox_ids().await?;
        self.folder_source()?
            .delete_channel(
                address.existing(),
                Mailbox {
                    object_id: &object_id,
                    tenant_id: &tenant_id,
                },
            )
            .await?;
        self.remember_deleted_channel(channel_id)?;
        self.store.remove_channel(channel_id)?;
        let _ = self.events.send(CoreEvent::SidebarChanged);
        Ok(())
    }

    pub(crate) fn deleted_channel_ids(&self) -> HashMap<String, i64> {
        let stored = self
            .store
            .meta(META_DELETED_CHANNELS)
            .ok()
            .flatten()
            .map(|text| parse_deleted_channels(&text))
            .unwrap_or_default();
        recent_deleted_channels(stored, Utc::now().timestamp_millis())
    }

    fn remember_deleted_channel(&self, channel_id: &str) -> Result<()> {
        let mut deleted = self.deleted_channel_ids();
        deleted.insert(channel_id.to_owned(), Utc::now().timestamp_millis());
        let text = serde_json::to_string(&deleted).unwrap_or_default();
        Ok(self.store.set_meta(META_DELETED_CHANNELS, &text)?)
    }

    fn existing_channel(&self, channel_id: &str) -> Result<ChannelAddress> {
        let channel = self
            .store
            .channel(channel_id)?
            .ok_or_else(|| Error::UnknownConversation(channel_id.to_owned()))?;
        let team_thread_id = self
            .store
            .team_thread_id(&channel.team_id)?
            .ok_or_else(|| Error::UnknownConversation(channel.team_id.clone()))?;
        Ok(ChannelAddress {
            team_thread_id,
            group_id: channel.team_id,
            channel_id: channel.id,
            substrate_group_id: self.store.channel_substrate_group_id(channel_id)?,
        })
    }

    async fn mailbox_ids(&self) -> Result<(String, String)> {
        Ok((
            self.my_user_id().await?,
            self.my_tenant_id_from_any_chat().await?,
        ))
    }
}

struct ChannelAddress {
    team_thread_id: String,
    group_id: String,
    channel_id: String,
    substrate_group_id: Option<String>,
}

impl ChannelAddress {
    fn existing(&self) -> ExistingChannel<'_> {
        ExistingChannel {
            team_thread_id: &self.team_thread_id,
            group_id: &self.group_id,
            channel_id: &self.channel_id,
            substrate_group_id: self.substrate_group_id.as_deref(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deleted_channels_are_kept_for_an_hour() {
        let now_ms = 10 * 60 * 60 * 1000;
        let deleted = parse_deleted_channels(&format!(
            r#"{{"19:fresh":{},"19:old":{}}}"#,
            now_ms - 1000,
            now_ms - 61 * 60 * 1000
        ));
        let recent = recent_deleted_channels(deleted, now_ms);
        assert!(recent.contains_key("19:fresh"));
        assert!(!recent.contains_key("19:old"));
        assert!(parse_deleted_channels("garbage").is_empty());
    }
}
