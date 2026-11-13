(async ({requests, resource, scope, marginMs, concurrency, maxBytes, forceRefresh, nestedApp}) => {
  const fresh = (expires) => expires > Date.now() + marginMs;
  const normalize = (value) => value.toLowerCase().replace(/\/\/+(?=[^/]*$)/, '/');
  const covers = (value, name) => value === name || value === name + '.all';
  const entries = [];
  const sealed = [];
  let activeHome = null;
  let opened = 0;
  const take = (key, entry) => {
    if (key.endsWith('active-account-filters') && entry.homeAccountId) activeHome = entry.homeAccountId;
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
  const plain = entries.length;
  // Newer MSAL.js seals its localStorage cache: AES-GCM under a key from the msal.cache.encryption session cookie.
  const unseal = async () => {
    const cookie = document.cookie.split(';').map((part) => part.trim()).find((part) => part.startsWith('msal.cache.encryption='));
    let sealing = null;
    try { sealing = cookie && JSON.parse(decodeURIComponent(cookie.slice(cookie.indexOf('=') + 1))); } catch (error) {}
    if (!sealing || !sealing.key || !sealing.id) return;
    const bytes = (text) => Uint8Array.from(atob(text.replace(/-/g, '+').replace(/_/g, '/').padEnd(Math.ceil(text.length / 4) * 4, '=')),
      (character) => character.charCodeAt(0));
    const memo = (window.__rustyTeamsUnsealed = window.__rustyTeamsUnsealed || new Map());
    const base = await crypto.subtle.importKey('raw', bytes(sealing.key), 'HKDF', false, ['deriveKey']);
    for (const [key, entry] of sealed) {
      if (entry.id !== sealing.id) continue;
      if (!memo.has(entry.nonce)) {
        // MSAL binds an entry to the client id in its key, or to an empty context when the key has none.
        const contexts = [...(key.match(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi) || []), ''];
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
      if (memo.has(entry.nonce)) {
        take(key, memo.get(entry.nonce));
        opened++;
      }
    }
  };
  if (sealed.length) await unseal();
  const ownAccount = (entry) => !activeHome || entry.homeAccountId === activeHome;
  const homeTenant = (entry) => (entry.homeAccountId || '').split('.')[1];
  const cache = (window.__rustyTeamsTokens = window.__rustyTeamsTokens || {});
  const acquire = async (resource, scope) => {
    const base = resource.replace(/\/+$/, '');
    const wanted = (base + '/' + scope).toLowerCase();
    const short = scope.toLowerCase();
    const underResource = (name) => normalize(name).startsWith(base.toLowerCase() + '/');
    const grants = (scopes) => scopes.some(s => (scope === '.default' ? underResource(s) : covers(normalize(s), wanted))
      || (base === 'https://graph.microsoft.com' && covers(s.toLowerCase(), short)));
    const cacheKey = nestedApp ? base + '|' + nestedApp.clientId : base;
    let token = null, expiresOn = 0;
    for (const entry of forceRefresh ? [] : entries) {
      if (entry.credentialType !== 'AccessToken' || (entry.tokenType && entry.tokenType !== 'Bearer')) continue;
      if (nestedApp && entry.clientId !== nestedApp.clientId) continue;
      if (!ownAccount(entry) || (entry.realm && entry.realm !== homeTenant(entry))) continue;
      const expires = Number(entry.expiresOn) * 1000;
      if (!grants((entry.target || '').split(' ')) || !fresh(expires) || expires <= expiresOn) continue;
      token = entry.secret; expiresOn = expires;
    }
    const cached = cache[cacheKey];
    if (!token && !forceRefresh && cached && fresh(cached.expires)) {
      if (grants(cached.scopes)) token = cached.token;
      else return {token: null, refreshError: `the app is not granted ${scope}`};
    }
    const refreshTokens = entries
      .filter(entry => entry.credentialType === 'RefreshToken' && ownAccount(entry))
      .filter(entry => !entry.expiresOn || Number(entry.expiresOn) * 1000 > Date.now())
      .sort((first, second) => Number(second.lastUpdatedAt || 0) - Number(first.lastUpdatedAt || 0));
    let refreshError = null;
    for (const refreshToken of token ? [] : refreshTokens) {
      const form = new URLSearchParams({
        client_id: refreshToken.clientId, grant_type: 'refresh_token', refresh_token: refreshToken.secret,
        scope: base + '/.default openid profile offline_access',
      });
      if (nestedApp) {
        form.set('client_id', nestedApp.clientId);
        form.set('brk_client_id', refreshToken.clientId);
        form.set('redirect_uri', nestedApp.redirectUri);
        form.set('brk_redirect_uri', nestedApp.brokerRedirectUri);
      }
      try {
        const response = await fetch(`https://login.microsoftonline.com/${homeTenant(refreshToken) || 'organizations'}/oauth2/v2.0/token`,
          {method: 'POST', body: form});
        const answer = await response.json();
        if (!response.ok) { refreshError = answer.error || `HTTP ${response.status}`; continue; }
        const scopes = (answer.scope || '').split(' ');
        cache[cacheKey] = {token: answer.access_token, scopes, expires: Date.now() + answer.expires_in * 1000};
        if (grants(scopes)) { token = answer.access_token; break; }
        refreshError = `the app is not granted ${scope}`;
      } catch (error) {
        refreshError = String(error);
      }
    }
    return {token, refreshError};
  };
  const {token, refreshError} = await acquire(resource, scope);
  const page = {visibility: document.visibilityState, plain, sealed: sealed.length, opened};
  if (!token) return {noToken: true, refreshError, page};
  const exchange = (request) => new Promise((resolve) => {
    const separator = '\u001e';
    const {handshake, invocations, doneTypes, timeoutMs} = request.signalr;
    const received = [];
    let socket = null, settled = false, shookHands = false;
    const finish = (status) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      try { socket && socket.close(); } catch (error) {}
      resolve({status, body: received});
    };
    const timer = setTimeout(() => finish(504), timeoutMs);
    try {
      socket = new WebSocket(request.url + (request.url.includes('?') ? '&' : '?') + 'access_token=' + encodeURIComponent(token));
    } catch (error) {
      clearTimeout(timer);
      resolve({status: 0, body: String(error)});
      return;
    }
    socket.onopen = () => socket.send(handshake + separator);
    socket.onmessage = (event) => {
      for (const part of String(event.data).split(separator)) {
        let message;
        try { message = part ? JSON.parse(part) : null; } catch (error) { message = null; }
        if (!message) continue;
        if (!shookHands) {
          shookHands = true;
          if (message.error) { received.push(message); finish(502); return; }
          invocations.forEach((invocation) => socket.send(invocation + separator));
          continue;
        }
        if (message.type === 6) { socket.send('{"type":6}' + separator); continue; }
        received.push(message);
        if (doneTypes.includes(message.type)) finish(200);
      }
    };
    socket.onerror = () => finish(0);
    socket.onclose = () => finish(received.length ? 200 : 0);
  });
  const converse = (request) => new Promise((resolve) => {
    const {stages, placeholder, doneMarker, timeoutMs, keepaliveMs} = request.augloop;
    const received = [];
    let socket = null, settled = false, stage = 0, keepalive = null;
    const finish = (status) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      clearInterval(keepalive);
      try { socket && socket.close(); } catch (error) {}
      resolve({status, body: received});
    };
    const timer = setTimeout(() => finish(504), timeoutMs);
    const sendStage = () => {
      for (const message of stages[stage].send) socket.send(message.split(placeholder).join(token));
    };
    try {
      socket = new WebSocket(request.url);
    } catch (error) {
      clearTimeout(timer);
      resolve({status: 0, body: String(error)});
      return;
    }
    socket.onopen = () => {
      socket.send('~');
      keepalive = setInterval(() => socket.send('~'), keepaliveMs);
      sendStage();
    };
    socket.onmessage = (event) => {
      const text = String(event.data);
      if (text === '~') return;
      let message;
      try { message = JSON.parse(text); } catch (error) { return; }
      const messageId = String(message.messageId || '');
      if (messageId.startsWith('s')) socket.send(JSON.stringify({H_: {T_: 'AugLoop_Session_Protocol_Response', B_: []}, messageId}));
      received.push(message);
      if (message.error) { finish(502); return; }
      if (text.includes(doneMarker)) { finish(200); return; }
      if (stage + 1 < stages.length && messageId === stages[stage].awaitMessageId) {
        stage++;
        sendStage();
      }
    };
    socket.onerror = () => finish(0);
    socket.onclose = () => finish(received.length ? 200 : 0);
  });
  const run = async (request) => {
    if (request.signalr) return exchange(request);
    if (request.augloop) return converse(request);
    try {
      let requestBody = request.body;
      for (const extra of request.bodyTokens || []) {
        const granted = await acquire(extra.resource, extra.scope);
        if (!granted.token) return {status: 0, body: `no token for ${extra.resource}`};
        requestBody = requestBody.split(extra.placeholder).join(granted.token);
      }
      const response = await fetch(request.url, {
        method: request.method,
        headers: request.anonymous ? request.headers : {Authorization: 'Bearer ' + token, ...request.headers},
        body: request.bodyBase64 ? Uint8Array.from(atob(request.bodyBase64), (character) => character.charCodeAt(0)) : requestBody,
      });
      if (request.binary && response.ok) {
        const bytes = new Uint8Array(await response.arrayBuffer());
        if (bytes.length > maxBytes) return {status: 413, body: `download is ${bytes.length} bytes, over the ${maxBytes} limit`};
        let binary = '';
        for (let offset = 0; offset < bytes.length; offset += 0x8000) {
          binary += String.fromCharCode.apply(null, bytes.subarray(offset, offset + 0x8000));
        }
        return {status: response.status, body: {base64: btoa(binary), contentType: response.headers.get('Content-Type')}};
      }
      const text = await response.text();
      let body = text;
      try { body = text ? JSON.parse(text) : null; } catch (error) {}
      const location = response.headers.get('Location');
      if (body === null && location) body = {location};
      return {status: response.status, retryAfter: response.headers.get('Retry-After'), body};
    } catch (error) {
      return {status: 0, body: String(error)};
    }
  };
  const results = new Array(requests.length);
  let next = 0;
  const worker = async () => { while (next < requests.length) { const index = next++; results[index] = await run(requests[index]); } };
  await Promise.all(Array.from({length: Math.min(concurrency, requests.length)}, worker));
  return {results};
})
