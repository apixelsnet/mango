# 15 — Tour

A short tour for a new account, and a picture of the reader's tap zones. All of it is in the web UI.

## What it does

- **Tour** (`lib/Tour.svelte`, `lib/guide.svelte.ts`). A few cards in a modal dialog: what Mango is, finding titles through extensions, the library and Updates, reading, downloads and backups, where Settings are. Back, Next, Skip, the close button and Escape all work; Skip and finishing count the same.
- It starts by itself on the Library screen when the library is empty and the account's setting `tour.done` is unset. An account that already has titles is not shown it unasked, so an upgrade does not put it in front of people who know Mango.
- **Tap zones** (`lib/Zones.svelte`). The first chapter an account opens draws over the page where a tap goes back, forward or opens the menu, computed from the reader's own `zone()` rule on a 12×12 grid, so it is right for every layout, direction and swap. Tap or Escape closes it and sets `tour.zones`. Not shown while the zones are off (manhwa and manhua by default). The reader's top bar has a button to show it again.
- **Again**: Settings → About → Show the tour. It clears `tour.zones` and opens the tour.
- Both flags are ordinary settings (`/api/settings`), so they belong to the account and follow it to every device.
- The empty screens (Library, Browse, Updates, Downloads, History, Extensions) already say what to do next; the tour does not repeat them.

## For whoever runs Mango for others

`MANGO_ACCOUNT_URL` (feature 09) names the page where a reader's plan and billing are kept. When it is set, `GET /api/auth` returns it as `account` to a signed-in caller, the tour gains a card about the account, and Settings → Account shows a link to it. Unset, which is the default, nothing about accounts, plans or billing appears anywhere in Mango: it has no plans or limits of its own.

## Checked

- `npm run check`, the Rust tests (config refuses an `http://`, `//`, `javascript:` or spaced address).
- In a browser against a fresh instance, with and without `MANGO_ACCOUNT_URL`: the tour opens on the empty library, steps forward and back, Skip and Escape end it and it stays ended after a reload; an account with titles does not see it; the zones show on the first chapter and not the second; About brings both back.
