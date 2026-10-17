use std::collections::HashMap;

use chatsvc::BotProfile;
use chrono::Utc;
use graph::Chat;
use store::{AvatarRecord, BOT_MRI_PREFIX, ChatRecord, MemberRecord};

use crate::engine::SyncEngine;
use crate::error::{Error, Result};
use crate::mapping::chat_record;
use crate::remote::Remote;

/// Bots Graph leaves out of 1:1 chat members, such as the Echo test call bot, by the id in the chat id.
#[derive(Default)]
pub(crate) struct BotPartners {
    known: HashMap<String, Option<BotProfile>>,
}

impl BotPartners {
    pub(crate) fn unknown_ids(&self, chats: &[Chat], my_user_id: &str) -> Vec<String> {
        let mut unknown: Vec<String> = chats
            .iter()
            .filter_map(|chat| chat.memberless_partner_id(my_user_id))
            .filter(|partner_id| !self.known.contains_key(*partner_id))
            .map(str::to_owned)
            .collect();
        unknown.sort();
        unknown.dedup();
        unknown
    }

    pub(crate) fn remember(&mut self, asked: &[String], profiles: Vec<BotProfile>) {
        for bot_id in asked {
            self.known.entry(bot_id.clone()).or_insert(None);
        }
        for profile in profiles {
            self.known.insert(profile.bot_id.clone(), Some(profile));
        }
    }

    pub(crate) fn profile(&self, bot_id: &str) -> Option<&BotProfile> {
        self.known.get(bot_id)?.as_ref()
    }

    pub(crate) fn chat_record(&self, chat: &Chat, my_user_id: &str) -> ChatRecord {
        let mut record = chat_record(chat, my_user_id);
        let partner = chat
            .memberless_partner_id(my_user_id)
            .and_then(|partner_id| self.profile(partner_id));
        if let Some(profile) = partner {
            record.title = profile.display_name.clone();
            record.member_summary = profile.display_name.clone();
            record.members.push(MemberRecord {
                user_id: Some(format!("{BOT_MRI_PREFIX}{}", profile.bot_id)),
                display_name: profile.display_name.clone(),
            });
        }
        record
    }
}

impl<R: Remote> SyncEngine<R> {
    pub(crate) async fn resolve_bot_partners(&self, chats: &[Chat], my_user_id: &str) {
        let unknown = self.bot_partners().unknown_ids(chats, my_user_id);
        self.fetch_bot_profiles(&unknown).await;
    }

    async fn fetch_bot_profiles(&self, bot_ids: &[String]) {
        if bot_ids.is_empty() {
            return;
        }
        match self.remote.bot_profiles(bot_ids).await {
            Ok(profiles) => self.bot_partners().remember(bot_ids, profiles),
            Err(Error::Unsupported(_)) => self.bot_partners().remember(bot_ids, Vec::new()),
            Err(error) => self.report(format!("cannot look up bot chats: {error}")),
        }
    }

    pub(crate) fn bot_partners(&self) -> std::sync::MutexGuard<'_, BotPartners> {
        self.bot_partners
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn partner_chat_record(&self, chat: &Chat, my_user_id: &str) -> ChatRecord {
        self.bot_partners().chat_record(chat, my_user_id)
    }

    pub(crate) async fn fetch_bot_icons(&self, bot_mris: &[String]) -> Result<Vec<String>> {
        let bot_ids: Vec<String> = bot_mris
            .iter()
            .filter_map(|mri| mri.strip_prefix(BOT_MRI_PREFIX))
            .map(str::to_owned)
            .collect();
        let missing: Vec<String> = {
            let partners = self.bot_partners();
            bot_ids
                .iter()
                .filter(|bot_id| !partners.known.contains_key(*bot_id))
                .cloned()
                .collect()
        };
        self.fetch_bot_profiles(&missing).await;
        let mut changed = Vec::new();
        for bot_id in bot_ids {
            let image_url = self
                .bot_partners()
                .profile(&bot_id)
                .and_then(|profile| profile.image_url.clone());
            let photo = match image_url {
                Some(url) => match self.remote.external_image(&url).await {
                    Ok(photo) => Some(photo),
                    Err(error) => {
                        self.report(format!("cannot load a bot icon: {error}"));
                        continue;
                    }
                },
                None => None,
            };
            let user_id = format!("{BOT_MRI_PREFIX}{bot_id}");
            if photo.is_some() {
                changed.push(user_id.clone());
            }
            self.store.upsert_avatar(&AvatarRecord {
                user_id,
                content_type: photo
                    .as_ref()
                    .map(|photo| photo.content_type.clone())
                    .unwrap_or_default(),
                bytes: photo.map(|photo| photo.bytes),
                fetched_at: Utc::now(),
            })?;
        }
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn echo_chat() -> Chat {
        serde_json::from_value(serde_json::json!({
            "id": "19:user-me_bot-echo@unq.gbl.spaces",
            "chatType": "oneOnOne",
            "members": [{"userId": "user-me", "displayName": "Test Me"}],
        }))
        .unwrap()
    }

    fn echo_profile() -> BotProfile {
        BotProfile {
            bot_id: "bot-echo".into(),
            display_name: "Echo".into(),
            image_url: None,
        }
    }

    #[test]
    fn a_resolved_bot_names_the_chat_and_joins_its_members() {
        let mut partners = BotPartners::default();
        let chats = [echo_chat()];
        assert_eq!(partners.unknown_ids(&chats, "user-me"), ["bot-echo"]);
        partners.remember(&["bot-echo".to_owned()], vec![echo_profile()]);
        assert!(partners.unknown_ids(&chats, "user-me").is_empty());
        let record = partners.chat_record(&chats[0], "user-me");
        assert_eq!(record.title, "Echo");
        assert_eq!(record.member_summary, "Echo");
        let bot = record.members.last().unwrap();
        assert_eq!(bot.user_id.as_deref(), Some("28:bot-echo"));
        assert!(bot.is_bot());
    }

    #[test]
    fn a_partner_that_is_no_bot_is_asked_once_and_keeps_the_graph_title() {
        let mut partners = BotPartners::default();
        let chats = [echo_chat()];
        partners.remember(&["bot-echo".to_owned()], Vec::new());
        assert!(partners.unknown_ids(&chats, "user-me").is_empty());
        let record = partners.chat_record(&chats[0], "user-me");
        assert_eq!(record.title, "(only you)");
        assert_eq!(record.members.len(), 1);
    }
}
