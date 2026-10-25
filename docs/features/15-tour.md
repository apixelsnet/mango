# 15 — Tour

A short tour for a new account, and a picture of the reader's tap zones. All of it is in the web UI.

## What it does

- **Tour** (`lib/Tour.svelte`, `lib/guide.svelte.ts`). A card in the corner (above the tabs on a phone) that walks through the app: each step opens the screen it is about — Library, Extensions, Browse, Updates, Downloads, Settings — so Next and Back move around Mango with the card still there. It is not modal: the screen behind it can be used. The step is kept in `tour.step`, so it survives the change of screen; inside a chapter the card is hidden and comes back where the reader is. Back, Next, Skip, the close button and Escape (from within the card) all work; Skip and finishing count the same.
- It starts by itself on the Library screen when the library is empty and the account's setting `tour.done` is unset. An account that already has titles is not shown it unasked, so an upgrade does not put it in front of people who know Mango.
- **Tap zones** (`lib/Zones.svelte`). The first chapter an account opens draws over the page where a tap goes back, forward or opens the menu, computed from the reader's own `zone()` rule on a 12×12 grid, so it is right for every layout, direction and swap. Tap or Escape closes it and sets `tour.zones`. Not shown while the zones are off (manhwa and manhua by default). The reader's top bar has a button to show it again.
- **Again**: Settings → About → Show the tour. It clears `tour.zones` and opens the tour.
- Both flags are ordinary settings (`/api/settings`), so they belong to the account and follow it to every device.
- The empty screens (Library, Browse, Updates, Downloads, History, Extensions) already say what to do next; the tour does not repeat them.

## For whoever runs Mango for others

`MANGO_ACCOUNT_URL` (feature 09) names the page where a reader's plan and billing are kept. When it is set, `GET /api/auth` returns it as `account` to a signed-in caller, the tour gains a card about the account, the top bar gains an account button beside Settings that opens Settings → Account, and that tab shows a link to the page. With it set, the UI also asks `GET /api/plan` (`lib/plan.svelte.ts`). Mango has no such route: a service in front of a hosted Mango answers it with `{name, storage: {used, of} | null, full, opds, trackers, autoDownload, updatesEvery}`, and a 404 means no plan. From the answer, Settings → Account shows the plan and a storage bar (`lib/Storage.svelte`); what the plan leaves out carries a lock mark that leads to the account page (`lib/Locked.svelte`: the OPDS address, tracker sign-in and linking, a title's automatic download, update intervals shorter than the plan allows). While the storage is full, or when a call is refused with `storage_full`, a card comes up on every screen but Settings and the download queue, where room is made (`lib/StorageFull.svelte`). It offers the account page, deleting chapters already read, files of titles left, clearing the page cache (which is said not to count), or a choice in Settings. The service in front then also refuses browsing and searching sources and imports, and holds the download queue through `MANGO_HOLD_FILE`. The locks are only shown: the service in front refuses the calls itself. Unset, which is the default, nothing about accounts, plans or billing appears anywhere in Mango: it has no plans or limits of its own.

## Checked

- `npm run check`, the Rust tests (config refuses an `http://`, `//`, `javascript:` or spaced address).
- In a browser against a fresh instance, with and without `MANGO_ACCOUNT_URL`: the tour opens on the empty library, steps forward and back, Skip and Escape end it and it stays ended after a reload; an account with titles does not see it; the zones show on the first chapter and not the second; About brings both back.
