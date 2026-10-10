use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use gpui_kit::*;
use serde_json::Value;
use teams_core::{
    Language, LanguageSettings, Translation, TranslationBehavior, TranslationStatus,
    TranslationTrigger, authoring_patch, behavior_patch, offered_language, parse_language_settings,
    target_patch,
};

use crate::app_state::{AppEvent, AppState};
use crate::notice::short_error;
use crate::runtime;
use crate::translation::message_version;

const SETTINGS_META_KEY: &str = "translation_settings";
const LANGUAGES_META_KEY: &str = "translation_languages";
const LANGUAGES_AT_META_KEY: &str = "translation_languages_at";
const LANGUAGES_TTL_HOURS: i64 = 24;
const DEFAULT_LOCALE: &str = "en";
const AUTO_DEBOUNCE: Duration = Duration::from_millis(300);
const STAMP_WINDOW: usize = 50;
const META_USER_ID: &str = "me_user_id";

type TranslationOutcome = Result<Vec<Translation>, String>;

impl AppState {
    pub fn load_cached_translation_settings(&mut self) {
        self.translation.settings = self
            .store
            .meta(SETTINGS_META_KEY)
            .ok()
            .flatten()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|value| parse_language_settings(&value).ok());
        self.translation.languages = self
            .store
            .meta(LANGUAGES_META_KEY)
            .ok()
            .flatten()
            .and_then(|text| serde_json::from_str::<Vec<(String, String)>>(&text).ok())
            .map(|pairs| {
                pairs
                    .into_iter()
                    .map(|(code, name)| Language { code, name })
                    .collect()
            })
            .unwrap_or_default();
        self.translation.languages_fetched_at = self
            .store
            .meta(LANGUAGES_AT_META_KEY)
            .ok()
            .flatten()
            .and_then(|text| DateTime::parse_from_rfc3339(&text).ok())
            .map(|time| time.with_timezone(&Utc));
    }

    fn save_translation_settings(&self) {
        if let Some(settings) = &self.translation.settings {
            let _ = self
                .store
                .set_meta(SETTINGS_META_KEY, &settings.to_account_value().to_string());
        }
    }

    fn save_translation_languages(&self) {
        let pairs: Vec<(&str, &str)> = self
            .translation
            .languages
            .iter()
            .map(|language| (language.code.as_str(), language.name.as_str()))
            .collect();
        let _ = self.store.set_meta(
            LANGUAGES_META_KEY,
            &serde_json::to_string(&pairs).unwrap_or_default(),
        );
        if let Some(fetched_at) = self.translation.languages_fetched_at {
            let _ = self
                .store
                .set_meta(LANGUAGES_AT_META_KEY, &fetched_at.to_rfc3339());
        }
    }

    pub fn refresh_translation_settings(&mut self, cx: &mut Context<Self>) {
        self.load_cached_translation_settings();
        let Some(engine) = self.engine.clone() else {
            return;
        };
        let languages_stale = self.translation.languages.is_empty()
            || self.translation.languages_fetched_at.is_none_or(|fetched| {
                Utc::now() - fetched > chrono::Duration::hours(LANGUAGES_TTL_HOURS)
            });
        let cached_locale = self
            .translation
            .settings
            .as_ref()
            .and_then(|settings| settings.display_locale.clone());
        let receiver = runtime::spawn(async move {
            let settings = engine.language_settings().await;
            let locale = settings
                .as_ref()
                .ok()
                .and_then(|settings| settings.display_locale.clone())
                .or(cached_locale)
                .unwrap_or_else(|| DEFAULT_LOCALE.to_owned());
            let languages = if languages_stale {
                engine.translation_languages(&locale).await.ok()
            } else {
                None
            };
            (settings, languages)
        });
        cx.spawn(async move |this, cx| {
            let Ok((settings, languages)) = receiver.await else {
                return;
            };
            this.update(cx, |state, cx| {
                if let Ok(settings) = settings {
                    state.translation.settings = Some(settings);
                    state.save_translation_settings();
                }
                if let Some(languages) = languages.filter(|languages| !languages.is_empty()) {
                    state.translation.languages = languages;
                    state.translation.languages_fetched_at = Some(Utc::now());
                    state.save_translation_languages();
                }
                state.translation_changed(cx);
                state.refresh_selected_translation(cx);
            })
            .ok();
        })
        .detach();
    }

    fn translation_changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(AppEvent::Translation);
        cx.notify();
    }

    fn refresh_selected_translation(&mut self, cx: &mut Context<Self>) {
        let Some(selection) = self.selection.clone() else {
            return;
        };
        self.refresh_language_stamps(selection.conversation_id(), cx);
        self.schedule_auto_translate(selection.conversation_id(), cx);
    }

    pub fn refresh_language_stamps(&mut self, conversation_id: &str, cx: &mut Context<Self>) {
        if self.translation.settings.is_none()
            || self.translation.behavior() == TranslationBehavior::Never
        {
            return;
        }
        if self.mode.demo {
            let stamps = crate::demo::language_stamps(conversation_id);
            if self.translation.merge_stamps(conversation_id, stamps) {
                self.translation_changed(cx);
            }
            self.schedule_auto_translate(conversation_id, cx);
            return;
        }
        let Some(engine) = self.engine.clone() else {
            return;
        };
        let unasked = self.unasked_message_ids(conversation_id);
        if unasked.is_empty()
            || !self
                .translation
                .stamps_in_flight
                .insert(conversation_id.to_owned())
        {
            return;
        }
        let conversation_id = conversation_id.to_owned();
        let requested = conversation_id.clone();
        let receiver = runtime::spawn(async move { engine.language_stamps(&requested).await });
        cx.spawn(async move |this, cx| {
            let outcome = receiver.await;
            this.update(cx, |state, cx| {
                state.translation.stamps_in_flight.remove(&conversation_id);
                let Ok(Ok(found)) = outcome else {
                    return;
                };
                state.translation.mark_asked(&conversation_id, unasked);
                let stamps: HashMap<String, String> = found
                    .into_iter()
                    .map(|entry| (entry.message_id, entry.stamp))
                    .collect();
                if state.translation.merge_stamps(&conversation_id, stamps) {
                    state.translation_changed(cx);
                }
                state.schedule_auto_translate(&conversation_id, cx);
            })
            .ok();
        })
        .detach();
    }

    fn unasked_message_ids(&self, conversation_id: &str) -> Vec<String> {
        let my_user_id = self.store.meta(META_USER_ID).ok().flatten();
        let records = self
            .store
            .messages(conversation_id, None, STAMP_WINDOW)
            .unwrap_or_default();
        records
            .into_iter()
            .filter(|record| !record.deleted && record.sender_id != my_user_id)
            .map(|record| record.message_id)
            .filter(|message_id| !self.translation.was_asked(conversation_id, message_id))
            .collect()
    }

    fn schedule_auto_translate(&mut self, conversation_id: &str, cx: &mut Context<Self>) {
        if self.translation.behavior() != TranslationBehavior::Auto
            || !self
                .translation
                .auto_scheduled
                .insert(conversation_id.to_owned())
        {
            return;
        }
        let conversation_id = conversation_id.to_owned();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(AUTO_DEBOUNCE).await;
            this.update(cx, |state, cx| {
                state.translation.auto_scheduled.remove(&conversation_id);
                state.run_auto_translate(&conversation_id, cx);
            })
            .ok();
        })
        .detach();
    }

    fn run_auto_translate(&mut self, conversation_id: &str, cx: &mut Context<Self>) {
        if self.translation.behavior() != TranslationBehavior::Auto {
            return;
        }
        let Some(stamps) = self.translation.stamps.get(conversation_id).cloned() else {
            return;
        };
        let ids: Vec<String> = stamps.keys().cloned().collect();
        let records = self
            .store
            .messages_by_id(conversation_id, &ids)
            .unwrap_or_default();
        let my_user_id = self.store.meta(META_USER_ID).ok().flatten();
        let (target, never) = (self.translation.target(), self.translation.never_codes());
        let mut due: Vec<(String, i64)> = records
            .values()
            .filter(|record| !record.deleted && record.sender_id != my_user_id)
            .filter(|record| {
                let id = &record.message_id;
                !self.translation.is_pending(conversation_id, id)
                    && !self.translation.is_failed(conversation_id, id)
                    && !self
                        .translation
                        .is_cached(conversation_id, id, message_version(record))
            })
            .filter(|record| {
                stamps
                    .get(&record.message_id)
                    .and_then(|stamp| offered_language(stamp, &target, &never))
                    .is_some()
            })
            .map(|record| (record.message_id.clone(), message_version(record)))
            .collect();
        if due.is_empty() {
            return;
        }
        due.sort();
        let trigger = TranslationTrigger::Automatic {
            known_languages: never,
        };
        self.request_translation(conversation_id, due, trigger, cx);
    }

    /// Offer link, menu item, retry and the See original toggle: flips a stored translation or asks for a new one.
    pub fn translate_message(
        &mut self,
        conversation_id: &str,
        message_id: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(record) = self.message_record(conversation_id, message_id) else {
            return;
        };
        let version = message_version(&record);
        if self
            .translation
            .is_cached(conversation_id, message_id, version)
        {
            self.translation.toggle(conversation_id, message_id);
            self.translation_changed(cx);
            return;
        }
        let items = vec![(message_id.to_owned(), version)];
        self.request_translation(conversation_id, items, TranslationTrigger::OnDemand, cx);
    }

    fn request_translation(
        &mut self,
        conversation_id: &str,
        items: Vec<(String, i64)>,
        trigger: TranslationTrigger,
        cx: &mut Context<Self>,
    ) {
        let target = self.translation.target();
        let engine = self.engine.clone();
        if engine.is_none() && !self.mode.demo {
            return;
        }
        for (message_id, _) in &items {
            self.translation.start(conversation_id, message_id);
        }
        self.translation_changed(cx);
        let conversation_id = conversation_id.to_owned();
        let Some(engine) = engine else {
            let outcome = Ok(crate::demo::translations(
                &items.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(),
            ));
            self.apply_translations(&conversation_id, &items, &target, outcome, cx);
            return;
        };
        let ids: Vec<String> = items.iter().map(|(id, _)| id.clone()).collect();
        let (requested_conversation, requested_target) = (conversation_id.clone(), target.clone());
        let receiver = runtime::spawn(async move {
            engine
                .translate_messages(&requested_conversation, &ids, &requested_target, &trigger)
                .await
        });
        cx.spawn(async move |this, cx| {
            let outcome = match receiver.await {
                Ok(Ok(translations)) => Ok(translations),
                Ok(Err(error)) => Err(short_error(&error)),
                Err(_) => Err("cancelled".to_owned()),
            };
            this.update(cx, |state, cx| {
                state.apply_translations(&conversation_id, &items, &target, outcome, cx)
            })
            .ok();
        })
        .detach();
    }

    fn apply_translations(
        &mut self,
        conversation_id: &str,
        items: &[(String, i64)],
        target: &str,
        outcome: TranslationOutcome,
        cx: &mut Context<Self>,
    ) {
        let answered = outcome.unwrap_or_default();
        for (message_id, version) in items {
            let html = answered
                .iter()
                .find(|translation| {
                    translation.message_id == *message_id
                        && translation.status == TranslationStatus::Done
                })
                .and_then(|translation| translation.content_html.clone());
            match html {
                Some(html) => {
                    self.translation
                        .finish(conversation_id, message_id, *version, target, html)
                }
                None => self.translation.fail(conversation_id, message_id),
            }
        }
        self.translation_changed(cx);
    }

    pub fn set_translation_target(&mut self, code: &str, cx: &mut Context<Self>) {
        let Some(settings) = self.translation.settings.clone() else {
            return;
        };
        let mut updated = settings.clone();
        updated.target_locale = Some(code.to_owned());
        self.update_language_settings("Translation language", updated, target_patch(code), cx);
    }

    pub fn set_translation_behavior(
        &mut self,
        behavior: TranslationBehavior,
        cx: &mut Context<Self>,
    ) {
        let Some(settings) = self.translation.settings.clone() else {
            return;
        };
        let patch = behavior_patch(&settings, behavior);
        let mut updated = settings;
        updated.behavior = behavior;
        updated.preferences = patch["translationPreferences"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        self.update_language_settings("Translation setting", updated, patch, cx);
    }

    pub fn never_translate_language(&mut self, code: &str, cx: &mut Context<Self>) {
        let Some(settings) = self.translation.settings.clone() else {
            return;
        };
        let locales = settings.with_never_translate(code);
        self.set_never_translate_locales(settings, locales, cx);
    }

    pub fn allow_translating_language(&mut self, code: &str, cx: &mut Context<Self>) {
        let Some(settings) = self.translation.settings.clone() else {
            return;
        };
        let locales = settings.without_never_translate(code);
        self.set_never_translate_locales(settings, locales, cx);
    }

    fn set_never_translate_locales(
        &mut self,
        settings: LanguageSettings,
        locales: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let patch = authoring_patch(&locales);
        let mut updated = settings;
        updated.authoring_locales = locales;
        self.update_language_settings("Translation setting", updated, patch, cx);
    }

    fn update_language_settings(
        &mut self,
        label: &'static str,
        updated: LanguageSettings,
        patch: Value,
        cx: &mut Context<Self>,
    ) {
        let Some(engine_or_demo) = self.chat_action_engine(cx) else {
            return;
        };
        let previous = self.translation.settings.replace(updated);
        self.save_translation_settings();
        self.translation_changed(cx);
        self.refresh_selected_translation(cx);
        let Some(engine) = engine_or_demo else {
            return;
        };
        self.run_chat_action(
            label,
            async move { engine.patch_language_settings(patch).await },
            move |state, cx| {
                state.translation.settings = previous;
                state.save_translation_settings();
                state.translation_changed(cx);
            },
            |_, _| {},
            cx,
        );
    }
}
