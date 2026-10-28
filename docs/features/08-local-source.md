# 08 — Local source

Read files the user already has. Pure Rust; the JVM is never involved.

## MVP

- A library folder laid out as `<root>/<title>/<chapter>`, where a chapter is a `.cbz`, a `.zip`, or a folder of images.
- Metadata from `ComicInfo.xml` when present; otherwise from folder and file names.
- Cover from `cover.jpg` / `cover.png`, else the first page.
- Natural sort for chapters and pages (`2` before `10`).
- Rescan on demand and on a schedule.
- Appears as a normal source: browse, add to library, read, track progress.

## Notes

- Never write into the user's folder unless they ask (metadata edits, later).
- Guard against archive path traversal and oversized images.

## Result (2026-10-10) — built

Code: `mango-reader/src/local.rs`. The folder is `local` in the data directory, or wherever `local_dir` points; Settings shows the path.

- It is listed as a source named Local, with id 0, and goes through the same code as any other from there on: lists, title page, library, reader, progress, history, updates.
- Browsing reads the folder as it is at that moment. A title's chapters are read again when it is refreshed, and by every library update, which is the schedule: a chapter dropped into the folder shows up under Updates.
- Chapter names are the file or folder names. A title's summary, author, artist and genre come from a `ComicInfo.xml` in its folder, or else from the one in its first chapter.
- Pages are read straight out of the archive or folder, in natural order, every time. Nothing is unpacked, cached or written.
- Guards: a title or chapter name must be a single plain name, so nothing reaches outside the folder; pages are read from archives by name and never extracted; an image over 64 MB is refused; hidden files and `__MACOSX` are skipped.

Checked on Linux and Windows by `smoke.py`: a folder with a CBZ chapter, a folder chapter and a cover file showed up as a title, read page by page with 2 before 10, joined the library and kept its progress, with the JVM asleep. With the host's files absent altogether, a 12-chapter title listed and read, a library update ran clean, and one chapter was read in a browser in all four reader modes.

Not checked: a large real collection.

## Chapter details and titles without chapters (2026-10-28)

- A chapter archive or folder with its own `ComicInfo.xml` takes its name from `<Title>`, its number from `<Number>` and its scanlator from `<Translator>`. What it does not say comes from the file name, as before.
- A title needs no chapter level. A folder with pages straight in it is one chapter, leaving out `cover.*`; a `.cbz` or `.zip` straight in the root is a title of its own, named without its extension, and is its one chapter. Such a chapter is stored under the name `.`.

Checked by unit tests, and end to end on Linux and Windows: chapter 12.5 "The Return" by its `ComicInfo`, a folder of two pages and a cover read as one two-page chapter, and a loose CBZ listed and read as a title.

## Done when

A folder of CBZ files shows up as titles and reads in every reader mode with the extension host absent.
