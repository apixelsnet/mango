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

## Waiting for the day (2026-10-12)

An option in Settings, off until switched on: a run leaves out the titles no chapter is expected of yet. Code: `expected` and `plan` in `updates.rs`, `rhythm` in `titles.rs`.

- A title's rhythm is read from the upload dates of its chapters: the middle one of the gaps between its last ten release days, from 1 to 28 days. Fewer than three days known, and the title is asked every run as before. A date still to come is not counted.
- A title is asked from the day before a chapter is expected to the day after, counted from its latest chapter. It is also asked once when such days went by unasked, so a Mango that was switched off catches up.
- After ten expected days with nothing new the days move apart, doubling up to four weeks. A series on a break is still looked at, only less often.
- It applies to "Update now" as well as to the timer. The Updates screen says how many titles were left out. A title's own refresh always asks.
- The title page shows the rhythm beside the status: Daily, Weekly, Every N days.

Checked by unit tests of the rule and the plan, and on Linux and Windows by `smoke.py`: a run with the option on asks the title or counts it as not due, without failures. By hand on Linux: a weekly title whose latest chapter was three days old was left out and counted.

Not checked: weeks of real runs. A series that changes its rhythm is found late by up to one of its old gaps.

## Keeping room

`MANGO_KEEP_FREE_MB` (off by default, 2026-10-18): while the disk under the data directory has less free than that, a chapter is not fetched; it fails with 507 `disk_full` "Less than N MB is free on the disk…" and takes the usual retries. A full disk would take the database's next write with it. Linux and macOS only. Checked on Linux with a real chapter: it was held back with that message and nothing was written.

## Done when

An overnight update finds new chapters, auto-downloads them, and they open with the network unplugged.

## Notifications, grouped updates and download maintenance (2026-10-25)

Implementation: `notify.rs`, `updates.rs`, `downloads.rs` (`retry_all`, `orphaned`), `routes/Updates.svelte` and `routes/Downloads.svelte`.

- **Notifications.** When a library update finds new chapters, a message is sent to the address configured in Settings › Library (`notify.url`). Three formats are supported: an ntfy topic (plain text with a title), a Discord webhook, and a generic JSON webhook receiving `{event, count, chapters: [{mangaId, title, chapterId, chapter}]}`. Up to twenty chapters are listed by name and the remainder are counted. Because the address is supplied by the account, it is subject to the same restrictions as repositories (`net.rs`): HTTPS and public addresses only, unless `MANGO_STORES_LOCAL=on` is set. `POST /api/notify/test` sends a test message. A delivery failure is logged and does not cause the update to fail. Web Push is described below.
- **Grouped updates.** The Updates screen groups each day's chapters by title. A title with several new chapters appears as a single expandable row with a button to download all of them.
- **Retry failed downloads.** All failed downloads can be retried at once (`POST /api/downloads/retry`).
- **Downloads of removed titles.** Settings › Downloads lists titles that are no longer followed but still have downloaded chapters, with their total size, and can delete those files (`GET` and `DELETE /api/downloads/orphans`). Reading progress is retained.
- **Downloads of read chapters.** Settings › Downloads also counts the downloaded chapters already read, with their total size, and can delete those files (`GET` and `DELETE /api/downloads/read`). The chapters stay in the library, marked read.
- **Page cache.** Settings › Downloads shows what the account's page cache takes and can empty it (`GET` and `DELETE /api/cache`). Covers, downloads and local titles stay.
- Titles merged into a library title are updated with it and inherit its categories and its automatic download setting; their chapters are listed under it.
- The Updates, Downloads and Import screens poll the server only while the page is visible (`every` in `lib/util.ts`). The update badge in the menu uses `GET /api/updates/fresh`, which performs one query instead of three.

Verification: unit tests and `smoke.py` on Linux, including the rejection of a test notification addressed to the local machine.

Not verified: delivery of an update notification to a real ntfy, Discord or webhook receiver.

## Web Push (2026-10-25)

A device can show a notification of new chapters with Mango closed, without a native app. Code: `mango-reader/src/push.rs`, `ui/src/lib/push.ts`, the end of `ui/public/sw.js`.

- **The toggle.** Settings › Library › "Notify this device", one per device and per account. Turning it on asks the browser for permission, subscribes with the account's public key and sends the subscription to `PUT /api/push`; turning it off unsubscribes and calls `DELETE /api/push`. Signing out turns it off for that device. "Send a test" (`POST /api/push/test`) sends to all of the account's devices and reports how many accepted.
- **Where it works.** Android and desktop browsers (Chrome, Edge, Firefox, Safari) from the page itself; iPhone and iPad (iOS 16.4 or later) only after Add to Home Screen, which the toggle explains in place of the switch. HTTPS is required, or an address on the same machine; a Mango served over plain HTTP on a home network shows why it cannot.
- **Sending.** After an update that finds new chapters, every subscribed device receives "N new chapters" with up to five named, alongside any ntfy, Discord or webhook address. Tapping the notification opens Updates. A newer notification replaces the previous one.
- **What happens on each device.** Every subscribed device of the account receives the same message; each decides how to show it:

  | Device and state | What the reader sees |
  | --- | --- |
  | Computer, Mango open in the window in front | No system notification. A notice in the corner of the page with Open and Dismiss, gone after 12 s; the count beside Updates goes up. |
  | Computer, Mango in a background tab, a window behind others, or closed | A system notification. Clicking it brings an open Mango forward at Updates, or opens one. |
  | Android, Mango open on screen (browser or home-screen app) | As on a computer in front: the notice in the page, above the bottom tabs. |
  | Android, Mango in the background, closed, or the phone locked | A system notification, on the lock screen too. Tapping it opens Mango at Updates. |
  | iPhone or iPad, home-screen app (iOS 16.4+) | Always a system notification, open or not, because WebKit cancels a subscription whose pushes show nothing; an open Mango only updates its count. In Safari itself, without Add to Home Screen, nothing: the toggle says so. |
  | Any device off or offline | The push service keeps the message for 24 hours and delivers it when the device is back. |
  | Reading a chapter | No notice over the reader; the count is updated for when the reader is closed. |

  A newer notification replaces the one before rather than stacking. Reading the chapters on one device does not clear the notification on another.
- **Security.** Each account has its own P-256 signing key (VAPID, RFC 8292), created on first use and stored in its database (`push_key`, migration 013), not among its settings. Messages are encrypted for each device (RFC 8291, `aes128gcm`), so the browser's push service sees only the size and the time. Subscription addresses come from the browser and are held to HTTPS and public addresses (`net.rs`), whatever `MANGO_STORES_LOCAL` says. A device whose push service answers 404 or 410 is forgotten. At most 20 devices per account.
- **Cost.** About 0.2 MB of binary for `p256`, `hkdf` and `aes-gcm`.

Verification: unit tests for the RFC 8291 example message byte for byte, the signed token, address checks and keeping devices; `smoke.py` passed on Linux. On the server, local and plain-HTTP addresses were refused and a made-up Mozilla subscription was forgotten after its push service answered. In Chrome against a Windows build on the same machine: the toggle subscribed through Google's push service, "Send a test" was delivered and shown by the service worker, turning it off removed the device on both sides, and signing out did the same.

Not verified: an iPhone, Firefox and Safari delivery, and a notification sent by a real update run (that path shares the code of the test).

## Download state on the title page and Updates (2026-10-29)

Each chapter row says where its download stands, so a chapter just sent to the queue is visible as such. Code: `lib/queue.ts`, `lib/DownloadTag.svelte`, used by `routes/Title.svelte` and `routes/Updates.svelte`.

- Tags beside the chapter: **Saved** (filled), **Queued** (or **Paused** / **On hold** when the queue waits, **Retrying** after a failed try), the percentage while it is being fetched, and **Failed** (dashed, the reason on hover).
- The row's button follows: download, cancel while queued or fetching, try again once failed, delete once saved. "Download the unread chapters" and the picked "Download" leave out chapters already queued.
- A line over the list counts the title's chapters in the queue, with paused, on hold and failed, and links to Downloads.
- The page reads `GET /api/downloads` once after the title, not before its first paint, and again every 2 s only while one of its chapters is queued or being fetched (and the page is in view). A chapter that leaves the queue makes the page read the title again, so it turns Saved without a reload.
- Updates does the same for the chapters it lists, and reads the queue again when a run ends, since a run queues chapters of titles set to download them. A title's grouped row shows Saved once all its new chapters are, otherwise the tag of the one being fetched or waiting.
- While picking chapters on a phone, the actions along the bottom wrap onto several lines; the list's bottom space follows the bar's real height, so the last chapters scroll clear of it.

## Skip titles not started (2026-10-28)

The option `updates.skipUnstarted` (Settings › Library, off by default) leaves out of a library update every title whose chapters were all left unopened: none is read and none has a page turned. A title with no chapters at all is still asked, since there is nothing to start on. Code: `plan` in `updates.rs`; checked by its unit test.
