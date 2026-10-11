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

## Result (2026-10-10) — built, with two gaps

Code: `mango-reader/src/titles.rs`, `images.rs`; UI in `mango-reader/ui/src/routes/` (`Browse`, `Search`, `Source`, `Title`, `Extensions`).

- Every title a source shows is stored, in the library or not, so it has an id, a cached cover and a place for chapters. A title page opens from the database; details and chapters are fetched the first time and on request.
- Global search asks each source on its own, five at a time, with a 30 s limit each. It covers pinned sources if there are any, else the sources in the languages picked on Browse, and at most 20.
- The source list keeps to English until other languages are picked, or to every language when no source is English. The language pills sit behind a small button in the page head; "All" lifts the limit.
- Covers and pages go through the host and are kept on disk. Only what is recognisably an image is passed on to the browser.
- Checked on Linux and Windows by `smoke.py` (popular 0.3 to 0.7 s, title with chapters 0.9 s, cover 0.1 to 0.3 s) and by hand in a browser.

Not built:
- Filters from the source. The host answers 501 for them; they need host work first. (Per-source preferences came on 2026-10-11, on the extension's page: see feature 02.)
- The source link opens the source's site in a new tab. It is put together from the source's base address, so it is missing for sources whose title keys are not paths.

## Done when

A title can be found by global search, opened, and added to the library.
