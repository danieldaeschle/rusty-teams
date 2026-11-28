//! Keeps the key of the Teams token cache across app restarts (issue #84).
//!
//! MSAL.js encrypts its localStorage cache with a key held in the session cookie
//! `msal.cache.encryption`. A session cookie dies with the page, so every restart made MSAL create
//! a new key, drop the cache and ask for sign-in again. While the page is signed in, the cookie
//! gets an expiry instead; the key then sits in the profile next to the cache it protects, which
//! the profile already holds. The managed Chromium path also does this once more on a graceful
//! stop as a second chance.
//!
//! Trade-off: the key moves from a session cookie into the profile's cookie store, encrypted by
//! the OS keyring (or the browser's fixed key where no keyring is available). The profile already
//! holds the equally powerful ESTS cookies that keep the session alive, so this is a conscious
//! choice, not a new credential at rest.

use std::collections::HashSet;

use serde_json::{Map, Value, json};

/// The cookie MSAL.js reads its cache encryption key from; only its name and value matter to it.
const MSAL_KEY_COOKIE: &str = "msal.cache.encryption";
/// How long the key outlives the last stop; every stop renews the expiry. 90 days is the chosen
/// value, not a refresh-token lifetime: Entra gives SPA clients refresh tokens valid for 24 hours,
/// and the long-lived session comes from the ESTS cookie, which enables silent token renewal.
const KEY_LIFETIME_SECS: u64 = 90 * 24 * 60 * 60;
/// `Network.Cookie` fields that `Network.CookieParam` takes unchanged (CDP browser_protocol.json).
const CARRIED_FIELDS: [&str; 10] = [
    "name",
    "value",
    "path",
    "secure",
    "httpOnly",
    "sameSite",
    "priority",
    "partitionKey",
    "sourceScheme",
    "sourcePort",
];

/// `Storage.setCookies` parameters for the current MSAL key cookies. Duplicates by
/// `(domain, path, partitionKey)` are collapsed, preferring host-only cookies over stale domain
/// cookies that could otherwise shadow the active key.
pub fn persisted_key_cookies(get_cookies: &Value, now_secs: u64) -> Option<Value> {
    let expires = now_secs + KEY_LIFETIME_SECS;
    let mut candidates: Vec<&Value> = get_cookies["cookies"]
        .as_array()?
        .iter()
        .filter(|cookie| cookie["name"] == MSAL_KEY_COOKIE)
        .collect();
    candidates.sort_by_key(|cookie| {
        cookie["domain"]
            .as_str()
            .is_some_and(|domain| domain.starts_with('.'))
    });
    let mut seen = HashSet::new();
    let cookies: Vec<Value> = candidates
        .into_iter()
        .filter_map(|cookie| {
            let key = (
                cookie["domain"]
                    .as_str()
                    .unwrap_or("")
                    .trim_start_matches('.')
                    .to_owned(),
                cookie["path"].as_str().unwrap_or("/").to_owned(),
                cookie
                    .get("partitionKey")
                    .map(Value::to_string)
                    .unwrap_or_default(),
            );
            seen.insert(key).then(|| cookie_param(cookie, expires))?
        })
        .collect();
    (!cookies.is_empty()).then(|| json!({ "cookies": cookies }))
}

fn cookie_param(cookie: &Value, expires: u64) -> Option<Value> {
    let domain = cookie["domain"].as_str()?;
    let path = cookie["path"].as_str().unwrap_or("/");
    let mut param: Map<String, Value> = CARRIED_FIELDS
        .iter()
        .filter_map(|field| Some(((*field).to_owned(), cookie.get(*field)?.clone())))
        .collect();
    if domain.starts_with('.') {
        param.insert("domain".into(), Value::String(domain.to_owned()));
    } else {
        // A host-only cookie set through `domain` would turn into a domain cookie; `url` keeps it host-only.
        param.insert(
            "url".into(),
            Value::String(format!("https://{domain}{path}")),
        );
    }
    param.insert("expires".into(), json!(expires));
    Some(Value::Object(param))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000;

    fn get_cookies() -> Value {
        json!({"cookies": [
            {"name": "msal.cache.encryption", "value": "{\"id\":\"k\",\"key\":\"secret\"}",
             "domain": "teams.cloud.microsoft", "path": "/", "expires": -1, "size": 40,
             "httpOnly": false, "secure": true, "session": true, "sameSite": "None",
             "priority": "Medium", "sourceScheme": "Secure", "sourcePort": 443},
            {"name": "other", "value": "x", "domain": ".microsoft.com", "path": "/",
             "expires": -1, "size": 6, "httpOnly": true, "secure": true, "session": true,
             "priority": "Medium", "sourceScheme": "Secure", "sourcePort": 443}
        ]})
    }

    #[test]
    fn the_msal_key_cookie_gets_an_expiry_and_keeps_its_attributes() {
        let params = persisted_key_cookies(&get_cookies(), NOW).unwrap();
        let cookies = params["cookies"].as_array().unwrap();
        assert_eq!(cookies.len(), 1, "only the MSAL key cookie: {params}");
        let cookie = &cookies[0];
        assert_eq!(cookie["name"], MSAL_KEY_COOKIE);
        assert_eq!(cookie["value"], "{\"id\":\"k\",\"key\":\"secret\"}");
        assert_eq!(cookie["expires"], NOW + KEY_LIFETIME_SECS);
        assert_eq!(cookie["secure"], true);
        assert_eq!(cookie["sameSite"], "None");
        assert_eq!(cookie["url"], "https://teams.cloud.microsoft/");
        assert!(
            cookie.get("domain").is_none(),
            "host-only stays host-only: {cookie}"
        );
        for read_only in ["size", "session"] {
            assert!(
                cookie.get(read_only).is_none(),
                "{read_only} is not a CookieParam field"
            );
        }
        assert_eq!(cookie["sourceScheme"], "Secure");
        assert_eq!(cookie["sourcePort"], 443);
    }

    #[test]
    fn a_domain_cookie_keeps_its_domain() {
        let mut result = get_cookies();
        result["cookies"][0]["domain"] = json!(".teams.cloud.microsoft");
        let params = persisted_key_cookies(&result, NOW).unwrap();
        assert_eq!(params["cookies"][0]["domain"], ".teams.cloud.microsoft");
        assert!(params["cookies"][0].get("url").is_none());
    }

    #[test]
    fn without_the_msal_key_cookie_nothing_is_set() {
        let mut result = get_cookies();
        result["cookies"].as_array_mut().unwrap().remove(0);
        assert_eq!(persisted_key_cookies(&result, NOW), None);
        assert_eq!(persisted_key_cookies(&json!({}), NOW), None);
    }

    #[test]
    fn host_only_key_wins_over_a_stale_domain_duplicate() {
        let mut result = get_cookies();
        let stale = json!({
            "name": MSAL_KEY_COOKIE, "value": "stale", "domain": ".teams.cloud.microsoft",
            "path": "/", "expires": -1, "size": 4, "httpOnly": false, "secure": true,
            "session": true, "sameSite": "None", "priority": "Medium",
            "sourceScheme": "Secure", "sourcePort": 443
        });
        result["cookies"].as_array_mut().unwrap().insert(1, stale);
        let params = persisted_key_cookies(&result, NOW).unwrap();
        let cookies = params["cookies"].as_array().unwrap();
        assert_eq!(cookies.len(), 1, "{params}");
        assert_eq!(cookies[0]["value"], "{\"id\":\"k\",\"key\":\"secret\"}");
        assert!(cookies[0].get("domain").is_none());
    }

    #[test]
    fn partition_key_separates_key_cookies() {
        let mut result = get_cookies();
        let mut partitioned = result["cookies"][0].clone();
        partitioned["partitionKey"] = json!({"topLevelSite": "https://teams.cloud.microsoft"});
        result["cookies"].as_array_mut().unwrap().push(partitioned);
        let params = persisted_key_cookies(&result, NOW).unwrap();
        let cookies = params["cookies"].as_array().unwrap();
        assert_eq!(cookies.len(), 2, "{params}");
        assert!(cookies[0].get("partitionKey").is_none());
        assert!(cookies[1].get("partitionKey").is_some());
    }

    #[test]
    fn an_existing_expiry_is_renewed() {
        let mut result = get_cookies();
        result["cookies"][0]["expires"] = json!(NOW + 100);
        let params = persisted_key_cookies(&result, NOW).unwrap();
        assert_eq!(params["cookies"][0]["expires"], NOW + KEY_LIFETIME_SECS);
    }

    #[test]
    fn a_cookie_without_a_domain_is_skipped() {
        let mut result = get_cookies();
        result["cookies"][0]
            .as_object_mut()
            .unwrap()
            .remove("domain");
        assert_eq!(persisted_key_cookies(&result, NOW), None);
    }
}
