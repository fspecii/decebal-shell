#!/usr/bin/env python3
"""Real native captures of the Plasma Breeze taskbar on every edge, at 100/125/200 % and in both colour modes, on a private Xvfb.
Usage: python3 tests/e2e/plasma-panel-native.py BIN_DIR THEMES_DIR EVIDENCE_DIR [--scales 1,1.25,2] [--edges bottom,top,left,right] [--modes dark,light]

For each combination it applies the package into a private config with `decthemectl theme apply --no-rdwm` (the user's edge is written to
`dock-settings.json` first, which is how Settings -> Panels stores a choice), lets the real `decdock` paint its taskbar over a plain backdrop
window under a private XRender compositor, and measures the root window:
* thickness, floating gap, ends and length of the strip in physical pixels against the declared `[taskbar]` numbers x the scale;
* the strut the dock publishes (`_NET_WM_STRUT_PARTIAL`) against thickness + gap;
* upright content in the right order along the strip: Start glyph first, the clock text, the show-desktop icon cell last;
* a pointer hover on the Start cell tints exactly that cell (the native hit area), and a point in the floating gap does not.
Private HOME/XDG/bus, no window manager, no live display. The display is an explicitly reserved HIGH number (>= 700) checked in both socket
namespaces before any client or input (tests/e2e/gnome_xvfb.py); only the processes of this run are stopped.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

from PIL import Image, ImageChops, ImageDraw

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gnome_xvfb  # noqa: E402

BACKDROP = (107, 111, 133)
W, H = 1920, 1080
# the declared [taskbar] numbers of themes/plasma-breeze (px at scale 1)
THICKNESS, GAP, CELL = 44, 8, 50


def wait(predicate, label, tries=300, delay=0.1):
    for _ in range(tries):
        try:
            value = predicate()
            if value:
                return value
        except (OSError, ValueError, subprocess.CalledProcessError):
            pass
        time.sleep(delay)
    raise AssertionError(label)


def sweep(private):
    """Terminate every process started with this run's private runtime directory (`theme apply` spawns decdock detached)."""
    needle = f"XDG_RUNTIME_DIR={private}/run".encode()
    for proc in Path("/proc").iterdir():
        if proc.name.isdigit() and int(proc.name) != os.getpid():
            try:
                if needle in (proc / "environ").read_bytes().split(b"\0"):
                    os.kill(int(proc.name), 15)
            except (OSError, ValueError):
                pass


def off_backdrop(image, tol=6):
    """Mask of pixels that differ from the plain backdrop."""
    solid = Image.new("RGB", image.size, BACKDROP)
    mask = ImageChops.difference(image, solid).convert("L").point(lambda v: 255 if v > tol else 0)
    # ImageMagick's `display -backdrop` window has a 2 px grey frame of its own: it is not the panel
    ImageDraw.Draw(mask).rectangle((0, 0, image.width - 1, image.height - 1), outline=0, width=2)
    return mask


def expected(edge, s):
    t, g = round(THICKNESS * s), round(GAP * s)
    if edge in ("bottom", "top"):
        x0, x1 = g, W - g
        y0, y1 = (H - g - t, H - g) if edge == "bottom" else (g, g + t)
    else:
        y0, y1 = g, H - g
        x0, x1 = (W - g - t, W - g) if edge == "right" else (g, g + t)
    return t, g, (x0, y0, x1, y1)


def run_one(bin_dir, themes, evidence, mode, edge, scale, report):
    t, g, box = expected(edge, scale)
    with tempfile.TemporaryDirectory(prefix="plasma-panel-native-") as private:
        root = Path(private)
        for sub in ["home", "config", "cache", "state", "data", "run", "bin", "empty-themes"]:
            (root / sub).mkdir(mode=0o700)
        for app in ["curl", "nmcli", "pactl", "wpctl", "playerctl", "bluetoothctl", "brightnessctl", "rdwmctl"]:
            stub = root / "bin" / app
            stub.write_text("#!/bin/sh\nexit 1\n")
            stub.chmod(0o755)
        display = gnome_xvfb.reserve_display()
        debug = root / "layout.txt"
        env = dict(
            os.environ,
            HOME=str(root / "home"), XDG_CONFIG_HOME=str(root / "config"), XDG_CACHE_HOME=str(root / "cache"),
            XDG_STATE_HOME=str(root / "state"), XDG_DATA_HOME=str(root / "data"), XDG_RUNTIME_DIR=str(root / "run"),
            DECEBAL_CONFIG=str(root / "config"), DECEBAL_THEMES=str(themes), DECEBAL_SYSTEM_THEMES=str(root / "empty-themes"),
            DECEBAL_BACKEND="x11", DECEBAL_SYSTEM_AUTOMATIC="0", DECEBAL_SCALE=str(scale), DECDOCK_DEBUG_LAYOUT=str(debug), DISPLAY=display,
            DBUS_SESSION_BUS_ADDRESS=f"unix:path={private}/no-bus", DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={private}/no-system",
            RDWMCTL=str(root / "bin/rdwmctl"), PATH=f'{root / "bin"}:{os.environ["PATH"]}',
        )
        for key in ["WAYLAND_DISPLAY", "RDWM_SOCK", "PULSE_SERVER", "XAUTHORITY"]:
            env.pop(key, None)
        tag = f"plasma-breeze-{mode}-{edge}-{int(scale * 100)}"
        log = (evidence / f"{tag}.log").open("w")
        server = subprocess.Popen(["Xvfb", display, "-screen", "0", f"{W}x{H}x24", "-nolisten", "tcp"], env=env, stdout=log, stderr=log)
        children = []
        try:
            def run(*args, timeout=30, check=True):
                return subprocess.run(args, env=env, text=True, capture_output=True, check=check, timeout=timeout).stdout.strip()

            gnome_xvfb.verify_owned(server, display)  # before any client connects or input is sent
            wait(lambda: server.poll() is None and run("xdotool", "getdisplaygeometry"), "private Xvfb ready")
            backdrop = root / "backdrop.png"
            Image.new("RGB", (W, H), BACKDROP).save(backdrop)
            children.append(subprocess.Popen(["display", "-backdrop", "-title", "plasma-port-backdrop", str(backdrop)], env=env, stdout=log, stderr=log))
            time.sleep(0.7)
            (root / "config").mkdir(exist_ok=True)
            # the user's edge exactly as Settings -> Panels stores it; a theme apply never touches it
            (root / "config/dock-settings.json").write_text(json.dumps({"edge": edge}))
            run(str(bin_dir / "decthemectl"), "theme", "apply", "plasma-breeze", "--no-rdwm")
            cfg_path = root / "config/decebal.json"
            cfg = json.loads(cfg_path.read_text())
            cfg["theme"]["mode"] = mode
            cfg_path.write_text(json.dumps(cfg))
            (root / "picom.conf").write_text('backend = "xrender";\nvsync = false;\nuse-damage = false;\nshadow = false;\nfading = false;\n')
            children.append(subprocess.Popen(["picom", "--config", str(root / "picom.conf")], env=env, stdout=log, stderr=log))
            time.sleep(1.0)
            shot = evidence / f"{tag}.png"

            def painted():
                run("import", "-window", "root", str(shot))
                return off_backdrop(Image.open(shot).convert("RGB")).getbbox() is not None

            wait(painted, "the taskbar painted", tries=150, delay=0.2)
            time.sleep(1.0)
            run("import", "-window", "root", str(shot))
            image = Image.open(shot).convert("RGB")
            bbox = off_backdrop(image).getbbox()
            x0, y0, x1, y1 = box
            tol = 2
            entry = {"mode": mode, "edge": edge, "scale": scale, "capture": shot.name, "sha256": hashlib.sha256(shot.read_bytes()).hexdigest(), "measured_box": bbox, "expected_box": box}
            assert bbox is not None, f"{tag}: nothing painted"
            assert all(abs(a - b) <= tol for a, b in zip(bbox, box)), f"{tag}: strip {bbox} but expected {box} (thickness {t}, floating gap {g}, both ends {g})"
            # the strut published to the window manager covers the strip and its gap
            win = wait(lambda: run("xdotool", "search", "--name", "decebal-dock", check=False).splitlines()[0], f"{tag}: dock window")
            strut = run("xprop", "-id", win, "_NET_WM_STRUT_PARTIAL", check=False)
            nums = [int(v) for v in strut.split("=")[-1].replace(" ", "").split(",") if v.lstrip("-").isdigit()]
            side = {"left": 0, "right": 1, "top": 2, "bottom": 3}[edge]
            entry["strut"] = nums[:4] if nums else None
            assert nums and nums[side] in (t + g, round((THICKNESS + GAP) * scale)), f"{tag}: strut {nums}"
            # content in order along the strip: Start first, clock text, show-desktop icon last
            horizontal = edge in ("bottom", "top")
            strip = image.crop(box)
            cell_px = round(CELL * scale)
            # along the strip: the first cell is Start, the last the show-desktop icon cell (cells are CELL px wide at scale 1)
            if horizontal:
                start_cell, end_cell = strip.crop((0, 0, cell_px, t)), strip.crop((strip.width - cell_px, 0, strip.width, t))
            else:
                start_cell, end_cell = strip.crop((0, 0, t, cell_px)), strip.crop((0, strip.height - cell_px, t, strip.height))
            for label, cell in (("Start", start_cell), ("show desktop", end_cell)):
                ink = off_panel_ink(cell)
                assert ink > 20 * scale, f"{tag}: the {label} cell is empty ({ink} px)"
                entry[f"{label.replace(' ', '_')}_ink_px"] = ink
            # native hit area: hovering the Start cell tints that cell and nothing outside it
            surface = surface_geometry(run, win)
            centre = start_centre(debug, edge, surface, scale)
            if centre:
                before = image
                run("xdotool", "mousemove", str(centre[0]), str(centre[1]))
                time.sleep(0.8)
                hover = evidence / f"{tag}-hover.png"
                run("import", "-window", "root", str(hover))
                after = Image.open(hover).convert("RGB")
                diff = ImageChops.difference(before, after).convert("L").point(lambda v: 255 if v > 3 else 0).getbbox()
                entry["hover_changed_box"] = diff
                assert diff is not None, f"{tag}: hovering Start changed nothing"
                hx0, hy0, hx1, hy1 = diff
                half = round(CELL * scale) // 2 + 3
                assert hx0 >= centre[0] - half - t and hx1 <= centre[0] + half + t and hy0 >= box[1] - 1 and hy1 <= box[3] + 1, f"{tag}: hover leaked outside the strip: {diff}"
                outside = (centre[0], max(0, box[1] - max(2, g // 2))) if horizontal else (max(0, box[0] - max(2, g // 2)), centre[1])
                if g >= 4:
                    run("xdotool", "mousemove", str(outside[0]), str(outside[1]))
                    time.sleep(0.5)
                    gap_shot = evidence / f"{tag}-gap.png"
                    run("import", "-window", "root", str(gap_shot))
                    assert not ImageChops.difference(before, Image.open(gap_shot).convert("RGB")).convert("L").point(lambda v: 255 if v > 3 else 0).getbbox(), f"{tag}: the floating gap reacts to the pointer"
            report.append(entry)
            print(f"ok {tag}: strip {bbox} strut {nums[:4]}")
        finally:
            sweep(root)
            for child in children:
                child.terminate()
            for child in children:
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    child.kill()
            gnome_xvfb.stop_owned(server)
            log.close()


def off_panel_ink(cell):
    """Pixels of a cell whose colour is far from the cell's own dominant (panel) colour: glyphs, text, icons."""
    colours = cell.getcolors(maxcolors=1 << 20) or []
    if not colours:
        return 0
    base = max(colours)[1]
    return sum(1 for px in cell.get_flattened_data() if max(abs(px[i] - base[i]) for i in range(3)) > 60)


def surface_geometry(run, win):
    out = run("xdotool", "getwindowgeometry", "--shell", win)
    vals = dict(line.split("=") for line in out.splitlines() if "=" in line)
    return int(vals["X"]), int(vals["Y"]), int(vals["WIDTH"]), int(vals["HEIGHT"])


def start_centre(debug, edge, surface, scale):
    """Screen position of the Start cell's centre, from the dock's debug layout dump (surface coordinates)."""
    try:
        text = Path(debug).read_text()
    except OSError:
        return None
    for line in text.splitlines():
        if line.startswith("start "):
            a, b = (float(v) for v in line.split()[1:3])
            return round(surface[0] + a), round(surface[1] + b)
    return None


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    opts = {a.split("=")[0]: a.split("=")[1] for a in sys.argv[1:] if a.startswith("--") and "=" in a}
    bin_dir, themes, evidence = (Path(p).resolve() for p in args[:3])
    evidence.mkdir(parents=True, exist_ok=True)
    scales = [float(v) for v in opts.get("--scales", "1,1.25,2").split(",")]
    edges = opts.get("--edges", "bottom,top,left,right").split(",")
    modes = opts.get("--modes", "dark,light").split(",")
    report = []
    for mode in modes:
        for edge in edges:
            for scale in scales:
                run_one(bin_dir, themes, evidence, mode, edge, scale, report)
    out = {"binary_sha256": {n: hashlib.sha256((bin_dir / n).read_bytes()).hexdigest() for n in ["decdock", "decthemectl"]}, "captures": report}
    (evidence / "report.json").write_text(json.dumps(out, indent=2))
    print(json.dumps({"captures": len(report)}))


if __name__ == "__main__":
    main()
