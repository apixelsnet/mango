# 04 — Library and history

The home screen: what the user follows, and where they left off.

## MVP

- Categories: create, rename, reorder; a title can be in several.
- Views: cover grid and list.
- Sort: last read, last updated, unread count, title, date added.
- Filter: unread, downloaded, completed. Search by title.
- Unread badge on every title.
- Bulk select: mark read or unread, download, move category, remove.
- Continue reading: the most recent title, front and centre.
- History: chapters read, by day; resume or remove an entry.
- Per-title settings: reader mode, auto-download.

## Data

SQLite: `extension`, `source`, `manga`, `chapter`, `category`, `manga_category`, `history`, `setting`.

## Notes

- The library must paint from the database alone. Nothing on this screen waits on the extension host.
- Must stay smooth at several thousand titles: virtualized grid, thumbnails cached on disk.

## Result (2026-10-10) — built

Code: `mango-reader/src/library.rs`; UI in `routes/Library.svelte`, `History.svelte`, `Settings.svelte`.

- The library comes in one answer, with counts per title; sorting, filtering and search happen on screen. Categories, bulk changes and the history are plain database work. None of it calls the host.
- "Last read" comes from the history, so marking chapters read in bulk does not shuffle the order.
- Continue reading is the chapter last opened if it is unfinished, else the next unread one of that title.
- The grid is not a hand-made virtual list. Each card is marked so the browser skips laying out and painting the ones off screen.

Measured with 2,000 titles and 120,000 chapters, host asleep: the library call takes 35 ms (400 KB); in the browser the slowest frame while scrolling was 9 ms, a search took 5 to 26 ms, and going back to all titles 77 ms.

Not built yet: download from the bulk bar and per-title auto-download, which belong to step 6. The bulk bar adds titles to a category; taking them out is done on the title page.

## Done when

A library of 2,000 titles opens, scrolls and filters without visible lag, with the host stopped.
