# 06 — Updates and downloads

Find new chapters, and keep chapters on disk.

## MVP — updates

- Scheduled library update with a configurable interval. Manual "update now".
- Options: skip completed titles, skip titles with unread chapters, include or exclude by category.
- Per-source concurrency cap and rate limit so sources are not hammered.
- Updates screen: new chapters grouped by day.
- Failures listed per title with the reason; one failing source never stops the run.

## MVP — downloads

- Queue: add, pause, resume, reorder, cancel. Progress per chapter.
- Retries with backoff. Survives a restart.
- Saved as CBZ with `ComicInfo.xml`, at `downloads/<source>/<title>/<chapter>.cbz`.
- Reader uses the CBZ when present, no network needed.
- Per title: auto-download new chapters. Global option: delete after reading.

## Notes

- Updates and downloads start the host; the host stops again when the queue is empty and idle.

## Result (2026-10-10) — built

Code: `mango-reader/src/updates.rs`, `downloads.rs`; the reader's side is in `images.rs`; UI in `routes/Updates.svelte`, `Downloads.svelte`.

Updates
- A run takes the library titles whose extension is installed, grouped by source. Four sources at a time; within a source one title after another, a second apart. Each title is one chapter-list call.
- A chapter counts as an update when Mango first saw it after its title joined the library. A title's back catalogue is never news.
- Options, all in Settings: the interval (12 hours by default, or only when asked), skip completed titles, skip titles with unread chapters, only or never certain categories.
- A title that fails is listed with the reason until a later run succeeds. Titles whose extension is gone are skipped.

Downloads
- The queue is a table, so it survives a restart; a chapter that was half fetched starts over. One chapter at a time, two pages at a time, written straight into the CBZ so memory holds two pages at most.
- A failed chapter waits 30 s, 2 min, 10 min, then 30 min; after five tries it is parked until the owner asks again.
- Files: `<data>/downloads/<source>/<title>/<chapter>.cbz`, pages `001…` in order plus `ComicInfo.xml`. Names are made safe for Linux, macOS and Windows.
- A downloaded chapter is read from its file and the source is never asked.
- Per title: download new chapters as updates find them. In Settings: delete a chapter's file once it is read.

Checked on Linux and Windows by `smoke.py` (64 checks in all): a 58-page chapter saved in 19 s as one CBZ with its `ComicInfo.xml`; with its page cache deleted and the JVM asleep it still read; an update found a chapter that had been removed from the database and queued it because the title asked for it. By hand: the queue went on after a restart, and both screens in a browser.

Not checked: the timer itself (only the rule it applies), a real overnight run, a source that fails during an update, retries after a real failure, and delete after reading.

## Done when

An overnight update finds new chapters, auto-downloads them, and they open with the network unplugged.
