// The plan a hosted Mango's reader is on: the room their files have, and what it leaves out.
// Asked only where whoever runs this Mango keeps accounts (`session.account`). The service in
// front of such a Mango answers it; Mango itself knows no plans, and answers that it has none.

import { gate, get } from './api';
import { session } from './session.svelte';

export type Plan = {
  name: string;
  /** What the reader's downloads and imports take, and what the plan gives them, in bytes. */
  storage: { used: number; of: number } | null;
  /** The storage is used up: no new downloads until some go. */
  full: boolean;
  opds: boolean;
  trackers: boolean;
  autoDownload: boolean;
  /** Setting a proxy the sources reach sites through. */
  proxy: boolean;
  /** The fewest hours between automatic library checks. */
  updatesEvery: number | null;
};

export const plan = $state({
  /** Null where no plan holds, and everything is open. */
  is: null as Plan | null,
  /** The card that says the storage is full is up. */
  asking: false,
});

// Asked at most this often when screens change: a few deletions can lift "full" soon.
const AGAIN = 15_000;
// Where room is made: the full card stays down there.
const FREEING = ['settings', 'downloads'];
let asked = 0;

export async function loadPlan(now = false) {
  if (!session.signedIn || !session.account) return;
  if (!now && Date.now() - asked < AGAIN) return;
  asked = Date.now();
  try {
    plan.is = await get<Plan>('/plan');
  } catch (e) {
    // Not found: no plan holds. Anything else is asked again later.
    if ((e as { status?: number }).status === 404) plan.is = null;
    return;
  }
}

/** Puts the storage-full card up. */
export function ask() {
  plan.asking = true;
}

/** Whether the full card comes up on this screen: on every one while full, but where room is made. */
export const asksOn = (area: string) => plan.is?.full === true && !FREEING.includes(area);

/** Whether the plan leaves this out. Never where no plan holds. */
export const lacks = (what: 'opds' | 'trackers' | 'autoDownload' | 'proxy') => plan.is !== null && !plan.is[what];

/** Whether an automatic check this many hours apart is more often than the plan allows. */
export const tooOften = (hours: number) => hours > 0 && plan.is?.updatesEvery != null && hours < plan.is.updatesEvery;

export const sized = (bytes: number) =>
  bytes >= 1 << 30 ? `${(bytes / (1 << 30)).toFixed(1).replace(/\.0$/, '')} GB` : `${Math.max(bytes ? 1 : 0, Math.round(bytes / (1 << 20)))} MB`;

gate.plan = (code) => {
  if (!session.account) return;
  if (code === 'storage_full') {
    if (plan.is) plan.is.full = true;
    ask();
  }
  loadPlan(true);
};
