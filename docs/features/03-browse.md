# 03 — Browse and search

Find titles on installed sources and open their title page.

## MVP

- Per source: Popular, Latest (when supported), Search.
- Filters from the source: Header, Separator, Select, Text, CheckBox, TriState, Group, Sort.
- Global search across chosen sources, run concurrently with a per-source timeout. Slow or failing sources never block the others.
- Title page: cover, details, chapter list, source link.
- Add to or remove from the library from any list. Titles already in the library are marked.
- Per-source preferences screen (230 extensions have one).
- Source list: pin, hide, filter by language.

## Notes

- Covers and pages are fetched through the host, never directly, so each source's headers and cookies apply.
- Errors from a source are shown as that source's error, with retry.

## Result (2026-10-10) — built, with one gap

Code: `mango-reader/src/titles.rs`, `images.rs`; UI in `mango-reader/ui/src/routes/` (`Browse`, `Search`, `Source`, `Title`, `Extensions`).

- Every title a source shows is stored, in the library or not, so it has an id, a cached cover and a place for chapters. A title page opens from the database; details and chapters are fetched the first time and on request.
- Global search asks each source on its own, five at a time, with a 30 s limit each. It covers pinned sources if there are any, else the sources in the languages picked on Browse, and at most 20.
- The source list keeps to English until other languages are picked, or to every language when no source is English. The language pills sit behind a small button in the page head; "All" lifts the limit.
- Covers and pages go through the host and are kept on disk. Only what is recognisably an image is passed on to the browser.
- Checked on Linux and Windows by `smoke.py` (popular 0.3 to 0.7 s, title with chapters 0.9 s, cover 0.1 to 0.3 s) and by hand in a browser.

Not built:
- The source link opens the source's site in a new tab. It is put together from the source's base address, so it is missing for sources whose title keys are not paths.

## Filters (2026-10-12)

A source's page has a Filter button beside its lists. It opens the source's own filters, the ones Mihon shows for it: `GET /api/sources/{id}/filters`.

- Code: `mango-reader/ui/src/lib/SourceFilters.svelte`, `Titles::filters` and `browse` in `titles.rs`. The host's side is in feature 01.
- The list is asked for when the panel is first opened, not with the page. It is kept, with what was set in it, while the page stays open.
- A checkbox is a pill, a tri-state a pill that goes round with it, without it, either way. Select and sort are drop-downs, sort with a direction beside it. A group is folded, with a count of what is switched on inside.
- Search sends only the filters that were set to something else, as `filters=[{position, state}]` on the search address, with the words typed if there are any. A search by filters alone shows as "Filtered".
- Reset asks the source for its list again, since some sources fill theirs in only after a first look at the site, and goes back to Popular.
- Global search and migration send no filters: the source's own list as it makes it, as in Mihon.

Checked on Linux and Windows by `smoke.py`, and by hand in a browser against a real source with all the kinds but text: a tag set with and without, words and filters together, close and open again, Reset, and a phone's width.

Not checked: a text filter in a browser, and a source whose list changes between the read and the search. Such a search is refused when a place no longer holds a filter of that kind; Reset reads the list again.

## Chapter list (2026-10-12)

The title page's list can be put in order, narrowed and bookmarked. Code: `routes/Title.svelte`; `Titles::bookmark` in `titles.rs`.

- Order: the source's own, by chapter number, or by upload date, each either way round. It is one setting for every title, kept on the server (`title.view`) like the library's.
- Filter: unread, downloaded, bookmarked, and one scanlation group when the title has more than one. It lasts as long as the page is open.
- A bookmark is a button on each row: `PUT /api/manga/{id}/chapters` with `{ids, bookmark}`, the address that marks chapters read. Bookmarks already came in with a backup and went out with one; now they show.
- The order and the filter are for looking. Reading goes on in the source's order: Resume and the reader's next chapter do not follow them.

Checked on Linux and Windows by `smoke.py`, and by hand in a browser at a desk's width and a phone's.

## Done when

A title can be found by global search, opened, and added to the library.
