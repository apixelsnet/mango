# 04 — Library and history

The home screen: what the user follows, and where they left off.

## MVP

- Categories: create, rename, reorder; a title can be in several.
- Views: cover grid and list.
- Sort: last read, last updated, unread count, title, date added.
- Filter: unread, downloaded, completed. Search by title.
- Unread badge on every title.
- Bulk select: mark read or unread, download, move category, remove.
- Continue reading: the most recent title, front and center.
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

## Nested categories, pagination, bulk migration and statistics (2026-10-25)

Implementation: `library.rs` (`set_parent`, `stats`, the library query), `routes/Library.svelte`, `routes/MigrateMany.svelte` and `routes/Stats.svelte`.

- **Nested categories.** A category may be placed inside another (`categories.parent_id`; `POST /api/categories` and `PUT /api/categories/{id}` accept `parent`, where 0 means the top level). Selecting a category also shows the titles of all categories nested within it, at any depth. A category cannot be placed inside itself or one of its descendants. The parent is set in Settings › Library, and the library displays categories as a tree.
- **Pagination.** The "Library in pages" option (Settings › Appearance, stored per device) shows 24, 48 or 96 titles per page with navigation buttons, which suits e-ink devices that scroll poorly.
- **Bulk migration.** The selection bar offers "Move to other sources" (`#/migrate/many?ids=`). The user selects sources in order of preference. Each title is searched in those sources; the first result with the same name is chosen, otherwise the first result found. After review, the titles are migrated with the same options as a single migration (feature 12).
- **Statistics.** `GET /api/stats` and `#/stats` report the number of titles, titles started and completed, chapters read, days with reading activity and the current streak, chapters read in each of the last twelve months, and library titles by source, genre and status. Days are counted in UTC.
- A title with merged sources includes their chapters in its counts; a title showing one upload per chapter counts chapters rather than uploads.

Verification: unit tests and `smoke.py` on Linux, and Chromium at both viewport sizes.

## Faster library and statistics (2026-10-26)

- Titles showing one upload per chapter are counted in a second, separate query. Before, every title paid for that counting, then threw it away. Statistics count titles with a lighter query of their own instead of building the whole library, and an index of read chapters (migration 014) speeds up the reading days and months.
- Measured with 1,000 titles (20 shown one upload per chapter, 10 merged into another), 300,000 chapters and 10,000 history entries: `GET /api/library` went from 190 to 67 ms and `GET /api/stats` from 252 to 91 ms, both answering exactly as before. A database from the previous version takes the new index on start.

## Undo, screen names and keyboard focus (2026-11-11)

Implementation: `lib/undoing.svelte.ts`, `lib/Undo.svelte`, `lib/named.svelte.ts`, `App.svelte`, `lib/api.ts`.

- **Undo.** Marking chapters read or unread, deleting saved files and taking a title out of the library (title page), marking titles read or unread and taking them out (library), and taking entries out of the history or clearing it show as done at once, with Undo in a bar at the foot of the screen (Ctrl+Z or ⌘Z too). Mango is told when the bar goes after 7 s, when another such action comes, when the screen is left, or when the page closes (the call is then sent with `keepalive`). Undo puts the screen back and nothing is sent, so a chapter's page is kept too. Reads made after a change is sent wait for it, so the next screen shows it. A change that fails says so in the same bar and puts the screen back as Mango has it.
- **Announcements.** The same bar, a polite live region, says what happened after adding a title to the library, putting titles in a category and queueing downloads from the library.
- **Screen names.** The browser's tab and history name the screen: "History · Mango", the title's name on its page, "Chapter 3 · Title · Mango" in the reader.
- **Focus on a new screen.** Opening another screen moves the focus to its heading, so a screen reader says where it is and Tab starts from the top. Moving within a screen (Settings' sections) does not, nor does a screen that puts the focus on a field of its own. A "Skip to content" button comes up on the first Tab.
- **Capitals.** Headings are written in normal case and set in capitals by CSS (`.caps`), because some screen readers spell out a word typed in capitals.
- **Contrast.** With the system asking for more contrast, secondary text is ink like the rest. In Windows' contrast themes a pressed button and the screen open in the menu take the system's highlight, and counts and tags keep a border.
- **Spelling.** Labels use US spelling ("Color wash", "Spot color", "Gray"). A reader background saved as "grey" is read as "gray".

Checked in Chromium against a mock API: marking all chapters read and pressing Undo, or Ctrl+Z, sent nothing; taking a title out and leaving the screen sent one call; a library "Mark read" was sent after 7 s; a history entry came back on Undo; the heading took the focus on each new screen and the tab names were right; the menu's current screen kept its highlight in forced colors. Not checked: a real screen reader, Windows' own contrast themes.

## Filter by source; search by maker and genre (2026-11-12)

Code: `routes/Library.svelte`, `library.rs`.

- The filter row has a source picker when the library's titles come from more than one source: "All sources", or one source by name. Titles whose extension is not installed are listed as "Not installed". The pick is kept with the other filters (`library.view`, as `source`); a source no longer in the library filters nothing.
- The search finds a title by its name, its author, its artist or a genre. A title page's author, artist and genre links open the library searched for them.

## The library keeps its place (2026-11-13)

Code: `routes/Library.svelte`, `lib/router.svelte.ts`. Asked for in #47.

- Coming back to the library from a title, with its Back button or the browser's, finds the same category, search and page, with the title that was opened where it was on screen. Opening the library from the menu keeps the category, search and page but starts at the top.
- They are held in memory for as long as the page is open, like the library itself; a reload starts again from All. A title page's author, artist and genre links open the library searched for them, in All, on the first page.
- The scroll is put back by the card, not in pixels. Cards off screen are not laid out until they come near (`content-visibility` in `lib/TitleCard.svelte`), so on a fresh shelf the rows above are only guessed at and a kept pixel offset landed rows away, further the deeper the scroll: 1,017 px off on a 400-title desktop shelf. The library notes the opened title's place in the list and its card's distance from the top of the screen, and on the way back scrolls until the card is there again, for up to 1.5 s as cards are laid out, ending it on a touch, the wheel or a key. A title that has moved in the list meanwhile (read, under "Last read") gets the router's pixel scroll instead.
- Before, the category belonged to the screen and went back to All each time it opened.

Checked with the Playwright MCP against the mock API with a 600-title library in nested categories, scrolled through with the wheel: phone grid, three trips in a row by both Back buttons; desktop grid twice; list layout; 24 a page on page 3. The opened card came back within 1 px each time, with the category and page kept. A reload starts from All; an author link opens All with the search. The e2e suite has a 400-title case that fails without the fix.
