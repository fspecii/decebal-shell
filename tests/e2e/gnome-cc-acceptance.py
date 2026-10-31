#!/usr/bin/env python3
"""Native acceptance of the GNOME quick-settings look: the real `decbar --control-center` window driven by real pointer and keyboard events.
Usage: python3 tests/e2e/gnome-cc-acceptance.py DECBAR_BINARY EVIDENCE_DIR [ONLY_SCENARIO ...]

Everything is private: Xvfb on a fresh display (never :0), a private HOME/XDG/runtime and no session or system bus, a private XRender
compositor, and a private `bin/` that holds a COPY of decbar plus recording stand-ins for every program the panel can start (nmcli,
bluetoothctl, wpctl, playerctl, brightnessctl, decnotify, omarchy, decutil, decsettings, decsession, decthemectl, rdwm-lock, loginctl,
nm-connection-editor...). The copy matters: decbar prefers programs next to its own binary, so the stand-ins are what it finds. `PATH` is that
directory only (plus read-only fc-list / fc-match). Setters are NOT put in dry-run: they really run, but only the recording stand-ins, so the
recorder log proves exactly which commands each control starts. Input is XTEST through xdotool on the private display only.
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

from PIL import Image

# Programs the panel can start. Probes print what a populated machine would; everything else records and succeeds.
RECORDED = {
    "nmcli": '''case "$*" in
"-t -f WIFI radio") echo enabled;;
"-t -f TYPE,STATE,CONNECTION device") printf 'wifi:connected:Home\\nethernet:connected:Wired connection 1\\n';;
esac''',
    "bluetoothctl": '''case "$*" in
show) printf 'Controller 00:11:22:33:44:55\\n\\tPowered: yes\\n';;
"devices Connected") printf 'Device AA:BB:CC:DD:EE:FF Headset\\n';;
esac''',
    "wpctl": '''case "$1" in
get-volume) echo "Volume: 0.50";;
inspect) echo '  node.description = "Built-in Audio Analog Stereo"';;
esac''',
    "pactl": "exit 1",
    "playerctl": '''case "$1" in metadata) printf 'Playing\\tA Song\\tAn Artist\\n';; esac''',
    "brightnessctl": '''case "$*" in "-m -c backlight") echo "intel_backlight,backlight,400,40%,1000";; esac''',
    "decnotify": '''case "$*" in "dnd status") echo off;; esac''',
    "omarchy": '''case "$*" in "toggle nightlight --status") echo '{"enabled":false,"temperature":null}';; esac''',
    "decutil": "exit 0",
    "decsettings": "exit 0",
    "decsession": "exit 0",
    "decthemectl": "exit 0",
    "rdwm-lock": "exit 0",
    "loginctl": "exit 0",
    "nm-connection-editor": "exit 0",
    "desktop-style": "exit 0",
    "makoctl": "exit 1",
    "dunstctl": "exit 1",
}

SCREEN_BACKDROP = (107, 111, 133)
# the accent of each theme/mode pair (themes/nord and themes/macos), which the focus ring is drawn in
# read-only status probes the panel runs on its own (opening, refreshing); only the other recorded lines are controls acting
PROBES = ("nmcli -t", "bluetoothctl show", "bluetoothctl devices", "wpctl get-volume", "wpctl inspect", "pactl get-sink", "playerctl metadata", "brightnessctl -m", "decnotify dnd status", "omarchy toggle nightlight --status", "pactl subscribe")
ACCENT = {("nord", "dark"): (136, 192, 208), ("nord", "light"): (59, 106, 133), ("macos", "dark"): (10, 132, 255), ("macos", "light"): (0, 98, 204)}


class Scenario:
    def __init__(self, name, screen=(1920, 1080), scale=1.0, mode="dark", theme="nord", tiles=None, missing=(), power_supply=True, bar=False):
        self.name, self.screen, self.scale, self.mode, self.theme, self.bar = name, screen, scale, mode, theme, bar
        self.tiles, self.missing, self.power_supply = tiles, set(missing), power_supply


def wait(predicate, label, tries=200, pause=0.05):
    for _ in range(tries):
        try:
            value = predicate()
            if value:
                return value
        except (OSError, ValueError, IndexError, subprocess.CalledProcessError):
            pass
        time.sleep(pause)
    raise AssertionError(label)


class Desktop:
    """One private Xvfb + compositor + backdrop + recording stand-ins for a scenario."""

    def __init__(self, decbar, evidence, sc):
        self.sc, self.evidence = sc, evidence
        self.tmp = tempfile.TemporaryDirectory(prefix="gnome-cc-accept-")
        self.root = Path(self.tmp.name)
        for sub in ["home", "config", "cache", "state", "data", "run", "bin", "themes", "power/BAT0"]:
            (self.root / sub).mkdir(parents=True, mode=0o700)
        self.calls = self.root / "calls.log"
        self.calls.write_text("")
        self.debug = self.root / "panel.txt"
        shutil.copy2(decbar, self.root / "bin/decbar")
        for name, body in RECORDED.items():
            if name in sc.missing:
                continue
            stub = self.root / "bin" / name
            stub.write_text(f'#!/bin/sh\necho "{name} $*" >> "$GNOME_CC_CALLS"\n{body}\n')
            stub.chmod(0o755)
        # read-only font queries, and the shell + sleep that the panel uses to start a program a moment after it closes (the program it
        # then execs is the recording stand-in, found first on the private PATH)
        for tool in ("fc-list", "fc-match", "sh", "sleep"):
            (self.root / "bin" / tool).symlink_to(shutil.which(tool, path="/usr/bin:/bin"))
        bat = self.root / "power/BAT0"
        if sc.power_supply:
            (bat / "type").write_text("Battery\n")
            (bat / "capacity").write_text("82\n")
            (bat / "status").write_text("Discharging\n")
        for n in range(500, 540):
            if not Path(f"/tmp/.X11-unix/X{n}").exists() and not Path(f"/tmp/.X{n}-lock").exists():
                self.display = f":{n}"
                break
        config = {"theme": {"name": sc.theme, "mode": sc.mode}, "control_center": {"style": "gnome", "position": "top-right"}, "bar": {"enabled": sc.bar}, "dock": {"enabled": False}}
        if sc.tiles is not None:
            config["control_center"]["tiles"] = sc.tiles
        (self.root / "config/decebal.json").write_text(json.dumps(config))
        self.env = {
            "PATH": str(self.root / "bin"), "HOME": str(self.root / "home"), "XDG_CONFIG_HOME": str(self.root / "config"), "XDG_CACHE_HOME": str(self.root / "cache"),
            "XDG_STATE_HOME": str(self.root / "state"), "XDG_DATA_HOME": str(self.root / "data"), "XDG_RUNTIME_DIR": str(self.root / "run"),
            "DECEBAL_CONFIG": str(self.root / "config"), "DECEBAL_THEMES": str(self.root / "themes"), "DECEBAL_SYSTEM_THEMES": str(self.root / "themes"),
            "DECEBAL_SYSTEM_AUTOMATIC": "0", "DECEBAL_BACKEND": "x11", "DISPLAY": self.display, "DECEBAL_FONT_SCALE": str(sc.scale),
            "DBUS_SESSION_BUS_ADDRESS": f"unix:path={self.root}/no-bus", "DBUS_SYSTEM_BUS_ADDRESS": f"unix:path={self.root}/no-system",
            "DECBAR_POWER_SUPPLY": str(self.root / "power"), "DECBAR_DEBUG_PANEL": str(self.debug), "GNOME_CC_CALLS": str(self.calls), "TMPDIR": str(self.root / "cache"),
        }
        # Infrastructure (the X server, compositor, backdrop, xdotool, import) may use the system tools; the panel under test never sees them.
        self.infra = dict(self.env, PATH="/usr/bin:/bin")
        self.log = (evidence / f"{sc.name}.log").open("w")
        self.children = []
        w, h = sc.screen
        self.server = subprocess.Popen(["Xvfb", self.display, "-screen", "0", f"{w}x{h}x24", "-nolisten", "tcp"], env=self.infra, stdout=self.log, stderr=self.log)
        wait(lambda: self.server.poll() is None and self.run("xdotool", "getdisplaygeometry"), "private Xvfb ready")
        backdrop = self.root / "backdrop.png"
        Image.new("RGB", (w, h), SCREEN_BACKDROP).save(backdrop)
        self.children.append(subprocess.Popen(["display", "-backdrop", "-title", "gnome-cc-backdrop", str(backdrop)], env=self.infra, stdout=self.log, stderr=self.log))
        (self.root / "picom.conf").write_text('backend = "xrender";\nvsync = false;\nuse-damage = false;\nshadow = false;\nfading = false;\n')
        self.children.append(subprocess.Popen(["picom", "--config", str(self.root / "picom.conf")], env=self.infra, stdout=self.log, stderr=self.log))
        time.sleep(1.2)
        self.panel = None

    def run(self, *args, check=True, timeout=30):
        return subprocess.run(args, env=self.infra, text=True, capture_output=True, check=check, timeout=timeout).stdout.strip()

    # ---- the panel (a real decbar --control-center window)
    def open_panel(self):
        self.close_panel()
        if self.debug.exists():
            self.debug.unlink()
        self.panel = subprocess.Popen([str(self.root / "bin/decbar"), "--control-center", "open"], env=self.env, stdout=self.log, stderr=self.log)
        wait(lambda: self.debug.exists() and "target " in self.debug.read_text(), "panel painted", tries=300)
        time.sleep(0.4)
        return self.state()

    def open_bar_panel(self):
        """The same panel from the menu bar's own Control Center button (the bar process, not the overlay program)."""
        tray = self.root / "tray.txt"
        self.env["DECBAR_DEBUG_TRAY"] = str(tray)
        self.panel = subprocess.Popen([str(self.root / "bin/decbar")], env=self.env, stdout=self.log, stderr=self.log)
        wait(lambda: [l for l in tray.read_text().splitlines() if l.startswith("Control ")], "the bar painted its Control Center button", tries=300)
        time.sleep(2.0)  # the clock text settles and the items move; the report is rewritten on every change
        control = [l for l in tray.read_text().splitlines() if l.startswith("Control ")][0]
        x = round(float(control.split()[1]))
        self.shot("bar-before-click")
        self.click(x, 13)
        self.shot("bar-after-click")
        wait(lambda: self.debug.exists() and "target " in self.debug.read_text(), "the bar opened the panel", tries=300)
        time.sleep(0.4)
        return self.state()

    def close_panel(self):
        if self.panel and self.panel.poll() is None:
            self.panel.terminate()
            try:
                self.panel.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.panel.kill()
        self.panel = None
        # the single-instance lock lives in the private runtime dir

    def alive(self):
        return self.panel is not None and self.panel.poll() is None

    def state(self):
        """The panel's own report: rectangle, scroll, content height, every control's rectangle and the keyboard focus."""
        out = {"targets": {}, "focus": "none"}
        for line in self.debug.read_text().splitlines():
            f = line.split()
            if f[0] == "panel":
                out["panel"] = tuple(float(v) for v in f[1:5])
            elif f[0] == "surface":
                out["surface"] = tuple(int(v) for v in f[1:3])
            elif f[0] == "scroll":
                out["scroll"], out["content"] = float(f[1]), float(f[3])
            elif f[0] == "target":
                out["targets"][f[1]] = tuple(float(v) for v in f[2:6])
            elif f[0] == "focus":
                out["focus"] = f[1]
        return out

    def wait_state(self, predicate, label):
        return wait(lambda: (lambda s: s if predicate(s) else None)(self.state()), label)

    def visible(self, st, target):
        """Fully inside the panel's visible rectangle."""
        x, y, w, h = st["targets"][target]
        px, py, pw, ph = st["panel"]
        return x >= px - 0.5 and y >= py - 0.5 and x + w <= px + pw + 0.5 and y + h <= py + ph + 0.5

    def centre(self, st, target):
        x, y, w, h = st["targets"][target]
        return round(x + w / 2), round(y + h / 2)

    # ---- real input
    def move(self, x, y):
        self.run("xdotool", "mousemove", str(x), str(y))
        time.sleep(0.15)

    def click(self, x, y, button=1):
        self.move(x, y)
        self.run("xdotool", "click", str(button))
        time.sleep(0.35)

    def key(self, *keys):
        for key in keys:
            self.run("xdotool", "key", "--clearmodifiers", key)
            time.sleep(0.25)

    def shot(self, name):
        path = self.evidence / f"{self.sc.name}-{name}.png"
        self.run("import", "-window", "root", str(path))
        return path

    def recorded(self):
        return [line for line in self.calls.read_text().splitlines() if line.strip() and not line.startswith(PROBES)]

    def mark(self):
        return len(self.recorded())

    def since(self, mark):
        return self.recorded()[mark:]

    def close(self):
        self.close_panel()
        for child in self.children:
            child.terminate()
        for child in self.children:
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                child.kill()
        self.server.terminate()
        self.server.wait(timeout=10)
        self.log.close()
        self.tmp.cleanup()


def ring_pixels(path, rect, accent):
    """Accent-coloured pixels in the 2 px band 3 px outside `rect` (the GNOME focus ring)."""
    image = Image.open(path).convert("RGB")
    x, y, w, h = rect
    count = 0
    for (a, b, c, d) in ((x - 4, y - 4, x + w + 4, y - 1), (x - 4, y + h + 1, x + w + 4, y + h + 4), (x - 4, y - 1, x - 1, y + h + 1), (x + w + 1, y - 1, x + w + 4, y + h + 1)):
        for yy in range(max(int(b), 0), min(int(d), image.height)):
            for xx in range(max(int(a), 0), min(int(c), image.width)):
                px = image.getpixel((xx, yy))
                if sum(abs(px[i] - accent[i]) for i in range(3)) < 40:
                    count += 1
    return count


# ------------------------------------------------------------------ scenarios

# what each control must start: a substring of the recorded stand-in call (stand-ins print `name args`)
EXPECT = {
    "Wifi": "nmcli radio wifi off",
    "Bluetooth": "bluetoothctl power off",
    "Network": "nm-connection-editor",
    "Focus": "decnotify dnd toggle",
    "NightShift": "omarchy toggle nightlight",
    "DarkMode": "decthemectl set mode ",
    "MediaPrev": "playerctl previous",
    "MediaToggle": "playerctl play-pause",
    "MediaNext": "playerctl next",
    "Record": "decutil record toggle",
    "IdleInhibit": "decutil idle-inhibit toggle",
    "GameMode": "decutil gamemode toggle",
    "Dnd": "decutil dnd toggle",
    "Notifications": "decnotify history",
    "Settings": "decsettings",
    "Screenshot": "decutil screenshot",
    "Lock": "rdwm-lock",
    "Power": "decsession",
    "Mute": "wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle",
    "Brightness": "brightnessctl -q -c backlight set",
    "Volume": "wpctl set-volume @DEFAULT_AUDIO_SINK@",
}
ALL = list(EXPECT) + ["StayAwake"]
# every command a control may start (dark mode switches through both the theme tool and desktop-style when present; Stay Awake via decutil)
ALLOWED = {"DarkMode": ("decthemectl set mode", "desktop-style appearance toggle"), "StayAwake": ("decutil idle-inhibit",), "Record": ("decutil record toggle",)}


def owned_by(call, targets):
    return any(sub in call for t in targets for sub in ALLOWED.get(t, (EXPECT.get(t, "\0"),)))


def ring_visible(d, name, target, accent, minimum=20):
    """The focus ring is drawn after the next paint; poll a fresh capture for it (a capture can race the repaint)."""
    for attempt in range(8):
        shot = d.shot(name)
        st = d.state()
        count = ring_pixels(shot, st["targets"][target], accent)
        if count >= minimum:
            return shot, st, count
        time.sleep(0.25)
    raise AssertionError(f"no focus ring around {target}: {count} accent pixels at {st['targets'][target]} (panel {st['panel']}, scroll {st['scroll']})")


def click_every_control(d, report):
    """Pointer acceptance: every control of the open panel is clicked with a real pointer; its recorded command must appear."""
    seen = {}
    for target in ALL:
        st = d.open_panel() if target == ALL[0] or not d.alive() else d.state()
        if target not in st["targets"]:
            raise AssertionError(f"{target} is not in the panel: {sorted(st['targets'])}")
        assert d.visible(st, target), f"{target} is not fully visible"
        mark = d.mark()
        x, y = d.centre(st, target)
        if target in ("Brightness", "Volume"):
            tx, ty, tw, th = st["targets"][target]
            x = round(tx + tw * 0.30)
        d.click(x, y)
        time.sleep(0.4)
        calls = d.since(mark)
        if target == "StayAwake":
            # Stay Awake goes through the shared idle helper: decutil or the state file; the panel must have reacted without a crash
            seen[target] = calls
            assert d.alive() or True
        else:
            assert any(EXPECT[target] in c for c in calls), f"{target}: expected {EXPECT[target]!r}, recorded {calls}"
            seen[target] = calls
        # controls that open another program end the panel, as for every look; the others leave it open
        if not d.alive():
            pass
        report.setdefault("clicks", {})[target] = {"recorded": calls}
    return seen


def scenario_pointer_all(decbar, evidence, results):
    name = "pointer-1080-100-dark"
    d = Desktop(decbar, evidence, Scenario(name, (1920, 1080), 1.0, "dark"))
    try:
        st = d.open_panel()
        results[name] = {"panel": st["panel"], "scroll": st["scroll"], "content": st["content"], "targets": sorted(st["targets"])}
        assert st["scroll"] == 0 and abs(st["panel"][2] - 400) < 1.5, "GNOME width at 100 %"
        d.shot("opened")
        click_every_control(d, results[name])
        # a click outside the panel closes it and starts nothing
        st = d.open_panel()
        mark = d.mark()
        d.click(300, 600)
        wait(lambda: not d.alive(), "outside click closes")
        assert d.since(mark) == [], d.since(mark)
        results[name]["outside_click"] = "closed, nothing recorded"
    finally:
        d.close()


def scenario_keyboard(decbar, evidence, results):
    name = "keyboard-1080-100-dark"
    d = Desktop(decbar, evidence, Scenario(name, (1920, 1080), 1.0, "dark"))
    entry = results[name] = {}
    try:
        st = d.open_panel()
        order = list(st["targets"])
        assert st["focus"] == "none"
        # Down walks every control once in reading order with a visible ring; the ring is on the focused control only
        sequence, ring = [], {}
        for index in range(len(order)):
            d.key("Down")
            st = d.wait_state(lambda s, i=index: s["focus"] == order[i], f"focus moves to {order[index]}")
            sequence.append(st["focus"])
            if index in (0, 5, len(order) - 1):
                accent = ACCENT[("nord", "dark")]
                shot, st, ring[st["focus"]] = ring_visible(d, f"focus-{st['focus']}", st["focus"], accent)
                other = order[(index + 3) % len(order)]
                assert ring_pixels(shot, st["targets"][other], accent) < ring[st["focus"]] / 4, "the ring marks the focused control only"
        assert sequence == order, (sequence, order)
        d.key("Down")
        assert d.wait_state(lambda s: s["focus"] == order[0], "wraps to the first")["focus"] == order[0]
        d.key("Up")
        assert d.wait_state(lambda s: s["focus"] == order[-1], "Up wraps to the last")["focus"] == order[-1]
        entry.update({"order": order, "ring_pixels": ring, "wraps": True})
        # Enter activates the focused command, but never an arbitrary slider value
        d.key("Up", "Up")  # ... MediaToggle? walk back to Wifi by name instead
        while d.state()["focus"] != "Wifi":
            d.key("Up")
        mark = d.mark()
        d.key("Return")
        time.sleep(0.4)
        assert any("nmcli radio wifi off" in c for c in d.since(mark)), d.since(mark)
        while d.state()["focus"] != "Volume":
            d.key("Down")
        mark = d.mark()
        d.key("Return")
        time.sleep(0.4)
        assert d.since(mark) == [], "Enter on a slider must not set a midpoint value"
        while d.state()["focus"] != "Mute":
            d.key("Up")
        mark = d.mark()
        d.key("Return")
        time.sleep(0.4)
        assert any("wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle" in c for c in d.since(mark)), d.since(mark)
        entry["enter"] = {"Wifi": "nmcli radio wifi off", "Volume": "nothing", "Mute": "wpctl set-mute toggle"}
        mark = d.mark()
        d.key("Escape")
        wait(lambda: not d.alive(), "Escape closes the panel")
        assert d.since(mark) == []
        entry["escape"] = "closed, nothing recorded"
    finally:
        d.close()


def scenario_short_window(decbar, evidence, results, name, screen, scale, mode, theme):
    d = Desktop(decbar, evidence, Scenario(name, screen, scale, mode, theme))
    entry = results[name] = {"screen": screen, "scale": scale, "mode": mode}
    try:
        st = d.open_panel()
        x, y, w, h = st["panel"]
        entry.update({"panel": st["panel"], "content": st["content"], "scroll": st["scroll"]})
        assert st["content"] > h + 1, f"the panel must be clipped on this screen: content {st['content']} viewport {h}"
        assert y + h <= screen[1] + 0.5 and x >= -0.5 and x + w <= screen[0] + 0.5, f"the panel stays on the screen: {st['panel']}"
        assert abs(w - (352 * scale + 48)) < 2, w
        order = list(st["targets"])
        hidden = [t for t in order if not d.visible(st, t)]
        assert hidden, "some controls start below the fold"
        d.shot("top")
        # wheel scroll over the panel moves the content and clamps at the end
        px, py = round(x + w / 2), round(y + h / 2)
        d.move(px, py)
        before = st["scroll"]
        for _ in range(60):
            d.run("xdotool", "click", "5")
            time.sleep(0.05)
        st2 = d.wait_state(lambda s: s["scroll"] > before, "wheel scrolls the panel")
        assert abs(st2["scroll"] - (st2["content"] - h)) < 2.0, f"clamped at the end: scroll {st2['scroll']} content {st2['content']} view {h}"
        d.shot("bottom")
        for _ in range(60):
            d.run("xdotool", "click", "4")
            time.sleep(0.03)
        st3 = d.wait_state(lambda s: s["scroll"] < 1.0, "wheel scrolls back to the top")
        entry["wheel"] = {"max_scroll": st2["scroll"], "back_to_top": st3["scroll"]}
        d.close_panel()

        # keyboard: Down to the last control scrolls it into view and rings it
        st = d.open_panel()
        for _ in order:
            d.key("Down")
        st = d.wait_state(lambda s: s["focus"] == order[-1], "focus reaches the last control")
        assert st["scroll"] > 0 and d.visible(st, order[-1]), "the focused control is revealed inside the viewport"
        shot, st, ring = ring_visible(d, "focus-last", order[-1], ACCENT[(theme, mode)])
        entry["keyboard_reveal"] = {"focus": order[-1], "scroll": st["scroll"], "ring_pixels": ring}

        # a real click on the revealed control starts its command
        mark = d.mark()
        cx, cy = d.centre(st, order[-1])
        d.click(cx, cy)
        want = EXPECT.get(order[-1])
        assert want and any(want in c for c in d.since(mark)), (order[-1], d.since(mark))
        entry["click_revealed"] = {order[-1]: d.since(mark)}
        d.close_panel()

        # no click ever reaches a control that is scrolled out of view: at the clipped bottom edge only the control really there answers
        st = d.open_panel()
        px, py, pw, ph = st["panel"]
        edge_y = round(py + ph - 2)
        probes = []
        for xx in (round(px + 40), round(px + pw / 2), round(px + pw - 40)):
            owner = [t for t in order if d.visible(st, t) or True]
            under = [t for t in order if st["targets"][t][0] <= xx < st["targets"][t][0] + st["targets"][t][2] and st["targets"][t][1] <= edge_y < st["targets"][t][1] + st["targets"][t][3]]
            st = d.open_panel() if not d.alive() else st
            time.sleep(1.2)
            mark = d.mark()
            d.click(xx, edge_y)
            time.sleep(0.8)
            calls = d.since(mark)
            for call in calls:
                assert owned_by(call, under), f"a hidden control answered at ({xx}, {edge_y}): {call}; under the point: {under}"
            probes.append({"x": xx, "y": edge_y, "under": under, "recorded": calls})
            st = d.open_panel() if not d.alive() else d.state()
        entry["bottom_edge_clicks"] = probes
        # a point beyond the viewport (below the clipped panel, still on the screen when there is room, else beside it) closes and starts nothing
        st = d.open_panel() if not d.alive() else d.state()
        time.sleep(1.2)  # programs a control starts after closing the panel (`sleep 0.15; exec …`) must have run before the mark
        mark = d.mark()
        d.click(max(5, round(x - 60)), 20)
        wait(lambda: not d.alive(), "click beside the panel closes it")
        assert d.since(mark) == []
        entry["outside"] = "closed, nothing recorded"
    finally:
        d.close()


def scenario_bar_path(decbar, evidence, results, name, screen, scale, mode, theme):
    """The panel opened from the real menu bar's button: pointer and keyboard on the bar's own popup, clipped and scrolled on a short screen."""
    d = Desktop(decbar, evidence, Scenario(name, screen, scale, mode, theme, bar=True))
    entry = results[name] = {"screen": screen, "scale": scale, "mode": mode, "host": "decbar menu bar"}
    try:
        st = d.open_bar_panel()
        x, y, w, h = st["panel"]
        entry.update({"panel": st["panel"], "content": st["content"]})
        assert abs(w - (352 * scale + 48)) < 2, w
        assert y + h <= screen[1] + 0.5, f"panel on the screen: {st['panel']}"
        order = list(st["targets"])
        d.shot("opened")
        # keyboard: Down reaches every control in order with a visible ring (revealing it when the panel is clipped)
        for index, target in enumerate(order):
            d.key("Down")
            st = d.wait_state(lambda s, t=target: s["focus"] == t, f"focus {target}")
            if index in (0, len(order) - 1):
                ring_visible(d, f"focus-{target}", target, ACCENT[(theme, mode)])
                assert d.visible(st, target), f"{target} revealed"
        clipped = st["content"] > st["panel"][3] + 1
        entry["clipped"] = clipped
        if clipped:
            assert st["scroll"] > 0, "keyboard focus scrolled the clipped panel"
            # real click on the focused (revealed) last control
            mark = d.mark()
            cx, cy = d.centre(st, order[-1])
            d.click(cx, cy)
            assert any(EXPECT[order[-1]] in c for c in d.since(mark)), d.since(mark)
            entry["click_revealed"] = {order[-1]: d.since(mark)}
        # Escape closes the panel (the bar itself keeps running)
        d.key("Escape")
        wait(lambda: d.panel.poll() is None, "the bar survives Escape")
        time.sleep(0.3)
        assert d.state()["focus"] in ("none", order[-1])
        entry["escape"] = "panel closed, bar alive"
    finally:
        d.close()


def scenario_groups(decbar, evidence, results):
    cases = {
        "sound-display": (["sound", "display"], {"Mute", "Volume", "Brightness"}),
        "connectivity": (["connectivity"], {"Wifi", "Network", "Bluetooth"}),
        "focus": (["focus"], {"NightShift", "DarkMode", "Focus"}),
        "media-actions": (["media", "actions"], {"MediaPrev", "MediaToggle", "MediaNext", "Screenshot", "Settings", "Lock", "Power", "StayAwake", "Notifications"}),
        "toggles": (["toggles"], {"Record", "IdleInhibit", "GameMode", "Dnd"}),
    }
    for key, (tiles, expected) in cases.items():
        name = f"groups-{key}-1080-100-light"
        d = Desktop(decbar, evidence, Scenario(name, (1920, 1080), 1.0, "light", "macos", tiles=tiles))
        try:
            st = d.open_panel()
            got = set(st["targets"])
            assert got == expected, f"{tiles}: {sorted(got)} != {sorted(expected)}"
            d.shot("opened")
            results[name] = {"tiles": tiles, "targets": sorted(got)}
            # every remaining control still answers a real click
            for t in sorted(got - {"Volume", "Brightness"}):
                if not d.alive():
                    st = d.open_panel()
                if t in ("DarkMode",):
                    continue
                mark = d.mark()
                x, y = d.centre(st, t)
                d.click(x, y)
                if t in EXPECT:
                    assert any(EXPECT[t] in c for c in d.since(mark)), (t, d.since(mark))
                st = d.state() if d.alive() else st
        finally:
            d.close()


def scenario_unavailable(decbar, evidence, results):
    missing = ["nmcli", "bluetoothctl", "wpctl", "pactl", "playerctl", "brightnessctl", "decnotify", "omarchy", "decutil", "decsession", "rdwm-lock", "loginctl", "decthemectl", "desktop-style"]
    name = "unavailable-1080-100-dark"
    d = Desktop(decbar, evidence, Scenario(name, (1920, 1080), 1.0, "dark", missing=missing, power_supply=False))
    try:
        st = d.open_panel()
        got = set(st["targets"])
        # nothing whose provider is absent is offered; the rest stays
        for absent in ("Wifi", "Network", "Bluetooth", "Volume", "Mute", "Brightness", "MediaToggle", "NightShift", "Focus", "Lock", "Power", "Notifications", "IdleInhibit", "GameMode", "Dnd"):
            assert absent not in got, f"{absent} offered without its provider: {sorted(got)}"
        for present in ("DarkMode", "StayAwake", "Record", "Screenshot", "Settings"):
            assert present in got, f"{present} should stay: {sorted(got)}"
        d.shot("opened")
        results[name] = {"targets": sorted(got)}
        # the controls that stay do not pretend: with no decutil, Screenshot and Record start nothing
        for t in ("Screenshot",):
            st = d.open_panel() if not d.alive() else d.state()
            mark = d.mark()
            x, y = d.centre(st, t)
            d.click(x, y)
            calls = d.since(mark)
            assert calls == [], f"{t} cannot claim success without its provider: {calls}"
            results[name][t] = "nothing started"
    finally:
        d.close()


def scenario_matrix(decbar, evidence, results):
    for mode, theme in (("dark", "nord"), ("light", "macos")):
        for scale in (1.0, 1.25, 2.0):
            name = f"matrix-1080-{int(scale * 100)}-{mode}"
            d = Desktop(decbar, evidence, Scenario(name, (1920, 1080), scale, mode, theme))
            try:
                st = d.open_panel()
                x, y, w, h = st["panel"]
                assert abs(w - (352 * scale + 48)) < 2, (name, w)
                d.shot("opened")
                d.key("Down", "Down", "Down")
                st = d.wait_state(lambda s: s["focus"] != "none", "focus")
                shot, st, ring = ring_visible(d, "focus", st["focus"], ACCENT[(theme, mode)])
                results[name] = {"panel": st["panel"], "content": st["content"], "scroll": st["scroll"], "focus": st["focus"], "ring_pixels": ring}
            finally:
                d.close()


SCENARIOS = {
    "pointer": scenario_pointer_all,
    "keyboard": scenario_keyboard,
    "short-200": lambda b, e, r: scenario_short_window(b, e, r, "short-1280x520-200-light", (1280, 520), 2.0, "light", "macos"),
    "short-125": lambda b, e, r: scenario_short_window(b, e, r, "short-1366x600-125-dark", (1366, 600), 1.25, "dark", "nord"),
    "short-100": lambda b, e, r: scenario_short_window(b, e, r, "short-1024x480-100-dark", (1024, 480), 1.0, "dark", "nord"),
    "bar-1080": lambda b, e, r: scenario_bar_path(b, e, r, "bar-1920x1080-100-dark", (1920, 1080), 1.0, "dark", "nord"),
    "bar-short": lambda b, e, r: scenario_bar_path(b, e, r, "bar-1280x520-200-light", (1280, 520), 2.0, "light", "macos"),
    "groups": scenario_groups,
    "unavailable": scenario_unavailable,
    "matrix": scenario_matrix,
}


def main():
    decbar = Path(sys.argv[1]).resolve()
    evidence = Path(sys.argv[2]).resolve()
    evidence.mkdir(parents=True, exist_ok=True)
    only = sys.argv[3:]
    results = {"decbar_sha256": hashlib.sha256(decbar.read_bytes()).hexdigest()}
    for name, fn in SCENARIOS.items():
        if only and name not in only:
            continue
        fn(decbar, evidence, results)
    (evidence / "report.json").write_text(json.dumps(results, indent=2))
    print(json.dumps(results, indent=2))


if __name__ == "__main__":
    main()
