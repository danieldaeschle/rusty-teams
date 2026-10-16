use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use store::MessageRecord;
use teams_core::{
    FALLBACK_TARGET, Language, LanguageSettings, LanguageStamp, TranslationBehavior,
    default_target, language_name, offered_language,
};

pub type MessageKey = (String, String);

#[derive(Debug, Clone, PartialEq)]
pub enum TranslationLine {
    Offer {
        language_code: String,
        language: String,
    },
    Working,
    Failed,
    Translated {
        language: Option<String>,
        showing_original: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CachedTranslation {
    pub html: String,
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    conversation_id: String,
    message_id: String,
    version: i64,
    target: String,
}

#[derive(Debug, Clone, Default)]
pub struct TranslationContext {
    pub enabled: bool,
    pub target: String,
    pub behavior: TranslationBehavior,
    pub never: Vec<String>,
    pub languages: Vec<Language>,
    pub stamps: HashMap<String, String>,
    pub cached: HashMap<(String, i64), CachedTranslation>,
    pub shown: HashSet<String>,
    pub working: HashSet<String>,
    pub failed: HashSet<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RowTranslation {
    pub line: Option<TranslationLine>,
    pub translated_html: Option<String>,
}

pub fn message_version(record: &MessageRecord) -> i64 {
    record
        .edited_at
        .map_or(0, |edited| edited.timestamp_millis())
}

impl TranslationContext {
    pub fn resolve(&self, record: &MessageRecord, own: bool) -> RowTranslation {
        if !self.enabled || record.deleted {
            return RowTranslation::default();
        }
        let id = &record.message_id;
        if self.working.contains(id) {
            return line_only(TranslationLine::Working);
        }
        let version = message_version(record);
        if let Some(cached) = self.cached.get(&(id.clone(), version)) {
            let showing = self.shown.contains(id);
            return RowTranslation {
                line: Some(TranslationLine::Translated {
                    language: cached
                        .source
                        .as_deref()
                        .map(|code| language_name(code, &self.languages)),
                    showing_original: !showing,
                }),
                translated_html: showing.then(|| cached.html.clone()),
            };
        }
        if self.failed.contains(id) {
            return line_only(TranslationLine::Failed);
        }
        if own || self.behavior != TranslationBehavior::Ask {
            return RowTranslation::default();
        }
        let offered = self
            .stamps
            .get(id)
            .and_then(|stamp| offered_language(stamp, &self.target, &self.never));
        match offered {
            Some(code) => line_only(TranslationLine::Offer {
                language: language_name(&code, &self.languages),
                language_code: code,
            }),
            None => RowTranslation::default(),
        }
    }
}

fn line_only(line: TranslationLine) -> RowTranslation {
    RowTranslation {
        line: Some(line),
        translated_html: None,
    }
}

#[derive(Default)]
pub struct TranslationState {
    pub settings: Option<LanguageSettings>,
    pub languages: Vec<Language>,
    pub languages_fetched_at: Option<DateTime<Utc>>,
    pub stamps: HashMap<String, HashMap<String, String>>,
    pub stamps_in_flight: HashSet<String>,
    asked: HashMap<String, HashSet<String>>,
    pub auto_scheduled: HashSet<String>,
    cache: HashMap<CacheKey, CachedTranslation>,
    shown: HashSet<MessageKey>,
    working: HashSet<MessageKey>,
    failed: HashSet<MessageKey>,
}

impl TranslationState {
    pub fn target(&self) -> String {
        match &self.settings {
            Some(settings) => default_target(
                settings.target_locale.as_deref(),
                settings.display_locale.as_deref(),
                &self.languages,
            ),
            None => FALLBACK_TARGET.to_owned(),
        }
    }

    pub fn behavior(&self) -> TranslationBehavior {
        self.settings
            .as_ref()
            .map_or(TranslationBehavior::Ask, |settings| settings.behavior)
    }

    /// The target counts as known, so nothing in it is ever offered.
    pub fn never_codes(&self) -> Vec<String> {
        let mut codes = self
            .settings
            .as_ref()
            .map(LanguageSettings::never_translate_codes)
            .unwrap_or_default();
        let target = self.target();
        if !codes.contains(&target) {
            codes.push(target);
        }
        codes
    }

    pub fn display_codes(&self) -> Vec<String> {
        self.settings
            .as_ref()
            .map(LanguageSettings::never_translate_codes)
            .unwrap_or_default()
    }

    pub fn language_name(&self, code: &str) -> String {
        language_name(code, &self.languages)
    }

    pub fn context_for(&self, conversation_id: &str) -> TranslationContext {
        let target = self.target();
        let of_conversation = |key: &MessageKey| (key.0 == conversation_id).then(|| key.1.clone());
        TranslationContext {
            enabled: self.settings.is_some(),
            behavior: self.behavior(),
            never: self.never_codes(),
            languages: self.languages.clone(),
            stamps: self
                .stamps
                .get(conversation_id)
                .cloned()
                .unwrap_or_default(),
            cached: self
                .cache
                .iter()
                .filter(|(key, _)| key.conversation_id == conversation_id && key.target == target)
                .map(|(key, cached)| ((key.message_id.clone(), key.version), cached.clone()))
                .collect(),
            shown: self.shown.iter().filter_map(of_conversation).collect(),
            working: self.working.iter().filter_map(of_conversation).collect(),
            failed: self.failed.iter().filter_map(of_conversation).collect(),
            target,
        }
    }

    pub fn is_cached(&self, conversation_id: &str, message_id: &str, version: i64) -> bool {
        self.cache.contains_key(&CacheKey {
            conversation_id: conversation_id.to_owned(),
            message_id: message_id.to_owned(),
            version,
            target: self.target(),
        })
    }

    pub fn is_pending(&self, conversation_id: &str, message_id: &str) -> bool {
        self.working
            .contains(&(conversation_id.to_owned(), message_id.to_owned()))
    }

    pub fn is_failed(&self, conversation_id: &str, message_id: &str) -> bool {
        self.failed
            .contains(&(conversation_id.to_owned(), message_id.to_owned()))
    }

    pub fn start(&mut self, conversation_id: &str, message_id: &str) {
        let key = (conversation_id.to_owned(), message_id.to_owned());
        self.failed.remove(&key);
        self.working.insert(key);
    }

    pub fn fail(&mut self, conversation_id: &str, message_id: &str) {
        let key = (conversation_id.to_owned(), message_id.to_owned());
        self.working.remove(&key);
        self.failed.insert(key);
    }

    pub fn finish(
        &mut self,
        conversation_id: &str,
        message_id: &str,
        version: i64,
        target: &str,
        html: String,
    ) {
        let key = (conversation_id.to_owned(), message_id.to_owned());
        self.working.remove(&key);
        self.failed.remove(&key);
        let source = self
            .stamps
            .get(conversation_id)
            .and_then(|stamps| stamps.get(message_id))
            .and_then(|stamp| LanguageStamp::parse(stamp))
            .and_then(|stamp| stamp.candidates.first().map(|(code, _)| code.clone()));
        self.cache.insert(
            CacheKey {
                conversation_id: conversation_id.to_owned(),
                message_id: message_id.to_owned(),
                version,
                target: target.to_owned(),
            },
            CachedTranslation { html, source },
        );
        self.shown.insert(key);
    }

    /// Flips between the translation and the original of a message that has a stored translation.
    pub fn toggle(&mut self, conversation_id: &str, message_id: &str) {
        let key = (conversation_id.to_owned(), message_id.to_owned());
        if !self.shown.remove(&key) {
            self.shown.insert(key);
        }
    }

    pub fn mark_asked(&mut self, conversation_id: &str, message_ids: Vec<String>) {
        self.asked
            .entry(conversation_id.to_owned())
            .or_default()
            .extend(message_ids);
    }

    pub fn was_asked(&self, conversation_id: &str, message_id: &str) -> bool {
        self.asked
            .get(conversation_id)
            .is_some_and(|asked| asked.contains(message_id))
    }

    pub fn merge_stamps(&mut self, conversation_id: &str, stamps: HashMap<String, String>) -> bool {
        let known = self.stamps.entry(conversation_id.to_owned()).or_default();
        let before = known.len();
        let mut changed = false;
        for (message_id, stamp) in stamps {
            changed |= known.insert(message_id, stamp.clone()).as_ref() != Some(&stamp);
        }
        changed || known.len() != before
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    const FRENCH: &str = "languages=fr:100;en:10;length:80;&detector=Bling";

    fn record(id: &str) -> MessageRecord {
        MessageRecord {
            message_id: id.to_owned(),
            conversation_id: "chat".to_owned(),
            body_html: "<p>Bonjour</p>".to_owned(),
            ..MessageRecord::default()
        }
    }

    fn state_with_stamp() -> TranslationState {
        let mut state = TranslationState {
            settings: Some(LanguageSettings::default()),
            ..TranslationState::default()
        };
        state.merge_stamps(
            "chat",
            HashMap::from([("m1".to_owned(), FRENCH.to_owned())]),
        );
        state
    }

    #[test]
    fn a_confident_foreign_message_gets_an_offer_in_ask_mode() {
        let context = state_with_stamp().context_for("chat");
        let resolved = context.resolve(&record("m1"), false);
        assert_eq!(
            resolved.line,
            Some(TranslationLine::Offer {
                language_code: "fr".into(),
                language: "French".into()
            })
        );
        assert_eq!(resolved.translated_html, None);
    }

    #[test]
    fn own_messages_and_unknown_settings_get_no_offer() {
        let context = state_with_stamp().context_for("chat");
        assert_eq!(
            context.resolve(&record("m1"), true),
            RowTranslation::default()
        );
        let mut unready = state_with_stamp();
        unready.settings = None;
        assert_eq!(
            unready.context_for("chat").resolve(&record("m1"), false),
            RowTranslation::default()
        );
    }

    #[test]
    fn other_modes_and_the_never_list_get_no_offer() {
        let mut state = state_with_stamp();
        let settings = state.settings.as_mut().unwrap();
        settings.behavior = TranslationBehavior::Never;
        assert_eq!(
            state.context_for("chat").resolve(&record("m1"), false),
            RowTranslation::default()
        );
        let settings = state.settings.as_mut().unwrap();
        settings.behavior = TranslationBehavior::Ask;
        settings.authoring_locales = vec!["fr-FR".into()];
        assert_eq!(
            state.context_for("chat").resolve(&record("m1"), false),
            RowTranslation::default()
        );
    }

    #[test]
    fn a_message_without_a_stamp_gets_no_offer() {
        let context = state_with_stamp().context_for("chat");
        assert_eq!(
            context.resolve(&record("m2"), false),
            RowTranslation::default()
        );
    }

    #[test]
    fn working_then_translated_then_toggled_back() {
        let mut state = state_with_stamp();
        state.start("chat", "m1");
        let working = state.context_for("chat").resolve(&record("m1"), false);
        assert_eq!(working.line, Some(TranslationLine::Working));

        state.finish("chat", "m1", 0, "en", "<p>Hello</p>".into());
        let shown = state.context_for("chat").resolve(&record("m1"), false);
        assert_eq!(shown.translated_html.as_deref(), Some("<p>Hello</p>"));
        assert_eq!(
            shown.line,
            Some(TranslationLine::Translated {
                language: Some("French".into()),
                showing_original: false
            })
        );

        state.toggle("chat", "m1");
        let original = state.context_for("chat").resolve(&record("m1"), false);
        assert_eq!(original.translated_html, None);
        assert_eq!(
            original.line,
            Some(TranslationLine::Translated {
                language: Some("French".into()),
                showing_original: true
            })
        );

        state.toggle("chat", "m1");
        assert!(
            state
                .context_for("chat")
                .resolve(&record("m1"), false)
                .translated_html
                .is_some()
        );
    }

    #[test]
    fn a_failed_request_shows_the_retry_line_until_started_again() {
        let mut state = state_with_stamp();
        state.start("chat", "m1");
        state.fail("chat", "m1");
        assert_eq!(
            state.context_for("chat").resolve(&record("m1"), false).line,
            Some(TranslationLine::Failed)
        );
        state.start("chat", "m1");
        assert!(!state.is_failed("chat", "m1"));
        assert!(state.is_pending("chat", "m1"));
    }

    #[test]
    fn an_edit_or_a_new_target_misses_the_cache() {
        let mut state = state_with_stamp();
        state.finish("chat", "m1", 0, "en", "<p>Hello</p>".into());
        assert!(state.is_cached("chat", "m1", 0));
        assert!(!state.is_cached("chat", "m1", 1));
        let mut edited = record("m1");
        edited.edited_at = Utc.timestamp_millis_opt(5_000).single();
        assert_eq!(message_version(&edited), 5_000);
        assert!(
            state
                .context_for("chat")
                .resolve(&edited, true)
                .translated_html
                .is_none()
        );

        state.settings.as_mut().unwrap().target_locale = Some("de".into());
        assert!(!state.is_cached("chat", "m1", 0));
        assert!(state.context_for("chat").cached.is_empty());
    }

    #[test]
    fn contexts_only_carry_their_own_conversation() {
        let mut state = state_with_stamp();
        state.finish("chat", "m1", 0, "en", "<p>Hello</p>".into());
        let other = state.context_for("other");
        assert!(other.cached.is_empty());
        assert!(other.shown.is_empty());
        assert!(other.stamps.is_empty());
    }

    #[test]
    fn the_target_is_always_a_known_language() {
        let state = state_with_stamp();
        assert_eq!(state.never_codes(), vec!["en".to_owned()]);
        assert!(state.display_codes().is_empty());
    }

    #[test]
    fn asked_messages_are_remembered_per_conversation() {
        let mut state = TranslationState::default();
        state.mark_asked("chat", vec!["m1".into()]);
        assert!(state.was_asked("chat", "m1"));
        assert!(!state.was_asked("chat", "m2"));
        assert!(!state.was_asked("other", "m1"));
    }

    #[test]
    fn merging_stamps_reports_changes_only() {
        let mut state = TranslationState::default();
        let stamps = HashMap::from([("m1".to_owned(), FRENCH.to_owned())]);
        assert!(state.merge_stamps("chat", stamps.clone()));
        assert!(!state.merge_stamps("chat", stamps));
    }
}
