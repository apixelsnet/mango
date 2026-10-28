# 13 — OPDS

The library as a catalog for reading apps. Lives in `mango-reader/src/opds.rs`.

## What it does

- `/opds` is an OPDS 1.2 catalog: the library, what is unread, each category, then a title's chapters with the first chapter first.
- A chapter can be fetched whole as a CBZ. A downloaded chapter is its file; any other is fetched from its source and packed for that one request, without being saved.
- Once Mango knows how many pages a chapter has (it was opened or downloaded), it is also offered page by page (OPDS-PSE), with the page last read.
- Search goes through the catalog's OpenSearch description and looks at titles in the library.

## Signing in

- A reading app keeps no session, so it sends an account's name and password with every call (HTTP Basic). It sees that account's library.
- A pair that was right is not hashed again for ten minutes. Wrong ones count toward the same wait as wrong passwords in the browser.
- A browser that is signed in may open the feeds too. With login switched off, no password is asked.

## Addresses

All under `/opds/v1.2`. Every address in a feed is relative, so a proxy may put Mango under a sub-path. The one whole address, the search template, is put together from `Host`, `X-Forwarded-Proto` and `X-Forwarded-Prefix`.

- `catalog` · `library?category=&unread=1&q=&page=` · `manga/{id}?page=` · `search.xml`
- `manga/{id}/cover?size=small` · `chapters/{id}/file` · `chapters/{id}/pages/{n}?look=1`, counted from 0

The library comes 100 titles to a feed and a title 200 chapters to a feed, with a `next` link.

## Notes

- A whole CBZ fetched marks nothing read: fetching a file is not reading it. Pages streamed do; see below.
- A chapter is packed in memory, up to 512 MB, one at a time for each account; a second request waits.
- A first page stands in as a chapter's picture only for downloaded chapters, so listing a title asks no source for anything.

## Result (2026-10-10) — built

The smoke test reads the catalog as an app would, on Linux and Windows: the sign-in is asked for and a wrong one refused, the library lists the title with its cover, a title lists its chapters, a saved chapter comes as its CBZ and page by page, a chapter of the local source that was never saved is packed, and a second account sees its own empty library.

Not checked: a real reading app. Packing a chapter that has to be fetched from a remote source. A reverse proxy and a sub-path.

## Progress from reading apps (2026-10-25)

OPDS has no way for an app to report what was read, so a page fetched through OPDS-PSE is taken as read, as Kavita and Komga do.

- A streamed page further on than the chapter's place moves the place to it; the chapter's last page marks it read, with the same follow-ups as the reader in the browser (delete after reading, tracker update, other uploads marked read when that option is on). Going back moves nothing, and a chapter already read is left as it is.
- A chapter's picture in a feed is its first page with `?look=1`, which does not count as reading. A title's thumbnail is now its small cover.
- Settings, Reading apps: "Keep the place from reading apps", on by default (`opds.progress`). Apps that fetch pages ahead can mark a chapter read a little before its end is shown.

Verification: a unit test for moving on, going back, the last page, a chapter already read and the option off; on Linux and Windows builds, a looked-at page moved nothing, page 3 of 4 set the place to 2, page 4 marked the chapter read, and the library feed's thumbnail pointed at the small cover.

Not verified: a real reading app.

## KOReader progress sync (2026-10-28)

KOReader's own sync speaks to `/opds/kosync` (`kosync.rs`), so it falls under the same plan rule as the catalog. In KOReader: Progress sync › Custom sync server, that address, then the account's name and an app password (feature 09). Making an account from KOReader is refused.

- KOReader names a document by the MD5 of 1 KB samples of its file, or by the MD5 of its name. Each time Mango hands out a chapter's CBZ (OPDS, or "Save as a CBZ file"), it notes both marks against the chapter (`sync_documents`).
- A place KOReader reports is kept as sent (`sync_progress`). For a document Mango handed out, the page moves the chapter's place, and the last page or 100 % marks it read, with the same follow-ups as the reader.
- Asked for a place, Mango answers KOReader's last report, or its own place when the chapter was read in Mango since, as device "Mango". A document Mango did not hand out still syncs between KOReader's devices.
- Streamed pages (OPDS-PSE) make no file and so no mark; their place moves by the pages fetched, as before.

Checked by unit tests (the sampling, both directions) and end to end on Linux and Windows with KOReader's calls made by hand: signing in with the app password and not the main one, a report moving the chapter, the report coming back, Mango's own place offered after reading on, a revoked app password refused.
- The catalog gives each chapter entry the series as its author. KOReader saves a file as "author - title" (its own words for a missing author otherwise), so the files of a series sit together, and Mango notes the mark of that very name (`koreader_name`), as well as of the name it sends in `Content-Disposition`, which KOReader uses when told to keep the server's names.
- `/opds` and `/opds/` are the catalog itself, no longer a redirect to it: KOReader leaves the name and password out when it follows a redirect, and `/opds/` was answered with the web page before.

Checked in the emulator of KOReader 2026.07.1, driven on Linux: the catalog opened at `/opds/` with an app password, a chapter downloaded as "Sample Series - Arrival.cbz", a push moved the chapter in Mango, a place read on in Mango was pulled back into KOReader, matching by file name moved it as well, and the last page marked the chapter read.
