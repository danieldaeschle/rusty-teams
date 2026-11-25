//! Keeps the key of the Teams token cache across browser restarts (issue #84).
//!
//! MSAL.js encrypts its localStorage cache with a key held in the session cookie
//! `msal.cache.encryption`. A session cookie dies with the process, so every restart (headless
//! hand-over, login window, hung renderer, app exit) made MSAL create a new key, drop the cache
//! and ask for sign-in again. Before Chromium stops, the cookie gets an expiry instead; the key
//! then sits in the profile next to the cache it protects, which the profile already holds.

use serde_json::{Map, Value, json};

/// The cookie MSAL.js reads its cache encryption key from; only its name and value matter to it.
const MSAL_KEY_COOKIE: &str = "msal.cache.encryption";
/// How long the key outlives the last stop: the refresh token it unlocks lasts up to 90 days of
/// inactivity, and every stop renews the expiry.
const KEY_LIFETIME_SECS: u64 = 90 * 24 * 60 * 60;
/// `Network.Cookie` fields that `Network.CookieParam` takes unchanged (CDP browser_protocol.json).
const CARRIED_FIELDS: [&str; 8] = [
    "name",
    "value",
    "path",
    "secure",
    "httpOnly",
    "sameSite",
    "priority",
    "partitionKey",
];

/// `Storage.setCookies` parameters that give every MSAL key cookie in a `Storage.getCookies`
/// result an expiry `KEY_LIFETIME_SECS` after `now_secs`, or `None` when there is none.
pub(crate) fn persisted_key_cookies(get_cookies: &Value, now_secs: u64) -> Option<Value> {
    let expires = now_secs + KEY_LIFETIME_SECS;
    let cookies: Vec<Value> = get_cookies["cookies"]
        .as_array()?
        .iter()
        .filter(|cookie| cookie["name"] == MSAL_KEY_COOKIE)
        .filter_map(|cookie| cookie_param(cookie, expires))
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
        for not_carried in ["sourceScheme", "sourcePort"] {
            assert!(
                cookie.get(not_carried).is_none(),
                "{not_carried} is not carried by cookie_param"
            );
        }
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
}
