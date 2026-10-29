#!/usr/bin/env python3
"""Smoke test for a running extension host.

    python3 smoke.py http://127.0.0.1:<port> <token> [solver address]

Installs a real extension from an extension store, reads one page through it,
then loads more extensions and reports the host's memory.

The store and the extension are not named here. They come from the environment:

    MANGO_SMOKE_STORE  the address of a Mihon extension store's repo.json
    MANGO_SMOKE_PKG    the package of an extension in that store

With a solver address, which must be the one the host was started with
(MANGO_HOST_SOLVER_URL) and on this machine, it stands in for that solver and for a
site behind a Cloudflare check, and has a source fetch a picture from the site.

With MANGO_SMOKE_PROXY_PORT set, the host must have been started with
MANGO_HOST_PROXY_URL=http://127.0.0.1:<that port> and
MANGO_HOST_PROXY_SITES=proxied.example: it stands in for that proxy and checks that
the named site goes through it and another does not.
"""
import http.server
import json
import os
import sys
import threading
import time
import urllib.request

try:
    # The folder the store's files are in: its repo.json's address without the file.
    STORE = os.environ["MANGO_SMOKE_STORE"].rsplit("/", 1)[0]
    TEST_PKG = os.environ["MANGO_SMOKE_PKG"]
except KeyError as missing:
    sys.exit(f"Set {missing.args[0]}: the top of this file says what goes in it.")

base, token = sys.argv[1].rstrip("/"), sys.argv[2]
solver = sys.argv[3].rstrip("/") if len(sys.argv) > 3 else None


def call(method, path, body=None, raw=False):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(base + path, data=data, method=method)
    req.add_header("Authorization", "Bearer " + token)
    start = time.time()
    try:
        with urllib.request.urlopen(req, timeout=180) as res:
            payload = res.read()
    except urllib.error.HTTPError as err:
        payload = err.read()
        print(f"  {method} {path} -> HTTP {err.code}: {payload[:300].decode(errors='replace')}")
        raise
    ms = int((time.time() - start) * 1000)
    return (payload if raw else json.loads(payload)), ms


def fetch(url):
    with urllib.request.urlopen(url, timeout=60) as res:
        return json.loads(res.read())


def health(label):
    h, _ = call("GET", "/health")
    print(f"{label:<34} rss={h['rssMb']} MB  extensions={h['loadedExtensions']}  sources={h['loadedSources']}")
    return h


health("host up")

index = fetch(STORE + "/index.json")["extensionList"]["extensions"]
hashes = fetch(STORE + "/release-assets.json")
by_pkg = {e["packageName"]: e for e in index}


def install(pkg):
    return call("POST", "/extensions/install", {"url": by_pkg[pkg]["resources"]["jarUrl"], "sha256": hashes[pkg]["jar"]["sha256"]})


ext, ms = install(TEST_PKG)
print(f"installed {ext['name']} {ext['versionName']} ({len(ext['sources'])} sources) in {ms} ms")
source = next((s for s in ext["sources"] if s["lang"] == "en"), ext["sources"][0])
sid = source["id"]

popular, ms = call("POST", f"/sources/{sid}/popular", {"page": 1})
print(f"popular: {len(popular['mangas'])} titles in {ms} ms")

cover, ms = call("POST", f"/sources/{sid}/image", {"cover": popular["mangas"][0]["thumbnailUrl"]}, raw=True)
print(f"cover: {len(cover)} bytes in {ms} ms")

update, ms = call("POST", f"/sources/{sid}/manga", {"manga": popular["mangas"][0]})
print(f"title: {len(update['chapters'])} chapters in {ms} ms")

pages = []
for chapter in update["chapters"][-5:]:
    result, ms = call("POST", f"/sources/{sid}/pages", {"chapter": chapter})
    pages = result["pages"]
    if pages:
        print(f"pages: {len(pages)} in {ms} ms")
        break

if pages:
    image, ms = call("POST", f"/sources/{sid}/image", {"page": pages[0]}, raw=True)
    print(f"image: {len(image)} bytes in {ms} ms")

found, ms = call("POST", f"/sources/{sid}/search", {"page": 1, "query": "one"})
print(f"search: {len(found['mangas'])} titles in {ms} ms")

# What the search can be narrowed by: read the list, set one filter of each kind it has,
# search by them alone, and see that a change for a place that is not there is refused.
filters, ms = call("GET", f"/sources/{sid}/filters")
filters = filters["filters"]


def kinds(items):
    return {f["type"] for f in items} | {k for f in items if f["type"] == "group" for k in kinds(f["filters"])}


def changes(items, done):
    out = []
    for position, f in enumerate(items):
        kind = f["type"]
        if kind == "group":
            inner = changes(f["filters"], done)
            if inner:
                out.append({"position": position, "state": inner})
            continue
        if kind in done or kind in ("header", "separator") or (kind in ("select", "sort") and len(f["values"]) < 2):
            continue
        done.add(kind)
        state = {
            "checkbox": lambda: not f["state"],
            "tristate": lambda: 1,
            "text": lambda: "",
            "select": lambda: (f["state"] + 1) % len(f["values"]),
            "sort": lambda: {"index": len(f["values"]) - 1, "ascending": not (f["state"] or {}).get("ascending", False)},
        }[kind]()
        out.append({"position": position, "state": state})
    return out


print(f"filters: {len(filters)} in {ms} ms ({', '.join(sorted(kinds(filters)))})")
if filters:
    tried = set()
    narrowed, ms = call("POST", f"/sources/{sid}/search", {"page": 1, "query": "", "filters": changes(filters, tried)})
    plain, _ = call("POST", f"/sources/{sid}/search", {"page": 1, "query": ""})
    same = [m["url"] for m in narrowed["mangas"]] == [m["url"] for m in plain["mangas"]]
    print(f"search by filters ({', '.join(sorted(tried))}): {len(narrowed['mangas'])} titles in {ms} ms, {'the same list as' if same else 'another list than'} without them")
    try:
        call("POST", f"/sources/{sid}/search", {"page": 1, "query": "", "filters": [{"position": len(filters), "state": True}]})
        sys.exit("FAIL: a filter that is not there was taken")
    except urllib.error.HTTPError:
        print("a filter that is not there is refused, as expected")
else:
    print("no filters on this source to try")

health("after one full read")

# An update replaces an extension that is loaded. Its sources must answer afterwards.
again, ms = install(TEST_PKG)
popular, _ = call("POST", f"/sources/{sid}/popular", {"page": 1})
print(f"installed again over itself in {ms} ms, popular: {len(popular['mangas'])} titles")

# What a source lets its user set: read it, change one entry, see that it stuck and
# that the source still answers, then put it back.
prefs, ms = call("GET", f"/sources/{sid}/preferences")
prefs = prefs["preferences"]
print(f"preferences: {len(prefs)} in {ms} ms ({', '.join(sorted({p['type'] for p in prefs}))})")
pick = next((p for p in prefs if p["enabled"] and p["visible"] and p["key"] and p["type"] == "switch"), None)
if pick:
    change = {"position": pick["position"], "key": pick["key"]}
    after, ms = call("PUT", f"/sources/{sid}/preferences", {**change, "value": not pick["value"]})
    got = after["preferences"][pick["position"]]["value"]
    popular, _ = call("POST", f"/sources/{sid}/popular", {"page": 1})
    print(f"preference {pick['key']}: {pick['value']} -> {got} in {ms} ms, popular: {len(popular['mangas'])} titles")
    if got == pick["value"]:
        sys.exit("FAIL: the preference did not change")
    back, _ = call("PUT", f"/sources/{sid}/preferences", {**change, "value": pick["value"]})
    if back["preferences"][pick["position"]]["value"] != pick["value"]:
        sys.exit("FAIL: the preference did not change back")
    try:
        call("PUT", f"/sources/{sid}/preferences", {**change, "key": "not-" + pick["key"], "value": True})
        sys.exit("FAIL: a change meant for another entry was taken")
    except urllib.error.HTTPError:
        print("a change for an entry that moved is refused, as expected")
else:
    print("no switch among them to try a change on")

if solver:
    PICTURE = b"\x89PNG\r\n\x1a\n" + bytes(64)
    asked = []

    class Quiet(http.server.BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def answer(self, status, kind, body):
            self.send_response(status)
            self.send_header("Content-Type", kind)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    class Site(Quiet):
        """Shows the check to everyone who does not carry the cookie a solver gets."""

        def version_string(self):
            return "cloudflare"

        def do_GET(self):
            passed = "cf_clearance=smoke" in self.headers.get("Cookie", "") and self.path != "/blocked.png"
            self.answer(200, "image/png", PICTURE) if passed else self.answer(403, "text/html", b"Just a moment...")

    class Solver(Quiet):
        def do_POST(self):
            request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            asked.append(request)
            if request["url"].endswith("/blocked.png"):
                # What a solver answers when the site bans its address.
                refused = {"status": "error", "message": "Error: Error solving the challenge. Cloudflare has blocked this request."}
                return self.answer(500, "application/json", json.dumps(refused).encode())
            cookie = {"name": "cf_clearance", "value": "smoke", "domain": "127.0.0.1", "path": "/"}
            solution = {"url": request["url"], "status": 200, "userAgent": "Smoke/1.0", "cookies": [cookie]}
            answer = {"status": "ok", "message": "Challenge solved!", "solution": solution, "startTimestamp": 0, "endTimestamp": 0, "version": "smoke"}
            self.answer(200, "application/json", json.dumps(answer).encode())

    site = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Site)
    stand_in = http.server.ThreadingHTTPServer(("127.0.0.1", int(solver.rsplit(":", 1)[1])), Solver)
    for server in (site, stand_in):
        threading.Thread(target=server.serve_forever, daemon=True).start()
    address = f"http://127.0.0.1:{site.server_port}/cover.png"
    got, ms = call("POST", f"/sources/{sid}/image", {"cover": address}, raw=True)
    sent = asked[0] if asked else {}
    if got != PICTURE or len(asked) != 1 or (sent.get("cmd"), sent.get("url"), sent.get("session")) != ("request.get", address, "mango"):
        sys.exit(f"solver: the check was not passed: {len(got)} bytes, asked {asked}")
    print(f"solver: one check handed over and passed in {ms} ms")
    got, ms = call("POST", f"/sources/{sid}/image", {"cover": address}, raw=True)
    if got != PICTURE or len(asked) != 1:
        sys.exit("solver: the cookie was not kept for the next call")
    print(f"solver: the next call went straight through in {ms} ms")
    req = urllib.request.Request(base + f"/sources/{sid}/image", data=json.dumps({"cover": address.replace("cover", "blocked")}).encode(), method="POST")
    req.add_header("Authorization", "Bearer " + token)
    try:
        urllib.request.urlopen(req, timeout=180)
        sys.exit("solver: a check the solver turned down came through")
    except urllib.error.HTTPError as err:
        said = json.loads(err.read())["error"]["message"]
        if err.code != 502 or "Cloudflare check" not in said:
            sys.exit(f"solver: a check the solver turned down answered HTTP {err.code}: {said}")
    print(f"solver: a check the solver turned down says so: {said}")

if os.environ.get("MANGO_SMOKE_PROXY_PORT"):
    PICTURE = b"\x89PNG\r\n\x1a\n" + bytes(32)
    through = []

    class StandIn(http.server.BaseHTTPRequestHandler):
        """Answers what is asked of it through a proxy by itself: the names it gets do not exist."""

        def log_message(self, *args):
            pass

        def do_GET(self):
            through.append(self.path)
            self.send_response(200)
            self.send_header("Content-Type", "image/png")
            self.send_header("Content-Length", str(len(PICTURE)))
            self.end_headers()
            self.wfile.write(PICTURE)

    stand_in = http.server.ThreadingHTTPServer(("127.0.0.1", int(os.environ["MANGO_SMOKE_PROXY_PORT"])), StandIn)
    threading.Thread(target=stand_in.serve_forever, daemon=True).start()
    got, ms = call("POST", f"/sources/{sid}/image", {"cover": "http://img.proxied.example/cover.png"}, raw=True)
    if got != PICTURE or through != ["http://img.proxied.example/cover.png"]:
        sys.exit(f"proxy: a named site did not go through it: {len(got)} bytes, it saw {through}")
    print(f"proxy: a named site went through it in {ms} ms")
    try:
        call("POST", f"/sources/{sid}/image", {"cover": "http://unnamed.example/cover.png"}, raw=True)
    except urllib.error.HTTPError:
        pass
    if len(through) != 1:
        sys.exit(f"proxy: a site not named went through it: {through}")
    print("proxy: a site not named went straight out")

loaded, failed = 1, []
for e in index:
    if loaded >= 20:
        break
    pkg = e["packageName"]
    if pkg == TEST_PKG or e["contentWarning"] != "CONTENT_WARNING_SAFE" or "jarUrl" not in e["resources"]:
        continue
    try:
        install(pkg)
        loaded += 1
    except Exception:
        failed.append(pkg)
    if loaded in (10, 20):
        health(f"{loaded} extensions installed")

if failed:
    print("failed to install:", ", ".join(failed))

listed, _ = call("GET", "/extensions")
print(f"registry lists {len(listed['extensions'])} extensions")

call("DELETE", f"/extensions/{TEST_PKG}")
health("after removing one")
