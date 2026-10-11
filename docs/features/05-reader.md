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
- Page cache: `<data>/cache`, 1 GB by default (`cache_mb`), least recently used goes first.

Checked in a browser against real sources: a 37-page chapter paged with keys and slider; an 82-image strip scrolled end to end at phone size with at most 12 images in the document and no slow frame; a reload came back to the same page; the chapter was marked read at the end. Page times: 0.3 s for the list, 0.3 to 0.5 s for a 3 MB page, 5 ms once cached.

Not checked: a 200-image chapter, a real phone, touch gestures, the failed-page placeholder.

## Result (2026-10-11) — settings by kind of series

Code: `ui/src/lib/reader.ts` (every setting, its limits, the layers), `ui/src/lib/ReaderPrefs.svelte` (the controls, shared by Settings and the reader), `Reader.svelte`; `kind_of` in `mango-reader/src/titles.rs`.

- A title is a manga, a manhwa, a manhua or a comic: by its tags first, then its source's name, then the source's language. The reader can tell a title otherwise ("Read as").
- Three layers, the later winning: what Mango ships for the kind (manga right to left, comics left to right, manhwa and manhua a long strip), what Settings › Reader holds for the kind (`reader.profiles`), what the title keeps for itself (`manga.reader_mode`, `reader_fit`, and `reader_prefs`, a JSON object). The old `reader.mode` and `reader.fit` still count, for manga.
- The reader has a settings sheet, beside the page or under it on a phone. A change goes to this title or to every title of its kind, as chosen there. Each group resets on its own.
- Layout: a fourth fit (the whole page on screen); two pages side by side, always or on a wide screen, with the cover alone if wanted, and a page wider than tall never paired; strip width and the space between pages.
- Auto-scroll: pixels a second down a strip, seconds a page otherwise; it pauses under a finger and while the controls are up, and can carry on into the next chapter.
- Tap zones: sides, Kindle, L shape or off, and swapped if wanted. How far a tap or Space scrolls. How many pages load ahead.
- Picture: background, brightness, contrast, saturation, hue, warmth, black and white, invert, and a colour wash with a blend mode.
- On screen: page number, progress bar, the screen kept on, full screen.
- Keys added: `A` auto-scroll, `[` `]` its speed, `S` settings, `D` two pages. `F` goes round the four fits.

Checked in a browser on the server's build, with made-up pages in the local source: a title tagged as a webtoon opened as a long strip and one tagged manga right to left; auto-scroll moved 161 px in 2 s at 80 px/s and 200 px at 100; contrast and brightness reached the images and were kept for the title; two pages sat right then left, a wide page stood alone, and the cover offset shifted the pairs; taps on the edges turned the page; a background set for all manga went to the settings and not to the title; a page every 2 s ran off the last page into the next chapter and kept going; the sheet at phone size. `smoke.py` passes on Linux and Windows.

Not checked: a real phone, the screen lock, full screen, real scans with split spreads. Not built: cropping borders, splitting a wide page in two, a strip that runs sideways, keys of one's own choosing.

## Done when

A 60-page chapter and a 200-image webtoon chapter both read end to end on desktop and phone with no stutter and correct resume.
