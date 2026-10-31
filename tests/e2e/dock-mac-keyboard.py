#!/usr/bin/env python3
"""Native acceptance of the macOS-style dock's keyboard mode (`decdock --focus`): real decdock, real rdwm window manager, fake apps, real XTEST input,
on a private Xvfb. Four edges x 100/125/200 % x light/dark.

Usage: python3 tests/e2e/dock-mac-keyboard.py BIN_DIR EVIDENCE_DIR [scenario-substring ...]
  BIN_DIR       the cargo output directory with `decdock` and `examples/fakewin` (e.g. $CARGO_TARGET_DIR/debug)
  EVIDENCE_DIR  a NEW directory: per-scenario logs, screenshots, report.json

Everything is private: the display is an explicitly reserved high number (>= 700) whose Xvfb is proved to be this script's own, in the filesystem and
the abstract socket namespaces, before any client or input (tests/e2e/gnome_xvfb.py); a private HOME/XDG/runtime; stand-in launcher/opener; invalid
session and system buses. Only processes started here are stopped. No host app, config, clipboard or input is touched.

Per scenario it proves, with real key events: the ring appears on the first item and follows the dock's axis; Home/End/wrap; the pointer never moves; keys do not reach the
focused app while the mode is on and do again after every way out (no stale grab); the arrow towards the desktop opens the item's menu, a row runs
(focus its window), Escape closes the menu then leaves; Escape puts the window focus back where the window manager moved it from; Enter/Space activate
(focus a running app, start a closed one, Launchpad, Trash); a pointer press leaves.
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

from PIL import Image, ImageChops

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gnome_xvfb  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
SCREEN = (1280, 800)
EDGES = ["bottom", "top", "left", "right"]
SCALES = ["1", "1.25", "2"]
MODES = ["dark", "light"]
# which key opens the menu / moves back and forth on each edge (see src/dock_app/keyboard.rs)
KEYS = {"bottom": ("Left", "Right", "Up"), "top": ("Left", "Right", "Down"), "left": ("Up", "Down", "Right"), "right": ("Up", "Down", "Left")}
AWAY = {"bottom": "Down", "top": "Up", "left": "Left", "right": "Right"}


def wait(cond, what, timeout=15.0):
    end = time.time() + timeout
    last = None
    while time.time() < end:
        try:
            last = cond()
            if last:
                return last
        except subprocess.CalledProcessError:
            pass
        time.sleep(0.1)
    raise AssertionError(f"timed out: {what}")


class Scenario:
    def __init__(self, bin_dir, evidence, edge, scale, mode):
        self.edge, self.scale, self.mode = edge, scale, mode
        self.name = f"{edge}-{scale.replace('.', '_')}-{mode}"
        self.evidence = evidence / self.name
        self.evidence.mkdir()
        self.tmp = tempfile.TemporaryDirectory(prefix="dock-kb-")
        root = self.root = Path(self.tmp.name)
        for sub in ["home", "config", "cache", "state", "data/applications", "run", "bin", "dc", "empty"]:
            (root / sub).mkdir(parents=True, mode=0o700)
        self.bin = bin_dir
        self.log = (self.evidence / "processes.log").open("w")
        self.children = []
        self.checks = []
        self.display = gnome_xvfb.reserve_display()
        self.fake_log = root / "fakewin.log"
        self.fake_log.write_text("")
        # stand-ins that record instead of launching anything real
        for name in ("launchpad", "open"):
            f = root / "bin" / f"{name}.sh"
            f.write_text(f"#!/bin/sh\necho \"$@\" >> {root}/{name}.log\n")
            f.chmod(0o755)
        for app, title, colour in (("alpha", "Alpha Window", "cc3333"), ("beta", "Beta Window", "33cc66"), ("gamma", "Gamma Window", "cccc33")):
            (root / "data/applications" / f"{app}.desktop").write_text(
                f'[Desktop Entry]\nType=Application\nName={app}\nExec={bin_dir}/examples/fakewin {app} "{title}" {colour}\nStartupWMClass={app}\n'
            )
        (root / "dc/decebal.json").write_text(json.dumps({"theme": {"name": "gnome-catppuccin-dock", "mode": mode}, "dock": {"enabled": True}, "bar": {"enabled": False}}))
        (root / "dc/dock-pinned-macos.json").write_text('{"version":1,"pinned":["launchpad","alpha","beta","gamma","trash"],"order":["launchpad","alpha","beta","gamma","trash"]}')
        (root / "dc/dock-settings.json").write_text(json.dumps({"edge": edge, "appSpacing": 8, "magnification": 1.4, "transparency": 0.3}))
        self.env = {
            "PATH": "/usr/bin:/bin", "HOME": str(root / "home"), "XDG_CONFIG_HOME": str(root / "config"), "XDG_CACHE_HOME": str(root / "cache"),
            "XDG_STATE_HOME": str(root / "state"), "XDG_DATA_HOME": str(root / "data"), "XDG_DATA_DIRS": str(root / "empty"), "XDG_RUNTIME_DIR": str(root / "run"),
            "DECEBAL_CONFIG": str(root / "dc"), "DECEBAL_THEMES": str(REPO / "themes"), "DECEBAL_SYSTEM_THEMES": str(root / "empty"), "DECEBAL_SYSTEM_AUTOMATIC": "0",
            "DECEBAL_BACKEND": "x11", "DECEBAL_SCALE": scale, "DISPLAY": self.display, "FAKEWIN_LOG": str(self.fake_log),
            "DECEBAL_LAUNCHPAD": str(root / "bin/launchpad.sh"), "DECEBAL_OPEN": str(root / "bin/open.sh"),
            "DBUS_SESSION_BUS_ADDRESS": f"unix:path={root}/no-bus", "DBUS_SYSTEM_BUS_ADDRESS": f"unix:path={root}/no-system",
        }
        (root / "run").chmod(0o700)
        w, h = SCREEN
        self.server = subprocess.Popen(["Xvfb", self.display, "-screen", "0", f"{w}x{h}x24", "+extension", "RANDR", "+extension", "SHAPE", "-nolisten", "tcp"], env=self.env, stdout=self.log, stderr=self.log)
        try:
            self.ownership = gnome_xvfb.verify_owned(self.server, self.display)  # before any client connects or any input is sent
        except Exception:
            gnome_xvfb.stop_owned(self.server)
            self.close_files()
            raise

    # ------------------------------------------------------------ helpers
    def x(self, *args, check=True, timeout=20):
        return subprocess.run(args, env=self.env, text=True, capture_output=True, check=check, timeout=timeout).stdout.strip()

    def spawn(self, *args):
        p = subprocess.Popen(args, env=self.env, stdout=self.log, stderr=self.log)
        self.children.append(p)
        return p

    def check(self, name, ok, detail=""):
        self.checks.append({"name": name, "ok": bool(ok), "detail": str(detail)})
        print(("  ok   " if ok else "  FAIL ") + f"{self.name}: {name}" + (f"  ({detail})" if detail and not ok else ""))
        return ok

    def close_files(self):
        self.log.close()
        self.tmp.cleanup()

    def pointer(self):
        out = self.x("xdotool", "getmouselocation", "--shell")
        return tuple(int(l.split("=")[1]) for l in out.splitlines() if l[:2] in ("X=", "Y="))

    def active(self):
        out = self.x("xprop", "-root", "_NET_ACTIVE_WINDOW")
        return int(out.split()[-1], 16) if "0x" in out else 0

    def win(self, title):
        out = self.x("xdotool", "search", "--name", f"^{title}$", check=False)
        return int(out.split()[0]) if out else None

    def fake_lines(self):
        return self.fake_log.read_text().splitlines()

    def shot(self, label):
        path = self.evidence / f"{label}.png"
        self.x("import", "-window", str(self.dock), str(path))
        return Image.open(path).convert("RGB")

    def geometry(self):
        out = self.x("xdotool", "getwindowgeometry", "--shell", str(self.dock))
        d = dict(l.split("=") for l in out.splitlines() if "=" in l)
        return int(d["X"]), int(d["Y"]), int(d["WIDTH"]), int(d["HEIGHT"])

    def key(self, name):
        self.x("xdotool", "key", "--clearmodifiers", name)
        time.sleep(0.25)

    def focus_request(self):
        self.x(str(self.bin / "decdock"), "--focus")
        time.sleep(0.5)

    def ring(self, base, label):
        """Bounding box of what changed against the no-focus baseline, i.e. the focus ring, as (centre along the axis, pixel count)."""
        img = self.shot(label)
        if img.size != base.size:
            return None
        diff = ImageChops.difference(base, img).convert("L").point(lambda v: 255 if v > 24 else 0)
        box = diff.getbbox()
        if not box:
            return None
        n = diff.histogram()[255]
        horizontal = self.edge in ("bottom", "top")
        centre = (box[0] + box[2]) / 2 if horizontal else (box[1] + box[3]) / 2
        return centre, n, box

    def leaked_keys(self):
        return [l for l in self.fake_lines() if " key " in l]

    # ------------------------------------------------------------- the run
    def start(self):
        self.x("xdotool", "mousemove", "1000", "700")
        self.spawn("rdwm", "--backend", "x11")
        wait(lambda: "window" in self.x("xprop", "-root", "_NET_SUPPORTING_WM_CHECK", check=False), "window manager")
        self.spawn(str(self.bin / "examples/fakewin"), "alpha", "Alpha Window", "cc3333")
        wait(lambda: self.win("Alpha Window"), "alpha window")
        self.spawn(str(self.bin / "examples/fakewin"), "beta", "Beta Window", "33cc66")
        wait(lambda: self.win("Beta Window"), "beta window")
        self.spawn(str(self.bin / "decdock"))
        self.dock = int(wait(lambda: self.x("xdotool", "search", "--name", "^decebal-dock$", check=False).split("\n")[0], "dock window"))
        time.sleep(1.0)
        self.x("xdotool", "windowactivate", "--sync", str(self.win("Beta Window")))
        self.x("xdotool", "mousemove", "1000", "700")
        time.sleep(0.5)

    def run(self):
        back, fwd, menu_key = KEYS[self.edge]
        self.start()
        base = self.shot("baseline")
        g0 = self.geometry()
        self.check("dock sits on the requested edge", self.edge_ok(g0), g0)
        p0 = self.pointer()
        alpha, beta = self.win("Alpha Window"), self.win("Beta Window")
        self.key("a")
        self.check("before --focus keys reach the focused app (the dock holds no keyboard)", len(self.leaked_keys()) == 1, self.leaked_keys())
        self.fake_log.write_text("")

        # ---- enter, axis, Home/End, wrap, no key leak
        self.focus_request()
        first = self.ring(base, "ring-first")
        self.check("--focus draws a ring on the first item", first is not None and first[1] > 20, first and first[1])
        self.key("a")
        self.key("b")
        self.check("letter keys are swallowed by the dock, none reaches the focused app", not self.leaked_keys(), self.leaked_keys())
        self.key(fwd)
        second = self.ring(base, "ring-second")
        self.key(fwd)
        third = self.ring(base, "ring-third")
        self.check("the forward arrow moves the ring along the dock axis", first and second and third and first[0] < second[0] < third[0], (first and first[0], second and second[0], third and third[0]))
        self.key(back)
        again = self.ring(base, "ring-back")
        self.check("the back arrow returns to the previous item", again and second and abs(again[0] - second[0]) < 2, (again and again[0], second and second[0]))
        self.key("End")
        last = self.ring(base, "ring-end")
        self.check("End jumps to the last item", last and third and last[0] > third[0], (last and last[0]))
        self.key(fwd)
        wrapped = self.ring(base, "ring-wrap")
        self.check("forward from the last item wraps to the first", wrapped and first and abs(wrapped[0] - first[0]) < 2, (wrapped and wrapped[0], first and first[0]))
        self.key("End")
        self.key("Home")
        home = self.ring(base, "ring-home")
        self.check("Home jumps to the first item", home and first and abs(home[0] - first[0]) < 2)
        self.key(AWAY[self.edge])
        self.check("the arrow away from the desktop is not bound", (self.ring(base, "ring-away") or (0,))[0] == home[0])
        self.check("the pointer never moved", self.pointer() == p0, (p0, self.pointer()))

        # ---- context menu
        self.key(fwd)  # on alpha
        self.key(menu_key)
        opened = self.shot("menu-open")
        self.check("the arrow towards the desktop opens the item's menu (the surface grows)", opened.size[0] * opened.size[1] > base.size[0] * base.size[1], (base.size, opened.size))
        self.key("Escape")
        self.check("Escape closes the menu first and keeps the ring", self.shot("after-menu-escape").size == base.size and self.ring(base, "ring-after-menu") is not None)
        self.key(menu_key)
        self.key("Down")
        self.key("Return")
        time.sleep(0.4)
        self.check("a menu row runs (the first row focuses Alpha's window, Beta had it) and ends the mode", self.active() == alpha and self.shot("after-row").size == base.size and self.ring(base, "ring-after-row") is None, hex(self.active()))
        self.fake_log.write_text("")
        self.key("c")
        self.check("after a menu action the keyboard is back with the app (no stale grab)", any("Alpha Window key" in l for l in self.fake_lines()) and len(self.leaked_keys()) == 1, self.leaked_keys())
        self.fake_log.write_text("")

        # ---- Enter on a running app, Space launching a closed one
        self.x("xdotool", "windowactivate", "--sync", str(beta))
        wait(lambda: self.active() == beta, "beta active", 5)
        self.focus_request()
        self.key(fwd)
        self.key("Return")
        self.check("Enter on a running app focuses its window and leaves the mode", self.active() == alpha and self.ring(base, "ring-after-enter") is None, hex(self.active()))
        self.key("d")
        self.check("keys reach the app after Enter", len(self.leaked_keys()) == 1, self.leaked_keys())
        self.fake_log.write_text("")
        self.focus_request()
        for _ in range(3):
            self.key(fwd)
        self.key("space")
        gamma = wait(lambda: self.win("Gamma Window"), "gamma started by Space", 12)
        self.check("Space on a closed app starts it", gamma is not None)
        wait(lambda: self.active() == gamma, "gamma focused by the window manager", 8)
        self.fake_log.write_text("")
        time.sleep(1.5)  # the launch bounce is over; the new running dot is part of the picture
        base = self.shot("baseline-gamma")
        self.key("e")
        self.check("the started app has the keyboard, the dock none", self.ring(base, "ring-after-space") is None and any("Gamma Window key" in l for l in self.fake_lines()), self.fake_lines())
        self.fake_log.write_text("")

        # ---- Escape restores the focus the window manager moved
        self.x("xdotool", "windowactivate", "--sync", str(beta))
        wait(lambda: self.active() == beta, "beta active", 5)
        self.focus_request()
        self.x("xdotool", "windowactivate", str(alpha), check=False)
        time.sleep(0.6)
        moved = self.active()
        self.key("Escape")
        time.sleep(0.6)
        self.check("the window manager really moved the focus while the mode was on", moved == alpha, (hex(moved), hex(alpha)))
        self.check("Escape leaves and puts the focus back on the window that had it", self.active() == beta, (hex(self.active()), hex(beta)))
        self.check("the ring is gone after Escape", self.ring(base, "ring-after-escape") is None)
        self.fake_log.write_text("")
        self.x("xdotool", "windowactivate", "--sync", str(beta))
        self.key("f")
        self.check("keys reach the app after Escape", len(self.leaked_keys()) == 1, self.leaked_keys())
        self.fake_log.write_text("")

        # ---- Launchpad and Trash through Enter
        self.focus_request()
        self.key("Return")
        launched = wait(lambda: (self.root / "launchpad.log").exists(), "launchpad stand-in", 6)
        self.check("Enter on Launchpad starts the launcher", launched)
        self.focus_request()
        self.key("End")
        self.key("Return")
        opened = wait(lambda: (self.root / "open.log").exists() and "trash" in (self.root / "open.log").read_text(), "trash opener", 6)
        self.check("Enter on Trash opens it", opened)

        # ---- a pointer press leaves
        self.x("xdotool", "windowactivate", "--sync", str(beta))
        self.focus_request()
        self.check("--focus works again", self.ring(base, "ring-again") is not None)
        self.x("xdotool", "mousemove", "1000", "700")
        self.x("xdotool", "click", "1")
        time.sleep(0.5)
        self.check("a pointer press leaves the mode", self.ring(base, "ring-after-press") is None)
        self.fake_log.write_text("")
        self.x("xdotool", "windowactivate", "--sync", str(beta), check=False)
        self.key("g")
        self.check("keys reach an app after the press (grabs released)", len(self.leaked_keys()) == 1, self.leaked_keys())

    def edge_ok(self, g):
        x, y, w, h = g
        sw, sh = SCREEN
        return {"bottom": y + h >= sh - 40 and y > sh // 2, "top": y < 80, "left": x < 80, "right": x + w >= sw - 40 and x > sw // 2}[self.edge]

    def close(self):
        for p in reversed(self.children):
            p.terminate()
        for p in self.children:
            try:
                p.wait(timeout=5)
            except subprocess.TimeoutExpired:
                p.kill()
        gnome_xvfb.stop_owned(self.server)
        self.close_files()


def main():
    bin_dir, evidence = Path(sys.argv[1]).resolve(), Path(sys.argv[2])
    filters = sys.argv[3:]
    evidence.mkdir(parents=True)  # a new directory: existing evidence is never overwritten
    report = []
    for edge in EDGES:
        for scale in SCALES:
            for mode in MODES:
                name = f"{edge}-{scale.replace('.', '_')}-{mode}"
                if filters and not any(f in name for f in filters):
                    continue
                print(f"== {name}")
                s = Scenario(bin_dir, evidence, edge, scale, mode)
                error = None
                try:
                    s.run()
                except Exception as e:  # keep going: one failed scenario must not hide the others
                    error = repr(e)
                    s.check("scenario completed", False, error)
                finally:
                    s.close()
                report.append({"scenario": name, "ownership": s.ownership, "checks": s.checks, "error": error})
    (evidence / "report.json").write_text(json.dumps(report, indent=2))
    failed = [(r["scenario"], c["name"]) for r in report for c in r["checks"] if not c["ok"]]
    total = sum(len(r["checks"]) for r in report)
    print(f"{total - len(failed)}/{total} checks passed in {len(report)} scenarios")
    for f in failed:
        print("FAILED", *f)
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
