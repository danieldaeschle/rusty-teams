(async (clientId) => {
  const entries = [];
  const sealed = [];
  let activeHome = null;
  const take = (key, entry) => {
    if (key.toLowerCase().endsWith('active-account-filters') && entry.homeAccountId) activeHome = entry.homeAccountId;
    if (typeof entry.secret === 'string') entries.push(entry);
  };
  for (let index = 0; index < localStorage.length; index++) {
    const key = localStorage.key(index);
    let entry;
    try { entry = JSON.parse(localStorage.getItem(key)); } catch (error) { continue; }
    if (!entry || typeof entry !== 'object') continue;
    if (entry.id && entry.nonce && entry.data) sealed.push([key, entry]);
    else take(key, entry);
  }

  // Keep this in sync with session/assets/fetch.js: newer MSAL.js seals its cache.
  const cookie = document.cookie.split(';').map((part) => part.trim()).find((part) => part.startsWith('msal.cache.encryption='));
  let sealing = null;
  try { sealing = cookie && JSON.parse(decodeURIComponent(cookie.slice(cookie.indexOf('=') + 1))); } catch (error) {}
  if (sealed.length && sealing && sealing.key && sealing.id) {
    const bytes = (text) => Uint8Array.from(atob(text.replace(/-/g, '+').replace(/_/g, '/').padEnd(Math.ceil(text.length / 4) * 4, '=')),
      (character) => character.charCodeAt(0));
    const base = await crypto.subtle.importKey('raw', bytes(sealing.key), 'HKDF', false, ['deriveKey']);
    for (const [key, entry] of sealed) {
      if (entry.id !== sealing.id) continue;
      const contexts = [...(key.match(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi) || []), ''];
      for (const context of new Set(contexts)) {
        try {
          const derived = await crypto.subtle.deriveKey(
            {name: 'HKDF', salt: bytes(entry.nonce), hash: 'SHA-256', info: new TextEncoder().encode(context)},
            base, {name: 'AES-GCM', length: 256}, false, ['decrypt']);
          const data = await crypto.subtle.decrypt({name: 'AES-GCM', iv: new Uint8Array(12)}, derived, bytes(entry.data));
          take(key, JSON.parse(new TextDecoder().decode(data)));
          break;
        } catch (error) {}
      }
    }
  }

  const wantedClient = clientId.toLowerCase();
  const refreshTokens = entries
    .filter((entry) => entry.credentialType === 'RefreshToken')
    .filter((entry) => String(entry.clientId || '').toLowerCase() === wantedClient)
    .filter((entry) => !activeHome || entry.homeAccountId === activeHome)
    .filter((entry) => !entry.expiresOn || Number(entry.expiresOn) * 1000 > Date.now())
    .sort((first, second) => Number(second.lastUpdatedAt || 0) - Number(first.lastUpdatedAt || 0));
  return refreshTokens[0] ? refreshTokens[0].secret : null;
})
