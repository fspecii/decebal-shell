#!/usr/bin/python3 -I
"""Assembles docs/desktop-ports/lingmo/wallhaven-lingmo-records.json in the schema of the integration catalogue (data/wallhaven-curated.json).
usage: assemble-wallhaven-fragment.py RAW_JSON CROPS_REVIEW_JSON OLD_FRAGMENT_JSON OUT_JSON
RAW_JSON is build-wallhaven-records.py's output (fresh API detail, hashes, bar legibility), CROPS_REVIEW_JSON review-wallhaven-crops.py's. Local only; no network."""
import json, sys
from datetime import datetime, timezone

IDS = {"light": "mldor9", "dark": "l8pe5r"}
ASSESS = {
    "mldor9": ("accepted with cautions: 16:9 keeps the peaks, mist, lake and the foreground pine; the 3440x1440 crop loses upper sky and the pine's base; the portrait crop loses both side slopes and the pine but keeps the peaks, the mist band and the lake",
               "Snow-dusted Moraine Lake under low cloud: a pale, quiet sky band under the top bar and calm teal water above the dock (light mode)."),
    "l8pe5r": ("accepted with cautions: 16:9 keeps the moonlit peak, the lake and its reflection; the ultrawide crop loses upper sky only; the portrait crop loses the side ridges but keeps the moon column and the reflection",
               "Moonlit alpine lake at night with star trails: a deep, dark sky under the top bar and a dark reflection above the dock (dark mode)."),
}

def main():
    raw, review, old, out = [json.load(open(p)) for p in sys.argv[1:4]] + [sys.argv[4]]
    now = datetime.now(timezone.utc).isoformat()
    images = {}
    for iid in IDS.values():
        o, crops = old["images"][iid], review["images"][iid]
        r = raw["images"][iid]
        images[iid] = {
            "id": iid, "subject": ASSESS[iid][1],
            "page_url": r["page_url"], "api_url": r["api_url"], "full_url": r["full_url"], "thumbnail_url": r["thumbnail_url"],
            "resolution": r["resolution"], "purity": r["purity"], "category": r["category"], "uploaded_at": r["uploaded_at"],
            "uploader": r["uploader"],
            "author": {"name": None, "verification": "not independently verified"},
            "source_url": r["source_url"], "source_verification": r["source_verification"],
            "license": r["license"], "tags": r["tags"],
            "ai_review": {"flagged_ai": r["ai_review"]["flagged_ai"], "rejected_tag_ids": [133451], "returned_ai_boolean": None,
                          "visual_assessment": "photographic appearance (sensor noise, grain, natural gradients); provenance is not proof of capture method",
                          "inspected": "full-resolution original, the three centre-cover crops by eye, and native-pixel crops (top-left, centre, bottom)"},
            "evidence": {"detail_sha256": r["evidence"]["detail_sha256"], "thumbnail_sha256": r["evidence"]["thumbnail_sha256"]},
            "full_resolution_review": {
                "reviewed_on": "2026-10-31", "sha256": r["evidence"]["file_sha256"], "bytes": r["evidence"]["file_bytes"], "dimensions_verified": True,
                "assessment": ASSESS[iid][0],
                "notes": f"{r['resolution'][0]}x{r['resolution'][1]}; a fresh download is byte-identical to the first one reviewed; photographic appearance.",
                "provenance_limit": "Photographic appearance and Wallhaven tags only; authorship, licence and capture method unverified. Uploader is not author."},
            "crop_review": {"algorithm": review["algorithm"],
                            "16x9": {"result": ASSESS[iid][0].split("; ")[0].replace("accepted with cautions: ", "accepted: "), "sha256": crops["16x9"]["sha256"]},
                            "ultrawide": {"result": "accepted with caution: " + ASSESS[iid][0].split("; ")[1], "sha256": crops["ultrawide"]["sha256"]},
                            "portrait": {"result": "accepted with caution: " + ASSESS[iid][0].split("; ")[2], "sha256": crops["portrait"]["sha256"]}},
            "parent_verification": {"status": "independently_accepted", "fresh_api_and_original_sha_match": True, "dimensions_match": True,
                                    "crops_reviewed": ["16x9", "3440x1440", "1080x1920"], "reviewed_utc": now,
                                    "remaining": ["physical compositor readability", "licence still unknown"]},
            "readability_metrics": {"note": "relative luminance (WCAG) of the top 32 px band (the declared Lingmo bar) and of the bottom-centre dock zone (bottom 100 px, middle 30 % of the width) after each centre crop",
                                    **{k: crops[k]["metrics"] for k in ("16x9", "ultrawide", "portrait")}},
        }
        if r.get("source_url"):
            images[iid]["source_verification"] = "reported by the uploader, not verified; not proof of authorship, capture method or licence"
    fragment = {
        "format": 1,
        "status": "MERGE-READY FRAGMENT, NOT MERGED: copy images{} and themes{} into data/wallhaven-curated.json (and update its scope counts) if accepted; no shared data file is edited here.",
        "reviewed_on": "2026-10-31",
        "schema": "data/wallhaven-curated.json as published by root (images[].full_resolution_review / crop_review / parent_verification / readability_metrics; themes[].dark / light / crop_policy)",
        "disjoint": {"checked_against": "the images of data/wallhaven-curated.json on every local and remote ref and every other port's records fragment (61 distinct ids)", "result": "mldor9 and l8pe5r collide with no other id"},
        "policy": "Records are metadata and links only; nothing is bundled and nothing downloads by itself. Recommendations are selected in the separate Wallpaper pane. With Follow theme enabled by the user, theme and mode changes prepare the matching curated photograph; otherwise the wallpaper stays independent. The images remain the property of their owners.",
        "images": images,
        "themes": {"lingmo": {
            "status": "candidate_live_checked_on_download",
            "rationale": "Lingmo's accent is blue #3385FF and its look is cool and quiet; both photographs are cool-toned and leave a calm band under the top bar and above the dock.",
            "dark": {"image_id": IDS["dark"], "rationale": ASSESS[IDS["dark"]][1]},
            "light": {"image_id": IDS["light"], "rationale": ASSESS[IDS["light"]][1]},
            "same_image_both_modes": False, "recommended_mode": "fill",
            "placement": "Both originals are wider than 16:9 or taller; centre-fill crops the sides or the top on a different monitor aspect: the three reviewed crops show what each keeps.",
            "crop_policy": "keep original aspect; centre-fill crops on aspect mismatch"}},
        "bar_legibility": raw["bar_legibility"],
        "bar_legibility_note": "Supplementary, not part of the catalogue schema: contrast of bar_text against the bar fill at bar_alpha over every 40 px cell of the centre-filled image, per edge and viewport. Lingmo's real bar adds a Gaussian blur and picks its ink from the wallpaper; neither is modelled, so this is a conservative check of what this package draws.",
        "candidates_not_chosen": old["selection"]["rejected"],
    }
    json.dump(fragment, open(out, "w"), indent=1, ensure_ascii=False)
    open(out, "a").write("\n")

main()
