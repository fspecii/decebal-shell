#!/usr/bin/python3
"""Full-resolution review helper for the Plasma wallpaper records (private cache only, no network).

usage: plasma-wallpaper-review.py THEMES_DIR IMAGE_DIR OUT_JSON ID=THEME:MODE:FILE ...

For each image it decodes the original with ImageMagick, verifies dimensions and sha256, and measures, at 1920x1080 and 2560x1080
(centre-fill), the mean colour of each panel-edge band (top, bottom, left, right; thickness from the package's own panel size).
The panel fill is composited over every 40 px cell with the package's own alpha, and the WCAG contrast of the package's bar text
against it is reported (minimum and mean). It judges legibility of the declared panel only; it does not judge taste.
"""
import hashlib, json, subprocess, sys, tomllib

def lum(c):
    f = lambda v: (v / 255) / 12.92 if v / 255 <= 0.03928 else (((v / 255) + 0.055) / 1.055) ** 2.4
    return 0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2])

def contrast(a, b):
    la, lb = lum(a), lum(b)
    return (max(la, lb) + 0.05) / (min(la, lb) + 0.05)

def hexrgb(h):
    h = h.lstrip("#")
    return tuple(int(h[i:i + 2], 16) for i in (0, 2, 4))

def cells(path, w, h, geom):
    """Mean colour of 40 px cells of the centre-filled image inside geom = WxH+X+Y (rows of cells along the longer side)."""
    gw, gh = int(geom.split("x")[0]), int(geom.split("x")[1].split("+")[0])
    cw, ch = (max(1, gw // 40), 1) if gw >= gh else (1, max(1, gh // 40))
    out = subprocess.run(["magick", path, "-resize", f"{w}x{h}^", "-gravity", "center", "-extent", f"{w}x{h}", "-gravity", "NorthWest", "-crop", geom, "+repage",
                          "-scale", f"{cw}x{ch}!", "-depth", "8", "txt:-"], capture_output=True, text=True, check=True).stdout
    vals = []
    for line in out.splitlines()[1:]:
        r = line.split("(")[1].split(")")[0].split(",")[:3]
        vals.append(tuple(int(float(x.strip().rstrip("%"))) for x in r))
    return vals

def main():
    themes, imgdir, out = sys.argv[1:4]
    res = {}
    for spec in sys.argv[4:]:
        iid, rest = spec.split("=")
        theme, mode, fname = rest.split(":")
        path = f"{imgdir}/{fname}"
        sha = hashlib.sha256(open(path, "rb").read()).hexdigest()
        dims = subprocess.run(["magick", "identify", "-format", "%w %h", path], capture_output=True, text=True, check=True).stdout.split()
        man = tomllib.load(open(f"{themes}/{theme}/theme.toml", "rb"))
        tok = man["colors"][mode]
        m = man["metrics"]
        # panel thickness and fill alpha as declared by the package (Breeze: bottom panel 44 px; Layan: 32 px top bar)
        thick = 44 if theme == "plasma-breeze" else 32
        panel = hexrgb(tok["bar"]); text = hexrgb(tok["bar_text"]); alpha = m["dock_alpha"] if theme == "plasma-breeze" else m["bar_alpha"]
        rec = {"file": fname, "sha256": sha, "bytes": len(open(path, "rb").read()), "dimensions": [int(dims[0]), int(dims[1])],
               "panel_fill": tok["bar"], "panel_alpha": alpha, "bar_text": tok["bar_text"], "panel_thickness_px": thick, "viewports": {}}
        for vw, vh in ((1920, 1080), (2560, 1080), (1920, 1200), (1080, 1920)):
            edges = {}
            for edge, geom in (("top", f"{vw}x{thick}+0+0"), ("bottom", f"{vw}x{thick}+0+{vh - thick}"),
                               ("left", f"{thick}x{vh}+0+0"), ("right", f"{thick}x{vh}+{vw - thick}+0")):
                cs = cells(path, vw, vh, geom)
                cr = []
                for c in cs:
                    comp = tuple(round(panel[i] * alpha + c[i] * (1 - alpha)) for i in range(3))
                    cr.append(contrast(text, comp))
                edges[edge] = {"min_contrast": round(min(cr), 2), "mean_contrast": round(sum(cr) / len(cr), 2), "cells": len(cr)}
            rec["viewports"][f"{vw}x{vh}"] = edges
        res[f"{theme}:{mode}:{iid}"] = rec
    json.dump(res, open(out, "w"), indent=1)

main()
