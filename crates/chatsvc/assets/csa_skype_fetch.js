const headers = {...args.headers, Authorization: 'Bearer ' + token, 'x-skypetoken': skypeCache.token};
if (skypeCache.chatService) headers['x-csendpoint'] = skypeCache.chatService;
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
