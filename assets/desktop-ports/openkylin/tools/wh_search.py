#!/usr/bin/env python3
"""Search Wallhaven (SFW, general, >=2560x1440, 16:9) and list candidates not already curated. Read-only API use.

Usage: python3 -I wh_search.py CURATED_JSON OUT_JSON 'query' [colors-hex ...]
"""
import json, sys, time, urllib.request, urllib.parse

curated = json.load(open(sys.argv[1]))
used = set(curated["images"]) | {r.get("id") for r in curated.get("rejected_recommendations", [])}
q = sys.argv[3]; colors = sys.argv[4:] or [None]
res = {}
for col in colors:
    params = {"q": q, "categories": "100", "purity": "100", "sorting": "favorites", "atleast": "2560x1440", "ratios": "16x9", "order": "desc"}
    if col: params["colors"] = col
    url = "https://wallhaven.cc/api/v1/search?" + urllib.parse.urlencode(params)
    req = urllib.request.Request(url, headers={"User-Agent": "decebal-openkylin-curation/1"})
    data = json.load(urllib.request.urlopen(req, timeout=30))
    for it in data["data"]:
        if it["id"] in used: continue
        res.setdefault(it["id"], {k: it[k] for k in ("id", "url", "resolution", "file_size", "favorites", "colors", "path")} | {"thumb": it["thumbs"]["large"], "query": q, "color": col})
    time.sleep(1.2)
json.dump(res, open(sys.argv[2], "w"), indent=1)
print(len(res), "candidates")
for k, v in res.items(): print(k, v["resolution"], v["favorites"], v["colors"][:3])
