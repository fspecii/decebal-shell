#!/usr/bin/env python3
"""The Plasma Breeze taskbar under a REAL window manager (rdwm), with fake client windows, on a private Xvfb.
Usage: python3 tests/e2e/plasma-wm-native.py BIN_DIR THEMES_DIR RDWM_DIR EVIDENCE_DIR [--scales 1,1.25,2] [--edges bottom,top,left,right] [--modes dark]

BIN_DIR holds decdock + decthemectl, RDWM_DIR the immutable rdwm + rdwmctl (checked by sha256 into the report). For every edge / scale / mode:
* rdwm (own bar and decorations off) manages the clients, the dock reserves its strut and the work area is asserted against the strut exactly;
  every client lies inside the work area;
* three fake windows (python-xlib, one WM_CLASS each) become task buttons; REAL pointer clicks (xdotool) on those buttons focus a task, minimise
  the focused one (the WM sets _NET_WM_STATE_HIDDEN, the tracker reads it, the button is painted in the minimised state) and restore it;
* an ICCCM urgency hint and an EWMH DEMANDS_ATTENTION state each tint the task in the attention colour and clear again;
* a click on pager cells switches the workspace (_NET_CURRENT_DESKTOP) and back, and the cell of the busy workspace shows its windows;
* a status popup opens on a click, stays on the output and closes on Escape;
* clicks in the floating gap neither focus a task nor open anything;
* the clock text keeps WCAG contrast >= 4.5 against its strip.
Private HOME/XDG/bus, explicit reserved HIGH display (>= 700) verified in both socket namespaces before any client or input (gnome_xvfb.py); the
live display is never contacted and only this run's processes are stopped.
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

from PIL import Image, ImageChops
from Xlib import X, Xatom, display as xdisplay, Xutil

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gnome_xvfb  # noqa: E402

W, H = 1920, 1080
THICKNESS, GAP = 44, 8  # the declared [taskbar] numbers of themes/plasma-breeze, px at scale 1


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


def luminance(px):
    def lin(v):
        v /= 255
        return v / 12.92 if v <= 0.03928 else ((v + 0.055) / 1.055) ** 2.4
    return 0.2126 * lin(px[0]) + 0.7152 * lin(px[1]) + 0.0722 * lin(px[2])


def contrast(a, b):
    la, lb = sorted((luminance(a), luminance(b)), reverse=True)
    return (la + 0.05) / (lb + 0.05)


def mean(image, box):
    crop = image.crop(box).convert("RGB")
    n = crop.width * crop.height
    return tuple(sum(px[i] for px in crop.get_flattened_data()) / n for i in range(3))


def apart(a, b):
    return sum(abs(x - y) for x, y in zip(a, b))


class Clients:
    """Fake application windows: real X clients (WM_CLASS, _NET_WM_NAME, WM_HINTS) the window manager manages."""

    def __init__(self, display):
        self.d = xdisplay.Display(display)
        self.root = self.d.screen().root
        self.utf8 = self.d.intern_atom("UTF8_STRING")
        self.net_name = self.d.intern_atom("_NET_WM_NAME")
        self.state = self.d.intern_atom("_NET_WM_STATE")
        self.demands = self.d.intern_atom("_NET_WM_STATE_DEMANDS_ATTENTION")

    def make(self, cls, title, shade):
        win = self.root.create_window(0, 0, 640, 400, 0, self.d.screen().root_depth, X.InputOutput, X.CopyFromParent, background_pixel=shade, event_mask=X.PropertyChangeMask)
        win.set_wm_class(cls.lower(), cls)
        win.set_wm_name(title)
        win.change_property(self.net_name, self.utf8, 8, title.encode())
        win.set_wm_hints(flags=Xutil.InputHint, input=1)
        win.map()
        self.d.sync()
        return win

    def urgency(self, win, on):
        win.set_wm_hints(flags=Xutil.InputHint | (Xutil.UrgencyHint if on else 0), input=1)
        self.d.sync()

    def demand_attention(self, win, on):
        """The EWMH way: a client message to the root asking the WM to add / remove the state (source 1 = application)."""
        from Xlib.protocol import event
        ev = event.ClientMessage(window=win, client_type=self.state, data=(32, [1 if on else 0, self.demands, 0, 1, 0]))
        self.root.send_event(ev, event_mask=X.SubstructureRedirectMask | X.SubstructureNotifyMask)
        self.d.sync()

    def close(self):
        self.d.close()


def run_one(bin_dir, themes, rdwm_dir, evidence, mode, edge, scale, report):
    tag = f"plasma-wm-{mode}-{edge}-{int(scale * 100)}"
    t, g = round(THICKNESS * scale), round(GAP * scale)
    horizontal = edge in ("bottom", "top")
    with tempfile.TemporaryDirectory(prefix="plasma-wm-native-") as private:
        root = Path(private)
        for sub in ["home", "config", "cache", "state", "data", "run", "bin", "empty-themes"]:
            (root / sub).mkdir(mode=0o700)
        for app in ["curl", "nmcli", "pactl", "wpctl", "playerctl", "bluetoothctl", "brightnessctl"]:
            stub = root / "bin" / app
            stub.write_text("#!/bin/sh\nexit 1\n")
            stub.chmod(0o755)
        display = gnome_xvfb.reserve_display()
        debug = root / "layout.txt"
        wm_config = root / "rdwm.toml"
        wm_config.write_text("[general]\nborder_px = 0\n[bar]\nshow = false\n[decoration]\nenabled = false\n")
        env = dict(
            os.environ,
            HOME=str(root / "home"), XDG_CONFIG_HOME=str(root / "config"), XDG_CACHE_HOME=str(root / "cache"),
            XDG_STATE_HOME=str(root / "state"), XDG_DATA_HOME=str(root / "data"), XDG_RUNTIME_DIR=str(root / "run"),
            DECEBAL_CONFIG=str(root / "config"), DECEBAL_THEMES=str(themes), DECEBAL_SYSTEM_THEMES=str(root / "empty-themes"),
            DECEBAL_BACKEND="x11", DECEBAL_SYSTEM_AUTOMATIC="0", DECEBAL_SCALE=str(scale), DECDOCK_DEBUG_LAYOUT=str(debug), DISPLAY=display,
            DBUS_SESSION_BUS_ADDRESS=f"unix:path={private}/no-bus", DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={private}/no-system",
            RDWMCTL=str(rdwm_dir / "rdwmctl"), RDWM_SOCK=str(root / "run/rdwm.sock"), RDWM_CONFIG=str(wm_config),
            PATH=f'{root / "bin"}:{os.environ["PATH"]}',
        )
        for key in ["WAYLAND_DISPLAY", "PULSE_SERVER", "XAUTHORITY", "SESSION_MANAGER", "HYPRLAND_INSTANCE_SIGNATURE"]:
            env.pop(key, None)
        log = (evidence / f"{tag}.log").open("w")
        server = subprocess.Popen(["Xvfb", display, "-screen", "0", f"{W}x{H}x24", "-nolisten", "tcp"], env=env, stdout=log, stderr=log)
        children, clients = [], None
        entry = {"mode": mode, "edge": edge, "scale": scale, "steps": []}
        try:
            def run(*args, timeout=30, check=True):
                return subprocess.run([str(a) for a in args], env=env, text=True, capture_output=True, check=check, timeout=timeout).stdout.strip()

            def ctl(*args):
                return json.loads(run(rdwm_dir / "rdwmctl", *args))

            def step(name, **data):
                entry["steps"].append({"step": name, **data})

            def shot(name):
                path = evidence / f"{tag}-{name}.png"
                run("import", "-window", "root", path)
                return Image.open(path).convert("RGB")

            gnome_xvfb.verify_owned(server, display)  # before any client connects or input is sent
            wait(lambda: server.poll() is None and run("xdotool", "getdisplaygeometry"), "private Xvfb ready")
            (root / "picom.conf").write_text('backend = "xrender";\nvsync = false;\nuse-damage = false;\nshadow = false;\nfading = false;\n')
            children.append(subprocess.Popen(["picom", "--config", str(root / "picom.conf")], env=env, stdout=log, stderr=log))
            time.sleep(0.8)
            children.append(subprocess.Popen([str(rdwm_dir / "rdwm"), "--backend", "x11", "--config", str(wm_config)], env=env, stdout=log, stderr=log))
            wait(lambda: (root / "run/rdwm.sock").exists() and ctl("state")["ok"], "private rdwm up")
            (root / "config/dock-settings.json").write_text(json.dumps({"edge": edge}))
            run(bin_dir / "decthemectl", "theme", "apply", "plasma-breeze", "--no-rdwm")
            cfg_path = root / "config/decebal.json"
            cfg = json.loads(cfg_path.read_text())
            cfg["theme"]["mode"] = mode
            cfg_path.write_text(json.dumps(cfg))

            # ---- the strut and the work area
            dock = wait(lambda: run("xdotool", "search", "--name", "decebal-dock", check=False).splitlines()[0], "dock window")
            strut = wait(lambda: [int(v) for v in run("xprop", "-id", dock, "_NET_WM_STRUT_PARTIAL").split("=")[-1].replace(" ", "").split(",") if v.lstrip("-").isdigit()][:4], "dock strut")
            side = {"left": 0, "right": 1, "top": 2, "bottom": 3}[edge]
            assert strut[side] in (t + g, round((THICKNESS + GAP) * scale)), f"{tag}: strut {strut}"
            reserve = strut[side]
            expect = {"bottom": (0, 0, W, H - reserve), "top": (0, reserve, W, H - reserve), "left": (reserve, 0, W - reserve, H), "right": (0, 0, W - reserve, H)}[edge]
            area = lambda: tuple(int(v) for v in run("xprop", "-root", "_NET_WORKAREA").split("=")[-1].replace(" ", "").split(",")[:4])
            wait(lambda: area() == expect, f"{tag}: work area {area()} should be {expect}")
            monitor = ctl("state")["state"]["monitors"][0]["workarea"]
            assert (monitor["x"], monitor["y"], monitor["w"], monitor["h"]) == expect, f"{tag}: rdwm work area {monitor} vs {expect}"
            step("work_area", strut=strut, work_area=list(expect))

            # ---- three fake windows -> three task buttons
            clients = Clients(display)
            names = [("PlasmaFakeA", "Alpha document", 0x335577), ("PlasmaFakeB", "Beta notes", 0x557733), ("PlasmaFakeC", "Gamma terminal", 0x775533)]
            wins = [clients.make(cls, title, shade) for cls, title, shade in names]
            ids = [w.id for w in wins]

            def layout():
                rows = {}
                for line in debug.read_text().splitlines():
                    parts = line.split()
                    rows.setdefault(parts[0], []).append(parts[1:])
                return rows

            surface = lambda: {k: int(v) for k, v in (line.split("=") for line in run("xdotool", "getwindowgeometry", "--shell", dock).splitlines() if "=" in line)}

            def tasks():
                rows = layout().get("task", [])
                return {r[0]: (float(r[1]), float(r[2]), r[3] == "1") for r in rows if r[0].startswith("plasmafake")}

            wait(lambda: len(tasks()) == 3, f"{tag}: three task buttons (have {tasks()})")
            geo = {}
            for i, win in enumerate(wins):
                out = run("xdotool", "getwindowgeometry", "--shell", win.id)
                v = dict(line.split("=") for line in out.splitlines() if "=" in line)
                geo[i] = (int(v["X"]), int(v["Y"]), int(v["WIDTH"]), int(v["HEIGHT"]))
                x, y, w_, h_ = geo[i]
                assert x >= expect[0] and y >= expect[1] and x + w_ <= expect[0] + expect[2] and y + h_ <= expect[1] + expect[3], f"{tag}: client {i} {geo[i]} leaves the work area {expect}"
            step("clients_inside_work_area", geometry=geo)

            def at(app):
                s = surface()
                x, y, _ = tasks()[app]
                return round(s["X"] + x), round(s["Y"] + y)

            def button_box(app, half=None):
                cx, cy = at(app)
                half = half or max(6, round(10 * scale))
                return (cx - half, cy - half, cx + half, cy + half)

            def click(x, y):
                run("xdotool", "mousemove", x, y)
                time.sleep(0.15)
                run("xdotool", "click", "1")
                time.sleep(0.5)

            def active():
                out = run("xprop", "-root", "_NET_ACTIVE_WINDOW")
                return int(out.split("#")[-1].strip(), 16) if "#" in out else 0

            def hidden(win):
                return "_NET_WM_STATE_HIDDEN" in run("xprop", "-id", win.id, "_NET_WM_STATE", check=False)

            keys = ["plasmafakea", "plasmafakeb", "plasmafakec"]
            # park the pointer in the middle of the desktop, then take the idle look of the buttons
            run("xdotool", "mousemove", W // 2, H // 2)
            time.sleep(0.6)
            idle = shot("idle")
            idle_look = {k: mean(idle, button_box(k)) for k in keys}

            # ---- focus, minimise, restore with real clicks
            click(*at(keys[0]))
            wait(lambda: active() == ids[0], f"{tag}: clicking the first task focuses its window")
            run("xdotool", "mousemove", W // 2, H // 2)
            time.sleep(0.6)
            focused = shot("focused")
            assert apart(mean(focused, button_box(keys[0])), idle_look[keys[0]]) > 20, f"{tag}: the focused task is not marked"
            assert apart(mean(focused, button_box(keys[1])), idle_look[keys[1]]) < 6, f"{tag}: an unfocused task changed"
            click(*at(keys[0]))  # clicking the focused task again minimises it
            wait(lambda: hidden(wins[0]), f"{tag}: the focused task's second click minimises (WM state HIDDEN)")
            run("xdotool", "mousemove", W // 2, H // 2)
            time.sleep(0.8)
            minimised = shot("minimised")
            m_look = mean(minimised, button_box(keys[0]))
            assert apart(m_look, mean(focused, button_box(keys[0]))) > 12, f"{tag}: the minimised task still looks focused"
            assert apart(m_look, idle_look[keys[0]]) > 3, f"{tag}: the minimised task looks like an idle one (the tracker flag is not painted)"
            step("minimised", delta_from_idle=apart(m_look, idle_look[keys[0]]))
            click(*at(keys[0]))  # restores
            wait(lambda: not hidden(wins[0]) and active() == ids[0], f"{tag}: clicking a minimised task restores and focuses it")
            step("restored")

            # ---- attention: the ICCCM urgency hint, then the EWMH state
            target = keys[2]
            clients.urgency(wins[2], True)
            run("xdotool", "mousemove", W // 2, H // 2)
            wait(lambda: apart(mean(shot("urgent"), button_box(target)), idle_look[target]) > 25, f"{tag}: an urgent window does not tint its task")
            clients.urgency(wins[2], False)
            wait(lambda: apart(mean(shot("urgent-cleared"), button_box(target)), idle_look[target]) < 8, f"{tag}: the tint stays after the urgency hint is cleared")
            clients.demand_attention(wins[1], True)
            wait(lambda: "DEMANDS_ATTENTION" in run("xprop", "-id", wins[1].id, "_NET_WM_STATE", check=False), f"{tag}: the WM sets DEMANDS_ATTENTION")
            wait(lambda: apart(mean(shot("demands"), button_box(keys[1])), idle_look[keys[1]]) > 25, f"{tag}: DEMANDS_ATTENTION does not tint its task")
            clients.demand_attention(wins[1], False)
            wait(lambda: apart(mean(shot("demands-cleared"), button_box(keys[1])), idle_look[keys[1]]) < 8, f"{tag}: the tint stays after the state is removed")
            step("attention")

            # ---- workspaces: a window on workspace 2, the pager's cells and its window geometry
            pager = layout().get("pagercell", [])
            assert len(pager) >= 2, f"{tag}: the pager has no cells in the dump {layout().keys()}"
            cell_xy = lambda i: (lambda s, along, cross: (round(s["X"] + along), round(s["Y"] + cross)) if horizontal else (round(s["X"] + cross), round(s["Y"] + along)))(surface(), float(pager[i][1]), float(layout()["start"][0][1] if horizontal else layout()["start"][0][0]))
            current = lambda: int(run("xprop", "-root", "_NET_CURRENT_DESKTOP").split("=")[-1])
            assert current() == 0
            before = shot("pager-ws1")
            click(*cell_xy(1))
            wait(lambda: current() == 1, f"{tag}: clicking pager cell 2 switches the workspace")
            run("xdotool", "mousemove", W // 2, H // 2)
            time.sleep(0.8)
            on_two = shot("pager-ws2")
            # the empty workspace is the active cell now: the strip changed there and the first cell lost its highlight
            assert ImageChops.difference(before, on_two).convert("L").point(lambda v: 255 if v > 8 else 0).getbbox(), f"{tag}: the pager did not repaint"
            step("pager_switch", cell=1)
            click(*cell_xy(0))
            wait(lambda: current() == 0, f"{tag}: clicking pager cell 1 comes back")
            # the windows' outlines are drawn inside the first cell: it differs from an empty one by more than its highlight
            run("xdotool", "mousemove", W // 2, H // 2)
            time.sleep(0.8)
            back = shot("pager-back")
            cell_box = lambda i: (lambda x, y, r: (x - r, y - r, x + r, y + r))(*cell_xy(i), max(3, round(5 * scale)))
            assert apart(mean(back, cell_box(0)), mean(back, cell_box(5))) > 10, f"{tag}: the pager cell of the busy workspace looks like an empty one"
            step("pager_windows", busy_vs_empty=apart(mean(back, cell_box(0)), mean(back, cell_box(5))))

            # ---- a popup stays on the output and closes on Escape
            clock = layout()["clock"][0]
            s = surface()
            cx, cy = round(s["X"] + float(clock[0])), round(s["Y"] + float(clock[1]))
            click(cx, cy)
            popup = wait(lambda: layout().get("popup"), f"{tag}: clicking the clock opens a popup")
            px, py, pw, ph = (float(v) for v in popup[0])
            sx, sy = surface()["X"], surface()["Y"]
            assert sx + px >= -1 and sy + py >= -1 and sx + px + pw <= W + 1 and sy + py + ph <= H + 1, f"{tag}: popup {popup} leaves the output"
            shot("popup")
            run("xdotool", "key", "Escape")
            wait(lambda: not layout().get("popup"), f"{tag}: Escape closes the popup")
            step("popup", rect=[px, py, pw, ph])

            # ---- the floating gap does not react
            before_active = active()
            bar = layout()["bar"][0]
            s = surface()
            if g >= 4:
                if horizontal:
                    outer_y = s["Y"] + s["HEIGHT"] - max(1, g // 2) if edge == "bottom" else s["Y"] + max(1, g // 2)
                    gap_points = [(round(s["X"] + float(layout()["task"][0][1])), outer_y)]
                else:
                    outer_x = s["X"] + s["WIDTH"] - max(1, g // 2) if edge == "right" else s["X"] + max(1, g // 2)
                    gap_points = [(outer_x, round(s["Y"] + float(layout()["task"][0][2])))]
                for gx, gy in gap_points:
                    click(gx, gy)
                assert active() == before_active and not layout().get("popup"), f"{tag}: a click in the floating gap acted"
                step("gap_clicks", points=gap_points)

            # ---- palette contrast of the clock against its strip
            image = shot("final")
            clock_box = (cx - round(24 * scale), cy - round(10 * scale), cx + round(24 * scale), cy + round(10 * scale))
            crop = image.crop(clock_box)
            colours = sorted(crop.getcolors(maxcolors=1 << 20), reverse=True)
            base = colours[0][1]
            ink = max((c for n, c in colours), key=lambda c: contrast(c, base))
            ratio = contrast(ink, base)
            assert ratio >= 4.5, f"{tag}: clock text contrast {ratio:.2f} < 4.5 ({ink} on {base})"
            step("contrast", ratio=round(ratio, 2), ink=ink, base=base)
            entry["ok"] = True
            print(f"ok {tag}: reserve {reserve}, contrast {ratio:.1f}")
        except AssertionError:
            entry["ok"] = False
            raise
        finally:
            report.append(entry)
            if clients:
                try:
                    clients.close()
                except Exception:
                    pass
            sweep(root)
            for child in reversed(children):
                child.terminate()
            for child in children:
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    child.kill()
            gnome_xvfb.stop_owned(server)
            log.close()


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    opts = {a.split("=")[0]: a.split("=")[1] for a in sys.argv[1:] if a.startswith("--") and "=" in a}
    bin_dir, themes, rdwm_dir, evidence = (Path(p).resolve() for p in args[:4])
    evidence.mkdir(parents=True, exist_ok=True)
    scales = [float(v) for v in opts.get("--scales", "1,1.25,2").split(",")]
    edges = opts.get("--edges", "bottom,top,left,right").split(",")
    modes = opts.get("--modes", "dark").split(",")
    report, failures = [], []
    for mode in modes:
        for edge in edges:
            for scale in scales:
                try:
                    run_one(bin_dir, themes, rdwm_dir, evidence, mode, edge, scale, report)
                except AssertionError as e:
                    failures.append(str(e))
                    print(f"FAIL {e}")
    sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
    out = {"binary_sha256": {"decdock": sha(bin_dir / "decdock"), "decthemectl": sha(bin_dir / "decthemectl"), "rdwm": sha(rdwm_dir / "rdwm"), "rdwmctl": sha(rdwm_dir / "rdwmctl")}, "cases": report, "failures": failures}
    (evidence / "wm-report.json").write_text(json.dumps(out, indent=2))
    print(json.dumps({"cases": len(report), "failures": len(failures)}))
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
