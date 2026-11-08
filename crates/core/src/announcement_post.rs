use chatsvc::ConversationRef;
use graph::MessageExtras;
use store::MessageRecord;

use crate::actions::outgoing;
use crate::announcement::{AnnouncementBanner, BannerBackground, TeamsBannerImage};
use crate::engine::{Conversation, SyncEngine};
use crate::error::{Error, Result};
use crate::mentions::MentionInput;
use crate::remote::Remote;

const BANNER_FILE_NAME: &str = "MicrosoftTeams-image.png";
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if !bytes.starts_with(PNG_SIGNATURE) || bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    let read = |range: std::ops::Range<usize>| -> Option<u32> {
        Some(u32::from_be_bytes(bytes.get(range)?.try_into().ok()?))
    };
    Some((read(16..20)?, read(20..24)?))
}

impl<R: Remote> SyncEngine<R> {
    /// Graph rejects the banner card from a user token, so the post goes out plain and the
    /// chat service adds the card. The image is uploaded first so a failed upload sends nothing.
    pub async fn post_announcement(
        &self,
        channel_id: &str,
        html: &str,
        subject: Option<&str>,
        mentions: &[MentionInput],
        extras: &MessageExtras,
        banner: &AnnouncementBanner,
    ) -> Result<MessageRecord> {
        let conversation = self.resolve(channel_id).await?;
        let Conversation::Channel { team_id } = &conversation else {
            return Err(Error::Unsupported("an announcement in a chat"));
        };
        let image = match &banner.background {
            BannerBackground::LocalImage(path) => Some(self.upload_banner_image(channel_id, path).await?),
            _ => None,
        };
        let (html, mentions) = outgoing(&conversation, html, mentions)?;
        let mut sent = self
            .remote
            .send_channel_message(team_id, channel_id, &html, subject, &mentions, extras)
            .await?;
        let cards = banner.teams_cards(image.as_ref());
        let target = ConversationRef::ChannelRoot {
            channel_id: channel_id.to_owned(),
        };
        if self
            .remote
            .set_message_cards(&target, &sent.id, &cards)
            .await
            .is_ok()
        {
            let shown = match &image {
                Some(image) => AnnouncementBanner {
                    title: banner.title.clone(),
                    background: BannerBackground::Image(image.url.clone()),
                },
                None => banner.clone(),
            };
            sent.attachments
                .insert(0, shown.attachment(format!("announcement-card-{}", sent.id)));
        }
        self.cache_sent(channel_id, &sent, &html, extras)
    }

    async fn upload_banner_image(&self, channel_id: &str, path: &str) -> Result<TeamsBannerImage> {
        let bytes = std::fs::read(path)
            .map_err(|_| Error::Unsupported("a banner image that is no longer on disk"))?;
        let (width, height) =
            png_size(&bytes).ok_or(Error::Unsupported("a banner image that is not a PNG"))?;
        let uploaded = self
            .remote
            .upload_ams_image(channel_id, &bytes, BANNER_FILE_NAME)
            .await?;
        Ok(TeamsBannerImage {
            object_id: uploaded.object_id,
            url: uploaded.url,
            width,
            height,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::png_size;

    #[test]
    fn png_size_reads_the_header() {
        let mut header = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        header.extend_from_slice(&1317u32.to_be_bytes());
        header.extend_from_slice(&366u32.to_be_bytes());
        assert_eq!(png_size(&header), Some((1317, 366)));
        assert_eq!(png_size(b"GIF89a"), None);
    }
}
