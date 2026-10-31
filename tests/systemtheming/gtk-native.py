#!/usr/bin/env python3
"""Real GTK 3 / GTK 4 / libadwaita / Nautilus pixels for `decthemectl apply-system --only gtk`.

    DECTHEMECTL=target/debug/decthemectl python3 tests/systemtheming/gtk-native.py OUT_DIR \
        [--themes nord,macos|all] [--modes dark,light] [--scales 1,2] [--kits gtk3,gtk4,adw,files] [--states] [--live]

OUT_DIR must not exist (evidence is never overwritten or deleted). Everything runs privately: HOME/XDG_* below OUT_DIR,
GSettings in a keyfile backend under the private config home, a private dbus-daemon with no activatable services, and an
owned Xvfb on a reserved display >= 700 whose lock pid and both sockets are verified before any client or input
(tests/e2e/gnome_xvfb.py). No window manager runs, so focus and backdrop are plain X input focus.

For every theme/mode/scale each probe app (tests/systemtheming/gtk_probe.py) is launched after the apply, captured, and
checked against the palette `decthemectl system palette --json` reports:
  * surfaces: window, header bar, sidebar, selected sidebar row, entry, buttons (normal / suggested / destructive) carry
    the role colours (exact within a small tolerance);
  * readability: text measured from the pixels (body, dim, button and selected-row labels) meets contrast floors;
  * --states: hover changes the normal button, a disabled button is distinct but not invisible, keyboard focus draws an
    accent ring on the entry, clicks/toggles arrive in the app (event log) and change the switch, and backdrop (focus
    moved to another window) shows the inactive header bar;
  * --live: apps started on theme A keep running while theme B is applied; each is classified against B's roles as live / stale /
    other (same pid and window proven, started on A), and a reopened process is checked separately (that proves the new palette
    reaches new processes, never a live change). --strict-live gtk3,adw,... fails unless those kits change live.
  * --self-test: failure-path regressions of the harness itself (bounded helper reads, group + token cleanup), no X or GTK.
Every child is its own process-group leader carrying a private run token; cleanup signals only token-carrying members of those
groups, and startup of the bus, Xvfb and focus helper runs inside the same try/finally as the cases.
Results go to OUT_DIR/results.json; screenshots per case to OUT_DIR/<case>/.
"""
import argparse
import json
import os
import pathlib
import select
import signal
import subprocess
import sys
import time

from PIL import Image

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(ROOT / "tests/e2e"))
import gnome_xvfb  # noqa: E402

ALL_THEMES = sorted(p.name for p in (ROOT / "themes").iterdir() if p.is_dir())

RUN_TOKEN_KEY = "DECEBAL_GTK_QA_RUN"


def bounded_readline(proc, timeout):
    """One line from a child's stdout, or None after `timeout` seconds (a helper that never answers cannot hang the run)."""
    deadline = time.monotonic() + timeout
    data = bytearray()
    while len(data) < 4096:
        left = deadline - time.monotonic()
        if left <= 0 or not select.select([proc.stdout], [], [], left)[0]:
            return None
        byte = os.read(proc.stdout.fileno(), 1)
        if not byte:
            return None
        data.extend(byte)
        if byte == b"\n":
            return data.decode("utf-8", errors="replace")
    raise RuntimeError("helper reply exceeds 4096 bytes")


def group_members(pgid, token):
    """Live processes in process group `pgid` that carry this run's private token in their environment. Membership is proved
    from /proc (group id and environment), never guessed from process names."""
    found = []
    for entry in os.listdir("/proc"):
        if not entry.isdigit():
            continue
        try:
            stat = pathlib.Path(f"/proc/{entry}/stat").read_text()
            if int(stat[stat.rfind(")") + 2 :].split()[2]) != pgid:
                continue
            if f"{RUN_TOKEN_KEY}={token}".encode() in pathlib.Path(f"/proc/{entry}/environ").read_bytes().split(b"\0"):
                found.append(int(entry))
        except (OSError, ValueError, IndexError):
            continue
    return found


class Session:
    """Every process this run starts: each is a session/group leader (start_new_session) carrying a private token, so cleanup can stop
    it and its descendants without touching anything else."""

    def __init__(self, env):
        self.token = f"{os.getpid()}-{time.time_ns()}"
        self.env = dict(env, **{RUN_TOKEN_KEY: self.token})
        self.owned = []

    def spawn(self, argv, log, extra=None, pipes=False):
        e = dict(self.env, **(extra or {}))
        io = dict(stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True) if pipes else dict(stdout=open(log, "w"))
        p = subprocess.Popen(argv, env=e, stderr=open(log, "a") if pipes else subprocess.STDOUT, start_new_session=True, **io)
        self.owned.append(p)
        return p

    def stop(self, p, grace=4.0):
        """SIGTERM then SIGKILL to the token-carrying members of `p`'s own group (p included), each wait bounded."""
        for sig in (signal.SIGTERM, signal.SIGKILL):
            members = group_members(p.pid, self.token)
            if not members and p.poll() is not None:
                break
            for pid in members:
                try:
                    fd = os.pidfd_open(pid)
                    try:
                        # Validate ownership after pinning; a reused numeric PID is never signalled.
                        if pid in group_members(p.pid, self.token):
                            signal.pidfd_send_signal(fd, sig)
                    finally:
                        os.close(fd)
                except ProcessLookupError:
                    pass
            deadline = time.monotonic() + grace
            while time.monotonic() < deadline and (p.poll() is None or group_members(p.pid, self.token)):
                time.sleep(0.05)
        try:
            p.wait(timeout=grace)
        except subprocess.TimeoutExpired:
            pass
        if p in self.owned and p.poll() is not None and not group_members(p.pid, self.token):
            self.owned.remove(p)

    def cleanup(self):
        for p in reversed(list(self.owned)):
            self.stop(p)

    def leftovers(self):
        return [pid for p in self.owned for pid in group_members(p.pid, self.token)]


def self_test():
    """Failure-path regressions with no X server, GTK or decthemectl: bounded helper reads, descendant cleanup by group + token,
    foreign processes left alone, startup failure after the first child still cleans up."""
    problems = []
    base = {k: v for k, v in os.environ.items() if k in ("PATH", "LANG")}
    s = Session(base)
    tmp = pathlib.Path(os.environ.get("TMPDIR", "/var/tmp"))
    log = tmp / f"gtk-native-selftest-{s.token}.log"
    silent = s.spawn([sys.executable, "-I", "-c", "import time; time.sleep(30)"], log, pipes=True)
    t0 = time.monotonic()
    if bounded_readline(silent, 0.5) is not None or time.monotonic() - t0 > 3:
        problems.append("bounded_readline did not time out")
    partial = s.spawn([sys.executable, "-I", "-c", "import sys,time;sys.stdout.write('partial');sys.stdout.flush();time.sleep(30)"], log, pipes=True)
    t0 = time.monotonic()
    if bounded_readline(partial, 0.5) is not None or time.monotonic() - t0 > 3:
        problems.append("partial helper line bypassed the timeout")
    tree = s.spawn(["sh", "-c", "sleep 30 & sleep 30 & wait"], log)
    time.sleep(0.3)
    if len(group_members(tree.pid, s.token)) < 3:
        problems.append(f"group members not found: {group_members(tree.pid, s.token)}")
    foreign = subprocess.Popen(["sleep", "30"], env=base, start_new_session=True)  # no token: must survive cleanup
    try:
        try:
            raise RuntimeError("simulated helper startup failure after the first child")
        except RuntimeError:
            pass
        finally:
            s.cleanup()
        if s.leftovers() or group_members(tree.pid, s.token) or group_members(silent.pid, s.token):
            problems.append("owned descendants survived cleanup")
        if foreign.poll() is not None:
            problems.append("a process without the run token was signalled")
    finally:
        foreign.kill()
        foreign.wait()
    try:
        log.unlink()
    except OSError:
        pass
    print("self-test:", "PASS" if not problems else "FAIL " + "; ".join(problems))
    return 1 if problems else 0


ap = argparse.ArgumentParser()
ap.add_argument("out", nargs="?")
ap.add_argument("--themes", default="nord,macos,windows11,high-contrast,solarized,synthwave")
ap.add_argument("--modes", default="dark,light")
ap.add_argument("--scales", default="1")
ap.add_argument("--kits", default="gtk3,gtk4,adw,files")
ap.add_argument("--states", action="store_true")
ap.add_argument("--live", action="store_true", help="keep apps running across a theme change and classify what each one does")
ap.add_argument("--strict-live", default="", help="comma list of kits that must change live (fails otherwise); implies --live")
ap.add_argument("--keep-going", action="store_true", help="record failures and continue")
ap.add_argument("--self-test", action="store_true", help="failure-path regressions only (no X, GTK or decthemectl)")
args = ap.parse_args()
if args.self_test:
    sys.exit(self_test())
if not args.out:
    ap.error("OUT_DIR is required")
strict_live = [k for k in args.strict_live.split(",") if k]
if strict_live:
    args.live = True

out = pathlib.Path(args.out)
if out.exists():
    sys.exit(f"{out} exists; evidence directories are never reused")
out.mkdir(parents=True)
ctl = os.environ.get("DECTHEMECTL", str(ROOT / "target/debug/decthemectl"))
# A baseline binary without `system palette` can be compared using a newer one for the roles only.
palette_ctl = os.environ.get("PALETTE_CTL", ctl)
themes = ALL_THEMES if args.themes == "all" else args.themes.split(",")
modes = args.modes.split(",")
scales = [int(s) for s in args.scales.split(",")]
kits = args.kits.split(",")

# ---------------------------------------------------------------- private session
priv = out / "p"
for d in ("home", "cfg", "data", "state", "cache", "run", "bus-services", "home/Files QA"):
    (priv / d).mkdir(parents=True)
os.chmod(priv / "run", 0o700)
for n in ("Alpha report.txt", "Budget.ods", "Notes.md"):
    (priv / "home/Files QA" / n).write_text("decebal gtk qa\n")
(priv / "home/Files QA/Projects").mkdir()

base_env = {k: v for k, v in os.environ.items() if k in ("PATH", "LANG", "LC_ALL", "TERM", "USER", "LOGNAME", "SHELL")}
base_env.update(
    HOME=str(priv / "home"), XDG_CONFIG_HOME=str(priv / "cfg"), XDG_DATA_HOME=str(priv / "data"), XDG_STATE_HOME=str(priv / "state"),
    XDG_CACHE_HOME=str(priv / "cache"), XDG_RUNTIME_DIR=str(priv / "run"), DECEBAL_CONFIG=str(priv / "cfg/decebal"),
    GSETTINGS_BACKEND="keyfile", GDK_BACKEND="x11", GSK_RENDERER="cairo", NO_AT_BRIDGE="1", GTK_A11Y="none",
    ADW_DISABLE_PORTAL="1", GDK_DEBUG="no-portals", GTK_USE_PORTAL="0", LIBGL_ALWAYS_SOFTWARE="1", DECEBAL_SYSTEM_AUTOMATIC="0",
    RDWMCTL="/bin/false", DECEBAL_BACKEND="x11",
)
(priv / "cfg/decebal").mkdir()
S = Session(base_env)
(out / "session.json").write_text(json.dumps({"pid": os.getpid(), "token": S.token}))
env = S.env  # every child, and every helper command, carries the run token
xvfb = display = wm = None


def spawn(argv, log, extra=None):
    return S.spawn(argv, log, extra)


def stop(p):
    S.stop(p)


def cleanup(*_):
    try:
        if xvfb is not None and xvfb.poll() is None:
            # The product manager may be a detached child of apply-system, outside Session's groups.
            # Stop only its marked owner on our re-proved private display before taking the server down.
            gnome_xvfb.verify_owned(xvfb, display)
            result = subprocess.run([ctl, "system", "xsettings", "stop", "--display", display], env=env, capture_output=True, text=True, timeout=3)
            (out / "xsettings-stop.json").write_text(json.dumps({"exit": result.returncode, "stdout": result.stdout, "stderr": result.stderr}))
    except Exception as error:
        (out / "xsettings-stop-error.txt").write_text(str(error))
    finally:
        S.cleanup()


def _terminated(signum, _frame):
    raise SystemExit(128 + signum)  # unwinds through the main try/finally, which cleans up


signal.signal(signal.SIGTERM, _terminated)
signal.signal(signal.SIGINT, _terminated)


def start_session():
    """Bus, owned Xvfb and the focus-hint helper. Called inside the main try: any failure here still cleans up what started."""
    global xvfb, display, wm
    conf = priv / "bus.conf"
    conf.write_text(f"""<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig><type>session</type><listen>unix:path={priv}/run/bus</listen><auth>EXTERNAL</auth><servicedir>{priv}/bus-services</servicedir>
<policy context="default"><allow send_destination="*" eavesdrop="true"/><allow eavesdrop="true"/><allow own="*"/></policy></busconfig>
""")
    bus = spawn(["dbus-daemon", "--nofork", "--config-file", str(conf)], out / "bus.log")
    deadline = time.monotonic() + 5
    while not (priv / "run/bus").is_socket():
        if bus.poll() is not None or time.monotonic() > deadline:
            raise RuntimeError("private dbus-daemon did not start; see bus.log")
        time.sleep(0.05)
    env["DBUS_SESSION_BUS_ADDRESS"] = f"unix:path={priv}/run/bus"
    screen = "2400x1600x24" if max(scales) > 1 else "1400x1000x24"
    display = gnome_xvfb.reserve_display()
    xvfb = spawn(["Xvfb", display, "-screen", "0", screen, "-nolisten", "tcp", "-noreset"], out / "xvfb.log")
    proof = gnome_xvfb.verify_owned(xvfb, display)
    env["DISPLAY"] = display
    (out / "display-proof.json").write_text(json.dumps(proof))
    # GDK reports :backdrop only under a window manager advertising _NET_WM_STATE_FOCUSED. This fixture has no window manager, so a
    # tiny helper claims exactly that hint on the owned display and toggles it per window. That is a synthetic stand-in: it proves
    # the GTK themes' :backdrop styles, not any window manager's focus handling (rdwm's own support is proved by rdwm's tests).
    wm = S.spawn([sys.executable, "-I", str(HERE / "ewmh_focus.py")], out / "ewmh.log", pipes=True)
    if (bounded_readline(wm, 10) or "").strip() != "ready":
        raise RuntimeError("EWMH focus helper did not become ready; see ewmh.log")


def hint(cmd, xid):
    wm.stdin.write(f"{cmd} {xid}\n")
    wm.stdin.flush()
    answer = bounded_readline(wm, 5)
    if (answer or "").strip() != "ok":
        raise RuntimeError(f"EWMH focus helper did not acknowledge {cmd} {xid}: {answer!r}")


def verify_display():
    """Re-proved before every burst of synthetic input."""
    gnome_xvfb.verify_owned(xvfb, display)


def run(argv, **kw):
    kw.setdefault("timeout", 120)
    return subprocess.run(argv, env=env, capture_output=True, text=True, **kw)


# ---------------------------------------------------------------- colour helpers
def lum(c):
    def ch(v):
        v /= 255
        return v / 12.92 if v <= 0.03928 else ((v + 0.055) / 1.055) ** 2.4

    return 0.2126 * ch(c[0]) + 0.7152 * ch(c[1]) + 0.0722 * ch(c[2])


def contrast(a, b):
    la, lb = sorted((lum(a), lum(b)), reverse=True)
    return (la + 0.05) / (lb + 0.05)


def hexrgb(h):
    h = h.lstrip("#")
    return tuple(int(h[i : i + 2], 16) for i in (0, 2, 4))


def near(a, b, tol=6):
    return all(abs(x - y) <= tol for x, y in zip(a, b))


def crop(img, box, scale, inset=3):
    x, y, w, h = (round(v * scale) for v in box)
    i = inset * scale
    return img.crop((x + i, y + i, max(x + i + 1, x + w - i), max(y + i + 1, y + h - i)))


def mode_colour(region):
    colours = region.getcolors(region.width * region.height) or []
    return max(colours)[1] if colours else (0, 0, 0)


def ink(region, bg):
    """The pixel farthest in contrast from the background: the text/glyph colour as drawn."""
    best, ratio = bg, 1.0
    for _, c in region.getcolors(region.width * region.height) or []:
        r = contrast(c, bg)
        if r > ratio:
            best, ratio = c, r
    return best, ratio


# ---------------------------------------------------------------- app control
def apply(theme, mode, extra_cfg=None):
    cfg = {"theme": {"name": theme, "mode": mode}, "system": {"enabled": True, "targets": {"omarchy": False, "browser": False, "editors": False, "hooks": False, "qt": False, "kvantum": False, "x11": False, "terminals": False}}}
    if extra_cfg:
        cfg["system"].update(extra_cfg)
    (priv / "cfg/decebal/decebal.json").write_text(json.dumps(cfg))
    r = run([ctl, "apply-system", "--only", "gtk", "--json"])
    if r.returncode:
        raise AssertionError(f"apply-system failed: {r.stderr}")
    pal = run([palette_ctl, "system", "palette", "--json"])
    if pal.returncode:
        raise AssertionError(f"palette failed: {pal.stderr}")
    return json.loads(r.stdout or "{}"), json.loads(pal.stdout)


launch_sequence = 0


def launch(kit, title, case, scale):
    global launch_sequence
    launch_sequence += 1
    extra = {"GDK_SCALE": str(scale)} if scale != 1 else {}
    js = case / f"{kit}-{launch_sequence}.json"
    log = case / f"{kit}-{launch_sequence}.log"
    if kit == "files":
        p = spawn(["nautilus", "--new-window", str(priv / "home/Files QA")], log, extra)
    else:
        p = spawn([sys.executable, "-I", str(HERE / "gtk_probe.py"), kit, title, str(js)], log, extra)
    deadline = time.time() + 25
    if kit == "files":
        while time.time() < deadline:
            r = run(["xdotool", "search", "--onlyvisible", "--name", "Files QA"])
            if r.returncode == 0 and r.stdout.split():
                xid = int(r.stdout.split()[-1])
                hint("focus", xid)
                time.sleep(2.5)
                return p, {"xid": xid, "scale": scale, "boxes": {}}
            time.sleep(0.2)
        raise AssertionError("Files window did not appear")
    while time.time() < deadline:
        if js.exists():
            info = json.loads(js.read_text())
            info["probe_json"] = str(js)
            hint("focus", info["xid"])
            time.sleep(0.4)
            return p, info
        if p.poll() is not None:
            raise AssertionError(f"{kit} probe exited: {log.read_text()[-2000:]}")
        time.sleep(0.1)
    raise AssertionError(f"{kit} probe did not report")


def shot(info, path):
    verify_display()
    # Xvfb has no compositor/backing pixmap for an obscured GTK3 window.
    # Raise it and allow Expose to repaint before sampling; do not change its synthetic focus hint.
    xdo("windowraise", info["xid"])
    time.sleep(0.2)
    r = run(["import", "-window", str(info["xid"]), str(path)], timeout=10)
    if r.returncode:
        raise AssertionError(f"capture failed: {r.stderr}")
    return Image.open(path).convert("RGB")


def xdo(*a):
    verify_display()
    r = run(["xdotool", *[str(x) for x in a]])
    if r.returncode:
        raise AssertionError(f"xdotool {a}: {r.stderr}")


def window_origin(xid):
    r = run(["xdotool", "getwindowgeometry", "--shell", str(xid)])
    vals = dict(line.split("=", 1) for line in r.stdout.splitlines() if "=" in line)
    return int(vals["X"]), int(vals["Y"])


def centre(info, name):
    x, y, w, h = info["boxes"][name]
    ox, oy = window_origin(info["xid"])
    s = info["scale"]
    return ox + round((x + w / 2) * s), oy + round((y + h / 2) * s)


# ---------------------------------------------------------------- checks
results = []
failures = []


def check(case_id, ok, what, detail):
    results.append({"case": case_id, "ok": bool(ok), "check": what, "detail": detail})
    if not ok:
        failures.append(f"{case_id}: {what}: {detail}")
        print(f"FAIL {case_id}: {what}: {detail}", flush=True)


def surfaces(case_id, kit, img, info, pal):
    s, b = info["scale"], info["boxes"]
    P = {k: hexrgb(v) for k, v in pal.items() if isinstance(v, str) and v.startswith("#")}
    sample = {name: mode_colour(crop(img, b[name], s)) for name in b if name != "window"}
    win_bg = mode_colour(crop(img, b["label"], s, 0))
    # libadwaita draws ToolbarView top bars flat over the window and tints destructive buttons (its design, kept); GTK 3 and plain
    # GTK 4 header bars carry the theme's titlebar role and destructive buttons are filled.
    expect = {"label": ("bg", win_bg), "headerbar": ("bg" if kit == "adw" else "titlebar", sample.get("headerbar"))}
    expect["sidebar_row"] = ("sidebar", sample.get("sidebar_row"))
    expect["btn_suggested"] = ("accent", sample.get("btn_suggested"))
    if kit != "adw":
        expect["btn_destructive"] = ("red", sample.get("btn_destructive"))
    for name, (role, got) in expect.items():
        if got is None:
            continue
        check(case_id, near(got, P[role], 8), f"{name} is the {role} colour", {"got": "#%02x%02x%02x" % got, "want": pal[role]})
    sel = sample.get("sidebar_selected")
    if sel is not None:
        check(case_id, contrast(sel, P["sidebar"]) >= 1.15 or near(sel, P["accent"], 10), "selected sidebar row stands out", {"row": "#%02x%02x%02x" % sel, "sidebar": pal["sidebar"]})
    # 4.5:1 for text on the window, views, controls and sidebars; 3:1 for short labels on accent / destructive fills (libadwaita's
    # own default accent with white text is 3.8:1).
    selected_floor = 3.0 if kit != "adw" else 4.5
    floors = {"label": 4.5, "dim": 4.5, "btn": 4.5, "btn_suggested": 3.0, "btn_destructive": 3.0 if kit != "adw" else 4.5, "entry": 4.5, "sidebar_row": 4.5, "sidebar_selected": selected_floor, "header_btn": 4.5}
    for name, floor in floors.items():
        if name not in b:
            continue
        reg = crop(img, b[name], s)
        bg = mode_colour(reg)
        c, ratio = ink(reg, bg)
        check(case_id, ratio >= floor, f"{name} text is readable", {"ratio": round(ratio, 2), "floor": floor, "bg": "#%02x%02x%02x" % bg, "ink": "#%02x%02x%02x" % c})
    if "btn_disabled" in b:
        reg = crop(img, b["btn_disabled"], s)
        bg = mode_colour(reg)
        _, ratio = ink(reg, bg)
        _, normal = ink(crop(img, b["btn"], s), mode_colour(crop(img, b["btn"], s)))
        check(case_id, 1.4 <= ratio < normal, "disabled button is legible but subdued", {"disabled": round(ratio, 2), "normal": round(normal, 2)})
    return sample


def states(case_id, kit, case, info, base, pal, title):
    s, b = info["scale"], info["boxes"]
    P = {k: hexrgb(v) for k, v in pal.items() if isinstance(v, str) and v.startswith("#")}
    ev = pathlib.Path(info["probe_json"] + ".events")
    xdo("windowfocus", "--sync", info["xid"])
    # hover
    x, y = centre(info, "btn")
    xdo("mousemove", "--sync", x, y)
    time.sleep(0.5)
    img = shot(info, case / f"{kit}-hover.png")
    hov = mode_colour(crop(img, b["btn"], s))
    rest = mode_colour(crop(base, b["btn"], s))
    check(case_id, hov != rest, "hover changes the normal button", {"rest": "#%02x%02x%02x" % rest, "hover": "#%02x%02x%02x" % hov})
    _, ratio = ink(crop(img, b["btn"], s), hov)
    check(case_id, ratio >= 4.5, "hovered button text readable", {"ratio": round(ratio, 2)})
    # action: a real click arrives and the switch toggles visibly
    xdo("click", "1")
    x, y = centre(info, "switch")
    sw_before = mode_colour(crop(base, b["switch"], s, 1))
    xdo("mousemove", "--sync", x, y)
    xdo("click", "1")
    time.sleep(0.6)
    img = shot(info, case / f"{kit}-switch.png")
    sw_after = mode_colour(crop(img, b["switch"], s, 1))
    log = ev.read_text() if ev.exists() else ""
    check(case_id, "clicked:btn" in log, "button click reaches the app", log.split())
    check(case_id, "toggled:switch:0" in log, "switch toggle reaches the app", log.split())
    check(case_id, sw_before != sw_after, "switch off state is drawn differently", {"on": "#%02x%02x%02x" % sw_before, "off": "#%02x%02x%02x" % sw_after})
    # keyboard focus on the entry: an accent focus indication
    x, y = centre(info, "entry")
    xdo("mousemove", "--sync", x, y)
    xdo("click", "1")
    xdo("mousemove", "--sync", 1, 1)
    time.sleep(0.5)
    img = shot(info, case / f"{kit}-focus.png")
    ex, ey, ew, eh = info["boxes"]["entry"]
    ring = img.crop((round((ex - 3) * s), round((ey - 3) * s), round((ex + ew + 3) * s), round((ey + eh + 3) * s)))
    # Rings are the accent, often mixed over the field (libadwaita: color-mix(accent 50%, transparent)): count pixels lying on the
    # accent-to-background line at 30 % accent or more.
    under = mode_colour(ring)

    def on_accent_line(c):
        for acc in (P["accent"], P["accent_on_bg"]):
            d = [a - b for a, b in zip(acc, under)]
            n = sum(x * x for x in d)
            if n < 300:
                continue
            t = sum((c[i] - under[i]) * d[i] for i in range(3)) / n
            if t >= 0.3 and all(abs(under[i] + t * d[i] - c[i]) <= 10 for i in range(3)):
                return True
        return False

    ring_px = sum(n for n, c in ring.getcolors(ring.width * ring.height) if on_accent_line(c))
    check(case_id, ring_px >= 40 * s, "focused entry shows an accent focus ring", {"accent_pixels": ring_px})
    # backdrop: the window manager says the window lost focus
    xdo("mousemove", "--sync", 1, 1)
    hint("blur", info["xid"])
    time.sleep(0.6)
    img = shot(info, case / f"{kit}-backdrop.png")
    hb = mode_colour(crop(img, b["headerbar"], s))
    if kit != "adw":
        check(case_id, near(hb, P["titlebar_inactive"], 10), "backdrop header bar uses the inactive titlebar colour", {"got": "#%02x%02x%02x" % hb, "want": pal["titlebar_inactive"]})
    reg = crop(img, b["headerbar"], s)
    _, ratio = ink(reg, mode_colour(reg))
    check(case_id, ratio >= 4.5, "backdrop header text stays readable", {"ratio": round(ratio, 2)})
    reg = crop(img, b["label"], s)
    _, ratio = ink(reg, mode_colour(reg))
    check(case_id, ratio >= 4.5, "backdrop body text stays readable", {"ratio": round(ratio, 2)})
    sel = mode_colour(crop(img, b["sidebar_selected"], s))
    reg = crop(img, b["sidebar_selected"], s)
    _, ratio = ink(reg, sel)
    check(case_id, ratio >= 3.0, "backdrop selected row stays readable", {"row": "#%02x%02x%02x" % sel, "ratio": round(ratio, 2)})
    hint("focus", info["xid"])


def files_checks(case_id, img, pal, scale):
    P = {k: hexrgb(v) for k, v in pal.items() if isinstance(v, str) and v.startswith("#")}
    total = img.width * img.height
    hist = img.getcolors(total)
    share = lambda c, tol=4: sum(n for n, x in hist if near(x, c, tol)) / total  # noqa: E731
    got = {r: round(share(P[r]), 3) for r in ("bg", "sidebar")}
    check(case_id, got["bg"] > 0.15, "Files content uses the window colour", got)
    check(case_id, got["sidebar"] > 0.05, "Files sidebar uses the sidebar colour", got)
    # Joined libadwaita bars may use titlebar, window or sidebar roles.
    # The Windows package deliberately has a titlebar distinct from its content.
    band = img.crop((8 * scale, 4 * scale, img.width - 8 * scale, 40 * scale))
    bh = band.getcolors(band.width * band.height)
    themed = sum(n for n, c in bh if any(near(c, P[role], 4) for role in ("titlebar", "bg", "sidebar"))) / (band.width * band.height)
    check(case_id, themed >= 0.25, "Files header band follows the theme", {"themed_header_share": round(themed, 3)})
    # The places sidebar: the left fifth below the header, its body text against its own background.
    side = img.crop((8, img.height // 5, img.width // 5, img.height * 3 // 5))
    bg = mode_colour(side)
    _, ratio = ink(side, bg)
    check(case_id, ratio >= 4.5, "Files sidebar labels readable", {"ratio": round(ratio, 2), "bg": "#%02x%02x%02x" % bg})


# ---------------------------------------------------------------- live acceptance
live = []

# What this fixture can deliver to a running process, so a stale result is reported against a stated expectation, never hidden.
# GTK 3 / GTK 4 on X11 learn a new gtk-theme-name only through XSETTINGS (none runs here; settings.ini is read at startup).
# libadwaita reads color-scheme from GSettings live (ADW_DISABLE_PORTAL: the keyfile backend notifies) but never re-reads user CSS.
LIVE_EXPECTATION = {
    "gtk3": "live requires an XSETTINGS provider; the product may start its own after apply (see apply report)",
    "gtk4": "live requires an XSETTINGS provider; the product may start its own after apply (see apply report)",
    "adw": "mode only at most: libadwaita follows color-scheme live but loads the user gtk.css palette only at startup",
    "files": "mode only at most: Files is libadwaita (same limit)",
}


def window_bg(kit, img, info):
    if kit == "files":
        return mode_colour(img.crop((img.width // 3, img.height // 3, img.width - 20, img.height - 20)))
    return mode_colour(crop(img, info["boxes"]["label"], info["scale"], 0))


def identity(kit, p, info):
    """The same process and window as before the change: our child is still running and its X window still exists."""
    if p.poll() is not None:
        return False, "process exited"
    if kit != "files" and info.get("pid") != p.pid:
        return False, f"probe pid {info.get('pid')} != child {p.pid}"
    r = run(["xdotool", "getwindowgeometry", str(info["xid"])])
    return (r.returncode == 0, "window exists" if r.returncode == 0 else "window gone")


def live_case(scale, a, b):
    """Apply theme A, start the apps, apply theme B while they run; classify each app against B's roles (live / stale / other) and
    separately prove a reopened process adopts B. Only `live` counts as a live change; strict kits must reach it."""
    case_id = f"live-{a[0]}-{a[1]}-to-{b[0]}-{b[1]}-x{scale}"
    case = out / case_id
    case.mkdir()
    _, pa = apply(*a)
    running = []
    try:
        for kit in kits:
            p, info = launch(kit, f"Decebal GTK QA {kit}", case, scale)
            img = shot(info, case / f"{kit}-before.png")
            running.append((kit, p, info, window_bg(kit, img, info)))
        _, pb = apply(*b)
        (case / "palettes.json").write_text(json.dumps({"before": pa, "after": pb}, indent=1))
        A, B = hexrgb(pa["bg"]), hexrgb(pb["bg"])
        time.sleep(3)
        for kit, p, info, before in running:
            same, why = identity(kit, p, info)
            img = shot(info, case / f"{kit}-after-live.png") if same else None
            after = window_bg(kit, img, info) if img else None
            if not same:
                outcome = "invalid"
            elif near(A, B, 10):
                outcome = "indistinguishable"
            elif not near(before, A, 10):
                outcome = "invalid"  # the app was not showing theme A to begin with: nothing to compare
            elif near(after, B, 8):
                outcome = "live"
            elif near(after, A, 8):
                outcome = "stale"
            else:
                outcome = "other"
            rec = {"case": case_id, "kit": kit, "outcome": outcome, "identity": why, "pid": p.pid, "xid": info["xid"], "before": "#%02x%02x%02x" % before, "after": after and "#%02x%02x%02x" % after, "want": pb["bg"], "fixture_expectation": LIVE_EXPECTATION.get(kit)}
            live.append(rec)
            print(f"live {case_id}/{kit}: {outcome} ({rec['before']} -> {rec['after']}, want {pb['bg']})", flush=True)
            if kit in strict_live:
                check(f"{case_id}/{kit}", outcome == "live", "running app changed live to the new theme (strict)", rec)
            check(f"{case_id}/{kit}", outcome != "invalid", "live comparison is valid (same process and window, started on theme A)", rec)
    finally:
        for _, p, _, _ in running:
            stop(p)
    # Reopening proves the new palette reaches new processes; it is recorded as such and never counted as a live change.
    for kit in kits:
        p, info = launch(kit, f"Decebal GTK QA {kit}", case, scale)
        try:
            bg = window_bg(kit, shot(info, case / f"{kit}-reopened.png"), info)
            check(f"{case_id}/{kit}", near(bg, hexrgb(pb["bg"]), 8), "a reopened process adopts the new theme (not a live change)", {"got": "#%02x%02x%02x" % bg, "want": pb["bg"]})
        finally:
            stop(p)


def live_pairs():
    """Consecutive themes in the first mode, then a mode flip of the first theme: each pair changes the window colour."""
    m = modes[0]
    pairs = [((themes[i], m), (themes[i + 1], m)) for i in range(len(themes) - 1)]
    if len(modes) > 1:
        pairs.append(((themes[0], modes[0]), (themes[0], modes[1])))
    return pairs


# ---------------------------------------------------------------- main loop
try:
    start_session()
    for scale in scales:
        for theme in themes:
            for mode in modes:
                case_id = f"{theme}-{mode}-x{scale}"
                case = out / case_id
                case.mkdir()
                try:
                    report, pal = apply(theme, mode)
                    (case / "palette.json").write_text(json.dumps(pal, indent=1))
                    (case / "apply.json").write_text(json.dumps(report, indent=1))
                    for kit in kits:
                        title = f"Decebal GTK QA {kit}"
                        p, info = launch(kit, title, case, scale)
                        try:
                            img = shot(info, case / f"{kit}.png")
                            if kit == "files":
                                files_checks(f"{case_id}/files", img, pal, scale)
                            else:
                                check(f"{case_id}/{kit}", info.get("scale") == scale, "toolkit runs at the requested scale", info.get("scale"))
                                surfaces(f"{case_id}/{kit}", kit, img, info, pal)
                                if args.states:
                                    states(f"{case_id}/{kit}", kit, case, info, img, pal, title)
                        finally:
                            stop(p)
                except Exception as e:  # noqa: BLE001 - recorded as a failed case, never hidden
                    check(case_id, False, "case ran", f"{type(e).__name__}: {e}")
                if failures and not args.keep_going:
                    raise SystemExit(1)
        if args.live:
            for a, b in live_pairs():
                try:
                    live_case(scale, a, b)
                except Exception as e:  # noqa: BLE001
                    check(f"live-{a}-{b}", False, "live case ran", f"{type(e).__name__}: {e}")
                if failures and not args.keep_going:
                    raise SystemExit(1)
finally:
    cleanup()
    leftovers = S.leftovers()
    if leftovers:
        failures.append({"case": "cleanup", "ok": False, "label": "owned processes survived cleanup", "detail": leftovers})
    (out / "results.json").write_text(json.dumps({"failures": failures, "results": results, "live": live, "strict_live": strict_live, "leftover_pids": leftovers}, indent=1))
    ok = sum(r["ok"] for r in results)
    counts = {o: sum(1 for r in live if r["outcome"] == o) for o in ("live", "stale", "other", "indistinguishable", "invalid")}
    print(f"{ok}/{len(results)} checks passed; {len(failures)} failures; live outcomes {counts if live else 'not requested'}; evidence {out}")
sys.exit(1 if failures else 0)
