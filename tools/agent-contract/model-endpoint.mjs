export function modelEndpoint(value) {
  let url;
  try { url = new URL(value); } catch { throw new Error('INVALID_MODEL_ENDPOINT'); }
  const loopback = ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname);
  if (url.username || url.password || url.search || url.hash ||
      (url.protocol !== 'https:' && !(url.protocol === 'http:' && loopback))) {
    throw new Error('INVALID_MODEL_ENDPOINT');
  }
  return url.href.replace(/\/$/, '');
}
