let authz;
try {
  authz = await fetch('https://teams.cloud.microsoft/api/authsvc/v1.0/authz', {
    method: 'POST',
    headers: {Authorization: 'Bearer ' + token},
  });
} catch (error) {
  return {error: 'authz_unreachable'};
}
if (!authz.ok) return {error: 'authz_http_' + authz.status};
let skypeToken = null;
try {
  skypeToken = (await authz.json()).tokens.skypeToken;
} catch (error) {
  return {error: 'authz_shape'};
}
if (!skypeToken) return {error: 'authz_no_skype_token'};
const recorder = await acquire(args.recorderResource, args.recorderScope);
if (!recorder.token) return {error: 'no_recorder_token'};
let response;
try {
  response = await fetch(args.url, {
    method: 'POST',
    headers: {
      Authorization: 'Bearer ' + recorder.token,
      'X-Skypetoken': skypeToken,
      requestid: args.requestId,
      'x-microsoft-skype-chain-id': args.chainId,
      'content-type': 'application/json',
    },
    body: JSON.stringify({...args.body, participantSkypeToken: skypeToken}),
  });
} catch (error) {
  return {error: 'command_unreachable'};
}
return {status: response.status};
