# 09 — Packaging and login

How Mango is shipped, configured, protected and kept up to date.

## MVP — packaging

- One Rust binary with the UI embedded.
- The extension host (jar and trimmed runtime) in a folder beside it. Mango runs without it, with only the local source.
- Config from a file and environment variables. One data directory: database, extensions, downloads, cache.
- Docker: multi-stage build, non-root user, health check, one volume.
  - Two images. The plain one has no way to give a source a web view. `full` adds the libraries Chromium links and `xvfb-run`; the browser itself is downloaded into the volume on first use.
- Works behind a reverse proxy, including under a sub-path.

## Config as built

`mango.toml` in the working directory, or the file `MANGO_CONFIG` names. Environment wins over the file. Unknown keys are an error.

| Key | Environment | Default |
|---|---|---|
| `listen` | `MANGO_LISTEN` | `127.0.0.1:8787`; the image sets `0.0.0.0:8787` |
| `data_dir` | `MANGO_DATA` | `data`; the image sets `/data` |
| `log` | `MANGO_LOG` | `info` |
| `cache_mb` | `MANGO_CACHE_MB` | `1024`: cap on cached pages and covers |
| `keep_free_mb` | `MANGO_KEEP_FREE_MB` | `0`; above it, no chapter is downloaded while the disk has less free (Linux and macOS) |
| `local_dir` | `MANGO_LOCAL` | `local` in the data directory |
| `login` | `MANGO_LOGIN` | `true`; `off` lets everyone in |
| | `MANGO_PASSWORD` | none. Sets the password on a start that finds none |
| `hosts` | `MANGO_HOSTS` | none. Names Mango goes by, comma separated in the variable; see below |
| `proxied` | `MANGO_PROXIED` | `false`; `on` when a proxy in front writes each caller's address into `X-Forwarded-For` |
| `signup` | `MANGO_SIGNUP` | `false`; `on` shows the main page to whoever is not signed in, and lets the holder of an admin's invitation make an account |
| `account_url` | `MANGO_ACCOUNT_URL` | none. For whoever runs Mango for others: where a reader's plan and billing are kept, an `https://` address or a path on the same site. Shown as a link in Settings, Account and in the tour; see feature 15 |
| `host.dir` | `MANGO_HOST_DIR` | `host` beside the binary |
| `host.java` | `MANGO_JAVA` | `java` |
| `host.idle_secs` | `MANGO_HOST_IDLE_SECS` | `300`; `0` never stops the host |
| `host.webview` | `MANGO_WEBVIEW` | `true`; `off` keeps sources from opening a web view |
| `stores.refresh_hours` | `MANGO_STORES_REFRESH_HOURS` | `12`; `0` turns the check off |
| `stores.local` | `MANGO_STORES_LOCAL` | `false`; `on` lets a store be on this machine or its network, and be fetched over plain HTTP |
| `stores.anyone_installs` | `MANGO_ANYONE_INSTALLS` | `false`; `on` lets every account bring extensions into the host, not only an admin |
| `stores.blocklist` | `MANGO_BLOCKLIST` | none. A file of extensions and stores no account may have; see feature 02 |
| `releases.repo` | `MANGO_RELEASES_REPO` | the repository the build came from, as `owner/name`; none in a build made by hand |
| `releases.token` | `MANGO_RELEASES_TOKEN` | none. Needed when that repository is not public |
| `releases.api` | `MANGO_RELEASES_API` | `https://api.github.com`; another address for a self-hosted forge |
| `releases.check_hours` | `MANGO_RELEASES_CHECK_HOURS` | `24`; `0` looks only when asked |

File only, under `[host]`: `jvm_args`, `start_timeout_secs`, `call_timeout_secs`, `source_timeout_secs`, `per_source`.

The data directory holds `mango.db`, `host/` (extension files, the host's own state), `cache/` (pages and covers, swept to stay under the cap), `covers/` (covers of library titles, kept), `downloads/` (CBZ files) and `local/` (the owner's own files). While Mango runs there is also `instance`, a mark made anew at each start, and until the first password is set `setup-code`. On Linux and macOS the databases, those two files, `host/` and `users/` are closed to every user but the one Mango runs as; the rest keeps the permissions the system gives.

Commands: `mango` runs the server, `mango health` asks a running one whether it answers and has it show that it is the Mango of this data folder, `mango address` prints where a browser on the same machine finds it, `mango reset-password` removes the first account's password, `mango version` prints the version.

## MVP — login

- Passwords hashed with Argon2, session cookie.
- Login can be switched off for a trusted network; off is never the default.
- The host's port is loopback-only and token-protected; it is never published by the container.

## Login as built

- A new Mango has its password set on the first visit (8 characters or more), unless `MANGO_PASSWORD` set it at the start. Until one exists, the API answers only the call that sets it.
- From another machine, setting it takes the setup code (2026-10-11): sixteen characters Mango writes to its log at the start and into `setup-code` in the data folder (`docker exec mango cat /data/setup-code`). So the first to reach a new Mango on the network is not its owner by that alone. A browser on Mango's own machine, with no proxy on the way, is not asked: that covers the desktop shell. A wrong code is answered 403 `setup_code`. Checked in a browser against a published port: the page asks for the code, shows the refusal for a wrong one and signs in with the right one.
- The password is stored as an Argon2id hash in a table the settings API cannot reach.
- Signing in gives the browser a random token in a cookie that scripts cannot read and other sites do not send (`HttpOnly`, `SameSite=Lax`, and `Secure` when a proxy says the request came over HTTPS). The database keeps only the token's hash. A session ends 30 days after it was last used.
- Five wrong passwords from one address, then that address waits: 1 second, doubling to 5 minutes. An IPv6 address counts as its /64, which one holder has whole. Behind a reverse proxy every visitor shares the proxy's address, and so the wait, unless `MANGO_PROXIED=on`: then the last address in `X-Forwarded-For` counts. Switch it on only where every request comes through the proxy, since anyone else could write that header.
- Answers of the API carry `Cache-Control: no-store` unless they say otherwise themselves, as pictures do.
- Text of a kilobyte or more (the page's script and styles, JSON, the OPDS feeds) goes out gzipped to a caller that says it reads gzip (2026-10-12). An answer marked `no-cache` (the page, the manifest and icons, a cover asked for without its version, an OPDS feed) carries an `ETag` made from its content, and a caller that shows the same one is answered 304 with nothing after it. Both are in `api.rs`, around every route. Checked on Linux and Windows by `smoke.py`: the page's script went out as 77 KB instead of 226 KB and unpacked to the same bytes, and the page asked for again with its mark was answered 304. Not checked: behind a reverse proxy, and a reading app that sends the mark back.
- A session cookie is for a host name, not a port: another web service under the same name is sent it. Give Mango a name of its own where that matters.
- Changing the password signs every other browser of that account out. `mango reset-password`, then a restart, opens the first-visit step again; other accounts are left as they are.
- Everything under `/api` needs a session except `/api/auth`: `GET` for who is asking, `POST /setup`, `/login`, `/logout`, `PUT /password`. The page and its files are served to anyone, since they are what shows the sign-in.

## Requests from other sites (2026-10-11)

A page on another site can make the owner's browser send requests to Mango. Two checks stand in the way.

- A change (anything but `GET`) sent by another site's page is answered 403 `cross_site`. Browsers label such requests (`Sec-Fetch-Site`), but only where the address is HTTPS or the machine's own. Without the label, the `Origin` the request carries has to be the `Host` it was sent to, or the `X-Forwarded-Host` a proxy put beside it. A request with neither, as a reading app or a script sends, is let through: it is not a page.
- While a request can act without signing in (login switched off, or no password set yet), Mango answers only under names such a page cannot point at it: an address, `localhost`, a name without a dot, a name ending in `.local`, `.lan`, `.internal`, `.home.arpa` or `.localhost`, and the names in `hosts`. Anything else is answered 403 `unknown_host`, the page included. Once a password is set and login is on, every name is answered: the session cookie is not sent under a name of someone else's.
- So a Mango behind a proxy under a name of its own needs that name in `MANGO_HOSTS` only with login off, or for the first-visit step; `MANGO_PASSWORD` spares it the step. The proxy has to pass the browser's `Host` on, or refuse names it does not know.

Checked on Linux, in a container with no network: before the change, a `POST` with another site's `Origin` and no label reached the import and the update run, with login off and with a session from another port of the same host; a request under a made-up host listed and added stores with login off, and set the first password. After it, each is refused, and the same calls under the machine's address, `localhost`, a one-word name, and with no `Origin` at all go through. The smoke test has the unlabelled request.

## Accounts (2026-10-10)

- The first account is the owner's: an admin that cannot be removed. Its name is `owner`. An admin adds accounts, sets their passwords, makes them admins or readers, and removes them: `GET | POST /api/users`, `PUT | DELETE /api/users/{id}`.
- While there is one account, signing in takes the password alone, as before. With more it takes a name too; names ignore case.
- Each account has everything of its own: its stores and their catalogs, its installed extensions and pinned sources, titles, chapters and progress, categories, history, downloads, cache, tracker sign-ins and settings. The owner's is the data directory, as it was. Every other account has `users/<id>/` in it, with a `mango.db`, `cache/`, `covers/` and `downloads/` of its own, and its own download queue and update schedule.
- Only the extension host is shared, and it holds one copy of each extension. The cookies sources keep live in the host, so they are shared too. An extension two accounts installed stays in the host until both removed it. When one account installs another version, the other account has that version too. What a store offers is checked against that store's key in the account that installs.
- The accounts themselves and the solver are for an admin; a reader is answered 403 `admin_only`.
- Removing an account deletes its folder, and takes its extensions out of the host where no other account has them. A database from before accounts is moved on by itself: its password becomes the owner's.
- An account's number is never given to another (2026-10-11). The number names the folder, and the folder goes a moment after the account, or at the next start when something still holds it. Before, an account added right after the newest one was removed got that number, and with it the removed account's library and tracker sign-ins. A new account whose folder is already there is refused.
- With login switched off, every request is the owner's.
- A title that two accounts read is fetched and stored once for each. That is the price of keeping accounts apart without touching every query.
- Extensions are code that runs in the one host, as the user Mango runs as. Bringing one in is for an admin (2026-10-11): a reader adds stores and installs what the host already holds at that version, which an admin put there; an extension or a version no account has yet is answered 403 `admin_only`. A source's settings are kept once for all accounts and may hold a sign-in, so a reader gets an empty list and cannot change them. `MANGO_ANYONE_INSTALLS=on` gives every account what an admin has here; then give accounts only to people you would let run code on the server.
- The host is not a wall between an admin and the server: whoever may bring in an extension can read every account's library through it.

An admin does not see into another account's library, and nothing shows one account what another reads.

## Main page and invitations (2026-10-11)

Closed unless the owner opens it: `MANGO_SIGNUP=on`. Without it nothing here exists, and a browser that is not signed in sees the sign-in form alone, as before.

- With it, that browser sees the main page: what Mango does, and beside it Sign in and Create account. Either one ends in the library.
- Making an account takes an invitation. An admin makes one under Settings, Account, with a note on who it is for: `GET | POST /api/invites`, `DELETE /api/invites/{id}`. The answer that makes it carries its code, sixteen characters, and a link that opens the main page with the code filled in. The code sits after the `#` of that link, which a browser sends to no server. Mango keeps the code's hash only, so it is shown once.
- `POST /api/auth/signup` takes a name, a password and the code. It makes a reader, never an admin, with an empty library, and signs the caller in. So what an invited person can bring into the host is what an admin already put there.
- A code opens once and holds a week (`days` on the call that makes it: 1 to 90). Spending it and making the account are one step in the database, so two callers with one code make one account. A name that is taken or a password too short leaves it open.
- A code that never was, one that was used and one that ran out are answered alike: 403 `invite_code`. Each counts against its sender as a wrong password does, and the same wait follows the fifth.
- Taking an invitation back leaves the account that was made with it. Invitations that ran out unused are cleared at the next start.
- Until the owner has a password there is no main page and no sign-up: the first visit stays what it was.

The main page is for a Mango that people are invited to. Every account still shares the one extension host and the cookies sources keep there, and has no limit on what it downloads: invite people you know.

Checked on Linux, in a container behind a published port:

- With the switch off, the smoke test (173 checks) passes: sign-up is answered 403 `signup_off`, and so is making an invitation.
- With it on, by script, 29 checks: nothing before the owner's password; an invitation whose code is in no answer but the first and not in the database; a short password, a taken name, a wrong code and a request from another site's page each refused, the first two leaving the code open; the code typed in capitals with spaces making a reader with an empty library in `users/2`; that reader refused the invitations and the accounts; the code refused a second time; one taken back refused; the sixth wrong try answered 429 and the right code working after the wait.
- In a browser: the main page at 1440 pixels wide, light and dark, and at 400 with no sideways scroll; a wrong password refused on the page; the owner signing in there and making an invitation; its link switching an open main page to Create account with the code filled in; two passwords that differ caught; the account made and landing in the library; that account's settings without Accounts and Invitations; the same link refused afterwards.
- 75 unit tests, one of them this.

Not checked: Windows and macOS were not run; nothing here is particular to a system. No proxy stood in front, so the per-caller wait behind one rests on the test of `caller`. Two callers spending one code at the same moment were not raced; that rests on the single statement that spends it.

Checked by the smoke test on Linux and Windows, with two accounts on one store and one extension:

- A second account starts with no store, no extension and an empty library. It adds the store, installs the extension the first account has and browses with it. Both browse the same source at the same moment, and neither's titles show up in the other's library.
- An account whose store names another signing key is refused the extension, though the host has it.
- When both accounts' lists say an older version than the host has, one account's update takes the host's version, the other account's list shows it too, and both go on working. A list that fell behind is put right when the host next starts.
- A reader is refused an extension no account has, gets no source settings and cannot change one; an account made an admin installs it (2026-10-11).
- The second account removes the extension and the store, and the first account's still work.
- An account that is removed with two extensions installed takes the one only it had out of the host and leaves the one the first account has.

The older version in these checks is a pretended one: the test changes version numbers in the databases. Two accounts on two really different versions were not tried, and neither was an update that replaces the file in the host.

## Desktop shell (2026-10-10)

`mango-desktop/` is a small program (about 1 MB) beside the binary: a window with the system's own web view, and a tray icon.

- It asks `mango address` where Mango is, shows a Mango that already answers, and otherwise starts `mango` and waits for `mango health`.
- Closing the window leaves Mango running behind the tray icon. The icon's menu opens the window, opens the system's browser, or quits.
- The shell holds a pipe to Mango's input (`MANGO_WATCH_STDIN=1`); Mango stops when it closes, so it stops on Quit and when the shell dies.
- Data goes where the system keeps an app's data (`%APPDATA%\Mango`, `~/Library/Application Support/Mango`, `~/.local/share/mango`), with the web view's own data and `mango.log` beside it, unless `MANGO_DATA`, `MANGO_CONFIG` or a `mango.toml` beside the program says otherwise.
- Login stays as configured: a desktop Mango asks for a password too.
- On Linux it links GTK 3 and WebKitGTK 4.1 and is built for x86_64 only (`--target shell` in the `Dockerfile`). A release archive carries it for Linux x86_64, macOS and Windows.

Checked: on Windows it opened the first-visit page in its window, started the core as its child, kept both alive when the window was closed, and the core stopped within seconds of the shell being ended. On Linux, under a virtual display, it started the core, and the core stopped with it.

Not checked: the tray icon's menu was never clicked, on any system. macOS was not built or run. The release workflow's new steps have not run.
- Under a sub-path nothing changes: the UI uses relative addresses, and the proxy strips the prefix.

The shell opens what `mango health` vouches for. Since 2026-10-11 that is more than a port that answers: the server writes a mark into its data folder at each start, `mango health` sends a random challenge, and only an answer made with that mark counts. Something else that took the port first, as another user of a shared machine could, is reported as not being this Mango. The container's health check goes the same way. Checked on Windows: the shell started its core and showed the first-visit page; with another program answering on the port it showed "Mango could not start" and not that program's page.

## Versions and releases

- A release's version is its tag. The build is told it (`MANGO_VERSION`, with `MANGO_COMMIT` and `MANGO_REPO`); any other build goes by `Cargo.toml` and does not know where releases are listed.
- Mango asks the repository's release list once a day and keeps the answer: `GET /api/version`, `POST /api/version/check`. Settings shows the running version, the newest release, its notes, and the file for this platform or the Docker command. A mark on the settings button says a newer one is out. A pre-release is offered only to someone already on one.
- Mango does not replace itself.
- `.github/workflows/release.yml` makes a release: run it from the Actions tab (it works out the next version, or takes one, and tags the tip of the main branch) or push a `v1.2.3` tag. It builds `mango-<version>-<os>-<arch>` archives for Linux (x86_64, aarch64), macOS (aarch64, x86_64) and Windows (x86_64), each with the host's jars, `LICENSE`, `NOTICE` and `THIRD-PARTY-LICENSES.txt` beside the binary, and pushes an amd64 and arm64 image to `ghcr.io` as `<version>`, `<major.minor>` and `latest`.
- The release's Linux job runs on GitHub's machine unless the repository variable `RELEASE_RUNNER` names another (`self-hosted`): a machine of the owner's keeps the build cache between releases. The job takes a builder that is already there, tries the image under a name and a port of the run's own, and removes both afterwards.
  - The variable alone is not what makes this safe. For a public repository to reach a runner, its runner group has to allow public repositories, and then every workflow in the repository reaches it. A pull request is run with the workflow files of its own branch, so a stranger's pull request can ask for that runner whatever `ci.yml` says here, and run its code on the owner's machine, which also builds the next release.
  - So, before the variable is set, one of two things: the runner group's workflow access is limited to `release.yml` on the main branch (then releases are started from the Actions tab; a tag pushed by hand does not match), or the repository asks for approval before any workflow of an outside contributor's pull request runs, and nobody approves one that touches `.github/workflows` unread. Without either, leave the variable unset: GitHub's machines cost a public repository nothing but time.
- `.github/workflows/ci.yml` checks and builds the UI and runs the core's tests on every push to the main branch and every pull request.

## Docker image as built

One `Dockerfile` at the repository root builds everything: the host's jars, the UI, the core (cross-compiled for arm64), a Java runtime linked with only the modules the host uses, on `debian:bookworm-slim`. It runs as uid 10001, writes only to the `/data` volume, reads `mango.toml` from there, and reports its health through `mango health`. `docker-compose.yml` beside it builds and runs it.

The `full` image (`--target full`, published as `<version>-full` and `full`) is the same with one more layer: the libraries Chromium links and `xvfb-run`, 252 MB. That makes about 610 MB against about 360 MB for the plain one. The browser is not in either; it goes into the volume when a source first opens a web view.

## Licence and notices (2026-10-11)

- Mango's own code is under the Apache License 2.0 (`LICENSE`). `NOTICE` names what it ships of other people's.
- The `Dockerfile`'s `notices` stage runs `cargo about` over the manifests of the server and the desktop shell and adds the licences of Svelte and the two fonts from the UI's packages: one text file, `THIRD-PARTY-LICENSES.txt`. `mango-reader/about.toml` lists the licences a crate may come under; a crate under any other fails the build, so a new dependency's licence gets looked at.
- A release archive carries `LICENSE`, `NOTICE` and that file beside the binary. The image has them in `/usr/share/doc/mango`.
- `.dockerignore` names what the build reads and leaves out everything else, instead of listing what to leave out.
- The smoke tests no longer name a store or an extension: they take them from `MANGO_SMOKE_STORE`, `MANGO_SMOKE_STORE_LINK` and `MANGO_SMOKE_PKG`, and say so when one is missing.
- The pictures in the README show a library of made-up titles with drawn covers, read from the local source.

Checked on Linux: the image built with the new `.dockerignore`; the three files are in it, and `--target dist` gives the same licence file (764 KB: 557 crate entries under seven licences, then Svelte and the fonts). The core's 74 unit tests pass, its smoke test passed against that image (171 checks) and the host's against the same jars. `actionlint` passes on both workflows.

Checked with release 1.0.3: the release workflow's changed steps ran, on GitHub's machines. The Windows archive holds the three files beside the two programs and `host/`, its checksum matches `SHA256SUMS`, and every tag of the image pulls without a login; the pulled image has the three files too.

Not checked: the other four archives were not opened. The release's Linux job has not run on a machine of the owner's yet. Windows was not run: nothing a program does changed. What `runtime.jar` carries is not in the licence file.

## Build rules

- Builds are incremental; the cache is cleared occasionally and only for this project. Release and measured builds are always clean.
- Tested on Linux in Docker first, then on Windows, then macOS.
- What a build downloads is pinned (2026-10-11): the Suwayomi jar and the Kotlin compiler by sha256 in the `Dockerfile`, `cargo-about` by version, the workflows' actions by commit, the emulation image by digest. A checkout leaves no token behind. Base images go by tag, so that a rebuild picks up their fixes.

## Done when

`docker compose up` on a clean machine gives a working, password-protected Mango that meets the size and memory targets below.

## Result (2026-10-10)

`docker compose up -d` from a checkout gave a healthy container in 3 seconds that asked for a password and answered 401 to everything else.

| | Target | Measured, Linux x86_64 |
|---|---|---|
| Image, unpacked | under 400 MB | 359 MB (179 MB to pull): Java runtime 156, host jars 99, base 85, binary 11 |
| Core memory, host asleep | under 100 MB | 10 MB after start, 22 MB after the whole smoke test |
| Host memory, awake | under 400 MB | 181 MB with 61 sources loaded |

- The smoke test (`mango-reader/smoke.py`, now 90 checks with login and the version check) passes against the image on Linux and against the binary on Windows with Java 24. 46 unit tests.
- In a browser: first-visit password, sign-in, a wrong password, changing the password, signing out, the version panel and the mark on the settings button.
- By hand: `reset-password`, `MANGO_LOGIN=off`, `MANGO_PASSWORD`, and the sixth wrong password answering 429.
- The version check ran against a stand-in for the release list: a newer release, a pre-release, a draft and the file for the platform were each handled as written above.
- Found on the way: glibc kept every 19 MB block a password hash had used, one per thread, and the core sat at 190 MB after seven sign-ins. Mango now tells it to hand big blocks back.

Not checked:
- The macOS and Windows archives of the release were built, not run: Windows was run with a GNU-toolchain binary, the release builds with MSVC.
- The arm64 image was built, not run, and the pushed image was not pulled back.
- The version check against the real release list, which needs `MANGO_RELEASES_TOKEN` while the repository is not public.
- A reverse proxy and a sub-path were not set up; only the `Secure` cookie rule has a test.

## Release 1.0.0 (2026-10-10)

Both workflows ran on GitHub. The release workflow, from the tag `v1.0.0`, made the five archives with `SHA256SUMS`, tried the amd64 image, and pushed the image for both platforms.

Its first run failed: amd64 and arm64 are built side by side, and both unpacked crates into one shared cargo cache. Each platform now has its own.
