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
//
// - Word of new chapters is shown as a notification; see the end.

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

// Word of new chapters, sent by Mango through the browser's push service (lib/push.ts
// turns it on and off). What comes is { title, body, url }.
//
// - Mango open and in front, on a computer or a phone: no notification; the page shows the
//   word itself (App.svelte) and counts the new chapters beside Updates.
// - Mango behind other windows, in a background tab, closed, or the phone locked: a
//   notification of the system's own. A device that is off gets it when it is back, within a day.
// - Safari, and every browser on an iPhone or iPad, cancel a subscription whose pushes show
//   nothing, so there the notification always comes, and the open page only counts.
// An app on an iPhone's home screen may not say Safari, but it says AppleWebKit, as Chrome does.
const STRICT = /AppleWebKit/.test(navigator.userAgent) && !/Chrome|Chromium|Edg|OPR|Android/.test(navigator.userAgent);

/** Whether a window is Mango's page itself, not a file or an answer of its API opened in a tab. */
function isApp(client) {
  const path = new URL(client.url).pathname;
  const root = new URL(self.registration.scope).pathname;
  return path === root || path === root + 'index.html';
}

self.addEventListener('push', (event) => {
  let note = {};
  try {
    note = event.data ? event.data.json() : {};
  } catch {
    note = { body: event.data ? event.data.text() : '' };
  }
  event.waitUntil(
    (async () => {
      const windows = (await self.clients.matchAll({ type: 'window', includeUncontrolled: true })).filter(isApp);
      const seen = windows.find((client) => client.focused && client.visibilityState === 'visible');
      for (const client of windows) client.postMessage({ type: 'mango-push', note, shown: client === seen && !STRICT });
      if (seen && !STRICT) return;
      await self.registration.showNotification(note.title || 'Mango', {
        body: note.body || '',
        icon: 'icons/icon-192.png',
        badge: 'icons/icon-192.png',
        // A newer word of chapters takes the place of the one before.
        tag: 'mango-chapters',
        data: { url: note.url || '' },
      });
    })(),
  );
});

// Tapping it brings Mango forward, at the place the note leads to.
self.addEventListener('notificationclick', (event) => {
  event.notification.close();
  const target = new URL(event.notification.data?.url || '', self.registration.scope).href;
  event.waitUntil(
    (async () => {
      for (const client of await self.clients.matchAll({ type: 'window', includeUncontrolled: true })) {
        if (!isApp(client)) continue;
        await client.focus();
        if ('navigate' in client) await client.navigate(target).catch(() => {});
        return;
      }
      await self.clients.openWindow(target);
    })(),
  );
});
