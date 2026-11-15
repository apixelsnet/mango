# 10 — Import from and export to Mihon-compatible apps

Bring an existing library across so nobody starts from zero, and take it out again.

## One importer covers both

Mihon and Suwayomi-Server write the same backup file: `.tachibk`, a gzip-compressed protobuf. Suwayomi's export is Mihon-compatible, so one parser handles both. Pure Rust; the extension host is not needed to import.

## Format (checked 2026-10 against an existing reader of it)

| Message | Fields seen |
|---|---|
| Backup | 1 backupManga, 2 backupCategories, 101 backupSources, 9000+ app-specific settings |
| BackupManga | 3 title, 4 artist, 5 author, 6 description, 7 genre, 8 status, 9 thumbnailUrl, 13 dateAdded, 14 viewer, 16 chapters, 17 categories, 18 tracking, 100 favorite, 104 history, 105 updateStrategy, 112 memo |
| BackupChapter | 3 scanlator, 4 read, 5 bookmark, 6 lastPageRead, 7 dateFetch, 8 dateUpload, 9 chapterNumber, 10 sourceOrder, 13 memo |
| BackupCategory | 2 order, 100 flags |
| BackupTracking | 4 trackingUrl, 5 title, 6 lastChapterRead, 7 totalChapters, 8 score, 9 status, 100 mediaId |

Fields 1 and 2 of each message (source id and url, chapter url and name, category name) and all of BackupHistory were not captured by the quick scan. Take the full schema from the format's published source before coding.

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

## Result (2026-10-10) — built

Code: `mango-reader/src/import.rs`; UI in `routes/Import.svelte`, reached from Settings. API: `POST /api/import/preview` and `POST /api/import` take the file itself as the body; `GET /api/import` gives progress and the report.

- The schema is written out in `import.rs` by field number. The fields the quick scan missed are: title 1 source id, 2 url; chapter 1 url, 2 name; category 1 name; history 1 url, 2 last read; source 1 name, 2 id. A title names its categories by their `order`. `favorite` is left out of the file when true. Reading mode is the low three bits of `viewer_flags`.
- Titles are decoded one at a time from the unpacked buffer. Caps: 128 MB upload, 768 MB unpacked, 200,000 titles, 100,000 chapters a title. An account has one backup looked at at a time; a second preview meanwhile is answered 409 `import_running`.
- A tracker link is kept only when it is an `https` address, as for a link made in Mango.
- The import runs in the background on a database connection of its own, in one transaction, so readers carry on and a failure changes nothing.
- Merge rule: a chapter read on either side is read; an unfinished chapter keeps the further page; a bookmark on either side stays. Mango's own details of a title are kept; the backup fills what is empty and can add the title to the library. This is simpler than "the newer state wins": the backup carries no time for a read mark, and this way nothing is ever taken back.
- Bookmarks and tracker links are stored (`chapters.bookmark`, table `tracks`) though no screen shows them yet.
- Sources the backup uses and Mango lacks are listed with an install button when an added store has their extension.

Checked on Linux and Windows by `smoke.py` (78 checks in all) with a small backup written out by hand, and in a browser. On Linux with a real backup from a Mihon-compatible server: 161 titles, 19,414 chapters, 6 categories, 4,559 history entries came in within 0.2 s, nothing was unreadable or skipped, the unread count matched the backup, and importing it again changed nothing. Half of its 18 sources had an extension in the store that was added.

Not checked: a 5,000-title backup, and the install button with a real missing extension.

## Export (2026-10-10) — built

Code: `mango-reader/src/export.rs`. API: `GET /api/export` answers with the file, named `mango_<date>.tachibk`. In the UI it is a button in Settings, Sources, beside the import.

- What goes out: the titles in the library with their chapters, read state, bookmarks, page reached, categories, history, reading mode and tracker links, and the names of their sources. Titles outside the library, settings and per-source preferences do not.
- It is written with the importer's own shapes, one title at a time into the gzip stream.
- The apps refuse a message that lacks a field they require, and a plain zero is left out of the wire. So the zeros that can occur are written by hand: the local source's id and a tracker link that was never sent.
- A category's `order` is its place in the list. Times go from Mango's seconds to the backup's milliseconds.

Checked on Linux and Windows by `smoke.py`: the export holds the imported title, its chapters, category and tracker link, and taking it back in changes nothing. A unit test exports a library, imports it into an empty database and finds the two alike.

## Checked against the real apps (2026-10-18)

Two Mihon-compatible apps, a server (v2.4.2366) in a container and an Android app (v0.21.2) in an Android 14 emulator, with a real backup of 161 titles, 19,448 chapters (10,157 read, 21 bookmarked, 115 part-read), 4,493 history entries and 6 categories. Compared title by title, by source and address: categories, chapter count, which chapters are read and bookmarked, the page reached, history, tracker links.

- Mango's export restored into both apps with nothing different and no errors.
- A backup made by each of them then came into Mango with nothing different; taking it in again changed nothing.
- Category names and order survive. Three tracker links (AniList, MyAnimeList, and MangaUpdates with an id past 32 bits) went to both apps and back whole.
- Reading mode reaches the Android app on a fresh restore and comes back from its backup. Restoring over a title the app already has keeps the app's own mode: that is how it merges.

## Export my files (2026-10-18) — built

Code: `mango-reader/src/files.rs`. API: `GET /api/export/files` sends the zip; `GET /api/export/files/size` says how many titles, chapters and bytes it would hold. In the UI it is a button in Settings, Downloads, with that size beside it.

- Every downloaded chapter of the account, as `<title>/<chapter>.cbz`, with a `ComicInfo.xml` for the title (name, summary, author, artist, genre, status, as the local source of Mihon-compatible apps reads it) and its cover when Mango keeps one. Unzipped into the local folder of Mihon, Suwayomi or Mango, the titles are there to read.
- Two titles of one name get a folder each: the second takes its source's name, or a number.
- Written while it is sent, so it takes no room on the disk. The `zip` crate needs to seek, so the format is written by hand: stored entries, each file read once for its checksum and once to send, ZIP64 past 4 GB. A caller that goes away stops it.
- Nothing downloaded answers 404 `nothing_downloaded`.

Checked on Linux: a 5 GB export (ZIP64) passed Python's `zipfile`, Info-ZIP `unzip -t` and 7-Zip, with Mango using about 16 MB; a download stopped half way stopped the writing with nothing logged. A small one unzipped with Windows' `Expand-Archive`, read as a local source in Mango (titles, details, covers, chapters, pages) and in the Android app (v0.21.2) (cover, author, artist, status, summary, genres, chapters, and a chapter opened in the reader).

Not checked: the server app's local source, and an export over a slow link through the hosting tunnel.

## Done when

Real backups from two Mihon-compatible apps both import with read progress and categories intact, and re-importing the same file changes nothing. An export opens in both apps with the same.

## Scheduled backups (2026-10-25)

Implementation: `backups.rs`. The schedule is configured in Settings › Sources: never, daily, every three days or weekly (`backup.hours`), retaining between 1 and 30 backups (`backup.keep`, default 3 since 2026-11-14, 7 before; a number an account picked is kept). Each backup is the same `.tachibk` file produced by "Export a backup" and is written to `<data>/backups/mango-YYYYMMDD-HHMMSS.tachibk` (UTC) with owner-only permissions; the oldest backups are deleted as new ones are created. Endpoints: `GET /api/backups`, `POST /api/backups` (create now), and `GET` and `DELETE /api/backups/{name}`; any other file name returns 404.

Backups count against an account's storage, so since 2026-11-14 they are kept small. A backup whose unpacked content equals the newest one's is not written again: the schedule moves on, and `POST /api/backups` answers that newest backup with `unchanged: true`, which Settings reports. The export, and so every backup, is gzip at the highest level. A database snapshot was considered for a backup that restores everything (settings, custom covers, source preferences) and left out on purpose: it would make every backup several times larger.

Exports omit custom covers, and include titles merged into another as separate library titles: the apps have no concept of merged titles, and their reading progress would otherwise be lost.

Verification: unit tests and `smoke.py` on Linux.

## Sent once, and memory (2026-11-14)

- A file bigger than 128 MB is turned down in the browser before it is sent.
- The file the preview read is kept for 30 minutes, and the import that follows takes it from there: it is sent once, not twice. When Mango no longer holds it (it restarted), the page sends it again.
- A backup sits in memory whole while it is read. One backup is read at a time on the whole server, not one per account, and what one may unpack to is `MANGO_IMPORT_MAX_MB` (768 unless set; see feature 09), so a small server can set it lower.
