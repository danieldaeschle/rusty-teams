//! Decides that Teams sign-in is finished only when the page can use a token.
//!
//! Local storage key names that contain `accesstoken` or `refreshtoken` are not enough: a wiped
//! cache still has those names, and treating them as success restarts Chromium and drops the
//! encryption cookie.

use serde_json::Value;

/// Page script. Returns `{opened, freshAccess}`. `opened` counts sealed entries decrypted with
/// the `msal.cache.encryption` cookie. `freshAccess` is a bearer access token whose `expiresOn`
/// is still in the future, in the clear or among the opened entries.
pub(crate) const PAGE_EXPRESSION: &str = r#"(async () => {
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
  const openedEntries = [];
  const cookie = document.cookie.split(';').map((part) => part.trim()).find((part) => part.startsWith('msal.cache.encryption='));
  let sealing = null;
  try { sealing = cookie && JSON.parse(decodeURIComponent(cookie.slice(cookie.indexOf('=') + 1))); } catch (error) {}
  if (sealing && sealing.key && sealing.id && sealed.length) {
    const bytes = (text) => Uint8Array.from(atob(text.replace(/-/g, '+').replace(/_/g, '/').padEnd(Math.ceil(text.length / 4) * 4, '=')), (character) => character.charCodeAt(0));
    const base = await crypto.subtle.importKey('raw', bytes(sealing.key), 'HKDF', false, ['deriveKey']);
    for (const [key, entry] of sealed) {
      if (entry.id !== sealing.id) continue;
      const contexts = [...(String(key).match(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi) || []), ''];
      for (const context of new Set(contexts)) {
        try {
          const derived = await crypto.subtle.deriveKey(
            {name: 'HKDF', salt: bytes(entry.nonce), hash: 'SHA-256', info: new TextEncoder().encode(context)},
            base, {name: 'AES-GCM', length: 256}, false, ['decrypt']);
          const data = await crypto.subtle.decrypt({name: 'AES-GCM', iv: new Uint8Array(12)}, derived, bytes(entry.data));
          openedEntries.push(JSON.parse(new TextDecoder().decode(data)));
          break;
        } catch (error) {}
      }
    }
  }
  const now = Date.now();
  const fresh = (entry) => entry && entry.credentialType === 'AccessToken' && (!entry.tokenType || entry.tokenType === 'Bearer') && Number(entry.expiresOn) * 1000 > now && typeof entry.secret === 'string' && entry.secret.length > 0;
  return {opened: openedEntries.length, freshAccess: plain.some(fresh) || openedEntries.some(fresh)};
})()"#;

/// True when the page opened a sealed entry or still holds a valid access token.
/// A `keyNames` flag is ignored on purpose.
pub(crate) fn sign_in_ready_from_page(value: &Value) -> bool {
    let opened = value.get("opened").and_then(Value::as_u64).unwrap_or(0);
    let fresh_access = value
        .get("freshAccess")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    opened > 0 || fresh_access
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
    fn an_opened_sealed_entry_is_ready() {
        assert!(sign_in_ready_from_page(
            &json!({"opened": 1, "freshAccess": false})
        ));
    }

    #[test]
    fn a_fresh_access_token_is_ready_without_sealed_entries() {
        assert!(sign_in_ready_from_page(
            &json!({"opened": 0, "freshAccess": true})
        ));
    }

    #[test]
    fn a_missing_report_is_not_ready() {
        assert!(!sign_in_ready_from_page(&json!({})));
        assert!(!sign_in_ready_from_page(&json!(null)));
    }
}
