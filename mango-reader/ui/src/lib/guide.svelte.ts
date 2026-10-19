// The tour for a new account: a few cards on what Mango does and where things are.
// Whether it was seen is a setting, so it is shown once per account, not per browser.

import { setPref } from './prefs.svelte';

// The step is kept here, not in the card, as each step opens the screen it is about.
export const tour = $state({ open: false, step: 0 });

export function startTour() {
  tour.step = 0;
  tour.open = true;
}

/** Closes it, skipped or finished: either way it is not shown again by itself. */
export function endTour() {
  tour.open = false;
  setPref('tour.done', true).catch(() => {});
}

/** Shows the tour and the reader's tap zones again, as for a new account. */
export function againTour() {
  setPref('tour.zones', null).catch(() => {});
  startTour();
}
