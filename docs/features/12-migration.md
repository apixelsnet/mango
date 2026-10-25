# 12 — Migration between sources

Move a title to the same title on another source, and keep what was read. For when a source dies, falls behind, or its extension is gone.

## What it does

- A title's menu has "Move to another source". It searches the other sources for the title's name; the name can be changed, since sources often know a title under another one.
- Picking a result puts the two side by side: source, chapters, how far each goes, how far was read. Nothing has changed yet.
- Four switches: reading progress, categories, tracker links, and whether the old title stays in the library (a copy instead of a move).
- Moving puts the new title in the library in the old one's place and opens it.

## How progress is carried

As Mihon does it. Chapters are matched by number, to the hundredth; a chapter with no number, such as a one-shot, by its name.

- Every numbered chapter of the new title up to the highest one read on the old title is marked read. So a source that splits or skips chapters still lands in the right place.
- A matching chapter also takes the bookmark, the page reached and its history entry.
- Merge, never overwrite: a chapter read on the new title stays read.
- The new title takes the old one's date added, and its reader mode, fit and "download new chapters" where it has none of its own.
- A tracker link of the old title replaces the new title's link to the same tracker.

After a move the old title is out of the library and its history and tracker links are gone from it. Its chapters, their read marks and its saved files stay, as when a title is taken out of the library by hand. All of it happens in one transaction, or none.

## API

- `GET /api/manga/{id}/migrate/{to}`: both titles and what would go along (`read`, `categories`, `tracks`). Asks `to`'s source for its chapters first.
- `POST /api/manga/{id}/migrate/{to}` with any of `chapters`, `categories`, `tracks` (default true) and `keep` (default false): does it, and answers with the counts. Asks the source only if the chapters were never fetched.
- `400` when both ids are the same title, `404` when either does not exist.

No change to the host protocol, and no new table.

## Notes

- The old title's source is never asked, so a title whose extension is not installed can be moved.
- The search is the one behind "Search all sources": pinned sources, or those in the languages picked on Browse.
- The title being moved is left out of the results. Another title of the same source can be picked.

## Result (2026-10-10) — built

Code: `mango-reader/src/migrate.rs`; UI in `routes/Migrate.svelte`, the pick mode of `routes/Search.svelte` (`#/search?q=&for=`), and the title menu.

Checked on Linux and Windows by `smoke.py` (117 checks in all), which moves the title of a source that does not exist onto a real one with its category, tracker link and progress. Checked in a browser on Linux: menu, search, comparison with its warning when the new source stops earlier, and the move.

Not checked: a title with hundreds of chapters on both sides, and a move between two real sources that number their chapters differently.

## Many at once (2026-10-25)

From the library's select bar; see feature 04. Each move is the one above, one after another.
