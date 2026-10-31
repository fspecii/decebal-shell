#!/usr/bin/env python3
"""Native openKylin package captures on a private Xvfb + picom(xrender): real decdock/decbar, no live session.

Usage: python3 tests/e2e/openkylin-native.py BIN_DIR OUT_DIR [--matrix full|quick] [--wallpapers DIR] [--icons-prefix XDG_DATA_DIR]

BIN_DIR holds decthemectl, decdock, decbar (a build of the shared panel framework, >= 0.3.0). Every case owns its Xvfb (an explicitly reserved display number >= 700, verified as ours in the lock file and in both the filesystem and the abstract socket namespaces before any input; never :0, never an auto-probe), HOME and XDG
dirs and a dead session bus; rdwm, curl, nmcli... are never started. Committed evidence uses a flat backdrop (no photograph is
redistributed); --wallpapers DIR (files wallhaven-<id>.*) adds photograph-backed captures meant to stay private.
Per case it records the real window geometry, the pixel colour of an empty part of the panel (must equal the package's panel colour
composited at its alpha over the backdrop) and the binary hashes, in OUT_DIR/native-checks.json.
"""
import hashlib, importlib.util, json, os, pathlib, shutil, subprocess, sys, tempfile, time, tomllib
from PIL import Image

ROOT = pathlib.Path(__file__).resolve().parents[2]
args = [a for a in sys.argv[1:] if not a.startswith("--")]
opt = lambda n, d=None: sys.argv[sys.argv.index(n) + 1] if n in sys.argv else d
BIN, OUT = pathlib.Path(args[0]).resolve(), pathlib.Path(args[1]).resolve(); OUT.mkdir(parents=True, exist_ok=True)
MATRIX = opt("--matrix", "quick"); WALLS = opt("--wallpapers"); ICONS = opt("--icons-prefix")
BACKDROP = {"light": (0xb4, 0xc0, 0xd0), "dark": (0x38, 0x40, 0x4c)}
PICKS = json.loads((ROOT / "assets/desktop-ports/openkylin/wallpapers.json").read_text())["themes"]


def set_root(display, image):  # `display` is the private Xvfb this case owns
    """Paint the root window from `image` and publish _XROOTPMAP_ID (what picom reads); the pixmap outlives this connection."""
    from Xlib import X, display as xdisplay
    d = xdisplay.Display(display); root = d.screen().root; w, h = root.get_geometry().width, root.get_geometry().height
    im = Image.open(image).convert("RGB")
    # cover-fit (what a wallpaper "fill" does), then BGRX for a 24-bit ZPixmap
    sc = max(w / im.width, h / im.height); im = im.resize((round(im.width * sc), round(im.height * sc)), Image.LANCZOS)
    im = im.crop(((im.width - w) // 2, (im.height - h) // 2, (im.width - w) // 2 + w, (im.height - h) // 2 + h))
    raw = Image.merge("RGBA", (*im.split()[::-1], Image.new("L", im.size, 255))).tobytes()
    pm = root.create_pixmap(w, h, d.screen().root_depth); gc = pm.create_gc()
    step = 8   # rows per request: stays under the core protocol's 256 KiB request limit
    for y in range(0, h, step):
        n = min(step, h - y); pm.put_image(gc, 0, y, w, n, X.ZPixmap, d.screen().root_depth, 0, raw[y * w * 4:(y + n) * w * 4])
    d.set_close_down_mode(X.RetainPermanent)
    for name in ("_XROOTPMAP_ID", "ESETROOT_PMAP_ID"):
        root.change_property(d.intern_atom(name), d.intern_atom("PIXMAP"), 32, [pm.id])
    root.change_attributes(background_pixmap=pm); root.clear_area(0, 0, w, h); d.sync(); d.close()


def sha(p): return hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()


_spec = importlib.util.spec_from_file_location("openkylin_fixture", pathlib.Path(__file__).with_name("openkylin_fixture.py"))
fixture = importlib.util.module_from_spec(_spec); _spec.loader.exec_module(fixture)
FIRST_DISPLAY, abstract_sockets, free_display, verify_owned = fixture.FIRST_DISPLAY, fixture.abstract_sockets, fixture.free_display, fixture.verify_owned


USED_DISPLAYS = set()


def wait(fn, label, n=150):
    for _ in range(n):
        try:
            r = fn()
            if r: return r
        except (OSError, ValueError, subprocess.CalledProcessError): pass
        time.sleep(0.1)
    raise AssertionError(label)


def case(theme, mode, scale, geom, edge=None, wallpaper=None, shot=None, widget=None, extra=None):
    with tempfile.TemporaryDirectory(prefix="okn-") as td:
        r = pathlib.Path(td)
        for d in ("home", "cfg", "cache", "state", "run", "data"): (r / d).mkdir(mode=0o700)
        if ICONS: os.symlink(pathlib.Path(ICONS) / "icons", r / "data/icons")
        n = free_display(USED_DISPLAYS)
        env = dict(PATH=os.environ["PATH"], LANG="C.UTF-8", HOME=str(r / "home"), XDG_CONFIG_HOME=str(r / "home"), XDG_CACHE_HOME=str(r / "cache"), XDG_DATA_HOME=str(r / "data"), XDG_STATE_HOME=str(r / "state"),
                   XDG_RUNTIME_DIR=str(r / "run"), DECEBAL_CONFIG=str(r / "cfg"), DECEBAL_THEMES=str(ROOT / "themes"), DECEBAL_SYSTEM_THEMES=str(r / "home"),
                   DECEBAL_PALETTES=str(ROOT / "palettes"), DECEBAL_SYSTEM_AUTOMATIC="0", DECEBAL_SCALE=str(scale), 
                   DISPLAY=f":{n}", DBUS_SESSION_BUS_ADDRESS="unix:path=/nonexistent")
        stub = r / "bin"; stub.mkdir()
        for app in ("curl", "nmcli", "pactl", "wpctl", "playerctl", "bluetoothctl", "brightnessctl", "rdwmctl"):
            (stub / app).write_text("#!/bin/sh\nexit 1\n"); (stub / app).chmod(0o755)
        env["PATH"] = f"{stub}:{env['PATH']}"
        (r / "cfg/decebal.json").write_text(json.dumps({"icons": {"theme": "ukui-icon-theme-default"}}))
        procs = []
        try:
            def run(*a, **kw): return subprocess.run(a, env=env, text=True, capture_output=True, **kw).stdout.strip()
            procs.append(subprocess.Popen(["Xvfb", f":{n}", "-screen", "0", f"{geom}x24", "-nolisten", "tcp"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
            verify_owned(n, procs[-1]); assert env["DISPLAY"] == f":{n}"
            wait(lambda: run("xdotool", "getdisplaygeometry"), "xvfb")
            a = subprocess.run([str(BIN / "decthemectl"), "apply", theme, "--mode", mode, "--no-rdwm"], env=env, text=True, capture_output=True); assert a.returncode == 0, a.stderr
            if edge:
                p = r / "cfg/dock-settings.json"; d = json.loads(p.read_text()) if p.exists() else {}; d["edge"] = edge; p.write_text(json.dumps(d))
                p = r / "cfg/decebal.json"; d = json.loads(p.read_text()); d.setdefault("bar", {})["edge"] = edge; p.write_text(json.dumps(d))
            if widget:   # switch the package's own saved-but-off UKUI time widget on, as the user would in Settings -> Widgets
                p = r / "cfg/decebal.json"; d = json.loads(p.read_text())
                d["widgets"] = {"enabled": True, "items": [{"use": widget, "place": "desktop", "anchor": "top-right", "x": -480, "y": 96, "enabled": True, "source": theme, "from_theme": True}]}; p.write_text(json.dumps(d))
            if wallpaper: set_root(env["DISPLAY"], wallpaper)
            else:
                # picom reads the root pixmap property, which xsetroot -solid does not set; the flat image is a test fixture only
                flat = r / "backdrop.png"; Image.new("RGB", (64, 64), BACKDROP[mode]).save(flat); set_root(env["DISPLAY"], flat)
            (r / "picom.conf").write_text('backend = "xrender"; use-damage = false; vsync = false;\nblur: { method = "gaussian"; size = 12; deviation = 5; };\n'
                                         'rules = ( { blur-background = false; }, { match = "_DECEBAL_BLUR@ = 1"; blur-background = true; } );\n')
            procs.append(subprocess.Popen(["picom", "--config", str(r / "picom.conf")], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)); time.sleep(0.8)
            for b in (("decdock", "decbar") + (("decwidgets",) if widget else ())):
                # DECEBAL_SCALE (every process, `apply` included) sizes the taskbar; the bar also follows DECEBAL_FONT_SCALE.
                # DECEBAL_FONT_SCALE on the taskbar double-scales its text, so only the bar gets it.
                benv = dict(env, **({"DECEBAL_FONT_SCALE": str(scale)} if b == "decbar" else {}))
                procs.append(subprocess.Popen([str(BIN / b)], env=benv, stdout=(r / f"{b}.log").open("w"), stderr=subprocess.STDOUT))
            if extra:   # one more shell process (e.g. the Control Center summoned by its key), started after the panel
                time.sleep(1.0); procs.append(subprocess.Popen([str(BIN / extra[0]), *extra[1:]], env=env, stdout=(r / f"{extra[0]}.log").open("w"), stderr=subprocess.STDOUT)); time.sleep(2.5)
            wins = {}
            def find():
                wins.clear()
                for w in run("xdotool", "search", "--name", "").split():
                    nm = run("xdotool", "getwindowname", w)
                    if nm in ("decebal-dock", "decebal-bar") or (widget and nm.startswith("decebal")):
                        g = run("xdotool", "getwindowgeometry", w).replace("\n", " ")
                        pos = g.split("Position:")[1].split("(")[0].strip().split(","); sz = g.split("Geometry:")[1].strip().split("x")
                        wins[nm] = dict(x=int(pos[0]), y=int(pos[1]), w=int(sz[0]), h=int(sz[1]))
                return wins
            wait(find, "panel window")
            if widget:
                # decwidgets clears the root background when it starts; repaint the backdrop (the wallpaper host owns this on a real desktop)
                time.sleep(1.5); set_root(env["DISPLAY"], wallpaper or r / "backdrop.png"); time.sleep(0.8)
            time.sleep(1.6); find()
            png = OUT / shot; run("import", "-window", "root", str(png))
            im = Image.open(png).convert("RGB")
            if not wallpaper:
                # the flat backdrop must be on screen at the centre (a rare startup race leaves the root black): repaint and recapture before measuring
                for _ in range(4):
                    cx = im.getpixel((im.width // 2, im.height // 2))
                    if max(abs(cx[i] - BACKDROP[mode][i]) for i in range(3)) <= 6: break
                    set_root(env["DISPLAY"], r / "backdrop.png"); time.sleep(1.2); find(); run("import", "-window", "root", str(png)); im = Image.open(png).convert("RGB")
            t = tomllib.loads((ROOT / "themes" / theme / "theme.toml").read_text())
            others = {}
            for w in run("xdotool", "search", "--name", "").split():
                nm = run("xdotool", "getwindowname", w)
                if nm: others[nm] = run("xdotool", "getwindowgeometry", w).replace("\n", " ").split("Geometry:")[1].strip()
            res = dict(theme=theme, mode=mode, scale=scale, geometry=geom, edge=edge or "theme default", windows=wins, other_windows=others, screenshot=shot, wallpaper=bool(wallpaper))
            panel = wins.get("decebal-dock") or wins.get("decebal-bar")
            if "decebal-bar" in wins and "decebal-dock" not in wins and not wallpaper:
                # Dux islands: the window is a transparent canvas; the most common colour that is not the backdrop is the island face
                b = wins["decebal-bar"]; from collections import Counter
                crop = im.crop((b["x"], b["y"] + b["h"] - int(72 * scale), b["x"] + b["w"], b["y"] + b["h"])) if not edge else im.crop((b["x"], b["y"], b["x"] + b["w"], b["y"] + b["h"]))
                c = Counter(v for v in crop.getdata() if max(abs(v[i] - BACKDROP[mode][i]) for i in range(3)) > 6).most_common(1)
                pc = t["colors"][mode]["bar"]; al = t["metrics"]["bar_alpha"]
                want = tuple(round(int(pc[1 + 2 * i:3 + 2 * i], 16) * al + BACKDROP[mode][i] * (1 - al)) for i in range(3))
                if c: res.update(panel_pixel=list(c[0][0]), panel_expected=list(want), panel_delta=max(abs(c[0][0][i] - want[i]) for i in range(3)), panel_sample="island face (most common non-backdrop colour)")
                else: res.update(panel_delta=None, panel_sample="no island pixels found")
            elif panel and not wallpaper:
                pc = t["colors"][mode]["bar"]; al = t["metrics"]["bar_alpha"] if "decebal-bar" in wins and "decebal-dock" not in wins else t["metrics"]["dock_alpha"]
                want = tuple(round(int(pc[1 + 2 * i:3 + 2 * i], 16) * al + BACKDROP[mode][i] * (1 - al)) for i in range(3))
                W, H = im.size
                # sample an empty stretch of the panel: the middle of the long axis
                px = im.getpixel((min(W - 1, panel["x"] + panel["w"] // 2), min(H - 1, panel["y"] + 4 if panel["h"] > 12 else panel["y"])))
                res.update(panel_pixel=list(px), panel_expected=list(want), panel_delta=max(abs(px[i] - want[i]) for i in range(3)))
            tb = t.get("taskbar")
            if tb and "decebal-dock" in wins:   # the generic [taskbar] keys: the strip thickness is the declared one times the scale
                dk = wins["decebal-dock"]; thick = dk["w"] if edge in ("left", "right") else dk["h"]
                res.update(taskbar_thickness=thick, taskbar_thickness_expected=round(tb["thickness"] * scale))
                assert abs(thick - round(tb["thickness"] * scale)) <= 1, f"{shot}: taskbar thickness {thick} != declared {tb['thickness']} x {scale}"
            if not wallpaper:
                assert res.get("panel_delta") is not None and res["panel_delta"] <= 4, f"{shot}: panel colour is not the package's ({res.get('panel_pixel')} vs {res.get('panel_expected')})"
                if edge is None and geom == "1920x1080" and not widget and not extra:   # bottom-edge captures: keep the strip, not a screen of flat backdrop
                    im.crop((0, im.height - int(190 * scale) if "decebal-bar" in wins and "decebal-dock" not in wins else im.height - int(150 * scale), im.width, im.height)).save(png)
                    res["cropped"] = "bottom strip"
            return res
        finally:
            for p in reversed(procs):
                p.terminate()
                try: p.wait(timeout=8)
                except subprocess.TimeoutExpired: p.kill()


def main():
    themes = ["openkylin", "openkylin-islands", "openkylin-light-seeking", "openkylin-heyin", "openkylin-classic"]
    if MATRIX == "cc":
        results = []
        for t in ("openkylin", "openkylin-heyin", "openkylin-classic"):
            for m in ("light", "dark"):
                for sc in (1, 2):
                    name = f"controlcenter-{t}-{m}-{int(sc * 100)}.png"
                    r = case(t, m, sc, "1920x1080", shot=name, extra=["decbar", "--control-center"])
                    cc = r["other_windows"].get("decebal-control-center"); assert cc, f"{name}: the Control Center did not open"
                    im = Image.open(OUT / name).convert("RGB"); W, H = im.size
                    im.crop((W - int(620 * sc), max(0, H - int(780 * sc)), W, H)).save(OUT / name); r["control_center_window"] = cc; results.append(r); print(name, cc)
        (OUT / "native-controlcenter.json").write_text(json.dumps(dict(binaries={b: sha(BIN / b) for b in ("decthemectl", "decbar", "decdock")}, results=results), indent=1))
        return
    if MATRIX == "widget":
        results = []
        for m in ("light", "dark"):
            for sc in (1, 2):
                name = f"widget-ukui-time-24-{m}-{int(sc * 100)}.png"
                r = case("openkylin", m, sc, "1920x1080", shot=name, widget="ukui-time-24")
                im = Image.open(OUT / name).convert("RGB"); W, H = im.size
                box = (W - 980, 96, W - 440, 296)   # desktop widgets are physical pixels: the host does not follow DECEBAL_SCALE; x = -480 is the widget's right-edge offset
                white = sum(1 for v in im.crop(box).getdata() if min(v) >= 245)
                r["widget_text_pixels"] = white; results.append(r); print(name, white)
                assert white > 300, f"{name}: the UKUI time widget did not paint white text in its slot"
                im.crop((W - 1000, 60, W - 420, 330)).save(OUT / name)
        (OUT / "native-widget.json").write_text(json.dumps(dict(binaries={b: sha(BIN / b) for b in ("decthemectl", "decwidgets")}, results=results), indent=1))
        return
    cases = [(t, m, s, "1920x1080", None) for t in themes for m in ("light", "dark") for s in ((1, 1.25, 2) if MATRIX == "full" else (1, 2))]
    if MATRIX == "full":
        FULL, SHORT = "1920x1080", "1280x560"
        classic = ("openkylin", "openkylin-classic")
        cases += [(t, m, s, FULL, e) for t in classic for m in ("light", "dark") for s in (1, 1.25, 2) for e in ("top", "left", "right")]          # every other edge, full size
        cases += [("openkylin", m, s, SHORT, e) for m in ("light", "dark") for s in (1, 1.25, 2) for e in (None, "top", "left", "right")]          # short windows, all four edges
        cases += [("openkylin-islands", m, s, FULL, e) for m in ("light", "dark") for s in (1, 1.25, 2) for e in ("top", "left", "right")]         # island bar on every other edge
        cases += [("openkylin-islands", m, s, SHORT, e) for m in ("light", "dark") for s in (1, 2) for e in (None, "left")]
    results = []
    for t, m, s, g, e in cases:
        name = f"{t}-{m}-{int(s * 100)}-{g}" + (f"-{e}" if e else "") + ".png"
        r = case(t, m, s, g, e, shot=name); results.append(r); print(name, r["windows"], r.get("panel_delta"))
        if WALLS and e is None and g == "1920x1080" and s == 1:
            pick = PICKS[t][m]["image_id"]; f = next(pathlib.Path(WALLS).glob(f"wallhaven-{pick}.*"))
            results.append(case(t, m, s, g, None, wallpaper=f, shot="photo-" + name))
    (OUT / "native-checks.json").write_text(json.dumps(dict(
        binaries={b: sha(BIN / b) for b in ("decthemectl", "decdock", "decbar")}, results=results,
        note="private Xvfb + picom xrender gaussian; no WM; hardware applets (battery/bluetooth/volume) absent by design on the fixture"), indent=1))
    print("wrote", OUT / "native-checks.json")


main()
