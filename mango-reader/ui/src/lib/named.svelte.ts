// The name of what a screen shows, for the browser's tab and history and for a screen
// reader. A screen that knows one (a title, a chapter, a source) sets it; the others are
// named by their place in the menu (App.svelte).

import { route } from './router.svelte';

/** The name and the address it was given for: a name left from another screen is not used. */
export const named = $state({ path: '', text: '' });

/** Names the screen open now. Nothing happens until there is a name to give. */
export function name(text: string | null | undefined) {
  if (!text) return;
  named.path = route.path;
  named.text = text;
}
