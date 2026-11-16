use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::Result;
use crate::store::Store;

pub const CALENDAR_ACCOUNTS_KEY: &str = "calendar.accounts";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CalendarProvider {
    #[default]
    Outlook,
    Google,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarRef {
    pub provider: CalendarProvider,
    /// Stable account id (empty for the single Outlook mailbox).
    #[serde(default)]
    pub account_id: String,
    /// Provider calendar id (`primary` for Google default, empty for Outlook).
    #[serde(default)]
    pub calendar_id: String,
}

impl CalendarRef {
    pub fn outlook() -> Self {
        CalendarRef {
            provider: CalendarProvider::Outlook,
            account_id: String::new(),
            calendar_id: String::new(),
        }
    }

    pub fn google(account_id: impl Into<String>, calendar_id: impl Into<String>) -> Self {
        CalendarRef {
            provider: CalendarProvider::Google,
            account_id: account_id.into(),
            calendar_id: calendar_id.into(),
        }
    }

    pub fn key(&self) -> String {
        format!(
            "{}:{}:{}",
            match self.provider {
                CalendarProvider::Outlook => "outlook",
                CalendarProvider::Google => "google",
            },
            self.account_id,
            self.calendar_id
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoogleCalendarInfo {
    pub id: String,
    pub summary: String,
    #[serde(default = "default_true")]
    pub selected: bool,
    #[serde(default)]
    pub primary: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GoogleTokenSet {
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    /// Unix seconds when the access token expires.
    #[serde(default)]
    pub expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoogleAccount {
    pub id: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub calendars: Vec<GoogleCalendarInfo>,
    #[serde(default)]
    pub tokens: GoogleTokenSet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarAccounts {
    #[serde(default = "CalendarRef::outlook")]
    pub home: CalendarRef,
    /// Extra write targets beyond home (may include Google calendars).
    #[serde(default)]
    pub write_targets: Vec<CalendarRef>,
    /// Show Outlook mailbox events in the calendar grid.
    #[serde(default = "default_true")]
    pub outlook_selected: bool,
    #[serde(default)]
    pub google_accounts: Vec<GoogleAccount>,
}

impl Default for CalendarAccounts {
    fn default() -> Self {
        CalendarAccounts {
            home: CalendarRef::outlook(),
            write_targets: Vec::new(),
            outlook_selected: true,
            google_accounts: Vec::new(),
        }
    }
}

impl CalendarAccounts {
    pub fn from_json(text: &str) -> std::result::Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| json!({}).to_string())
    }

    pub fn has_google(&self) -> bool {
        !self.google_accounts.is_empty()
    }

    pub fn google_account(&self, id: &str) -> Option<&GoogleAccount> {
        self.google_accounts.iter().find(|account| account.id == id)
    }

    pub fn google_account_mut(&mut self, id: &str) -> Option<&mut GoogleAccount> {
        self.google_accounts
            .iter_mut()
            .find(|account| account.id == id)
    }

    pub fn disconnect_google(&mut self, account_id: &str) {
        self.google_accounts
            .retain(|account| account.id != account_id);
        if self.home.provider == CalendarProvider::Google && self.home.account_id == account_id {
            self.home = CalendarRef::outlook();
        }
        self.write_targets.retain(|target| {
            !(target.provider == CalendarProvider::Google && target.account_id == account_id)
        });
    }

    /// Home plus extra write targets, de-duplicated by key.
    pub fn resolved_write_targets(&self) -> Vec<CalendarRef> {
        let mut targets = vec![self.home.clone()];
        for target in &self.write_targets {
            if targets
                .iter()
                .all(|existing| existing.key() != target.key())
            {
                targets.push(target.clone());
            }
        }
        targets
    }
}

pub fn load_calendar_accounts(store: &Store) -> CalendarAccounts {
    store
        .meta(CALENDAR_ACCOUNTS_KEY)
        .ok()
        .flatten()
        .and_then(|json| CalendarAccounts::from_json(&json).ok())
        .unwrap_or_default()
}

pub fn save_calendar_accounts(store: &Store, accounts: &CalendarAccounts) -> Result<()> {
    store.set_meta(CALENDAR_ACCOUNTS_KEY, &accounts.to_json())
}

/// Save Google tokens refreshed during a fetch without touching anything else.
///
/// A fetch works on a copy of the accounts it read before it started. Saving that copy
/// back would undo a calendar toggle the user made meanwhile, so only tokens are merged
/// into the accounts as they are stored now.
pub fn merge_google_tokens(store: &Store, fetched: &CalendarAccounts) -> Result<()> {
    let mut current = load_calendar_accounts(store);
    let mut changed = false;
    for account in &mut current.google_accounts {
        if let Some(fresh) = fetched.google_account(&account.id)
            && fresh.tokens != account.tokens
        {
            account.tokens = fresh.tokens.clone();
            changed = true;
        }
    }
    if changed {
        save_calendar_accounts(store, &current)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use tempfile::tempdir;

    #[test]
    fn defaults_to_outlook_home() {
        let accounts = CalendarAccounts::default();
        assert_eq!(accounts.home.provider, CalendarProvider::Outlook);
        assert!(accounts.write_targets.is_empty());
        assert!(!accounts.has_google());
    }

    #[test]
    fn roundtrips_through_store_meta() {
        let directory = tempdir().unwrap();
        let store = Store::open(&directory.path().join("db.sqlite")).unwrap();
        let mut accounts = CalendarAccounts::default();
        accounts.google_accounts.push(GoogleAccount {
            id: "g1".into(),
            email: "joe@example.com".into(),
            calendars: vec![GoogleCalendarInfo {
                id: "primary".into(),
                summary: "Joe".into(),
                selected: true,
                primary: true,
            }],
            tokens: GoogleTokenSet {
                access_token: "access".into(),
                refresh_token: "refresh".into(),
                expires_at: 1,
            },
        });
        accounts.home = CalendarRef::google("g1", "primary");
        accounts.write_targets.push(CalendarRef::outlook());
        save_calendar_accounts(&store, &accounts).unwrap();
        let loaded = load_calendar_accounts(&store);
        assert_eq!(loaded.home, accounts.home);
        assert_eq!(loaded.google_accounts[0].email, "joe@example.com");
        assert_eq!(loaded.resolved_write_targets().len(), 2);
    }

    #[test]
    fn disconnect_clears_home_and_write_targets() {
        let mut accounts = CalendarAccounts {
            home: CalendarRef::google("g1", "primary"),
            write_targets: vec![CalendarRef::google("g1", "work")],
            outlook_selected: true,
            google_accounts: vec![GoogleAccount {
                id: "g1".into(),
                email: "a@b.c".into(),
                calendars: Vec::new(),
                tokens: GoogleTokenSet::default(),
            }],
        };
        accounts.disconnect_google("g1");
        assert!(!accounts.has_google());
        assert_eq!(accounts.home.provider, CalendarProvider::Outlook);
        assert!(accounts.write_targets.is_empty());
    }

    #[test]
    fn token_merge_keeps_a_toggle_made_during_the_fetch() {
        let directory = tempdir().unwrap();
        let store = Store::open(&directory.path().join("db.sqlite")).unwrap();
        let mut stored = CalendarAccounts::default();
        stored.google_accounts.push(GoogleAccount {
            id: "g1".into(),
            email: "me@example.com".into(),
            calendars: vec![GoogleCalendarInfo {
                id: "work".into(),
                summary: "Work".into(),
                selected: true,
                primary: false,
            }],
            tokens: GoogleTokenSet::default(),
        });
        save_calendar_accounts(&store, &stored).unwrap();
        // The fetch starts from this copy and refreshes the token.
        let mut fetched = load_calendar_accounts(&store);
        fetched.google_accounts[0].tokens.access_token = "fresh".into();
        // Meanwhile the user turns the calendar off and Outlook off.
        stored.google_accounts[0].calendars[0].selected = false;
        stored.outlook_selected = false;
        save_calendar_accounts(&store, &stored).unwrap();

        merge_google_tokens(&store, &fetched).unwrap();

        let merged = load_calendar_accounts(&store);
        assert!(!merged.outlook_selected);
        assert!(!merged.google_accounts[0].calendars[0].selected);
        assert_eq!(merged.google_accounts[0].tokens.access_token, "fresh");
    }
}
