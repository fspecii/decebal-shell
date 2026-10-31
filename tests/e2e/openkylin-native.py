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
BIN, OUT = pathlib.Path(args[0]).resolve(), pathlib.Path(args[1]).resolve(); OUT.mkdir(parents=True, exist_ok=False)
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


def measure_cluster(im, backdrop, bottom_edge=True, scale=1.0):
    """Face runs of a horizontal Dux cluster in a screenshot: x-runs of non-backdrop pixels on the row through the middle of the faces, separated by backdrop
    gaps, plus the vertical extent of the first face. Returns (runs [(x0, x1)], gaps, face_height, distance_from_screen_edge)."""
    W, H = im.size; px = im.load(); far = lambda v: max(abs(v[i] - backdrop[i]) for i in range(3)) > 6
    # the faces stand 12 px above the bottom edge and are 48 px high: probe the row 36 px above the edge (middle of a 48 px face), top edge mirrored
    y = H - round(36 * scale) if bottom_edge else round(36 * scale)
    runs, start = [], None
    for x in range(W):
        if far(px[x, y]) and start is None: start = x
        if not far(px[x, y]) and start is not None:
            runs.append([start, x]); start = None
    if start is not None: runs.append([start, W])
    merged = []
    for r in runs:   # tiny backdrop specks inside a face (antialiased rounded ends) do not split a face: gaps narrower than 3 px merge
        if merged and r[0] - merged[-1][1] < 3: merged[-1][1] = r[1]
        else: merged.append(r)
    gaps = [b[0] - a[1] for a, b in zip(merged, merged[1:])]
    cx = (merged[0][0] + merged[0][1]) // 2
    col = [yy for yy in range(H) if far(px[cx, yy])]
    top, bottom = (min(col), max(col) + 1) if col else (0, 0)
    return merged, gaps, bottom - top, (H - bottom) if bottom_edge else top


def sha(p): return hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()


_spec = importlib.util.spec_from_file_location("openkylin_fixture", pathlib.Path(__file__).with_name("openkylin_fixture.py"))
fixture = importlib.util.module_from_spec(_spec); _spec.loader.exec_module(fixture)
FIRST_DISPLAY, abstract_sockets, free_display, verify_owned = fixture.FIRST_DISPLAY, fixture.abstract_sockets, fixture.free_display, fixture.verify_owned


USED_DISPLAYS = set()


def clock_mask(edge, sc, thick, W=1920, H=1080):
    """The taskbar's clock/tray end of the strip, which changes between two screenshots (the minute ticks): ignored when looking for the opened card, which sits at the Start end."""
    return [{"bottom": (W - 320 * sc, H - thick, W, H), "top": (W - 320 * sc, 0, W, thick), "left": (0, H - 160 * sc, thick, H), "right": (W - thick, H - 160 * sc, W, H)}[edge]]


def diff_bbox(base, shot, threshold=24, ignore=()):
    """Bounding box (x0, y0, x1, y1) of what differs between two screenshots of the same private server (the opened card and its shadow), or None."""
    a, b = Image.open(base).convert("RGB"), Image.open(shot).convert("RGB"); W, H = a.size; pa, pb = a.load(), b.load(); xs, ys = [], []
    for y in range(0, H, 2):
        for x in range(0, W, 2):
            if any(x0 <= x < x1 and y0 <= y < y1 for x0, y0, x1, y1 in ignore): continue
            if max(abs(pa[x, y][i] - pb[x, y][i]) for i in range(3)) > threshold: xs.append(x); ys.append(y)
    return (min(xs), min(ys), max(xs) + 2, max(ys) + 2) if xs else None


def wait(fn, label, n=150):
    for _ in range(n):
        try:
            r = fn()
            if r: return r
        except (OSError, ValueError, subprocess.CalledProcessError): pass
        time.sleep(0.1)
    raise AssertionError(label)


def case(theme, mode, scale, geom, edge=None, wallpaper=None, shot=None, widget=None, extra=None, pre=None, post=None, blur=True):
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
        # programs the start menu's sidebar launches: recorded, never run (the case reads r/calls.log)
        calls = r / "calls.log"; calls.write_text("")
        for app in ("peony", "gio", "xdg-open", "decsession", "decsettings"):
            (stub / app).write_text(f'#!/bin/sh\necho "{app} $*" >> {calls}\nexit 0\n'); (stub / app).chmod(0o755)
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
                                         + ('rules = ( { blur-background = false; }, { match = "_DECEBAL_BLUR@ = 1"; blur-background = true; } );\n' if blur else 'rules = ( { blur-background = false; } );\n'))
            procs.append(subprocess.Popen(["picom", "--config", str(r / "picom.conf")], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)); time.sleep(0.8)
            for b in (("decdock", "decbar") + (("decwidgets",) if widget else ())):
                # DECEBAL_SCALE (every process, `apply` included) sizes the taskbar; the bar also follows DECEBAL_FONT_SCALE.
                # DECEBAL_FONT_SCALE on the taskbar double-scales its text, so only the bar gets it.
                benv = dict(env, **({"DECEBAL_FONT_SCALE": str(scale)} if b == "decbar" else {}))
                procs.append(subprocess.Popen([str(BIN / b)], env=benv, stdout=(r / f"{b}.log").open("w"), stderr=subprocess.STDOUT))
            before = pre(run, r, env) if pre else None   # e.g. a baseline screenshot of the panel alone
            if extra:   # one more shell process (e.g. the Control Center summoned by its key), started after the panel
                time.sleep(1.0); procs.append(subprocess.Popen([str(BIN / extra[0]), *extra[1:]], env=env, stdout=(r / f"{extra[0]}.log").open("w"), stderr=subprocess.STDOUT)); time.sleep(2.5)
            after = post(run, r, env, before) if post else None   # input sent to the opened window (xdotool on the private server) and what the stubs recorded
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
            res = dict(theme=theme, mode=mode, scale=scale, geometry=geom, edge=edge or "theme default", windows=wins, other_windows=others, screenshot=shot, wallpaper=bool(wallpaper), post=after)
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
                if t["bar"].get("cluster") and edge in (None, "top") and "decebal-bar" in wins:
                    runs, gaps, fh, off = measure_cluster(im, BACKDROP[mode], bottom_edge=edge is None, scale=scale)
                    W = im.width; mid = (runs[0][0] + runs[-1][1]) / 2 if runs else None
                    res.update(cluster_faces=len(runs), cluster_runs=runs, cluster_gaps=gaps, cluster_face_height=fh, cluster_edge_distance=off, cluster_centre=mid)
                    # the decbar surface is drawn at the display scale, so every logical number is multiplied by it (anti-aliased edges cost up to 2 px)
                    gg = t["bar"]["group_gap"] * scale; fhx = (t["bar"]["height"] - 10) * scale; offx = (t["bar"]["margin"] + 5) * scale
                    assert len(runs) >= 2 and all(abs(g - gg) <= 2 for g in gaps), f"{shot}: faces {runs} gaps {gaps} != {gg}"
                    assert abs(mid - W / 2) <= 2, f"{shot}: the cluster is not centred ({mid} vs {W / 2})"
                    assert abs(fh - fhx) <= 2 and abs(off - offx) <= 2, f"{shot}: face height {fh} (want {fhx}) / edge distance {off} (want {offx})"
                    res.update(cluster_expected=dict(gap=gg, face_height=fhx, edge_distance=offx))
            elif panel and not wallpaper:
                pc = t["colors"][mode]["bar"]; al = t["metrics"]["bar_alpha"] if "decebal-bar" in wins and "decebal-dock" not in wins else t["metrics"]["dock_alpha"]
                want = tuple(round(int(pc[1 + 2 * i:3 + 2 * i], 16) * al + BACKDROP[mode][i] * (1 - al)) for i in range(3))
                W, H = im.size
                # sample an empty stretch of the panel: the middle of the long axis
                late = bool(extra or post)   # a card is open at the Start end: sample the far end of the strip, away from it
                px = im.getpixel((min(W - 1, panel["x"] + (panel["w"] * 17 // 20 if late and panel["w"] > panel["h"] else panel["w"] // 2)), min(H - 1, panel["y"] + (panel["h"] * 3 // 4 if late and panel["h"] > panel["w"] else 4 if panel["h"] > 12 else 0))))
                res.update(panel_pixel=list(px), panel_expected=list(want), panel_delta=max(abs(px[i] - want[i]) for i in range(3)))
            tb = t.get("taskbar")
            if tb and "decebal-dock" in wins:   # the generic [taskbar] keys: the strip thickness is the declared one times the scale
                dk = wins["decebal-dock"]; thick = dk["w"] if edge in ("left", "right") else dk["h"]
                res.update(taskbar_thickness=thick, taskbar_thickness_expected=round(tb["thickness"] * scale))
                assert abs(thick - round(tb["thickness"] * scale)) <= 1, f"{shot}: taskbar thickness {thick} != declared {tb['thickness']} x {scale}"
            if not wallpaper:
                assert res.get("panel_delta") is not None and res["panel_delta"] <= 4, f"{shot}: panel colour is not the package's ({res.get('panel_pixel')} vs {res.get('panel_expected')})"
                if edge is None and geom == "1920x1080" and not widget and not extra and not pre and not post:   # bottom-edge captures: keep the strip, not a screen of flat backdrop
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
    if MATRIX in ("launcher", "launcher-edges"):
        # the UKUI start menu (`launcher.style = "ukui"`, 766 x 688 at 1x) opened from the first taskbar cell on the private fixture: the card on the bottom edge (bbox of what
        # differs from the flat backdrop), then the card on every edge against a baseline of the panel alone, then real input
        results = []
        for t in ("openkylin", "openkylin-light-seeking", "openkylin-heyin", "openkylin-classic") if MATRIX == "launcher" else ():
            for m in ("light", "dark"):
                for sc in (1, 1.25, 2):
                    name = f"startmenu-{t}-{m}-{int(sc * 100)}.png"; thick = round(48 * sc)
                    r = case(t, m, sc, "1920x1080", shot=name, extra=["declaunch", "open", "--edge", "bottom", "--anchor", f"{round(24 * sc)},{1080 - thick // 2}", "--thickness", str(thick)])
                    win = r["other_windows"].get("UKUI Menu"); assert win, f"{name}: the launcher did not open ({r['other_windows']})"
                    im = Image.open(OUT / name).convert("RGB"); W, H = im.size; px = im.load(); bd = BACKDROP[m]
                    xs, ys = [], []
                    for y in range(0, H - thick, 2):
                        for x in range(0, W, 2):
                            if max(abs(px[x, y][i] - bd[i]) for i in range(3)) > 6: xs.append(x); ys.append(y)
                    assert xs, f"{name}: no card pixels"
                    x0, x1, y0, y1 = min(xs), max(xs) + 2, min(ys), max(ys) + 2; w, h = x1 - x0, y1 - y0
                    want = (766 * sc, min(688 * sc, 1080 - thick - 8 * sc))
                    # the bounding box includes the card's soft shadow (and the 8 px gap above the panel): at least the card, at most the card plus 2 x 18 px of shadow
                    assert 0 <= w - want[0] <= 36 * sc and 0 <= h - want[1] <= 40 * sc, f"{name}: bbox {w}x{h} at {x0},{y0}, card {want}"
                    r["launcher_window"] = win; r["card"] = dict(x=x0, y=y0, w=w, h=h, card_w=want[0], card_h=want[1], note="bbox of non-backdrop pixels = card + shadow"); results.append(r); print(name, r["card"])
                    im.crop((0, max(0, H - int(740 * sc)), min(W, int(820 * sc)), H)).save(OUT / name)
        # every edge, classic packages: the card never covers the panel strip (baseline diff), at 100 % and 200 %, light and dark
        for t in ("openkylin", "openkylin-classic"):
            for m in ("light", "dark"):
                for sc in (1, 2):
                    thick = round(48 * sc)
                    for edge in ("bottom", "top", "left", "right"):
                        name = f"startmenu-edge-{edge}-{t}-{m}-{int(sc * 100)}.png"
                        point = {"bottom": (round(24 * sc), 1080 - thick // 2), "top": (round(24 * sc), thick // 2), "left": (thick // 2, round(24 * sc)), "right": (1920 - thick // 2, round(24 * sc))}[edge]
                        def baseline(run, r, env, name=name):
                            run("import", "-window", "root", str(OUT / ("base-" + name))); return None
                        r = case(t, m, sc, "1920x1080", edge=None if edge == "bottom" else edge, shot=name, pre=baseline, extra=["declaunch", "open", "--edge", edge, "--anchor", f"{point[0]},{point[1]}", "--thickness", str(thick)])
                        assert r["other_windows"].get("UKUI Menu"), f"{name}: the launcher did not open ({r['other_windows']})"
                        box = diff_bbox(OUT / ("base-" + name), OUT / name, ignore=clock_mask(edge, sc, thick)); assert box, f"{name}: the card did not change the picture"
                        W, H = 1920, 1080
                        x0, y0, x1, y1 = box
                        # the strip along the edge stays clear (a few px of soft shadow may reach it); the card is never larger than 766 x 688 at this scale
                        slack = 12 * sc
                        ok = {"bottom": y1 <= H - thick + slack, "top": y0 >= thick - slack, "left": x0 >= thick - slack, "right": x1 <= W - thick + slack}[edge]
                        assert ok and (x1 - x0) <= 766 * sc + 40 * sc and (y1 - y0) <= 688 * sc + 40 * sc, f"{name}: the card changes {x0},{y0}..{x1},{y1}, covering the {edge} panel strip of {thick}"
                        r["card_diff_bbox"] = dict(x0=x0, y0=y0, x1=x1, y1=y1, strip=thick, edge=edge); results.append(r); print(name, r["card_diff_bbox"])
        (OUT / "native-launcher.json").write_text(json.dumps(dict(binaries={b: sha(BIN / b) for b in ("decthemectl", "decdock", "decbar", "declaunch")}, results=results), indent=1))
        return
    if MATRIX == "start-click":
        # the REAL taskbar Start cell is clicked (xdotool on the private server): the dock spawns declaunch with its own edge and anchor (needs root's fbddaf8); the card must open at the Start
        # corner, clear of the panel strip, on every edge
        results = []
        for t in ("openkylin", "openkylin-classic"):
            for m in ("light", "dark"):
                for sc in (1, 2):
                    thick = round(48 * sc)
                    for edge in ("bottom", "top", "left", "right"):
                        name = f"start-click-{edge}-{t}-{m}-{int(sc * 100)}.png"
                        def baseline(run, r, env, name=name):
                            run("import", "-window", "root", str(OUT / ("base-" + name))); return None
                        def click_start(run, r, env, before, edge=edge, sc=sc):
                            win = next((w for w in run("xdotool", "search", "--name", "decebal-dock").split()), None); assert win, "no dock window"
                            g = run("xdotool", "getwindowgeometry", win).replace("\n", " ")
                            x, y = (int(v) for v in g.split("Position:")[1].split("(")[0].strip().split(",")); w, h = (int(v) for v in g.split("Geometry:")[1].strip().split("x"))
                            # the Start cell is the first 48 px cell along the strip, which is left/top aligned
                            cx, cy = (x + w // 2, y + round(24 * sc)) if edge in ("left", "right") else (x + round(24 * sc), y + h // 2)
                            run("xdotool", "mousemove", str(cx), str(cy)); time.sleep(0.3); run("xdotool", "click", "1")
                            opened = False
                            for _ in range(80):
                                time.sleep(0.1)
                                if any(run("xdotool", "getwindowname", w2) == "UKUI Menu" for w2 in run("xdotool", "search", "--onlyvisible", "--name", "UKUI Menu").split()): opened = True; time.sleep(1.5); break
                            return dict(click=(cx, cy), dock=(x, y, w, h), opened=opened, calls=(r / "calls.log").read_text().strip().splitlines())
                        r = case(t, m, sc, "1920x1080", edge=None if edge == "bottom" else edge, shot=name, pre=baseline, post=click_start)
                        got = r["post"]; assert got["opened"], f"{name}: clicking Start did not open the launcher ({got})"
                        box = diff_bbox(OUT / ("base-" + name), OUT / name, ignore=clock_mask(edge, sc, thick)); assert box, f"{name}: no card"
                        x0, y0, x1, y1 = box; slack = 12 * sc
                        clear = {"bottom": y1 <= 1080 - thick + slack, "top": y0 >= thick - slack, "left": x0 >= thick - slack, "right": x1 <= 1920 - thick + slack}[edge]
                        # opened from the Start corner: the card hugs the panel's start end (within its 8 px margin plus shadow), not the middle of the screen
                        near = {"bottom": x0 <= 24 * sc, "top": x0 <= 24 * sc, "left": y0 <= 24 * sc + 8 * sc, "right": y0 <= 24 * sc + 8 * sc}[edge]
                        assert clear and near, f"{name}: card {box} for a {edge} strip of {thick} (clear {clear}, near Start {near})"
                        r["card_diff_bbox"] = dict(x0=x0, y0=y0, x1=x1, y1=y1, strip=thick, edge=edge); results.append(r); print(name, got["click"], r["card_diff_bbox"])
        (OUT / "native-start-click.json").write_text(json.dumps(dict(binaries={b: sha(BIN / b) for b in ("decthemectl", "decdock", "decbar", "declaunch")}, results=results), indent=1))
        return
    if MATRIX == "launcher-expand":
        # the expand button (sidebar, top) turns the card into the full-screen menu; the restore button (top right) brings the start menu back; the typed search survives
        results = []
        for m, sc in (("light", 1), ("dark", 2), ("light", 1.25)):
            thick = round(48 * sc); H = 1080
            card_bottom = H - thick - 8 * sc; card_h = min(688 * sc, card_bottom); right = 8 * sc + 766 * sc; b = min(36 * sc, 56 * sc)
            expand = (right - 28 * sc, card_bottom - card_h + 12 * sc + b / 2)
            top = max(round(48 * ((H - thick) / sc) / 1080), 8) * sc
            restore = (1920 - 40 * sc - 18 * sc, top + 18 * sc)
            for what in ("expanded", "restored"):
                name = f"startmenu-{what}-{m}-{int(sc * 100)}.png"
                def baseline(run, r, env, name=name):
                    run("import", "-window", "root", str(OUT / ("base-" + name))); return None
                def act(run, r, env, before, what=what, expand=expand, restore=restore, card_bottom=card_bottom, card_h=card_h, sc=sc):
                    run("xdotool", "mousemove", str(int(8 * sc + 160 * sc)), str(int(card_bottom - card_h + 32 * sc))); run("xdotool", "click", "1"); run("xdotool", "type", "--delay", "80", "te"); time.sleep(0.5)
                    run("xdotool", "mousemove", str(int(expand[0])), str(int(expand[1]))); time.sleep(0.3); run("xdotool", "click", "1"); time.sleep(1.2)
                    if what == "restored":
                        run("xdotool", "mousemove", str(int(restore[0])), str(int(restore[1]))); time.sleep(0.3); run("xdotool", "click", "1"); time.sleep(1.2)
                    return dict(expand=expand, restore=restore)
                r = case("openkylin", m, sc, "1920x1080", shot=name, pre=baseline, post=act, extra=["declaunch", "open", "--edge", "bottom", "--anchor", f"{round(24 * sc)},{1080 - thick // 2}", "--thickness", str(thick)])
                box = diff_bbox(OUT / ("base-" + name), OUT / name, ignore=clock_mask("bottom", sc, thick)); assert box, f"{name}: nothing is shown"
                x0, y0, x1, y1 = box; w, h = x1 - x0, y1 - y0
                if what == "expanded":
                    assert x0 <= 4 and x1 >= 1916 and y0 <= 4 and abs(y1 - (H - thick)) <= 6 * sc, f"{name}: the full-screen menu covers {box}, not the whole free area (strip {thick})"
                else:
                    assert w <= 766 * sc + 40 * sc and h <= 688 * sc + 40 * sc, f"{name}: restore did not bring the start menu back ({box})"
                r["bbox"] = dict(x0=x0, y0=y0, x1=x1, y1=y1); results.append(r); print(name, r["bbox"])
        (OUT / "native-launcher-expand.json").write_text(json.dumps(dict(binaries={b: sha(BIN / b) for b in ("decthemectl", "decdock", "decbar", "declaunch")}, results=results), indent=1))
        return
    if MATRIX == "material":
        # The composited material of the start menu over a HIGH-CONTRAST SYNTHETIC backdrop (4 px black / 4 px white columns): not a UKUI screenshot and not original-application
        # proof, a measurement of what this implementation composites. With compositor blur the empty card area must be window colour x panel_alpha + the backdrop mean, and smooth
        # (the stripes blurred away); without blur the stripes show through at (1 - alpha); in both cases the text stays opaque ink.
        results = []
        wall = OUT / "stripes.png"
        row = b"".join(b"\xff\xff\xff" if (x // 4) % 2 else b"\x00\x00\x00" for x in range(1920)); Image.frombytes("RGB", (1920, 1080), row * 1080).save(wall)   # 4 px black / 4 px white columns, exactly the screen size
        import statistics
        for t in ("openkylin", "openkylin-heyin", "openkylin-classic"):
            for m in ("light", "dark"):
                th = tomllib.loads((ROOT / "themes" / t / "theme.toml").read_text()); a = th["metrics"]["panel_alpha"]; win = th["colors"][m]["background"]; ink = th["colors"][m]["text"]
                win = tuple(int(win[1 + 2 * i:3 + 2 * i], 16) for i in range(3)); ink = tuple(int(ink[1 + 2 * i:3 + 2 * i], 16) for i in range(3))
                for blur in (True, False):
                    sc = 1; thick = 48; name = f"material-{t}-{m}-{'blur' if blur else 'noblur'}.png"
                    r = case(t, m, sc, "1920x1080", shot=name, wallpaper=wall, blur=blur, extra=["declaunch", "open", "--edge", "bottom", "--anchor", f"24,{1080 - thick // 2}", "--thickness", str(thick)])
                    im = Image.open(OUT / name).convert("RGB"); card_x, card_y = 8, 1080 - thick - 8 - 688
                    def patch(x0, y0, w=80, h=80): return [im.getpixel((card_x + x0 + i, card_y + y0 + j)) for i in range(w) for j in range(h)]
                    px = patch(520, 420)   # the empty widget page below the favourites
                    chan = lambda c: [p[c] for p in px]
                    mean = [statistics.fmean(chan(c)) for c in range(3)]; sd = max(statistics.pstdev(chan(c)) for c in range(3))
                    want = [a * win[c] + (1 - a) * 127.5 for c in range(3)]
                    dev = max(abs(mean[c] - want[c]) for c in range(3))
                    rows = patch(60, 160, 200, 40)   # the first app row: text pixels must reach the ink colour, whatever is behind the translucent card
                    core = min(rows, key=lambda p: sum(abs(p[c] - ink[c]) for c in range(3))); ink_dev = max(abs(core[c] - ink[c]) for c in range(3))
                    ok_blur = dev <= 8 and sd <= 14 if blur else dev <= 8 and sd >= 25
                    r["material"] = dict(panel_alpha=a, window=win, expected_mean=[round(v, 1) for v in want], mean=[round(v, 1) for v in mean], max_mean_dev=round(dev, 1), stripe_stddev=round(sd, 1), blur=blur, text_core_pixel=core, ink=ink, text_ink_dev=ink_dev, backdrop="synthetic 4 px black/white columns")
                    assert ok_blur, f"{name}: material {r['material']}"
                    assert ink_dev <= 24, f"{name}: the text is not opaque ink ({core} vs {ink})"
                    results.append(r); print(name, r["material"])
        (OUT / "native-material.json").write_text(json.dumps(dict(note="synthetic stripe backdrop; measures this implementation's composited material, not an original UKUI application", binaries={b: sha(BIN / b) for b in ("decthemectl", "decdock", "decbar", "declaunch")}, results=results), indent=1))
        return
    if MATRIX == "launcher-input":
        # real input on the start menu (private Xvfb, xdotool): typing searches, every sidebar button runs its program (recorded stubs: nothing is launched), Escape closes
        results = []
        def buttons(sc):
            thick = round(48 * sc); bottom = 1080 - thick - 8 * sc; h = min(688 * sc, bottom); right = 8 * sc + 766 * sc; b = min(36 * sc, 56 * sc, (h - 24 * sc - 12 * sc) / 4)
            cx = right - 28 * sc; ys = [bottom - 12 * sc - b / 2 - i * (b + 4 * sc) for i in range(4)]
            return {"power": (cx, ys[0]), "settings": (cx, ys[1]), "computer": (cx, ys[2]), "account": (cx, ys[3])}, (8 * sc + 160 * sc, bottom - h + 32 * sc)
        want = {"account": "decsettings users", "computer": "peony computer:///", "settings": "decsettings", "power": "decsession"}
        for m, sc in (("light", 1), ("dark", 2)):
            for what in ("search", "account", "computer", "settings", "power", "keyboard", "escape"):
                name = f"startmenu-input-{what}-{m}-{int(sc * 100)}.png"; thick = round(48 * sc)
                def act(run, r, env, before, what=what, sc=sc):
                    btn, search = buttons(sc); out = {}
                    if what == "search":
                        run("xdotool", "mousemove", str(int(search[0])), str(int(search[1]))); run("xdotool", "click", "1"); run("xdotool", "type", "--delay", "80", "fire"); time.sleep(0.8)
                    elif what == "keyboard":
                        # Tab walks list -> favourites -> tabs -> the sidebar; Down steps expand, account, computer, settings; Return activates the focused control
                        for k in ("Tab", "Tab", "Tab", "Down", "Down", "Down", "Return"):
                            run("xdotool", "key", k); time.sleep(0.25)
                        for _ in range(60):
                            time.sleep(0.1)
                            if (r / "calls.log").read_text().strip(): break
                    elif what == "escape":
                        run("xdotool", "key", "Escape")
                        for _ in range(60):
                            time.sleep(0.1)
                            if not any(run("xdotool", "getwindowname", w) == "UKUI Menu" for w in run("xdotool", "search", "--onlyvisible", "--name", "UKUI Menu").split()): break
                    else:
                        x, y = btn[what]; run("xdotool", "mousemove", str(int(x)), str(int(y))); time.sleep(0.2); run("xdotool", "click", "1")
                        for _ in range(60):   # the launcher spawns the program asynchronously; the recording stub appends one line
                            time.sleep(0.1)
                            if (r / "calls.log").read_text().strip(): break
                    out["calls"] = (r / "calls.log").read_text().strip().splitlines()
                    out["launcher_open"] = any(run("xdotool", "getwindowname", w) == "UKUI Menu" for w in run("xdotool", "search", "--onlyvisible", "--name", "UKUI Menu").split())
                    return out
                r = case("openkylin", m, sc, "1920x1080", shot=name, post=act, extra=["declaunch", "open", "--edge", "bottom", "--anchor", f"{round(24 * sc)},{1080 - thick // 2}", "--thickness", str(thick)])
                got = r["post"]
                if what in want:
                    assert any(c == want[what] or c.startswith(want[what] + " ") and want[what].count(" ") == c.count(" ") for c in got["calls"]), f"{name}: {what} ran {got['calls']}, want {want[what]}"
                    assert not got["launcher_open"], f"{name}: the launcher stays open after {what}"
                elif what == "keyboard":
                    assert got["calls"] == ["decsettings"] and not got["launcher_open"], f"{name}: the keyboard walk ended on {got}, want the Settings button"
                elif what == "escape":
                    assert not got["launcher_open"], f"{name}: Escape did not close the launcher"
                else:
                    assert got["launcher_open"] and not got["calls"], f"{name}: typing must search, not launch ({got})"
                results.append(r); print(name, got)
        (OUT / "native-launcher-input.json").write_text(json.dumps(dict(binaries={b: sha(BIN / b) for b in ("decthemectl", "decdock", "decbar", "declaunch")}, results=results), indent=1))
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
        cases += [("openkylin-islands", m, s, SHORT, e) for m in ("light", "dark") for s in (1, 1.25, 2) for e in (None, "top", "left", "right")]
    only = os.environ.get("OPENKYLIN_THEME_FILTER", "")
    if only:
        cases = [case for case in cases if case[0] == only]
        if not cases:
            raise ValueError(f"no native cases for theme {only!r}")
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
