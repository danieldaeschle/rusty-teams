use std::collections::HashSet;

use chatsvc::{
    ConversationRef, Language, LanguageSettings, MAX_TRANSLATE_BATCH, MessageLanguage,
    TranslateRequest, Translation, TranslationTrigger,
};
use serde_json::Value;

use crate::engine::{Conversation, SyncEngine};
use crate::error::{Error, Result};
use crate::remote::Remote;

const STAMP_PAGE_SIZE: usize = 50;
const MAX_STAMP_REPLY_THREADS: usize = 5;
const STAMP_CACHE_MESSAGES: usize = 100;

impl<R: Remote> SyncEngine<R> {
    pub async fn language_stamps(&self, conversation_id: &str) -> Result<Vec<MessageLanguage>> {
        let targets =
            match self.resolve(conversation_id)? {
                Conversation::Chat => vec![ConversationRef::chat(conversation_id)],
                Conversation::Channel { .. } => {
                    let cached =
                        self.store
                            .messages(conversation_id, None, STAMP_CACHE_MESSAGES)?;
                    let mut seen = HashSet::new();
                    let reply_roots = cached
                        .iter()
                        .filter_map(|record| record.reply_to_id.as_deref())
                        .filter(|root_id| seen.insert(*root_id))
                        .take(MAX_STAMP_REPLY_THREADS);
                    std::iter::once(ConversationRef::channel_root(conversation_id))
                        .chain(reply_roots.map(|root_id| {
                            ConversationRef::channel_reply(conversation_id, root_id)
                        }))
                        .collect()
                }
            };
        let mut stamps = Vec::new();
        let mut last_error = None;
        for target in &targets {
            match self.remote.message_languages(target, STAMP_PAGE_SIZE).await {
                Ok(found) => stamps.extend(found),
                Err(error) => last_error = Some(error),
            }
        }
        match last_error {
            Some(error) if stamps.is_empty() => Err(error),
            _ => Ok(stamps),
        }
    }

    /// One entry per answered message; a message the service skipped has none.
    pub async fn translate_messages(
        &self,
        conversation_id: &str,
        message_ids: &[String],
        to_language: &str,
        trigger: &TranslationTrigger,
    ) -> Result<Vec<Translation>> {
        let conversation = self.resolve(conversation_id)?;
        let mut translations = Vec::new();
        match conversation {
            Conversation::Chat => {
                for batch in message_ids.chunks(MAX_TRANSLATE_BATCH) {
                    let requests: Vec<TranslateRequest> =
                        batch.iter().map(|id| TranslateRequest::new(id)).collect();
                    translations.extend(
                        self.remote
                            .translate_chat_messages(
                                conversation_id,
                                to_language,
                                &requests,
                                trigger,
                            )
                            .await?,
                    );
                }
            }
            Conversation::Channel { team_id } => {
                let records = self.store.messages_by_id(conversation_id, message_ids)?;
                let mut by_root: Vec<(String, Vec<TranslateRequest>)> = Vec::new();
                for message_id in message_ids {
                    let record = records
                        .get(message_id)
                        .ok_or(Error::Unsupported("translating an uncached message"))?;
                    let root_id = record
                        .reply_to_id
                        .clone()
                        .unwrap_or_else(|| record.message_id.clone());
                    match by_root.iter_mut().find(|(root, _)| *root == root_id) {
                        Some((_, requests)) => requests.push(TranslateRequest::new(message_id)),
                        None => by_root.push((root_id, vec![TranslateRequest::new(message_id)])),
                    }
                }
                for (root_id, requests) in by_root {
                    for batch in requests.chunks(MAX_TRANSLATE_BATCH) {
                        translations.extend(
                            self.remote
                                .translate_channel_messages(
                                    &team_id,
                                    conversation_id,
                                    &root_id,
                                    to_language,
                                    batch,
                                    trigger,
                                )
                                .await?,
                        );
                    }
                }
            }
        }
        Ok(translations)
    }

    pub async fn translation_languages(&self, locale: &str) -> Result<Vec<Language>> {
        self.remote.translation_languages(locale).await
    }

    pub async fn language_settings(&self) -> Result<LanguageSettings> {
        self.remote.language_settings().await
    }

    pub async fn patch_language_settings(&self, patch: Value) -> Result<()> {
        self.remote.patch_language_settings(patch).await
    }
}
