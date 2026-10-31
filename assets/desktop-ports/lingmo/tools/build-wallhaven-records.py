#!/usr/bin/python3 -I
"""Builds docs/desktop-ports/lingmo/wallhaven-lingmo-records.json from a private cache. No network, no package contents touched.

usage: build-wallhaven-records.py THEME_TOML CACHE_DIR OUT_JSON MODE=ID ...

CACHE_DIR holds detail/<id>.json (the Wallhaven API detail response as fetched), thumb/<id>.jpg, full/<id>.img (the original).
For each image it re-checks size and dimensions against the API, hashes the files, and measures the legibility of the package's own
bar text on each of the four panel edges (32 px, the status bar height) at four viewports: the bar fill is composited over every
40 px cell of the centre-filled image with the package's bar alpha and the WCAG contrast of its bar_text is reported. It judges
legibility of the declared panel only, not taste, authorship or licence.
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
    gw, gh = int(geom.split("x")[0]), int(geom.split("x")[1].split("+")[0])
    cw, ch = (max(1, gw // 40), 1) if gw >= gh else (1, max(1, gh // 40))
    out = subprocess.run(["magick", path, "-resize", f"{w}x{h}^", "-gravity", "center", "-extent", f"{w}x{h}", "-gravity", "NorthWest",
                          "-crop", geom, "+repage", "-scale", f"{cw}x{ch}!", "-depth", "8", "txt:-"], capture_output=True, text=True, check=True).stdout
    vals = []
    for line in out.splitlines()[1:]:
        r = line.split("(")[1].split(")")[0].split(",")[:3]
        vals.append(tuple(int(float(x.strip().rstrip("%"))) for x in r))
    return vals

def sha(path):
    return hashlib.sha256(open(path, "rb").read()).hexdigest()

def main():
    theme, cache, out = sys.argv[1:4]
    man = tomllib.load(open(theme, "rb"))
    thick = man["bar"]["height"]
    alpha = man["metrics"]["bar_alpha"]
    images, reviews = {}, {}
    for spec in sys.argv[4:]:
        mode, iid = spec.split("=")
        detail_path = f"{cache}/detail/{iid}.json"
        d = json.load(open(detail_path))["data"]
        full, thumb = f"{cache}/full/{iid}.img", f"{cache}/thumb/{iid}.jpg"
        dims = subprocess.run(["magick", "identify", "-format", "%w %h", full], capture_output=True, text=True, check=True).stdout.split()
        size = len(open(full, "rb").read())
        assert [int(dims[0]), int(dims[1])] == [int(x) for x in d["resolution"].split("x")], iid
        assert size == d["file_size"], iid
        tag_ids = [t["id"] for t in d["tags"]]
        images[iid] = {
            "id": iid,
            "page_url": d["url"], "api_url": f"https://wallhaven.cc/api/v1/w/{iid}", "full_url": d["path"], "thumbnail_url": d["thumbs"]["large"],
            "resolution": [int(dims[0]), int(dims[1])], "purity": d["purity"], "category": d["category"], "uploaded_at": d["created_at"],
            "favorites": d["favorites"],
            "uploader": {"username": d["uploader"]["username"], "is_author": None},
            "author": {"name": None, "verification": "not independently verified"},
            "source_url": d.get("source") or None,
            "source_verification": "reported by the uploader, not verified; not proof of authorship, capture method or licence" if d.get("source") else "source not provided by Wallhaven",
            "license": {"identifier": None, "url": None, "status": "unknown; no redistribution permission established"},
            "tags": [{"id": t["id"], "name": t["name"]} for t in d["tags"]],
            "ai_review": {"flagged_ai": 133451 in tag_ids, "rejected_tag_ids": [133451],
                          "note": "absence of the AI tag is not certification; native-pixel crops were inspected by eye"},
            "evidence": {"detail_sha256": sha(detail_path), "thumbnail_sha256": sha(thumb), "file_sha256": sha(full), "file_bytes": size},
        }
        panel, text = hexrgb(man["colors"][mode]["bar"]), hexrgb(man["colors"][mode]["bar_text"])
        rec = {"image": iid, "mode": mode, "bar_fill": man["colors"][mode]["bar"], "bar_alpha": alpha, "bar_text": man["colors"][mode]["bar_text"], "panel_thickness_px": thick, "viewports": {}}
        for vw, vh in ((1920, 1080), (2560, 1080), (1920, 1200), (1080, 1920)):
            edges = {}
            for edge, geom in (("top", f"{vw}x{thick}+0+0"), ("bottom", f"{vw}x{thick}+0+{vh - thick}"), ("left", f"{thick}x{vh}+0+0"), ("right", f"{thick}x{vh}+{vw - thick}+0")):
                cr = [contrast(text, tuple(round(panel[i] * alpha + c[i] * (1 - alpha)) for i in range(3))) for c in cells(full, vw, vh, geom)]
                edges[edge] = {"min_contrast": round(min(cr), 2), "mean_contrast": round(sum(cr) / len(cr), 2), "cells": len(cr)}
            rec["viewports"][f"{vw}x{vh}"] = edges
        reviews[f"{mode}:{iid}"] = rec
    json.dump({"images": images, "bar_legibility": reviews}, open(out, "w"), indent=1)

main()
