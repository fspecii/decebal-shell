#!/usr/bin/env python3
"""A tiny fake of wallhaven.cc for the end-to-end tests. Binds 127.0.0.1 only.

usage: wallhaven_server.py STATE_DIR FIXTURES_DIR [PORT]
  writes STATE_DIR/port when ready, appends one JSON line per request to STATE_DIR/requests.jsonl
  (path, user-agent, whether an X-API-Key header came, never the key itself) and reads STATE_DIR/mode:
    ok (default) | 429  (the API answers 429 with Retry-After: 30)

Serves, under the base http://127.0.0.1:PORT :
  /api/v1/search?...   fixtures/search.json (page 1: the 6 recorded items + clones), q=... -> search_query.json;
                       3 pages of 24; cloned items get ids like s1004x (valid wallhaven ids)
  /api/v1/w/<id>       fixtures/detail.json with the id swapped in
  /th/{small,lg}/..    generated gradient thumbnails (PNG bytes, whatever the extension)
  /full/<xx>/wallhaven-<id>.{png,jpg}   a generated 1920x1080 image, sent slowly in chunks (progress bar)
The recorded fixtures point at https://w.wallhaven.cc and https://th.wallhaven.cc; they are rewritten to this server.
"""
import copy
import hashlib
import io
import json
import os
import re
import sys
import time
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from PIL import Image, ImageDraw

STATE, FIX = sys.argv[1], sys.argv[2]
PER_PAGE, LAST_PAGE = 24, 3


def load(name):
    with open(os.path.join(FIX, name)) as f:
        return json.load(f)


SEARCH, QUERY, DETAIL = load("search.json"), load("search_query.json"), load("detail.json")


def colour(seed):
    h = hashlib.sha256(seed.encode()).digest()
    return tuple(60 + h[i] % 170 for i in range(3)), tuple(30 + h[3 + i] % 200 for i in range(3))


def picture(seed, w, h, fmt):
    a, b = colour(seed)
    img = Image.new("RGB", (w, h))
    px = img.load()
    for y in range(h):
        for x in range(w):
            t = (x / w + y / h) / 2
            px[x, y] = tuple(int(a[i] * (1 - t) + b[i] * t) for i in range(3))
    d = ImageDraw.Draw(img)
    d.ellipse((w * 0.55, h * 0.12, w * 0.85, h * 0.12 + w * 0.3), outline=(255, 255, 255), width=max(2, w // 80))
    d.text((8, 6), seed, fill=(255, 255, 255))
    out = io.BytesIO()
    img.save(out, fmt)
    return out.getvalue()


def page_items(base, page, source):
    items = []
    for i in range(PER_PAGE):
        if page == 1 and i < len(source["data"]):
            it = copy.deepcopy(source["data"][i])
        else:
            it = copy.deepcopy(source["data"][i % len(source["data"])])
            nid = "s%d%03dx" % (page, i)
            old = it["id"]
            it = json.loads(json.dumps(it).replace(old, nid))
            it["resolution"] = ["3440x1440", "3840x2160", "2560x1440", "5120x2160"][i % 4]
            it["dimension_x"], it["dimension_y"] = map(int, it["resolution"].split("x"))
        items.append(it)
    return items


def rewrite(text, base):
    return text.replace("https://w.wallhaven.cc", base).replace("https://th.wallhaven.cc", base + "/th")


class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def reply(self, code, body, ctype="application/json", extra=()):
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Connection", "close")
        for k, v in extra:
            self.send_header(k, v)
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        u = urllib.parse.urlparse(self.path)
        q = urllib.parse.parse_qs(u.query)
        base = "http://%s" % self.headers.get("Host")
        with open(os.path.join(STATE, "requests.jsonl"), "a") as f:
            f.write(json.dumps({"path": self.path, "ua": self.headers.get("User-Agent", ""), "key": "X-API-Key" in self.headers}) + "\n")
        try:
            mode = open(os.path.join(STATE, "mode")).read().strip()
        except OSError:
            mode = "ok"
        if u.path.startswith("/api/"):
            if mode == "429":
                return self.reply(429, b'{"error":"Too Many Requests"}', extra=[("Retry-After", "30")])
            m = re.fullmatch(r"/api/v1/w/([a-z0-9]{1,16})", u.path)
            if m:
                d = copy.deepcopy(DETAIL)
                d["data"] = json.loads(json.dumps(d["data"]).replace(d["data"]["id"], m.group(1)))
                return self.reply(200, rewrite(json.dumps(d), base).encode())
            if u.path == "/api/v1/search":
                page = int(q.get("page", ["1"])[0])
                src = QUERY if q.get("q") else SEARCH
                body = {"data": page_items(base, page, src), "meta": {"current_page": page, "last_page": LAST_PAGE, "per_page": PER_PAGE, "total": PER_PAGE * LAST_PAGE, "query": q.get("q", [None])[0], "seed": "aB3xyZ" if q.get("sorting") == ["random"] else None}}
                return self.reply(200, rewrite(json.dumps(body), base).encode())
            return self.reply(404, b"{}")
        m = re.fullmatch(r"/th/(small|lg|orig)/[a-z0-9]{2}/([a-z0-9]{1,16})\.\w+", u.path)
        if m:
            w, h = (200, 112) if m.group(1) == "small" else (300, 200)
            return self.reply(200, picture(m.group(2), w, h, "PNG"), "image/png")
        m = re.fullmatch(r"/full/[a-z0-9]{2}/wallhaven-([a-z0-9]{1,16})\.(png|jpg)", u.path)
        if m:
            fmt = "PNG" if m.group(2) == "png" else "JPEG"
            body = picture(m.group(1), 1920, 1080, fmt)
            self.send_response(200)
            self.send_header("Content-Type", "image/png" if fmt == "PNG" else "image/jpeg")
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Connection", "close")
            self.end_headers()
            n = 8
            step = len(body) // n + 1
            for i in range(0, len(body), step):
                self.wfile.write(body[i : i + step])
                self.wfile.flush()
                time.sleep(0.12)
            return
        self.reply(404, b"not found", "text/plain")


srv = ThreadingHTTPServer(("127.0.0.1", int(sys.argv[3]) if len(sys.argv) > 3 else 0), H)
with open(os.path.join(STATE, "port.tmp"), "w") as f:
    f.write(str(srv.server_address[1]))
os.rename(os.path.join(STATE, "port.tmp"), os.path.join(STATE, "port"))
srv.allow_reuse_address = True
srv.serve_forever()
