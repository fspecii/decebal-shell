#!/usr/bin/env python3
"""Native acceptance of the Plasma Breeze package (the taskbar renderer) against the branding contract, on ONE private Xvfb at a time.

  python3 tests/e2e/plasma-packages-native.py --bin-dir DIR --repo DIR --rdwm FILE --rdwmctl FILE --out NEW_DIR
        [--packages plasma-breeze] [--edges bottom,top,left,right] [--scales 1,1.25,2] [--modes dark,light] [--cases glyph,applets,launcher,popup,keyboard]

Everything runs with an environment built from nothing (no host DISPLAY, WAYLAND_DISPLAY, XAUTHORITY, session or bus variable): a private HOME / XDG,
a private dbus-daemon, `tests/e2e/private_x11.py` (a reserved high display number, both listening transports verified to belong to the exact Xvfb child
before any client or input), the pinned window manager and rdwmctl given by --rdwm / --rdwmctl (their sha256 are recorded), the debug binaries of
--bin-dir (recorded too) and a private XRender compositor. Nothing outside the output directory is written and only the processes of this run are stopped.

Per package x edge x scale x mode it proves, with real pointer and keyboard input and the real window manager:
* strut == the work area rdwm reports, and a fake client window lies inside it;
* glyph:    the Start button shows Kickoff's icon (`start-here-kde-symbolic`), which differs from the neutral grid, the Windows squares and the
            Apple glyph (control packages are rendered the same way), and no search box is drawn for Breeze;
* applets:  every registry applet toggled alone, then all on and all off through `decthemectl panel applet` (the user's real switch): the effective set
            reported by `decthemectl panel --json` and, for the applets this system can supply, the zones the dock reports follow it;
* launcher: a click on Start opens Kickoff (`declaunch`, style kde) on the desktop side of the panel, inside the output, clear of the strip, centred on
            the Start button along the panel as far as the screen allows; a second click closes it;
* popup:    the clock's popup opens on the desktop side of the strip and inside the output, and Escape closes it;
* keyboard: `decdock --focus` draws a ring that moves with the arrow keys, Enter on Start opens Kickoff and Escape leaves keyboard mode.
The first failure of a case is recorded with a screenshot and the run goes on; the exit status is 1 when any case failed.
"""
import argparse
import hashlib
import itertools
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

from PIL import Image, ImageChops

sys.path.insert(0, str(Path(__file__).resolve().parent))
import private_x11  # noqa: E402

W, H = 1920, 1080
THICKNESS, GAP = 44, 8  # the declared [taskbar] numbers of plasma-breeze, px at scale 1
REGISTRY = ["launcher", "app_menu", "workspaces", "window_title", "tasks", "layout", "media", "clock", "weather", "tray", "clipboard", "cpu", "memory",
            "notifications", "stay_awake", "agents", "lock_keys", "volume", "network", "bluetooth", "battery", "brightness", "search", "dashboard",
            "control_center", "power", "show_desktop"]
ALONE = ("launcher", "tasks", "clock", "show_desktop", "workspaces")  # the applets whose drawing this machine can supply in a private world


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def wait(predicate, label, tries=150, delay=0.1):
    last = None
    for _ in range(tries):
        try:
            value = predicate()
            if value:
                return value
        except (OSError, ValueError, subprocess.CalledProcessError, IndexError, KeyError) as e:
            last = e
        time.sleep(delay)
    raise AssertionError(f"{label}{f' ({last})' if last else ''}")


def luminance(px):
    def lin(v):
        v /= 255
        return v / 12.92 if v <= 0.03928 else ((v + 0.055) / 1.055) ** 2.4
    return 0.2126 * lin(px[0]) + 0.7152 * lin(px[1]) + 0.0722 * lin(px[2])


def contrast(a, b):
    la, lb = sorted((luminance(a), luminance(b)), reverse=True)
    return (la + 0.05) / (lb + 0.05)


def ink_distance(a, b):
    return sum(max(abs(p - q) for p, q in zip(pa, pb)) for pa, pb in zip(a.get_flattened_data(), b.get_flattened_data()))


WORLD = itertools.count(1)


class Case:
    """One private world: Xvfb, bus, compositor, window manager, the dock of a package."""

    def __init__(self, args, package, edge, scale, mode, tag, themes_dir=None):
        self.args, self.package, self.edge, self.scale, self.mode, self.tag = args, package, edge, scale, mode, tag
        self.horizontal = edge in ("bottom", "top")
        self.t, self.g = round(THICKNESS * scale), round(GAP * scale)
        self.procs = []
        self.themes = themes_dir or Path(args.repo) / "themes"
        self.root = Path(args.worlds) / f"w{next(WORLD)}"  # short on purpose: unix socket paths are limited to 108 bytes
        self.root.mkdir(mode=0o700)
        r = self.root
        for sub in ("home", "config", "cache", "state", "data", "run", "bin", "empty"):
            (r / sub).mkdir(mode=0o700)
        for app in ("curl", "nmcli", "pactl", "wpctl", "playerctl", "bluetoothctl", "brightnessctl"):
            (r / "bin" / app).write_text("#!/bin/sh\nexit 1\n")
            (r / "bin" / app).chmod(0o755)
        self.debug = r / "layout.txt"
        (r / "rdwm.toml").write_text("[general]\nborder_px = 0\n[bar]\nshow = false\n[decoration]\nenabled = false\n")
        self.env = dict(
            HOME=str(r / "home"), XDG_CONFIG_HOME=str(r / "config"), XDG_CACHE_HOME=str(r / "cache"), XDG_STATE_HOME=str(r / "state"),
            XDG_DATA_HOME=str(r / "data"), XDG_RUNTIME_DIR=str(r / "run"), DECEBAL_CONFIG=str(r / "config"), DECEBAL_THEMES=str(self.themes),
            DECEBAL_SYSTEM_THEMES=str(r / "empty"), DECEBAL_BACKEND="x11", DECEBAL_SYSTEM_AUTOMATIC="0", DECEBAL_SCALE=str(scale),
            DECDOCK_DEBUG_LAYOUT=str(self.debug), RDWMCTL=str(args.rdwmctl), RDWM_SOCK=str(r / "run/rdwm.sock"), RDWM_CONFIG=str(r / "rdwm.toml"),
            PATH=f"{r / 'bin'}:{args.bin_dir}:/usr/bin:/bin", LANG="C.UTF-8",
        )
        self.log = (Path(args.out) / f"{tag}.log").open("w")
        self.steps = []

    # ------------------------------------------------------------------ plumbing
    def run(self, *argv, timeout=30, check=True):
        assert self.env.get("DISPLAY", "").startswith(":") and int(self.env["DISPLAY"][1:]) >= 700, "refusing a client call before the private server was verified"
        return subprocess.run([str(a) for a in argv], env=self.env, text=True, capture_output=True, check=check, timeout=timeout).stdout.strip()

    def spawn(self, *argv):
        assert "DISPLAY" in self.env
        p = subprocess.Popen([str(a) for a in argv], env=self.env, stdout=self.log, stderr=self.log, start_new_session=True)
        self.procs.append(p)
        return p

    def bin(self, name):
        return Path(self.args.bin_dir) / name

    def step(self, name, **data):
        self.steps.append({"step": name, **data})

    def start(self):
        self.x, display = private_x11.start_private_xvfb(env=dict(self.env), log=self.log, screen=f"{W}x{H}x24", executable="Xvfb")
        self.env["DISPLAY"] = display
        self.display = display
        bus = subprocess.Popen(["dbus-daemon", "--session", "--nofork", "--nopidfile", "--print-address=1"], env=self.env, stdout=subprocess.PIPE, stderr=self.log, start_new_session=True, text=True)
        self.procs.append(bus)
        self.env["DBUS_SESSION_BUS_ADDRESS"] = bus.stdout.readline().strip()
        self.env["DBUS_SYSTEM_BUS_ADDRESS"] = "unix:path=" + str(self.root / "no-system-bus")
        wait(lambda: self.run("xdotool", "getdisplaygeometry"), "private Xvfb answers")
        (self.root / "picom.conf").write_text('backend = "xrender";\nvsync = false;\nuse-damage = false;\nshadow = false;\nfading = false;\n')
        self.spawn("picom", "--config", self.root / "picom.conf")
        time.sleep(0.8)
        self.spawn(self.args.rdwm, "--backend", "x11", "--config", self.root / "rdwm.toml")
        wait(lambda: (self.root / "run/rdwm.sock").exists() and json.loads(self.run(self.args.rdwmctl, "state"))["ok"], "private pinned rdwm up")
        self.sweep_needle = f"XDG_RUNTIME_DIR={self.root}/run".encode()

    def stop(self):
        # everything started with this world's runtime directory (`theme apply` spawns the dock detached), then our own children
        for proc in Path("/proc").iterdir():
            if proc.name.isdigit() and int(proc.name) != os.getpid():
                try:
                    if getattr(self, "sweep_needle", None) in (proc / "environ").read_bytes().split(b"\0"):
                        os.kill(int(proc.name), 15)
                except (OSError, ValueError):
                    pass
        for p in reversed(self.procs):
            p.terminate()
        for p in self.procs:
            try:
                p.wait(timeout=5)
            except subprocess.TimeoutExpired:
                p.kill()
        if getattr(self, "x", None):
            private_x11.close(self.x)
        self.log.close()

    def shot(self, name, crop=None):
        path = Path(self.args.out) / f"{self.tag}-{name}.png"
        self.run("import", "-window", "root", path)
        im = Image.open(path).convert("RGB")
        return im.crop(crop) if crop else im

    @property
    def bar_mode(self):
        """The package's panel is the flat bar (`decbar`) rather than the taskbar (`decdock`)."""
        return self.package == "plasma-layan"

    def apply(self):
        # a taskbar package follows the user's dock edge; a bar package follows the user's bar edge (Settings -> Panels writes these)
        (self.root / "config/dock-settings.json").write_text(json.dumps({"edge": "bottom" if self.bar_mode else self.edge}))
        self.run(self.bin("decthemectl"), "theme", "apply", self.package, "--no-rdwm")
        cfg = json.loads((self.root / "config/decebal.json").read_text())
        cfg["theme"]["mode"] = self.mode
        if self.bar_mode:
            cfg.setdefault("bar", {})["edge"] = self.edge
        (self.root / "config/decebal.json").write_text(json.dumps(cfg))
        if self.bar_mode:
            self.spawn(self.bin("decbar"))
            self.bar_t = None

    def dump(self):
        rows = {}
        for _ in range(60):
            try:
                text = self.debug.read_text()
            except FileNotFoundError:
                text = ""
            if text.strip():
                for line in text.splitlines():
                    parts = line.split()
                    rows.setdefault(parts[0], []).append(parts[1:])
                return rows
            time.sleep(0.1)
        raise AssertionError("the dock's layout dump never appeared")

    def surface(self):
        win = self.dock_window()
        out = self.run("xdotool", "getwindowgeometry", "--shell", win)
        v = dict(line.split("=") for line in out.splitlines() if "=" in line)
        return {k: int(x) for k, x in v.items() if k in ("X", "Y", "WIDTH", "HEIGHT")}

    def dock_window(self):
        return wait(lambda: self.run("xdotool", "search", "--name", "decebal-dock", check=False).splitlines()[0], "dock window")

    def at(self, kind, index=0):
        """Screen position of a dumped zone's centre (the dump's coordinates are surface coordinates: x across / along the strip as drawn)."""
        s, rows = self.surface(), self.dump()
        row = rows[kind][index]
        if kind == "pagercell":
            start = rows["start"][0]
            along = float(row[1])
            return (round(s["X"] + along), round(s["Y"] + float(start[1]))) if self.horizontal else (round(s["X"] + float(start[0])), round(s["Y"] + along))
        a, b = (float(row[1]), float(row[2])) if kind == "task" else (float(row[0]), float(row[1]))
        return round(s["X"] + a), round(s["Y"] + b)

    def click(self, x, y):
        self.run("xdotool", "mousemove", x, y)
        time.sleep(0.15)
        self.run("xdotool", "click", "1")
        time.sleep(0.6)

    def park(self):
        if self.bar_mode:
            x0, y0, x1, y1 = self.strip_box()
            self.run("xdotool", "mousemove", *((1100, (y0 + y1) // 2) if self.horizontal else ((x0 + x1) // 2, 1100)))
            return
        s = self.surface()
        sx, sy = (float(v) for v in self.dump()["start"][0])
        self.run("xdotool", "mousemove", round(s["X"] + 1100) if self.horizontal else round(s["X"] + sx), round(s["Y"] + sy) if self.horizontal else round(s["Y"] + 1100))

    def windows(self):
        return set(self.run("xdotool", "search", "--onlyvisible", "--name", ".", check=False).split())

    def geometry(self, win):
        out = self.run("xdotool", "getwindowgeometry", "--shell", win)
        v = dict(line.split("=") for line in out.splitlines() if "=" in line)
        return int(v["X"]), int(v["Y"]), int(v["WIDTH"]), int(v["HEIGHT"])

    def bar_window(self):
        return wait(lambda: self.run("xdotool", "search", "--name", "decebal-bar", check=False).splitlines()[0], "bar window")

    def bar_strut(self):
        """[left, right, top, bottom] the bar publishes."""
        return wait(lambda: [int(v) for v in self.run("xprop", "-id", self.bar_window(), "_NET_WM_STRUT_PARTIAL").split("=")[-1].replace(" ", "").split(",") if v.lstrip("-").isdigit()][:4], "bar strut")

    def strip_box(self):
        if self.bar_mode:
            l, r, t_, b = self.bar_strut()
            return {"top": (0, 0, W, t_), "bottom": (0, H - b, W, H), "left": (0, 0, l, H), "right": (W - r, 0, W, H)}[self.edge]
        t, g = self.t, self.g
        return {"bottom": (g, H - g - t, W - g, H - g), "top": (g, g, W - g, g + t), "left": (g, g, g + t, H - g), "right": (W - g - t, g, W - g, H - g)}[self.edge]

    def desktop_side_ok(self, rect, what):
        """`rect` (x, y, w, h) lies inside the output and entirely on the desktop side of the strip."""
        x, y, w, h = rect
        sx0, sy0, sx1, sy1 = self.strip_box()
        assert x >= -1 and y >= -1 and x + w <= W + 1 and y + h <= H + 1, f"{what} {rect} leaves the output"
        ok = {"bottom": y + h <= sy0 + 1, "top": y >= sy1 - 1, "left": x >= sx1 - 1, "right": x + w <= sx0 + 1}[self.edge]
        assert ok, f"{what} {rect} is not on the desktop side of the strip {self.strip_box()} ({self.edge})"


# ---------------------------------------------------------------------------------------------------------------------- cases
def case_glyph(c, control_shot):
    """The Start glyph is Kickoff's icon, not a neutral grid / Windows / Apple mark; Breeze draws no search box."""
    c.park()
    time.sleep(0.5)
    shot = c.shot("start")
    x, y = c.at("start")
    half = round(14 * c.scale)
    box = (x - half, y - half, x + half, y + half)
    glyph = shot.crop(box)
    ink = sum(1 for px in glyph.get_flattened_data() if max(px) - min(px) > 60 or luminance(px) > 0.6 or luminance(px) < 0.02)
    assert ink > 20 * c.scale, f"{c.tag}: the Start cell shows no glyph"
    c.step("glyph", box=box, ink_px=ink)
    if control_shot is not None:
        for name, other in control_shot.items():
            d = ink_distance(glyph, other.crop(box))
            assert d > 400 * c.scale, f"{c.tag}: the Start glyph equals the {name} control (distance {d})"
            c.step("glyph_vs_" + name, distance=d)
    assert "search" not in c.dump(), f"{c.tag}: Breeze draws a search zone"
    c.step("no_search_zone")


def panel_json(c):
    return json.loads(c.run(c.bin("decthemectl"), "panel", "--json"))


def effective(c):
    """{applet key: on?} from `decthemectl panel --json`, whatever shape the listing has."""
    data = panel_json(c)
    found = {}

    def walk(node):
        if isinstance(node, dict):
            key = node.get("key") or node.get("id") or node.get("name")
            if key in REGISTRY and any(k in node for k in ("on", "enabled", "shown", "active")):
                found[key] = bool(node.get("on", node.get("enabled", node.get("shown", node.get("active")))))
            for v in node.values():
                walk(v)
        elif isinstance(node, list):
            for v in node:
                walk(v)
    walk(data)
    if not found and isinstance(data.get("applets"), dict):
        found = {k: bool(v if not isinstance(v, dict) else v.get("on", v.get("enabled"))) for k, v in data["applets"].items() if k in REGISTRY}
    return found


def ink_groups(c, shot):
    """Runs of content along the strip: the strip's interior (rounded ends and edge line excluded) is reduced to the pixels that differ from its own
    dominant colour, projected on the strip's axis, and runs separated by less than a few pixels are one group. The dump lists every zone even when
    its applet is off, and zones collapse into each other's place when a neighbour is off, so what is drawn is counted, not where zones are reported."""
    sx0, sy0, sx1, sy1 = c.strip_box()
    crop = shot.crop((sx0 + 6, sy0 + 6, sx1 - 6, sy1 - 6))
    colours = crop.getcolors(maxcolors=1 << 22) or []
    base = max(colours)[1]
    mask = Image.new("L", crop.size, 0)
    mask.putdata([255 if max(abs(px[i] - base[i]) for i in range(3)) > 40 else 0 for px in crop.get_flattened_data()])
    along = mask.resize((crop.width, 1), Image.BOX) if c.horizontal else mask.resize((1, crop.height), Image.BOX)
    on = [v > 0 for v in along.get_flattened_data()]
    runs, start, last = [], None, None
    join = max(5, round(6 * c.scale))
    for i, v in enumerate(on):
        if v:
            if start is None:
                start = i
            elif i - last > join:
                runs.append((start, last))
                start = i
            last = i
    if start is not None:
        runs.append((start, last))
    return runs


def drawn(c, label):
    c.park()
    time.sleep(1.0)
    shot = c.shot(label)
    return len(ink_groups(c, shot))


def case_applets(c):
    """Every registry applet alone, all on, all off: through the user's own switch (`decthemectl panel applet`), read back from the CLI and from what
    the dock draws along the strip."""
    results = {}
    baseline = effective(c)
    missing = sorted(set(REGISTRY) - set(baseline))
    assert not missing, f"{c.tag}: the panel listing lacks applets {missing}"
    ctl = c.bin("decthemectl")
    for key in REGISTRY:
        c.run(ctl, "panel", "applet", key, "off")
    wait(lambda: not any(effective(c).values()), f"{c.tag}: all applets off")
    n = drawn(c, "all-off")
    assert n == 0, f"{c.tag}: with every applet off the dock still draws {n} groups"
    results["all_off_groups"] = n
    # groups each applet draws alone: the glyph, the pinned task buttons (one group each, 4+ px apart), the clock, the show-desktop icon, the pager cells
    for key in ALONE:
        c.run(ctl, "panel", "applet", key, "on")
        wait(lambda: effective(c).get(key) and sum(effective(c).values()) == 1, f"{c.tag}: {key} alone")
        n = drawn(c, f"alone-{key}")
        results[key + "_alone_groups"] = n
        assert n >= 1, f"{c.tag}: {key} is on but draws nothing"
        # a side strip stacks the clock as two rows (time over date, upright), so its clock is two groups along the strip
        if key == "clock" and not c.horizontal:
            assert n == 2, f"{c.tag}: the stacked clock draws {n} groups"
        elif key != "tasks":
            assert n == 1, f"{c.tag}: {key} alone draws {n} separate groups"
        c.run(ctl, "panel", "applet", key, "off")
        wait(lambda: not any(effective(c).values()), f"{c.tag}: {key} off again")
    # the applets that need data this private world cannot supply (network, volume, battery, ...) and the rest of the registry: on and off through the
    # same switch, read back from the CLI
    for key in REGISTRY:
        if key in ALONE:
            continue
        c.run(ctl, "panel", "applet", key, "on")
        wait(lambda: effective(c).get(key) is True, f"{c.tag}: {key} on")
        c.run(ctl, "panel", "applet", key, "off")
        wait(lambda: effective(c).get(key) is False, f"{c.tag}: {key} off")
    results["registry_roundtrip"] = len(REGISTRY)
    for key in REGISTRY:
        c.run(ctl, "panel", "applet", key, "on")
    wait(lambda: all(effective(c).values()), f"{c.tag}: all on")
    n_on = drawn(c, "all-on")
    assert n_on >= 5, f"{c.tag}: with everything on only {n_on} groups are drawn"
    results["all_on_groups"] = n_on
    c.run(ctl, "panel", "reset")
    wait(lambda: effective(c) == baseline, f"{c.tag}: `panel reset` returns the package's own set")
    c.step("applets", **results)


def card_box(c, base, open_shot):
    """The card Kickoff draws: its window is a transparent full-output overlay (the strip is left clear), so the card is where the screen changed
    outside the strip."""
    mask = ImageChops.difference(base, open_shot).convert("L").point(lambda v: 255 if v > 10 else 0)
    x0, y0, x1, y1 = c.strip_box()
    mask.paste(0, (x0 - 1, y0 - 1, x1 + 1, y1 + 1))
    return mask.getbbox()


def await_card(c, base, label):
    """Kickoff's window maps before its card is painted (the application scan runs first): poll the screen for up to 8 s instead of one fixed delay."""
    deadline, shot, box = time.time() + 8, None, None
    while time.time() < deadline:
        time.sleep(0.5)
        shot = c.shot(label)
        box = card_box(c, base, shot)
        if box:
            time.sleep(0.4)  # let it finish drawing, then measure the settled card
            shot = c.shot(label)
            return card_box(c, base, shot), shot
    return None, shot


def case_launcher(c):
    """A click on Start opens Kickoff towards the desktop (an anchored popup beside the panel); a second click closes it."""
    c.park()
    time.sleep(0.6)
    base = c.shot("kickoff-before")
    before = c.windows()
    c.click(*c.at("start"))
    new = wait(lambda: c.windows() - before, f"{c.tag}: Kickoff did not open", tries=100)
    box, opened = await_card(c, base, "kickoff")
    if not box:
        c.click(*c.at("start"))
    assert box, f"{c.tag}: Kickoff drew nothing outside the strip"
    rect = (box[0], box[1], box[2] - box[0], box[3] - box[1])
    try:
        c.desktop_side_ok(rect, "Kickoff card")
    except AssertionError:
        c.click(*c.at("start"))
        raise
    # the style's gap to the panel is 4 logical px (docs/launcher-styles.md); allow the shadow / antialiasing
    sx0, sy0, sx1, sy1 = c.strip_box()
    gap = {"bottom": sy0 - box[3], "top": box[1] - sy1, "left": box[0] - sx1, "right": sx0 - box[2]}[c.edge]
    near = 0 <= gap <= round(24 * c.scale)
    sx, sy = c.at("start")
    along = ((box[0] + box[2]) / 2 - sx) if c.horizontal else ((box[1] + box[3]) / 2 - sy)
    clamped = (box[0] <= round(12 * c.scale)) if c.horizontal else (box[1] <= round(12 * c.scale))
    anchored = near and (clamped or abs(along) <= 3)
    # The taskbar's Start currently runs `declaunch` without `--edge/--anchor/--thickness` (dock_app/windows.rs, `Hit::Start`), so Kickoff opens in its
    # natural place (centred along the panel, against the strip) instead of hanging from the button. That is recorded as a measured gap; with
    # --require-anchor it is a failure.
    c.click(*c.at("start"))
    wait(lambda: not (c.windows() & new), f"{c.tag}: a second click did not close Kickoff")
    c.park()
    c.step("launcher", card=rect, gap=gap, near_the_panel=near, centre_offset_along=round(along, 1), anchored_on_start=anchored, clamped_to_margin=clamped)
    assert anchored or not c.args.require_anchor, f"{c.tag}: Kickoff is not anchored on Start (gap {gap}px from the strip, offset {along:.0f}px along it)"


def case_popup(c):
    c.park()
    cx, cy = c.at("clock")
    c.click(cx, cy)
    popup = wait(lambda: c.dump().get("popup"), f"{c.tag}: the clock popup did not open")
    px, py, pw, ph = (float(v) for v in popup[0])
    s = c.surface()
    c.desktop_side_ok((s["X"] + px, s["Y"] + py, pw, ph), "clock popup")
    c.shot("popup")
    c.run("xdotool", "key", "Escape")
    wait(lambda: not c.dump().get("popup"), f"{c.tag}: Escape did not close the popup")
    c.step("popup", rect=[px, py, pw, ph])


def case_keyboard(c):
    """`decdock --focus`: a ring on Start that moves with the arrows, Enter opens Kickoff, Escape leaves."""
    c.park()
    time.sleep(0.5)
    base = c.shot("kb-base")
    c.spawn(c.bin("decdock"), "--focus")
    time.sleep(1.0)
    ringed = c.shot("kb-ring")
    box = base.getbbox()
    assert ink_distance(ringed, base) > 200, f"{c.tag}: --focus drew no ring"
    key = "Down" if not c.horizontal else "Right"
    c.run("xdotool", "key", key)
    time.sleep(0.6)
    moved = c.shot("kb-moved")
    assert ink_distance(moved, ringed) > 100, f"{c.tag}: the ring did not move with the arrow key"
    c.run("xdotool", "key", "Escape")
    time.sleep(0.8)
    left = c.shot("kb-left")
    assert ink_distance(left, base) < ink_distance(ringed, base) * 0.2, f"{c.tag}: Escape did not leave keyboard mode"
    c.step("keyboard", ring=ink_distance(ringed, base), moved=ink_distance(moved, ringed))



# ------------------------------------------------------------------------------------------------------------ the flat bar (Layan)
def first_item(c, shot):
    """Centre (screen px) of the first thing the bar draws: the Kickoff button."""
    sx0, sy0, sx1, sy1 = c.strip_box()
    runs = ink_groups(c, shot)
    assert runs, f"{c.tag}: the bar draws nothing"
    along = (runs[0][0] + runs[0][1]) / 2 + 6
    return (round(sx0 + along), (sy0 + sy1) // 2) if c.horizontal else ((sx0 + sx1) // 2, round(sy0 + along))


def case_bar(c, controls):
    """The bar is on its edge with its strut, the work area follows every strut, the glyph is Kickoff's (not Apple, Windows or the grid) and the
    search applet is a magnifier icon, not a box."""
    c.park()
    time.sleep(1.0)
    shot = c.shot("bar")
    struts = {"bar": c.bar_strut(), "dock": [int(v) for v in c.run("xprop", "-id", c.dock_window(), "_NET_WM_STRUT_PARTIAL").split("=")[-1].replace(" ", "").split(",") if v.lstrip("-").isdigit()][:4]}
    left, right, top, bottom = (max(st[i] for st in struts.values()) for i in range(4))
    expect = (left, top, W - left - right, H - top - bottom)
    area = json.loads(c.run(c.args.rdwmctl, "state"))["state"]["monitors"][0]["workarea"]
    assert (area["x"], area["y"], area["w"], area["h"]) == expect, f"{c.tag}: work area {area} vs the struts {struts}"
    side = {"left": 0, "right": 1, "top": 2, "bottom": 3}[c.edge]
    assert struts["bar"][side] > 0, f"{c.tag}: the bar publishes no strut on its edge {struts['bar']}"
    c.step("bar_strut", struts=struts, work_area=list(expect))
    x, y = first_item(c, shot)
    half = round(14 * c.scale)
    box = (x - half, y - half, x + half, y + half)
    glyph = shot.crop(box)
    for name, other in (controls or {}).items():
        d = ink_distance(glyph, other.crop(box))
        assert d > 300 * c.scale, f"{c.tag}: the first item equals the {name} control (distance {d})"
        c.step("glyph_vs_" + name, distance=d)
    eff = effective(c)
    assert eff.get("search") and not eff.get("app_menu") and eff.get("window_title"), f"{c.tag}: the package's applet set is not in effect: {eff}"
    # the search applet is an icon: switching it off removes ink from the bar and nothing else moves (the strip's other items keep their place)
    ctl = c.bin("decthemectl")
    c.run(ctl, "panel", "applet", "search", "off")
    wait(lambda: effective(c).get("search") is False, f"{c.tag}: search off")
    c.park()
    time.sleep(1.0)
    without = c.shot("bar-no-search")
    c.run(ctl, "panel", "applet", "search", "theme")
    wait(lambda: effective(c).get("search") is True, f"{c.tag}: search back to the theme's choice")
    # the search applet is one icon button: switching it off removes exactly one item from the strip (the neighbours reflow, so a pixel diff would be wider)
    on_groups, off_groups = len(ink_groups(c, shot)), len(ink_groups(c, without))
    assert on_groups == off_groups + 1, f"{c.tag}: search on draws {on_groups} items, off {off_groups}: it is not one icon"
    c.step("search_icon", items_on=on_groups, items_off=off_groups)


def case_bar_launcher(c):
    """The bar passes `--edge/--anchor/--thickness`, so Kickoff opens from the button towards the desktop, centred on it or at the output margin."""
    c.park()
    time.sleep(0.8)
    base = c.shot("kickoff-before")
    x, y = first_item(c, base)
    before = c.windows()
    c.click(x, y)
    new = wait(lambda: c.windows() - before, f"{c.tag}: Kickoff did not open", tries=100)
    box, _ = await_card(c, base, "kickoff")
    c.click(x, y)
    assert box, f"{c.tag}: Kickoff drew nothing outside the strip"
    rect = (box[0], box[1], box[2] - box[0], box[3] - box[1])
    c.desktop_side_ok(rect, "Kickoff card")
    sx0, sy0, sx1, sy1 = c.strip_box()
    gap = {"bottom": sy0 - box[3], "top": box[1] - sy1, "left": box[0] - sx1, "right": sx0 - box[2]}[c.edge]
    assert 0 <= gap <= round(24 * c.scale), f"{c.tag}: Kickoff sits {gap}px from the bar"
    along = ((box[0] + box[2]) / 2 - x) if c.horizontal else ((box[1] + box[3]) / 2 - y)
    clamped = (box[0] <= round(12 * c.scale)) if c.horizontal else (box[1] <= round(12 * c.scale))
    assert clamped or abs(along) <= 3, f"{c.tag}: Kickoff is neither centred on its button nor at the output margin (offset {along})"
    wait(lambda: not (c.windows() & new), f"{c.tag}: a second click did not close Kickoff")
    c.step("launcher", card=rect, gap=gap, centre_offset_along=round(along, 1), clamped_to_margin=clamped)


def case_bar_appmenu(c):
    """`app_menu` on: with no exporting window the bar says so honestly (a neutral label, never File/Edit/View of another desktop); with a real Qt window
    that exports its menu over com.canonical.AppMenu.Registrar the bar shows that window's own titles."""
    ctl = c.bin("decthemectl")
    c.park()
    time.sleep(0.8)
    base = c.shot("appmenu-off")
    c.run(ctl, "panel", "applet", "app_menu", "on")
    wait(lambda: effective(c).get("app_menu") is True, f"{c.tag}: app_menu on")
    c.park()
    time.sleep(2.0)
    idle = c.shot("appmenu-no-window")
    assert ImageChops.difference(base, idle).convert("L").point(lambda v: 255 if v > 24 else 0).getbbox(), f"{c.tag}: turning app_menu on changed nothing"
    record, ready = c.root / "appmenu.record", c.root / "appmenu.ready"
    env = dict(c.env, QT_QPA_PLATFORM="xcb", APPMENU_RECORD=str(record), APPMENU_READY=str(ready))
    p = subprocess.Popen([str(c.args.qt_fixture)], env=env, stdout=c.log, stderr=c.log, stdin=subprocess.DEVNULL, start_new_session=True)
    c.procs.append(p)
    wait(lambda: ready.exists() and ready.read_text().strip(), f"{c.tag}: the Qt fixture window did not map", tries=200)
    c.park()
    time.sleep(3.0)
    exported = c.shot("appmenu-qt-window")
    strip = c.strip_box()
    changed = ImageChops.difference(idle.crop(strip), exported.crop(strip)).convert("L").point(lambda v: 255 if v > 24 else 0).getbbox()
    assert changed, f"{c.tag}: the bar did not change for a focused window that exports a menu"
    # the fixture's titles are File and Edit; the strip now holds more items than with no window (the window's own titles beside the neutral ones)
    before_items, after_items = len(ink_groups(c, idle)), len(ink_groups(c, exported))
    assert after_items > before_items, f"{c.tag}: the exported titles were not drawn ({before_items} -> {after_items} items)"
    c.step("appmenu_live", changed_in_strip=changed, items_without_window=before_items, items_with_exporting_window=after_items, fixture_sha256=sha(c.args.qt_fixture))
    c.run(ctl, "panel", "applet", "app_menu", "theme")
    wait(lambda: effective(c).get("app_menu") is False, f"{c.tag}: app_menu back to the theme's choice (off)")


def case_keyboard_bar(c):
    """`decbar --focus`: a ring that moves with the arrows; Escape leaves."""
    c.park()
    time.sleep(0.6)
    base = c.shot("kb-base")
    c.spawn(c.bin("decbar"), "--focus")
    time.sleep(1.2)
    ringed = c.shot("kb-ring")
    assert ink_distance(ringed, base) > 150, f"{c.tag}: --focus drew no ring"
    c.run("xdotool", "key", "Right" if c.horizontal else "Down")
    time.sleep(0.6)
    moved = c.shot("kb-moved")
    assert ink_distance(moved, ringed) > 80, f"{c.tag}: the ring did not move with the arrow key"
    c.run("xdotool", "key", "Escape")
    time.sleep(0.8)
    assert ink_distance(c.shot("kb-left"), base) < ink_distance(ringed, base) * 0.25, f"{c.tag}: Escape did not leave keyboard mode"
    c.step("keyboard", ring=ink_distance(ringed, base), moved=ink_distance(moved, ringed))


CASES = {"glyph": case_glyph, "applets": case_applets, "launcher": case_launcher, "popup": case_popup, "keyboard": case_keyboard,
         "bar": case_bar, "bar_launcher": case_bar_launcher, "appmenu": case_bar_appmenu, "keyboard_bar": case_keyboard_bar}


def control_copy(args, package, logo, out_root):
    """A fresh copy of the package whose logo is replaced by `logo`: the control a real glyph must differ from."""
    d = Path(out_root)
    shutil.copytree(Path(args.repo) / "themes" / package, d / package)
    text = (d / package / "theme.toml").read_text()
    new = text.replace('logo = "icon:start-here-kde-symbolic"', f'logo = "{logo}"')
    assert new != text
    (d / package / "theme.toml").write_text(new)
    return d


def run_case(args, package, edge, scale, mode, cases, controls, report):
    for attempt in range(2):
        before = len(report)
        ok = run_case_once(args, package, edge, scale, mode, cases, controls, report)
        entry = report[-1]
        infra = entry.get("error") and any(k in entry["error"] for k in ("Xvfb", "dock window", "bar window", "painted", "private pinned rdwm"))
        if ok or not infra or attempt == 1:
            return ok
        entry["retried"] = True
        print(f"retry {entry.get('package')} {edge} {scale} {mode}: {entry['error']}", flush=True)
        del report[before:]
    return False


def run_case_once(args, package, edge, scale, mode, cases, controls, report):
    tag = f"{package}-{mode}-{edge}-{int(scale * 100)}"
    c = Case(args, package, edge, scale, mode, tag)
    entry = {"package": package, "edge": edge, "scale": scale, "mode": mode, "cases": {}}
    try:
        c.start()
        c.apply()
        if c.bar_mode:
            c.bar_window()
            time.sleep(3.0)
        else:
            c.dock_window()
            wait(lambda: c.dump()["start"], f"{tag}: the dock painted")
            time.sleep(1.0)
            # the strut is the work area, and a client window lies inside it
            strut = wait(lambda: [int(v) for v in c.run("xprop", "-id", c.dock_window(), "_NET_WM_STRUT_PARTIAL").split("=")[-1].replace(" ", "").split(",") if v.lstrip("-").isdigit()][:4], "strut")
            side = {"left": 0, "right": 1, "top": 2, "bottom": 3}[edge]
            reserve = strut[side]
            assert reserve in (c.t + c.g, round((THICKNESS + GAP) * scale)), f"{tag}: strut {strut}"
            expect = {"bottom": (0, 0, W, H - reserve), "top": (0, reserve, W, H - reserve), "left": (reserve, 0, W - reserve, H), "right": (0, 0, W - reserve, H)}[edge]
            area = json.loads(c.run(args.rdwmctl, "state"))["state"]["monitors"][0]["workarea"]
            assert (area["x"], area["y"], area["w"], area["h"]) == expect, f"{tag}: work area {area} vs strut {expect}"
            entry["strut"], entry["work_area"] = strut, list(expect)
        for name in cases:
            try:
                fn = CASES[name]
                fn(c, controls.get((package, mode, edge, scale))) if name in ("glyph", "bar") else fn(c)
                entry["cases"][name] = "ok"
            except AssertionError as e:
                entry["cases"][name] = f"FAIL: {e}"
                try:
                    c.shot(f"fail-{name}")
                except Exception:
                    pass
        entry["steps"] = c.steps
    except BaseException as e:  # a world that did not come up is a failure of this entry, not of the run
        entry["error"] = f"{type(e).__name__}: {e}"
    finally:
        c.stop()
        report.append(entry)
    bad = {k: v for k, v in entry["cases"].items() if v != "ok"}
    print(("ok  " if not bad and "error" not in entry else "FAIL") + f" {tag} {bad or entry.get('error', '')}", flush=True)
    try:
        (Path(args.out) / "partial-report.json").write_text(json.dumps(report, indent=2))
    except OSError:
        pass
    return not bad and "error" not in entry


def capture_controls_once(args, package, edge, scale, mode, report):
    """Start-button crops of the same panel with the logo replaced by builtin:grid / builtin:windows / builtin:apple."""
    shots = {}
    for logo in ("grid", "windows", "apple"):
        themes = control_copy(args, package, f"builtin:{logo}", Path(args.worlds) / f"ctl{next(WORLD)}")
        c = Case(args, package, edge, scale, mode, f"{package}-{mode}-{edge}-{int(scale * 100)}-control-{logo}", themes_dir=themes)
        try:
            c.start()
            c.apply()
            if c.bar_mode:
                c.bar_window()
                time.sleep(3.0)
            else:
                wait(lambda: c.dump()["start"], "control dock painted")
            c.park()
            time.sleep(1.2)
            shots[logo] = c.shot("start")
        finally:
            c.stop()
    return shots


def capture_controls(args, package, edge, scale, mode, report):
    """Infrastructure (a private Xvfb that did not come up in time) is retried; an assertion about the product is not."""
    for attempt in range(3):
        try:
            return capture_controls_once(args, package, edge, scale, mode, report)
        except AssertionError as e:
            if "Xvfb" not in str(e) and "dock painted" not in str(e) and "bar window" not in str(e):
                raise
            print(f"retry controls ({attempt + 1}): {e}", flush=True)
    raise AssertionError("controls could not be captured")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin-dir", required=True)
    ap.add_argument("--repo", required=True)
    ap.add_argument("--rdwm", required=True)
    ap.add_argument("--rdwmctl", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--packages", default="plasma-breeze")
    ap.add_argument("--edges", default="bottom,top,left,right")
    ap.add_argument("--scales", default="1,1.25,2")
    ap.add_argument("--modes", default="dark,light")
    ap.add_argument("--require-anchor", action="store_true", help="fail when Kickoff is not anchored on the Start button")
    ap.add_argument("--qt-fixture", default=str(Path.home() / ".cache/decebal-appmenu-target/appmenu-fixtures/qt-fixture"), help="the root's real Qt menu exporter (tests/e2e/appmenu/qt-fixture.cpp), built by tests/e2e/appmenu-real.sh")
    ap.add_argument("--binary-note", default="", help="free text recorded in the report, e.g. the source revision an immutable binary was built from")
    ap.add_argument("--cases", default="", help="default: the package's own list")
    args = ap.parse_args()
    out = Path(args.out)
    out.mkdir(parents=True)  # a fresh directory only
    args.worlds = Path(tempfile.mkdtemp(prefix="plpk-", dir="/var/tmp"))  # short, private, kept (nothing is deleted by path)
    report, failures = [], 0
    for package in args.packages.split(","):
        cases = args.cases.split(",") if args.cases else (["bar", "bar_launcher", "appmenu", "keyboard_bar"] if package == "plasma-layan" else ["glyph", "applets", "launcher", "popup", "keyboard"])
        for mode in args.modes.split(","):
            for edge in args.edges.split(","):
                for scale in (float(v) for v in args.scales.split(",")):
                    controls = {}
                    if "glyph" in cases or "bar" in cases:
                        controls[(package, mode, edge, scale)] = capture_controls(args, package, edge, scale, mode, report)
                    if not run_case(args, package, edge, scale, mode, cases, controls, report):
                        failures += 1
    repo = Path(args.repo)
    manifest = {
        "binary_note": args.binary_note,
        "source_head": subprocess.run(["git", "-C", str(repo), "rev-parse", "HEAD"], text=True, capture_output=True).stdout.strip(),
        "binaries": {p.name: sha(p) for p in sorted(Path(args.bin_dir).glob("dec*")) if p.is_file() and p.name in ("decdock", "decbar", "decthemectl", "declaunch", "decspot")},
        "rdwm": {"path": str(args.rdwm), "sha256": sha(args.rdwm)}, "rdwmctl": {"path": str(args.rdwmctl), "sha256": sha(args.rdwmctl)}, "qt_fixture": {"path": str(args.qt_fixture), "sha256": sha(args.qt_fixture)},
        "harness": {p.name: sha(p) for p in (Path(__file__), Path(private_x11.__file__))},
        "themes": {p: sha(repo / "themes" / p / "theme.toml") for p in args.packages.split(",")},
        "entries": report, "failures": failures,
        "limits": ["X11 only; no Wayland", "network/audio/battery/bluetooth zones need system data this private world does not have, so they are checked as effective settings only"],
    }
    (out / "report.json").write_text(json.dumps(manifest, indent=2))
    print(json.dumps({"entries": len(report), "failures": failures}))
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
