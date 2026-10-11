#!/usr/bin/env python3
"""Smoke test for a running Mango core.

    python3 smoke.py http://127.0.0.1:<port> [path to mango.db]

Sets the password on a new Mango (or signs in with $MANGO_PASSWORD), adds a second
account, reads the library through the OPDS catalog, looks at the version, adds an extension store, searches its catalog, installs an extension through the
extension host, browses a source, opens a title and reads a page, puts it in the
library, downloads a chapter, runs a library update, imports a backup, exports one, asks
the trackers what can be asked without an account and moves a title to another source, then waits for the host to
fall asleep again and removes the store and the extension. Given the database
path, which must be on this machine, it also looks at the files Mango writes and
plays through a new chapter appearing and a store changing its signing key.

Run it against a fresh data folder, and start Mango with MANGO_HOST_IDLE_SECS=10
so the sleep check is quick.

The store and the extension it tries are not named here. They come from the
environment:

    MANGO_SMOKE_STORE       the address of a Mihon extension store's repo.json
    MANGO_SMOKE_STORE_LINK  the same store, by the link to its index.pb
    MANGO_SMOKE_PKG         the package of an extension in that store whose sources
                            come in several languages, English among them
"""
import base64
import glob
import gzip
import http.cookiejar
import io
import json
import os
import shutil
import sqlite3
import sys
import threading
import time
import urllib.error
import urllib.request
import xml.etree.ElementTree
import zipfile

try:
    STORE = os.environ["MANGO_SMOKE_STORE"]
    # The same store, by the link to its index: the form in which store addresses are passed around.
    STORE_LINK = os.environ["MANGO_SMOKE_STORE_LINK"]
    TEST_PKG = os.environ["MANGO_SMOKE_PKG"]
except KeyError as missing:
    sys.exit(f"Set {missing.args[0]}: the top of this file says what goes in it.")
# What the catalog is searched for: the last part of the package's name.
WORD = TEST_PKG.rsplit(".", 1)[-1]

site = sys.argv[1].rstrip("/")
base = site + "/api"
db_path = sys.argv[2] if len(sys.argv) > 2 else None
data_dir = os.path.dirname(os.path.abspath(db_path)) if db_path else None
failures = []
PASSWORD = os.environ.get("MANGO_PASSWORD", "smoke-test-password")
# Keeps the session cookie between calls, as a browser would.
jar = http.cookiejar.CookieJar()
opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))


def raw(method, url, body=None, headers=None):
    """One request. Answers with the status, the headers, the body and the time it took."""
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(url, data=data, method=method, headers=headers or {})
    if data:
        req.add_header("Content-Type", "application/json")
    start = time.time()
    try:
        with opener.open(req, timeout=300) as res:
            status, got, payload = res.status, res.headers, res.read()
    except urllib.error.HTTPError as err:
        status, got, payload = err.code, err.headers, err.read()
    return status, got, payload, int((time.time() - start) * 1000)


def call(method, path, body=None, expect=200, headers=None):
    status, _, payload, ms = raw(method, base + path, body, headers)
    if status != expect:
        sys.exit(f"FAIL {method} {path}: HTTP {status}, wanted {expect}: {payload[:300].decode(errors='replace')}")
    return json.loads(payload), ms


def check(what, ok):
    print(("  ok    " if ok else "  FAIL  ") + what)
    if not ok:
        failures.append(what)


def host():
    return call("GET", "/health")[0]["host"]


def online_sources():
    """The sources extensions provide. The one with id 0 reads local files, is always there, and is left out."""
    return [s for s in call("GET", "/sources")[0]["sources"] if s["id"] != "0"]


def wait_asleep(limit=90):
    start = time.time()
    while time.time() - start < limit:
        if host()["state"] == "asleep":
            return int(time.time() - start)
        time.sleep(1)
    return None


print("login")
who, _ = call("GET", "/auth")
if who["login"]:
    if who["setup"]:
        err, _ = call("GET", "/library", expect=401)
        check("nothing answers before a password is set", err["error"]["code"] == "setup_required")
        call("POST", "/auth/setup", {"password": "short"}, expect=400)
        code = os.environ.get("MANGO_SETUP_CODE", "")
        if who["setupCode"]:
            # Mango is not on this machine, or sits behind a published port: it keeps the code in its data folder.
            if not code and data_dir:
                with open(os.path.join(data_dir, "setup-code"), encoding="utf-8") as kept:
                    code = kept.read().strip()
            err, _ = call("POST", "/auth/setup", {"password": PASSWORD, "code": "0000-0000-0000-0000"}, expect=403)
            check("from another machine, the first password takes the setup code", err["error"]["code"] == "setup_code" and len(code) == 19)
        call("POST", "/auth/setup", {"password": PASSWORD, "code": code})
        check("setting the password signs in", call("GET", "/auth")[0]["signedIn"])
        err, _ = call("POST", "/auth/setup", {"password": PASSWORD + "2"}, expect=403)
        check("it can be set only once", err["error"]["code"] == "already_set")
        call("POST", "/auth/logout")
    err, _ = call("GET", "/library", expect=401)
    check("signed out, the API is closed", err["error"]["code"] == "signed_out")
    check("the page itself still loads, to show the sign-in", raw("GET", site + "/")[0] == 200)
    err, _ = call("POST", "/auth/login", {"password": "not the password"}, expect=401)
    check("a wrong password is refused", err["error"]["code"] == "wrong_password")
    call("POST", "/auth/login", {"password": PASSWORD})
    check("the right one opens it", call("GET", "/auth")[0]["signedIn"] and "items" in call("GET", "/library")[0])
    check("the session cookie is kept from scripts", all(c.has_nonstandard_attr("HttpOnly") for c in jar) and len(jar) == 1)
    call("PUT", "/auth/password", {"current": "not the password", "password": PASSWORD + "!"}, expect=401)
    call("PUT", "/auth/password", {"current": PASSWORD, "password": PASSWORD + "!"})
    call("PUT", "/auth/password", {"current": PASSWORD + "!", "password": PASSWORD})
    check("the password can be changed, and the session stays", call("GET", "/auth")[0]["signedIn"])
else:
    print("  login is switched off")

me, _ = call("GET", "/auth")
# Unless the owner opened it to everyone, only an admin brings an extension into the host.
restricted = not call("GET", "/health")[0]["anyoneInstalls"]
check("answers of the API are kept nowhere", raw("GET", base + "/library")[1].get("Cache-Control") == "no-store")
KID, KID_PASSWORD = "smoke-kid", PASSWORD + "-kid"
kid = None
if who["login"]:
    print("accounts")
    check("the first account is an admin and signs in without a name", me["admin"] and me["name"] and not me["named"])
    kid, _ = call("POST", "/users", {"name": KID, "password": KID_PASSWORD})
    check("an admin adds an account", kid["id"] > 1 and not kid["admin"] and call("GET", "/auth")[0]["named"])
    err, _ = call("POST", "/users", {"name": KID.upper(), "password": KID_PASSWORD}, expect=409)
    check("a name is taken once", err["error"]["code"] == "name_taken")
    call("DELETE", "/users/1", expect=400)
    # A second browser, for the new account.
    owner, opener = opener, urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
    call("POST", "/auth/login", {"password": KID_PASSWORD}, expect=401)
    call("POST", "/auth/login", {"name": KID.upper(), "password": KID_PASSWORD})
    mine, _ = call("GET", "/auth")
    check("the new account signs in by name", mine["signedIn"] and mine["name"] == KID and not mine["admin"])
    err, _ = call("PUT", "/solver", {"url": ""}, expect=403)
    check("it cannot change what all accounts share", err["error"]["code"] == "admin_only" and raw("GET", base + "/users")[0] == 403)
    call("PUT", "/settings", {"smoke.kid": 1})
    kid_browser, opener = opener, owner
    check("its settings are its own", "smoke.kid" not in call("GET", "/settings")[0])
    if data_dir:
        check("and so is its database", os.path.isfile(os.path.join(data_dir, "users", str(kid["id"]), "mango.db")))
    if not me["signup"]:
        # Unless the owner switched it on, nobody makes an account for themselves, and there is nothing to invite to.
        newcomer = {"name": "smoke-newcomer", "password": KID_PASSWORD, "code": "0000-0000-0000-0000"}
        err, _ = call("POST", "/auth/signup", newcomer, expect=403)
        check("nobody signs themselves up", err["error"]["code"] == "signup_off")
        err, _ = call("POST", "/invites", {"note": "smoke"}, expect=403)
        check("and no invitation is made", err["error"]["code"] == "signup_off" and call("GET", "/invites")[0]["invites"] == [])


def as_kid(path):
    """A call the second account makes the way a reading app does: name and password in the header."""
    return opds(path, KID, KID_PASSWORD)


def opds(path, name=None, password=PASSWORD):
    """One call to the OPDS catalog. Answers with the status, the headers, the body and the address it ended at."""
    headers = {}
    if who["login"] and name != "":
        headers["Authorization"] = "Basic " + base64.b64encode(f"{name or me['name']}:{password}".encode()).decode()
    req = urllib.request.Request(site + "/opds" + path, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=300) as res:
            return res.status, res.headers, res.read(), res.geturl()
    except urllib.error.HTTPError as err:
        return err.code, err.headers, err.read(), ""

print("version")
about, _ = call("GET", "/version")
print(f"  Mango {about['current']} {about['commit']} on {about['platform']}")
check("it knows its version and platform", about["current"] == call("GET", "/health")[0]["version"] and "-" in about["platform"])
if about["canCheck"]:
    checked, ms = call("POST", "/version/check")
    print(f"  release list asked in {ms} ms: {checked['error'] or str(len(checked['releases'])) + ' release(s)'}")
    check("the answer is kept", checked["checkedAt"] and call("GET", "/version")[0]["checkedAt"] == checked["checkedAt"])
else:
    err, _ = call("POST", "/version/check", expect=400)
    check("a build that knows no release list says so", "MANGO_RELEASES_REPO" in err["error"]["message"])

# Leftovers from an earlier run.
if any(e["pkg"] == TEST_PKG for e in call("GET", "/extensions")[0]["extensions"]):
    call("DELETE", f"/extensions/{TEST_PKG}")
for old in call("GET", "/stores")[0]["stores"]:
    # The same owner's store, whichever of its addresses it was added by.
    if old["indexUrl"].split("/")[3:4] == STORE.split("/")[3:4]:
        call("DELETE", f"/stores/{old['id']}")
if host()["state"] != "asleep":
    wait_asleep()

print("store")
store, ms = call("POST", "/stores", {"url": STORE})
print(f"  added {store['name']}: {store['extensions']} extensions in {ms} ms")
check("the store has a catalog and a pinned key", store["extensions"] > 1000 and len(store["signingKey"]) == 64)
err, _ = call("POST", "/stores", {"url": STORE}, expect=409)
check("adding it twice is refused", err["error"]["code"] == "store_exists")
call("POST", "/stores", {"url": "file:///etc/hostname"}, expect=400)
call("POST", "/stores", {"url": "https://example.org/"}, expect=502)
refused = [call("POST", "/stores", {"url": url}, expect=400)[0]["error"]["code"] for url in ("http://example.org/repo.json", "https://127.0.0.1/repo.json", "https://localhost/repo.json")]
check("a store is fetched over HTTPS, and not from this machine or its network", refused == ["address_refused"] * 3)
err, _ = call("POST", "/stores/refresh", expect=403, headers={"Sec-Fetch-Site": "cross-site"})
check("a request from another site is refused", err["error"]["code"] == "cross_site")
# Over plain HTTP a browser leaves that label off, and says only where the request comes from.
err, _ = call("POST", "/stores/refresh", expect=403, headers={"Origin": "http://other.example"})
check("and so is one the browser did not label", err["error"]["code"] == "cross_site")
refreshed, ms = call("POST", f"/stores/{store['id']}/refresh")
print(f"  refresh with nothing new: {ms} ms")
check("a refresh keeps the catalog", refreshed["extensions"] == store["extensions"] and not refreshed["error"])

print("catalog")
found, ms = call("GET", f"/catalog?q={WORD}")
print(f"  search: {found['total']} found in {ms} ms")
check("finds the extension by name", any(i["pkg"] == TEST_PKG for i in found["items"]))
default, _ = call("GET", "/catalog?limit=500")
everything, _ = call("GET", "/catalog?content=safe,mixed,nsfw&limit=1")
nsfw, _ = call("GET", "/catalog?content=nsfw&limit=1")
check(
    "NSFW is hidden unless asked for",
    all(i["contentWarning"] < 3 for i in default["items"])
    and nsfw["total"] > 0
    and default["total"] + nsfw["total"] == everything["total"] == store["extensions"],
)
english, _ = call("GET", "/catalog?lang=en&limit=500")
check("language filter", english["total"] > 0 and all("en" in i["languages"] for i in english["items"]))
detail, _ = call("GET", f"/catalog/{store['id']}/{TEST_PKG}")
check("an extension lists its sources", len(detail["sources"]) == detail["sourceCount"] > 1)
check("all of that left the JVM asleep", host()["state"] == "asleep")

print("install")
ext, ms = call("POST", f"/extensions/{TEST_PKG}/install")
print(f"  {ext['name']} {ext['versionName']}, {len(ext['sources'])} sources, in {ms} ms from a sleeping host")
check("the host is running", host()["state"] == "running")
installed, _ = call("GET", "/extensions")
check(
    "it is listed as installed and up to date",
    [e["pkg"] for e in installed["extensions"]] == [TEST_PKG] and not installed["extensions"][0]["updateAvailable"],
)
sources = {"sources": online_sources()}
check("its sources are listed", len(sources["sources"]) == len(ext["sources"]) > 1)
found, _ = call("GET", f"/catalog?q={WORD}&state=installed")
check("the catalog marks it installed", found["total"] == 1 and found["items"][0]["installed"])
_, ms = call("POST", f"/extensions/{TEST_PKG}/install")
print(f"  again, host awake: {ms} ms")
updates, _ = call("POST", "/extensions/update-all")
check("nothing to update", updates["results"] == [])

print("browse")
sid = next(s["id"] for s in sources["sources"] if s["lang"] == "en")
popular, ms = call("GET", f"/sources/{sid}/popular")
print(f"  popular: {len(popular['items'])} titles in {ms} ms")
check("popular lists titles with covers", len(popular["items"]) > 5 and all(i["cover"] for i in popular["items"][:5]))
found, ms = call("GET", f"/sources/{sid}/search?q=one")
print(f"  search: {len(found['items'])} titles in {ms} ms")
check("search finds titles", len(found["items"]) > 0)
call("GET", f"/sources/{sid}/sideways", expect=404)
card = popular["items"][0]
status, got, cover, ms = raw("GET", f"{site}/{card['cover']}")
print(f"  cover: {len(cover)} bytes in {ms} ms")
check("a cover comes through as an image", status == 200 and got["Content-Type"].startswith("image/") and len(cover) > 1000)
title, _ = call("GET", f"/manga/{card['id']}")
check("a title seen in a list is stored, without chapters yet", title["title"] == card["title"] and title["chapters"] == [])
title, ms = call("POST", f"/manga/{card['id']}/refresh")
print(f"  title: {len(title['chapters'])} chapters in {ms} ms")
check("opening it brings details and chapters", title["initialized"] and len(title["chapters"]) > 0 and title["sourceName"])
call("PUT", f"/manga/{card['id']}/library", {"inLibrary": True})
again, _ = call("GET", f"/sources/{sid}/popular")
check("lists mark it once it is in the library", next(i for i in again["items"] if i["id"] == card["id"])["inLibrary"])
call("PUT", f"/sources/{sid}", {"pinned": True})
check("a pinned source stays pinned", next(s for s in call("GET", "/sources")[0]["sources"] if s["id"] == sid)["pinned"])
prefs, ms = call("GET", f"/sources/{sid}/preferences")
prefs = prefs["preferences"]
print(f"  preferences: {len(prefs)} in {ms} ms")
switch = next((p for p in prefs if p["type"] == "switch" and p["enabled"] and p["key"]), None)
check("a source says what it lets its user set", switch is not None)
if switch:
    change = {"position": switch["position"], "key": switch["key"]}
    after, _ = call("PUT", f"/sources/{sid}/preferences", {**change, "value": not switch["value"]})
    check("a preference changes", after["preferences"][switch["position"]]["value"] != switch["value"])
    check("and the source still answers", len(call("GET", f"/sources/{sid}/popular")[0]["items"]) > 5)
    call("PUT", f"/sources/{sid}/preferences", {**change, "value": switch["value"]})
    err, _ = call("PUT", f"/sources/{sid}/preferences", {**change, "key": "another", "value": True}, expect=400)
    check("a change meant for another entry is refused", err["error"]["code"] == "bad_request")
check("the local source has nothing to set", call("GET", "/sources/0/preferences")[0]["preferences"] == [])
call("GET", "/sources/12345/preferences", expect=404)

print("read")
# Some chapters only link to a publisher's site and have no pages; take the first that has.
for chapter in reversed(title["chapters"][-8:]):
    status, _, payload, ms = raw("GET", f"{base}/chapters/{chapter['id']}/pages")
    if status == 200:
        count = json.loads(payload)["count"]
        break
else:
    sys.exit("FAIL none of the first eight chapters has pages")
cid = chapter["id"]
print(f"  page list: {count} pages in {ms} ms")
status, got, image, ms = raw("GET", f"{base}/chapters/{cid}/pages/0")
print(f"  first page: {len(image)} bytes in {ms} ms")
check("a page comes through as an image", status == 200 and got["Content-Type"].startswith("image/") and len(image) > 1000)
status, _, same, ms = raw("GET", f"{base}/chapters/{cid}/pages/0")
print(f"  the same page again: {ms} ms")
check("a page fetched once is served from disk", status == 200 and same == image and ms < 200)
check("a page past the end is refused", raw("GET", f"{base}/chapters/{cid}/pages/{count}")[0] == 404)
call("PUT", f"/chapters/{cid}/progress", {"page": 3})
reader, _ = call("GET", f"/chapters/{cid}")
check(
    "the reader gets its place back",
    reader["chapter"]["lastPageRead"] == 3 and reader["chapter"]["pageCount"] == count and not reader["chapter"]["read"],
)
check("and knows the chapters around it", len(reader["chapters"]) == len(title["chapters"]) and reader["mangaId"] == card["id"])
call("PUT", f"/chapters/{cid}/progress", {"page": count - 1, "read": True})
check("a finished chapter is marked read", call("GET", f"/chapters/{cid}")[0]["chapter"]["read"])
call("PUT", f"/manga/{card['id']}/reader", {"mode": "webtoon", "fit": "screen", "prefs": {"contrast": 120, "kind": "manhwa"}})
call("PUT", f"/manga/{card['id']}/reader", {"mode": "sideways"}, expect=400)
call("PUT", f"/manga/{card['id']}/reader", {"prefs": [1]}, expect=400)
kept = call("GET", f"/chapters/{cid}")[0]
check("the reading mode is kept for the title", kept["readerMode"] == "webtoon" and kept["readerFit"] == "screen")
check(
    "and so are its other reader settings, beside the kind of series it looks like",
    kept["readerPrefs"] == {"contrast": 120, "kind": "manhwa"} and kept["kind"] in ("manga", "manhwa", "manhua", "comic"),
)
call("PUT", f"/manga/{card['id']}/chapters", {"read": False})
check("mark all unread", not any(c["read"] for c in call("GET", f"/manga/{card['id']}")[0]["chapters"]))
print("library")
library, ms = call("GET", "/library")
print(f"  library: {ms} ms")
check(
    "the library lists the title with its counts",
    [i["id"] for i in library["items"]] == [card["id"]]
    and library["items"][0]["total"] == library["items"][0]["unread"] == len(title["chapters"]),
)
check("it knows where to go on reading", library["resume"] and library["resume"]["chapterId"] == cid)
category, _ = call("POST", "/categories", {"name": "Reading"})
err, _ = call("POST", "/categories", {"name": "reading"}, expect=409)
check("a category name is taken once", err["error"]["code"] == "category_exists")
call("PUT", f"/manga/{card['id']}/categories", {"ids": [category["id"]]})
check(
    "a title can be put in a category",
    call("GET", "/library")[0]["items"][0]["categories"] == [category["id"]]
    and call("GET", f"/manga/{card['id']}")[0]["categories"] == [category["id"]],
)
call("POST", "/library/bulk", {"ids": [card["id"]], "action": "read"})
check("several titles can be marked read at once", call("GET", "/library")[0]["items"][0]["unread"] == 0)
call("POST", "/library/bulk", {"ids": [card["id"]], "action": "unread"})
call("POST", "/library/bulk", {"ids": [card["id"]], "action": "sing"}, expect=400)
entries = call("GET", "/history")[0]["entries"]
check("the history has the chapter that was read", [e["chapterId"] for e in entries] == [cid] and entries[0]["mangaId"] == card["id"])
call("DELETE", f"/history/{cid}")
check("a history entry can be removed", call("GET", "/history")[0]["entries"] == [])
call("DELETE", f"/categories/{category['id']}")
check(
    "removing a category keeps its titles",
    len(call("GET", "/library")[0]["items"]) == 1 and call("GET", "/categories")[0]["categories"] == [],
)

print("downloads")
call("POST", "/downloads/pause")
queued, _ = call("POST", "/downloads", {"chapters": [cid]})
waiting = call("GET", "/downloads")[0]
check("a queued chapter waits while the queue is paused", queued["queued"] == 1 and waiting["paused"] and waiting["queue"][0]["state"] == "queued")
check("the same chapter is not queued twice", call("POST", "/downloads", {"chapters": [cid]})[0]["queued"] == 0)
call("POST", "/downloads/resume")
start = time.time()
while call("GET", "/downloads")[0]["queue"] and time.time() - start < 240:
    time.sleep(1)
print(f"  {count} pages saved in {int(time.time() - start)} s")
saved = next(c for c in call("GET", f"/manga/{card['id']}")[0]["chapters"] if c["id"] == cid)
check("the chapter is saved and leaves the queue", saved["downloaded"] and saved["pageCount"] == count)
if data_dir:
    files = glob.glob(os.path.join(data_dir, "downloads", "*", "*", "*.cbz"))
    names = zipfile.ZipFile(files[0]).namelist() if files else []
    check("it is one CBZ with its pages and a ComicInfo.xml", len(files) == 1 and len(names) == count + 1 and "ComicInfo.xml" in names)

print("opds")
ATOM = "{http://www.w3.org/2005/Atom}"
if who["login"]:
    status, got, _, _ = opds("/v1.2/catalog", name="")
    check("a reading app is asked for a name and password", status == 401 and got.get("WWW-Authenticate", "").startswith("Basic"))
    check("and a wrong one is refused", opds("/v1.2/catalog", password="not the password")[0] == 401)
status, got, feed, landed = opds("")
catalog = xml.etree.ElementTree.fromstring(feed)
check(
    "the catalog opens with the library and its parts",
    status == 200 and landed.endswith("/opds/v1.2/catalog") and "kind=navigation" in got["Content-Type"] and len(catalog.findall(ATOM + "entry")) >= 2,
)
feed = xml.etree.ElementTree.fromstring(opds("/v1.2/library")[2])
entries = {e.find(ATOM + "id").text: e for e in feed.findall(ATOM + "entry")}
links = [l.get("href") for l in entries.get(f"urn:mango:manga:{card['id']}", [])]
check("the library lists the title with its cover", f"manga/{card['id']}" in links and f"manga/{card['id']}/cover" in links)
check("a search finds nothing it should not", not xml.etree.ElementTree.fromstring(opds("/v1.2/library?q=zzzzqqqq")[2]).findall(ATOM + "entry"))
status, got, picture, _ = opds(f"/v1.2/manga/{card['id']}/cover")
check("the cover comes through", status == 200 and picture == cover)
feed = xml.etree.ElementTree.fromstring(opds(f"/v1.2/manga/{card['id']}")[2])
entries = {e.find(ATOM + "id").text: e for e in feed.findall(ATOM + "entry")}
links = {l.get("rel"): l for l in entries[f"urn:mango:chapter:{cid}"].findall(ATOM + "link")}
stream = links.get("http://vaemendis.net/opds-pse/stream")
check(
    "a title lists its chapters, to fetch whole or page by page",
    len(entries) == len(title["chapters"])
    and links["http://opds-spec.org/acquisition"].get("href") == f"../chapters/{cid}/file"
    and stream is not None
    and stream.get("{http://vaemendis.net/opds-pse/ns}count") == str(count),
)
status, got, packed, _ = opds(f"/v1.2/chapters/{cid}/file")
names = zipfile.ZipFile(io.BytesIO(packed)).namelist() if status == 200 else []
check("a saved chapter is handed over as its CBZ", len(names) == count + 1 and ".cbz" in got.get("Content-Disposition", ""))
status, got, first, _ = opds(f"/v1.2/chapters/{cid}/pages/0")
check("and page by page", status == 200 and got["Content-Type"].startswith("image/") and len(first) > 1000)
check("the search description names the library feed", b"/opds/v1.2/library?q={searchTerms}" in opds("/v1.2/search.xml")[2])
if kid:
    theirs = xml.etree.ElementTree.fromstring(as_kid("/v1.2/library")[2])
    check("another account's catalog is its own, empty library", not theirs.findall(ATOM + "entry") and as_kid(f"/v1.2/manga/{card['id']}")[0] == 404)

    print("stores of its own")
    owner, opener = opener, kid_browser
    check(
        "a new account has no store, no extension and only the local source",
        call("GET", "/stores")[0]["stores"] == [] and call("GET", "/extensions")[0]["extensions"] == [] and online_sources() == [],
    )
    call("POST", f"/extensions/{TEST_PKG}/install", expect=404)
    theirs, _ = call("POST", "/stores", {"url": STORE_LINK})
    check("it adds a store by the link to its index.pb", theirs["extensions"] > 1000 and theirs["signingKey"] == store["signingKey"])
    err, _ = call("POST", "/stores", {"url": STORE}, expect=409)
    check("the same store under its other address is known", err["error"]["code"] == "store_exists")
    call("POST", f"/extensions/{TEST_PKG}/install")
    check("it installs what the first account has too", len(online_sources()) == len(ext["sources"]))
    check("and browses with it", len(call("GET", f"/sources/{sid}/popular")[0]["items"]) > 5)
    if restricted:
        fresh = next(i["pkg"] for i in call("GET", "/catalog?content=safe&limit=30")[0]["items"] if i["installable"] and i["pkg"] != TEST_PKG)
        err, _ = call("POST", f"/extensions/{fresh}/install", expect=403)
        check(
            "a reader does not bring in an extension no account has, nor see or change a source's settings",
            err["error"]["code"] == "admin_only"
            and call("GET", f"/sources/{sid}/preferences")[0] == {"preferences": []}
            and call("PUT", f"/sources/{sid}/preferences", {"position": 0, "key": "x", "value": "y"}, expect=403)[0]["error"]["code"] == "admin_only",
        )

    print("two accounts, one extension")

    def browse(browser, answers):
        for page in (1, 2, 3):
            try:
                with browser.open(f"{base}/sources/{sid}/popular?page={page}", timeout=120) as res:
                    answers.append(len(json.loads(res.read())["items"]))
            except Exception as why:
                answers.append(str(why))

    answers = ([], [])
    threads = [threading.Thread(target=browse, args=pair) for pair in zip((owner, kid_browser), answers)]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    check("both browse the same source at the same moment", all(isinstance(n, int) and n > 0 for n in answers[0] + answers[1]) and len(answers[0]) == len(answers[1]) == 3)
    check("and what one of them saw is not in the other's library", call("GET", "/library")[0]["items"] == [])

    installed_as = lambda: next(e for e in call("GET", "/extensions")[0]["extensions"] if e["pkg"] == TEST_PKG)
    if db_path:
        kid_db = os.path.join(data_dir, "users", str(kid["id"]), "mango.db")
        real = installed_as()

        def change(path, sql, *values):
            db = sqlite3.connect(path, timeout=10)
            db.execute(sql, values)
            db.commit()
            db.close()

        # The second account's store names another key than the extension is signed with.
        change(kid_db, "UPDATE stores SET signing_key = ?", "0" * 64)
        err, _ = call("POST", f"/extensions/{TEST_PKG}/install", expect=422)
        check("an account's own store key decides, though the host has the extension", err["error"]["code"] == "signing_key_mismatch")
        change(kid_db, "UPDATE stores SET signing_key = ?", store["signingKey"])

        # Both accounts' lists say an older version than the host has, and the second account updates.
        for path in (db_path, kid_db):
            change(path, "UPDATE extensions SET version_code = ?, version_name = 'old' WHERE pkg = ?", real["versionCode"] - 1, TEST_PKG)
        results, _ = call("POST", "/extensions/update-all")
        check(
            "one account's update takes the version the host has",
            [(r["pkg"], r["versionName"], r["error"]) for r in results["results"]] == [(TEST_PKG, real["versionName"], None)]
            and len(call("GET", f"/sources/{sid}/popular")[0]["items"]) > 5,
        )
        opener = owner
        mine = installed_as()
        check(
            "and the other account has that version too, and goes on working",
            (mine["versionCode"], mine["versionName"]) == (real["versionCode"], real["versionName"])
            and len(call("GET", f"/sources/{sid}/popular")[0]["items"]) > 5,
        )

        # The index is not signed: one that lists an older version than it did before is not followed.
        change(db_path, "UPDATE seen_versions SET version_code = ? WHERE pkg = ?", real["versionCode"] + 1, TEST_PKG)
        err, _ = call("POST", f"/extensions/{TEST_PKG}/install", expect=409)
        check("a store that goes back to an older version is not followed", err["error"]["code"] == "older_version")
        change(db_path, "UPDATE seen_versions SET version_code = ? WHERE pkg = ?", real["versionCode"], TEST_PKG)

        # An account's list that fell behind is put right when the host starts.
        change(db_path, "UPDATE extensions SET version_code = ?, version_name = 'old' WHERE pkg = ?", real["versionCode"] - 1, TEST_PKG)
        wait_asleep()
        call("GET", f"/sources/{sid}/popular")
        start = time.time()
        while installed_as()["versionName"] == "old" and time.time() - start < 15:
            time.sleep(1)
        check("a host that starts puts right the list of an account that fell behind", installed_as()["versionCode"] == real["versionCode"])
        opener = kid_browser

    call("DELETE", f"/extensions/{TEST_PKG}")
    call("DELETE", f"/stores/{theirs['id']}")
    check("the second account removes both again", online_sources() == [] and call("GET", "/stores")[0]["stores"] == [])
    opener = owner
    check(
        "the first account still has its own, and they still work",
        len(online_sources()) == len(ext["sources"]) and len(call("GET", "/stores")[0]["stores"]) == 1 and len(call("GET", f"/sources/{sid}/popular")[0]["items"]) > 5,
    )

    print("an account that leaves")
    # It installs an extension nobody has yet, which takes an admin unless that is open to everyone.
    guest, _ = call("POST", "/users", {"name": "smoke-guest", "password": KID_PASSWORD, "admin": restricted})
    opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
    call("POST", "/auth/login", {"name": "smoke-guest", "password": KID_PASSWORD})
    call("POST", "/stores", {"url": STORE_LINK})
    other = next(i["pkg"] for i in call("GET", "/catalog?content=safe&limit=30")[0]["items"] if i["installable"] and i["pkg"] != TEST_PKG)
    call("POST", f"/extensions/{TEST_PKG}/install")
    call("POST", f"/extensions/{other}/install")
    check("it installs one extension the first account has and one it has not", len(call("GET", "/extensions")[0]["extensions"]) == 2)
    opener = owner

    def in_host():
        """The packages the extension host holds, read from its own list."""
        with open(os.path.join(data_dir, "host", "extensions", "installed.json"), encoding="utf-8") as registry:
            return set(json.load(registry))

    if data_dir:
        check("the host holds what either account installed", {TEST_PKG, other} <= in_host())
    call("DELETE", f"/users/{guest['id']}")
    if data_dir:
        check("removing the account takes what only it had out of the host, and nothing else", in_host() == {TEST_PKG})
    check(
        "and the first account's extension still works",
        [e["pkg"] for e in call("GET", "/extensions")[0]["extensions"]] == [TEST_PKG] and len(call("GET", f"/sources/{sid}/popular")[0]["items"]) > 5,
    )

print("updates")
call("PUT", f"/manga/{card['id']}/auto-download", {"on": True})
victim = next((c for c in title["chapters"] if c["id"] != cid), None)
if db_path and victim:
    # Pretend one chapter was never seen, so the update has something to find.
    db = sqlite3.connect(db_path, timeout=10)
    db.execute("DELETE FROM chapters WHERE id = ?", (victim["id"],))
    db.commit()
    db.close()
status, ms = call("POST", "/updates/run")
check("an update starts in the background", status["running"] and ms < 500)
start = time.time()
while call("GET", "/updates")[0]["status"]["running"] and time.time() - start < 120:
    time.sleep(1)
updates = call("GET", "/updates")[0]
print(f"  update: {updates['status']['total']} title(s) in {int(time.time() - start)} s, {updates['status']['found']} new")
check("the update checks the library title without failures", updates["status"]["total"] == 1 and not updates["errors"])
if db_path and victim:
    found = [i for i in updates["items"] if i["chapterName"] == victim["name"]]
    check("it finds the missing chapter and lists it as new", updates["status"]["found"] == 1 and len(found) == 1 and updates["fresh"] >= 1)
    again = call("GET", f"/manga/{card['id']}")[0]["chapters"]
    queue = [q["chapterId"] for q in call("GET", "/downloads")[0]["queue"]]
    check(
        "and queues it for download, as the title asks",
        any(c["name"] == victim["name"] and (c["downloaded"] or c["id"] in queue) for c in again),
    )
call("DELETE", "/downloads")
call("PUT", f"/manga/{card['id']}/auto-download", {"on": False})

call("PUT", "/settings", {"smoke.test": {"n": 1}})
check("settings are kept", call("GET", "/settings")[0]["smoke.test"] == {"n": 1})
call("PUT", "/settings", {"smoke.test": None})
status, got, page, _ = raw("GET", site + "/")
check("the web UI is served", status == 200 and b'id="app"' in page and "default-src 'self'" in got.get("Content-Security-Policy", ""))

print("idle")
time.sleep(5)
health = host().get("health") or {}
print(f"  host memory {health.get('rssMb')} MB, {health.get('loadedSources')} sources loaded")
seconds = wait_asleep()
check("the host stops when idle", seconds is not None)
check("sources are still listed with the JVM asleep", len(online_sources()) == len(ext["sources"]) and host()["state"] == "asleep")
status, _, same, _ = raw("GET", f"{base}/chapters/{cid}/pages/0")
check("a cached page and the title open with the JVM asleep", status == 200 and same == image and call("GET", f"/manga/{card['id']}")[0]["chapters"])
status, got, kept, _ = raw("GET", f"{site}/{card['cover']}")
check("so do the library and its covers", status == 200 and kept == cover and len(call("GET", "/library")[0]["items"]) == 1)
if data_dir:
    # With the page cache gone, a saved chapter must still read, straight from its file.
    shutil.rmtree(os.path.join(data_dir, "cache", "pages", str(cid)), ignore_errors=True)
    status, _, same, _ = raw("GET", f"{base}/chapters/{cid}/pages/0")
    check("a downloaded chapter reads from its file, with no cache and no JVM", status == 200 and same == image and host()["state"] == "asleep")
check("and it stayed asleep", host()["state"] == "asleep")

print("solver")
kept, _ = call("PUT", "/solver", {"url": " http://127.0.0.1:9/ ", "timeoutSecs": 1})
check("the solver's address is tidied and kept", kept == {"url": "http://127.0.0.1:9", "timeoutSecs": 5} and call("GET", "/solver")[0] == kept)
call("PUT", "/solver", {"url": "ftp://nowhere"}, expect=400)
check("and can be taken away again", call("PUT", "/solver", {"url": ""})[0]["url"] == "")

if data_dir:
    print("local files")
    # A title on disk: one chapter as a CBZ, one as a folder of images, and a cover.
    png = bytes([0x89]) + b"PNG" + bytes([13, 10, 26, 10]) + b"mango smoke "
    folder = os.path.join(data_dir, "local", "Smoke Title")
    os.makedirs(os.path.join(folder, "Chapter 2"), exist_ok=True)
    with zipfile.ZipFile(os.path.join(folder, "Chapter 1.cbz"), "w") as archive:
        for name in ("10.png", "2.png", "1.png"):
            archive.writestr(name, png + name.encode())
        archive.writestr("ComicInfo.xml", "<ComicInfo><Summary>Made by the smoke test</Summary><Writer>Mango</Writer></ComicInfo>")
    for name in ("b.png", "a.png"):
        with open(os.path.join(folder, "Chapter 2", name), "wb") as page:
            page.write(png + name.encode())
    with open(os.path.join(folder, "cover.png"), "wb") as page:
        page.write(png + b"cover")

    listed = call("GET", "/sources/0/popular")[0]["items"]
    mine = next((i for i in listed if i["title"] == "Smoke Title"), None)
    check("a folder shows up as a title", mine is not None)
    local = call("POST", f"/manga/{mine['id']}/refresh")[0]
    check(
        "its chapters are found, in natural order, with details from ComicInfo.xml",
        [c["name"] for c in local["chapters"]] == ["Chapter 2", "Chapter 1"]
        and local["description"] == "Made by the smoke test"
        and local["sourceName"] == "Local",
    )
    first = local["chapters"][-1]["id"]
    pages = [raw("GET", f"{base}/chapters/{first}/pages/{n}")[2] for n in range(3)]
    check(
        "an archive reads page by page, 2 before 10",
        call("GET", f"/chapters/{first}/pages")[0]["count"] == 3 and pages == [png + n for n in (b"1.png", b"2.png", b"10.png")],
    )
    second = local["chapters"][0]["id"]
    check("so does a folder of images", raw("GET", f"{base}/chapters/{second}/pages/0")[2] == png + b"a.png")
    check("the cover file is the cover", raw("GET", f"{site}/{local['cover']}")[2] == png + b"cover")
    call("PUT", f"/manga/{mine['id']}/library", {"inLibrary": True})
    call("PUT", f"/chapters/{first}/progress", {"page": 2, "read": True})
    kept = next(i for i in call("GET", "/library")[0]["items"] if i["id"] == mine["id"])
    check("it joins the library and keeps its progress", (kept["total"], kept["unread"], kept["sourceName"]) == (2, 1, "Local"))
    packed = zipfile.ZipFile(io.BytesIO(opds(f"/v1.2/chapters/{first}/file")[2]))
    check("a chapter that was never saved is packed for a reading app", packed.namelist()[0] == "001.png" and [packed.read(n) for n in packed.namelist()] == pages)
    check("none of that needed the JVM", host()["state"] == "asleep")
    call("PUT", f"/manga/{mine['id']}/library", {"inLibrary": False})
    shutil.rmtree(folder)

if db_path:
    print("signing key")
    real = store["signingKey"]
    db = sqlite3.connect(db_path, timeout=10)
    # Pretend the store was added under another key, and that its index changed since.
    db.execute("UPDATE stores SET signing_key = ?, etag = NULL WHERE id = ?", ("0" * 64, store["id"]))
    db.commit()
    err, _ = call("POST", f"/extensions/{TEST_PKG}/install", expect=422)
    check("a jar signed by another key is refused", err["error"]["code"] == "signing_key_mismatch")
    check("and the host was not woken for it", host()["state"] == "asleep")
    changed, _ = call("POST", f"/stores/{store['id']}/refresh")
    check(
        "the store reports the new key and keeps the old catalog",
        changed["keyChanged"] and changed["offeredKey"] == real and changed["extensions"] == store["extensions"],
    )
    err, _ = call("POST", f"/extensions/{TEST_PKG}/install", expect=409)
    check("installs wait until the key is settled", err["error"]["code"] == "signing_key_changed")
    accepted, _ = call("POST", f"/stores/{store['id']}/accept-key")
    check("accepting pins the new key", accepted["signingKey"] == real and not accepted["keyChanged"])
    call("POST", f"/stores/{store['id']}/accept-key", expect=409)

    # Pretend an older version is installed.
    db.execute("UPDATE extensions SET version_code = version_code - 1 WHERE pkg = ?", (TEST_PKG,))
    db.commit()
    db.close()
    pending, _ = call("GET", "/catalog?state=updates")
    check("an older version shows as an update", [i["pkg"] for i in pending["items"]] == [TEST_PKG])
    updates, ms = call("POST", "/extensions/update-all")
    print(f"  update all: {ms} ms from a sleeping host")
    check("update all installs it", [r["pkg"] for r in updates["results"]] == [TEST_PKG] and not updates["results"][0]["error"])
    check("then nothing is pending", call("GET", "/catalog?state=updates")[0]["total"] == 0)

print("import")


def varint(n):
    out = bytearray()
    while True:
        out.append((n & 0x7F) | (0x80 if n > 0x7F else 0))
        n >>= 7
        if not n:
            return bytes(out)


def number(field, value):
    return varint(field << 3) + varint(value)


def text(field, value):
    return varint(field << 3 | 2) + varint(len(value.encode())) + value.encode()


def inner(field, body):
    return varint(field << 3 | 2) + varint(len(body)) + body


def backup_chapter(url, read):
    return text(1, url) + text(2, "Chapter " + url[-1]) + number(4, int(read)) + number(6, 0 if read else 4)


# A small backup in Mihon's format, written out by hand: one title from a source that
# is not installed, with two chapters, a category and a history entry.
entry = (
    number(1, 4242)
    + text(2, "/smoke")
    + text(3, "Smoke Import")
    + inner(16, backup_chapter("/smoke/2", False))
    + inner(16, backup_chapter("/smoke/1", True))
    + number(17, 5)
    + inner(104, text(1, "/smoke/1") + number(2, 1700000000000))
)
backup = gzip.compress(inner(1, entry) + inner(2, text(1, "Imported") + number(2, 5)) + inner(101, text(1, "Nowhere") + number(2, 4242)))


def upload(path, expect=200):
    req = urllib.request.Request(base + path, data=backup, method="POST")
    try:
        with opener.open(req, timeout=120) as res:
            status, payload = res.status, res.read()
    except urllib.error.HTTPError as err:
        status, payload = err.code, err.read()
    if status != expect:
        sys.exit(f"FAIL POST {path}: HTTP {status}, wanted {expect}: {payload[:300].decode(errors='replace')}")
    return json.loads(payload)


def imported():
    upload("/import")
    while call("GET", "/import")[0]["running"]:
        time.sleep(0.2)
    return call("GET", "/import")[0]["report"]


seen = upload("/import/preview")
check("a backup can be looked at first", (seen["titles"], seen["chapters"], seen["readChapters"], seen["categories"]) == (1, 2, 1, ["Imported"]))
check("it names the source that is missing", [(s["name"], s["installed"]) for s in seen["sources"]] == [("Nowhere", False)])
check("looking changes nothing", not any(i["title"] == "Smoke Import" for i in call("GET", "/library")[0]["items"]))
first = imported()
check(
    "the import adds the title, its chapters and its category",
    (first["titlesAdded"], first["chaptersAdded"], first["categoriesAdded"], first["historyEntries"]) == (1, 2, 1, 1),
)
mine = next(i for i in call("GET", "/library")[0]["items"] if i["title"] == "Smoke Import")
check(
    "with read state, history and category intact",
    (mine["total"], mine["unread"], len(mine["categories"]), mine["lastReadAt"]) == (2, 1, 1, 1700000000),
)
again = imported()
check(
    "the same file again changes nothing",
    (again["titlesAdded"], again["titlesMerged"], again["chaptersAdded"], again["chaptersUpdated"], again["historyEntries"]) == (0, 1, 0, 0, 0),
)
backup = b"not a backup"
check("a file that is not a backup is refused", upload("/import/preview", expect=422)["error"]["code"] == "bad_backup")

print("export")
if db_path:
    # A tracker link, as a backup would have brought it: it has to go out with the export.
    db = sqlite3.connect(db_path)
    db.execute("INSERT INTO tracks (manga_id, tracker, media_id, title, last_chapter_read) VALUES (?, 2, 30013, 'Smoke Link', 1)", (mine["id"],))
    db.commit()
    db.close()
status, got, exported, _ = raw("GET", base + "/export")
check("the library exports as a .tachibk", status == 200 and exported[:2] == b"\x1f\x8b" and ".tachibk" in got.get("Content-Disposition", ""))
unpacked = gzip.decompress(exported)
check("with the title, its chapters and its category in it", all(part in unpacked for part in (b"Smoke Import", b"/smoke/2", b"Imported")))
if db_path:
    check("and its tracker link", b"Smoke Link" in unpacked)
backup = exported
back = imported()
check(
    "taking its own export back in changes nothing",
    (back["titlesAdded"], back["chaptersAdded"], back["chaptersUpdated"], back["categoriesAdded"], back["historyEntries"], back["trackerLinks"], back["skipped"])
    == (0, 0, 0, 0, 0, 0, 0),
)

print("trackers")
listed = call("GET", "/trackers")[0]["trackers"]
check(
    "four trackers, nobody signed in",
    [t["name"] for t in listed] == ["AniList", "MyAnimeList", "Kitsu", "MangaUpdates"] and not any(t["signedIn"] for t in listed),
)
for t in listed:
    if t["login"] != "oauth":
        results, ms = call("GET", f"/trackers/{t['id']}/search?q=one%20piece")
        print(f"  {t['name']} search: {ms} ms")
        check(f"{t['name']} finds a title without an account", any("one piece" in r["title"].lower() and r["mediaId"] > 0 for r in results["results"]))
err, _ = call("GET", "/trackers/1/search?q=one%20piece", expect=409)
check("MyAnimeList's search waits for a sign-in", err["error"]["code"] == "tracker_signed_out")
err, _ = call("POST", f"/manga/{mine['id']}/tracks/2", {"mediaId": 30013}, expect=409)
check("so does linking a title", err["error"]["code"] == "tracker_signed_out")
call("POST", f"/manga/{mine['id']}/tracks/99", {"mediaId": 1}, expect=404)
# Sign-ins that must fail, to see each tracker is asked at the right address in the right way.
err, _ = call("POST", "/trackers/2/login", {"token": "not-a-token"}, expect=422)
check("AniList refuses a made-up token", err["error"]["code"] == "tracker_login")
err, _ = call("POST", "/trackers/3/login", {"username": "nobody@example.org", "password": "not-the-password"}, expect=422)
check("Kitsu knows Mango's client and refuses a made-up account", err["error"]["code"] == "tracker_login" and "authentication failed" not in err["error"]["message"].lower())
err, _ = call("POST", "/trackers/7/login", {"username": "mango-smoke-nobody", "password": "not-the-password"}, expect=422)
check("MangaUpdates refuses a made-up account", err["error"]["code"] == "tracker_login")
call("POST", "/trackers/2/authorize", {"clientId": "1", "redirect": site + "/"}, expect=400)
there = call("POST", "/trackers/1/authorize", {"clientId": "smoke-client", "redirect": site + "/"})[0]["url"]
check(
    "MyAnimeList's sign-in page is addressed with a challenge and a state",
    there.startswith("https://myanimelist.net/v1/oauth2/authorize?") and all(part in there for part in ("client_id=smoke-client", "code_challenge=", "state=")),
)
err, _ = call("POST", "/trackers/1/login", {"code": "made-up", "state": "not-the-state"}, expect=422)
check("an answer to another sign-in is refused", err["error"]["code"] == "tracker_login")
view, _ = call("GET", f"/manga/{mine['id']}/tracks")
if db_path:
    check("a link a backup brought shows on the title", [(l["tracker"], l["mediaId"], l["title"]) for l in view["tracks"]] == [(2, 30013, "Smoke Link")])
    call("PUT", f"/manga/{mine['id']}/chapters", {"read": True})
    time.sleep(0.5)
    kept = call("GET", f"/manga/{mine['id']}/tracks")[0]["tracks"][0]
    check("reading leaves it alone while nobody is signed in", (kept["lastChapterRead"], kept["error"]) == (1, None))
    call("DELETE", f"/manga/{mine['id']}/tracks/2")
check("a title without links has none", call("GET", f"/manga/{mine['id']}/tracks")[0]["tracks"] == [] and len(view["trackers"]) == 4)
call("DELETE", f"/manga/{mine['id']}/tracks/2", expect=404)
check("no token ever shows in the settings", "tracker" not in json.dumps(call("GET", "/settings")[0]))

print("migrate")
# The imported title's source does not exist, which is when one moves: onto a title of a source that does.
other = popular["items"][1]
if db_path:
    db = sqlite3.connect(db_path)
    db.execute("INSERT INTO tracks (manga_id, tracker, media_id, title) VALUES (?, 2, 30013, 'Smoke Link')", (mine["id"],))
    # Its two chapters are read by now. Numbered past any other, they put everything behind them.
    db.execute("UPDATE chapters SET chapter_number = 9999 WHERE manga_id = ?", (mine["id"],))
    db.commit()
    db.close()
seen, ms = call("GET", f"/manga/{mine['id']}/migrate/{other['id']}")
print(f"  compared with {seen['to']['chapters']} chapters of the other source in {ms} ms")
check(
    "two titles are put side by side, the other one's chapters fetched",
    (seen["from"]["title"], seen["from"]["chapters"], seen["to"]["id"]) == ("Smoke Import", 2, other["id"]) and seen["to"]["chapters"] > 0,
)
there = call("GET", f"/manga/{other['id']}")[0]
reached = round(seen["from"]["lastRead"] * 100)
due = sum(1 for c in there["chapters"] if 0 <= c["number"] and round(c["number"] * 100) <= reached and not c["read"])
print(f"  {due} chapters to mark read")
check("it says what would go along", (seen["read"], seen["categories"], seen["tracks"]) == (due, 1, 1 if db_path else 0))
if db_path:
    check("every numbered chapter up to the last one read counts as read", due > 0)
check("comparing changes nothing", not there["inLibrary"] and not any(c["read"] for c in there["chapters"]))
call("POST", f"/manga/{mine['id']}/migrate/{mine['id']}", {}, expect=400)
call("GET", f"/manga/{mine['id']}/migrate/999999", expect=404)
moved, _ = call("POST", f"/manga/{mine['id']}/migrate/{other['id']}", {})
check("moving reports what went along", (moved["read"], moved["categories"]) == (due, 1))
there = call("GET", f"/manga/{other['id']}")[0]
check(
    "the other title takes its place, its category and its progress",
    there["inLibrary"] and there["categories"] == mine["categories"] and sum(c["read"] for c in there["chapters"]) == due,
)
check(
    "the old one leaves the library and keeps its chapters",
    [i["id"] for i in call("GET", "/library")[0]["items"] if i["id"] in (mine["id"], other["id"])] == [other["id"]]
    and len(call("GET", f"/manga/{mine['id']}")[0]["chapters"]) == 2,
)
if db_path:
    links = [call("GET", f"/manga/{i}/tracks")[0]["tracks"] for i in (mine["id"], other["id"])]
    check("its tracker link moves too", links[0] == [] and [l["mediaId"] for l in links[1]] == [30013])
call("POST", "/library/bulk", {"ids": [mine["id"], other["id"]], "action": "remove"})
call("DELETE", f"/categories/{mine['categories'][0]}")

print("remove")
call("DELETE", f"/chapters/{cid}/download")
gone = not next(c for c in call("GET", f"/manga/{card['id']}")[0]["chapters"] if c["id"] == cid)["downloaded"]
check("a saved chapter's file can be deleted", gone and not (data_dir and glob.glob(os.path.join(data_dir, "downloads", "*", "*", "*.cbz"))))
call("DELETE", f"/extensions/{TEST_PKG}")
check(
    "the extension and its sources are gone",
    call("GET", "/extensions")[0]["extensions"] == [] and online_sources() == [],
)
call("DELETE", f"/extensions/{TEST_PKG}", expect=404)
call("DELETE", f"/stores/{store['id']}")
check("the store and its catalog are gone", call("GET", "/stores")[0]["stores"] == [] and call("GET", "/catalog")[0]["total"] == 0)
call("POST", f"/extensions/{TEST_PKG}/install", expect=404)

if kid:
    print("accounts, again")
    call("PUT", f"/users/{kid['id']}", {"password": KID_PASSWORD + "2", "admin": True})
    check("a new password from the admin closes the old one", as_kid("/v1.2/catalog")[0] == 401 and opds("/v1.2/catalog", KID, KID_PASSWORD + "2")[0] == 200)
    call("DELETE", f"/users/{kid['id']}")
    check("a removed account is gone", opds("/v1.2/catalog", KID, KID_PASSWORD + "2")[0] == 401 and not call("GET", "/auth")[0]["named"])
    # Added at once, while the removed account's folder may still be there.
    heir, _ = call("POST", "/users", {"name": KID, "password": KID_PASSWORD})
    check("the next account gets a number of its own", heir["id"] > kid["id"])
    call("DELETE", f"/users/{heir['id']}")
    if data_dir:
        folder = os.path.join(data_dir, "users", str(kid["id"]))
        start = time.time()
        while os.path.exists(folder) and time.time() - start < 15:
            time.sleep(1)
        check("and its library with it", not os.path.exists(folder))

if who["login"]:
    call("POST", "/auth/logout")
    call("GET", "/library", expect=401)
    check("signing out closes it again", not call("GET", "/auth")[0]["signedIn"] and len(jar) == 0)

print()
print("FAILED: " + "; ".join(failures) if failures else "all checks passed")
sys.exit(1 if failures else 0)
