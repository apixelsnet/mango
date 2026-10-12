// Routes live after the # in the address, so the server only ever serves one page
// and Mango works under any sub-path.

import { tick } from 'svelte';

export const route = $state({
  path: '/',
  query: new URLSearchParams(),
  /** This screen was come back to, with the back or forward of the browser or the device. */
  returned: false,
});

// Each screen opened gets a number, one more than the screen it was opened from,
// kept with its place in the browser's history. The number tells a screen come back
// to from a new one, and holds how far down each was left.
let at = 0;
let replacing = false;
const trail = new Map<number, { path: string; top: number }>();

// The browser would put the scroll back before the screen has anything to scroll.
history.scrollRestoration = 'manual';

function read() {
  const raw = location.hash.replace(/^#/, '') || '/';
  const cut = raw.indexOf('?');
  route.path = cut < 0 ? raw : raw.slice(0, cut);
  route.query = new URLSearchParams(cut < 0 ? '' : raw.slice(cut + 1));
}

let turn = 0;

/** Scrolls to where a screen was left, once it is long enough again. A touch or a key ends the wait. */
function settle(top: number) {
  const mine = ++turn;
  const until = performance.now() + 3000;
  const step = () => {
    if (mine !== turn) return;
    scrollTo(0, top);
    if (Math.abs(scrollY - top) > 1 && performance.now() < until) requestAnimationFrame(step);
  };
  tick().then(step);
}

function arrive() {
  const known = history.state?.mango;
  route.returned = typeof known === 'number';
  if (route.returned) at = known;
  else {
    if (!replacing) at += 1;
    history.replaceState({ mango: at }, '');
  }
  replacing = false;
  read();
  const top = (route.returned && trail.get(at)?.top) || 0;
  trail.set(at, { path: route.path, top });
  settle(top);
}

addEventListener('hashchange', arrive);
addEventListener(
  'scroll',
  () => {
    const here = trail.get(at);
    if (here) here.top = scrollY;
  },
  { passive: true },
);
for (const kind of ['touchstart', 'wheel', 'keydown']) addEventListener(kind, () => turn++, { passive: true });
arrive();

/** Opens a route. With `replace` the current one is not kept in the history. */
export function go(to: string, replace = false) {
  if (location.hash === '#' + to) return;
  if (replace) {
    replacing = true;
    location.replace('#' + to);
  } else location.hash = to;
}

/** Leaves for the screen before this one. Opened straight from outside there is none of Mango's, and `fallback` takes its place. */
export function back(fallback = '/') {
  if (trail.has(at - 1)) history.back();
  else go(fallback, true);
}

/** Opens the screen this one came out of: by going back when that is where back leads, so the two do not pile up in the history. */
export function up(to: string) {
  if (trail.get(at - 1)?.path === to) history.back();
  else go(to, true);
}
