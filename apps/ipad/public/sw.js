// Bump this when a client-side recovery fix ships.  It makes a previously
// installed iPad PWA discard the old shell instead of reviving a stale bundle.
const CACHE = 'vision-pro-shell-v12';
const SHELL = ['/', '/manifest.webmanifest', '/icons/icon.svg'];
// With only a USB cable (no Wi-Fi) the app's `.local` name may never resolve.
// Serve the installed shell after this delay instead of leaving the iPad on a
// blank screen; the network answer still refreshes the cache when it arrives.
const NETWORK_TIMEOUT_MS = 2500;

self.addEventListener('install', (event) => {
  event.waitUntil(caches.open(CACHE).then((cache) => cache.addAll(SHELL)).then(() => self.skipWaiting()));
});

self.addEventListener('activate', (event) => {
  event.waitUntil(caches.keys().then((keys) => Promise.all(keys.filter((key) => key !== CACHE).map((key) => caches.delete(key)))).then(() => self.clients.claim()));
});

function delay(ms) {
  return new Promise((resolve) => setTimeout(() => resolve(null), ms));
}

self.addEventListener('fetch', (event) => {
  const request = event.request;
  if (request.method !== 'GET') return;
  const url = new URL(request.url);
  if (url.origin !== location.origin || url.pathname.startsWith('/api/') || url.pathname.startsWith('/ws')) return;
  event.respondWith((async () => {
    const cache = await caches.open(CACHE);
    const network = fetch(request).then((response) => {
      if (response.ok) cache.put(request, response.clone());
      return response;
    });
    const answer = await Promise.race([network.catch(() => null), delay(NETWORK_TIMEOUT_MS)]);
    if (answer) return answer;
    const cached = (await cache.match(request)) ?? (request.mode === 'navigate' ? await cache.match('/') : undefined);
    if (cached) {
      event.waitUntil(network.catch(() => undefined));
      return cached;
    }
    // Nothing cached yet: the network is the only possible answer.
    return network;
  })());
});
