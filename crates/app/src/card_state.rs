use std::collections::{HashMap, HashSet};
use std::time::Duration;

use teams_core::AdaptiveCard;

use gpui_kit::App;

use crate::card_inputs::{FieldHandle, InputField};

const DIALOG_KEY: &str = "task-dialog";
pub const DONE_HOLD: Duration = Duration::from_millis(1600);
pub const MAX_REASON_CHARS: usize = 160;
const MAX_REFRESHES_IN_FLIGHT: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionPhase {
    Busy,
    Done,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotIdentity {
    pub name: String,
    pub icon_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaskDialogState {
    pub title: String,
    pub card: AdaptiveCard,
    pub scope: CardScope,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CardOverride {
    pub basis: String,
    pub cards: Vec<AdaptiveCard>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Surface {
    Message,
    Dialog,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardScope {
    pub conversation_id: String,
    pub message_id: String,
    pub card_index: usize,
    pub surface: Surface,
}

impl CardScope {
    pub fn message(conversation_id: &str, message_id: &str, card_index: usize) -> Self {
        CardScope {
            conversation_id: conversation_id.to_owned(),
            message_id: message_id.to_owned(),
            card_index,
            surface: Surface::Message,
        }
    }

    pub fn card_key(&self) -> String {
        match self.surface {
            Surface::Message => format!("{}-card-{}", self.message_id, self.card_index),
            Surface::Dialog => Self::dialog_key(),
        }
    }

    pub fn input_key(&self, input_id: &str) -> String {
        format!("{}/{input_id}", self.card_key())
    }

    pub fn dialog_key() -> String {
        DIALOG_KEY.to_owned()
    }
}

#[derive(Default)]
pub struct CardState {
    phases: HashMap<String, ActionPhase>,
    visibility: HashMap<String, bool>,
    open_cards: HashMap<String, usize>,
    slots: HashMap<String, usize>,
    notes: HashMap<String, String>,
    overrides: HashMap<(String, String), CardOverride>,
    input_values: HashMap<String, String>,
    input_errors: HashMap<String, String>,
    input_fields: HashMap<String, FieldHandle>,
    refreshed: HashSet<(String, String)>,
    refreshes_in_flight: usize,
}

impl CardState {
    pub fn phase(&self, action_key: &str) -> Option<&ActionPhase> {
        self.phases.get(action_key)
    }

    pub fn is_busy(&self, action_key: &str) -> bool {
        self.phases.get(action_key) == Some(&ActionPhase::Busy)
    }

    pub fn set_phase(&mut self, action_key: &str, phase: ActionPhase) {
        self.phases.insert(action_key.to_owned(), phase);
    }

    pub fn clear_done(&mut self, action_key: &str) {
        if self.phases.get(action_key) == Some(&ActionPhase::Done) {
            self.phases.remove(action_key);
        }
    }

    pub fn clear_card(&mut self, card_key: &str) {
        self.phases.retain(|key, _| !key.starts_with(card_key));
        self.visibility.retain(|key, _| !key.starts_with(card_key));
        self.open_cards.retain(|key, _| !key.starts_with(card_key));
        self.slots.retain(|key, _| !key.starts_with(card_key));
        self.notes.remove(card_key);
        self.input_values
            .retain(|key, _| !key.starts_with(card_key));
        self.input_errors
            .retain(|key, _| !key.starts_with(card_key));
        self.input_fields
            .retain(|key, _| !key.starts_with(card_key));
    }

    pub fn has_input(&self, input_key: &str) -> bool {
        self.input_values.contains_key(input_key)
    }

    pub fn seed_input(&mut self, input_key: String, value: String, field: Option<FieldHandle>) {
        if let Some(field) = field {
            self.input_fields.insert(input_key.clone(), field);
        }
        self.input_values.insert(input_key, value);
    }

    pub fn input_value(&self, input_key: &str) -> &str {
        self.input_values.get(input_key).map_or("", String::as_str)
    }

    pub fn input_text(&self, input_key: &str, cx: &App) -> String {
        match self.input_fields.get(input_key) {
            Some(handle) => handle
                .field
                .text(cx)
                .unwrap_or_else(|| self.input_value(input_key).to_owned()),
            None => self.input_value(input_key).to_owned(),
        }
    }

    pub fn input_field(&self, input_key: &str) -> Option<&InputField> {
        self.input_fields.get(input_key).map(|handle| &handle.field)
    }

    pub fn set_input_value(&mut self, input_key: &str, value: String) -> bool {
        self.input_values.insert(input_key.to_owned(), value);
        self.input_errors.remove(input_key).is_some()
    }

    pub fn input_error(&self, input_key: &str) -> Option<&str> {
        self.input_errors.get(input_key).map(String::as_str)
    }

    pub fn set_input_error(&mut self, input_key: &str, message: String) {
        self.input_errors.insert(input_key.to_owned(), message);
    }

    pub fn clear_input_errors(&mut self, input_keys: &[String]) {
        for input_key in input_keys {
            self.input_errors.remove(input_key);
        }
    }

    pub fn is_visible(&self, element_key: &str, initial: bool) -> bool {
        self.visibility.get(element_key).copied().unwrap_or(initial)
    }

    pub fn toggle_visibility(&mut self, element_key: &str, initial: bool, forced: Option<bool>) {
        let next = forced.unwrap_or(!self.is_visible(element_key, initial));
        self.visibility.insert(element_key.to_owned(), next);
    }

    pub fn open_card(&self, actions_key: &str) -> Option<usize> {
        self.open_cards.get(actions_key).copied()
    }

    pub fn toggle_open_card(&mut self, actions_key: &str, index: usize) {
        if self.open_cards.get(actions_key) == Some(&index) {
            self.open_cards.remove(actions_key);
        } else {
            self.open_cards.insert(actions_key.to_owned(), index);
        }
    }

    pub fn slot(&self, slot_key: &str, default: usize) -> usize {
        self.slots.get(slot_key).copied().unwrap_or(default)
    }

    pub fn set_slot(&mut self, slot_key: &str, value: usize) {
        self.slots.insert(slot_key.to_owned(), value);
    }

    pub fn note(&self, card_key: &str) -> Option<&str> {
        self.notes.get(card_key).map(String::as_str)
    }

    pub fn set_note(&mut self, card_key: &str, text: String) {
        self.notes.insert(card_key.to_owned(), text);
    }

    pub fn clear_note(&mut self, card_key: &str) {
        self.notes.remove(card_key);
    }

    pub fn set_override(
        &mut self,
        conversation_id: &str,
        message_id: &str,
        replaced: CardOverride,
    ) {
        self.overrides.insert(
            (conversation_id.to_owned(), message_id.to_owned()),
            replaced,
        );
    }

    pub fn begin_refresh(&mut self, conversation_id: &str, message_id: &str, manual: bool) -> bool {
        let key = (conversation_id.to_owned(), message_id.to_owned());
        if !manual
            && (self.refreshed.contains(&key)
                || self.refreshes_in_flight >= MAX_REFRESHES_IN_FLIGHT)
        {
            return false;
        }
        self.refreshed.insert(key);
        self.refreshes_in_flight += 1;
        true
    }

    pub fn cancel_refresh(&mut self, conversation_id: &str, message_id: &str) {
        self.refreshed
            .remove(&(conversation_id.to_owned(), message_id.to_owned()));
        self.refreshes_in_flight = self.refreshes_in_flight.saturating_sub(1);
    }

    /// True when the throttle was full, so a deferred refresh may start now.
    pub fn finish_refresh(&mut self) -> bool {
        let was_full = self.refreshes_in_flight >= MAX_REFRESHES_IN_FLIGHT;
        self.refreshes_in_flight = self.refreshes_in_flight.saturating_sub(1);
        was_full
    }

    pub fn overrides_for(&self, conversation_id: &str) -> HashMap<String, CardOverride> {
        self.overrides
            .iter()
            .filter(|((conversation, _), _)| conversation == conversation_id)
            .map(|((_, message_id), replaced)| (message_id.clone(), replaced.clone()))
            .collect()
    }
}

pub fn short_reason(reason: &str) -> String {
    reason.chars().take(MAX_REASON_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn done_clears_but_busy_and_failed_stay() {
        let mut state = CardState::default();
        state.set_phase("a", ActionPhase::Done);
        state.set_phase("b", ActionPhase::Busy);
        state.set_phase("c", ActionPhase::Failed("nope".into()));
        for key in ["a", "b", "c"] {
            state.clear_done(key);
        }
        assert_eq!(state.phase("a"), None);
        assert!(state.is_busy("b"));
        assert_eq!(state.phase("c"), Some(&ActionPhase::Failed("nope".into())));
    }

    #[test]
    fn visibility_toggles_from_the_initial_value_or_is_forced() {
        let mut state = CardState::default();
        assert!(!state.is_visible("e", false));
        state.toggle_visibility("e", false, None);
        assert!(state.is_visible("e", false));
        state.toggle_visibility("e", false, None);
        assert!(!state.is_visible("e", false));
        state.toggle_visibility("e", false, Some(true));
        state.toggle_visibility("e", false, Some(true));
        assert!(state.is_visible("e", false));
    }

    #[test]
    fn one_show_card_is_open_per_action_set() {
        let mut state = CardState::default();
        state.toggle_open_card("set", 1);
        assert_eq!(state.open_card("set"), Some(1));
        state.toggle_open_card("set", 2);
        assert_eq!(state.open_card("set"), Some(2));
        state.toggle_open_card("set", 2);
        assert_eq!(state.open_card("set"), None);
    }

    #[test]
    fn slots_default_until_set_and_clear_with_their_card() {
        let mut state = CardState::default();
        assert_eq!(state.slot("m-card-0-3", 2), 2);
        state.set_slot("m-card-0-3", 5);
        assert_eq!(state.slot("m-card-0-3", 2), 5);
        state.clear_card("m-card-0");
        assert_eq!(state.slot("m-card-0-3", 2), 2);
    }

    #[test]
    fn overrides_are_scoped_to_their_conversation() {
        let mut state = CardState::default();
        let replaced = CardOverride {
            basis: "[]".into(),
            cards: Vec::new(),
        };
        state.set_override("c1", "m1", replaced.clone());
        assert_eq!(state.overrides_for("c1").get("m1"), Some(&replaced));
        assert!(state.overrides_for("c2").is_empty());
    }

    #[test]
    fn a_message_refreshes_once_unless_asked_manually() {
        let mut state = CardState::default();
        assert!(state.begin_refresh("c", "m", false));
        assert!(!state.begin_refresh("c", "m", false));
        assert!(state.begin_refresh("c", "m", true));
        assert!(state.begin_refresh("c", "other", false));
    }

    #[test]
    fn at_most_three_automatic_refreshes_run_at_once() {
        let mut state = CardState::default();
        for message_id in ["a", "b", "c"] {
            assert!(state.begin_refresh("c", message_id, false));
        }
        assert!(!state.begin_refresh("c", "d", false));
        assert!(state.begin_refresh("c", "d", true));
        assert!(state.finish_refresh());
        assert!(state.finish_refresh());
        assert!(!state.finish_refresh());
        assert!(state.begin_refresh("c", "e", false));
    }

    #[test]
    fn a_cancelled_refresh_may_start_again() {
        let mut state = CardState::default();
        assert!(state.begin_refresh("c", "m", false));
        state.cancel_refresh("c", "m");
        assert!(state.begin_refresh("c", "m", false));
    }

    #[test]
    fn reasons_are_shortened() {
        assert_eq!(
            short_reason(&"x".repeat(500)).chars().count(),
            MAX_REASON_CHARS
        );
    }

    #[test]
    fn card_keys_differ_by_surface_and_index() {
        let message = CardScope::message("c", "m", 2);
        assert_eq!(message.card_key(), "m-card-2");
        let dialog = CardScope {
            surface: Surface::Dialog,
            ..message
        };
        assert_eq!(dialog.card_key(), "task-dialog");
    }
}
