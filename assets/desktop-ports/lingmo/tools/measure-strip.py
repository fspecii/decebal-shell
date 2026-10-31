#!/usr/bin/python3 -I
"""Measures the thickness of the bar strip in a native capture, in physical pixels: the run of pixels from the screen edge, along a line through an
empty part of the bar, that differ from the flat backdrop.
usage: measure-strip.py CAPTURE.png EDGE top|bottom|left|right BACKDROP_HEX [OUT_JSON]   -> prints the thickness and the screen size
Needs ImageMagick (`magick`) to decode; no network, nothing executed from the capture."""
import json, subprocess, sys

def main():
    png, edge, bg_hex = sys.argv[1:4]
    bg = tuple(int(bg_hex.lstrip("#")[i:i + 2], 16) for i in (0, 2, 4))
    w, h = map(int, subprocess.run(["magick", "identify", "-format", "%w %h", png], capture_output=True, text=True, check=True).stdout.split())
    raw = subprocess.run(["magick", png, "-depth", "8", "rgb:-"], capture_output=True, check=True).stdout
    px = lambda x, y: tuple(raw[(y * w + x) * 3:(y * w + x) * 3 + 3])
    differs = lambda p: max(abs(p[i] - bg[i]) for i in range(3)) > 6
    if edge in ("top", "bottom"):
        x = w // 2
        rows = range(h) if edge == "top" else range(h - 1, -1, -1)
        n = 0
        for y in rows:
            if not differs(px(x, y)):
                break
            n += 1
    else:
        y = h // 2
        cols = range(w) if edge == "left" else range(w - 1, -1, -1)
        n = 0
        for x in cols:
            if not differs(px(x, y)):
                break
            n += 1
    out = {"capture": png.split("/")[-1], "edge": edge, "thickness_px": n, "screen": [w, h]}
    print(json.dumps(out))
    if len(sys.argv) > 5:
        json.dump(out, open(sys.argv[5], "w"))

main()
