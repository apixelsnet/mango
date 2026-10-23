<a id="readme-top"></a>

<!-- PROJECT SHIELDS -->
<div align="center">

[![Release][release-shield]][releases-url]
[![CI][ci-shield]][ci-url]
[![Stargazers][stars-shield]][stars-url]
[![Forks][forks-shield]][forks-url]
[![Issues][issues-shield]][issues-url]
[![Contributors][contributors-shield]][contributors-url]
[![License][license-shield]][license-url]

[![Platforms][platform-shield]][releases-url]
[![Docker][docker-shield]][packages-url]
[![Extensions][extensions-shield]](#extensions)
[![Ko-fi][kofi-shield]][kofi-url]

</div>

<!-- PROJECT TITLE -->
<br />
<div align="center">
  <a href="https://github.com/apixelsnet/mango">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="docs/images/logo-dark.svg" />
      <img src="docs/images/logo.svg" alt="Mango logo" width="96" height="96" />
    </picture>
  </a>

  <h1 align="center">Mango</h1>

  <p align="center">
    A self-hosted manga and manhwa reader that runs Mihon-compatible extensions and stays light.
    <br />
    <a href="docs/features"><strong>Explore the docs »</strong></a>
    <br />
    <br />
    <a href="https://mangoreader.org"><strong>Live Demo</strong></a>
    &middot;
    <a href="https://github.com/apixelsnet/mango/releases">Releases</a>
    &middot;
    <a href="https://github.com/apixelsnet/mango/issues/new?labels=bug">Report Bug</a>
    &middot;
    <a href="https://github.com/apixelsnet/mango/issues/new?labels=enhancement">Request Feature</a>
    &middot;
    <a href="https://ko-fi.com/pixelsthecoder">Support on Ko-fi</a>
  </p>
</div>

<!-- TABLE OF CONTENTS -->
<details>
  <summary>Table of Contents</summary>
  <ol>
    <li>
      <a href="#about-the-project">About The Project</a>
      <ul>
        <li><a href="#features">Features</a></li>
        <li><a href="#screenshots">Screenshots</a></li>
        <li><a href="#built-with">Built With</a></li>
      </ul>
    </li>
    <li>
      <a href="#getting-started">Getting Started</a>
      <ul>
        <li><a href="#prerequisites">Prerequisites</a></li>
        <li><a href="#installation">Installation</a></li>
      </ul>
    </li>
    <li>
      <a href="#usage">Usage</a>
      <ul>
        <li><a href="#passwords-and-accounts">Passwords and accounts</a></li>
        <li><a href="#extensions">Extensions</a></li>
        <li><a href="#your-own-files">Your own files</a></li>
        <li><a href="#reading-apps">Reading apps</a></li>
        <li><a href="#settings">Settings</a></li>
        <li><a href="#new-versions">New versions</a></li>
      </ul>
    </li>
    <li>
      <a href="#contributing">Contributing</a>
      <ul>
        <li><a href="#building">Building</a></li>
        <li><a href="#releasing">Releasing</a></li>
      </ul>
    </li>
    <li><a href="#support">Support</a></li>
    <li><a href="#disclaimer">Disclaimer</a></li>
    <li><a href="#license">License</a></li>
    <li><a href="#contact">Contact</a></li>
    <li><a href="#acknowledgments">Acknowledgments</a></li>
  </ol>
</details>

<!-- ABOUT THE PROJECT -->
## About The Project

[![The library][library-screenshot]][library-screenshot]

Mango reads manga and manhwa from the sources Mihon reads them from, and from
your own files. It runs extensions made for Mihon unchanged, so a source that
works in Mihon works here.

It is built to stay small. The server is one Rust binary with the web UI inside
it. Extensions need Java, so they run in a separate process that starts when an
extension is used and stops when it has been idle for a while.

| | Measured, Linux x86_64 |
|---|---|
| Binary | 13 MB |
| Docker image, unpacked | 368 MB (185 MB to pull) |
| Memory, extension host asleep | 10 to 22 MB |
| Memory, extension host awake | about 180 MB |

Each feature has a short description under [docs/features](docs/features): what
it does, how it was built, and what was and was not tried.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

### Features

* **Extensions.** Add a store by its address, search the catalog, install,
  update and remove. Every download is checked against the store's signing key.
* **Browse and search.** Popular and latest lists per source, and one search
  across all of them.
* **Reader.** Paged left-to-right and right-to-left, vertical, and webtoon long
  strip. Keyboard, touch and wheel. It resumes at the page you left.
* **Library and history.** Categories, unread counts, and what you read last.
  A title's chapters can be sorted, filtered and bookmarked.
* **Updates and downloads.** New chapters are found on a schedule. Chapters save
  as CBZ and read with no network, and leave again as one zip that Mihon,
  Suwayomi or Mango reads as a local source.
* **Local source.** A folder of CBZ files or image folders, read without Java.
* **Import and export.** A Mihon or Suwayomi backup (`.tachibk`) brings your
  library, read state and history across, and Mango writes one they read.
* **Trackers.** AniList, MyAnimeList, Kitsu and MangaUpdates follow along as you
  read.
* **Migration.** Move a title to another source and keep your progress,
  categories and tracker links.
* **Themes.** Day by default, or Night, or whatever the system says. Six
  presets, six accent colours, or your own colours.
* **Phones.** Tabs along the bottom on a narrow screen, and it can be added to
  a home screen as an app.
* **Tour.** A new account is walked through the app once, and the reader shows
  where a tap goes.
* **Accounts.** A password is asked for by default. More accounts can be added,
  each with stores, extensions, a library, reading progress, downloads and
  settings of its own.
* **Reading apps.** The library is an OPDS catalog: apps that read one browse it
  and fetch chapters whole or page by page.
* **Cloudflare.** Sources behind a Cloudflare check hand it to a FlareSolverr
  you point Mango at.
* **Web view.** Sources that open a browser of their own get one: a Chromium
  that is downloaded the first time such a source is used.
* **Desktop.** A small program shows Mango in a window of its own, with a tray
  icon.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

### Screenshots

The titles and covers in these pictures are made up for them.

| | |
|---|---|
| [![A title and its chapters][title-screenshot]][title-screenshot] | [![New chapters, by day][updates-screenshot]][updates-screenshot] |
| A title and its chapters | New chapters, by day |

Themes and colours, under Settings:

[![Themes and colours][settings-screenshot]][settings-screenshot]

The same library in the Midnight theme:

[![The library in the Midnight theme][library-dark-screenshot]][library-dark-screenshot]

<p align="right">(<a href="#readme-top">back to top</a>)</p>

### Built With

* [![Rust][rust-shield]][rust-url]
* [![SQLite][sqlite-shield]][sqlite-url]
* [![Svelte][svelte-shield]][svelte-url]
* [![Vite][vite-shield]][vite-url]
* [![Kotlin][kotlin-shield]][kotlin-url]
* [![Docker][docker-built-shield]][docker-url]

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- GETTING STARTED -->
## Getting Started

Want to explore Mango before installing it? A hosted version is available at
[mangoreader.org](https://mangoreader.org).

Docker is the shortest way. Mango also runs as a plain program on Linux, macOS
and Windows.

### Prerequisites

* With Docker: Docker with Compose. Nothing else.
* Without Docker: Java 21 or newer on the `PATH`, for extensions. Without Java,
  Mango still reads the local source.
  ```sh
  java -version
  ```

### Installation

#### Docker

1. Clone the repository
   ```sh
   git clone https://github.com/apixelsnet/mango.git
   cd mango
   ```
2. Build and start it
   ```sh
   docker compose up -d
   ```
3. Open `http://localhost:8787` and set a password.

Everything Mango writes lives in the `mango-data` volume. To use a published
image instead of building one:

```sh
docker run -d --name mango -p 8787:8787 -v mango-data:/data ghcr.io/apixelsnet/mango
```

There are two images. The plain one is small and cannot give a source a web
view. `ghcr.io/apixelsnet/mango:full` can: it carries the libraries a browser
links. To build that one from a checkout, uncomment `target: full` in
`docker-compose.yml`.

#### Without Docker

1. Take the archive for your system from the [releases page][releases-url] and
   unpack it.
2. Start `mango` (`mango.exe` on Windows).
3. Open `http://127.0.0.1:8787` and set a password.

Mango keeps its data in `data/` beside where it was started.

Or start `mango-desktop` from the same folder: it runs `mango` for you and shows
it in a window. Closing the window leaves Mango running behind a tray icon, and
Quit in the icon's menu stops it. Started this way, the data goes where your
system keeps an app's data (`%APPDATA%\Mango`, `~/Library/Application
Support/Mango`, `~/.local/share/mango`), unless a `mango.toml` beside the program
or `MANGO_DATA` says otherwise. On Linux it needs WebKitGTK 4.1, and it comes for
x86_64 only.

* To reach it from other devices, set `MANGO_LISTEN=0.0.0.0:8787`.
* macOS holds back files that came through a browser:
  ```sh
  xattr -dr com.apple.quarantine <the unpacked folder>
  ```

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- USAGE EXAMPLES -->
## Usage

### Passwords and accounts

A new Mango has its password set on the first visit. From another machine that
takes the setup code as well, which Mango writes to its log when it starts and
into `setup-code` in its data folder (`docker compose exec mango cat
/data/setup-code`); on Mango's own machine it does not. `MANGO_PASSWORD` sets
the password without the visit. Change it under Settings. If it is lost:

```sh
mango reset-password                        # then restart Mango
docker compose exec mango mango reset-password && docker compose restart mango
```

That first account is an admin. Under Settings, Account, an admin adds more
accounts and sets their passwords. Each account has its own stores and
extensions, library, categories, reading progress, history, downloads, tracker
sign-ins and settings: a new account starts empty and adds its own stores. With
more than one account, signing in asks for a name too; the first account's is
`owner`. Removing an account deletes everything it had.

With `MANGO_SIGNUP=on`, a browser that is not signed in sees a main page that
says what Mango does, with Sign in and Create account beside it. Making an
account there takes an invitation: an admin makes one under Settings, Account,
and sends its link. Each invitation makes one account, a reader's, and holds a
week.

Extensions are code that runs on the server, so bringing one in is for an
admin. Other accounts add stores and install what an admin already has;
`MANGO_ANYONE_INSTALLS=on` lets them install anything; then give accounts only
to people you would let run code on the server.

### Extensions

Open Extensions, add a store by the link to its `index.pb` or its `repo.json`, then search and
install. The store's signing key is remembered when the store is added; if it
ever changes, Mango refuses to install from that store until you accept the new
key. Installed sources show up under Browse.

A few sources open a web view to read a page the way a browser does. For
those the extension host downloads a Chromium, once, about 550 MB, into the data
folder, and runs it without a window while such a source is in use. That works on
Windows as it is. On Linux it needs a display, or `xvfb-run` to lend one, and the
libraries Chromium links: the `full` image has them, the plain image does not,
and there such a source answers that the web view is switched off. There is none
for macOS. `MANGO_WEBVIEW=off` turns it off anywhere.

Some sources sit behind a Cloudflare check that only a browser passes. Run a
[FlareSolverr](https://github.com/FlareSolverr/FlareSolverr), or anything that
answers like one, and give Mango its address under Settings, Sources. Mango does
not run one itself.

### Your own files

Put them in `local` inside the data folder, one folder per title, one `.cbz`,
`.zip` or folder of images per chapter:

```
local/
  Some Title/
    cover.jpg
    Chapter 1.cbz
    Chapter 2/
      001.png
      002.png
```

In Docker, mount your folder there by uncommenting the line in
`docker-compose.yml`:

```yaml
volumes:
  - mango-data:/data
  - /path/to/manga:/data/local:ro
```

### Reading apps

Mango's address with `/opds` at the end is an OPDS 1.2 catalog, for example
`http://localhost:8787/opds`. An app signs in with an account's name and
password and sees that account's library: its categories, its titles, their
chapters. A chapter comes as a CBZ, downloaded or not, and page by page once
Mango knows how many pages it has.

### Settings

A `mango.toml` in the working directory, or `MANGO_*` variables; the variables
win. The full list is in
[docs/features/09-packaging-auth.md](docs/features/09-packaging-auth.md).

| Variable | What it does |
|---|---|
| `MANGO_LISTEN` | Address to serve on. `127.0.0.1:8787`; the image uses `0.0.0.0:8787`. |
| `MANGO_DATA` | The one directory Mango writes to. |
| `MANGO_PASSWORD` | Sets the password on a start that finds none. |
| `MANGO_LOGIN` | `off` lets everyone in. Only for a network you trust. |
| `MANGO_HOSTS` | Names Mango goes by, such as `mango.example.org`. Needed only with login off, or to set the first password, when it is opened by a name with a dot in it. |
| `MANGO_PROXIED` | `on` when a reverse proxy stands in front and writes each visitor's address into `X-Forwarded-For`. Wrong passwords are then counted per visitor, not for the proxy. |
| `MANGO_STORES_LOCAL` | `on` lets an extension store be on this machine or its network, and be fetched over plain HTTP. Otherwise stores are fetched over HTTPS from public addresses. |
| `MANGO_ANYONE_INSTALLS` | `on` lets every account bring extensions into Mango. Otherwise only an admin does, and other accounts install what an admin already has. |
| `MANGO_SIGNUP` | `on` shows a main page with what Mango does to whoever is not signed in, and lets the holder of an admin's invitation make their own account there. Otherwise only an admin adds accounts. |
| `MANGO_WEBVIEW` | `off` keeps sources from opening a web view, and the browser from being downloaded. |
| `MANGO_RELEASES_TOKEN` | A token that may read the repository, when its releases are not public. |

### New versions

Settings shows the running version and looks once a day for a newer release.
Mango does not replace itself: in Docker, pull the new image and start the
container again; otherwise unpack the new archive over the old files. The data
folder is left alone either way.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- CONTRIBUTING -->
## Contributing

If you have a suggestion that would make Mango better, fork the repository and
open a pull request, or open an issue with the tag "enhancement".

1. Fork the Project
2. Create your Feature Branch (`git checkout -b feature/AmazingFeature`)
3. Commit your Changes (`git commit -m 'Add some AmazingFeature'`)
4. Push to the Branch (`git push origin feature/AmazingFeature`)
5. Open a Pull Request

Every pull request is checked: `npm run check` and `npm run build` for the UI,
`cargo test` for the core, `cargo check` for the desktop shell.

What you send in goes under the same [licence](LICENSE) as the rest. A security
problem does not go in an issue: [SECURITY.md](SECURITY.md) says where it goes.

### Building

`docker build -t mango .` builds everything. By hand:

1. Build the UI
   ```sh
   cd mango-reader/ui
   npm ci
   npm run build
   ```
2. Build the server
   ```sh
   cd mango-reader
   cargo build --release
   ```
3. Put the extension host's jars in a `host` folder beside the binary
   ```sh
   docker buildx build --target jars -o host .
   ```
4. For the desktop shell, build `mango-desktop` and put it beside the binary too
   ```sh
   cd mango-desktop
   cargo build --release
   ```

### Releasing

Run the **Release** workflow from the Actions tab and pick how far to bump the
version; it tags the tip of the main branch, builds that tag for Linux, macOS and
Windows, pushes the image and publishes the release. Pushing a `v1.2.3` tag by
hand does the same.

### Top contributors

<a href="https://github.com/apixelsnet/mango/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=apixelsnet/mango" alt="contrib.rocks image" />
</a>

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- SUPPORT -->
## Support

Mango is free and stays free. If it is of use to you and you would like to say
thanks, there is a Ko-fi page:

[![Support Mango on Ko-fi][kofi-shield]][kofi-url]

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- DISCLAIMER -->
## Disclaimer

Mango is a reader. It comes with no sources, no extensions and no manga, and
this repository holds none.

* Extensions, and the stores that list them, are other people's work. Mango does
  not make, host, choose or check them, and has no say in what they reach.
* What you read through Mango, and whether you may where you live, is yours to
  answer for. Read from places that have the right to offer what they offer, and
  support the people who make what you read.
* Mango is not affiliated with Mihon, Suwayomi, any extension store, AniList,
  MyAnimeList, Kitsu, MangaUpdates, FlareSolverr, Cloudflare or any source, and
  none of them endorses it. Their names belong to their owners.
* Mango comes as it is, with no warranty of any kind. The [licence](LICENSE)
  says so in full.

**If you hold the rights to something a source shows:** Mango does not host it,
keeps no copy of it anywhere but on the machine of whoever runs it, and cannot
take it down. That is a matter for the site it comes from. If something in this
repository itself is yours and should not be here, open an
[issue][issues-url] and it will be looked at.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- LICENSE -->
## License

Mango is under the Apache License 2.0: see [LICENSE](LICENSE).

It ships with other people's work under their own licences. [NOTICE](NOTICE)
says what, and a release carries the full texts in `THIRD-PARTY-LICENSES.txt`.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- CONTACT -->
## Contact

Questions, bugs and ideas go to the [issue tracker][issues-url].

Project Link: [https://github.com/apixelsnet/mango](https://github.com/apixelsnet/mango)

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- ACKNOWLEDGMENTS -->
## Acknowledgments

* [Suwayomi-Server](https://github.com/Suwayomi/Suwayomi-Server), whose release
  jar the extension host is built on
* [Mihon](https://github.com/mihonapp/mihon), for the extension API and the
  backup format
* [Dela Gothic One](https://fonts.google.com/specimen/Dela+Gothic+One) and
  [Zen Kaku Gothic New](https://fonts.google.com/specimen/Zen+Kaku+Gothic+New),
  the fonts in the web UI
* [Best-README-Template](https://github.com/othneildrew/Best-README-Template)
* [Shields.io](https://shields.io)

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- MARKDOWN LINKS & IMAGES -->
[platform-shield]: https://img.shields.io/badge/platform-Linux%20%7C%20macOS%20%7C%20Windows-111111?style=for-the-badge
[docker-shield]: https://img.shields.io/badge/docker-amd64%20%7C%20arm64-2496ED?style=for-the-badge&logo=docker&logoColor=white
[extensions-shield]: https://img.shields.io/badge/extensions-Mihon%20compatible-FFC400?style=for-the-badge
[release-shield]: https://img.shields.io/github/v/release/apixelsnet/mango?style=for-the-badge
[releases-url]: https://github.com/apixelsnet/mango/releases
[ci-shield]: https://img.shields.io/github/actions/workflow/status/apixelsnet/mango/ci.yml?branch=main&style=for-the-badge&label=CI
[ci-url]: https://github.com/apixelsnet/mango/actions/workflows/ci.yml
[stars-shield]: https://img.shields.io/github/stars/apixelsnet/mango.svg?style=for-the-badge
[stars-url]: https://github.com/apixelsnet/mango/stargazers
[forks-shield]: https://img.shields.io/github/forks/apixelsnet/mango.svg?style=for-the-badge
[forks-url]: https://github.com/apixelsnet/mango/network/members
[issues-shield]: https://img.shields.io/github/issues/apixelsnet/mango.svg?style=for-the-badge
[issues-url]: https://github.com/apixelsnet/mango/issues
[contributors-shield]: https://img.shields.io/github/contributors/apixelsnet/mango.svg?style=for-the-badge
[contributors-url]: https://github.com/apixelsnet/mango/graphs/contributors
[license-shield]: https://img.shields.io/github/license/apixelsnet/mango.svg?style=for-the-badge
[license-url]: LICENSE
[kofi-shield]: https://img.shields.io/badge/Ko--fi-support-FF5E5B?style=for-the-badge&logo=kofi&logoColor=white
[kofi-url]: https://ko-fi.com/pixelsthecoder
[packages-url]: https://github.com/apixelsnet/mango/pkgs/container/mango
[library-screenshot]: docs/images/library.jpg
[library-dark-screenshot]: docs/images/library-dark.jpg
[title-screenshot]: docs/images/title.png
[updates-screenshot]: docs/images/updates.png
[settings-screenshot]: docs/images/settings.png
[rust-shield]: https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white
[rust-url]: https://www.rust-lang.org/
[sqlite-shield]: https://img.shields.io/badge/SQLite-003B57?style=for-the-badge&logo=sqlite&logoColor=white
[sqlite-url]: https://sqlite.org/
[svelte-shield]: https://img.shields.io/badge/Svelte-4A4A55?style=for-the-badge&logo=svelte&logoColor=FF3E00
[svelte-url]: https://svelte.dev/
[vite-shield]: https://img.shields.io/badge/Vite-646CFF?style=for-the-badge&logo=vite&logoColor=white
[vite-url]: https://vite.dev/
[kotlin-shield]: https://img.shields.io/badge/Kotlin-7F52FF?style=for-the-badge&logo=kotlin&logoColor=white
[kotlin-url]: https://kotlinlang.org/
[docker-built-shield]: https://img.shields.io/badge/Docker-2496ED?style=for-the-badge&logo=docker&logoColor=white
[docker-url]: https://www.docker.com/
