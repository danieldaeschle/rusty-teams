let response;
try {
  response = await fetch('https://teams.cloud.microsoft/api/authsvc/v1.0/authz', {
    method: 'POST',
    headers: {Authorization: 'Bearer ' + token},
  });
} catch (error) {
  return {error: 'authz_unreachable'};
}
if (!response.ok) return {error: 'authz_http_' + response.status};
let url = null;
try {
  const answer = await response.json();
  url = answer.regionGtms.calling_trouterUrl;
} catch (error) {
  return {error: 'authz_shape'};
}
try {
  return {host: new URL(url).host};
} catch (error) {
  return {error: 'authz_no_trouter_url'};
}
