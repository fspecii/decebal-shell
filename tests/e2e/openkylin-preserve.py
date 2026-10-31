#!/usr/bin/env python3
"""Applying an openKylin package must keep the user's separate choices (private HOME/XDG, no display, no live config).

Usage: python3 tests/e2e/openkylin-preserve.py BIN_DIR
Applies each package, then switches palette and mode; seeds a private decebal.json with an icon theme and cursor theme (explicit asset-pack choices), an explicit palette (`decthemectl palette nord`), panel overrides (bar edge + applets), a user wallpaper, rdwm tiling values and widgets, runs
`decthemectl apply <id>` for every package and both modes, and asserts those keys are byte-for-byte unchanged while the theme itself changed.
"""
import json, os, pathlib, subprocess, sys, tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
BIN = pathlib.Path(sys.argv[1]).resolve()
IDS = ["openkylin", "openkylin-islands", "openkylin-light-seeking", "openkylin-heyin", "openkylin-classic"]
SEED = {
    "icons": {"theme": "Papirus-Dark", "size_scale": 1.1},
    "bar": {"edge": "right", "applets": {"weather": True, "clock": False}},
    "wallpaper": {"image": "/user/own/wall.jpg", "mode": "fit", "match_colors": False},
    "rdwm": {"layouts": ["tile", "monocle"], "mfact": 0.62, "nmaster": 2, "smart_gaps": True},
    "widgets": {"enabled": True, "items": [{"use": "builtin:clock", "place": "desktop", "x": 10, "y": 10}]},
    "system": {"options": {"gtk.font": False}, "cursorTheme": "Adwaita"},
    "asset_packs": {"icons": {"user": True}, "cursors": {"user": True}},   # explicit choices made in the Icons pane / `decthemectl assets use`
}
failures = []
for pid in IDS:
    for mode in ("light", "dark"):
        with tempfile.TemporaryDirectory(prefix="okp-") as td:
            r = pathlib.Path(td)
            for d in ("home", "cfg", "cache", "state", "run", "data"): (r / d).mkdir()
            (r / "cfg/decebal.json").write_text(json.dumps(SEED))
            env = dict(PATH=os.environ["PATH"], LANG="C.UTF-8", HOME=str(r / "home"), XDG_CONFIG_HOME=str(r / "home"), XDG_CACHE_HOME=str(r / "cache"), XDG_DATA_HOME=str(r / "data"),
                       XDG_STATE_HOME=str(r / "state"), XDG_RUNTIME_DIR=str(r / "run"), DECEBAL_CONFIG=str(r / "cfg"), DECEBAL_THEMES=str(ROOT / "themes"), DECEBAL_SYSTEM_THEMES=str(r / "home"),
                       DECEBAL_PALETTES=str(ROOT / "palettes"), DECEBAL_SYSTEM_AUTOMATIC="0", DBUS_SESSION_BUS_ADDRESS="unix:path=/nonexistent")
            a = subprocess.run([str(BIN / "decthemectl"), "apply", pid, "--mode", mode, "--no-rdwm", "--keep-widgets"], env=env, text=True, capture_output=True)
            if a.returncode != 0: failures.append(f"{pid}/{mode}: apply failed: {a.stderr.strip()}"); continue
            # an explicit palette chosen after the apply must survive a mode switch (selection-only commits keep it; a *full* apply resets colours by design)
            other = "dark" if mode == "light" else "light"
            for step in (("palette", "nord"), ("set", "mode", other)):
                st = subprocess.run([str(BIN / "decthemectl"), *step], env=env, text=True, capture_output=True)
                if st.returncode != 0: failures.append(f"{pid}/{mode}: {' '.join(step)} failed: {st.stderr.strip()[:200]}")
            after = json.loads((r / "cfg/decebal.json").read_text())
            if after.get("theme", {}).get("name") != pid or after["theme"].get("mode") != other: failures.append(f"{pid}/{mode}: theme/mode not as chosen: {after.get('theme')}")
            if after.get("theme", {}).get("palette") != "nord": failures.append(f"{pid}/{mode}: the explicit palette did not survive the mode switch: {after.get('theme')}")
            for k in ("icons", "wallpaper", "rdwm", "system", "asset_packs"):
                if after.get(k) != SEED[k]: failures.append(f"{pid}/{mode}: {k} changed: {after.get(k)}")
            if after.get("bar", {}).get("edge") != "right" or after["bar"].get("applets") != SEED["bar"]["applets"]: failures.append(f"{pid}/{mode}: user panel choices changed: {after.get('bar')}")
            if after.get("widgets", {}).get("items") != SEED["widgets"]["items"]: failures.append(f"{pid}/{mode}: widgets changed with --keep-widgets")
            cur = lambda d: sorted(k for k in json.dumps(d).replace('"', " ").split() if "cursor" in k.lower())
            if cur(after) != cur(SEED): failures.append(f"{pid}/{mode}: cursor settings changed: {cur(after)} vs {cur(SEED)}")
if failures:
    print("\n".join(failures)); sys.exit(1)
print(f"PASS {len(IDS) * 2} applies kept explicit icons/cursor choices, palette (across a mode switch), wallpaper, tiling, panel overrides and widgets")
