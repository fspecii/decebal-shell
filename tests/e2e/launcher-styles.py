#!/usr/bin/env python3
"""Native launcher-style evidence and interaction gate, in a private X11 session; never touches the live desktop.

    LAUNCHER_STYLES_EVIDENCE=/var/tmp/launcher-styles python3 tests/e2e/launcher-styles.py [--quick]

For every style x panel edge (via the --edge/--anchor/--thickness contract) x UI scale (Xft.dpi) x light/dark it
captures the real declaunch window from a private Xvfb, checks the panel strip stays uncovered and the card fits
on screen, and records the binary's sha256 and the source commit. It then drives keyboard (type, arrows, Tab,
Enter, Escape) and pointer (hover, click, wheel, outside click) interactions and checks the fixture apps launched.
LAUNCHER_BIN_DIR selects the binaries (default: $CARGO_TARGET_DIR or target, debug profile).
"""
from pathlib import Path
import hashlib, json, os, shutil, signal, subprocess, sys, tempfile, time
from PIL import Image

repo = Path(__file__).resolve().parents[2]
target = Path(os.environ.get("CARGO_TARGET_DIR", repo / "target"))
bins = Path(os.environ.get("LAUNCHER_BIN_DIR", target / "debug"))
quick = "--quick" in sys.argv
interactions_only = "--interactions-only" in sys.argv
evidence = Path(os.environ.get("LAUNCHER_STYLES_EVIDENCE", "/var/tmp/launcher-styles-evidence"))
evidence.mkdir(parents=True, exist_ok=True)
STYLES = [s for s in ["default", "dux", "windows", "gnome", "kde", "xfce", "blackberry"] if s in os.environ.get("LAUNCHER_STYLES", "default,dux,windows,gnome,kde,xfce,blackberry").split(",")]
EDGES = ["top", "bottom", "left", "right"]
SCALES = [1.0, 1.25, 1.5, 2.0]
THICK = 36  # logical panel thickness used for the captures

# -I omits the script directory; import only this trusted fixture directory.
import sys
sys.path.insert(0, str(Path(__file__).resolve().parent))
from private_x11 import start_private_xvfb
root = Path(tempfile.mkdtemp(prefix="decebal-launcher-styles-", dir=os.environ.get("LAUNCHER_STYLES_WORK", "/var/tmp")))
root.chmod(0o700)
env = dict(os.environ)
for k in list(env):
    if k in ("DISPLAY", "WAYLAND_DISPLAY", "XAUTHORITY", "RDWM_SOCK", "SESSION_MANAGER", "AT_SPI_BUS_ADDRESS", "DBUS_SESSION_BUS_ADDRESS") or k.startswith(("DECEBAL_", "DECSPOT_", "DECSETTINGS_")):
        env.pop(k, None)
for k, v in {"HOME": "home", "XDG_CONFIG_HOME": "cfg", "XDG_CONFIG_DIRS": "nocfg", "XDG_DATA_HOME": "data", "XDG_DATA_DIRS": "data", "XDG_CACHE_HOME": "cache",
             "XDG_STATE_HOME": "state", "XDG_RUNTIME_DIR": "run", "DECEBAL_CONFIG": "shell", "DECEBAL_THEMES": "themes", "DECEBAL_SYSTEM_THEMES": "systemthemes"}.items():
    env[k] = str(root / v)
    Path(env[k]).mkdir(parents=True, exist_ok=True)
os.chmod(env["XDG_RUNTIME_DIR"], 0o700)
env.update(DECEBAL_BACKEND="x11", DECEBAL_SYSTEM_AUTOMATIC="0", DBUS_SYSTEM_BUS_ADDRESS="unix:path=" + str(root / "no-system-bus"), USER="evidence")
calls = root / "calls"
calls.write_text("")
fixture_bin = root / "bin"
fixture_bin.mkdir()
app = fixture_bin / "fixture-app"
app.write_text('#!/bin/sh\nprintf "%s\\n" "$*" >> "$LAUNCHER_FIXTURE_CALLS"\n')
app.chmod(0o755)
# decsession is the power target; a fixture stands in so the evidence never reaches a real session menu.
session = fixture_bin / "decsession"
session.write_text('#!/bin/sh\nprintf "decsession\\n" >> "$LAUNCHER_FIXTURE_CALLS"\n')
session.chmod(0o755)
env["LAUNCHER_FIXTURE_CALLS"] = str(calls)
apps_dir = Path(env["XDG_DATA_HOME"]) / "applications"
apps_dir.mkdir()
cats = ["Network;WebBrowser;", "System;TerminalEmulator;", "Graphics;2DGraphics;", "Development;IDE;", "AudioVideo;Player;", "Office;WordProcessor;", "Utility;TextEditor;", "Game;", "Settings;", "Education;"]
colors = ["#0a84ff", "#30d158", "#ff9f0a", "#bf5af2", "#ff375f", "#64d2ff", "#ffd60a", "#ac8e68", "#8e8e93", "#5e5ce6"]
names = ["Browser", "Terminal", "Paint", "Code Studio", "Video Player", "Writer", "Text Editor", "Chess", "Control Panel", "Atlas", "Calendar", "Mail", "Music", "Photos", "Notes", "Weather", "Maps", "Clock", "Files", "Camera", "Calculator", "Contacts", "Books", "Podcasts", "Recorder", "Scanner", "Fonts", "Disks", "Monitor", "Backup", "Archive", "Viewer", "Sketch", "Studio", "Mixer", "Reader"]
for i, name in enumerate(names):
    icon = root / f"icon{i}.svg"
    c = colors[i % len(colors)]
    icon.write_text(f'<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><rect x="4" y="4" width="56" height="56" rx="14" fill="{c}"/><circle cx="32" cy="32" r="13" fill="#ffffff" opacity="0.85"/></svg>')
    (apps_dir / f"fixture{i:02}.desktop").write_text(f"[Desktop Entry]\nType=Application\nName={name}\nComment=Fixture {i}\nIcon={icon}\nExec={app} {i:02}\nCategories={cats[i % len(cats)]}\nKeywords=fixture;sample{i};\n")
# Spotlight's history gives Recent / Recommended something real to show.
state = Path(env["XDG_STATE_HOME"]) / "decebal"
state.mkdir(parents=True)
now = int(time.time())
(state / "spotlight-history").write_text("".join(f"{3 - k}\t{now - k * 60}\tapp:{n}\n" for k, n in enumerate(["Writer", "Browser", "Paint", "Terminal", "Mail", "Photos"])))
shutil.copy2(bins / "declaunch", fixture_bin / "declaunch")
launcher = fixture_bin / "declaunch"
env["PATH"] = f"{fixture_bin}:{env['PATH']}"


def sha(p):
    return hashlib.sha256(Path(p).read_bytes()).hexdigest()


def git(*a):
    return subprocess.run(["git", "-C", str(repo), *a], capture_output=True, text=True).stdout.strip()


owned = []


def wait(fn, what, seconds=10):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        v = fn()
        if v:
            return v
        time.sleep(0.03)
    raise AssertionError(what)


def run(*args, check=True):
    return subprocess.run(list(map(str, args)), env=env, check=check, capture_output=True, text=True, timeout=20).stdout.strip()


def start_x(size):
    log = (root / f"xvfb-{len(owned)}.log").open("w")
    x, env["DISPLAY"] = start_private_xvfb(env=env, log=log, screen=f"{size[0]}x{size[1]}x24")
    log.close()
    owned.append(x)
    wait(lambda: run("xdotool", "getdisplaygeometry", check=False), "private Xvfb")
    return x


DESK, PANEL, APPLET = (0x5A, 0x6A, 0x7A), (0x1E, 0x22, 0x28), (0xE0, 0x8A, 0x2C)


def desktop(size, edge=None, t=0, at=None):
    """Paints the root window: a plain desktop, and a stand-in panel strip with the applet marked where it was."""
    img = Image.new("RGB", size, DESK)
    if edge:
        w, h = size
        box = {"top": (0, 0, w, t), "bottom": (0, h - t, w, h), "left": (0, 0, t, h), "right": (w - t, 0, w, h)}[edge]
        img.paste(PANEL, box)
        if at:
            r = max(4, t // 3)
            img.paste(APPLET, (at[0] - r, at[1] - r, at[0] + r, at[1] + r))
    f = root / "desktop.png"
    img.save(f)
    # A plain window at +0+0 (no window manager runs), so the X server repaints it when the launcher's shaped
    # full-screen popup uncovers it. (Drawing on the root only lasts until the first expose.)
    global desk
    if desk is not None:
        desk.terminate()
        desk.wait(timeout=5)
    desk = subprocess.Popen(["display", "-immutable", "-borderwidth", "0", "-geometry", "+0+0", "-title", "launcher-evidence-desktop", str(f)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    owned.append(desk)
    wait(lambda: run("xdotool", "search", "--onlyvisible", "--name", "launcher-evidence-desktop", check=False), "desktop stand-in")
    time.sleep(0.2)


desk = None


def stop_x(x):
    global desk
    if desk is not None:
        desk.terminate()
        desk.wait(timeout=5)
        desk = None
    x.terminate()
    x.wait(timeout=10)


def configure(style, mode, theme="macos"):
    shell = Path(env["DECEBAL_CONFIG"]) / "decebal.json"
    shell.write_text(json.dumps({"theme": {"name": theme, "mode": mode}, "launcher": {"style": style}}))


def set_scale(scale):
    subprocess.run(["xrdb", "-merge"], input=f"Xft.dpi: {int(round(96 * scale))}\n", env=env, text=True, check=True)


def capture(out):
    """A screenshot once the picture stopped changing (the first frame of a full-screen style takes a moment)."""
    last = None
    for _ in range(40):
        run("import", "-window", "root", out)
        data = Image.open(out).convert("RGB").tobytes()
        if data == last:
            return
        last = data
        time.sleep(0.15)
    raise AssertionError(f"{out}: the picture never settled")


def window():
    return run("xdotool", "search", "--onlyvisible", "--class", "declaunch", check=False).split()


def open_launcher(args):
    log = (root / "declaunch.log").open("a")
    p = subprocess.Popen([str(launcher), *map(str, args)], env=env, stdout=log, stderr=log, start_new_session=True)
    owned.append(p)
    wait(window, f"declaunch {args} did not map")
    time.sleep(0.25)  # first paint and keyboard grab
    return p


def close(p):
    if p.poll() is None:
        run("xdotool", "key", "Escape", check=False)
        try:
            p.wait(timeout=5)
        except subprocess.TimeoutExpired:
            p.kill()
            p.wait()
    assert p.returncode in (0, None) or p.returncode == -9, p.returncode
    wait(lambda: not window(), "window still mapped")


def anchor_args(edge, size, scale):
    t = round(THICK * scale)
    w, h = size
    x, y = {"top": (w // 5, t // 2), "bottom": (w // 2, h - t // 2), "left": (t // 2, h * 3 // 4), "right": (w - t // 2, h // 3)}[edge]
    return ["toggle", "--edge", edge, "--anchor", f"{x},{y}", "--thickness", t], t, (x, y)


def strip_clear(img, edge, t):
    """The panel strip still shows the root background (the launcher left it alone)."""
    w, h = img.size
    pts = {"top": [(w * k // 10, t // 2) for k in range(1, 10)], "bottom": [(w * k // 10, h - 1 - t // 2) for k in range(1, 10)],
           "left": [(t // 2, h * k // 10) for k in range(1, 10)], "right": [(w - 1 - t // 2, h * k // 10) for k in range(1, 10)]}[edge]
    return all(img.getpixel(p)[:3] in (PANEL, APPLET) for p in pts)


records = {"commit": git("rev-parse", "HEAD"), "dirty": bool(git("status", "--porcelain")), "declaunch_sha256": sha(launcher), "captures": [], "interactions": []}
try:
    # --- the capture matrix
    matrix = []
    for size, scales, modes in ([(1600, 900), SCALES, ["dark", "light"]], [(1024, 600), [1.0, 2.0], ["dark", "light"]]):
        if quick:
            scales, modes = scales[:1] + scales[-1:], modes
        matrix.append((size, scales, modes))
    for size, scales, modes in [] if interactions_only else matrix:
        x = start_x(size)
        for scale in scales:
            set_scale(scale)
            for mode in modes:
                for style in STYLES:
                    configure(style, mode)
                    for edge in EDGES + ["none"]:
                        args, t, at = anchor_args(edge, size, scale) if edge != "none" else (["toggle"], 0, None)
                        desktop(size, edge if edge != "none" else None, t, at)
                        p = open_launcher(args)
                        name = f"{style}-{edge}-{size[0]}x{size[1]}-s{int(scale * 100)}-{mode}.png"
                        out = evidence / "captures" / name
                        out.parent.mkdir(exist_ok=True)
                        capture(out)
                        img = Image.open(out).convert("RGB")
                        rec = {"file": str(out), "style": style, "edge": edge, "screen": size, "scale": scale, "mode": mode, "args": list(map(str, args)), "declaunch_sha256": records["declaunch_sha256"], "commit": records["commit"]}
                        if edge != "none":
                            rec["panel_strip_uncovered"] = strip_clear(img, edge, t)
                            assert rec["panel_strip_uncovered"], f"{name}: the launcher covers the panel strip"
                        geo = run("xdotool", "getwindowgeometry", window()[0], check=False)
                        rec["window"] = geo.replace("\n", " ")
                        records["captures"].append(rec)
                        close(p)
        stop_x(x)

    # --- interactions, at 1366x768, scale 1.25, dark
    x = start_x((1366, 768))
    set_scale(1.25)
    desktop((1366, 768))
    # --output: the one RandR monitor of this Xvfb by name, and an unknown name falling back (Xvfb cannot expose
    # several RandR monitors, so the multi-output choice is covered by backend::x11::monitor_tests).
    configure("kde", "dark")
    monitors = run("xrandr", "--listmonitors")
    name = monitors.splitlines()[1].split()[-1]
    out = {"randr_monitors": monitors}
    for label, args in [("named", ["open", "--output", name]), ("unknown", ["open", "--output", "NO-SUCH-OUTPUT"])]:
        p = open_launcher(args)
        geo = dict(kv.split("=") for kv in run("xdotool", "getwindowgeometry", "--shell", window()[0]).split())
        out[label] = {k: geo[k] for k in ("X", "Y", "WIDTH", "HEIGHT")}
        assert (geo["X"], geo["Y"], geo["WIDTH"], geo["HEIGHT"]) == ("0", "0", "1366", "768"), (label, geo)
        close(p)
    records["output_option"] = out
    for style in STYLES:
        configure(style, "dark")
        result = {"style": style}
        before = calls.read_text().count("\n")
        # type-to-search + Enter launches the best match
        p = open_launcher(["toggle", "--edge", "bottom", "--anchor", "683,746", "--thickness", "44"])
        run("xdotool", "type", "--delay", "20", "atlas")
        time.sleep(0.3)
        (evidence / "interactions").mkdir(exist_ok=True)
        capture(evidence / "interactions" / f"{style}-search.png")
        run("xdotool", "key", "Return")
        assert p.wait(timeout=8) == 0, (style, "enter exit")
        wait(lambda: "09" in calls.read_text().splitlines()[before:], f"{style}: Enter did not launch Atlas")
        result["type_enter_launches"] = True
        # arrows and Tab move the focus (the picture changes), Escape closes with status 0
        p = open_launcher(["open"])
        shots = []
        # Dux keeps its ported keys: Left/Right edit the query and Tab does nothing, so it moves with Down/Up.
        sequence = [[], ["Down"], ["Down"], ["Up"], ["Up"]] if style == "dux" else [[], ["Down"], ["Right"], ["Tab"], ["shift+Tab"]]
        for k, keys in enumerate(sequence):
            for key in keys:
                run("xdotool", "key", key)
            time.sleep(0.25)
            f = evidence / "interactions" / f"{style}-keys-{k}.png"
            capture(f)
            shots.append(Image.open(f).convert("RGB").tobytes())
        result["arrows_tab_change_focus"] = len(set(shots)) >= 3
        assert result["arrows_tab_change_focus"], f"{style}: arrows/Tab did not move the focus"
        run("xdotool", "key", "Escape")
        assert p.wait(timeout=8) == 0
        result["escape_closes"] = True
        # hover changes the highlight, the wheel scrolls/pages, a click outside closes
        desktop((1366, 768), "top", 36, (100, 18))
        p = open_launcher(["open", "--edge", "top", "--anchor", "100,18", "--thickness", "36"])
        geo = dict(kv.split("=") for kv in run("xdotool", "getwindowgeometry", "--shell", window()[0]).split())
        # hover two real app icons (found by their fixture colours in a capture): the highlight must follow
        f0 = evidence / "interactions" / f"{style}-hover-before.png"
        capture(f0)
        img = Image.open(f0).convert("RGB")
        px = img.load()
        spots = []
        for colour in [(0x0A, 0x84, 0xFF), (0x30, 0xD1, 0x58), (0xFF, 0x9F, 0x0A), (0xBF, 0x5A, 0xF2)]:
            hit = next(((x0, y0) for y0 in range(0, img.size[1], 2) for x0 in range(0, img.size[0], 2) if px[x0, y0] == colour and not any(abs(x0 - a) < 40 and abs(y0 - b) < 40 for a, b in spots)), None)
            if hit:
                spots.append(hit)
            if len(spots) == 2:
                break
        assert len(spots) == 2, f"{style}: two app icons on screen"
        frames = []
        for (hx, hy) in spots:
            run("xdotool", "mousemove", hx + 4, hy + 8)
            time.sleep(0.2)
            f = evidence / "interactions" / f"{style}-hover-{len(frames)}.png"
            capture(f)
            frames.append(Image.open(f).convert("RGB").tobytes())
        result["hover_moves_highlight"] = frames[0] != frames[1]
        assert result["hover_moves_highlight"], f"{style}: hovering another app did not move the highlight"
        # the wheel over the list / grid scrolls or pages it when there is more than one screenful
        run("xdotool", "click", "5")
        run("xdotool", "click", "5")
        time.sleep(0.3)
        f = evidence / "interactions" / f"{style}-wheel.png"
        capture(f)
        result["wheel_changes_view"] = Image.open(f).convert("RGB").tobytes() != frames[1]
        # outside every style's card: the panel strip (full-screen styles cover the rest of the screen)
        run("xdotool", "mousemove", 1360, 10)
        run("xdotool", "click", "1")
        status = p.wait(timeout=8)
        assert status == 0, (style, "outside click", status)
        result["outside_click_closes"] = True
        # a pointer click on an app launches it: find the Chess fixture's icon colour in a real capture and click it
        p = open_launcher(["open"])
        before = calls.read_text().count("\n")
        run("xdotool", "type", "--delay", "20", "chess")
        time.sleep(0.35)
        f = evidence / "interactions" / f"{style}-click-target.png"
        capture(f)
        img = Image.open(f).convert("RGB")
        px = img.load()
        hit = next(((x0, y0) for y0 in range(img.size[1]) for x0 in range(img.size[0]) if px[x0, y0] == (0xAC, 0x8E, 0x68)), None)
        assert hit, f"{style}: the Chess result is not on screen"
        run("xdotool", "mousemove", hit[0], hit[1] + 6)
        time.sleep(0.15)
        run("xdotool", "click", "1")
        assert p.wait(timeout=8) == 0, (style, "click exit")
        wait(lambda: "07" in calls.read_text().splitlines()[before:], f"{style}: clicking Chess did not launch it")
        result["click_launches"] = True
        records["interactions"].append(result)
        desktop((1366, 768))
    stop_x(x)

    # --- two RandR monitors side by side: --output, the anchor's monitor, and --output over a conflicting anchor
    x = start_x((2400, 900))
    run("xrandr", "--setmonitor", "LEFT", "1200/300x900/225+0+0", "screen")
    run("xrandr", "--setmonitor", "RIGHT", "1200/300x900/225+1200+0", "none")
    active = run("xrandr", "--listactivemonitors")
    assert "LEFT" in active and "RIGHT" in active, active
    set_scale(1.0)
    desktop((2400, 900))
    LEFT, RIGHT = (0, 0, 1200, 900), (1200, 0, 1200, 900)
    cases = [("output-right", ["open", "--output", "RIGHT"], RIGHT), ("output-left", ["open", "--output", "LEFT"], LEFT),
             ("anchor-on-right", ["open", "--edge", "bottom", "--anchor", "1800,882", "--thickness", "36"], RIGHT),
             ("output-left-anchor-right", ["open", "--output", "LEFT", "--edge", "bottom", "--anchor", "1800,882", "--thickness", "36"], LEFT)]
    multi = {"monitors": active, "cases": []}
    for style in STYLES:
        configure(style, "dark")
        for label, args, mon in cases:
            p = open_launcher(args)
            geo = {k: int(v) for k, v in (kv.split("=") for kv in run("xdotool", "getwindowgeometry", "--shell", window()[0]).split()) if k in ("X", "Y", "WIDTH", "HEIGHT")}
            g = (geo["X"], geo["Y"], geo["WIDTH"], geo["HEIGHT"])
            f = evidence / "multimonitor" / f"{style}-{label}.png"
            f.parent.mkdir(exist_ok=True)
            capture(f)
            if style in ("default", "dux"):
                # the card-sized popup lies inside the chosen monitor (and above the panel strip when anchored)
                inside = mon[0] <= g[0] and g[0] + g[2] <= mon[0] + mon[2] and g[1] >= 0 and g[1] + g[3] <= 900 - (36 if "anchor" in label else 0)
            else:
                # the full-monitor popup is exactly the chosen monitor
                inside = g == mon
            # nothing is drawn on the other monitor
            img = Image.open(f).convert("RGB")
            other = RIGHT if mon == LEFT else LEFT
            untouched = all(img.getpixel((other[0] + other[2] * k // 8, 450 + (k % 3) * 100))[:3] == DESK for k in range(1, 8))
            multi["cases"].append({"style": style, "case": label, "args": args, "window": g, "monitor": mon, "inside": inside, "other_monitor_untouched": untouched, "capture": str(f)})
            assert inside and untouched, (style, label, g, mon, untouched)
            close(p)
    records["multi_monitor"] = multi
    stop_x(x)
    records["passed"] = True
finally:
    for p in reversed(owned):
        if p.poll() is None:
            p.send_signal(signal.SIGTERM)
            try:
                p.wait(timeout=5)
            except subprocess.TimeoutExpired:
                p.kill()
    if (root / "declaunch.log").exists():
        shutil.copy2(root / "declaunch.log", evidence / "declaunch.log")
    (evidence / "manifest.json").write_text(json.dumps(records, indent=1) + "\n")
    print(json.dumps({k: v for k, v in records.items() if k not in ("captures",)}, indent=1))
    print("captures:", len(records["captures"]), "evidence:", evidence)
    shutil.rmtree(root, ignore_errors=True)
