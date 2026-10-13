# 02 — Extension store

Add stores, browse the catalog, install, update and remove extensions.

## MVP

- Add a store by URL (`repo.json` or `index.pb`). More than one store allowed. A store that is there already under another address is recognised by its signing key and not added twice.
- Parse the index in Rust. Browsing the catalog never starts the JVM.
- List with search, language filter and content-warning filter. NSFW hidden by default.
- Install: download the JAR (APK as fallback), check sha256 and signing key, hand to the host.
- Pin the store's signing key on first add. Warn loudly if it changes.
- Update check on a schedule, with "update all". Remove an extension.
- Show per extension: name, version, lib version, languages, sources, installed or update available.

## How it works

Code: `mango-reader/src/store/`. Test: `mango-reader/smoke.py`.

- A store is a `repo.json` that points to an `index.pb`, a gzipped protobuf. The core parses it and keeps the catalog in SQLite. A refresh sends the ETag, so an unchanged store costs one small request.
- The signing key is the certificate fingerprint in the index. It is pinned when the store is added. If a later index names another key, the catalog is left as it was, installs from that store are refused, and the store reports `keyChanged` until the owner accepts the new key or removes the store.
- Install: the core downloads the jar and checks two things. Its sha256 against the store's `release-assets.json`, when the store publishes one. Its JAR signature against the pinned key. Then it gives the host the URL and the hash of the bytes that passed; the host downloads the same file and checks the hash again.
- The signature check is the JAR v1 scheme, SHA-256 with RSA. Every file in the jar must be covered by it. Anything else is refused.
- Updates come only from the store an extension was installed from.
- Installed extensions and their sources are mirrored in the core's database, so listing them never starts the JVM. After each host start the core compares its list with the host's and follows the host.

## API

Under `/api`. Errors are `{"error": {"code", "message"}}`.

- `GET /stores` · `POST /stores {url}` · `DELETE /stores/{id}`
- `POST /stores/{id}/refresh` · `POST /stores/refresh` · `POST /stores/{id}/accept-key`
- `GET /catalog?q=&lang=&content=&state=&store=&limit=&offset=` · `GET /catalog/{store}/{pkg}`
  - `content`: any of `safe`, `mixed`, `nsfw`. Left out, NSFW is hidden.
  - `state`: `installed`, `available` or `updates`.
- `GET /extensions` · `POST /extensions/{pkg}/install` · `DELETE /extensions/{pkg}` · `POST /extensions/update-all`
- `GET /sources` — sources of the installed extensions
- `GET /sources/{id}/preferences` · `PUT /sources/{id}/preferences {position, key, value}` — what a source lets its user set; both answer `{preferences}`. Passed on to the host; see feature 01.
- `GET /health` — version, and whether the host is asleep, starting, running, in backoff or missing

## Result (2026-10-10) — core passed on Linux and Windows

18 unit tests, then `smoke.py` (26 checks) against a real Mihon extension store, with the real host. A few runs each.

| | Linux (Docker) | Windows (native) |
|---|---|---|
| Core memory, host asleep | 14 MB | 18 MB |
| Core and host, one extension loaded | 180 MB | about 170 MB |
| Add the store (1,428 extensions) | 0.7 to 1.3 s | 0.7 to 1.0 s |
| Refresh, nothing new | 0.08 s | 0.1 s |
| Catalog search | 1 ms | 1 ms |
| Install, host asleep / awake | 1.8 to 2.2 s / 0.25 s | 1.2 to 1.7 s / 0.2 s |
| Host start | 0.4 to 0.8 s | 0.45 s |

The binary is 8 MB. Also checked by hand: a killed host is restarted on the next call, a host that keeps dying is backed off, a wiped host folder is noticed, and the host exits with the core.

Still open:
- No UI. "Done when" below needs Browse, which is step 4.
- No login until step 9. Until then the API is open to anyone who can reach the port, so it listens on loopback by default, and requests a browser marks as coming from another site are refused.
- APK-only extensions are refused (`apk_not_supported`). The store tested against has none.
- Jars signed with SHA-1, DSA or ECDSA are refused.
- A store that keeps its extension list in a separate file (`extensionListUrl`) is handled in code but was never met in testing.
- No endpoint yet that lists the languages a store has, for a filter menu.

## An extension's own page (2026-10-11)

An installed extension opens a page of its own (`ui/src/routes/Extension.svelte`, `#/extension/{pkg}`): its sources, each with a button that hides it from Browse and one that opens its settings. The settings are the source's own preferences, read when the panel opens and saved one change at a time. A source without any says so.

Checked by hand in a browser on Linux against an extension with 61 sources: a switch, a list, a multi and a text were each changed and put back, and a source was hidden and shown again.

## Rows that can be followed (2026-10-13)

On a wide window a name sat far from its button, with nothing to lead the eye across. In `ui/src/routes/Extensions.svelte`:

- The available extensions are laid out in as many columns as fit, each at least 340 px wide, a name and its button boxed together. A phone still gets one column.
- Stores and installed extensions stay rows, every other one shaded.
- The row or box under the pointer, or the one holding the keyboard's focus, is lit in the spot colour with its Install button filled.
- The mark that loads the next sixty is made anew after each load. In columns a load may not push it out of sight, and it is only asked once per sighting.

Checked in a browser on Linux against a store of 1,433 extensions, in the light theme, the dark one and on a black paper: 5 columns at 2000 px, 7 at 2560, 3 at 1280, 1 at 390, a button at most 295 px from its name, nothing running off the side, with and without the 18+ mark. At 2560 by 1440 the list went on to 180 by itself and stopped.

## What a store may not do (2026-10-11)

Only extensions are signed; a store's index is not. Four rules close what that left open.

- A store, its index, its hash list and its jars are fetched over HTTPS and from public addresses only: the address given, every address its name resolves to, and every redirect. Otherwise an account could aim the server at this machine or its network and read from the error what answered there, and a store added over plain HTTP would have its key pinned from whatever the network offered. Refused with 400 `address_refused`. `MANGO_STORES_LOCAL=on` allows both, for a store of one's own. A store added earlier over plain HTTP keeps its catalog and reports the refusal at its next refresh.
- The highest version of each extension a store ever listed is kept (`seen_versions`). An index that lists an older one is shown as it is, but installing from it is answered 409 `older_version`: the older file would still carry a valid signature. Removing the store and adding it again forgets the mark.
- The host is told which package and which version were asked for, and refuses a file that declares another package or an older version (422 `bad_extension`). Before, one store's file could replace an extension that came from another store.
- An archive with more than one end-of-archive record is refused. The check here and the JVM that loads the file read a zip with different code; a second record is the known way to show each of them another content.

Checked by unit tests and, for the first two, by the smoke test on Linux and Windows. On Linux also with the real store and its catalog changed in the database: an entry pointed at another extension's file was answered 422 "The file is …, not the extension that was asked for", one listed a version higher than its file "The file is an older version than the store lists", and both times the host kept what it had and went on answering. A real name that resolves to a private address was refused at lookup, and real redirects to plain HTTP, to a private address and to such a name were refused on the way.

Not checked: a store of one's own that serves such an index, which would need a signing key.

## Done when

A fresh install can add a Mihon extension store, find an extension by name, install it and see its sources under Browse.
