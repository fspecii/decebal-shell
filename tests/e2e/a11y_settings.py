#!/usr/bin/env python3
"""Native AT-SPI2 acceptance test for decsettings. Private only.

Runs the real decsettings binary inside a private HOME/XDG tree, a private `dbus-run-session` (so the real
at-spi-bus-launcher and at-spi2-registryd are activated privately), and a private Xvfb on a free display, and drives
it as a screen reader would: GI Atspi (libatspi) plus raw Gio calls on the accessibility bus. Never pointer events.
The only synthetic input is XTEST keyboard input into the private Xvfb, used to prove that keyboard focus and AT
focus agree. Nothing here reads or changes the live session bus, display, input, config or processes.

    python3 -I tests/e2e/a11y_settings.py --bin target/debug/decsettings --evidence ~/.cache/decebal-a11y-evidence/run

Writes <evidence>/results.json (every check with its detail) and per-scenario logs and layout dumps.
"""
import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import time
import traceback
from pathlib import Path

HERE = Path(__file__).resolve()


def sha256(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


# ---------------------------------------------------------------- outer: private environment, then dbus-run-session


def outer(a):
    ev = Path(a.evidence).expanduser().resolve()
    ev.mkdir(parents=True, exist_ok=True)
    root = ev / "root"
    if root.exists():
        shutil.rmtree(root)
    for d in ["home", "cfg", "data", "cache", "state", "run", "tmp"]:
        (root / d).mkdir(parents=True)
    os.chmod(root / "run", 0o700)
    drop = ("DBUS_", "AT_SPI", "ATSPI_", "DECEBAL_", "DECSETTINGS_", "GTK_", "GDK_", "QT_", "XDG_", "WAYLAND_", "GSETTINGS_", "NO_AT_BRIDGE", "RDWM")
    env = {k: v for k, v in os.environ.items() if not k.startswith(drop) and k not in ("DISPLAY", "XAUTHORITY", "SESSION_MANAGER", "PULSE_SERVER", "DESKTOP_SESSION")}
    env.update(
        HOME=str(root / "home"),
        XDG_CONFIG_HOME=str(root / "cfg"),
        XDG_DATA_HOME=str(root / "data"),
        XDG_CACHE_HOME=str(root / "cache"),
        XDG_STATE_HOME=str(root / "state"),
        XDG_RUNTIME_DIR=str(root / "run"),
        TMPDIR=str(root / "tmp"),
        GSETTINGS_BACKEND="memory",
        PULSE_SERVER="unix:" + str(root / "no-audio"),
        DBUS_SYSTEM_BUS_ADDRESS="unix:path=" + str(root / "no-system-bus"),
        LANG="C.UTF-8",
    )
    cmd = ["dbus-run-session", "--", sys.executable, "-I", str(HERE), "--inner", "--bin", str(Path(a.bin).resolve()), "--evidence", str(ev)]
    if a.only:
        cmd += ["--only", a.only]
    log = open(ev / "session.log", "w")
    t0 = time.time()
    r = subprocess.run(cmd, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=a.timeout)
    log.close()
    res = json.loads((ev / "results.json").read_text()) if (ev / "results.json").exists() else {"passed": False, "error": "no results"}
    res["session_exit"] = r.returncode
    res["wall_s"] = round(time.time() - t0, 1)
    # Every process of the private session must be gone (dbus-run-session reaps the bus; we check our own markers).
    left = []
    for p in Path("/proc").iterdir():
        if p.name.isdigit():
            try:
                e = (p / "environ").read_bytes()
            except OSError:
                continue
            if str(root).encode() in e:
                left.append(int(p.name))
    res["leftover_private_processes"] = left
    for pid in left:
        # Provably ours (the private root is in their environment): never anything else.
        try:
            os.kill(pid, 15)
        except OSError:
            pass
    (ev / "results.json").write_text(json.dumps(res, indent=2) + "\n")
    print(json.dumps({k: res[k] for k in ("passed", "failed", "session_exit", "leftover_private_processes", "wall_s") if k in res}, indent=2))
    return 0 if res.get("passed") and not left else 1


# ---------------------------------------------------------------- inner: runs inside the private session bus


class Fail(Exception):
    pass


def inner(a):
    import gi

    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi, Gio, GLib

    ev = Path(a.evidence)
    root = ev / "root"
    results = {"checks": [], "scenarios": {}, "provenance": {}}
    checks = results["checks"]
    owned = []

    def check(name, ok, detail=None):
        checks.append({"name": name, "ok": bool(ok), "detail": detail})
        print(("PASS " if ok else "FAIL ") + name + (f" :: {detail}" if detail is not None else ""), flush=True)
        return bool(ok)

    def must(name, ok, detail=None):
        if not check(name, ok, detail):
            raise Fail(name)

    def wait(fn, why, t=8.0, step=0.05):
        end = time.time() + t
        last = None
        while time.time() < end:
            try:
                v = fn()
                if v:
                    return v
            except Exception as e:  # noqa: BLE001 - keep polling, report the last error
                last = e
            pump(step)
        raise Fail(f"timeout: {why} (last error: {last!r})")

    ctx = GLib.MainContext.default()

    def pump(t=0.0):
        end = time.time() + t
        while True:
            while ctx.pending():
                ctx.iteration(False)
            if time.time() >= end:
                return
            time.sleep(0.01)

    # Private Xvfb on a free display.
    num = next(n for n in range(700, 760) if not Path(f"/tmp/.X11-unix/X{n}").exists() and not Path(f"/tmp/.X{n}-lock").exists())
    display = f":{num}"
    xlog = open(ev / "xvfb.log", "w")
    xvfb = subprocess.Popen(["Xvfb", display, "-screen", "0", "1920x1200x24", "-nolisten", "tcp", "-noreset"], stdout=xlog, stderr=subprocess.STDOUT, start_new_session=True)
    owned.append(xvfb)
    os.environ["DISPLAY"] = display
    session = Gio.bus_get_sync(Gio.BusType.SESSION, None)

    def scall(dest, path, iface, member, params=None, rtype=None, flags=0):
        return session.call_sync(dest, path, iface, member, params, GLib.VariantType(rtype) if rtype else None, flags, 5000, None).unpack()

    def has_owner(name):
        return scall("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "NameHasOwner", GLib.Variant("(s)", (name,)), "(b)")[0]

    def set_enabled(on):
        scall("org.a11y.Bus", "/org/a11y/bus", "org.freedesktop.DBus.Properties", "Set", GLib.Variant("(ssv)", ("org.a11y.Status", "IsEnabled", GLib.Variant("b", on))))

    def x(*args, check=True):
        r = subprocess.run(["xdotool", *args], capture_output=True, text=True, env={**os.environ, "DISPLAY": display}, timeout=10)
        if check and r.returncode != 0:
            raise Fail(f"xdotool {args}: {r.stderr.strip()}")
        return r.stdout.strip()

    def ticks(pid, tid=None):
        p = Path(f"/proc/{pid}/task/{tid}/stat") if tid else Path(f"/proc/{pid}/stat")
        f = p.read_text().rsplit(") ", 1)[1].split()
        return int(f[11]) + int(f[12])

    def fds(pid):
        return len(os.listdir(f"/proc/{pid}/fd"))

    def per_thread(pid, secs):
        def snap():
            out = {}
            for t in os.listdir(f"/proc/{pid}/task"):
                try:
                    out[t] = (Path(f"/proc/{pid}/task/{t}/comm").read_text().strip(), ticks(pid, t))
                except OSError:
                    pass
            return out

        a0 = snap()
        time.sleep(secs)
        a1 = snap()
        return {f"{a1[t][0]}:{t}": a1[t][1] - a0[t][1] for t in a1 if t in a0 and a1[t][1] - a0[t][1] > 0}

    registry = {"p": None}

    def pixel(px, py):
        """RGB of one root-window pixel of the private display (proves which theme mode was painted)."""
        import ctypes
        import ctypes.util

        X = ctypes.CDLL(ctypes.util.find_library("X11"))
        X.XOpenDisplay.restype = ctypes.c_void_p
        X.XOpenDisplay.argtypes = [ctypes.c_char_p]
        X.XDefaultRootWindow.restype = ctypes.c_ulong
        X.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
        X.XGetImage.restype = ctypes.c_void_p
        X.XGetImage.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_int, ctypes.c_uint, ctypes.c_uint, ctypes.c_ulong, ctypes.c_int]
        X.XGetPixel.restype = ctypes.c_ulong
        X.XGetPixel.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int]
        X.XCloseDisplay.argtypes = [ctypes.c_void_p]
        d = X.XOpenDisplay(display.encode())
        img = X.XGetImage(d, X.XDefaultRootWindow(d), px, py, 1, 1, 0xFFFFFFFF, 2)
        v = X.XGetPixel(img, 0, 0)
        X.XCloseDisplay(d)
        return ((v >> 16) & 255, (v >> 8) & 255, v & 255)

    def start_registry():
        """The private a11y bus is dbus-broker, which activates the registry through systemd (absent here): start the
        same /usr/lib/at-spi2-registryd ourselves on the private bus, as systemd would."""
        addr = scall("org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus", "GetAddress", None, "(s)")[0]
        log = open(ev / "registry.log", "a")
        p = subprocess.Popen(["/usr/lib/at-spi2-registryd"], env={**os.environ, "AT_SPI_BUS_ADDRESS": addr}, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        owned.append(p)
        registry["p"] = p
        return p

    def thread(pid, name):
        for t in os.listdir(f"/proc/{pid}/task"):
            try:
                if Path(f"/proc/{pid}/task/{t}/comm").read_text().strip() == name:
                    return t
            except OSError:
                pass
        return None

    class Settings:
        """One decsettings process with its own private HOME and config."""

        def __init__(self, name, scale=1.0, mode="dark", pane="general", wayland=None):
            self.name = name
            self.dir = root / "apps" / name
            for d in ["home", "cfg", "decebal"]:
                (self.dir / d).mkdir(parents=True, exist_ok=True)
            self.config = self.dir / "cfg" / "config.toml"
            self.config.write_text("# private decsettings a11y test\n[general]\n")
            (self.dir / "decebal" / "decebal.json").write_text(json.dumps({"theme": {"mode": mode}}))
            self.dump = self.dir / "layout.json"
            self.log = open(self.dir / "decsettings.log", "w")
            env = dict(os.environ)
            env.update(HOME=str(self.dir / "home"), XDG_CONFIG_HOME=str(self.dir / "cfg"), DECEBAL_CONFIG=str(self.dir / "decebal"), DECEBAL_BACKEND="x11", DECSETTINGS_DEBUG_LAYOUT=str(self.dump), DECSETTINGS_SCALE=str(scale), RDWMCTL="/nonexistent/rdwmctl", RDWM_SOCK=str(self.dir / "no-rdwm.sock"), DECEBAL_LOG="debug", DECEBAL_LOG_DIR=str(self.dir / "logs"), DECEBAL_A11Y_REPLY_TIMEOUT_MS="20000")
            env.pop("DECEBAL_A11Y", None)
            if wayland:
                env.pop("DISPLAY", None)
                env.update(WAYLAND_DISPLAY=wayland, DECEBAL_BACKEND="wayland")
            self.p = subprocess.Popen([a.bin, "--config", str(self.config), pane], env=env, stdout=self.log, stderr=subprocess.STDOUT, start_new_session=True)
            owned.append(self.p)
            if wayland:
                self.wid = None
            else:
                # decsettings sets no _NET_WM_PID; only one runs at a time (single-instance lock), so its class finds it.
                self.wid = wait(lambda: x("search", "--onlyvisible", "--class", "DecSettings", check=False).split("\n")[0], "decsettings window")
            wait(lambda: self.layout(), "first layout dump")

        def layout(self):
            try:
                return json.loads(self.dump.read_text())
            except (OSError, ValueError):
                return None

        def ctl(self, cid):
            for c in (self.layout() or {}).get("ctls", []):
                if c["id"] == cid:
                    return c
            return None

        def geometry(self):
            g = dict(l.split("=") for l in x("getwindowgeometry", "--shell", self.wid).split("\n"))
            return int(g["X"]), int(g["Y"]), int(g["WIDTH"]), int(g["HEIGHT"])

        def text(self):
            return self.config.read_text()

        def close(self):
            if self.p.poll() is None:
                self.p.terminate()
                try:
                    self.p.wait(5)
                except subprocess.TimeoutExpired:
                    self.p.kill()
                    self.p.wait()
            self.log.close()

    # ------------------------------------------------------------ AT helpers

    def find_app(name="System Settings", pid=None, t=10.0):
        def go():
            d = Atspi.get_desktop(0)
            for i in range(d.get_child_count()):
                c = d.get_child_at_index(i)
                if c is not None and c.get_name() == name and (pid is None or c.get_process_id() == pid):
                    return c
            return None

        return wait(go, f"application {name!r} in the registry", t)

    def index(acc, limit=6000):
        out, queue = {}, [acc]
        while queue and len(out) < limit:
            n = queue.pop(0)
            try:
                k = n.get_accessible_id()
            except GLib.Error:
                continue
            out[k] = n
            for i in range(n.get_child_count()):
                c = n.get_child_at_index(i)
                if c is not None:
                    queue.append(c)
        return out

    def states(n):
        s = n.get_state_set()
        return {st.value_nick for st in Atspi.StateType.__enum_values__.values() if s.contains(st)}

    def ext(n, coord=Atspi.CoordType.SCREEN):
        r = Atspi.Component.get_extents(n, coord)
        return [r.x, r.y, r.width, r.height]

    def act(n, name):
        for i in range(Atspi.Action.get_n_actions(n)):
            if Atspi.Action.get_action_name(n, i) == name:
                return Atspi.Action.do_action(n, i)
        raise Fail(f"{n.get_accessible_id()} has no action {name}")

    events = []

    def on_event(e, *_):
        try:
            src = e.source.get_accessible_id() if e.source is not None else None
        except GLib.Error:
            src = None
        events.append({"type": e.type, "src": src, "d1": e.detail1, "d2": e.detail2, "t": time.time()})

    listener = Atspi.EventListener.new(on_event)
    event_types = ["object:state-changed:focused", "object:state-changed:checked", "object:state-changed:expanded", "object:state-changed:showing", "object:property-change:accessible-value", "object:property-change:accessible-name", "object:children-changed", "object:text-changed", "object:text-caret-moved", "object:announcement", "object:selection-changed", "window:activate", "window:deactivate"]

    def got(typ, src=None, d1=None, since=0.0, t=5.0):
        def ok():
            return next((e for e in events if e["t"] >= since and e["type"].startswith(typ) and (src is None or e["src"] == src) and (d1 is None or e["d1"] == d1)), None)

        return wait(ok, f"event {typ} from {src}", t)

    def a11y_conn():
        addr = scall("org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus", "GetAddress", None, "(s)")[0]
        return Gio.DBusConnection.new_for_address_sync(addr, Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION, None, None), addr

    def bus_of(conn, name="System Settings"):
        kids = conn.call_sync("org.a11y.atspi.Registry", "/org/a11y/atspi/accessible/root", "org.a11y.atspi.Accessible", "GetChildren", None, GLib.VariantType("(a(so))"), 0, 5000, None).unpack()[0]
        for bus, path in kids:
            n = conn.call_sync(bus, path, "org.freedesktop.DBus.Properties", "Get", GLib.Variant("(ss)", ("org.a11y.atspi.Accessible", "Name")), GLib.VariantType("(v)"), 0, 5000, None).unpack()[0]
            if n == name:
                return bus
        return None

    def paths(conn, bus):
        items = conn.call_sync(bus, "/org/a11y/atspi/cache", "org.a11y.atspi.Cache", "GetItems", None, GLib.VariantType("(a((so)(so)(so)iiassusau))"), 0, 5000, None).unpack()[0]
        out = {}
        for it in items:
            path = it[0][1]
            k = conn.call_sync(bus, path, "org.freedesktop.DBus.Properties", "Get", GLib.Variant("(ss)", ("org.a11y.atspi.Accessible", "AccessibleId")), GLib.VariantType("(v)"), 0, 5000, None).unpack()[0]
            out[k] = path
        return out, len(items)

    def async_calls(conn, bus, path, iface, member, params, rtype, n):
        res = []

        def done(c, r, _):
            try:
                res.append(("ok", c.call_finish(r).unpack()))
            except GLib.Error as e:
                res.append(("err", e.message))

        for _ in range(n):
            conn.call(bus, path, iface, member, params, GLib.VariantType(rtype), 0, 30000, None, done, None)
        wait(lambda: len(res) == n, f"{n} replies to {member}", 60)
        return res

    # ------------------------------------------------------------ provenance

    results["provenance"] = {
        "decsettings": {"path": a.bin, "sha256": sha256(a.bin)},
        "at-spi2-registryd": {"path": "/usr/lib/at-spi2-registryd", "sha256": sha256("/usr/lib/at-spi2-registryd")},
        "at-spi-bus-launcher": {"path": "/usr/lib/at-spi-bus-launcher", "sha256": sha256("/usr/lib/at-spi-bus-launcher")},
        "libatspi": "%d.%d.%d" % (Atspi.get_version()),
        "display": display,
        "session_bus": os.environ.get("DBUS_SESSION_BUS_ADDRESS"),
        "pkg": subprocess.run(["pacman", "-Q", "at-spi2-core", "python-gobject", "xorg-server-xvfb", "xdotool", "dbus"], capture_output=True, text=True).stdout.split("\n"),
    }

    passed = True
    try:
        wait(lambda: Path(f"/tmp/.X11-unix/X{num}").exists(), "private Xvfb")
        # -------------------------------------------------------- lazy enable
        must("launcher not running before anything asked", not has_owner("org.a11y.Bus"))
        s = Settings("main", pane="general")
        pump(1.5)
        check("lazy: decsettings does not start the bus launcher (NO_AUTO_START)", not has_owner("org.a11y.Bus"))
        fd_idle = fds(s.p.pid)
        bridge_tid = thread(s.p.pid, "a11y-bridge")
        check("bridge worker thread exists", bridge_tid is not None, bridge_tid)
        pump(2.0)
        busy_threads = per_thread(s.p.pid, 3.0)
        results["scenarios"]["idle_disabled_ticks_by_thread_3s"] = busy_threads
        check("idle with accessibility off: bridge thread 0 ticks in 3 s", not any(k.startswith("a11y-bridge") for k in busy_threads), busy_threads)
        # Activate the launcher (as an AT would) and enable accessibility.
        scall("org.a11y.Bus", "/org/a11y/bus", "org.freedesktop.DBus.Peer", "Ping")
        pump(0.5)
        check("launcher running but disabled: no registration", True)
        # Registering listeners initialises libatspi, which starts the launcher: only after the lazy checks.
        Atspi.init()
        # Settings saves synchronously (with fsync) on its UI thread; under host I/O load that alone can take seconds,
        # so the client and the bridge get generous deadlines here (the bridge default stays 5 s).
        Atspi.set_timeout(25000, 30000)
        for t in event_types:
            listener.register(t)
        t_enable = time.time()
        set_enabled(True)
        start_registry()
        app = find_app(pid=s.p.pid)
        results["scenarios"]["main"] = {"registered_after_s": round(time.time() - t_enable, 3)}
        check("registered after IsEnabled=true (watch, not poll)", True, results["scenarios"]["main"])
        check("application role", app.get_role() == Atspi.Role.APPLICATION)
        check("toolkit name", app.get_toolkit_name() == "Decebal", app.get_toolkit_name())
        check("registry assigned an application id (Application.Id set during Embed)", app.get_id() >= 0, app.get_id())
        conn, addr = a11y_conn()
        bus = bus_of(conn)
        results["scenarios"]["main"]["bus"] = bus
        ix = index(app)
        must("tree has the frame, sidebar, search, pages and content", all(k in ix for k in ["frame", "sidebar", "search", "pages", "content", "ctl:r:general:default_binds"]), sorted(ix)[:20])
        check("frame role", ix["frame"].get_role() == Atspi.Role.FRAME)
        check("search is an editable entry", ix["search"].get_role() == Atspi.Role.ENTRY and "editable" in states(ix["search"]))
        check("pages is a list with selection", ix["pages"].get_role() == Atspi.Role.LIST and "Selection" in ix["pages"].get_interfaces(), ix["pages"].get_interfaces())
        check("current page selected", "selected" in states(ix["pane:general"]))
        sw = ix["ctl:r:general:default_binds"]
        check("switch role/name", sw.get_role() == Atspi.Role.SWITCH and sw.get_name() == "Built-in key bindings", (sw.get_role_name(), sw.get_name()))
        check("switch states", {"enabled", "sensitive", "focusable", "checkable", "showing", "visible"} <= states(sw), sorted(states(sw)))
        check("stepper role + value", ix["ctl:r:general:chord_timeout_ms"].get_role() == Atspi.Role.SPIN_BUTTON and "Value" in ix["ctl:r:general:chord_timeout_ms"].get_interfaces(), ix["ctl:r:general:chord_timeout_ms"].get_interfaces())
        check("text entry role + text", ix["ctl:r:general:tags"].get_role() == Atspi.Role.ENTRY and {"Text", "EditableText"} <= set(ix["ctl:r:general:tags"].get_interfaces()), ix["ctl:r:general:tags"].get_interfaces())
        results["scenarios"]["main"]["nodes"] = len(ix)
        # Bounds: real X11 root coordinates.
        gx, gy, gw, gh = s.geometry()
        check("frame extents = X window geometry (screen)", ext(ix["frame"]) == [gx, gy, gw, gh], (ext(ix["frame"]), [gx, gy, gw, gh]))
        c = s.ctl("r:general:default_binds")
        want = [round(gx + c["rect"][0]), round(gy + c["rect"][1]), round(c["rect"][2]), round(c["rect"][3])]
        got_e = ext(sw)
        check("switch extents = window origin + painted rect (screen)", all(abs(p - q) <= 1 for p, q in zip(got_e, want)), (got_e, want))
        check("switch extents in window coordinates", all(abs(p - q) <= 1 for p, q in zip(ext(sw, Atspi.CoordType.WINDOW), [round(v) for v in c["rect"]])), ext(sw, Atspi.CoordType.WINDOW))
        x("windowmove", s.wid, "40", "60")
        pump(0.5)
        gx2, gy2, _, _ = s.geometry()
        wait(lambda: ext(sw)[0] == got_e[0] - gx + gx2, "bounds follow a window move")
        check("bounds follow a window move", ext(sw)[:2] == [got_e[0] - gx + gx2, got_e[1] - gy + gy2], (ext(sw), gx2, gy2))
        hit = Atspi.Component.get_accessible_at_point(ix["frame"], ext(sw)[0] + 2, ext(sw)[1] + 2, Atspi.CoordType.SCREEN)
        check("hit test at the switch returns the switch", hit is not None and hit.get_accessible_id() == sw.get_accessible_id(), hit.get_accessible_id() if hit else None)

        # -------------------------------------------------------- actions change real files
        before = s.text()
        was = "checked" in states(sw)
        since = time.time()
        check("do_action(toggle) accepted", act(sw, "toggle"))
        check("config.toml changed by the AT action", wait(lambda: f"default_binds = {str(not was).lower()}" in s.text(), "file write"), s.text())
        got("object:state-changed:checked", "ctl:r:general:default_binds", since=since)
        check("checked state follows (cached read)", ("checked" in states(sw)) == (not was))
        sw.clear_cache()
        check("checked state follows (fresh read)", ("checked" in states(sw)) == (not was))
        stn = ix["ctl:r:general:chord_timeout_ms"]
        lo, step = Atspi.Value.get_minimum_value(stn), Atspi.Value.get_minimum_increment(stn)
        target = round((lo + 4 * step) / step) * step
        since = time.time()
        check("set_current_value accepted", Atspi.Value.set_current_value(stn, target))
        check("stepper value written", wait(lambda: f"chord_timeout_ms = {int(target)}" in s.text(), "stepper write"), s.text())
        got("object:property-change:accessible-value", "ctl:r:general:chord_timeout_ms", since=since)
        check("value reads back", Atspi.Value.get_current_value(stn) == target, Atspi.Value.get_current_value(stn))
        # libatspi 2.62 aborts the client (dbus_message_unref(NULL)) when a Value Set fails, so invalid values go
        # through raw Properties.Set calls.
        pmap, n_items = paths(conn, bus)
        for bad in [float("nan"), Atspi.Value.get_maximum_value(stn) + 1000]:
            try:
                conn.call_sync(bus, pmap["ctl:r:general:chord_timeout_ms"], "org.freedesktop.DBus.Properties", "Set", GLib.Variant("(ssv)", ("org.a11y.atspi.Value", "CurrentValue", GLib.Variant("d", bad))), None, 0, 5000, None)
                check(f"invalid value {bad} rejected", False)
            except GLib.Error as e:
                check(f"invalid value {bad} rejected with a clear error", "InvalidArgs" in e.message, e.message)
        check("value unchanged after invalid writes", f"chord_timeout_ms = {int(target)}" in s.text())
        # Text field: draft until activated, caret and selection.
        tags = ix["ctl:r:general:tags"]
        et = tags
        before = s.text()
        check("set_text_contents accepted", Atspi.EditableText.set_text_contents(et, "web,code,chat"))
        pump(0.3)
        check("draft is not written yet", s.text() == before)
        check("draft attribute exposed", tags.get_attributes().get("draft") == "true", tags.get_attributes())
        tx = tags
        check("text reads the draft", Atspi.Text.get_text(tx, 0, -1) == "web,code,chat", Atspi.Text.get_text(tx, 0, -1))
        check("caret at end", Atspi.Text.get_caret_offset(tx) == 13, Atspi.Text.get_caret_offset(tx))
        check("set_selection accepted", Atspi.Text.set_selection(tx, 0, 4, 8))
        pump(0.2)
        r = Atspi.Text.get_selection(tx, 0)
        check("selection reads back", (r.start_offset, r.end_offset) == (4, 8), (r.start_offset, r.end_offset))
        check("word at offset", Atspi.Text.get_string_at_offset(tx, 5, Atspi.TextGranularity.WORD).content == "code,", Atspi.Text.get_string_at_offset(tx, 5, Atspi.TextGranularity.WORD).content)
        # AT-SPI InsertText inserts at an offset (it does not replace the selection): delete, then insert.
        check("delete_text accepted", Atspi.EditableText.delete_text(et, 4, 8))
        pump(0.2)
        check("text after delete", Atspi.Text.get_text(tx, 0, -1) == "web,,chat", Atspi.Text.get_text(tx, 0, -1))
        check("insert_text accepted", Atspi.EditableText.insert_text(et, 4, "mail", 4))
        pump(0.2)
        check("text after insert", Atspi.Text.get_text(tx, 0, -1) == "web,mail,chat", Atspi.Text.get_text(tx, 0, -1))
        check("activate commits the draft", act(tags, "activate"))
        check("tags written", wait(lambda: "web" in s.text() and "mail" in s.text(), "tags write"), s.text())

        # -------------------------------------------------------- stale, disabled, refused, queue pressure
        pmap, n_items = paths(conn, bus)
        check("Cache.GetItems lists the tree", n_items == len(pmap) and n_items >= len(ix) - 5, (n_items, len(ix)))
        swp = pmap["ctl:r:general:default_binds"]
        before = s.text()
        res = async_calls(conn, bus, swp, "org.a11y.atspi.Action", "DoAction", GLib.Variant("(i)", (0,)), "(b)", 2)
        oks = [r for r in res if r[0] == "ok"]
        stale = [r for r in res if r[0] == "err" and "Stale" in r[1]]
        check("double activation: one applied, the second rejected as stale", len(oks) == 1 and len(stale) == 1, res)
        flips = s.text().count("default_binds")
        check("file toggled exactly once", wait(lambda: s.text() != before, "toggle"), flips)
        nodes = index(app)
        busy = async_calls(conn, bus, pmap["search"], "org.a11y.atspi.Component", "GrabFocus", None, "(b)", 48)
        nb = sum(1 for r in busy if r[0] == "err" and "LimitsExceeded" in r[1])
        nok = sum(1 for r in busy if r[0] == "ok")
        check("queue pressure: excess requests refused at once (LimitsExceeded), the rest answered", nb >= 1 and nok >= 1 and nb + nok == 48, {"busy": nb, "ok": nok, "other": [r for r in busy if r[0] == "err" and "LimitsExceeded" not in r[1]][:3]})
        check("app still responsive after pressure", act(nodes["pane:windows"], "activate"))
        wait(lambda: (s.layout() or {}).get("pane") == "windows", "windows pane")
        pump(0.5)
        ix = index(app)
        check("content follows the page", ix["content"].get_name().startswith("Windows"), ix["content"].get_name())
        # Disabled control (Appearance → Themes → Market): rejected.
        act(ix["pane:appearance"], "activate")
        wait(lambda: (s.layout() or {}).get("pane") == "appearance", "appearance pane")
        pump(0.5)
        ix = index(app)
        mk = ix.get("ctl:th:market")
        if check("disabled market button exposed", mk is not None and "enabled" not in states(mk), sorted(states(mk)) if mk else None):
            try:
                act(mk, "click")
                check("disabled control rejected", False)
            except GLib.Error as e:
                check("disabled control rejected with a clear error", "Disabled" in e.message or "disabled" in e.message, e.message)
        # Tabs.
        tabs = ix["ctl:ap:tab"]
        check("tab strip role", tabs.get_role() == Atspi.Role.PAGE_TAB_LIST)
        check("select_child on the tab strip", Atspi.Selection.select_child(tabs, 1))
        wait(lambda: (s.layout() or {}).get("tab") == "palettes", "palettes tab")
        check("app followed the tab selection", (s.layout() or {}).get("tab") == "palettes")
        Atspi.Selection.select_child(tabs, 0)
        wait(lambda: (s.layout() or {}).get("tab") == "theme", "themes tab")

        # -------------------------------------------------------- modal list, colour dialog, refusal
        act(ix["pane:windows"], "activate")
        wait(lambda: (s.layout() or {}).get("pane") == "windows", "windows pane")
        pump(0.5)
        ix = index(app)
        ck = next((k for k, v in ix.items() if k.startswith("ctl:r:") and v.get_role() == Atspi.Role.COMBO_BOX and "showing" in states(v)), None)
        combo = ix.get(ck) if ck else None
        results["scenarios"]["main"]["combo"] = ck
        if check("combo box exposed", combo is not None, ck):
            cfg_key = ck.split(":")[-1]
            before = s.text()
            since = time.time()
            act(combo, "press")
            got("object:state-changed:expanded", ck, 1, since=since)
            menu = index(combo)[f"{ck}/menu"]
            check("open list is showing and modal", {"showing", "modal"} <= states(menu), sorted(states(menu)))
            try:
                other = next(v for k, v in ix.items() if k.startswith("ctl:r:") and v.get_role() == Atspi.Role.SWITCH)
                act(other, "toggle")
                check("modal list refuses other controls", False)
            except GLib.Error as e:
                check("modal list refuses other controls with a clear error", "Refused" in e.message or "open" in e.message, e.message)
            items = [k for k in index(combo) if k.startswith(f"{ck}/opt:")]
            unselected = next(k for k in items if "selected" not in states(index(combo)[k]))
            opt = index(combo)[unselected]
            check("list item role", opt.get_role() == Atspi.Role.MENU_ITEM)
            act(opt, "click")
            check("list choice written", wait(lambda: s.text() != before and cfg_key in s.text(), "combo write"), s.text())
            check("list closed", "collapsed" in states(combo) or "expanded" not in states(combo), sorted(states(combo)))
        colk = next((k for k, v in ix.items() if k.startswith("ctl:r:") and "color" in v.get_attributes()), None)
        col = ix.get(colk) if colk else None
        results["scenarios"]["main"]["colour"] = colk
        if check("colour control exposed", col is not None, colk):
            Atspi.Component.scroll_to(col, Atspi.ScrollType.ANYWHERE)
            wait(lambda: "showing" in states(col), "colour control scrolled into view")
            before = s.text()
            act(col, "press")
            wait(lambda: "colorpop" in index(ix["frame"], 400), "colour dialog")
            dlg = index(ix["frame"], 400)
            check("colour dialog is modal", {"modal", "showing"} <= states(dlg["colorpop"]) and dlg["colorpop"].get_role() == Atspi.Role.DIALOG)
            try:
                act(ix["pane:general"], "activate")
                check("modal colour dialog refuses the sidebar", False)
            except GLib.Error as e:
                check("modal colour dialog refuses the sidebar with a clear error", "refused" in e.message.lower(), e.message)
            act(dlg["colorpop:sw:2"], "click")
            check("colour written", wait(lambda: "#ffd60a" in s.text() and s.text() != before, "colour write"), s.text())
            check("colour dialog closed", "colorpop" not in index(ix["frame"], 400))
        # Error flow: a value the model refuses comes back as an error and an alert.
        act(ix["pane:general"], "activate")
        wait(lambda: (s.layout() or {}).get("pane") == "general", "general pane")
        pump(0.5)
        ix = index(app)
        before = s.text()
        since = time.time()
        Atspi.EditableText.set_text_contents(ix["ctl:r:general:tags"], ",".join(f"t{i}" for i in range(40)))
        try:
            act(ix["ctl:r:general:tags"], "activate")
            check("refused value returns an error", False)
        except GLib.Error as e:
            check("refused value returns an error naming the problem", "refused" in e.message.lower() and "31" in e.message, e.message)
        check("refused value not written", s.text() == before)
        ann = next((e for e in events if e["t"] >= since and e["type"].startswith("object:announcement")), None) or next((e for e in events if e["t"] >= since and e["type"].startswith("object:children-changed")), None)
        check("the error alert was announced / added", ann is not None, ann)
        ix = index(app)
        check("alert node present", any(k.startswith("note:") and v.get_role() == Atspi.Role.ALERT for k, v in ix.items()))

        # -------------------------------------------------------- focus: AT and keyboard agree
        x("windowfocus", "--sync", s.wid)
        pump(0.5)
        check("window active state", "active" in states(ix["frame"]), sorted(states(ix["frame"])))
        sw = ix["ctl:r:general:default_binds"]
        since = time.time()
        check("grab_focus accepted", Atspi.Component.grab_focus(sw))
        got("object:state-changed:focused", "ctl:r:general:default_binds", 1, since=since)
        check("app keyboard focus = AT focus", (s.layout() or {}).get("focus") == "r:general:default_binds", (s.layout() or {}).get("focus"))
        before = s.text()
        x("key", "space")
        check("Space toggles the control the AT focused", wait(lambda: s.text() != before, "space toggle"), s.text())
        since = time.time()
        x("key", "Tab")
        e = got("object:state-changed:focused", None, 1, since=since)
        fk = wait(lambda: (s.layout() or {}).get("focus"), "focus after Tab")
        check("Tab: AT focus event names the app's new keyboard focus", e["src"] == f"ctl:{fk}", (e["src"], fk))
        ix = index(app)
        focused = [k for k, v in ix.items() if "focused" in states(v)]
        check("exactly one focused node", len(focused) == 1, focused)

        # -------------------------------------------------------- search and sidebar
        sr = ix["search"]
        Atspi.Component.grab_focus(sr)
        Atspi.EditableText.set_text_contents(sr, "border")
        wait(lambda: index(app, 200)["pages"].get_name() == "Search results", "search results")
        check("search results listed", index(app, 200)["pages"].get_child_count() > 0)
        act(index(app, 50)["search"], "activate")
        pump(0.5)
        check("search opened a result page", (s.layout() or {}).get("pane") in ("windows", "appearance", "general", "tiling"), (s.layout() or {}).get("pane"))
        ix = index(app)
        act(ix["search:clear"], "click") if "search:clear" in ix else None
        pump(0.3)
        ix = index(app)
        pg = ix["pages"]
        kids = [ix["pages"].get_child_at_index(i).get_accessible_id() for i in range(ix["pages"].get_child_count())]
        i_tiling = kids.index("pane:tiling") if "pane:tiling" in kids else 0
        check("select_child in the page list", Atspi.Selection.select_child(pg, i_tiling))
        wait(lambda: (s.layout() or {}).get("pane") == "tiling", "tiling pane")

        # -------------------------------------------------------- short window and scrolling
        x("windowsize", s.wid, "900", "420")
        pump(0.8)
        act(index(app, 300)["pane:general"], "activate")
        wait(lambda: (s.layout() or {}).get("pane") == "general", "general pane")
        pump(0.5)
        ix = index(app)
        tg = ix["ctl:r:general:tags"]
        check("short window: offscreen control not showing", "showing" not in states(tg), sorted(states(tg)))
        check("scroll_to accepted", Atspi.Component.scroll_to(tg, Atspi.ScrollType.ANYWHERE))
        wait(lambda: "showing" in states(tg), "scrolled into view")
        g = s.geometry()
        e2 = ext(tg)
        check("scrolled control is inside the window", g[1] <= e2[1] and e2[1] + e2[3] <= g[1] + g[3], (e2, g))

        # -------------------------------------------------------- idle with an AT listening, FDs, disable/enable
        pump(1.0)
        busy_on = per_thread(s.p.pid, 3.0)
        results["scenarios"]["idle_registered_ticks_by_thread_3s"] = busy_on
        check("idle while registered: bridge thread 0 ticks in 3 s", not any(k.startswith("a11y-bridge") for k in busy_on), busy_on)
        fd_on = fds(s.p.pid)
        set_enabled(False)
        wait(lambda: find_app_absent(Atspi, s.p.pid), "unregistered after IsEnabled=false", 8)
        check("disable: app leaves the registry", True)
        # The accessibility socket closes when the worker sees IsEnabled=false; allow it a moment.
        fd_off = wait(lambda: fds(s.p.pid) if fds(s.p.pid) <= fd_idle else None, "fd back to idle", 3) if fds(s.p.pid) > fd_idle else fds(s.p.pid)
        check("disable: accessibility bus fd closed", fd_off == fd_idle, {"idle": fd_idle, "registered": fd_on, "disabled": fd_off})
        set_enabled(True)
        # libatspi keeps its own view of the desktop; check the registry on the wire.
        conn, addr = a11y_conn()
        check("re-enable: registered again", wait(lambda: bus_of(conn), "registered again", 10) is not None)

        # -------------------------------------------------------- close
        x("windowfocus", "--sync", s.wid)
        x("key", "ctrl+w")
        rc = wait(lambda: s.p.poll() is not None and [s.p.returncode], "decsettings exit", 6)
        check("close: process exits cleanly", rc == [0], rc)
        wait(lambda: find_app_absent(Atspi, s.p.pid), "registry forgets the app", 6)
        check("close: registry forgot the app (Unembed / name lost)", True)
        s.close()

        # -------------------------------------------------------- bounds at scales and modes
        for scale in [1.0, 1.25, 2.0]:
            for mode in ["dark", "light"]:
                name = f"scale{scale}-{mode}"
                t = Settings(name, scale=scale, mode=mode, pane="general")
                try:
                    ap = find_app(pid=t.p.pid, t=15)
                    tix = index(ap)
                    gx, gy, gw, gh = t.geometry()
                    rows = []
                    ok = ext(tix["frame"]) == [gx, gy, gw, gh]
                    for cid in ["r:general:default_binds", "r:general:chord_timeout_ms", "a:reload"]:
                        c = t.ctl(cid)
                        n = tix.get(f"ctl:{cid}")
                        if c is None or n is None:
                            ok = False
                            rows.append({"id": cid, "missing": True})
                            continue
                        want = [round(gx + c["rect"][0]), round(gy + c["rect"][1]), round(c["rect"][2]), round(c["rect"][3])]
                        e = ext(n)
                        ok &= all(abs(p - q) <= 1 for p, q in zip(e, want))
                        rows.append({"id": cid, "at": e, "painted": want})
                    cb = Atspi.Component.get_extents(tix["content"], Atspi.CoordType.SCREEN)
                    bg = pixel(cb.x + cb.width - 20, cb.y + cb.height - 20)
                    results["scenarios"][name] = {"window": [gx, gy, gw, gh], "controls": rows, "nodes": len(tix), "content_background_rgb": bg}
                    check(f"bounds match painted rects at scale {scale}, {mode}", ok, rows)
                finally:
                    t.close()
                    wait(lambda: find_app_absent(Atspi, t.p.pid), "previous app gone", 6)
        # -------------------------------------------------------- Wayland (private headless sway)
        if shutil.which("sway"):
            cfgp = root / "sway.cfg"
            cfgp.write_text("xwayland disable\noutput HEADLESS-1 resolution 1280x800 position 0,0\ndefault_border none\n")
            before_sock = set(os.listdir(os.environ["XDG_RUNTIME_DIR"]))
            slog = open(ev / "sway.log", "w")
            senv = {k: v for k, v in os.environ.items() if k not in ("DISPLAY",)}
            senv.update(WLR_BACKENDS="headless", WLR_RENDERER="pixman", WLR_LIBINPUT_NO_DEVICES="1")
            sway = subprocess.Popen(["sway", "-c", str(cfgp)], env=senv, stdout=slog, stderr=subprocess.STDOUT, start_new_session=True)
            owned.append(sway)
            sock = wait(lambda: next((f for f in set(os.listdir(os.environ["XDG_RUNTIME_DIR"])) - before_sock if f.startswith("wayland-") and not f.endswith(".lock")), None), "private sway socket", 10)
            results["provenance"]["sway"] = {"path": shutil.which("sway"), "sha256": sha256(shutil.which("sway")), "version": subprocess.run(["sway", "--version"], capture_output=True, text=True).stdout.strip()}
            w = Settings("wayland", pane="general", wayland=sock)
            try:
                ap = find_app(pid=w.p.pid, t=15)
                wix = index(ap)
                fr_s, fr_w = ext(wix["frame"]), ext(wix["frame"], Atspi.CoordType.WINDOW)
                c = w.ctl("r:general:default_binds")
                want = [round(v) for v in c["rect"]]
                sw_s, sw_w = ext(wix["ctl:r:general:default_binds"]), ext(wix["ctl:r:general:default_binds"], Atspi.CoordType.WINDOW)
                results["scenarios"]["wayland"] = {"socket": sock, "frame_screen": fr_s, "frame_window": fr_w, "switch_screen": sw_s, "switch_window": sw_w, "painted": want, "frame_states": sorted(states(wix["frame"]))}
                check("wayland: registered through the same bridge", ap.get_toolkit_name() == "Decebal")
                check("wayland: bounds are window-relative (screen = window coordinates; no global position on Wayland)", fr_s == fr_w and fr_s[:2] == [0, 0] and sw_s == sw_w, results["scenarios"]["wayland"])
                check("wayland: control bounds = painted rect", all(abs(p - q) <= 1 for p, q in zip(sw_w, want)), (sw_w, want))
                before = w.text()
                act(wix["ctl:r:general:default_binds"], "toggle")
                check("wayland: AT action writes config.toml", wait(lambda: w.text() != before, "wayland write"), w.text())
                check("wayland: grab_focus moves the app's keyboard focus", Atspi.Component.grab_focus(wix["ctl:r:general:chord_timeout_ms"]) and wait(lambda: (w.layout() or {}).get("focus") == "r:general:chord_timeout_ms", "wayland focus"))
                # No keyboard device in a headless compositor: no wl_keyboard.enter, so the frame is honestly not active.
                check("wayland: window activation follows wl_keyboard (none here: not active)", "active" not in states(wix["frame"]), sorted(states(wix["frame"])))
            finally:
                w.close()
                wait(lambda: find_app_absent(Atspi, w.p.pid), "wayland app gone", 6)
                sway.terminate()
                sway.wait(5)
                slog.close()
        else:
            check("wayland: sway not installed, scenario skipped", True, "skipped")

        # -------------------------------------------------------- reconnect: registry restart, then bus restart
        s = Settings("reconnect", pane="general")
        conn, addr = a11y_conn()
        bus = wait(lambda: bus_of(conn), "reconnect scenario registered", 10)
        fd_on = fds(s.p.pid)
        reg_pid = conn.call_sync("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "GetConnectionUnixProcessID", GLib.Variant("(s)", ("org.a11y.atspi.Registry",)), GLib.VariantType("(u)"), 0, 5000, None).unpack()[0]
        check("registry pid is a private process", str(root).encode() in Path(f"/proc/{reg_pid}/environ").read_bytes(), reg_pid)
        os.kill(reg_pid, 15)
        if registry["p"] is not None:
            registry["p"].wait(5)
        pump(0.5)
        # Restarted (by systemd on a real desktop, by us here); the bridge re-embeds on NameOwnerChanged. libatspi does
        # not survive its registry going away within one process, so this is checked on the wire.
        start_registry()
        b2 = wait(lambda: bus_of(conn), "re-embedded in the new registry", 15)
        check("registry restart: re-embedded", b2 == bus, (b2, bus))
        launcher_pid = scall("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "GetConnectionUnixProcessID", GLib.Variant("(s)", ("org.a11y.Bus",)), "(u)")[0]
        bus_pid = None
        for p in Path("/proc").iterdir():
            if p.name.isdigit():
                try:
                    stat = (p / "stat").read_text().rsplit(") ", 1)[1].split()
                    if int(stat[1]) == launcher_pid and b"dbus" in (p / "cmdline").read_bytes():
                        bus_pid = int(p.name)
                except OSError:
                    pass
        results["scenarios"]["reconnect"] = {"launcher": launcher_pid, "a11y_bus": bus_pid}
        if check("accessibility bus daemon found (child of the private launcher)", bus_pid is not None, bus_pid):
            os.kill(bus_pid, 15)
            t_kill = time.time()
            pump(1.0)
            # Re-enable through the launcher (restarted by activation if it exited), as an AT would.
            def reenabled():
                try:
                    set_enabled(True)
                except GLib.Error:
                    return False
                return True

            wait(reenabled, "launcher back", 10)
            if registry["p"] is not None and registry["p"].poll() is None:
                registry["p"].terminate()
                registry["p"].wait(5)
            start_registry()
            conn, addr = wait(lambda: a11y_conn(), "new accessibility bus", 10)
            b3 = wait(lambda: bus_of(conn), "re-registered on the new bus", 40)
            results["scenarios"]["reconnect"]["recovered_s"] = round(time.time() - t_kill, 2)
            results["scenarios"]["reconnect"]["new_address"] = addr
            check("bus restart: bridge reconnected and re-registered", b3 is not None, results["scenarios"]["reconnect"])
            pm3, n3 = paths(conn, b3)
            check("tree complete after reconnect", "ctl:r:general:default_binds" in pm3 and "search" in pm3, n3)
            # And it still acts: toggle through the new connection.
            before = s.text()
            r3 = conn.call_sync(b3, pm3["ctl:r:general:default_binds"], "org.a11y.atspi.Action", "DoAction", GLib.Variant("(i)", (0,)), GLib.VariantType("(b)"), 0, 5000, None).unpack()
            check("actions work after reconnect", r3 == (True,) and wait(lambda: s.text() != before, "write after reconnect"), r3)
        fd_after = fds(s.p.pid)
        check("no fd leak across reconnects", fd_after <= fd_on, {"registered": fd_on, "after_reconnects": fd_after})

        x("windowfocus", "--sync", s.wid)
        x("key", "ctrl+w")
        rc = wait(lambda: s.p.poll() is not None and [s.p.returncode], "decsettings exit", 6)
        check("close after reconnects: process exits cleanly", rc == [0], rc)
        wait(lambda: bus_of(conn) is None, "registry forgets the app", 6)
        check("close after reconnects: registry forgot the app", True)
        s.close()
        lum = {k: sum(v["content_background_rgb"]) for k, v in results["scenarios"].items() if k.startswith("scale") and "content_background_rgb" in v}
        if lum:
            check("light and dark themes were really painted (light background brighter at every scale)", all(lum[f"scale{sc}-light"] > lum[f"scale{sc}-dark"] + 150 for sc in [1.0, 1.25, 2.0]), lum)
    except Fail as e:
        passed = False
        check("aborted", False, str(e))
    except Exception:  # noqa: BLE001
        passed = False
        check("crashed", False, traceback.format_exc())
    finally:
        # On failure: what every thread of a still-running decsettings is doing (no ptrace needed).
        if not passed:
            diag = {}
            for p in owned:
                if p.poll() is None and Path(f"/proc/{p.pid}/task").exists() and b"decsettings" in Path(f"/proc/{p.pid}/cmdline").read_bytes():
                    for t in os.listdir(f"/proc/{p.pid}/task"):
                        b = Path(f"/proc/{p.pid}/task/{t}")
                        try:
                            diag[f"{p.pid}/{t}"] = {"comm": (b / "comm").read_text().strip(), "state": (b / "stat").read_text().rsplit(") ", 1)[1].split()[0], "wchan": (b / "wchan").read_text(), "syscall": (b / "syscall").read_text().split()[0] if (b / "syscall").exists() else None, "ticks": ticks(p.pid, t)}
                        except OSError:
                            pass
            results["threads_on_failure"] = diag
        try:
            listener.deregister("object:state-changed:focused")
        except Exception:  # noqa: BLE001
            pass
        for p in reversed(owned):
            if p.poll() is None:
                p.terminate()
        for p in reversed(owned):
            try:
                p.wait(5)
            except subprocess.TimeoutExpired:
                p.kill()
                p.wait()
        xlog.close()
    failed = [c["name"] for c in checks if not c["ok"]]
    results["passed"] = passed and not failed
    results["failed"] = failed
    results["events_seen"] = len(events)
    results["x_socket_removed"] = not Path(f"/tmp/.X11-unix/X{num}").exists()
    (ev / "results.json").write_text(json.dumps(results, indent=2, default=str) + "\n")
    return 0 if results["passed"] else 1


def find_app_absent(Atspi, pid):
    d = Atspi.get_desktop(0)
    for i in range(d.get_child_count()):
        c = d.get_child_at_index(i)
        try:
            if c is not None and c.get_process_id() == pid:
                return False
        except Exception:  # noqa: BLE001
            pass
    return True


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", required=True)
    ap.add_argument("--evidence", required=True)
    ap.add_argument("--inner", action="store_true")
    ap.add_argument("--only")
    ap.add_argument("--timeout", type=int, default=600)
    a = ap.parse_args()
    sys.exit(inner(a) if a.inner else outer(a))
