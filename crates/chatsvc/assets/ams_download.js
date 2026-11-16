try {
  const response = await fetch(args.url, {
    headers: {Authorization: 'skype_token ' + skypeCache.token, 'x-ms-client-version': '1415/1.0.0.2018090100'},
  });
  if (!response.ok) return {status: response.status, body: (await response.text()).slice(0, 300)};
  const blob = await response.blob();
  const base64 = await new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result).split(',')[1] || '');
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(blob);
  });
  return {status: response.status, body: {base64, contentType: blob.type}};
} catch (error) {
  return {status: 0, body: String(error)};
}
