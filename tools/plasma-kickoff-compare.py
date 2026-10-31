#!/usr/bin/env python3
"""Original-versus-native comparison of Kickoff's footer, from matching crops (655 x 516 logical px at 100 %, the same mode).

  python3 tools/plasma-kickoff-compare.py REFS_DIR NATIVE_DIR OUT.json

REFS_DIR holds the pinned plasma-workspace clone (`plasma-workspace/lookandfeel/org.kde.breeze[dark]/contents/previews/fullscreenpreview.jpg`, the
identified original capture), NATIVE_DIR the PNGs `native-{dark,light}-applications.png` that `cargo test --lib kickoff_capture_dark|light -- --ignored`
writes. The original is never copied into the repository (it keeps its upstream licence); only its path, commit and sha256 are recorded, with the offsets
of the footer's content groups: for each group of ink along the footer row, the native start and end minus the original's.
"""
import hashlib
import json
import sys
from pathlib import Path

from PIL import Image

CARD = (8, 504, 663, 1020)  # the card in the 1920 x 1080 capture
ROW = (478, 512)  # footer text row inside the card crop


def groups(im, gap=9):
    px = im.load()
    bg = px[330, 495]
    cols = [any(max(abs(px[x, y][i] - bg[i]) for i in range(3)) > 70 for y in range(*ROW)) for x in range(im.width)]
    out, start, last = [], None, None
    for x, v in enumerate(cols):
        if v:
            if start is None:
                start = last = x
            elif x - last > gap:
                out.append((start, last))
                start = x
            last = x
    if start is not None:
        out.append((start, last))
    return out


def main():
    refs, native, out = (Path(a) for a in sys.argv[1:4])
    report = {"source": {}, "modes": {}}
    for mode, name in (("dark", "breezedark"), ("light", "breeze")):
        path = refs / "plasma-workspace/lookandfeel" / f"org.kde.{name}/contents/previews/fullscreenpreview.jpg"
        original = Image.open(path).convert("RGB").crop(CARD)
        n = Image.open(native / f"native-{mode}-applications.png").convert("RGBA")
        bg = Image.new("RGBA", n.size, (0, 0, 0, 255))
        bg.alpha_composite(n)
        mine = bg.convert("RGB")
        go, gn = groups(original), groups(mine)
        report["source"][mode] = {"path": f"plasma-workspace/lookandfeel/org.kde.{name}/contents/previews/fullscreenpreview.jpg", "sha256": hashlib.sha256(path.read_bytes()).hexdigest(), "size": [1920, 1080], "card_crop": CARD}
        report["modes"][mode] = {"original_groups": go, "native_groups": gn, "offsets_native_minus_original": [[a[0] - b[0], a[1] - b[1]] for a, b in zip(gn, go)], "group_count_equal": len(go) == len(gn)}
    out.write_text(json.dumps(report, indent=2))
    print(json.dumps({m: r["offsets_native_minus_original"] for m, r in report["modes"].items()}))


if __name__ == "__main__":
    main()
