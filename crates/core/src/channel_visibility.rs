use chatsvc::{ChannelKind, ChannelTarget};
use chrono::Utc;

use crate::engine::SyncEngine;
use crate::error::{Error, Result};
use crate::events::CoreEvent;
use crate::message_link::{ChannelAddress, channel_link, team_link};
use crate::remote::Remote;

impl<R: Remote> SyncEngine<R> {
    pub async fn set_channel_pinned(&self, channel_id: &str, pinned: bool) -> Result<()> {
        let channel_ids = self
            .folder_source()?
            .set_channel_pinned(channel_id, pinned)
            .await?;
        self.store.replace_pinned_channels(&channel_ids)?;
        let _ = self.events.send(CoreEvent::FoldersChanged);
        Ok(())
    }

    pub async fn set_channel_shown(&self, channel_id: &str, shown: bool) -> Result<()> {
        let channel = self
            .store
            .channel(channel_id)?
            .ok_or_else(|| Error::UnknownConversation(channel_id.to_owned()))?;
        let general = self.store.team_thread_id(&channel.team_id)?.as_deref() == Some(channel_id);
        let own_user_id = self.my_user_id().await?;
        let target = ChannelTarget {
            channel_id,
            general,
            kind: ChannelKind::from_membership_type(channel.membership_type.as_deref()),
        };
        self.folder_source()?
            .set_channel_shown(target, &own_user_id, shown, Utc::now().timestamp_millis())
            .await?;
        self.store.set_channel_hidden(channel_id, !shown)?;
        let _ = self.events.send(CoreEvent::SidebarChanged);
        Ok(())
    }

    pub async fn set_team_shown(&self, team_id: &str, shown: bool) -> Result<()> {
        let team_thread_id = self.known_team_thread_id(team_id)?;
        self.folder_source()?
            .set_team_shown(&team_thread_id, shown)
            .await?;
        self.store.set_team_hidden(team_id, !shown)?;
        let _ = self.events.send(CoreEvent::SidebarChanged);
        Ok(())
    }

    pub async fn channel_link(&self, channel_id: &str) -> Result<String> {
        let channel = self
            .store
            .channel(channel_id)?
            .ok_or_else(|| Error::UnknownConversation(channel_id.to_owned()))?;
        Ok(channel_link(&ChannelAddress {
            tenant_id: self.my_tenant_id_from_any_chat().await?,
            group_id: channel.team_id,
            channel_id: channel.id,
            channel_name: channel.name,
            membership_type: channel.membership_type,
        }))
    }

    pub async fn team_link(&self, team_id: &str) -> Result<String> {
        let team_thread_id = self.known_team_thread_id(team_id)?;
        let tenant_id = self.my_tenant_id_from_any_chat().await?;
        Ok(team_link(&team_thread_id, team_id, &tenant_id))
    }

    fn known_team_thread_id(&self, team_id: &str) -> Result<String> {
        self.store
            .team_thread_id(team_id)?
            .ok_or_else(|| Error::UnknownConversation(team_id.to_owned()))
    }
}
