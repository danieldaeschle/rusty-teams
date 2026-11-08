const skypeCache = (window.__rustyTeamsSkype = window.__rustyTeamsSkype || {});
if (!skypeCache.token || skypeCache.expires <= Date.now() + 60000) {
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
    skypeCache.token = answer.tokens.skypeToken;
    skypeCache.expires = Date.now() + Number(answer.tokens.expiresIn || 0) * 1000;
    skypeCache.chatService = answer.regionGtms.chatService;
  } catch (error) {
    return {status: 0, body: 'authz_shape'};
  }
}
