# 05 — Reader

The core of the app. Must feel instant.

## MVP

- Modes: paged left-to-right, paged right-to-left, vertical paged, webtoon long strip. Remembered per title.
- Fit: width, height, original size. Zoom and pan.
- Input: keyboard (arrows, space, J/K), touch zones, swipe, mouse wheel.
- Progress saved per page; reopening resumes at the same page.
- Preload the next pages and the first pages of the next chapter.
- Chapter transitions: next and previous without leaving the reader.
- Controls overlay: chapter picker, page slider, mode, fit. Hidden while reading.
- Failed page: placeholder with retry; the rest of the chapter keeps loading.

## Notes

- Pages come from the host through the core, then land in a disk cache with a size cap.
- Webtoon mode uses a virtualized list; only nearby images are decoded.
- First page must not wait on anything else: chapter list, tracker and history writes happen after paint.

## Result (2026-10-10) — built

Code: `mango-reader/ui/src/routes/Reader.svelte`; pages come from `mango-reader/src/images.rs`.

- All four modes, three fits, zoom (Ctrl and wheel, `+` `-` `0`, pinch), keys, tap zones, swipe, wheel, the controls overlay. Mode and fit are remembered per title.
- A chapter opened before shows its first page without asking the source: the page count is in the database and the pages are on disk. Otherwise the page list is the only thing the first page waits for.
- The next three pages are fetched ahead, and near the end the first two of the next chapter.
- Webtoon mode keeps only the images near the screen in the document; each page keeps its height once seen.
- Page cache: `<data>/cache`, 1 GB by default (`cache_mb`), least recently used goes first. It is swept every ten minutes, and at once when a tenth of the cap went in since the last sweep. A single image over an eighth of the cap is sent but not kept.

Checked in a browser against real sources: a 37-page chapter paged with keys and slider; an 82-image strip scrolled end to end at phone size with at most 12 images in the document and no slow frame; a reload came back to the same page; the chapter was marked read at the end. Page times: 0.3 s for the list, 0.3 to 0.5 s for a 3 MB page, 5 ms once cached.

Not checked: a 200-image chapter, a real phone, touch gestures, the failed-page placeholder.

## Result (2026-10-11) — settings by kind of series

Code: `ui/src/lib/reader.ts` (every setting, its limits, the layers), `ui/src/lib/ReaderPrefs.svelte` (the controls, shared by Settings and the reader), `Reader.svelte`; `kind_of` in `mango-reader/src/titles.rs`.

- A title is a manga, a manhwa, a manhua or a comic: by its tags first, then its source's name, then the source's language. The reader can tell a title otherwise ("Read as").
- Three layers, the later winning: what Mango ships for the kind (manga right to left, comics left to right, manhwa and manhua a long strip), what Settings › Reader holds for the kind (`reader.profiles`), what the title keeps for itself (`manga.reader_mode`, `reader_fit`, and `reader_prefs`, a JSON object). The old `reader.mode` and `reader.fit` still count, for manga.
- The reader has a settings sheet, beside the page or under it on a phone. A change goes to this title or to every title of its kind, as chosen there. Each group resets on its own.
- Layout: a fourth fit (the whole page on screen); two pages side by side, always or on a wide screen, with the cover alone if wanted, and a page wider than tall never paired; strip width and the space between pages.
- Auto-scroll: pixels a second down a strip, seconds a page otherwise; it pauses under a finger and while the controls are up, and can carry on into the next chapter.
- A strip scrolls by whole pixels, so on a fast screen auto-scroll would stand still a frame and then jump. The fraction the scroll could not take is made up by shifting the pages with a transform; measured in Chromium, the strip then moves the same amount every frame, within 0.02 px of a steady line. Strip images decode off the main thread.
- Tap zones: sides, Kindle, L shape or off, and swapped if wanted. How far a tap or Space scrolls. How many pages load ahead.
- Picture: background, brightness, contrast, saturation, hue, warmth, black and white, invert, and a color wash with a blend mode.
- On screen: page number, progress bar, the screen kept on, full screen.
- Keys added: `A` auto-scroll, `[` `]` its speed, `S` settings, `D` two pages. `F` goes round the four fits.

Checked in a browser on the server's build, with made-up pages in the local source: a title tagged as a webtoon opened as a long strip and one tagged manga right to left; auto-scroll moved 161 px in 2 s at 80 px/s and 200 px at 100; contrast and brightness reached the images and were kept for the title; two pages sat right then left, a wide page stood alone, and the cover offset shifted the pairs; taps on the edges turned the page; a background set for all manga went to the settings and not to the title; a page every 2 s ran off the last page into the next chapter and kept going; the sheet at phone size. `smoke.py` passes on Linux and Windows.

Not checked: a real phone, the screen lock, full screen, real scans with split spreads. Margin cropping, page splitting, the sideways strip and custom key bindings were added on 2026-10-25; see below.

## Done when

A 60-page chapter and a 200-image webtoon chapter both read end to end on desktop and phone with no stutter and correct resume.

## Cropping, page splitting, sideways strip and image enhancement (2026-10-25)

Implementation: `lib/pages.ts` (page processing on a canvas), `lib/reader.ts` and `routes/Reader.svelte`.

- **Crop margins.** Plain white or black borders are detected on a downscaled copy of the page and removed, leaving a small margin. The four corners must agree on the border color, and a row or column counts as plain when fewer than 1% of its pixels differ. No crop is applied if it would remove less than 3% of the area or leave less than 40% of either dimension. Pages larger than 16 megapixels are left unchanged, as mobile browsers may refuse a canvas of that size.
- **Split wide pages.** In the paged modes, a page wider than it is tall is shown as two halves in reading order (the right half first in right-to-left mode). Page navigation passes through both halves.
- Processed pages are served as blob URLs; up to sixteen are retained at a time. Whether a page is wide is determined from the original image.
- **Sideways strip.** A fifth reading mode places pages side by side at screen height with horizontal scrolling. The mouse wheel scrolls horizontally, and auto-scroll and page spacing apply.
- **Scaling and sharpening.** Scaling may be set to smooth, crisp or pixelated (`image-rendering`). Sharpening applies an SVG convolution filter with an adjustable strength.
- **Custom key bindings.** Settings › Reader lists each reader action with its key and allows it to be reassigned (`reader.keys`). The arrow keys, Space, Home, End and Escape cannot be reassigned.
- **Saved pages.** The bookmark button in the top bar saves the current page with an optional note (`PUT` and `DELETE /api/chapters/{id}/marks/{page}`); the B key saves or removes a page without a note. `#/read/{id}?page=N` opens a chapter at a given page.
- **Download ahead.** When enabled in Settings › Reader, opening a chapter of a library title queues the next one to ten chapters for download.
- **Incognito mode.** When enabled in Settings › Appearance (stored per device), reading progress, history and tracker updates are not recorded, and no chapters are downloaded ahead. The top bar indicates that the mode is active.

Verification in Chromium against a Linux build: a 600×900 page with 70-pixel margins was cropped to 471×770; a 1200×800 spread was shown as two halves in both directions; sharpening was applied; a saved page was stored; and the sideways strip scrolled with the Space key.

Not verified: real scans with uneven margins, canvas limits on mobile devices, and Safari support for the sharpening filter.

## Settings by device (2026-10-28)

Implementation: `lib/display.svelte.ts` (what the device is), `lib/reader.ts` (`BY_DEVICE`, `resolve`, `keep`), Settings › Reader and the reader's sheet.

- Page size, two pages, strip width, tap zones and their swap, how far a tap scrolls, pages loaded ahead and auto-scroll speed are kept apart for phones, tablets and computers. Everything else stays the same on every device.
- The device is told by its screen: a touch screen whose short side is under 600 CSS pixels is a phone, a larger one a tablet, a mouse a computer. Settings › Reader can say otherwise; that choice is kept in the browser.
- Each layer keeps a device's answers beside its shared ones: Settings in `reader.devices` (device, then kind), a title in `devices` inside its `reader_prefs`. A device's answer wins over the shared one in the same layer. Every phone shares one set, so a new phone needs no setup.
- Shipped for phones: fit width. Computers and tablets keep fit height.
- Settings › Reader › Devices turns the whole thing off (`reader.byDevice`; on by default), and can set up another kind of device from this one. Settings kept by device carry the device's name as a tag.
- Letting a setting go (a group's Reset) lets it go on this device and from the shared answer, so the shipped one shows. "Reset manga" and "Forget what this title keeps" clear every device.

Checked in Chromium against a Linux build, with a desktop window and a phone emulated with touch at 390×844: a manga opened at fit height on the desktop and fit width on the phone; F on the phone stored `devices.phone.fit` for the title and left the desktop at height; F on the desktop stored its own and left the phone alone; the phone's manga fit set from the desktop's Settings reached the phone and not the desktop; turned off, both read the same, and back on, the phone's answer returned.

Not checked: a real phone or tablet, a touch laptop.

## Loading, failed pages and accessibility (2026-11-11)

Implementation: `routes/Reader.svelte`, `lib/reader.ts`.

- **Loading.** A page still on its way shows a spinner and "Loading page N…", over the screen in the paged modes and over its own frame in a strip. It appears after 0.3 s, so a cached page does not flicker. After 10 s it says the page is slow and offers to try again.
- **Failed pages.** A page that fails is tried again by itself twice, after 1 s and 2 s. Then it shows why, as Mango answered (for example the source's error), with Try again, and Try all again when more than one failed. R loads every failed or slow page again. Offline, the page says so and loads again by itself when the device is back online.
- **Screen readers.** The page that is up is announced once turning settles ("Page 3 of 20"), and so is a page that did not load. A strip frame is `aria-busy` while its page loads.
- **Keyboard.** `?` (or the info button in the top bar on a computer) lists every key, the owner's own included. The list and the settings sheet take focus when they open and give it back when they close; Escape closes them first.
- **Motion.** Scrolling by Space, taps and auto-scroll steps does not glide when the system asks for reduced motion or the e-ink setting is on; the spinner pulses instead of turning.

Checked in Chromium against a mock API: a page answering 502 was tried three times, then showed the source's message; a page failing twice then succeeding loaded on its own; a 15 s page showed the spinner and, at 10 s, the slow notice; R retried; `?` opened the list with focus inside and Escape closed it without leaving the reader. Not checked: a real screen reader, a real phone.

## Pinch zoom, page previews, sharing a page and focus (2026-11-11)

Implementation: `routes/Reader.svelte`, `lib/Icon.svelte`.

- **Pinch zoom.** Two fingers zoom the page itself, from 50% to 400%, in the paged modes and the long strip, keeping the spot between the fingers where it is. The rest of the app no longer zooms with it (`touch-action: pan-x pan-y`). A pinch is never taken as a swipe, and a zoom ending within 8% of the page's own size snaps back to it. Ctrl and the wheel now zoom at the pointer, and the zoom keys say the new level ("Zoom 120%"). In a strip, a zoom past the screen's width can be scrolled across.
- **Page previews.** While the page slider is dragged, the page under the thumb shows small above it with its number, and the reader turns to it on letting go. Arrow keys on the slider still turn at once. The slider tells a screen reader "Page 3 of 20" rather than a bare number.
- **Sharing a page.** A button beside the reader settings hands the page that is up to the system's share sheet on a phone or tablet that has one, and downloads it as a picture elsewhere, named "title - chapter - page N".
- **Focus.** The settings sheet and the list of keys are dialogs: Tab goes round inside them instead of out to the page behind.
- **High contrast.** In Windows' contrast themes the spinner's turning part and the progress bar take the system's highlight color.

Checked in Chromium against a mock API: a synthetic two-finger pinch from 90 to 290 px apart zoomed to 322% without turning the page, and the live region said so; dragging the slider showed page 7 above the thumb while page 1 stayed up; the download was named "Test title - Chapter 1 - page 1.svg"; Tab from the sheet's last control came back to its first. Not checked: a real phone's pinch and share sheet, iOS Safari.

## Continuous chapters (2026-11-13)

Implementation: `routes/Reader.svelte` (the run of chapters), `lib/reader.ts` (`chain`, `breaks`), `lib/router.svelte.ts` (`rename`).

- **One reader, many chapters.** Near the end of the last chapter in the reader, the next one is added after it: in a strip its pages follow on below (or across), and in the paged modes turning past the last page shows the next chapter's first page. The reader is not opened again, so nothing reloads and auto-scroll carries straight on.
- Pages are numbered across the whole run, and chapters are only ever added at the end, so nothing above the page being read moves when one arrives. The page number, slider, progress bar, saved pages and sharing all go by the chapter being read.
- **The chapter being read** is the one whose page crosses the middle of the screen. The address bar and history follow it without reopening the screen, so a reload or a way back opens that chapter.
- **Scrollbar safety.** Only one unread chapter is ever waiting ahead. While the mouse holds the strip's scrollbar, and for a moment after a jump of more than a screen and a half, no chapter is added, so the thumb does not change meaning under the pointer. A press counts as on the scrollbar outside the pages or, where the scrollbar is drawn over them and takes no room (Firefox, Safari), within 18 px of the edge it scrolls by. A pointer moving with no button down counts as let go, in case the release was not heard.
- **Finished chapters.** A chapter is kept as read when its last page is up, as before, or when reading goes on from it into the next chapter, however fast (a throw on a phone may never stop on a short last page). Got to by a jump (the scrollbar held, a single scroll of more than a screen and a half, N, P, the chapter buttons or the chapter list), it keeps only the page it was on.
- **Settings › Reader › Turning:** *Continuous chapters* (on by default) and *Mark where a chapter ends* (a line naming the chapter finished and the one starting). With continuous chapters off, the reader stops at the end of a chapter as before. With *Carry on into the next chapter* off, auto-scroll stops when the next chapter comes up.
- Going back past the first chapter in the reader opens the previous one as before. If the next chapter does not load, the strip's end says why and offers Try again and the Next chapter button; if it is still on its way after four seconds in sight, it offers to open it on its own. A chapter reached by reading on starts at its first page, even if it was begun before.

Checked with Playwright on a desktop window and an emulated phone, three runs in a row, against the mock API (`e2e/chapters.spec.ts`, `e2e/scrollbar.spec.ts`, `e2e/smoke.spec.ts`; the mock has a twelve-chapter manhwa run, one chapter of five pages):
- a strip read through all twelve chapters in one reader: eleven chapter lines, every chapter written as read once and in order, fewer than 20 pictures in the document, no long task over 250 ms;
- a throw across the end of a chapter still finishes it; a strip dragged through a last page to the middle of the next chapter keeps the page and not the finish, and reading on from it later finishes it;
- the real scrollbar pressed (Chromium with scrollbars shown) holds the next chapter back while the strip is scrolled to its end, and one comes on letting go;
- auto-scroll runs on into the next chapter, and stops as it comes up with *Carry on* off; paged auto-turning carries on in place;
- right to left, left to right, vertical and the sideways strip all cross chapters in place and back; two pages side by side leave a chapter's odd last page alone;
- a next chapter that fails to load says why, and comes on Try again;
- in a later chapter the slider, page count and saved pages go by that chapter; N, P and the chapter list move in place; a reload and Back open the chapter reached; with the setting off, the strip ends on the Next chapter button.

Checked against a real build of this branch (Linux, local source, a three-chapter manhwa of PNG pages): the strip read through all three with each chapter kept as read on the server, and a reload opened the chapter reached; pages turned across chapters in place with two side by side kept apart; N did not finish the chapter left and a saved page landed on its own chapter; the scrollbar press held the next chapter back. The server logged no warning.

In visible browser windows (2026-11-13):
- Chrome 157, the real build: the scrollbar thumb dragged top to bottom by the system's own mouse scrolled the strip to the end of chapter 1 with no chapter added while held; chapter 2 came 0.46 s after letting go and the place did not move. The real mouse wheel then scrolled on into chapter 2: the address and title changed in place and chapter 1 was kept as read.
- Firefox (Playwright's build), the mock: its overlay scrollbar dragged by the system's mouse first showed chapter 2 added in the middle of the drag (the press lands inside the pages there); the edge rule above was added, and the same drag then added nothing until let go, and chapter 2 came 0.41 s after.
- Firefox and WebKit ran `chapters.spec.ts` headed: all pass. WebKit's Windows build draws no scrollbar, so a real drag there does not scroll.

Not checked: a real phone; Safari itself (macOS); an online source. One Firefox run of the auto-scroll test stalled while its window was covered (animation frames are slowed in a hidden window); it passed three times alone.

## Double-tap, and data saver (2026-11-14)

- **Double-tap to zoom**: two taps in the middle of the page zoom to 200% at that spot, two more back to the page's size. A tap in the middle now waits 300 ms for a second one before it shows or hides the bars; taps at the sides turn the page at once, as before.
- **Data saver on this device** (Settings › Reader): pages come as JPEGs no wider than 1080 pixels, at quality 70 (`?light=1`, `Images::lighter_page`). Mango makes each the first time and keeps it beside the page; a page that would come out no smaller comes as it is. It is this browser's setting, kept in its own storage, since a phone on mobile data wants it and a computer at home does not. Checked by the unit test `lighter_pages_are_narrower_and_smaller`.
