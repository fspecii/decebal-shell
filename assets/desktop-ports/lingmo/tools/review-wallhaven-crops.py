#!/usr/bin/python3 -I
"""Local review of Wallhaven originals for the Lingmo records: three centre-cover crops per image, their hashes, and readability metrics.
usage: review-wallhaven-crops.py ORIGINALS_DIR OUT_DIR ID...      (OUT_DIR must not exist; ORIGINALS_DIR/<id>.img are the downloaded originals)
No network. Needs ImageMagick (`magick`). Crop algorithm (stated in the records): centre cover crop then Lanczos resize, JPEG quality 90, the files hashed.
Readability: relative luminance (WCAG) of the top 32 px band (the declared bar) and of the bottom-centre dock zone (bottom 100 px, middle 30 % of the width)
of each crop: p95 and max for the panel, p95 for the dock zone. Not a judgement of taste, authorship or licence."""
import hashlib, json, os, subprocess, sys

CROPS = {"16x9": (1920, 1080), "ultrawide": (3440, 1440), "portrait": (1080, 1920)}

def lum(r, g, b):
    f = lambda v: v / 12.92 if v <= 0.03928 else ((v + 0.055) / 1.055) ** 2.4
    return 0.2126 * f(r / 255) + 0.7152 * f(g / 255) + 0.0722 * f(b / 255)

def region(path, w, h, x, y, rw, rh):
    raw = subprocess.run(["magick", path, "-crop", f"{rw}x{rh}+{x}+{y}", "+repage", "-depth", "8", "rgb:-"], capture_output=True, check=True).stdout
    return [lum(raw[i], raw[i + 1], raw[i + 2]) for i in range(0, len(raw), 3)]

def pct(vals, p):
    s = sorted(vals)
    return round(s[min(len(s) - 1, int(p * len(s)))], 3)

def main():
    src, out = sys.argv[1:3]
    ids = sys.argv[3:]
    if os.path.exists(out):
        sys.exit(f"refusing to reuse {out}")
    os.makedirs(out)
    version = subprocess.run(["magick", "-version"], capture_output=True, text=True).stdout.splitlines()[0]
    result = {"algorithm": f"centre cover crop then Lanczos resize ({version.split(' x86')[0]}); JPEG q90 files hashed", "images": {}}
    for i in ids:
        original = os.path.join(src, f"{i}.img")
        rec = {}
        for name, (w, h) in CROPS.items():
            path = os.path.join(out, f"{i}-{name}.jpg")
            subprocess.run(["magick", original, "-filter", "Lanczos", "-resize", f"{w}x{h}^", "-gravity", "center", "-extent", f"{w}x{h}", "-quality", "90", path], check=True)
            panel = region(path, w, h, 0, 0, w, 32)
            dock = region(path, w, h, int(w * 0.35), h - 100, int(w * 0.30), 100)
            rec[name] = {"file": os.path.basename(path), "sha256": hashlib.sha256(open(path, "rb").read()).hexdigest(), "size": [w, h],
                         "metrics": {"panel_p95": pct(panel, 0.95), "panel_max": round(max(panel), 3), "dock_zone_p95": pct(dock, 0.95)}}
        result["images"][i] = rec
    json.dump(result, open(os.path.join(out, "review.json"), "w"), indent=1)

main()
