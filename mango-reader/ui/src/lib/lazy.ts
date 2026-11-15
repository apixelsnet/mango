// Screens fetched when first opened (App.svelte). When the fetch fails the screen would stay
// blank. Most often Mango was updated while the page was open, and the old file names are
// gone: the page reloads once to get the new ones. If it fails again, the screen says so.

import Outdated from './Outdated.svelte';

const RELOADED = 'mango-reloaded-for-screen';
/** One reload per this long, so a screen that cannot load never makes the page reload over and over. */
const ONCE_PER_MS = 60_000;

function reloadOnce(): boolean {
  try {
    const last = Number(sessionStorage.getItem(RELOADED) ?? 0);
    if (Date.now() - last < ONCE_PER_MS) return false;
    sessionStorage.setItem(RELOADED, String(Date.now()));
  } catch {
    return false;
  }
  location.reload();
  return true;
}

/** A screen's loader that, failing, reloads the page once, then stands in with a message. */
export function lazy<T>(loader: () => Promise<T>): () => Promise<T> {
  return () =>
    loader().catch(() => {
      // While the page reloads, nothing is shown rather than a message that flashes past.
      if (reloadOnce()) return new Promise<T>(() => {});
      return { default: Outdated } as unknown as T;
    });
}
