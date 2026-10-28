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
- Global search asks each source on its own, five at a time, with a 30 s limit each. It covers pinned sources if there are any, else the sources in the languages picked on Browse, and at most 20. It starts from the field at the top of Browse, and its results carry the field for another try. Until 2026-10-13 the field was in the header of every screen, where it stood beside the library's own and was taken for it.
- The source list keeps to English until other languages are picked, or to every language when no source is English. The language pills sit behind a small button in the page head; "All" lifts the limit.
- Covers and pages go through the host and are kept on disk. Only what is recognisably an image is passed on to the browser.
- A JPEG cover over 256 KB is decoded at a half, quarter or eighth of its size (no smaller than 600×900) and saved again before it is kept or sent, one at a time; some sources hand out 10 to 20 MB scans. Other formats, progressive JPEGs over 16 MP and covers with an EXIF turn are left as they came.
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

- One button over the list, "Sort & filter", opens both; the list's head keeps to three controls. What it opens is drop-downs, not a row of buttons.
- Order: the source's own, by chapter number, or by upload date, each either way round. It is one setting for every title, kept on the server (`title.view`) like the library's.
- Filter: all chapters, or only the unread, the downloaded or the bookmarked, one at a time; and one scanlation group when the title has more than one. It lasts as long as the page is open.
- A row keeps to two buttons: download, and one that opens the chapter's own menu, where it is marked read or unread and bookmarked. "Mark read up to here" also marks every older chapter, in the source's order and whatever the filter shows; only the unread ones are sent, so chapters already read keep their read date.
- Selecting, after Mihon: a long press on a row, "Select" in the head or in a chapter's menu, or a shift-click starts it. While selecting, a tap picks one chapter, and a long press or shift-click on another picks every chapter shown between the two. The dock turns into the actions: all shown, invert, mark read or unread, mark read up to here (one chapter picked), bookmark, download, delete the saved files (asked twice). Only the chapters shown count; Escape stops. Each action sends only the chapters it changes. Deleting is one `DELETE /api/chapters/{id}/download` per chapter; there is no bulk address for it. A bookmarked chapter carries a small mark before its name.
- A bookmark is `PUT /api/manga/{id}/chapters` with `{ids, bookmark}`, the address that marks chapters read. Bookmarks already came in with a backup and went out with one; now they show.
- The order and the filter are for looking. Reading goes on in the source's order: Resume and the reader's next chapter do not follow them.

Checked on Linux and Windows by `smoke.py`, and by hand in a browser at a desk's width and a phone's.

## Done when

A title can be found by global search, opened, and added to the library.

## Title page: editing, merging and chapter preferences (2026-10-25)

Implementation: `titles.rs` (`edit_details`, `merge`, `prefer`, `listed_chapters`) and `routes/Title.svelte`. Schema changes are in migration `012_requests.sql`.

- **Editing details.** The title menu opens a form for the name, author, artist, description, genres and status (`PUT /api/manga/{id}/details`). A custom cover may be uploaded (`PUT /api/manga/{id}/cover`, with the image as the request body, up to 8 MB; the content is validated as an image). Each edited field is recorded in `manga.locked` and is preserved when the title is refreshed or appears in a source listing. "Follow the source again" (`DELETE /api/manga/{id}/details`) removes all locks and refreshes the title. Custom covers are stored in `covers/` as `<id>-custom-<hash>` and are excluded from backups.
- **Merging sources.** "Add another source" searches all sources for the title (`#/search?q=&merge=`). The selected title is merged into the current one (`POST /api/manga/{id}/merge/{other}`); its chapters are fetched first if they have never been fetched. A merged title is removed from the library as a separate entry; it is updated, downloaded and read as part of the title it was merged into, and its chapters are listed there by number with their source shown. "Take out" (`DELETE /api/manga/{id}/merge/{other}`) reverses the merge; the separated title is not returned to the library. Opening a merged title redirects to the title that contains it.
- **Scanlation groups.** The user may define a preferred order of scanlation groups and enable "one upload per chapter" (`PUT /api/manga/{id}/scanlators`). When enabled, every upload other than the preferred one is marked `duplicate`: it is hidden unless "Other uploads" is selected, skipped by the reader's next and previous actions, and counted once in the library. Source order takes precedence over group order: the title's own source first, followed by merged sources in the order they were added.
- **Volumes.** The chapter list can be grouped under volume headings. The volume is derived from chapter names such as "Vol.2 Ch. 10" (`ChapterView.volume`).
- **Saved pages.** Pages saved from the reader, together with their notes, are listed below the tracker section. Each entry opens the reader at the saved page.
- **Duplicate detection.** Before a title is added to the library, the server is queried for library titles with the same name (`GET /api/manga/{id}/namesakes`; names are compared by their letters and digits only). If a match exists, the user is informed and may either read the new title as an additional source of the existing one or add it anyway.
- **Source folders.** On the Browse screen, a source can be assigned to a user-defined folder (`PUT /api/sources/{id}` with `folder`; an empty value removes the assignment). Folders are listed below the pinned sources.

Verification: unit tests cover locked fields across refreshes, merging and member order, preferred-upload selection, volume parsing and saved pages; `smoke.py` passed all 204 checks on Linux. In Chromium against a Linux build, at 1280×860 and 390×844, the following were confirmed: the edit form; a merge showing both sources and the merged title redirecting to its parent; "one upload per chapter" reducing five rows to four, with the library reporting four chapters; and volume headings. No screen scrolled horizontally and no console errors were logged.

Not verified: a merge of two real online sources, and a custom cover uploaded from a phone.

## Small covers (2026-10-25)

Grids and lists ask for a cover's small copy (`GET /api/manga/{id}/cover?size=small`); the title page still shows the cover itself.

- The copy fits within 360×540, twice the widest card, keeping the cover's shape, and is saved as a JPEG at quality 82. It is made once, on first request, and stored next to the cover with `-small` appended to the name: under `covers/` for library titles and hand-set covers, and in the swept cache for browsed titles and local ones. A local title's copy is named by a hash of its cover, so a changed cover gets a new copy.
- JPEG, PNG and WebP are read (`png` and `image-webp` crates, about 0.4 MB added to the binary). A cover that already fits, another format, animated or see-through artwork, an EXIF turn, or a copy that would come out larger is served as the cover itself.
- JPEGs are decoded at the smallest power-of-two scale that still covers the box; the remainder of the reduction averages the source pixels each output pixel covers. Work is done one cover at a time, as with the existing shrink.
- The service worker matches kept covers without their query string, so a title kept for offline reading still shows its cover in grids.

Verification: unit tests for JPEG and PNG copies, see-through and small inputs, and the averaging; `smoke.py` passed on Linux and Windows. Against a running build on both, a 1200×1800 PNG cover of 1.1 MB came back as a 360×540 JPEG of 77 KB, the same file on a second request; a hand-set WebP of 30 KB came back as a 360×241 JPEG of 20 KB.

Not verified: a library grid in a browser, and a real source's progressive JPEG.

## Source link and saving a chapter (2026-10-28)

- The source link is the title's page as the source makes it (feature 01, protocol 6), so it is there for sources whose title keys are not paths. A title that was not refreshed since keeps the link put together as before until its next refresh.
- A chapter's menu has "Save as a CBZ file": `GET /api/chapters/{id}/file`, the same file OPDS hands out (the downloaded CBZ when there is one, else the pages packed for that one request). KOReader's sync learns the file's marks from it, as it does from OPDS (feature 13).

Checked on Linux and Windows: the file is a CBZ named after title and chapter, and saves from the menu in a browser.
