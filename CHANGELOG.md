# Changelog

What changed for people who read with Mango, in plain words. Changes to how Mango is built and tested are left out; the release pages list every pull request. Versions follow `MAJOR.MINOR.PATCH`. Notes for versions before 1.5.2 are on the [releases page](https://github.com/apixelsnet/mango/releases).

## Unreleased

## 1.5.2 — 2026-11-15

### Added
- **Continuous chapters**: at the end of a chapter, the next one follows in the same reader, below in a long strip or as the next page in the page-by-page modes, with a line where one chapter ends. Dragging the scrollbar doesn't pull in chapters you skip past. Turn it off under **Settings › Reader**.
- **Data saver** under **Settings › Reader**: pages come smaller, for reading on mobile data. It is set on each device on its own, so your phone can save data while your computer at home doesn't.
- **Double-tap to zoom** in the reader: two taps zoom in where you tapped, two more zoom back out.
- **Fetch pages again** in a chapter's menu, for a chapter whose pages came through broken or changed on the source.
- **Stop** a library update under **Updates**.
- **Signed-in devices** in **Settings**, with **Sign out** for each, for when you forgot to sign out somewhere.
- **EPUB books** in your local folder, read in the book's own page order.
- **Your settings travel with backups**: a backup now carries your settings, and importing one fills in only the ones you haven't set. Where notifications go and how often updates run are never taken from a backup.
- **API tokens**, for scripts and other apps that talk to Mango, and a **header for notifications**, for a notification address that needs a key.
- **Pages at a time** for downloads, and download ahead now follows the scanlation group you read.

### Changed
- **The library keeps your place**: after you open a title and go back, the category, search, page and scroll position are where you left them. Asked for by folfoa in #47.
- Reading done while Mango can't be reached is saved once it answers again.
- A library update leaves a source that keeps failing for the next run, so the update finishes sooner, and checks titles that failed last time at the end.
- Backups keep the last three by default (was seven), and no new backup file is written when nothing changed.
- Titles you only saw while browsing, never opened, are cleaned up after 90 days.
- **Statistics** count days by your own clock.
- Importing a large backup uses much less memory.
- Installing an extension downloads it once instead of twice.
- On a Mango with several accounts, the page cache limit now covers all accounts together.
- The desktop app uses newer window, browser and tray parts. You stay signed in after updating.

### Fixed
- Chapters a source moves to new addresses keep what you read, and chapters that briefly disappear from a source come back with your progress instead of as new chapters.
- **Mark all read** keeps the time a chapter was first read.
- Moving a title to another source now also carries its page notes, the details and cover you changed by hand, and its scanlation group settings.
- A failed extension update keeps the version that worked.
- On a browser that several accounts use, one account can no longer see another's saved chapters.
- Unknown folders in Mango's data folder are set aside at start-up instead of deleted.
- A notification header you set replaces Mango's own instead of being sent twice.
- Security: the proxy and solver addresses can't be read or changed through an account's settings; the reading feed for other apps (OPDS) refuses requests from other websites; and covers are never fetched from addresses in your own network.
