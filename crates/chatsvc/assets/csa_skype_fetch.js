const cache = (window.__rustyTeamsSkype = window.__rustyTeamsSkype || {});
if (!cache.token || cache.expires <= Date.now() + 60000) {
  const spaces = await acquire('https://api.spaces.skype.com', 'authorization.readwrite');
  if (!spaces.token) return {status: 0, body: 'no token for authsvc'};
  let response;
  try {
    response = await fetch('https://teams.cloud.microsoft/api/authsvc/v1.0/authz', {
      method: 'POST',
      headers: {Authorization: 'Bearer ' + spaces.token},
    });
  } catch (error) {
    return {status: 0, body: 'authz_unreachable'};
  }
  if (!response.ok) return {status: 0, body: 'authz_http_' + response.status};
  try {
    const answer = await response.json();
    cache.token = answer.tokens.skypeToken;
    cache.expires = Date.now() + Number(answer.tokens.expiresIn || 0) * 1000;
    cache.chatService = answer.regionGtms.chatService;
  } catch (error) {
    return {status: 0, body: 'authz_shape'};
  }
}
const headers = {...args.headers, Authorization: 'Bearer ' + token, 'x-skypetoken': cache.token};
if (cache.chatService) headers['x-csendpoint'] = cache.chatService;
try {
  const response = await fetch(args.url, {
    method: args.method,
    headers,
    body: args.body === null ? undefined : JSON.stringify(args.body),
  });
  const text = await response.text();
  let body = text;
  try { body = text ? JSON.parse(text) : null; } catch (error) {}
  return {status: response.status, retryAfter: response.headers.get('Retry-After'), body};
} catch (error) {
  return {status: 0, body: String(error)};
}
