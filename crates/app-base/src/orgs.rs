//! The organizations of the signed-in account and which one the window shows.
//!
//! Lives in its own file beside the caches, because it decides which cache to open.
//! The home tenant keeps `cache.sqlite3`; a guest tenant gets `cache-<tenant id>.sqlite3`.
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const ORGS_FILE: &str = "orgs.json";
/// Start flag that opens one chat once its org has loaded, used when a toast of another org is clicked.
pub const OPEN_CONVERSATION_FLAG: &str = "--open-conversation";
const HOME_DATABASE: &str = "cache.sqlite3";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Org {
    pub id: String,
    pub name: String,
    pub guest: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Orgs {
    /// Tenant id of the org the window shows; `None` is the home tenant.
    pub active: Option<String>,
    pub list: Vec<Org>,
}

impl Orgs {
    pub fn path() -> Option<PathBuf> {
        crate::support::local_data_dir().map(|root| root.join(ORGS_FILE))
    }

    /// A missing or unreadable file is the home tenant with no known orgs.
    pub fn load(path: &Path) -> Orgs {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, bytes)
    }

    /// The active org when it is a guest tenant, the only case that needs its own tokens and cache.
    pub fn active_guest(&self) -> Option<&Org> {
        let active = self.active.as_deref()?;
        self.list
            .iter()
            .find(|org| org.guest && org.id == active && is_tenant_id(&org.id))
    }

    pub fn is_active(&self, org: &Org) -> bool {
        match self.active_guest() {
            Some(active) => active.id == org.id,
            None => !org.guest,
        }
    }

    pub fn database_file(&self) -> String {
        match self.active_guest() {
            Some(org) => format!("cache-{}.sqlite3", org.id),
            None => HOME_DATABASE.to_owned(),
        }
    }

    /// Takes the account's current org list; an active org that is gone falls back to home.
    pub fn replace_list(&mut self, list: Vec<Org>) {
        self.list = list;
        if self.active_guest().is_none() {
            self.active = None;
        }
    }

    pub fn activate(&mut self, org: &Org) {
        self.active = org.guest.then(|| org.id.clone());
    }
}

/// Tenant ids are GUIDs; anything else must not end up in a file name.
fn is_tenant_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|character| character.is_ascii_hexdigit() || character == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn org(id: &str, guest: bool) -> Org {
        Org {
            id: id.into(),
            name: id.into(),
            guest,
        }
    }

    #[test]
    fn a_guest_org_gets_its_own_cache_and_home_keeps_the_old_one() {
        let mut orgs = Orgs {
            active: None,
            list: vec![org("aaaa-01", false), org("bbbb-02", true)],
        };
        assert_eq!(orgs.database_file(), "cache.sqlite3");
        assert!(orgs.is_active(&orgs.list[0].clone()));
        orgs.activate(&orgs.list[1].clone());
        assert_eq!(orgs.database_file(), "cache-bbbb-02.sqlite3");
        assert!(orgs.is_active(&orgs.list[1].clone()));
        orgs.activate(&orgs.list[0].clone());
        assert_eq!(orgs.active, None);
    }

    #[test]
    fn an_unknown_or_unsafe_active_org_is_the_home_tenant() {
        let mut orgs = Orgs {
            active: Some("../evil".into()),
            list: vec![org("../evil", true)],
        };
        assert_eq!(orgs.database_file(), "cache.sqlite3");
        orgs.active = Some("bbbb-02".into());
        orgs.replace_list(vec![org("aaaa-01", false)]);
        assert_eq!(orgs.active, None);
    }
}
