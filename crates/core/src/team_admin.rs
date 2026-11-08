use chatsvc::{ChannelPrivacy, Mailbox, NewChannel};

use crate::engine::SyncEngine;
use crate::error::{Error, Result};
use crate::events::CoreEvent;
use crate::remote::Remote;

pub const MAX_CHANNEL_NAME_LENGTH: usize = 50;
pub const MAX_CHANNEL_DESCRIPTION_LENGTH: usize = 1024;

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
        source
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
            .await?;
        self.refresh_teams(true).await;
        if let Err(error) = self.refresh_team_layout(source, true).await {
            self.report(format!("cannot refresh the team layout: {error}"));
        }
        let _ = self.events.send(CoreEvent::SidebarChanged);
        Ok(())
    }
}
