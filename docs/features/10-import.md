# 10 — Import from and export to Mihon and Suwayomi

Bring an existing library across so nobody starts from zero, and take it out again.

## One importer covers both

Mihon and Suwayomi-Server write the same backup file: `.tachibk`, a gzip-compressed protobuf. Suwayomi's export is Mihon-compatible, so one parser handles both. Pure Rust; the extension host is not needed to import.

## Format (checked 2026-10 against Suwayomi's reader)

| Message | Fields seen |
|---|---|
| Backup | 1 backupManga, 2 backupCategories, 101 backupSources, 9000+ app-specific settings |
| BackupManga | 3 title, 4 artist, 5 author, 6 description, 7 genre, 8 status, 9 thumbnailUrl, 13 dateAdded, 14 viewer, 16 chapters, 17 categories, 18 tracking, 100 favorite, 104 history, 105 updateStrategy, 112 memo |
| BackupChapter | 3 scanlator, 4 read, 5 bookmark, 6 lastPageRead, 7 dateFetch, 8 dateUpload, 9 chapterNumber, 10 sourceOrder, 13 memo |
| BackupCategory | 2 order, 100 flags |
| BackupTracking | 4 trackingUrl, 5 title, 6 lastChapterRead, 7 totalChapters, 8 score, 9 status, 100 mediaId |

Fields 1 and 2 of each message (source id and url, chapter url and name, category name) and all of BackupHistory were not captured by the quick scan. Take the full schema from Mihon's source before coding.

## MVP

- Upload a `.tachibk`. Show a preview first: titles, categories, chapters, sources needed.
- Import: library titles, categories, chapter read state, bookmarks, last page read, history.
- Titles are matched by source id and url. A title whose extension is missing is still imported and marked "extension needed", with a one-click install when the store has it.
- Merge, never overwrite: existing titles keep the newer read state.
- Tracker links are stored even though trackers come later, so nothing is lost.
- Report at the end: imported, skipped, failed, with reasons.

## Notes

- The file is untrusted input: cap decompressed size, bound list lengths, never trust paths.
- Import is one transaction; a failure leaves the library unchanged.
- Runs in the background with progress; a 5,000-title backup must not block the UI.

## Result (2026-10-10) — built; checked with Suwayomi, not with Mihon

Code: `mango-reader/src/import.rs`; UI in `routes/Import.svelte`, reached from Settings. API: `POST /api/import/preview` and `POST /api/import` take the file itself as the body; `GET /api/import` gives progress and the report.

- The schema is written out in `import.rs` by field number. The fields the quick scan missed are: title 1 source id, 2 url; chapter 1 url, 2 name; category 1 name; history 1 url, 2 last read; source 1 name, 2 id. A title names its categories by their `order`. `favorite` is left out of the file when true. Reading mode is the low three bits of `viewer_flags`.
- Titles are decoded one at a time from the unpacked buffer. Caps: 128 MB upload, 768 MB unpacked, 200,000 titles, 100,000 chapters a title. An account has one backup looked at at a time; a second preview meanwhile is answered 409 `import_running`.
- A tracker link is kept only when it is an `https` address, as for a link made in Mango.
- The import runs in the background on a database connection of its own, in one transaction, so readers carry on and a failure changes nothing.
- Merge rule: a chapter read on either side is read; an unfinished chapter keeps the further page; a bookmark on either side stays. Mango's own details of a title are kept; the backup fills what is empty and can add the title to the library. This is simpler than "the newer state wins": the backup carries no time for a read mark, and this way nothing is ever taken back.
- Bookmarks and tracker links are stored (`chapters.bookmark`, table `tracks`) though no screen shows them yet.
- Sources the backup uses and Mango lacks are listed with an install button when an added store has their extension.

Checked on Linux and Windows by `smoke.py` (78 checks in all) with a small backup written out by hand, and in a browser. On Linux with a real Suwayomi backup: 161 titles, 19,414 chapters, 6 categories, 4,559 history entries came in within 0.2 s, nothing was unreadable or skipped, the unread count matched the backup, and importing it again changed nothing. Half of its 18 sources had an extension in the store that was added.

Not checked: a real Mihon backup (none was at hand; the schema is Mihon's), tracker links from a real file (that backup had none), a 5,000-title backup, and the install button with a real missing extension.

## Export (2026-10-10) — built; read back by Mango, not yet by Mihon or Suwayomi

Code: `mango-reader/src/export.rs`. API: `GET /api/export` answers with the file, named `mango_<date>.tachibk`. In the UI it is a button in Settings, Sources, beside the import.

- What goes out: the titles in the library with their chapters, read state, bookmarks, page reached, categories, history, reading mode and tracker links, and the names of their sources. Titles outside the library, settings and per-source preferences do not.
- It is written with the importer's own shapes, one title at a time into the gzip stream.
- Mihon refuses a message that lacks a field it requires, and a plain zero is left out of the wire. So the zeros that can occur are written by hand: the local source's id and a tracker link that was never sent.
- A category's `order` is its place in the list. Times go from Mango's seconds to the backup's milliseconds.

Checked on Linux and Windows by `smoke.py`: the export holds the imported title, its chapters, category and tracker link, and taking it back in changes nothing. A unit test exports a library, imports it into an empty database and finds the two alike.

Not checked: opening the file in Mihon or in Suwayomi. The format is theirs and the required fields are covered, but neither app was at hand.

## Done when

A real Mihon backup and a real Suwayomi backup both import with read progress and categories intact, and re-importing the same file changes nothing. An export opens in Mihon and in Suwayomi with the same.
