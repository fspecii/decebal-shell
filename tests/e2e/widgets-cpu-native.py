#!/usr/bin/env python3
"""Native CPU / worker-lifecycle / media regression for the widget host (`decwidgets daemon`).

  WIDGETS_BIN=/abs/decwidgets FAKE_MPRIS=/abs/examples/fake_mpris [THEME=nextkde OPACITY=0.4 THEMES_DIR=/abs/themes] python3 tests/e2e/widgets-cpu-native.py [OUTDIR]

Everything is private: its own Xvfb (display must be free: socket AND lock file), an xrender compositor (picom) so cards get
the real glass/ARGB path, a full-size wallpaper pixmap published as _XROOTPMAP_ID (like rdwm), a private session bus, private HOME/XDG,
no system bus, and a PATH shim for `playerctl`. Nothing touches the desktop, window manager or live config. Only processes
started here are stopped.

It repeats what a theme / wallpaper / colour change does to the host (a config stamp change = full rebuild) and checks, from
/proc per-thread CPU: idle stays near zero, a rebuild leaves no thread behind (the regression: every rebuild leaked one MPRIS
worker that spun at 100% for ever), dropping the data groups really ends the old workers, disabling all cards ends every worker,
a live player's title still updates, real mouse clicks still reach `playerctl`, and (with a glass theme and OPACITY<1) the native glass
backdrop still follows a desktop-type window that keeps changing under the cards.
"""
import json, os, shutil, subprocess, sys, tempfile, time
from pathlib import Path
from PIL import Image, ImageChops

TICK = os.sysconf("SC_CLK_TCK")
REBUILDS = int(os.environ.get("REBUILDS", "4"))  # config-stamp rebuilds before the media checks (0 isolates the glass checks)
ALTERNATIONS = int(os.environ.get("ALTERNATIONS", "3"))
OPACITY = float(os.environ.get("OPACITY", "1.0"))  # < 1 makes the card translucent so its glass backdrop is visible
THEME = os.environ.get("THEME", "caelestia")  # e.g. THEME=nextkde exercises the native glass sampler
BIN = Path(os.environ["WIDGETS_BIN"]).resolve()
FAKE = Path(os.environ["FAKE_MPRIS"]).resolve()
OUT = Path(sys.argv[1] if len(sys.argv) > 1 else "/tmp/decebal-widgets-cpu").resolve()
OUT.mkdir(parents=True, exist_ok=True)
results, procs = [], []


def check(name, ok, detail=""):
    results.append({"check": name, "ok": bool(ok), "detail": detail})
    print(("PASS " if ok else "FAIL ") + name + (f"  [{detail}]" if detail else ""), flush=True)


def threads(pid):
    r = {}
    for t in Path(f"/proc/{pid}/task").iterdir():
        try:
            f = (t / "stat").read_text().rsplit(")", 1)[1].split()
            r[int(t.name)] = int(f[11]) + int(f[12])
        except OSError:
            pass
    return r


def sample(pid, secs):
    a = threads(pid); t0 = time.monotonic(); time.sleep(secs); b = threads(pid); dt = time.monotonic() - t0
    per = {tid: (c - a.get(tid, 0)) / TICK / dt * 100 for tid, c in b.items()}
    return sum(per.values()), sorted((round(v) for v in per.values() if v > 50), reverse=True), len(b)  # "hot" = a spinning thread (the regression pegs one at 100%)


def free_display():
    for n in range(400, 470):
        if not Path(f"/tmp/.X11-unix/X{n}").exists() and not Path(f"/tmp/.X{n}-lock").exists():
            return n
    raise SystemExit("no free display")


n = free_display()
tmp = Path(tempfile.mkdtemp(prefix="widgets-cpu-"))
try:
    for d in ("home", "config", "cache", "data", "state", "run", "cfg", "shim"):
        (tmp / d).mkdir(mode=0o700)
    calls = tmp / "playerctl.log"
    shim = tmp / "shim/playerctl"
    shim.write_text(f"#!/bin/sh\necho \"$@\" >> {calls}\n"); shim.chmod(0o755)
    env = dict(os.environ, HOME=str(tmp / "home"), DISPLAY=f":{n}", XDG_CONFIG_HOME=str(tmp / "config"), XDG_CACHE_HOME=str(tmp / "cache"),
               XDG_DATA_HOME=str(tmp / "data"), XDG_STATE_HOME=str(tmp / "state"), XDG_RUNTIME_DIR=str(tmp / "run"), DECEBAL_CONFIG=str(tmp / "cfg"),
               DECEBAL_BACKEND="x11", DECEBAL_SYSTEM_AUTOMATIC="0", XDG_SESSION_TYPE="x11", DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={tmp}/no-system-bus",
               PATH=f"{tmp}/shim:" + os.environ["PATH"], FAKE_MPRIS_LOG=str(tmp / "mpris.log"))
    if os.environ.get("THEMES_DIR"):  # theme packages to load (e.g. the repository's themes/ for the glass themes)
        env["DECEBAL_THEMES"] = os.environ["THEMES_DIR"]
    for k in ("WAYLAND_DISPLAY", "RDWM_SOCK", "HYPRLAND_INSTANCE_SIGNATURE", "DBUS_STARTER_ADDRESS", "XAUTHORITY", "SESSION_MANAGER"):
        env.pop(k, None)
    procs.append(subprocess.Popen(["Xvfb", env["DISPLAY"], "-screen", "0", "3440x1440x24", "+extension", "COMPOSITE", "-nolisten", "tcp"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
    for _ in range(100):
        if Path(f"/tmp/.X11-unix/X{n}").exists():
            break
        time.sleep(0.1)
    procs.append(subprocess.Popen(["dbus-daemon", "--session", "--nofork", f"--address=unix:path={tmp}/bus"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
    time.sleep(0.5)
    env["DBUS_SESSION_BUS_ADDRESS"] = f"unix:path={tmp}/bus"
    # A full-size wallpaper pixmap published like rdwm does (_XROOTPMAP_ID + ESETROOT_PMAP_ID, set as the root background).
    # The helper keeps its connection open, so the pixmap lives as long as this test. Vertical bands of varying colour.
    wall = tmp / "wall.py"
    wall.write_text(r"""
import ctypes, ctypes.util, struct, sys, time
x = ctypes.CDLL(ctypes.util.find_library("X11"))
x.XOpenDisplay.restype = ctypes.c_void_p; x.XDefaultRootWindow.restype = ctypes.c_ulong; x.XCreatePixmap.restype = ctypes.c_ulong
x.XCreateGC.restype = ctypes.c_void_p; x.XInternAtom.restype = ctypes.c_ulong
d = x.XOpenDisplay(None); root = x.XDefaultRootWindow(ctypes.c_void_p(d)); W, H = 3440, 1440
pm = x.XCreatePixmap(ctypes.c_void_p(d), ctypes.c_ulong(root), W, H, 24)
gc = x.XCreateGC(ctypes.c_void_p(d), ctypes.c_ulong(pm), 0, None)
for i in range(43):
    x.XSetForeground(ctypes.c_void_p(d), ctypes.c_void_p(gc), ctypes.c_ulong(((30 + i * 5) << 16) | ((200 - i * 4) << 8) | (60 + (i * 37) % 160)))
    x.XFillRectangle(ctypes.c_void_p(d), ctypes.c_ulong(pm), ctypes.c_void_p(gc), i * 80, 0, 80, H)
x.XSetWindowBackgroundPixmap(ctypes.c_void_p(d), ctypes.c_ulong(root), ctypes.c_ulong(pm)); x.XClearWindow(ctypes.c_void_p(d), ctypes.c_ulong(root))
val = (ctypes.c_ulong * 1)(pm)
for name in (b"_XROOTPMAP_ID", b"ESETROOT_PMAP_ID"):
    a = x.XInternAtom(ctypes.c_void_p(d), name, 0)
    x.XChangeProperty(ctypes.c_void_p(d), ctypes.c_ulong(root), ctypes.c_ulong(a), ctypes.c_ulong(20), 32, 0, ctypes.byref(val), 1)  # 20 = XA_PIXMAP
x.XSync(ctypes.c_void_p(d), 0); print("ready", flush=True)
time.sleep(100000)
""")
    procs.append(subprocess.Popen([sys.executable, str(wall)], env=env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL))
    procs[-1].stdout.readline()
    procs.append(subprocess.Popen(["picom", "--backend", "xrender", "--config", "/dev/null"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
    time.sleep(1.0)

    salt = [0]
    def config(items, enabled=True):
        salt[0] += 1
        cfg = {"theme": {"name": THEME, "mode": "dark", "opacity": OPACITY, "radius": 24.0},
               "widgets": {"enabled": enabled, "items": [{"use": u, "place": "desktop", "anchor": "top-right", "x": -32, "y": 64 + 160 * i} for i, u in enumerate(items)]}}
        p = tmp / "cfg" / "decebal.json"
        p.write_text(json.dumps(cfg)); t = time.time() + salt[0]; os.utime(p, (t, t))  # a new stamp every time: the host rebuilds

    def run(*a):
        return subprocess.run(a, env=env, capture_output=True, text=True).stdout
    def windows():
        return run("xdotool", "search", "--name", "^decwidgets: ").split()
    def shot(name):
        p = OUT / f"{name}.png"; subprocess.run(["import", "-window", "root", str(p)], env=env, check=True); return Image.open(p).convert("RGB")
    def card_box(title):
        for w in windows():
            if run("xdotool", "getwindowname", w).strip() == f"decwidgets: {title}":
                g = dict(l.split("=") for l in run("xdotool", "getwindowgeometry", "--shell", w).split() if "=" in l)
                return int(g["X"]), int(g["Y"]), int(g["WIDTH"]), int(g["HEIGHT"])
        return None

    media = ["builtin:clock-card", "builtin:now-playing-mini"]
    config(media)
    log = (OUT / "decwidgets.log").open("w")
    host = subprocess.Popen([str(BIN), "daemon"], env=env, stdout=log, stderr=log)
    procs.append(host)
    for _ in range(100):
        if len(windows()) >= 2:
            break
        time.sleep(0.1)
    check("host shows both cards", len(windows()) >= 2, f"{len(windows())} windows")
    player = subprocess.Popen([str(FAKE), "test", "First Song"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    procs.append(player)
    time.sleep(2.0)

    cpu, hot, nthreads = sample(host.pid, 6)
    base_threads = nthreads
    check("idle with a media player: low CPU", cpu < 15 and not hot, f"{cpu:.1f}% threads={nthreads} hot={hot}")
    box = card_box("Now playing")
    check("now-playing card exists", box is not None, str(box))
    before = shot("media-first").crop((box[0], box[1], box[0] + box[2], box[1] + box[3]))

    # ---- repeated rebuilds (theme / wallpaper / colour change)
    for i in range(1, REBUILDS + 1):
        config(media)
        time.sleep(2.0)
        cpu, hot, nt = sample(host.pid, 5)
        check(f"after rebuild {i}: CPU low, no hot thread, no extra thread", cpu < 15 and not hot and nt <= base_threads, f"{cpu:.1f}% threads={nt} (baseline {base_threads}) hot={hot}")

    # ---- real media update + real mouse clicks
    player.terminate(); player.wait(timeout=5)
    player = subprocess.Popen([str(FAKE), "test", "Second Song, a longer title"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    procs.append(player)
    time.sleep(2.5)
    box = card_box("Now playing")
    after = shot("media-second").crop((box[0], box[1], box[0] + box[2], box[1] + box[3]))
    check("a new player title repaints the card", ImageChops.difference(before, after).getbbox() is not None)
    bx, by, bw, bh = box
    y = by + bh // 2
    for name, x in (("prev", bx + bw - 12 - 30 - 10 - 36 - 10 - 15), ("play-pause", bx + bw - 12 - 30 - 10 - 18), ("next", bx + bw - 12 - 15)):
        calls.unlink(missing_ok=True)
        run("xdotool", "mousemove", str(x), str(y), "click", "1")
        for _ in range(40):
            if calls.exists():
                break
            time.sleep(0.1)
        got = calls.read_text().strip() if calls.exists() else ""
        want = {"prev": "previous", "play-pause": "play-pause", "next": "next"}[name]
        check(f"real click on {name} reaches playerctl", got == want, f"got {got!r}")

    # ---- data groups change: the old source must really stop, the new one must work
    config(media + ["builtin:resource-overview"])
    time.sleep(2.5)
    cpu, hot, nt = sample(host.pid, 5)
    check("new data groups: old workers ended (no hot thread), CPU low", not hot and cpu < 25, f"{cpu:.1f}% threads={nt} hot={hot}")
    for _ in range(ALTERNATIONS):
        config(media + ["builtin:resource-overview"]); time.sleep(1.5)
        config(media); time.sleep(1.5)
    cpu, hot, nt = sample(host.pid, 5)
    check("alternating group sets 6x: no leaked workers", not hot and nt <= base_threads + 1, f"{cpu:.1f}% threads={nt} baseline={base_threads} hot={hot}")
    box = card_box("Now playing")
    calls.unlink(missing_ok=True)
    run("xdotool", "mousemove", str(box[0] + box[2] - 27), str(box[1] + box[3] // 2), "click", "1")
    time.sleep(1.0)
    check("media click still works after source replacement", calls.exists() and calls.read_text().strip() == "next")

    # ---- glass: what really sits under a card is the wallpaper and a desktop-type (live wallpaper) window that keeps changing.
    # That drives the native sampler's damage -> capture -> blur -> repaint path continuously.
    live = tmp / "livewall.py"
    live.write_text(r"""
import ctypes, ctypes.util, sys, time
x = ctypes.CDLL(ctypes.util.find_library("X11"))
x.XOpenDisplay.restype = ctypes.c_void_p; x.XDefaultRootWindow.restype = ctypes.c_ulong; x.XCreateSimpleWindow.restype = ctypes.c_ulong; x.XInternAtom.restype = ctypes.c_ulong
D = ctypes.c_void_p(x.XOpenDisplay(None)); root = x.XDefaultRootWindow(D)
w = x.XCreateSimpleWindow(D, ctypes.c_ulong(root), 2900, 100, 540, 520, 0, 0, 0)
t = (ctypes.c_ulong * 1)(x.XInternAtom(D, b"_NET_WM_WINDOW_TYPE_DESKTOP", 0))
x.XChangeProperty(D, ctypes.c_ulong(w), ctypes.c_ulong(x.XInternAtom(D, b"_NET_WM_WINDOW_TYPE", 0)), ctypes.c_ulong(4), 32, 0, ctypes.byref(t), 1)
x.XMapWindow(D, ctypes.c_ulong(w)); x.XSync(D, 0); print("ready", flush=True)
step, every = 0, float(sys.argv[1])
while True:
    x.XSetWindowBackground(D, ctypes.c_ulong(w), ctypes.c_ulong((((step * 53) % 200 + 40) << 16) | (((step * 97) % 200 + 40) << 8) | ((step * 31) % 200 + 40))); x.XClearWindow(D, ctypes.c_ulong(w)); x.XFlush(D)
    step += 1; time.sleep(every)
""")
    bgp = subprocess.Popen([sys.executable, str(live), "0.1"], env=env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL); procs.append(bgp)
    bgp.stdout.readline()
    config(media)  # a real session has its wallpaper window before the cards: rebuild so the cards are stacked above it
    time.sleep(3.0)
    cpu, hot, nt = sample(host.pid, 6)
    check("live wallpaper window changing 10x/s under the cards: no spinning thread, bounded CPU", cpu < 60 and not hot, f"{cpu:.1f}% threads={nt} hot={hot}")
    wid = next(w for w in windows() if run("xdotool", "getwindowname", w).strip() == "decwidgets: Now playing")
    def card_pixels(name):
        p = OUT / f"{name}.png"; subprocess.run(["import", "-window", wid, str(p)], env=env, check=True); return Image.open(p).convert("RGB")
    shot("root-with-live-wallpaper-window")
    shots = []
    for k in range(4):
        shots.append(card_pixels(f"glass-card-{k}")); time.sleep(0.45)
    changes = sum(1 for a, b in zip(shots, shots[1:]) if ImageChops.difference(a, b).getbbox() is not None)
    if OPACITY < 1.0:
        check("glass backdrop follows the changing window under the card", changes >= 2, f"theme {THEME}, opacity {OPACITY}, {changes}/3 repaints differ; hints: {' '.join(l.strip() for l in run('xprop', '-id', wid).splitlines() if 'DECEBAL_GLASS_NATIVE' in l)}")
    bgp.terminate(); bgp.wait(timeout=5)

    # ---- no cards: every worker must be gone
    config([], enabled=False)
    time.sleep(2.5)
    cpu, hot, nt = sample(host.pid, 5)
    check("widgets disabled: idle process, only the main and theme threads left", cpu < 3 and not hot and nt <= 3, f"{cpu:.1f}% threads={nt} hot={hot}")
    config(media); time.sleep(2.5)
    cpu, hot, nt = sample(host.pid, 5)
    check("re-enabled after disabling: healthy again", cpu < 15 and not hot and nt <= base_threads, f"{cpu:.1f}% threads={nt}")
finally:
    for p in reversed(procs):
        try: p.terminate()
        except Exception: pass
    for p in procs:
        try: p.wait(timeout=5)
        except Exception:
            try: p.kill()
            except Exception: pass
    shutil.rmtree(tmp, ignore_errors=True)
(OUT / "checks.json").write_text(json.dumps(results, indent=2))
failed = [r for r in results if not r["ok"]]
print(f"\n{len(results) - len(failed)}/{len(results)} checks passed; evidence in {OUT}")
sys.exit(1 if failed else 0)
