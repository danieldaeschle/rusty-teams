const objects = args.base + 'v1/objects/';
const auth = {Authorization: 'skype_token ' + skypeCache.token, 'x-ms-client-version': '1415/1.0.0.2018090100'};
try {
  const created = await fetch(objects, {
    method: 'POST',
    headers: {...auth, 'Content-Type': 'application/json', Accept: 'application/json'},
    body: JSON.stringify({
      type: 'pish/image',
      permissions: {[args.conversationId]: ['read']},
      sharingMode: 'Inline',
      filename: args.filename,
    }),
  });
  if (!created.ok) return {status: created.status, body: 'create_' + (await created.text()).slice(0, 300)};
  const id = (await created.json()).id;
  const bytes = Uint8Array.from(atob(args.base64), (character) => character.charCodeAt(0));
  const stored = await fetch(objects + encodeURIComponent(id) + '/content/imgpsh', {
    method: 'PUT',
    headers: {...auth, 'Content-Type': 'application/octet-stream', 'Content-Range': `bytes 0-${bytes.length - 1}/${bytes.length}`},
    body: bytes,
  });
  if (!stored.ok) return {status: stored.status, body: 'put_' + (await stored.text()).slice(0, 300)};
  return {status: stored.status, body: {id}};
} catch (error) {
  return {status: 0, body: String(error)};
}
