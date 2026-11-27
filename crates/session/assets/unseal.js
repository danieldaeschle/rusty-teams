// Newer MSAL.js seals its localStorage cache: AES-GCM under a key from the msal.cache.encryption session cookie.
// Answers [key, entry] for every sealed entry it could open. `memo` keeps opened entries by nonce.
async (sealed, memo) => {
  const opened = [];
  const cookie = document.cookie.split(';').map((part) => part.trim()).find((part) => part.startsWith('msal.cache.encryption='));
  let sealing = null;
  try { sealing = cookie && JSON.parse(decodeURIComponent(cookie.slice(cookie.indexOf('=') + 1))); } catch (error) {}
  if (!sealing || !sealing.key || !sealing.id || !sealed.length) return opened;
  const bytes = (text) => Uint8Array.from(atob(text.replace(/-/g, '+').replace(/_/g, '/').padEnd(Math.ceil(text.length / 4) * 4, '=')),
    (character) => character.charCodeAt(0));
  const base = await crypto.subtle.importKey('raw', bytes(sealing.key), 'HKDF', false, ['deriveKey']);
  for (const [key, entry] of sealed) {
    if (entry.id !== sealing.id) continue;
    if (!memo.has(entry.nonce)) {
      // MSAL binds an entry to the client id in its key, or to an empty context when the key has none.
      const contexts = [...(String(key).match(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi) || []), ''];
      for (const context of new Set(contexts)) {
        try {
          const derived = await crypto.subtle.deriveKey(
            {name: 'HKDF', salt: bytes(entry.nonce), hash: 'SHA-256', info: new TextEncoder().encode(context)},
            base, {name: 'AES-GCM', length: 256}, false, ['decrypt']);
          const data = await crypto.subtle.decrypt({name: 'AES-GCM', iv: new Uint8Array(12)}, derived, bytes(entry.data));
          memo.set(entry.nonce, JSON.parse(new TextDecoder().decode(data)));
          break;
        } catch (error) {}
      }
    }
    if (memo.has(entry.nonce)) opened.push([key, memo.get(entry.nonce)]);
  }
  return opened;
}
