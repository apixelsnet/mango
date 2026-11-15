// Places in chapters the reader could not send: read with no connection, from chapters kept
// on this device (lib/offline.ts). They wait in this browser, one list per account, and go
// once Mango answers again. Signing out drops them, with the kept chapters.

import { ApiError, get, send } from './api';
import { session } from './session.svelte';

type Place = { page: number; read: boolean };

const PREFIX = 'mango-unsent-';
const key = () => PREFIX + encodeURIComponent(session.name ?? 'owner');

function load(): Record<string, Place> {
  try {
    return JSON.parse(localStorage.getItem(key()) || '{}') || {};
  } catch {
    return {};
  }
}

function store(all: Record<string, Place>) {
  try {
    if (Object.keys(all).length) localStorage.setItem(key(), JSON.stringify(all));
    else localStorage.removeItem(key());
  } catch {
    // No room or no storage: the place is lost, as it was before this list.
  }
}

/** Keeps a place that could not be sent. A chapter finished stays finished. */
export function hold(chapter: number, page: number, read: boolean) {
  const all = load();
  const before = all[chapter];
  all[chapter] = before?.read && !read ? before : { page, read };
  store(all);
}

/** Drops what waits for a chapter whose newer place did reach Mango. */
export function sent(chapter: number) {
  const all = load();
  if (!(chapter in all)) return;
  delete all[chapter];
  store(all);
}

/** Empties every account's list on this device: for signing out. */
export function dropAll() {
  try {
    for (const name of Object.keys(localStorage)) if (name.startsWith(PREFIX)) localStorage.removeItem(name);
  } catch {
    // Nothing to drop.
  }
}

let sending = false;

/**
 * Sends what waits, and stops at the first call Mango does not answer. A place is never
 * taken back: a chapter Mango has as read, or further on than the place waiting, keeps
 * what Mango has, since another device may have read on since.
 */
export async function sendHeld() {
  if (sending || !(session.signedIn || !session.login)) return;
  sending = true;
  try {
    for (const [id, place] of Object.entries(load())) {
      try {
        const { chapter } = await get<{ chapter: { read: boolean; lastPageRead: number } }>(`/chapters/${id}`);
        const behind = chapter.read || (!place.read && chapter.lastPageRead >= place.page);
        if (!behind) await send('PUT', `/chapters/${id}/progress`, place.read ? { page: place.page, read: true } : { page: place.page });
        sent(Number(id));
      } catch (e) {
        // The chapter is gone: nothing to send it to. Anything else: try again later.
        if (e instanceof ApiError && e.status === 404) sent(Number(id));
        else return;
      }
    }
  } finally {
    sending = false;
  }
}
