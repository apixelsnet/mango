# 14 — Phones

Mango on a narrow screen, and as an app on a phone's home screen. All of it is in the web UI: `mango-reader/ui`.

## As an app

- `public/manifest.webmanifest` and the icons in `public/icons` make it installable. Addresses in the manifest are relative, so it works under a sub-path.
- On iOS it is added from the share menu. It then opens without the address bar, and the page reaches up under the clock, which iOS draws in white. A band in the darker of paper and ink (`--bar`) sits behind it. In a browser the band has no height.
- Every bar that touches an edge keeps clear of the notch and the home bar (`--safe-*` in `app.css`). The reader starts below the clock.
- On a home screen iOS measures the page as if the clock still took its room, so on a short page the tabs stopped short of the bottom by the clock's height. `html` is made as tall as the screen there (`display-mode: standalone` in `app.css`).
- A phone fetches the icon and the manifest without cookies. Behind a proxy that asks for a sign-in first, `/icons/`, `/manifest.webmanifest` and `/apple-touch-icon.png` have to be let through, or the home screen shows a letter instead of the mark.
- There is no service worker: Mango needs its server for everything it shows.

## Getting around

- Narrower than 760px, the menu is a row of tabs along the bottom. A press on the tab already open goes to its top. The header is one line: the name and the settings.
- An app on a home screen has no back button, so a screen that is not one of the tabs gets one in the header. A title is a screen of its own: its bar stays at the top with the way back, and the tabs make room for its Resume button.
- The reader's back goes back to the title it was opened from, instead of opening a second copy. Before, the title's own back then led into the reader again.
- `lib/router.svelte.ts` numbers each screen in the browser's history. A new screen starts at its top. One come back to is put where it was left, as soon as it is long enough again.
- `lib/keep.ts` holds what a list last showed. Library, Updates, History, Browse and a title show it at once and then ask again. A source's list and a search are shown as they were left, with no new request. It is emptied by signing in or out, which reloads the page.

## Reading and touching

- Fields are 16px on a touch screen, or iOS zooms the page in on them.
- A tap has no wait and no grey flash. Rows of chips slide sideways to the screen's edges instead of wrapping.
- In the library, the order and the layout wait behind the Filter button, to keep the tools to one line. The selection bar stays in reach while scrolling.

## For each device

Settings → Appearance → This device. Kept in the browser (`mango.display`), not on the server: a phone and a desk want different answers. `lib/display.svelte.ts`.

| Setting | Choices |
|---|---|
| Size | Small 90%, Normal, Large 110%, Largest 125%. Text and controls together, by CSS `zoom` on everything but the reader. |
| Covers | Small (three across a phone), Medium, Large. |
| Menu | Automatic, Bottom, Top. |

## Result (2026-10-12) — built

Checked in Chromium against a Mango on the Linux server, at 390×844 and 320×568: no screen scrolls sideways; library, a long title and a source's list are put back where they were left; reader and title no longer loop; the three settings hold after a reload; the wide layout is as it was at 1440×900.

Not checked: a real iPhone. The band behind the clock, the notch and home-bar spacing, and the icon on a home screen cannot be seen in a desktop browser.

## Offline reading, e-ink support and incognito mode (2026-10-25)

- **Offline reading.** From a chapter's menu or the selection bar, chapters can be saved to the browser's cache (`lib/offline.ts`), together with the data the title page and the reader request, the session, the settings and the reader's code. The service worker (`public/sw.js`) serves this copy when Mango cannot be reached, and serves saved pages from it first. The application shell is cached by the service worker as it is fetched. Other pages under the same host name are not cached, because the hosted service shares its host name with the account pages. Signing out clears the saved chapters of every account. Browsers provide this cache only over HTTPS or on the local machine.
- **E-ink and privacy.** "Still screen", "Library in pages" and "Incognito" are available in Settings › Appearance and are stored per device.

Verification in Chromium against a Linux build: a chapter was saved, the network was disabled, the page was reloaded and all four pages of the chapter were displayed.

Not verified: a real phone and a real e-ink device.
