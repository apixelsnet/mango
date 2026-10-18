// The tour for a new account: a few cards on what Mango does and where things are.
// Whether it was seen is a setting, so it is shown once per account, not per browser.

import { setPref } from './prefs.svelte';

export const tour = $state({ open: false });

export function startTour() {
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
