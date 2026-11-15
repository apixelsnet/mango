# API reference

Every screen of Mango talks to the server through `/api`, and scripts can use the same routes.

**Signing in from a script.** In Settings › Account, make an app password "For scripts (API)", and send it as `Authorization: Bearer <token>`. The token can do whatever the account can. Revoking it takes effect at once. A wrong token makes its sender wait, as a wrong password does. Plain app passwords open only the reading-app catalog (`/opds`) and reading sync (`/opds/kosync`).

**Shape.** Bodies are JSON unless a route says otherwise. Changes are `POST`, `PUT` or `DELETE`. A failure answers `{"error": {"code", "message"}}` with an HTTP status that fits. Ids are numbers, except source ids, which are 64-bit and travel as strings. Times are seconds since 1970 unless a field says otherwise. Field names are camelCase.

**Admins.** Accounts, invitations and the solver need an admin. Who may set the proxy depends on `MANGO_PROXY_SETTING`. Bringing in an extension that no account has yet needs an admin unless `MANGO_ANYONE_INSTALLS` is on. Feature doc 09 has the details.

A test in `api.rs` checks that every route the server answers is listed here.

## Account and sign-in

| Route | What it does |
|---|---|
| `GET /api/auth` | Whether login is on, who is signed in, and whether a password is set yet |
| `POST /api/auth/setup` | Sets the first password |
| `POST /api/auth/login` | Signs in: `{name, password, code}`; sets the session cookie |
| `POST /api/auth/signup` | Makes an account from an invitation and signs it in |
| `POST /api/auth/logout` | Ends this session |
| `PUT /api/auth/password` | Changes the password: `{current, password}` |
| `GET /api/auth/two-step` | Whether two-step sign-in is on |
| `POST /api/auth/two-step` | Starts turning it on: answers the secret to scan |
| `PUT /api/auth/two-step` | Confirms it with a code from the app |
| `DELETE /api/auth/two-step` | Turns it off |
| `POST /api/auth/two-step/codes` | New one-time recovery codes |
| `GET /api/auth/apps` | App passwords and API tokens |
| `POST /api/auth/apps` | Makes one: `{name, api}`; the answer carries it, this once |
| `DELETE /api/auth/apps/{id}` | Revokes one |
| `GET /api/auth/devices` | Browsers signed in to this account |
| `DELETE /api/auth/devices/{id}` | Signs one of them out |
| `GET /api/users` | Accounts (admin) |
| `POST /api/users` | Adds an account: `{name, password, admin}` (admin) |
| `PUT /api/users/{id}` | Changes an account (admin) |
| `DELETE /api/users/{id}` | Removes an account with its library (admin) |
| `GET /api/invites` | Invitations (admin) |
| `POST /api/invites` | Makes an invitation; the answer carries its code, this once (admin) |
| `DELETE /api/invites/{id}` | Withdraws one (admin) |

## Server

| Route | What it does |
|---|---|
| `GET /api/health` | Answers when the server is up |
| `GET /api/version` | This Mango's version, and the newest release when one was found |
| `POST /api/version/check` | Asks the release list now |
| `GET /api/settings` | The account's settings, as one object |
| `PUT /api/settings` | Sets the keys given; `null` removes one; keys not named stay |
| `GET /api/solver` | The address Cloudflare checks are sent to (admin) |
| `PUT /api/solver` | Sets it (admin) |
| `GET /api/proxy` | The proxy sources reach sites through; the password is never sent back |
| `PUT /api/proxy` | Sets it |
| `GET /api/stats` | What the reading adds up to; `?tz=` is minutes the reader's clock is ahead of UTC |

## Stores, extensions and sources

| Route | What it does |
|---|---|
| `GET /api/stores` | The account's stores |
| `POST /api/stores` | Adds one: `{url}` |
| `POST /api/stores/refresh` | Reads every store's list again |
| `DELETE /api/stores/{id}` | Removes a store |
| `POST /api/stores/{id}/refresh` | Reads one store's list again |
| `POST /api/stores/{id}/accept-key` | Accepts a store's new signing key |
| `GET /api/catalog` | Extensions the stores offer; `?q=`, `?lang=`, `?content=`, `?state=` |
| `GET /api/catalog/{store}/{pkg}` | One extension in a store's list |
| `GET /api/extensions` | Installed extensions |
| `POST /api/extensions/update-all` | Updates every extension with a newer version |
| `POST /api/extensions/{pkg}/install` | Installs, or updates to the store's version; `?store=` when several list it |
| `DELETE /api/extensions/{pkg}` | Removes an extension |
| `GET /api/sources` | Installed sources |
| `PUT /api/sources/{id}` | Pins, hides or files a source: `{pinned, hidden, folder}` |
| `GET /api/sources/{id}/preferences` | What a source lets its user set |
| `PUT /api/sources/{id}/preferences` | Changes one: `{position, key, value}` (admin where only admins bring in extensions) |
| `GET /api/sources/{id}/filters` | What a source's search can be narrowed by |
| `GET /api/sources/{id}/{kind}` | `popular`, `latest` or `search`; `?page=`, `?q=`, `?filters=` |

## Titles and chapters

| Route | What it does |
|---|---|
| `GET /api/manga/{id}` | A title with its chapters |
| `POST /api/manga/{id}/refresh` | Asks the source for the title and its chapters again |
| `GET /api/manga/{id}/cover` | The cover image |
| `PUT /api/manga/{id}/cover` | Sets a cover by hand; the body is the image itself |
| `PUT /api/manga/{id}/details` | Edits the details; edited fields stop following the source |
| `DELETE /api/manga/{id}/details` | Lets the details follow the source again |
| `PUT /api/manga/{id}/scanlators` | Group order, duplicates, and following one group |
| `POST /api/manga/{id}/merge/{other}` | Merges another title into this one |
| `DELETE /api/manga/{id}/merge/{other}` | Takes a merged title out again |
| `GET /api/manga/{id}/namesakes` | Library titles by the same name |
| `GET /api/manga/{id}/marks` | The title's page notes |
| `PUT /api/manga/{id}/library` | Adds the title to the library or takes it out: `{inLibrary}` |
| `PUT /api/manga/{id}/reader` | The title's reading mode and fit |
| `PUT /api/manga/{id}/chapters` | Marks chapters read, unread or bookmarked |
| `PUT /api/manga/{id}/categories` | The title's categories: `{ids}` |
| `PUT /api/manga/{id}/auto-download` | Whether new chapters download by themselves |
| `GET /api/manga/{id}/migrate/{to}` | The title beside the one it would move to |
| `POST /api/manga/{id}/migrate/{to}` | Moves or copies the title's place, progress and links |
| `GET /api/chapters/{id}` | What the reader needs to open a chapter |
| `GET /api/chapters/{id}/pages` | How many pages it has |
| `POST /api/chapters/{id}/pages/reload` | Fetches the page list again from the source |
| `GET /api/chapters/{id}/pages/{index}` | One page image; `?light=1` for a smaller JPEG |
| `PUT /api/chapters/{id}/progress` | Saves where the reader is: `{page, read}` |
| `PUT /api/chapters/{id}/marks/{page}` | Sets a note on a page |
| `DELETE /api/chapters/{id}/marks/{page}` | Removes it |
| `GET /api/chapters/{id}/file` | The chapter as a CBZ file |
| `DELETE /api/chapters/{id}/download` | Deletes a downloaded chapter's file |

## Library, history and updates

| Route | What it does |
|---|---|
| `GET /api/library` | Every library title with its counts, and the categories |
| `POST /api/library/bulk` | One change to many titles at once: `{action, ids}` |
| `GET /api/categories` | The categories |
| `POST /api/categories` | Adds one: `{name, parent}` |
| `PUT /api/categories` | Puts them in a new order |
| `PUT /api/categories/{id}` | Renames one |
| `DELETE /api/categories/{id}` | Removes one |
| `GET /api/history` | Chapters read, newest first |
| `DELETE /api/history` | Clears the history |
| `DELETE /api/history/{chapter}` | Forgets one entry |
| `GET /api/updates` | New chapters of library titles, and the last run's state |
| `GET /api/updates/fresh` | Only the number of new chapters |
| `POST /api/updates/run` | Starts a library update |
| `DELETE /api/updates/run` | Stops the one going on |

## Downloads and storage

| Route | What it does |
|---|---|
| `GET /api/downloads` | The queue |
| `POST /api/downloads` | Queues chapters, or titles' unread chapters: `{chapters, titles}` |
| `DELETE /api/downloads` | Empties the queue |
| `PUT /api/downloads/order` | Puts the queue in a new order |
| `POST /api/downloads/pause` | Pauses the queue |
| `POST /api/downloads/resume` | Resumes it |
| `POST /api/downloads/retry` | Tries again every chapter that gave up |
| `DELETE /api/downloads/{chapter}` | Takes one chapter out of the queue |
| `POST /api/downloads/{chapter}/retry` | Tries one chapter again |
| `GET /api/downloads/orphans` | Titles outside the library with chapters saved |
| `DELETE /api/downloads/orphans` | Deletes those files |
| `GET /api/downloads/read` | Saved chapters already read, and their size |
| `DELETE /api/downloads/read` | Deletes those files |
| `GET /api/cache` | What the account's page cache takes |
| `DELETE /api/cache` | Empties it |

## Import, export and backups

| Route | What it does |
|---|---|
| `POST /api/import/preview` | What a backup holds; the body is the `.tachibk` file itself |
| `POST /api/import` | Starts importing the body, or with no body the file just previewed |
| `GET /api/import` | How the import is going, and its report |
| `GET /api/export` | The library as a `.tachibk` |
| `GET /api/export/files` | Every downloaded chapter as one zip |
| `GET /api/export/files/size` | How big that zip would be |
| `GET /api/backups` | Backups kept on the server |
| `POST /api/backups` | Makes one now |
| `GET /api/backups/{name}` | One backup file |
| `DELETE /api/backups/{name}` | Deletes one |

## Trackers and notifications

| Route | What it does |
|---|---|
| `GET /api/trackers` | The trackers and whether the account is signed in |
| `POST /api/trackers/{id}/authorize` | Starts a sign-in through the tracker's own page |
| `POST /api/trackers/{id}/login` | Signs in, or finishes a sign-in |
| `DELETE /api/trackers/{id}/login` | Signs out |
| `GET /api/trackers/{id}/search` | Searches the tracker: `?q=` |
| `GET /api/manga/{id}/tracks` | A title's tracker links |
| `POST /api/manga/{id}/tracks/{tracker}` | Links the title to an entry |
| `PUT /api/manga/{id}/tracks/{tracker}` | Changes the entry's status, score or dates |
| `DELETE /api/manga/{id}/tracks/{tracker}` | Removes the link |
| `POST /api/manga/{id}/tracks/{tracker}/refresh` | Reads the entry from the tracker again |
| `POST /api/notify/test` | Sends a message to the notification address |
| `GET /api/notify/header` | Whether a header goes with notifications, and its name |
| `PUT /api/notify/header` | Sets it: `{header: "Name: value"}`; empty takes it away |
| `GET /api/push` | The key browsers subscribe with, and how many devices hear |
| `PUT /api/push` | Adds a device's subscription |
| `DELETE /api/push` | Removes it |
| `POST /api/push/test` | Sends a note to every device |
