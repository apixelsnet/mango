// Mango's service worker. It lets chapters kept on this device be read with no connection.
//
// - The page itself and its scripts: from the network, and from the last copy when Mango
//   cannot be reached.
// - A kept chapter's pages: from this device first, since they never change.
// - What the reader and the title page ask for, and who is signed in with what settings:
//   from the network, and from this device's copy when Mango cannot be reached.
// - Everything else goes straight to Mango, untouched.
//
// lib/offline.ts fills the `mango-offline-*` caches; nothing here adds to them.

const SHELL = 'mango-shell';
const KEPT = 'mango-offline-';
/** The most files the page's own copy keeps; each release brings new script names. */
const SHELL_MOST = 60;

self.addEventListener('install', () => self.skipWaiting());
self.addEventListener('activate', (event) => event.waitUntil(self.clients.claim()));

/** A kept copy of this request, in any account's cache. */
async function kept(request) {
  for (const name of await caches.keys()) {
    if (!name.startsWith(KEPT)) continue;
    const found = await (await caches.open(name)).match(request, { ignoreSearch: true });
    if (found) return found;
  }
  return undefined;
}

async function shell(request) {
  try {
    const answer = await fetch(request);
    if (answer.ok && answer.type === 'basic') {
      const cache = await caches.open(SHELL);
      await cache.put(request, answer.clone());
      const keys = await cache.keys();
      for (const old of keys.slice(0, Math.max(0, keys.length - SHELL_MOST))) await cache.delete(old);
    }
    return answer;
  } catch (error) {
    const cache = await caches.open(SHELL);
    // Every address that is not a file is the app's page.
    const found = (await cache.match(request, { ignoreSearch: true })) ?? (request.mode === 'navigate' ? await cache.match(new URL('./', self.registration.scope)) : undefined);
    if (found) return found;
    throw error;
  }
}

async function networkFirst(request) {
  try {
    return await fetch(request);
  } catch (error) {
    const found = await kept(request);
    if (found) return found;
    throw error;
  }
}

async function keptFirst(request) {
  return (await kept(request)) ?? fetch(request);
}

self.addEventListener('fetch', (event) => {
  const request = event.request;
  if (request.method !== 'GET') return;
  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;
  const path = url.pathname.slice(new URL(self.registration.scope).pathname.length);
  if (/^api\/chapters\/\d+\/pages\/\d+$/.test(path)) event.respondWith(keptFirst(request));
  // Who is signed in and their settings come along, or the app could not open a kept chapter at all.
  else if (/^api\/(chapters\/\d+(\/pages)?|manga\/\d+(\/cover)?|auth|settings)$/.test(path)) event.respondWith(networkFirst(request));
  // Only Mango's own page and files: other pages under the same name, an account's say, are none of its business.
  else if (path === '' || path === 'index.html' || path === 'manifest.webmanifest' || /^(assets|icons)\//.test(path)) event.respondWith(shell(request));
});
