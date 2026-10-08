(async ({requests, resource, scope, marginMs, concurrency, maxBytes, forceRefresh}) => {
  const fresh = (expires) => expires > Date.now() + marginMs;
  const normalize = (value) => value.toLowerCase().replace(/\/\/+(?=[^/]*$)/, '/');
  const covers = (value, name) => value === name || value === name + '.all';
  const entries = [];
  let activeHome = null;
  for (let index = 0; index < localStorage.length; index++) {
    const key = localStorage.key(index);
    let entry;
    try { entry = JSON.parse(localStorage.getItem(key)); } catch (error) { continue; }
    if (!entry || typeof entry !== 'object') continue;
    if (key.endsWith('active-account-filters') && entry.homeAccountId) activeHome = entry.homeAccountId;
    if (typeof entry.secret === 'string') entries.push(entry);
  }
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
    let token = null, expiresOn = 0;
    for (const entry of forceRefresh ? [] : entries) {
      if (entry.credentialType !== 'AccessToken' || (entry.tokenType && entry.tokenType !== 'Bearer')) continue;
      if (!ownAccount(entry) || (entry.realm && entry.realm !== homeTenant(entry))) continue;
      const expires = Number(entry.expiresOn) * 1000;
      if (!grants((entry.target || '').split(' ')) || !fresh(expires) || expires <= expiresOn) continue;
      token = entry.secret; expiresOn = expires;
    }
    const cached = cache[base];
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
      try {
        const response = await fetch(`https://login.microsoftonline.com/${homeTenant(refreshToken) || 'organizations'}/oauth2/v2.0/token`,
          {method: 'POST', body: form});
        const answer = await response.json();
        if (!response.ok) { refreshError = answer.error || `HTTP ${response.status}`; continue; }
        const scopes = (answer.scope || '').split(' ');
        cache[base] = {token: answer.access_token, scopes, expires: Date.now() + answer.expires_in * 1000};
        if (grants(scopes)) { token = answer.access_token; break; }
        refreshError = `the app is not granted ${scope}`;
      } catch (error) {
        refreshError = String(error);
      }
    }
    return {token, refreshError};
  };
  const {token, refreshError} = await acquire(resource, scope);
  if (!token) return {noToken: true, refreshError};
  const run = async (request) => {
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
