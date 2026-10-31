#!/usr/bin/env python3
"""Rebuild wallpapers.json: fetch + verify each pick (wh_fetch.py), then attach rationale and policy.

Usage: python3 -I make_wallpapers.py REPO_ROOT CACHE_DIR      (network: Wallhaven API + full-resolution files, polite 1 req/s)
"""
import json, pathlib, subprocess, sys, tempfile

root, cache = pathlib.Path(sys.argv[1]), sys.argv[2]
here = pathlib.Path(__file__).resolve().parent
picks = json.load(open(here / "wallpaper_picks.json"))
with tempfile.TemporaryDirectory() as td:
    pj = pathlib.Path(td) / "picks.json"; raw = pathlib.Path(td) / "raw.json"
    pj.write_text(json.dumps({t: {m: v[m][0] for m in ("light", "dark")} for t, v in picks.items()}))
    subprocess.run([sys.executable, "-I", str(here / "wh_fetch.py"), str(pj), cache, str(raw)], check=True)
    d = json.loads(raw.read_text())
reported = ("Trey Ratcliff", "Paul Wilson", "Marek Piwnicki", "Darkcloud Photography")
for v in d["images"].values():
    cand = next((t["name"] for t in v["tags"] if t["name"] in reported), None)
    v["author"]["reported_candidate"] = cand; v["author"]["candidate_basis"] = "Wallhaven detail tags only" if cand else None
    v["provenance_limit"] = "Photographic appearance and Wallhaven tags only; authorship, licence and capture method unverified. Uploader is not author."
# records in the shape of data/wallhaven-curated.json, so the file is a drop-in fragment for that catalogue (see merge_catalogue.py)
for wid, v in list(d["images"].items()):
    w, h = v["resolution"]
    d["images"][wid] = {
        "id": wid, "subject": next((r for t, p in picks.items() for m in ("light", "dark") if p[m][0] == wid for r in [p[m][1]]), ""),
        "page_url": v["page_url"], "api_url": v["api_url"], "full_url": v["full_url"], "thumbnail_url": v["thumbnail_url"],
        "resolution": v["resolution"], "purity": v["purity"], "category": v["category"], "uploaded_at": v["uploaded_at"], "uploader": v["uploader"],
        "author": v["author"], "source_url": v["source_url"], "source_verification": "source not provided by Wallhaven" if not v["source_url"] else "source link as reported by Wallhaven; not independently verified",
        "license": v["license"], "tags": v["tags"],
        "ai_review": {"flagged_ai": False, "rejected_tag_ids": [133451], "returned_ai_boolean": None, "visual_assessment": "photographic appearance; provenance is not proof of capture method",
                      "inspected": "contact-sheet thumbnail and 640x360 downsample of the full-resolution original"},
        "evidence": {"detail_sha256": v["detail_sha256"], "thumbnail_sha256": v["thumbnail_sha256"]},
        "full_resolution_review": {"reviewed_on": v["reviewed_on"], "sha256": v["sha256"], "bytes": v["bytes"], "dimensions_verified": True,
                                    "assessment": "usable with stated crop/readability cautions", "notes": f"{w}x{h} ({w / h:.3f}:1); photographic appearance with colour grading; authorship, licence and capture method unverified.",
                                    "provenance_limit": v["provenance_limit"]}}
themes = {t: {"status": "candidate_live_checked_on_download", "rationale": v["why"],
              **{m: {"image_id": v[m][0], "rationale": v[m][1]} for m in ("dark", "light")},
              "same_image_both_modes": False, "recommended_mode": "fill",
              "placement": "All images are native 16:9: centre-fill crops nothing at 16:9; verify ultrawide/portrait separately.",
              "crop_policy": "keep original aspect; centre-fill crops on aspect mismatch"} for t, v in picks.items()}
d["themes"] = themes
d["policy"] = {"purity": "sfw", "category": "general", "min_resolution": "2560x1440", "ratio": "16x9",
               "ai_filter": "rejected AI-art tag 133451 and AI-generator tag names on each detail response; absence of a tag is not proof of non-AI origin",
               "copyright": "Images remain the property of their owners. This file stores links, hashes and metadata only; no image is redistributed in the package.",
               "runtime": "Belongs to Wallpaper → For this theme. Packages set no wallpaper; apply never downloads.",
               "distinct_from": "data/wallhaven-curated.json (42 ids); all ids here are distinct from it and from each other"}
(root / "assets/desktop-ports/openkylin/wallpapers.json").write_text(json.dumps(d, indent=1, ensure_ascii=False) + "\n")
print(len(d["images"]), "images,", len(themes), "themes")
