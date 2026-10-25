// Chapters kept on this device, to read with no connection. What the reader asks for to
// open one, and every page of it, go into a cache of the browser's own, one per account;
// public/sw.js answers from it when Mango cannot be reached. Signing out empties it.
//
// The browser only offers this cache over HTTPS, or at an address of this machine.

import { session } from './session.svelte';

const PREFIX = 'mango-offline-';

/** Whether this browser can keep chapters: it has the cache, and the page is served securely. */
export const canKeep = typeof caches !== 'undefined' && 'serviceWorker' in navigator && isSecureContext;

const cacheName = () => PREFIX + encodeURIComponent(session.name ?? 'owner');

/** Starts the service worker. Without it nothing kept could be read offline. */
export function startWorker() {
  if (canKeep) navigator.serviceWorker.register('sw.js').catch(() => {});
}

/** Puts a title's page and chapters on this device. `done` hears of each page saved. Answers how many chapters were kept. */
export async function keep(title: number, chapters: number[], done?: (pages: number) => void): Promise<number> {
  const cache = await caches.open(cacheName());
  const put = async (path: string) => {
    const answer = await fetch(path);
    if (!answer.ok) throw new Error(`Mango answered ${answer.status} for ${path}`);
    await cache.put(path, answer);
  };
  // The reader's own code is fetched once now, so the worker has a copy of it too.
  await import('../routes/Reader.svelte').catch(() => {});
  // What the app asks for before anything else: whether anyone is signed in, and their settings.
  await put('api/auth');
  await put('api/settings');
  await put(`api/manga/${title}`);
  await put(`api/manga/${title}/cover`).catch(() => {});
  let saved = 0;
  let pages = 0;
  for (const chapter of chapters) {
    await put(`api/chapters/${chapter}`);
    const counted = await fetch(`api/chapters/${chapter}/pages`);
    if (!counted.ok) continue;
    const { count } = (await counted.clone().json()) as { count: number };
    await cache.put(`api/chapters/${chapter}/pages`, counted);
    for (let page = 0; page < count; page++) {
      await put(`api/chapters/${chapter}/pages/${page}`);
      done?.(++pages);
    }
    saved++;
  }
  return saved;
}

/** The chapters kept on this device. */
export async function keptChapters(): Promise<Set<number>> {
  if (!canKeep) return new Set();
  const cache = await caches.open(cacheName());
  const ids = (await cache.keys()).map((request) => /\/api\/chapters\/(\d+)\/pages$/.exec(new URL(request.url).pathname)?.[1]).filter(Boolean);
  return new Set(ids.map(Number));
}

/** Lets go of chapters kept on this device. */
export async function forget(chapters: number[]) {
  const cache = await caches.open(cacheName());
  const gone = new Set(chapters.map(String));
  for (const request of await cache.keys()) {
    const id = /\/api\/chapters\/(\d+)(\/|$)/.exec(new URL(request.url).pathname)?.[1];
    if (id && gone.has(id)) await cache.delete(request);
  }
}

/** Empties every account's kept chapters on this device: for signing out, so the next one sees none of them. */
export async function forgetAll() {
  if (!canKeep) return;
  for (const name of await caches.keys()) if (name.startsWith(PREFIX)) await caches.delete(name);
}
