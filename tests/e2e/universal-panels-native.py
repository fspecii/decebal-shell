#!/usr/bin/env python3
"""Native acceptance of the universal panels on private X servers. Never touches the live session: every case gets an
owned high-number Xvfb (tests/e2e/private_x11.py: both listening transports verified as the child's own), its own
HOME/XDG/config/runtime directories, its own session bus (a dbus-daemon this script starts and reaps), a sanitised
environment built from nothing and a PATH holding only fonts tools (plus, for the probe cases, recording stand-ins).
Binaries are read from a pinned directory and never built or written here.

  python3 tests/e2e/universal-panels-native.py --bin-dir /abs/bin --out /abs/new-dir [--repo /abs/checkout] [--cases taskbar,bar,cc,probe] [--quick]

--repo is the checkout whose themes/ the cases load and whose commit the manifest records (default: the checkout this file
lives in); give it when running a copy of this file from elsewhere.

Cases:
  taskbar  the Windows-style taskbar (windows11 theme) on all four edges at display scale 1, 1.25 and 2: on its edge
           with a strut, Start/tray/clock zones, the quick-settings popup towards the desktop, `decdock --focus` ring
           shown and cleared, the summoned Control Center (`decbar --control-center open`) inside the output and off
           the strip.
  bar      every bar style (menu bar, floating, pill rail, Dux) on all four edges with every applet: on its edge, the
           Control Center popup towards the desktop, `decbar --focus` ring shown and cleared.
  cc       the responsive Control Center: card/fluent/Dux/GNOME in the overlay, menu bar, rail and taskbar on short
           outputs at display and text scales up to 200 %: inside the output, every control walked by the keyboard
           inside the visible panel, the pointer reaching the last control after the scroll and the first after the
           wheel.
  probe    disabled applets stop their probes: recording nmcli/pactl/wpctl/bluetoothctl/brightnessctl/playerctl,
           none called with only the clock, all with every applet, none after switching live to the clock only.

Exit status: 0 when every selected case passes, 1 when any fails, 2 when the run could not be set up. The output
directory gets manifest.json (source, binary, harness and theme hashes, the cases and their verdicts, the limits below),
results.json and SHA256SUMS over every artifact.

Limits (also in the manifest): binaries are identified by hash only, not by the source that built them; the strut is
checked as published (no window manager runs, so no work area is computed or honoured); X11 only, no compositor.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import selectors
import shutil
import signal
import subprocess
import sys
import time
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parent))
import private_x11  # noqa: E402

REPO = Path(__file__).resolve().parents[2]  # replaced by --repo
KEYS = ["launcher", "app_menu", "workspaces", "window_title", "tasks", "layout", "media", "clock", "weather", "tray",
        "clipboard", "cpu", "memory", "notifications", "stay_awake", "agents", "lock_keys", "volume", "network",
        "bluetooth", "battery", "brightness", "search", "dashboard", "control_center", "power", "show_desktop"]
PROBE_TOOLS = ["nmcli", "pactl", "wpctl", "bluetoothctl", "brightnessctl", "playerctl"]
TOOLS = ["fc-match", "fc-list"]
HOST_TOOLS = {"xdotool", "xprop", "import"}
# host programs, resolved once against the invoking PATH (the cases' own PATH holds only their stand-ins)
XVFB, DBUS = shutil.which("Xvfb"), shutil.which("dbus-daemon")


def sha(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def write_json(path, obj, **kw):
    with open(path, "w") as f:
        json.dump(obj, f, **kw)


def proc_stat(pid):
    """(uid, process group) of `pid`, or None when it is gone or unreadable."""
    try:
        uid = os.stat(f"/proc/{pid}").st_uid
        stat = Path(f"/proc/{pid}/stat").read_text()
        # the command name is in parentheses and may contain spaces: the fields after the last ')' are fixed
        fields = stat[stat.rindex(")") + 2:].split()
        return uid, int(fields[2])
    except (OSError, ValueError, IndexError):
        return None


def has_marker(pid, marker):
    try:
        return marker in Path(f"/proc/{pid}/environ").read_bytes().split(b"\0")
    except OSError:
        return False


class Case:
    """One case's private world: directories, an owned Xvfb, an owned session bus, owned children.

    Ownership: every process a case starts carries DECEBAL_PANEL_HARNESS_OWNER=<random id> in its environment (its
    descendants inherit it). Cleanup signals only processes of this uid whose environment holds that exact marker,
    re-checked immediately before each signal; nothing else is ever signalled."""

    BUS_TIMEOUT = 10.0

    def __init__(self, out, name, width, height, scale=None, text=None):
        if not hasattr(os, "pidfd_open") or not hasattr(signal, "pidfd_send_signal"):
            raise RuntimeError("private process cleanup requires Linux pidfds")
        self.dir = out / name
        self.dir.mkdir(mode=0o700, parents=True)
        self.name, self.size, self.scale, self.text = name, (width, height), scale, text
        self.procs = []
        self.files = []
        self.x = None
        self.owner = uuid.uuid4().hex
        self.marker = f"DECEBAL_PANEL_HARNESS_OWNER={self.owner}".encode()
        self.log = open(self.dir / "fixture.log", "w")
        for d in ["home", "cfg/omarchy", "cache/decebal", "data", "state", "run", "bin"]:
            (self.dir / d).mkdir(mode=0o700, parents=True, exist_ok=True)
        for tool in TOOLS:
            src = shutil.which(tool)
            if src:
                os.symlink(src, self.dir / "bin" / tool)
        self.env = {
            "HOME": str(self.dir / "home"),
            "XDG_CONFIG_HOME": str(self.dir / "cfg"),
            "XDG_CACHE_HOME": str(self.dir / "cache"),
            "XDG_DATA_HOME": str(self.dir / "data"),
            "XDG_STATE_HOME": str(self.dir / "state"),
            "XDG_RUNTIME_DIR": str(self.dir / "run"),
            "DECEBAL_CONFIG": str(self.dir / "cfg/omarchy"),
            "DECEBAL_THEMES": str(REPO / "themes"),
            "DECEBAL_SYSTEM_AUTOMATIC": "0",
            "DECEBAL_BACKEND": "x11",
            "PATH": str(self.dir / "bin"),
            "LANG": "C.UTF-8",
            "USER": "panel-test",
            "DBUS_SYSTEM_BUS_ADDRESS": "unix:path=" + str(self.dir / "no-system-bus"),
            "DECBAR_DEBUG_TRAY": str(self.dir / "tray.txt"),
            "DECDOCK_DEBUG_LAYOUT": str(self.dir / "layout.txt"),
            "DECBAR_DEBUG_PANEL": str(self.dir / "panel.txt"),
            "DECEBAL_PANEL_HARNESS_OWNER": self.owner,
        }
        if scale:
            self.env["DECEBAL_SCALE"] = str(scale)
        if text:
            self.env["DECEBAL_FONT_SCALE"] = str(text)
        # a fresh weather reading, so nothing is fetched
        write_json(self.dir / "cache/decebal/weather.json", {"temp_c": 9, "code": 122, "night": False, "fetched": int(time.time()), "feels_c": 6, "humidity": 80,
                                                              "wind_kmh": 14, "wind_dir": "NW", "desc": "Overcast", "location": "Fixture", "days": []})

    def __enter__(self):
        # a setup that fails part-way cleans up what it started before the error goes on (__exit__ would not run)
        try:
            self.x, self.display = private_x11.start_private_xvfb(env=dict(self.env), log=self.log, screen=f"{self.size[0]}x{self.size[1]}x24", executable=XVFB)
            self.env["DISPLAY"] = self.display
            bus = subprocess.Popen([DBUS, "--session", "--nofork", "--nopidfile", "--print-address=1"], env=self.env, stdout=subprocess.PIPE,
                                   stderr=self.log, start_new_session=True)
            self.procs.append(bus)
            self.env["DBUS_SESSION_BUS_ADDRESS"] = self.read_line(bus, self.BUS_TIMEOUT)
            return self
        except BaseException:
            self.cleanup()
            raise

    def __exit__(self, *exc):
        self.cleanup()
        return False

    @staticmethod
    def read_line(p, timeout):
        """The first line `p` writes, within `timeout` seconds (a bus that stalls or exits is a setup error)."""
        sel = selectors.DefaultSelector()
        sel.register(p.stdout, selectors.EVENT_READ)
        data, end = b"", time.monotonic() + timeout
        try:
            while b"\n" not in data:
                left = end - time.monotonic()
                if left <= 0:
                    raise RuntimeError(f"no session bus address within {timeout} s")
                if sel.select(left):
                    chunk = os.read(p.stdout.fileno(), 4096)
                    if not chunk:
                        raise RuntimeError("the session bus exited before giving its address")
                    data += chunk
                    if len(data) > 4096:
                        raise RuntimeError("the session bus address exceeds 4096 bytes")
        finally:
            sel.close()
        address = data.split(b"\n", 1)[0].decode().strip()
        if not address:
            raise RuntimeError("the session bus gave an empty address")
        return address

    def owned(self, pgid=None):
        """Processes of this uid carrying this case's marker (optionally only those in process group `pgid`)."""
        out = []
        for d in os.listdir("/proc"):
            if not d.isdigit() or int(d) == os.getpid():
                continue
            st = proc_stat(int(d))
            if st and st[0] == os.getuid() and (pgid is None or st[1] == pgid) and has_marker(int(d), self.marker):
                out.append(int(d))
        return out

    def signal_owned(self, pids, sig):
        for pid in pids:
            try:
                handle = os.pidfd_open(pid)
            except (ProcessLookupError, PermissionError):
                continue
            try:
                st = proc_stat(pid)
                # The handle is acquired before checking ownership: PID reuse between the
                # check and signal can only leave this exact old process gone, never target a new one.
                if st and st[0] == os.getuid() and has_marker(pid, self.marker):
                    try:
                        signal.pidfd_send_signal(handle, sig)
                    except ProcessLookupError:
                        pass
            finally:
                os.close(handle)

    def sweep(self, pgid=None, grace=3.0):
        """Ends the marked processes (of group `pgid`, or all of this case's): TERM, then KILL what remains."""
        self.signal_owned(self.owned(pgid), signal.SIGTERM)
        end = time.monotonic() + grace
        while self.owned(pgid) and time.monotonic() < end:
            time.sleep(0.05)
        self.signal_owned(self.owned(pgid), signal.SIGKILL)
        end = time.monotonic() + grace
        while self.owned(pgid) and time.monotonic() < end:
            time.sleep(0.05)

    def stop(self, p):
        """Ends a child this case started, then what is left of its process group (it led one, start_new_session),
        also when the leader had already exited and its descendants live on."""
        if p.poll() is None:
            try:
                os.killpg(p.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                p.wait(timeout=5)
            except subprocess.TimeoutExpired:
                try:
                    os.killpg(p.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                p.wait(timeout=5)
        self.sweep(pgid=p.pid)
        for pipe in (p.stdin, p.stdout, p.stderr):
            if pipe is not None and not pipe.closed:
                pipe.close()

    def cleanup(self):
        """Everything this case started, whatever state setup or the case reached; safe to call twice."""
        for p in reversed(self.procs):
            self.stop(p)
        self.procs = []
        if self.x is not None:
            private_x11.close(self.x)
            self.x = None
        # anything else still carrying the marker (a descendant that left its group)
        self.sweep()
        for f in self.files + [self.log]:
            if not f.closed:
                f.close()
        self.files = []

    def spawn(self, argv, name, **extra):
        log = open(self.dir / f"{name}.log", "w")
        self.files.append(log)
        p = subprocess.Popen(argv, env={**self.env, **extra}, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        self.procs.append(p)
        return p

    def run(self, argv):
        return subprocess.run(argv, env=self.env, capture_output=True, text=True, timeout=20)

    def config(self, shell, dock):
        write_json(self.dir / "cfg/omarchy/decebal.json", shell)
        write_json(self.dir / "cfg/omarchy/dock-settings.json", dock)

    def x11(self, *argv):
        """X client tools against this case's server only."""
        env = {"DISPLAY": self.display, "PATH": "/usr/bin", "HOME": self.env["HOME"]}
        return subprocess.run([shutil.which(argv[0]), *argv[1:]], env=env, capture_output=True, text=True, timeout=20)

    def shot(self, name):
        path = self.dir / name
        self.x11("import", "-window", "root", str(path))
        return path

    def window(self, name):
        ids = self.x11("xdotool", "search", "--name", f"^{name}$").stdout.split()
        if not ids:
            return None
        g = dict(l.split("=", 1) for l in self.x11("xdotool", "getwindowgeometry", "--shell", ids[0]).stdout.split() if "=" in l)
        strut = self.x11("xprop", "-id", ids[0], "_NET_WM_STRUT_PARTIAL").stdout.strip()
        vals = [int(v) for v in strut.split("=")[1].split(",")] if "=" in strut else []
        return {"x": int(g["X"]), "y": int(g["Y"]), "w": int(g["WIDTH"]), "h": int(g["HEIGHT"]), "strut": vals}

    def wait(self, fn, seconds=6.0):
        end = time.monotonic() + seconds
        value = None
        while time.monotonic() < end:
            value = fn()
            if value:
                return value
            time.sleep(0.1)
        return value


def pixels(im):
    """The image's pixels as a flat sequence (Pillow >= 12 has get_flattened_data; getdata is deprecated there)."""
    flat = getattr(im, "get_flattened_data", None)
    return list(flat() if flat else im.getdata())


def report(path):
    """The DECBAR_DEBUG_PANEL report, only when complete (hosts rewrite it in place; `hover` is its last line)."""
    d = {"targets": []}
    try:
        lines = Path(path).read_text().splitlines()
    except OSError:
        return None
    for l in lines:
        p = l.split()
        if not p:
            continue
        if p[0] == "panel":
            d["panel"] = tuple(float(v) for v in p[1:5])
        elif p[0] == "scroll":
            d["scroll"] = float(p[1])
        elif p[0] == "zoom":
            d["zoom"], d["width"] = float(p[1]), float(p[3])
        elif p[0] == "target":
            d["targets"].append((p[1], *(float(v) for v in p[2:6])))
        elif p[0] in ("focus", "hover"):
            d[p[0]] = p[1]
    return d if "panel" in d and "hover" in d else None


def diff_bbox(a, b, mask_rect=None):
    from PIL import Image, ImageChops
    m = ImageChops.difference(Image.open(a).convert("RGB"), Image.open(b).convert("RGB")).convert("L").point(lambda v: 255 if v > 24 else 0)
    if mask_rect:
        m.paste(0, tuple(int(v) for v in mask_rect))
    return m.getbbox()


def ring(before, focus, after, strip):
    """The keyboard ring: among the colours keyboard mode changed most in the strip, one that gains pixels in keyboard mode
    and loses them again after Escape (content that re-fits meanwhile changes colours that do not come back)."""
    from PIL import Image, ImageChops
    from collections import Counter
    a, b, c = (Image.open(p).convert("RGB").crop(tuple(int(v) for v in strip)) for p in (before, focus, after))
    m = ImageChops.difference(a, b).convert("L").point(lambda v: 255 if v > 24 else 0)
    changed = [px for px, k in zip(pixels(b), pixels(m)) if k]
    if not changed:
        return {"shown_and_cleared": False}
    ca, cb, cc = Counter(pixels(a)), Counter(pixels(b)), Counter(pixels(c))
    rows = [{"rgb": col, "before": ca.get(col, 0), "focus": cb.get(col, 0), "after": cc.get(col, 0)} for col, _ in Counter(changed).most_common(6)]
    for r in rows:
        if r["focus"] >= r["before"] + 20 and r["after"] <= r["before"] + 8:
            return {**r, "shown_and_cleared": True}
    return {**rows[0], "shown_and_cleared": False}


def ring_box(before, focus, rgb, strip):
    """Bounding box (screen coordinates) of the pixels that take the ring's colour in keyboard mode inside `strip`."""
    from PIL import Image
    x0, y0, x1, y1 = (int(v) for v in strip)
    a, b = (Image.open(p).convert("RGB").crop((x0, y0, x1, y1)) for p in (before, focus))
    w = x1 - x0
    rgb = tuple(rgb)
    pts = [(i % w, i // w) for i, (pa, pb) in enumerate(zip(pixels(a), pixels(b))) if pb == rgb and pa != rgb]
    if not pts:
        return None
    xs, ys = [p[0] for p in pts], [p[1] for p in pts]
    return (x0 + min(xs), y0 + min(ys), x0 + max(xs) + 1, y0 + max(ys) + 1)


def strip_of(edge, win, thickness, W, H):
    return {"top": (0, win["y"], W, win["y"] + thickness), "bottom": (0, win["y"] + win["h"] - thickness, W, win["y"] + win["h"]),
            "left": (win["x"], 0, win["x"] + thickness, H), "right": (win["x"] + win["w"] - thickness, 0, win["x"] + win["w"], H)}[edge]


def touches(edge, win, W, H):
    return {"top": win["y"] == 0, "bottom": win["y"] + win["h"] == H, "left": win["x"] == 0, "right": win["x"] + win["w"] == W}[edge]


def summon(c, bins, edge, strip, scale):
    """`decbar --control-center open` beside the panel: its reported panel inside the output and clear of the strip."""
    own = c.dir / "overlay-panel.txt"
    p = c.spawn([str(bins / "decbar"), "--control-center", "open"], "overlay", DECBAR_DEBUG_PANEL=str(own))
    win = c.wait(lambda: c.window("decebal-control-center"))
    r = c.wait(lambda: report(own))
    time.sleep(0.4)
    # the window as it is once the panel is laid out (it resizes for the panel after it first maps)
    win = c.window("decebal-control-center") or win
    r = report(own) or r
    c.shot("summoned.png")
    c.stop(p)
    if not (win and r):
        return {"ok": False, "error": "no overlay or report"}
    x, y, w, h = r["panel"]
    k = scale or 1.0
    x0, y0, x1, y1 = win["x"] + x * k, win["y"] + y * k, win["x"] + (x + w) * k, win["y"] + (y + h) * k
    W, H = c.size
    gap = {"top": y0 - strip[3], "bottom": strip[1] - y1, "left": x0 - strip[2], "right": strip[0] - x1}[edge]
    return {"panel": [x0, y0, x1, y1], "gap": gap, "ok": x0 >= -0.5 and y0 >= -0.5 and x1 <= W + 0.5 and y1 <= H + 0.5 and gap >= 1}


def luminance(rgb):
    return (0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]) / 255.0


def taskbar_case(out, bins, edge, scale, mode="dark"):
    W, H = 1600, 900
    name = f"taskbar-{edge}-{scale}-{mode}"
    rec = {"case": name, "kind": "taskbar", "edge": edge, "scale": scale, "mode": mode}
    with Case(out, name, W, H, scale=scale) as c:
        c.config({"theme": {"name": "windows11", "mode": mode}, "bar": {"enabled": False}, "system_theme": {"automatic": False}}, {"style": "windows", "edge": edge})
        c.spawn([str(bins / "decdock")], "decdock")
        win = c.wait(lambda: c.window("decebal-dock"))
        lay = c.wait(lambda: (c.dir / "layout.txt").exists() and (c.dir / "layout.txt").read_text())
        rec["window"] = win
        if not (win and lay):
            return {**rec, "ok": False, "error": "no taskbar window or layout"}
        zones = [l.split() for l in lay.splitlines()]
        rec["zones"] = sorted({z[0] + ("" if z[0] != "tray" else ":" + z[1]) for z in zones if z})
        rec["touches_edge"] = touches(edge, win, W, H)
        thick = win["strut"][{"left": 0, "right": 1, "top": 2, "bottom": 3}[edge]] if win["strut"] else 0
        rec["strut"] = thick
        strip = strip_of(edge, win, thick, W, H)
        # work area as the strut gives it (no window manager here computes one): the strip's own thickness, along the whole edge
        span = {"left": (4, 5, H - 1), "right": (6, 7, H - 1), "top": (8, 9, W - 1), "bottom": (10, 11, W - 1)}[edge]
        across = win["w"] if edge in ("left", "right") else win["h"]
        rec["strut_matches_strip"] = bool(win["strut"]) and thick == across and win["strut"][span[0]] == 0 and win["strut"][span[1]] == span[2]
        # the palette: the strip's commonest colour, dark or light as the theme's mode says
        from PIL import Image
        from collections import Counter
        closed = c.shot("0-closed.png")
        common = Counter(pixels(Image.open(closed).convert("RGB").crop(tuple(int(v) for v in strip)))).most_common(1)[0][0]
        rec["strip_rgb"] = common
        rec["palette_matches_mode"] = luminance(common) < 0.35 if mode == "dark" else luminance(common) > 0.6
        ctl = [z for z in zones if z[:2] == ["tray", "Control"]]
        rec["has_start_tray_clock"] = any(z and z[0] == "start" for z in zones) and bool(ctl) and any(z and z[0] == "clock" for z in zones)
        # the zones' centres (screen coordinates), in the order the taskbar reads, each inside the strip and in that order along it
        vertical = edge in ("left", "right")
        centres = []
        for z in zones:
            if z and z[0] in ("start", "search", "clock", "desktop"):
                centres.append((z[0], win["x"] + float(z[1]), win["y"] + float(z[2])))
            elif z and z[0] in ("task", "tray"):
                centres.append((z[0] + ":" + z[1], win["x"] + float(z[2]), win["y"] + float(z[3])))
        rec["zone_centres"] = centres
        rank = {"start": 0, "search": 1, "task": 2, "tray": 3, "clock": 4, "desktop": 5}
        along = [c[2] if vertical else c[1] for c in centres]
        inside = all(strip[0] <= cx <= strip[2] and strip[1] <= cy <= strip[3] for _, cx, cy in centres)
        ordered = all(rank[centres[i][0].split(":")[0]] <= rank[centres[i + 1][0].split(":")[0]] and along[i] < along[i + 1] for i in range(len(centres) - 1))
        rec["zones_inside_and_ordered"] = bool(centres) and inside and ordered
        if ctl:
            cx, cy = win["x"] + float(ctl[0][2]), win["y"] + float(ctl[0][3])
            c.x11("xdotool", "mousemove", str(int(cx)), str(int(cy)))
            time.sleep(0.4)
            a = c.shot("1-before.png")
            c.x11("xdotool", "click", "1")
            time.sleep(1.0)
            b = c.shot("2-quick-settings.png")
            bb = diff_bbox(a, b, strip)
            rec["popup_bbox"] = bb
            rec["popup_on_desktop_side"] = bool(bb) and {"top": bb[1] >= strip[3] - 1, "bottom": bb[3] <= strip[1] + 1, "left": bb[0] >= strip[2] - 1, "right": bb[2] <= strip[0] + 1}[edge]
            # the quick-settings panel at its own size (a 1600 x 900 output has room for it at every scale tested)
            qs = c.wait(lambda: report(c.dir / "panel.txt"), 3.0)
            rec["quick_settings"] = qs and {"panel": qs["panel"], "zoom": qs.get("zoom"), "width": qs.get("width")}
            rec["quick_settings_full_size"] = bool(qs) and qs.get("zoom") == 1.0 and qs["panel"][2] >= 300
            if qs:
                # the panel where the dock says it drew it (its report, logical pixels of the dock's surface); every change
                # the screenshot shows outside the strip lies inside it
                wq = c.window("decebal-dock") or win
                k = scale or 1.0
                px, py, pw, ph = qs["panel"]
                pr = (wq["x"] + px * k, wq["y"] + py * k, wq["x"] + (px + pw) * k, wq["y"] + (py + ph) * k)
                rec["quick_settings_screen"] = pr
                off = {"top": pr[1] - strip[3], "bottom": strip[1] - pr[3], "left": pr[0] - strip[2], "right": strip[0] - pr[2]}[edge]
                within = bool(bb) and bb[0] >= pr[0] - 6 and bb[1] >= pr[1] - 6 and bb[2] <= pr[2] + 6 and bb[3] <= pr[3] + 6
                rec["quick_settings_placed"] = pr[0] >= -0.5 and pr[1] >= -0.5 and pr[2] <= W + 0.5 and pr[3] <= H + 0.5 and off >= 1 and within
            c.x11("xdotool", "key", "Escape")
            time.sleep(0.5)
        c.x11("xdotool", "mousemove", str(W // 2), str(H // 2))
        time.sleep(0.4)
        a = c.shot("3-before-focus.png")
        rec["focus_exit"] = c.run([str(bins / "decdock"), "--focus"]).returncode
        time.sleep(0.6)
        c.x11("xdotool", "key", "Down" if edge in ("left", "right") else "Right")
        time.sleep(0.5)
        b = c.shot("4-focus.png")
        c.x11("xdotool", "key", "Escape")
        time.sleep(0.5)
        d = c.shot("5-after-escape.png")
        rec["keyboard"] = ring(a, b, d, strip)
        # where the ring is: around the zone one arrow press focuses (the second in reading order) and around no other zone
        box = ring_box(a, b, rec["keyboard"]["rgb"], strip) if rec["keyboard"].get("shown_and_cleared") else None
        rec["ring_box"] = box
        if box and len(centres) > 1:
            inside = lambda cx, cy: box[0] <= cx <= box[2] and box[1] <= cy <= box[3]
            _, ex, ey = centres[1]
            rec["ring_expected"] = (ex, ey)
            rec["ring_on_focused_zone"] = inside(ex, ey) and not any(inside(cx, cy) for i, (_, cx, cy) in enumerate(centres) if i != 1)
        rec["summoned"] = summon(c, bins, edge, strip, scale)
    rec["ok"] = bool(rec.get("touches_edge") and rec.get("strut_matches_strip") and rec.get("palette_matches_mode") and rec.get("has_start_tray_clock")
                     and rec.get("zones_inside_and_ordered") and rec.get("popup_on_desktop_side") and rec.get("quick_settings_full_size")
                     and rec.get("quick_settings_placed") and rec.get("focus_exit") == 0 and rec["keyboard"].get("shown_and_cleared")
                     and rec.get("ring_on_focused_zone") and rec["summoned"].get("ok"))
    return rec


BAR_STYLES = [("macos", "menubar"), ("everforest", "floating"), ("caelestia", "rail"), ("dux-lavender", "dux")]


def bar_case(out, bins, theme, key, edge):
    W, H = 1600, 900
    name = f"bar-{key}-{edge}"
    rec = {"case": name, "kind": "bar", "style": key, "edge": edge}
    with Case(out, name, W, H) as c:
        c.config({"theme": {"name": theme, "mode": "dark"}, "bar": {"enabled": True, "edge": edge, "applets": KEYS}, "system_theme": {"automatic": False}}, {"style": "mac"})
        c.spawn([str(bins / "decbar")], "decbar")
        win = c.wait(lambda: c.window("decebal-bar"))
        tray = c.wait(lambda: (c.dir / "tray.txt").exists() and (c.dir / "tray.txt").read_text())
        rec["window"] = win
        if not (win and tray):
            return {**rec, "ok": False, "error": "no bar window or items"}
        items = [l.split() for l in tray.splitlines() if l.split()]
        rec["items"] = len(items)
        rec["touches_edge"] = touches(edge, win, W, H)
        thick = (win["strut"][{"left": 0, "right": 1, "top": 2, "bottom": 3}[edge]] if win["strut"] else 0) or 26
        strip = strip_of(edge, win, thick, W, H)
        ctl = [i for i in items if i[0] == "Control"]
        if ctl:
            cx, cy = win["x"] + float(ctl[0][1]), win["y"] + float(ctl[0][2] if len(ctl[0]) > 2 else 13)
            c.x11("xdotool", "mousemove", str(int(cx)), str(int(cy)))
            time.sleep(0.4)
            a = c.shot("1-before.png")
            c.x11("xdotool", "click", "1")
            time.sleep(1.0)
            b = c.shot("2-control-center.png")
            bb = diff_bbox(a, b, strip)
            rec["popup_bbox"] = bb
            rec["popup_on_desktop_side"] = bool(bb) and {"top": bb[3] > 120, "bottom": bb[1] < H - 120, "left": bb[2] > 120, "right": bb[0] < W - 120}[edge]
            c.x11("xdotool", "key", "Escape")
            time.sleep(0.5)
        a = c.shot("3-before-focus.png")
        rec["focus_exit"] = c.run([str(bins / "decbar"), "--focus"]).returncode
        time.sleep(0.6)
        c.x11("xdotool", "key", "Right")
        time.sleep(0.5)
        b = c.shot("4-focus.png")
        c.x11("xdotool", "key", "Escape")
        time.sleep(0.5)
        d = c.shot("5-after-escape.png")
        rec["keyboard"] = ring(a, b, d, strip)
    rec["ok"] = bool(rec.get("touches_edge") and rec.get("popup_on_desktop_side") and rec.get("focus_exit") == 0 and rec["keyboard"].get("shown_and_cleared"))
    return rec


CC_THEME = {"card": "macos", "fluent": "windows11", "dux": "dux-lavender", "gnome": "gnome-adwaita"}


def cc_case(out, bins, host, style, W, H, scale, text):
    name = f"cc-{host}-{style}-{W}x{H}-d{scale}-t{text}"
    rec = {"case": name, "kind": "cc", "host": host, "style": style, "output": [W, H], "scale": scale, "text": text}
    with Case(out, name, W, H, scale=scale, text=text) as c:
        taskbar = host.startswith("taskbar")
        edge = host.split("-")[1] if "-" in host else "top"
        theme = "caelestia" if host.startswith("rail") else CC_THEME[style]
        c.config({"theme": {"name": theme, "mode": "dark"}, "bar": {"enabled": host.startswith(("menubar", "rail")), "edge": edge}, "dock": {"enabled": taskbar},
                  "control_center": {"enabled": True, "position": "auto", "style": style}, "system_theme": {"automatic": False}},
                 {"style": "windows" if taskbar else "mac", "edge": edge if taskbar else "bottom"})
        if host == "overlay":
            c.spawn([str(bins / "decbar"), "--control-center", "open"], "overlay")
            win = c.wait(lambda: c.window("decebal-control-center"))
        else:
            prog = "decdock" if taskbar else "decbar"
            c.spawn([str(bins / prog)], prog)
            name_ = "decebal-dock" if taskbar else "decebal-bar"
            win = c.wait(lambda: c.window(name_))
            if not win:
                return {**rec, "ok": False, "error": "no host window"}
            if taskbar:
                lay = c.wait(lambda: (c.dir / "layout.txt").exists() and (c.dir / "layout.txt").read_text())
                z = [l.split() for l in (lay or "").splitlines() if l.startswith("tray Control")]
                pos = (win["x"] + float(z[0][2]), win["y"] + float(z[0][3])) if z else None
            else:
                tray = c.wait(lambda: (c.dir / "tray.txt").exists() and (c.dir / "tray.txt").read_text())
                z = [l.split() for l in (tray or "").splitlines() if l.startswith("Control ")]
                # a horizontal bar's lines give x only (its strip is 26 px tall), a rail's x and y
                pos = (win["x"] + float(z[0][1]) * scale, win["y"] + (float(z[0][2]) if len(z[0]) > 2 else 13.0) * scale) if z else None
            if not pos:
                return {**rec, "ok": False, "error": "no Control Center button"}
            c.x11("xdotool", "mousemove", str(int(pos[0])), str(int(pos[1])))
            time.sleep(0.3)
            c.x11("xdotool", "click", "1")
            win = c.wait(lambda: c.window(name_))
        r0 = c.wait(lambda: report(c.dir / "panel.txt"))
        time.sleep(0.4)
        # the window and the report as they are once the panel is laid out (surfaces resize for it after they map)
        win = c.window("decebal-control-center" if host == "overlay" else ("decebal-dock" if taskbar else "decebal-bar")) or win
        r0 = report(c.dir / "panel.txt") or r0
        if not (win and r0):
            return {**rec, "ok": False, "error": "no panel report"}
        c.shot("1-open.png")
        root = lambda px, py: (win["x"] + px * scale, win["y"] + py * scale)
        px, py, pw, ph = r0["panel"]
        (x0, y0), (x1, y1) = root(px, py), root(px + pw, py + ph)
        rec["panel"], rec["zoom"], rec["content_width"] = [x0, y0, x1, y1], r0.get("zoom"), r0.get("width")
        rec["inside_output"] = x0 >= -0.5 and y0 >= -0.5 and x1 <= W + 0.5 and y1 <= H + 0.5
        order = [t[0] for t in r0["targets"]]
        rec["controls"] = len(order)
        seen, revealed = [], True
        for want in order:
            c.x11("xdotool", "key", "Down")
            r = c.wait(lambda: (lambda d: d if d and d.get("focus") == want else None)(report(c.dir / "panel.txt")), 2.0)
            seen.append(r.get("focus") if r else None)
            if r:
                t = [t for t in r["targets"] if t[0] == want][0]
                revealed &= t[2] >= r["panel"][1] - 1 and t[2] + t[4] <= r["panel"][1] + r["panel"][3] + 1
        rec["keyboard_order"], rec["keyboard_revealed"] = seen == order, revealed
        rec["order"], rec["walked"] = order, seen
        last = c.wait(lambda: report(c.dir / "panel.txt"))
        c.shot("2-walked.png")
        t = [t for t in last["targets"] if t[0] == order[-1]][0]
        c.x11("xdotool", "mousemove", *(str(int(v)) for v in root(t[1] + t[3] / 2, t[2] + t[4] / 2)))
        rec["pointer_last"] = bool(c.wait(lambda: (lambda d: d and d.get("hover") == order[-1])(report(c.dir / "panel.txt")), 2.0))
        c.x11("xdotool", "mousemove", *(str(int(v)) for v in root(px + 4, last["panel"][1] + 4)))
        time.sleep(0.2)
        for _ in range(40):
            c.x11("xdotool", "click", "4")
        top = c.wait(lambda: (lambda d: d if d and d.get("scroll") == 0.0 else None)(report(c.dir / "panel.txt")), 3.0)
        rec["wheel_to_top"] = bool(top)
        if top:
            t = [t for t in top["targets"] if t[0] == order[0]][0]
            c.x11("xdotool", "mousemove", *(str(int(v)) for v in root(t[1] + t[3] / 2, t[2] + t[4] / 2)))
        rec["pointer_first"] = bool(top and c.wait(lambda: (lambda d: d and d.get("hover") == order[0])(report(c.dir / "panel.txt")), 2.0))
        c.shot("3-pointer.png")
    rec["ok"] = all(rec.get(k) for k in ["inside_output", "keyboard_order", "keyboard_revealed", "pointer_last", "wheel_to_top", "pointer_first"]) and rec["controls"] >= 5
    return rec


def probe_case(out, bins, name, first, then=None, wait=70):
    rec = {"case": name, "kind": "probe", "first": len(first), "then": None if then is None else len(then)}
    with Case(out, name, 1600, 900) as c:
        log = c.dir / "probe-calls.txt"
        for tool in PROBE_TOOLS:
            path = c.dir / "bin" / tool
            path.write_text(f"#!/bin/sh\necho \"$(/usr/bin/date +%s.%N) {tool} $*\" >> '{log}'\nexit 1\n")
            path.chmod(0o755)
        shell = lambda a: {"theme": {"name": "macos", "mode": "dark"}, "bar": {"enabled": True, "edge": "top", "applets": a}, "system_theme": {"automatic": False}}
        c.config(shell(first), {"style": "mac"})
        p = c.spawn([str(bins / "decbar")], "decbar")

        def calls(since=0.0):
            if not log.exists():
                return {t: 0 for t in PROBE_TOOLS}
            rows = [l.split() for l in log.read_text().splitlines() if l.split()]
            return {t: sum(1 for r in rows if r[1] == t and float(r[0]) >= since) for t in PROBE_TOOLS}

        if then is None:
            time.sleep(wait)
            rec["calls"] = calls()
        else:
            time.sleep(8)
            rec["calls_before_switch"] = calls()
            c.config(shell(then), {"style": "mac"})
            time.sleep(5)
            settle = time.time()
            time.sleep(wait)
            rec["calls_after_switch"] = calls(settle)
        rec["alive"] = p.poll() is None
    if then is not None:
        rec["ok"] = rec["alive"] and sum(rec["calls_before_switch"].values()) > 0 and sum(rec["calls_after_switch"].values()) == 0
    elif name.endswith("clock-only"):
        rec["ok"] = rec["alive"] and sum(rec["calls"].values()) == 0
    else:
        rec["ok"] = rec["alive"] and all(rec["calls"][t] > 0 for t in ("nmcli", "bluetoothctl", "brightnessctl")) and rec["calls"]["pactl"] + rec["calls"]["wpctl"] > 0
    return rec


def plan(kinds, quick):
    jobs = []
    if "taskbar" in kinds:
        for edge in ["top", "bottom", "left", "right"]:
            for scale in ([1.25] if quick else [1.0, 1.25, 2.0]):
                jobs.append((f"taskbar-{edge}-{scale}-dark", lambda out, bins, e=edge, s=scale: taskbar_case(out, bins, e, s, "dark")))
            jobs.append((f"taskbar-{edge}-1.0-light", lambda out, bins, e=edge: taskbar_case(out, bins, e, 1.0, "light")))
    if "bar" in kinds:
        for theme, key in BAR_STYLES:
            for edge in (["bottom", "left"] if quick else ["top", "bottom", "left", "right"]):
                jobs.append((f"bar-{key}-{edge}", lambda out, bins, t=theme, k=key, e=edge: bar_case(out, bins, t, k, e)))
    if "cc" in kinds:
        combos = [("overlay", "gnome", 1024, 480, 2.0, 2.0), ("rail-left", "dux", 1024, 480, 2.0, 2.0), ("taskbar-bottom", "card", 1024, 480, 2.0, 2.0),
                  ("taskbar-left", "fluent", 1024, 480, 2.0, 2.0), ("menubar-bottom", "gnome", 1280, 520, 1.25, 1.25), ("menubar-top", "dux", 1024, 480, 2.0, 2.0)]
        if not quick:
            combos += [("overlay", st, w, h, d, t) for st in ["card", "fluent", "dux", "gnome"] for (w, h) in [(1280, 520), (1024, 480)] for d in [1.0, 2.0] for t in [1.0, 2.0]]
        for host, style, w, h, d, t in combos:
            jobs.append((f"cc-{host}-{style}-{w}x{h}-d{d}-t{t}", lambda out, bins, a=(host, style, w, h, d, t): cc_case(out, bins, *a)))
    if "probe" in kinds:
        jobs.append(("probe-clock-only", lambda out, bins: probe_case(out, bins, "probe-clock-only", ["clock"])))
        jobs.append(("probe-all", lambda out, bins: probe_case(out, bins, "probe-all", KEYS)))
        jobs.append(("probe-all-then-clock-only", lambda out, bins: probe_case(out, bins, "probe-all-then-clock-only", KEYS, ["clock"])))
    return jobs


def main():
    global REPO
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--bin", "--bin-dir", dest="bin", required=True, help="directory with the pinned decbar and decdock (read only)")
    ap.add_argument("--repo", default=str(REPO), help="checkout providing themes/ and the recorded source commit")
    ap.add_argument("--out", required=True, help="new output directory")
    ap.add_argument("--cases", default="taskbar,bar,cc,probe")
    ap.add_argument("--quick", action="store_true", help="a smaller subset of each selected kind")
    ap.add_argument("--only", default="", help="comma-separated substrings: run only the cases whose names contain one")
    a = ap.parse_args()
    REPO = Path(a.repo).resolve()
    bins, out = Path(a.bin).resolve(), Path(a.out).resolve()
    kinds = {k.strip() for k in a.cases.split(",") if k.strip()}
    try:
        if not (REPO / "themes" / "windows11" / "theme.toml").is_file():
            raise RuntimeError(f"{REPO} is not a Decebal checkout with themes/ (give --repo)")
        if not kinds <= {"taskbar", "bar", "cc", "probe"}:
            raise RuntimeError(f"unknown case kinds: {sorted(kinds - {'taskbar', 'bar', 'cc', 'probe'})}")
        for b in ("decbar", "decdock"):
            if not os.access(bins / b, os.X_OK):
                raise RuntimeError(f"{bins / b} is not an executable")
        missing = [t for t in sorted(HOST_TOOLS | {"Xvfb", "dbus-daemon"}) if not shutil.which(t)]
        if missing:
            raise RuntimeError(f"missing tools: {missing}")
        if out.exists():
            raise RuntimeError(f"{out} exists: give a new directory")
        out.mkdir(mode=0o700, parents=True)
    except (RuntimeError, OSError) as e:
        print(f"setup: {e}", file=sys.stderr)
        return 2
    src = subprocess.run(["git", "-C", str(REPO), "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip()
    dirty = bool(subprocess.run(["git", "-C", str(REPO), "status", "--porcelain", "--", "src", "themes", "tests/e2e"], capture_output=True, text=True).stdout.strip())
    themes = hashlib.sha256()
    for f in sorted((REPO / "themes").rglob("theme.toml")):
        themes.update(f.relative_to(REPO).as_posix().encode() + b"\0" + f.read_bytes())
    manifest = {
        "source": src, "source_dirty": dirty, "repo": str(REPO),
        "binaries": {b: sha(bins / b) for b in ("decbar", "decdock")}, "bin_dir": str(bins),
        "harness": {p.name: sha(p) for p in (Path(__file__).resolve(), Path(private_x11.__file__).resolve())},
        "themes_manifests_sha256": themes.hexdigest(), "cases_selected": sorted(kinds), "quick": a.quick,
        "limits": {
            "binary_provenance": "sha256 of the binaries only: the harness cannot tell which source built them",
            "source": "the --repo checkout's commit and dirty flag; its themes/ is what the cases load",
            "work_area": "strut publication only: no window manager runs in the fixture, so no work area is computed or honoured",
            "display": "private Xvfb without compositor or window manager (X11 only; Wayland not exercised)",
        },
        "started_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "python": sys.version.split()[0],
    }
    results = []
    only = [o for o in a.only.split(",") if o]
    for name, job in [j for j in plan(kinds, a.quick) if not only or any(o in j[0] for o in only)]:
        try:
            r = job(out, bins)
        except Exception as e:  # a case that breaks is a failed case, with its reason
            r = {"case": name, "ok": False, "error": repr(e)}
        results.append(r)
        print(("PASS " if r.get("ok") else "FAIL ") + r.get("case", "?") + ("" if r.get("ok") else f"  {r.get('error', '')}"), flush=True)
        write_json(out / "results.json", results, indent=1, default=str)
    passed = sum(1 for r in results if r.get("ok"))
    manifest.update({"finished_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "passed": passed, "total": len(results),
                     "failed": [r.get("case") for r in results if not r.get("ok")]})
    write_json(out / "manifest.json", manifest, indent=1)
    with open(out / "SHA256SUMS", "w") as f:
        for p in sorted(out.rglob("*")):
            if p.is_file() and p.name != "SHA256SUMS":
                f.write(f"{sha(p)}  {p.relative_to(out).as_posix()}\n")
    print(f"{passed} of {len(results)} cases pass")
    return 0 if results and passed == len(results) else 1


if __name__ == "__main__":
    sys.exit(main())
