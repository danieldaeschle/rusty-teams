//! Decides that Teams sign-in is finished only when the page holds a fresh access token.
//!
//! Local storage key names that contain `accesstoken` or `refreshtoken` are not enough: a wiped
//! cache still has those names, and treating them as success restarts Chromium and drops the
//! encryption cookie. Merely decrypting sealed entries is not enough either once the encryption
//! cookie persists across restarts, because an expired session still unseals its old entries.

use std::sync::LazyLock;

use serde_json::Value;

/// Page script. Returns `{opened, freshAccess}`. `opened` counts sealed entries decrypted with
/// the `msal.cache.encryption` cookie. `freshAccess` is a bearer access token whose `expiresOn`
/// is still in the future, in the clear or among the opened entries.
pub(crate) static PAGE_EXPRESSION: LazyLock<String> =
    LazyLock::new(|| PAGE_TEMPLATE.replace("__UNSEAL__", session::UNSEAL_SCRIPT));

const PAGE_TEMPLATE: &str = r#"(async () => {
  const sealed = [];
  const plain = [];
  for (let index = 0; index < localStorage.length; index++) {
    const key = localStorage.key(index);
    let entry;
    try { entry = JSON.parse(localStorage.getItem(key)); } catch (error) { continue; }
    if (!entry || typeof entry !== 'object') continue;
    if (entry.id && entry.nonce && entry.data) sealed.push([key, entry]);
    else plain.push(entry);
  }
  const unsealEntries = __UNSEAL__;
  const openedEntries = (await unsealEntries(sealed, new Map())).map(([key, entry]) => entry);
  const now = Date.now();
  const fresh = (entry) => entry && entry.credentialType === 'AccessToken' && (!entry.tokenType || entry.tokenType === 'Bearer') && Number(entry.expiresOn) * 1000 > now && typeof entry.secret === 'string' && entry.secret.length > 0;
  return {opened: openedEntries.length, freshAccess: plain.some(fresh) || openedEntries.some(fresh)};
})()"#;

/// True when the page holds a valid access token, either plain or decrypted from a sealed entry.
/// Key names and expired unsealed entries are ignored on purpose.
pub(crate) fn sign_in_ready_from_page(value: &Value) -> bool {
    value
        .get("freshAccess")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn key_names_without_a_usable_token_are_not_ready() {
        let report = json!({"opened": 0, "freshAccess": false, "keyNames": true});
        assert!(!sign_in_ready_from_page(&report));
    }

    #[test]
    fn opened_sealed_entries_without_a_fresh_access_token_are_not_ready() {
        assert!(!sign_in_ready_from_page(
            &json!({"opened": 3, "freshAccess": false})
        ));
    }

    #[test]
    fn a_fresh_access_token_is_ready_plain_or_sealed() {
        assert!(sign_in_ready_from_page(
            &json!({"opened": 0, "freshAccess": true})
        ));
        assert!(sign_in_ready_from_page(
            &json!({"opened": 2, "freshAccess": true})
        ));
    }

    #[test]
    fn a_missing_report_is_not_ready() {
        assert!(!sign_in_ready_from_page(&json!({})));
        assert!(!sign_in_ready_from_page(&json!(null)));
    }
}
