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
- `manga/{id}/cover` · `chapters/{id}/file` · `chapters/{id}/pages/{n}`, counted from 0

The library comes 100 titles to a feed and a title 200 chapters to a feed, with a `next` link.

## Notes

- Reading through OPDS does not mark anything read: the protocol has no way to say so.
- A chapter is packed in memory, up to 512 MB, one at a time for each account; a second request waits.
- A first page stands in as a chapter's picture only for downloaded chapters, so listing a title asks no source for anything.

## Result (2026-10-10) — built

The smoke test reads the catalog as an app would, on Linux and Windows: the sign-in is asked for and a wrong one refused, the library lists the title with its cover, a title lists its chapters, a saved chapter comes as its CBZ and page by page, a chapter of the local source that was never saved is packed, and a second account sees its own empty library.

Not checked: a real reading app. Packing a chapter that has to be fetched from a remote source. A reverse proxy and a sub-path.
