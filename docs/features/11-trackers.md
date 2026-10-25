# 11 — Trackers

Keep reading progress in step with a tracking site: AniList, MyAnimeList, Kitsu, MangaUpdates.

## What it does

- Sign in to a tracker in Settings, Trackers.
- On a title's page, search the tracker and link the title to its entry there.
- Change the status, the chapter reached and the score from that page.
- Finishing a chapter, or marking chapters read, moves the entry along. Never back.
- Links that came in with a Mihon or Suwayomi backup work as soon as someone signs in, and go out again with an export.

## Signing in

No tracker hands out a token to an address it does not know, and a self-hosted Mango has no fixed address. So:

| Tracker | How | What the owner needs |
|---|---|---|
| AniList | Paste a token | An API client of their own with the redirect URL `https://anilist.co/api/v2/oauth/pin`. Its "authorize" page then shows a token. |
| MyAnimeList | Its own sign-in page, and back | An API client of their own, type "other", whose redirect URL is their Mango's address. |
| Kitsu | E-mail and password | Nothing else. |
| MangaUpdates | User name and password | Nothing else. |

A password is sent to the tracker once and not kept. Tokens are kept in the `state` table, which the settings API never shows. Client ids are ordinary settings.

## Data

Table `tracks` (migration 005, `error` added in 007), one row per title and tracker. Tracker ids, statuses and scores are stored as Mihon stores them, so a backup's links need no translation: tracker 1 MyAnimeList, 2 AniList, 3 Kitsu, 7 MangaUpdates. Scores are shown out of ten everywhere; AniList's are stored out of 100.

## API

- `GET /api/trackers`: the four, who is signed in, their statuses and score steps.
- `POST /api/trackers/{id}/login` with `token`, or `username` and `password`, or `code` and `state`; `DELETE` signs out.
- `POST /api/trackers/{id}/authorize` with `clientId` and `redirect`: the address of the tracker's sign-in page (MyAnimeList only).
- `GET /api/trackers/{id}/search?q=`.
- `GET /api/manga/{id}/tracks`: the trackers and the title's links, from the database alone.
- `POST /api/manga/{id}/tracks/{tracker}` links to a search result, `PUT` changes status, score or chapter, `DELETE` unlinks, `POST …/refresh` compares with the tracker again.

## Notes

- A title's page asks for its links after it is up, and no tracker is asked until something is pressed.
- Linking takes the tracker's status and score and the further chapter of the two sides.
- The chapter reached is the highest number among the read chapters, in whole chapters.
- Reaching the last chapter the tracker knows of completes the entry; reading a planned one starts it. Paused and dropped stay as they are.
- When a tracker cannot be reached, the link keeps Mango's progress and the reason, and is sent again with the next chapter read.
- Unlinking leaves the entry on the tracker. Signing out leaves the links.
- Search results show no covers: the page loads images from Mango only.

## Result (2026-10-10) — built; everything that needs an account is unchecked

Code: `mango-reader/src/trackers/` (`mod.rs` and one file per tracker); UI in `lib/Tracking.svelte` on the title page and the Trackers group of `routes/Settings.svelte`.

Checked on Linux and Windows by `smoke.py` (109 checks in all) and in a browser:

- Search on AniList, Kitsu and MangaUpdates, which need no account.
- Each sign-in reaches its tracker and a made-up token or account is refused with the tracker's own reason. Kitsu's answer shows it knows the client Mango names.
- MyAnimeList's sign-in page is addressed with a challenge and a state, an answer with another state is refused, and coming back to Mango with `?code=&state=` lands on the Trackers settings with the address tidied.
- A link from a backup shows on the title, goes out with an export, and is left alone by reading while nobody is signed in.
- The title page: status, chapter and score fields, a failed sending's reason, the search panel, and a refusal when the tracker rejects the token. A link whose address is not a web address is shown as text.
- No token appears in the settings API.

Not checked, for want of an account on any of the four: a sign-in that succeeds, MyAnimeList's search, linking, changing an entry, the push after a chapter is read, and the renewal of a token. These follow each tracker's published API and Mihon's use of it; expect the first real sign-in to find something.

## Two-way synchronization (2026-10-25)

Refreshing a tracker link marks as read every numbered chapter up to the last chapter the tracker reports as read, so progress made in other applications is reflected in Mango. When "Take in what was read elsewhere" is enabled (`trackers.pull`), the links of every library title are refreshed after each library update, two seconds apart. Chapters of merged sources count toward the title they belong to. Verification: unit tests only; no tracker account was used.
