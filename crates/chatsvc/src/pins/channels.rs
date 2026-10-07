use serde_json::{Value, json};
use session::{ApiResponse, Method, Request};

use super::Pins;
use super::chats::{api_error, ensure_success};
use super::state::{PinnedChannels, error_text, parse_pinned_channels};
use super::transport::CsaTransport;
use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub(super) enum ChannelAction {
    Pin { channel_id: String },
    Unpin { channel_id: String },
    Reorder { channel_ids: Vec<String> },
}

impl ChannelAction {
    pub(super) fn method(&self) -> Method {
        match self {
            ChannelAction::Unpin { .. } => Method::Delete,
            _ => Method::Post,
        }
    }

    pub(super) fn body(&self) -> Value {
        match self {
            ChannelAction::Pin { channel_id } => json!({"newlyPinnedChannels": [channel_id]}),
            ChannelAction::Unpin { channel_id } => json!({"channelIdsToUnpin": [channel_id]}),
            ChannelAction::Reorder { channel_ids } => json!({"pinnedChannelOrder": channel_ids}),
        }
    }
}

fn is_version_mismatch(answer: &ApiResponse) -> bool {
    answer.status == 412
        && error_text(&answer.body)
            .to_ascii_lowercase()
            .contains("versionmismatch")
}

fn is_missing_from_pin_list(answer: &ApiResponse) -> bool {
    answer.status == 412 && error_text(&answer.body).contains("does not include all requested ids")
}

impl<T: CsaTransport> Pins<T> {
    pub async fn pinned_channels(&self) -> Result<PinnedChannels> {
        let answer = self
            .transport
            .send(Request::get(format!("{}/pinnedChannels", self.base_url)))
            .await?;
        ensure_success(&answer)?;
        parse_pinned_channels(&answer.body)
    }

    pub async fn pin_channel(&self, channel_id: &str) -> Result<PinnedChannels> {
        let action = ChannelAction::Pin {
            channel_id: channel_id.to_owned(),
        };
        self.write_channels(action).await
    }

    pub async fn unpin_channel(&self, channel_id: &str) -> Result<PinnedChannels> {
        let action = ChannelAction::Unpin {
            channel_id: channel_id.to_owned(),
        };
        self.write_channels(action).await
    }

    pub async fn reorder_channels(&self, channel_ids: &[String]) -> Result<PinnedChannels> {
        let action = ChannelAction::Reorder {
            channel_ids: channel_ids.to_vec(),
        };
        self.write_channels(action).await
    }

    async fn write_channels(&self, action: ChannelAction) -> Result<PinnedChannels> {
        let mut state = self.pinned_channels().await?;
        for attempt in 0..2 {
            let mut request = Request::with_body(
                action.method(),
                format!("{}/pinnedChannels", self.base_url),
                action.body(),
            );
            request
                .headers
                .push(("if-match".into(), state.order_version.clone()));
            let answer = self.transport.send(request).await?;
            if answer.is_success() {
                return self.pinned_channels().await;
            }
            if is_version_mismatch(&answer) && attempt == 0 {
                state = self.pinned_channels().await?;
                continue;
            }
            if is_version_mismatch(&answer) {
                return Err(Error::VersionConflict);
            }
            if matches!(action, ChannelAction::Unpin { .. }) && is_missing_from_pin_list(&answer) {
                return self.pinned_channels().await;
            }
            return Err(api_error(&answer));
        }
        Err(Error::VersionConflict)
    }
}
