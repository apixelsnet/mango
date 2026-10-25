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

## The title page, edited and merged (2026-10-25)

What Mihon's forks are asked for most, on a title's page. Code: `titles.rs` (`edit_details`, `merge`, `prefer`, `listed_chapters`), `routes/Title.svelte`; migration `012_requests.sql`.

- **Edit details**, from the title's menu: name, author, artist, description, genres, status, and a cover of one's own (`PUT /api/manga/{id}/details`, `PUT /api/manga/{id}/cover` with the picture as the body, 8 MB at most, sniffed like any image). Each field changed is locked (`manga.locked`): a refresh and a source's list leave it alone. "Follow the source again" (`DELETE .../details`) unlocks them all and refreshes. A cover set by hand lives in `covers/` as `<id>-custom-<mark>`; a backup leaves it out.
- **Another source**: "Add another source" searches every source for the title (`#/search?q=&merge=`); the title picked is merged in (`POST /api/manga/{id}/merge/{other}`, its chapters fetched first when they never were). A merged title leaves the library as a title of its own and updates, downloads and is read under the one it went into; its chapters show there, by number, with their source named. "Take out" (`DELETE .../merge/{other}`) parts them; the title taken out is not in the library after. Opening a merged title opens the one it is in.
- **Scanlation groups**: an order of the groups wanted first, and "one upload per chapter" (`PUT /api/manga/{id}/scanlators`). With it on, every other upload of a chapter is marked `duplicate`, hidden unless "Other uploads" is picked, passed over by the reader's next and previous, and counted once in the library. Sources come before groups: the title's own first, then those merged in, in their order.
- **By volume**: the chapter list can be headed by volume, read from names such as "Vol.2 Ch. 10" (`ChapterView.volume`).
- **Kept pages**: pages kept from the reader, with their notes, listed under the trackers; each opens the reader on its page.
- **Same name**: adding a title asks for library titles of the same name (`GET /api/manga/{id}/namesakes`, letters and digits compared). If there is one, it says so and offers to read this title as one of its sources, or to add it anyway.
- Browse: a source can be put in a folder of the owner's own (`PUT /api/sources/{id}` with `folder`; empty takes it out). Folders show under the sources, after the pinned ones.

Checked by unit tests (edits held through refreshes, merges and their order, the preferred upload, volumes, kept pages) and by `smoke.py` on Linux (204 checks). Checked in Chromium against a Mango on the Linux server, at 1280×860 and 390×844: edit form, merge with two sources listed and the merged title leading to its head, one upload per chapter (5 rows to 4, the library counting 4), volume headings; no screen scrolls sideways and the console stayed clean.

Not checked: two real online sources merged, a cover set by hand from a phone.
