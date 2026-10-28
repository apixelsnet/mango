# 01 — Extension host

Runs Mihon extensions for the Rust core. Lives in `mango-extension-host/`.

## Why a JVM

Extensions are compiled Kotlin. They call OkHttp, Jsoup, kotlinx.serialization and Android APIs. They cannot be parsed into data or run from Rust. Suwayomi-Server already solved this with an Android compatibility layer; we take that part and nothing else.

## Facts (checked 2026-10)

- Store index is protobuf, `index.pb`, linked from the store's `repo.json` as `index_v2`.
  - Store: name=1, badgeLabel=2, signingKey=3, contact=4, extensionList=101, extensionListUrl=102.
  - Extension: name=1, packageName=2, resources=3, extensionLib=4, versionCode=5, versionName=6, contentWarning=7, sources=8.
  - Resources: apkUrl=1, iconUrl=2, jarUrl=501.
  - Source: id=1, name=2, language=3, homeUrl=4, mirrorUrls=5, message=7.
- The store tested against publishes a ready JVM `.jar` for each extension beside the APK, with a sha256 in `release-assets.json`. Use the JAR. Keep dex2jar only as a fallback for APK-only stores.
- Extension API is `tachiyomix` 1.6, suspend-based: `getPopularManga`, `getLatestUpdates`, `getSearchManga`, `getMangaUpdate`, `getPageList`. Version 1.4 is still in use.
- Manifest keys: `tachiyomi.extension.class`, `tachiyomix.extensionLib`, `tachiyomix.contentWarning`, `tachiyomix.name`.
- Usage across 1,428 extensions (text search, approximate):

| Uses | Extensions |
|---|---|
| lib 1.6 / lib 1.4 | 1,381 / 47 |
| Own OkHttp interceptor | 279 |
| Preferences | 230 |
| Bitmap or Canvas (image descrambling) | 58, plus 3 shared themes |
| WebView | 39, plus 1 shared theme |
| JS engine | 10, plus 1 shared theme |

## How the runtime is taken from Suwayomi-Server

Not by copying source. The build downloads Suwayomi-Server's release jar at a pinned version and sha256, deletes the parts the host never touches, and ships the rest as `runtime.jar`. Our own code is one small jar beside it.

- Removed: embedded browser (JCEF), web server, GraphQL, databases, web UI, tray, Android framework resources. 183 MB becomes 135 MB. Until host 0.7.1 the JS engine (GraalVM) went too, and it was 99 MB. Bouncy Castle goes too, for its signed jar beside it (see below).
- Kept and used: Android compatibility layer, source API, network stack, extension loaders, config.
- Our code (`mango-extension-host/src/`) starts only that runtime and serves the protocol. It never starts Suwayomi's server.
- Upgrading Suwayomi is a version and hash bump in the `Dockerfile` (at the repository root since step 9; `--target host` builds the host alone, `--target jars` its two jars), followed by the smoke test.
- Cost: our code calls Suwayomi internals (`PackageTools`, `AndroidManifestParser`, `createAppModule`, `ServerConfig`). A Suwayomi release can rename them; the smoke test catches that.
- The strip list is by trial: removing a package that something still references fails at start-up or on first use. Extend it only with the smoke test passing.

License: the Suwayomi jar is MPL-2.0, some parts Apache-2.0. We ship it unmodified apart from deleted entries and point to its source tag.

## Protocol v3 (frozen)

Version 3 (2026-10-11, host 0.5.0) changed one thing: preferences answer and take a change, where they were reserved; see below.

Version 2 (2026-10-10, host 0.2.0) changed one thing: the image call takes a cover address as well as a page. Version 1 had no way to ask for a cover, and a cover fetched as a page fails on sources that build page requests their own way (found with a real extension).

The contract is `mango-extension-host/protocol/openapi.yaml`. Changing a field or status code means a new protocol version. Summary:

Local HTTP, loopback only. The core passes a bearer token when it starts the host. JSON bodies; images are streamed raw. Source ids travel as strings.

The host implements everything below. APK-only extensions answer 501.

- `GET /health` — host version, protocol version, loaded sources
- `POST /extensions/install` `{jarUrl | apkUrl, sha256}` · `DELETE /extensions/{pkg}`
- `GET /sources` · `GET /sources/{id}/filters` · `GET | PUT /sources/{id}/preferences`
- `POST /sources/{id}/popular | latest | search` `{page, query, filters}`
- `POST /sources/{id}/manga` `{manga, chapters, fetchDetails, fetchChapters}`
- `POST /sources/{id}/pages` `{chapter}`
- `POST /sources/{id}/image` `{page}` — image bytes, fetched through the extension's own client

## Rules

- The host knows nothing about the library. It owns extension files, per-source preferences and the cookie jar.
- The core supervises it: start on first call, health ping, restart with backoff, per-call timeout, per-source concurrency cap, stop after an idle timeout.
- Verify sha256 and the store signing key before loading an extension.
- A web view's browser is not in the host: it is downloaded on first use; see below. The JS engine is in the jar and runs interpreted.
- JVM: trimmed runtime (jlink), class-data sharing, capped heap.

## Spike (first build step)

Bare JVM, vendored compat layer, one extension JAR. Call popular, manga and pages, then fetch one image.

Measure: cold start, time to first result, idle RSS, RSS with 10 and 20 sources loaded, size of jar plus runtime.

Pass: the host stays under 400 MB while awake, or we know exactly which part breaks that.

## Spike result (2026-10-10, Linux, in Docker) — passed

The spike's code is no longer in the repository; the host replaced it. It used Suwayomi-Server v2.4.2366 classes as a library and ran only the minimal start-up path (compat layer and dependency injection; no web server, database, GraphQL or WebView). One run per setting.

| Checkpoint | Default JVM | Tuned JVM |
|---|---|---|
| Host ready | 0.54 s, 110 MB | 0.33 s, 101 MB |
| 1 extension loaded (61 sources) | 172 MB | 154 MB |
| Popular, title, pages, one image read | works, 169 MB | works, 173 MB |
| 10 extensions (114 sources) | 182 MB | 175 MB |
| 20 extensions (262 sources) | 220 MB | 181 MB |

Tuned = `-XX:+UseSerialGC -Xmx256m -Xss512k -XX:TieredStopAtLevel=1`. Memory is resident set size.

Calls through a real extension: load 0.1 s, popular 0.4 s, details and chapters 0.9 s, one 7 MB image 0.5 s. The JAR's sha256 matched `release-assets.json`.

For comparison, the full Suwayomi-Server sat at 1.05 GiB idle with 11 extensions.

What this settles:
- The "host awake under 400 MB" target holds with room to spare; about 180 MB is realistic.
- Start-up is fast enough to start the host on demand and stop it when idle.
- Extension JARs load directly. No dex2jar step was needed.

What it does not settle:
- Size of an extracted host jar. The spike ran against the full 183 MB server jar.
- WebView and JS-engine extensions; none were exercised.
- Windows and macOS. Not run yet.
- Long-running behaviour: memory after hours of use, many concurrent calls.

## Host 0.1 result (2026-10-10) — passed on Linux and Windows

`smoke.py` against the real host over the protocol: install an extension from the store, popular, title, pages, image, search, then 20 extensions, then remove one.

| | Linux (Docker) | Windows (native Java) |
|---|---|---|
| Host up | 98 MB | not measured |
| After one full read | 163 MB | not measured |
| 20 extensions, 262 sources | 178 MB | 188 MB |
| Install (download, verify, load) | 1.0 s | 0.7 s |
| Popular / title / pages / 7 MB image | 0.5 / 1.0 / 0.2 / 0.5 s | 0.5 / 0.9 / 0.3 / 0.7 s |

No errors in the host log on either system.

Still open:
- Docker image is 654 MB against a 400 MB target. The runtime jar is 99 MB; the rest is the stock Java base image. Fix in step 9: trimmed Java runtime (jlink), slim base, and more stripping (ICU 33 MB, fonts 36 MB).
- Filters, per-source preferences, APK fallback: reserved in the protocol, not implemented.
- WebView and JS-engine extensions fail with `needs_optional_module` until those modules exist.
- Only one extension was read end to end. Nineteen more were loaded but not called.
- macOS not run.

## Host 0.1.1 and the supervisor (2026-10-10)

The core's side is `mango-reader/src/host.rs`. It starts the host with a fresh token on a free port and waits for the `MANGO_HOST_READY` line.

- Stop is by closing the host's stdin, not by a kill. The core sets `MANGO_HOST_WATCH_STDIN=1` and holds a pipe; the host exits when the pipe closes. Found on Windows: `java` on the PATH can be a launcher that runs the JVM as its own child, so a kill ends the launcher and leaves the JVM holding the extension files open. The pipe also ends the host when the core dies. A kill follows if the host has not gone after 5 s.
- Idle stop after 5 minutes by default. A ping every 15 s; two missed pings and the host is replaced.
- One crash is invisible to the caller, who gets a slower answer. A host that keeps dying is retried after 0.5, 1, 2 s and so on up to 30 s, and callers get `host_unavailable` meanwhile.
- The HTTP contract did not change. `smoke.py` passes on 0.1.1.

Still open:
- A failed update loses the old version: the host unloads the installed jar before it has loaded the new one.
- The host's `runtime` field reads `unknown` outside its own image.

## Cloudflare solver (2026-10-10, host 0.3.0)

A source that meets a Cloudflare check hands it to a FlareSolverr, or anything that answers like one. Mango does not run one: an admin gives its address under Settings, Sources (`GET | PUT /api/solver`, `{url, timeoutSecs}`; an empty address means none).

- The runtime taken from Suwayomi-Server already knows how: its network stack notices the check, asks the solver, keeps the cookie it gets and tries again. The host only switches that on.
- The core starts the host with `MANGO_HOST_SOLVER_URL` and `MANGO_HOST_SOLVER_TIMEOUT_SECONDS`. A change stops a running host, and the next call starts one that knows. The HTTP contract did not change: still protocol 2.
- Without a solver, such a source fails with the runtime's own message, as before.

Checked on Linux with `smoke.py`, which stands in for both a site that shows the check and the solver: the first call went through the solver once, the next went straight through on the kept cookie. The core's side (the setting is tidied, kept and taken away) is in the core's smoke test on Linux and Windows.

Not checked: a real FlareSolverr against a real Cloudflare check.

## Web view (2026-10-11, host 0.4.0)

About 40 extensions open a web view. The runtime taken from Suwayomi-Server has one, built on a Chromium (JCEF from the JetBrains runtime) that draws without a window. The host keeps the classes that drive it (6 MB more in `runtime.jar`) and switches it on.

- The browser starts with the first web view a source opens, not with the host. The first time, the runtime downloads it into the host's data folder (`host/bin/kcef`, about 550 MB). A call that waits for the download runs into its time limit; the next one finds the browser there.
- It runs for as long as the host does, and stops with it. The host clears the lock a browser leaves behind when it was cut short, or the next one would refuse its profile.
- The core says whether to (`MANGO_HOST_WEBVIEW`), from `host.webview` / `MANGO_WEBVIEW`, on by default. The browser hangs without a display, so on Linux with no `DISPLAY` the core starts the host under `xvfb-run` when there is one, and otherwise leaves the web view off: such a source then answers that it is switched off. The runtime has none for macOS, so there it is off too. Without the web view Java runs headless, as before.
- The host, the browser and a lent display share a process group, so a host that has to be killed takes them along.
- `java -cp mango-host.jar:runtime.jar mango.host.MainKt check-webview` opens a page in a web view, runs a script in it and prints `MANGO_WEBVIEW_OK` or `MANGO_WEBVIEW_FAILED`. It serves nothing and needs no token.
- The HTTP contract did not change: still protocol 2.

Checked with `check-webview`: on Linux in a throwaway container with the libraries and a virtual display, inside the `full` image with its trimmed Java, and on Windows with Java 24. Each downloaded the browser (529 MB on Linux, 572 MB on Windows), loaded a page and got the script's answer back. A second start took 1.2 s on Linux. With the browser up, everything in the container together held about 920 MB, shared memory counted more than once. The whole smoke test of the core passes against the `full` image, where the host runs under `xvfb-run`.

Not checked: a real extension that opens a web view. None was looked for, so what such a source does with the page, its cookies and its script hooks is untried. The `full` image on arm64. The web view under a real desktop session on Linux.

Also in host 0.4.0: an installed extension's file gets a name of its own each time. Replacing the file of an extension that was loaded failed on Windows, where a loaded jar cannot be replaced, so an update there could not work. A file that could not be deleted yet is cleared at the next start.

## Preferences (2026-10-11, host 0.5.0, protocol 3)

`GET /sources/{id}/preferences` lists what a source lets its user set; `PUT` with `{position, key, value}` changes one entry and answers with the list as it is afterwards.

- The host has the source draw its preference screen, as it would on a phone, into the runtime's stand-in for Android's, and reads the entries off it: `switch`, `list`, `multi`, `text`, or `info` for one with nothing to set.
- The screen is drawn anew for each call, so nothing is kept between a read and a change. An entry is named by its place; the key goes along, and a change is refused when another entry is at that place by now.
- As on a phone, the source hears of a change first and may refuse it. Then the value is saved, and the extension's sources are made anew at their next call, because a source reads its preferences when it is made.
- The host keeps one set of preferences per source. Accounts that have the same extension share them.

Checked on Linux with `smoke.py` against a real extension: 12 entries of all four kinds read in 9 ms; a switch changed, the source still answered, the switch went back; a change with another entry's key was refused. Through the core on Linux and Windows by the core's smoke test, and each kind changed by hand in a browser.

Not checked: an extension whose screen changes between calls, and one that refuses a value.

## Install says what was asked for (2026-10-11, host 0.6.0, protocol 4)

`POST /extensions/install` takes `pkg` and `versionCode` beside `url` and `sha256`: the package the core asked the store for and the version the store lists. The host reads both from the file's manifest before it unloads anything, and answers 422 `bad_extension` when the file is another package or an older version. Both fields may be left out, as the host's own smoke test does.

## Filters (2026-10-12, host 0.7.0, protocol 5)

`GET /sources/{id}/filters` lists what a source's search can be narrowed by; `POST /sources/{id}/search` takes `filters`, a list of `{position, state}`.

- The kinds are Mihon's: `header`, `separator`, `select`, `text`, `checkbox`, `tristate` (0 either way, 1 with it, 2 without it), `sort` (`{index, ascending}` or null) and `group`, which holds filters of its own.
- Nothing is kept between the read and the search, as Suwayomi does it now: the source makes its list anew for each search, and the changes are set on that. A filter is named by its place; a group's state is the changes inside it. A place that is not there, or a state of the wrong kind, is refused with 400.
- A search without `filters` carries the source's list as the source makes it, where it carried an empty one. That is what Mihon hands a source, and what extensions are written against.

Checked on Linux with `smoke.py` against a real extension: 12 filters read in 4 ms, one of each kind it has set (checkbox, select, sort, tri-state) and searched by, with another list than without them; a change for a place that is not there was refused. Then one source from each of 19 extensions: 9 list filters, of every kind, and 10 have none. A search by filters answered on all but two, and those two are sources for a server of the reader's own, with no address set.

Not checked: a source whose list changes between the read and the search.

## BouncyCastle ciphers and the JS engine (2026-10-26, host 0.7.1)

Android ships BouncyCastle as a cipher provider, and some sources ask for ciphers only it has: Mangago decrypts its page list with `AES/CBC/ZEROBYTEPADDING`. Nothing registered the provider, so the call failed with "Cannot find any provider supporting …". The host now registers it in `initRuntime`.

Registering is not enough on Oracle's Java: it takes a cipher provider only from a jar signed for that, and the copy inside the runtime jar has lost its signature, so the same error stays. The host is therefore three jars: Bouncy Castle's own signed `bcprov.jar` (from Maven Central at a pinned version and sha256) sits beside the other two, `mango-host.jar` names it in its manifest's `Class-Path`, and the runtime jar's copy is removed. The core still passes two jars. The build stops unless every Bouncy Castle class in the runtime jar is in the signed jar byte for byte, so a runtime upgrade that brings another version says so.

Mangago then runs a script to find each scrambled page's key, so the JS engine is no longer stripped from the runtime jar. It runs interpreted on a stock JVM; its warning about that is switched off. Jars: 105 MB before, 145 MB now (135 MB runtime, 10 MB Bouncy Castle).

Checked, with the released 1.1.14 image failing the same way as the report: on Linux under Temurin, in the image under its own trimmed Java, and on Windows under Oracle Java 24 with the two jars the core passes, Mangago lists, reads chapters, gives a page list (50 pages, 40 of them scrambled) and serves the pages as JPEG, the same bytes everywhere. The tiles come back in order: the jump in pixel values at tile edges is 1.7 times that inside a tile, against 10.7 with the tiles shuffled. Oracle Java 24 refused the unsigned copy and takes the signed jar. The host smoke test and the core's smoke test against the image pass; a build with another Bouncy Castle version stops. Memory is as before for sources that run no script; after a Mangago read the host held 235 to 265 MB. The image is 632 MB.

Not checked: macOS, and arm64, which cannot be run here (its jars are the same files).

## Second class-data archive dropped (2026-10-26)

The trimmed Java carries two class-data archives, the second for heaps over 32 GB. The host's heap is capped at 256 MB, so the image now drops the second one (13 MB): 615 MB, from 632. Checked: Java in the image still starts with sharing on, the host builds its own archive on top, and the core's smoke test against the image passes.

## A title's own address (2026-10-28, host 0.8.0, protocol 6)

`/sources/{id}/manga` answers `webUrl` beside the title and its chapters: the title's page on the source's site, as the source itself makes it, or null when it cannot. The core keeps it in `manga.web_url` (migration 015) and the title page links to it. Before, the link was put together from the source's base address and the title's key, so it was missing where the key is not a path, and wrong where the key is a path other than the page's.

Checked: the smoke test against the real repository passes with the new jars; a MangaDex title got `https://mangadex.org/title/<id>/<slug>`, where the old link pointed at `/manga/<id>`.

## Done when

The core installs an extension from the store and reads a chapter through the host, on Linux and on Windows.
