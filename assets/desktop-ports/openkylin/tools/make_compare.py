#!/usr/bin/env python3
"""Side-by-side sheets: the official upstream global-theme preview image (ukui-globaltheme, a theme mock-up, not a screenshot of a running desktop) next to the native Decebal captures of the
same theme and mode. Writes one PNG per theme and replica-compare.json (identities: upstream commit, file blob and sha256, our capture sha256 and size, scale, mode).
Usage: python3 -I make_compare.py RUN_DIR UKUI_GLOBALTHEME_CHECKOUT OUT_DIR   (RUN_DIR holds launcher/ and cc/ native outputs; read-only inputs, new OUT_DIR)"""
import hashlib, json, pathlib, subprocess, sys
from PIL import Image, ImageDraw

run, gt, out = (pathlib.Path(a).resolve() for a in sys.argv[1:4]); out.mkdir(parents=True, exist_ok=False)
commit = subprocess.check_output(["git", "-C", str(gt), "rev-parse", "HEAD"], text=True).strip()
sha = lambda p: hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
H = 600
THEMES = [("Light-Seeking", "openkylin-light-seeking", "light", None), ("HeYin", "openkylin-heyin", "dark", "openkylin-heyin"), ("Classic", "openkylin-classic", "light", "openkylin-classic")]
meta = []
for gname, pid, mode, ccid in THEMES:
    ref = gt / "globaltheme" / gname / "preview.png"
    tiles = [("ORIGINAL: ukui-globaltheme/%s/preview.png (mock-up)" % gname, Image.open(ref).convert("RGB"))]
    files = {"original": dict(path=f"globaltheme/{gname}/preview.png", sha256=sha(ref), commit=commit)}
    start = run / "launcher" / f"startmenu-{pid}-{mode}-100.png"
    tiles.append((f"NATIVE start menu: {pid}, {mode}, 100 %, 1920x1080 private Xvfb", Image.open(start).convert("RGB"))); files["start_menu"] = dict(file=start.name, sha256=sha(start))
    if ccid:
        cc = run / "cc" / f"controlcenter-{ccid.replace('openkylin-', 'openkylin-') if ccid != 'openkylin' else 'openkylin'}-{mode}-100.png"
        if cc.exists(): tiles.append((f"NATIVE shortcut panel: {ccid}, {mode}, 100 %", Image.open(cc).convert("RGB"))); files["shortcut_panel"] = dict(file=cc.name, sha256=sha(cc))
    scaled = [(c, im.resize((round(im.width * H / im.height), H), Image.LANCZOS)) for c, im in tiles]
    W = sum(i.width for _, i in scaled) + 12 * (len(scaled) + 1); sheet = Image.new("RGB", (W, H + 56), (24, 24, 24)); d = ImageDraw.Draw(sheet); x = 12
    for cap, im in scaled:
        sheet.paste(im, (x, 40)); d.text((x, 12), cap, fill=(230, 230, 230)); x += im.width + 12
    d.text((12, H + 44), "Different wallpaper and application list: compare composition, not pixels. The upstream image is a theme preview mock-up, not a capture of a running UKUI desktop.", fill=(170, 170, 170))
    name = f"replica-compare-{pid}-{mode}.png"; sheet.save(out / name); meta.append(dict(sheet=name, theme=pid, mode=mode, scale=1.0, inputs=files))
(out / "replica-compare.json").write_text(json.dumps(dict(upstream="https://gitee.com/openkylin/ukui-globaltheme.git", commit=commit, sheets=meta), indent=1))
print(len(meta), "sheets")
