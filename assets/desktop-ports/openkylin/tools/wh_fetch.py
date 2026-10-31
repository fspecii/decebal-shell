#!/usr/bin/env python3
"""Fetch Wallhaven detail records + full-resolution files for the chosen ids; verify and record provenance.

Usage: python3 -I wh_fetch.py PICKS_JSON CACHE_DIR OUT_JSON
PICKS_JSON: {"theme": {"light": "id", "dark": "id"}, ...}.  Nothing is added to the repository except OUT_JSON.
Rejects non-SFW, non-general, AI-art tag 133451 or AI-generator tag names. Unknown author/licence stay null.
"""
import json, sys, hashlib, time, urllib.request, pathlib, subprocess, datetime

picks = json.load(open(sys.argv[1])); cache = pathlib.Path(sys.argv[2]); cache.mkdir(parents=True, exist_ok=True)
H = {"User-Agent": "decebal-openkylin-curation/1"}
AI_NAMES = ("ai art", "ai generated", "ai-generated", "stable diffusion", "midjourney", "dall-e", "dalle", "ai_art", "artificial intelligence", "neural")

def get(url, **kw):
    return urllib.request.urlopen(urllib.request.Request(url, headers=H), timeout=kw.get("timeout", 60)).read()

images, themes = {}, {}
for theme, modes in picks.items():
    themes[theme] = {}
    for mode, wid in modes.items():
        themes[theme][mode] = {"image_id": wid}
        if wid in images: continue
        raw = get(f"https://wallhaven.cc/api/v1/w/{wid}")
        d = json.loads(raw)["data"]; time.sleep(1.0)
        tags = [{"id": t["id"], "name": t["name"]} for t in d.get("tags", [])]
        assert d["purity"] == "sfw" and d["category"] == "general", wid
        assert not any(t["id"] == 133451 or any(a in t["name"].lower() for a in AI_NAMES) for t in tags), f"{wid}: AI tag"
        f = cache / f"wallhaven-{wid}.{d['file_type'].split('/')[-1].replace('jpeg','jpg')}"
        if not f.exists():
            f.write_bytes(get(d["path"], timeout=300)); time.sleep(1.0)
        data = f.read_bytes(); sha = hashlib.sha256(data).hexdigest()
        dim = subprocess.run(["identify", "-format", "%w %h", str(f) + "[0]"], capture_output=True, text=True).stdout.split()
        w, h = int(dim[0]), int(dim[1])
        assert [w, h] == [int(d["dimension_x"]), int(d["dimension_y"])], (wid, w, h)
        th_url = d["thumbs"]["large"]; th = get(th_url); time.sleep(1.0)
        images[wid] = {
            "id": wid, "page_url": d["url"], "api_url": f"https://wallhaven.cc/api/v1/w/{wid}", "full_url": d["path"],
            "resolution": [w, h], "bytes": len(data), "sha256": sha, "purity": d["purity"], "category": d["category"],
            "uploaded_at": d["created_at"], "uploader": {"username": d["uploader"]["username"], "is_author": None},
            "author": {"name": None, "verification": "not independently verified"},
            "source_url": d.get("source") or None,
            "license": {"identifier": None, "url": None, "status": "unknown; no redistribution permission established"},
            "tags": tags, "colors": d.get("colors", []),
            "detail_sha256": hashlib.sha256(raw).hexdigest(), "thumbnail_url": th_url, "thumbnail_sha256": hashlib.sha256(th).hexdigest(),
            "reviewed_on": datetime.date.today().isoformat(),
        }
out = {"format": 1, "curated_on": datetime.date.today().isoformat(), "provider": "https://wallhaven.cc", "images": images, "themes": themes}
json.dump(out, open(sys.argv[3], "w"), indent=1, ensure_ascii=False)
print(len(images), "images")
