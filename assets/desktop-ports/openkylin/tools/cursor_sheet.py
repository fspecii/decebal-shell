#!/usr/bin/env python3
"""Render a preview sheet of Xcursor themes (what the cursor looks like at 100/125/200 %) from the installed files.

EVIDENCE ONLY (not an installer). Usage: python3 -I cursor_sheet.py OUT.png THEME_DIR [THEME_DIR ...]
THEME_DIR is a theme the shared installer put in a private icon root (`decthemectl assets install ...`); it holds cursors/ (Xcursor files).
Sizes: nominal 24 (100 %), 32 (125 %, the nearest shipped size) and 48 (200 %) as X selects the nearest nominal size.
Hotspots are drawn as a red dot. Output is for review only: the artwork stays under its upstream licence and is not committed.
"""
import importlib.util, pathlib, sys
from PIL import Image, ImageDraw

spec = importlib.util.spec_from_file_location("xcursor_read", pathlib.Path(__file__).with_name("xcursor_read.py")); xc = importlib.util.module_from_spec(spec); spec.loader.exec_module(xc)
NAMES = ["left_ptr", "hand2", "xterm", "watch", "left_ptr_watch", "sb_h_double_arrow", "sb_v_double_arrow", "crosshair", "question_arrow", "grabbing", "copy", "link", "move", "all-scroll", "top_left_corner", "not-allowed"]


def frames(path):
    out = {}
    for r in xc.read(path):
        if r["nominal"] not in out:
            out[r["nominal"]] = (Image.frombuffer("RGBA", (r["w"], r["h"]), r["argb"], "raw", "BGRA", 0, 1), *r["hot"])   # premultiplied; fine for a preview
    return out


def main():
    out, themes = sys.argv[1], [pathlib.Path(p) for p in sys.argv[2:]]
    sizes = (24, 32, 48); cell = 64; label = 150
    sheet = Image.new("RGB", (label + len(NAMES) * cell, len(themes) * len(sizes) * 2 * cell // 1), (255, 255, 255))
    d = ImageDraw.Draw(sheet); y = 0
    for t in themes:
        for mode, bg in (("light", (245, 245, 245)), ("dark", (46, 46, 46))):
            for sz in sizes:
                d.rectangle((0, y, sheet.width, y + cell), fill=bg)
                d.text((6, y + 24), f"{t.name} {mode} {sz}px", fill=(0, 0, 0) if mode == "light" else (230, 230, 230))
                for i, nm in enumerate(NAMES):
                    p = t / "cursors" / nm
                    if not p.exists(): continue
                    fr = frames(p.resolve()); im, hx, hy = fr.get(sz) or fr[min(fr, key=lambda s: abs(s - sz))]
                    x0 = label + i * cell + 8; sheet.paste(im, (x0, y + 8), im); d.ellipse((x0 + hx - 2, y + 8 + hy - 2, x0 + hx + 2, y + 8 + hy + 2), outline=(220, 0, 0))
                y += cell
    sheet.crop((0, 0, sheet.width, y)).save(out)
    print("wrote", out)


main()
