#!/usr/bin/env python3
"""Loopback-only live-detail/download fixture for curated-wallpaper.sh.

Generated pixels are test fixtures, never product wallpaper assets. Modes are
read per request: ok, ai, nsfw. No request is proxied to a public service.
"""
import io
import json
import re
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from PIL import Image

state = Path(sys.argv[1])
state.mkdir(parents=True, exist_ok=True)
buf = io.BytesIO()
Image.new("RGB", (640, 360), (36, 110, 136)).save(buf, "PNG")
pixels = buf.getvalue()


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def reply(self, code, data, content="application/json"):
        self.send_response(code)
        self.send_header("Content-Type", content)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        mode = (state / "mode").read_text().strip() if (state / "mode").exists() else "ok"
        with (state / "requests.jsonl").open("a") as log:
            log.write(json.dumps({"path": self.path, "mode": mode}) + "\n")
        match = re.fullmatch(r"/api/v1/w/([a-z0-9]{1,16})", self.path)
        base = "http://127.0.0.1:" + str(self.server.server_address[1])
        if match:
            ident = match[1]
            wall = {"id": ident, "url": "https://wallhaven.cc/w/" + ident,
                    "path": base + "/full/" + ident + ".png",
                    "thumbs": {"small": base + "/thumb/" + ident, "large": base + "/thumb/" + ident},
                    "resolution": "640x360", "dimension_x": 640, "dimension_y": 360,
                    "file_size": len(pixels), "file_type": "image/png", "category": "general",
                    "purity": "nsfw" if mode == "nsfw" else "sfw", "ai_art": mode == "ai",
                    "tags": [{"id": 1, "name": "photography"}], "colors": ["#246e88"]}
            return self.reply(200, json.dumps({"data": wall}).encode())
        if self.path.startswith("/full/") or self.path.startswith("/thumb/"):
            return self.reply(200, pixels, "image/png")
        self.reply(404, b'{}')


server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
(state / "port").write_text(str(server.server_address[1]))
server.serve_forever()
