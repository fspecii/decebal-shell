#!/usr/bin/env python3
"""Merge this port's wallpaper records into data/wallhaven-curated.json (root-owned file; root runs this, it is not applied by the lane).

Usage: python3 -I merge_catalogue.py CATALOGUE_IN CATALOGUE_OUT
Adds the five `themes` entries and their ten `images` unchanged in shape; refuses to overwrite an existing id and checks every image id is new and every theme is not yet present.
"""
import json, pathlib, sys

here = pathlib.Path(__file__).resolve().parent
mine = json.loads((here.parent / "wallpapers.json").read_text()); cat = json.loads(pathlib.Path(sys.argv[1]).read_text())
for k in mine["images"]:
    assert k not in cat["images"], f"image {k} already in the catalogue"
for k in mine["themes"]:
    assert k not in cat["themes"], f"theme {k} already in the catalogue"
cat["images"].update(mine["images"]); cat["themes"].update(mine["themes"])
cat["scope"]["openkylin_integration"] = "five openKylin packages (v11, islands, Light-Seeking, HeYin, Classic), ten new distinct photographs; authorship and licence unknown."
pathlib.Path(sys.argv[2]).write_text(json.dumps(cat, indent=1, ensure_ascii=False) + "\n")
print("merged", len(mine["themes"]), "themes,", len(mine["images"]), "images")
