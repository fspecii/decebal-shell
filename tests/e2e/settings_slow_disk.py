#!/usr/bin/env python3
"""Native evidence: decsettings stays responsive while its durable writes are blocked. Private only.

Needs a decsettings built with `--features settings-io-fault` (never a package): every fsync waits while the file
named by DECSETTINGS_FAULT_SYNC_GATE exists, and fails with EIO for files named in DECSETTINGS_FAULT_SYNC_FAIL.

Runs inside a private HOME/XDG tree and a private `dbus-run-session` (the real at-spi-bus-launcher, and
at-spi2-registryd started on that private bus), on a private Xvfb with a reserved display number >= 700 that is
verified to be ours (lock file pid, filesystem and abstract sockets held by our Xvfb) before any input. Input is
XTEST pointer/keyboard into that private display plus AT-SPI calls; nothing touches the live display, buses or files.

    python3 -I tests/e2e/settings_slow_disk.py --bin path/to/decsettings --evidence ~/.cache/x/run

Writes <evidence>/results.json: every check, the latency of every probe and the threads' kernel wait channels.
"""
import argparse
import hashlib
import json
import os
import random
import re
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path

HERE = Path(__file__).resolve()
HOLD_S = 6.0  # how long the disk is held still in the blocked scenario
DROP = ("DBUS_", "AT_SPI", "ATSPI_", "DECEBAL_", "DECSETTINGS_", "GTK_", "GDK_", "QT_", "XDG_", "WAYLAND_", "GSETTINGS_", "NO_AT_BRIDGE", "RDWM")


def sha256(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


def outer(a):
    ev = Path(a.evidence).expanduser().resolve()
    ev.mkdir(parents=True, exist_ok=False)
    root = ev / "root"
    for d in ["home", "cfg", "data", "cache", "state", "run", "tmp"]:
        (root / d).mkdir(parents=True)
    os.chmod(root / "run", 0o700)
    env = {k: v for k, v in os.environ.items() if not k.startswith(DROP) and k not in ("DISPLAY", "XAUTHORITY", "SESSION_MANAGER", "PULSE_SERVER", "DESKTOP_SESSION")}
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
    cmd = ["dbus-run-session", "--", sys.executable, "-I", str(HERE), "--inner", "--bin", str(Path(a.bin).resolve()), "--evidence", str(ev), "--label", a.label] + (["--only", a.only] if a.only else [])
    log = open(ev / "session.log", "w")
    t0 = time.time()
    r = subprocess.run(cmd, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=a.timeout)
    log.close()
    res = json.loads((ev / "results.json").read_text()) if (ev / "results.json").exists() else {"passed": False, "error": "no results"}
    res["session_exit"] = r.returncode
    res["wall_s"] = round(time.time() - t0, 1)
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
    for pid in left:  # provably ours: the private root is in their environment
        try:
            os.kill(pid, 15)
        except OSError:
            pass
    (ev / "results.json").write_text(json.dumps(res, indent=2) + "\n")
    print(json.dumps({k: res.get(k) for k in ("label", "passed", "failed", "latency_ms", "session_exit", "leftover_private_processes", "wall_s")}, indent=2))
    return 0 if res.get("passed") and not left else 1


class Fail(Exception):
    pass


# ---------------------------------------------------------------- private display, owned and verified


def unix_sockets():
    out = []
    for line in Path("/proc/net/unix").read_text().splitlines()[1:]:
        f = line.split()
        if len(f) >= 8:
            out.append(f[7])
    return out


def free_display():
    socks = set(unix_sockets())
    for n in random.sample(range(700, 900), 200):
        if Path(f"/tmp/.X{n}-lock").exists() or Path(f"/tmp/.X11-unix/X{n}").exists():
            continue
        if f"/tmp/.X11-unix/X{n}" in socks or f"@/tmp/.X11-unix/X{n}" in socks:
            continue
        return n
    raise Fail("no free reserved display number in 700-899")


def owned_display(n, pid):
    """The lock names our Xvfb, and both the socket path and the abstract socket are listening sockets of it."""
    try:
        if int(Path(f"/tmp/.X{n}-lock").read_text().strip()) != pid:
            return False
    except (OSError, ValueError):
        return False
    ss = subprocess.run(["ss", "-xlp"], capture_output=True, text=True).stdout
    path = [l for l in ss.splitlines() if re.search(rf" /tmp/\.X11-unix/X{n} ", l)]
    abstract = [l for l in ss.splitlines() if re.search(rf" @/tmp/\.X11-unix/X{n} ", l)]
    return bool(path) and bool(abstract) and all(f"pid={pid}," in l for l in path + abstract)


def inner(a):
    import gi

    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi, Gio, GLib

    ev = Path(a.evidence)
    root = ev / "root"
    results = {"label": a.label, "checks": [], "latency_ms": {}, "scenarios": {}, "provenance": {}}
    checks, lat = results["checks"], results["latency_ms"]
    owned = []

    def check(name, ok, detail=None):
        checks.append({"name": name, "ok": bool(ok), "detail": detail})
        print(("PASS " if ok else "FAIL ") + name + (f" :: {detail}" if detail is not None else ""), flush=True)
        return bool(ok)

    ctx = GLib.MainContext.default()

    def pump(t=0.0):
        end = time.time() + t
        while True:
            while ctx.pending():
                ctx.iteration(False)
            if time.time() >= end:
                return
            time.sleep(0.005)

    def wait(fn, why, t=10.0, step=0.005):
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

    def timed(name, fn, why, t=12.0, t0=None):
        """Latency from t0 (default now) until fn() holds, in ms (None when it never did within t)."""
        t0 = t0 or time.time()
        try:
            wait(fn, why, t)
            lat[name] = round((time.time() - t0) * 1000, 1)
        except Fail:
            lat[name] = None
        return lat[name]

    # ------------------------------------------------------------ the display
    num = free_display()
    display = f":{num}"
    xauth = root / "xauthority"
    xauth.touch(mode=0o600)
    cookie = subprocess.run(["mcookie"], capture_output=True, text=True, check=True).stdout.strip()
    subprocess.run(["xauth", "-q", "-f", str(xauth), "add", display, ".", cookie], check=True)
    xlog = open(ev / "xvfb.log", "w")
    xvfb = subprocess.Popen(["Xvfb", display, "-auth", str(xauth), "-screen", "0", "1600x1000x24", "-nolisten", "tcp", "-noreset"], stdout=xlog, stderr=subprocess.STDOUT, start_new_session=True)
    owned.append(xvfb)
    try:
        wait(lambda: owned_display(num, xvfb.pid) or (xvfb.poll() is not None and (_ for _ in ()).throw(Fail("Xvfb exited"))), f"Xvfb {display} owned", 10)
    except Fail as e:
        xvfb.terminate()  # only our own process; nothing foreign is touched or unlinked
        xvfb.wait(5)
        results.update(passed=False, failed=[str(e)])
        (ev / "results.json").write_text(json.dumps(results, indent=2) + "\n")
        return 1
    check("private display is ours: lock pid, socket path and abstract socket held by our Xvfb", True, display)
    os.environ.update(DISPLAY=display, XAUTHORITY=str(xauth))
    session = Gio.bus_get_sync(Gio.BusType.SESSION, None)

    def scall(dest, path, iface, member, params=None, rtype=None):
        return session.call_sync(dest, path, iface, member, params, GLib.VariantType(rtype) if rtype else None, 0, 5000, None).unpack()

    def x(*args, check_rc=True):
        r = subprocess.run(["xdotool", *args], capture_output=True, text=True, timeout=20)
        if check_rc and r.returncode != 0:
            raise Fail(f"xdotool {args}: {r.stderr.strip()}")
        return r.stdout.strip()

    def threads(pid):
        out = []
        for t in sorted(os.listdir(f"/proc/{pid}/task"), key=int):
            b = Path(f"/proc/{pid}/task/{t}")
            try:
                out.append({"tid": int(t), "comm": (b / "comm").read_text().strip(), "state": (b / "stat").read_text().rsplit(") ", 1)[1].split()[0], "wchan": (b / "wchan").read_text()})
            except OSError:
                pass
        return out

    registry = {}

    def start_registry():
        addr = scall("org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus", "GetAddress", None, "(s)")[0]
        p = subprocess.Popen(["/usr/lib/at-spi2-registryd"], env={**os.environ, "AT_SPI_BUS_ADDRESS": addr}, stdout=open(ev / "registry.log", "a"), stderr=subprocess.STDOUT, start_new_session=True)
        owned.append(p)
        registry["p"] = p

    gate = root / "fsync-gate"

    class Settings:
        def __init__(self, name, pane="general", fail=None, extra_files=None, env_extra=None):
            self.name = name
            self.dir = root / "apps" / name
            for d in ["home", "cfg", "decebal", "logs"]:
                (self.dir / d).mkdir(parents=True, exist_ok=True)
            self.config = self.dir / "cfg" / "config.toml"
            self.config.write_text("# private decsettings slow-disk test\n[general]\nunknown_future_key = \"kept\"\n")
            (self.dir / "decebal" / "decebal.json").write_text(json.dumps({"theme": {"name": "nord", "mode": "dark"}, "future": {"kept": True}}, indent=2) + "\n")
            for rel, text in (extra_files or {}).items():
                (self.dir / rel).write_text(text)
            self.dump = self.dir / "layout.json"
            self.log = open(self.dir / "decsettings.out", "w")
            env = dict(os.environ)
            env.update(
                HOME=str(self.dir / "home"),
                XDG_CONFIG_HOME=str(self.dir / "cfg"),
                DECEBAL_CONFIG=str(self.dir / "decebal"),
                DECEBAL_BACKEND="x11",
                DECSETTINGS_DEBUG_LAYOUT=str(self.dump),
                RDWMCTL="/nonexistent/rdwmctl",
                RDWM_SOCK=str(self.dir / "no-rdwm.sock"),
                DECEBAL_LOG="debug",
                DECEBAL_LOG_DIR=str(self.dir / "logs"),
                DECSETTINGS_FAULT_SYNC_GATE=str(gate),
            )
            if fail:
                env["DECSETTINGS_FAULT_SYNC_FAIL"] = fail
            env.update(env_extra or {})
            self.p = subprocess.Popen([a.bin, "--config", str(self.config), pane], env=env, stdout=self.log, stderr=subprocess.STDOUT, start_new_session=True)
            owned.append(self.p)
            self.wid = wait(lambda: x("search", "--onlyvisible", "--class", "DecSettings", check_rc=False).split("\n")[0], "decsettings window")
            wait(lambda: self.layout(), "first layout dump")

        def layout(self):
            try:
                return json.loads(self.dump.read_text())
            except (OSError, ValueError):
                return None

        def ctl(self, cid):
            return next((c for c in (self.layout() or {}).get("ctls", []) if c["id"] == cid), None)

        def click(self, cid):
            c = self.ctl(cid)
            if c is None:
                raise Fail(f"no control {cid}")
            r = c["rect"]
            x("mousemove", "--window", self.wid, str(int(r[0] + r[2] / 2)), str(int(r[1] + r[3] / 2)))
            x("click", "1")

        def frames(self):
            return (self.layout() or {}).get("frames", 0)

        def close(self):
            if self.p.poll() is None:
                self.p.terminate()
                try:
                    self.p.wait(30)
                except subprocess.TimeoutExpired:
                    self.p.kill()
                    self.p.wait()
            self.log.close()

    def find_app(pid):
        def go():
            d = Atspi.get_desktop(0)
            for i in range(d.get_child_count()):
                c = d.get_child_at_index(i)
                if c is not None and c.get_name() == "System Settings" and c.get_process_id() == pid:
                    c.set_cache_mask(Atspi.Cache.NONE)  # every read asks the app: no stale names or states
                    return c
            return None

        return wait(go, "System Settings in the registry", 15)

    def index(acc):
        out, queue = {}, [acc]
        while queue and len(out) < 6000:
            n = queue.pop(0)
            try:
                out[n.get_accessible_id()] = n
            except GLib.Error:
                continue
            for i in range(n.get_child_count()):
                c = n.get_child_at_index(i)
                if c is not None:
                    queue.append(c)
        return out

    def states(n):
        s = n.get_state_set()
        return {st.value_nick for st in Atspi.StateType.__enum_values__.values() if s.contains(st)}

    def do_action(n, name):
        """(reply ms, result or error text) of one AT-SPI DoAction."""
        t0 = time.time()
        try:
            for i in range(Atspi.Action.get_n_actions(n)):
                if Atspi.Action.get_action_name(n, i) == name:
                    r = Atspi.Action.do_action(n, i)
                    return round((time.time() - t0) * 1000, 1), r
            return None, f"no action {name}"
        except GLib.Error as e:
            return round((time.time() - t0) * 1000, 1), f"error: {e.message}"

    def toast(app):
        ix = index(app)
        n = ix.get("toast")
        return (n.get_name(), sorted(states(n))) if n is not None else (None, [])

    def alerts(app):
        return [n.get_name() for n in index(app).values() if n.get_role() == Atspi.Role.ALERT]

    results["provenance"] = {
        "decsettings": {"path": a.bin, "sha256": sha256(a.bin)},
        "display": display,
        "pkg": subprocess.run(["pacman", "-Q", "at-spi2-core", "python-gobject", "xorg-server-xvfb", "xdotool", "dbus"], capture_output=True, text=True).stdout.split("\n"),
        "kernel": os.uname().release,
    }
    passed = True
    s = None
    try:
        Atspi.init()
        # A client deadline well below the hold: a reply that has to wait for the disk shows up as a timeout.
        Atspi.set_timeout(2500, 2500)
        scall("org.a11y.Bus", "/org/a11y/bus", "org.freedesktop.DBus.Properties", "Set", GLib.Variant("(ssv)", ("org.a11y.Status", "IsEnabled", GLib.Variant("b", True))))
        start_registry()

        def want(name):
            return not a.only or name in a.only.split(",")

        # ======================================================== 1. blocked disk: everything still answers
        if not want("base"):
            raise StopIteration
        s = Settings("blocked")
        app = find_app(s.p.pid)
        ix = wait(lambda: (lambda i: i if "ctl:r:general:default_binds" in i else None)(index(app)), "tree with the general pane")
        sw, step = ix["ctl:r:general:default_binds"], ix["ctl:r:general:chord_timeout_ms"]
        was = "checked" in states(sw)
        v0 = Atspi.Value.get_current_value(step)
        inc = Atspi.Value.get_minimum_increment(step) or 1.0
        before = s.config.read_bytes()
        gate.write_text("held by the harness\n")
        release = threading.Timer(HOLD_S, lambda: gate.unlink(missing_ok=True))
        t_hold = time.time()
        release.start()
        # pointer: the switch flips on screen at once
        t_click = time.time()
        s.click("r:general:default_binds")
        flipped = lambda: s.ctl("r:general:default_binds")["value"] == (not was)  # noqa: E731
        timed("pointer_click_to_paint", flipped, "switch painted flipped", t=0.4, t0=t_click)
        time.sleep(max(0.0, 0.4 - (time.time() - t_click)))
        snap = threads(s.p.pid)
        results["scenarios"]["threads_0.4s_after_click"] = snap
        main = next(t for t in snap if t["tid"] == s.p.pid)
        worker = [t for t in snap if t["comm"] == "decset-persist"]
        check("UI thread is not the one held in the blocked write", "nanosleep" not in main["wchan"], main)
        check("the persistence worker is the thread held in the blocked write", any("nanosleep" in t["wchan"] for t in worker), worker)
        if lat["pointer_click_to_paint"] is None:
            timed("pointer_click_to_paint", flipped, "switch painted flipped", t0=t_click)
        # AT: two increments, each answered while the disk is still held
        replies = [do_action(step, "increment") for _ in range(2)]
        results["scenarios"]["at_increment_replies"] = replies
        lat["at_increment_reply_max"] = max((r[0] for r in replies if r[0] is not None), default=None)
        held = time.time() - t_hold < HOLD_S - 0.3
        check("AT increments replied True while the disk was still held", held and all(r[1] is True for r in replies), {"replies": replies, "elapsed_s": round(time.time() - t_hold, 2)})
        name, st = toast(app)
        results["scenarios"]["toast_while_blocked"] = [name, st]
        check("AT toast says Saving… and is busy while held", held and name == "Saving…" and "busy" in st, [name, st])
        check("nothing was written while the disk was held", held and s.config.read_bytes() == before)
        # keyboard
        x("windowfocus", "--sync", s.wid, check_rc=False)
        f0 = (s.layout() or {}).get("focus")
        x("key", "Tab")
        timed("key_tab_to_focus_paint", lambda: (s.layout() or {}).get("focus") != f0, "focus moved")
        # search: Ctrl+F, type, then clear
        x("key", "ctrl+f")
        x("type", "--delay", "30", "gap")
        timed("search_typed_to_paint", lambda: (s.layout() or {}).get("search_text") == "gap", "search text painted")
        x("key", "BackSpace", "BackSpace", "BackSpace")
        timed("search_cleared_to_paint", lambda: (s.layout() or {}).get("search_text") == "", "search cleared")
        # paint: hovering another sidebar row repaints
        fr0 = s.frames()
        x("mousemove", "--window", s.wid, "110", "230")
        x("mousemove", "--window", s.wid, "110", "300")
        timed("hover_to_paint", lambda: s.frames() > fr0, "repainted after hover")
        check("all pointer/key/search/paint probes ran while the disk was still held", time.time() - t_hold < HOLD_S - 0.2, round(time.time() - t_hold, 2))
        # the disk comes back: the newest values, exactly
        want = before.decode() + f"default_binds = {str(not was).lower()}\nchord_timeout_ms = {int(v0 + 2 * inc)}\n"
        timed("release_to_file_committed", lambda: not gate.exists() and s.config.read_text() == want, "newest edits committed", HOLD_S + 15)
        check("file has the newest edits exactly (unknown key kept)", s.config.read_text() == want, {"got": s.config.read_text(), "want": want})
        applog = lambda: "\n".join(p.read_text(errors="replace") for p in (s.dir / "logs").rglob("*") if p.is_file())  # noqa: E731
        wait(lambda: "notice: Saved" in applog(), "Saved notice after the commit", 10)
        wait(lambda: "busy" not in toast(app)[1], "toast no longer busy", 5)
        name, st = toast(app)
        check("after the commit: Saved (logged), the AT toast is no longer busy", "busy" not in st and (name is None or name.startswith("Saved")), [name, st])
        census = [t["comm"] for t in threads(s.p.pid)]
        results["scenarios"]["thread_names"] = census
        check("one persistence worker and one file watcher", census.count("decset-persist") == 1 and sum(c.startswith("decsettings-wat") for c in census) == 1, census)
        log = applog() + (s.dir / "decsettings.out").read_text(errors="replace")
        results["scenarios"]["notices_logged"] = [l for l in log.splitlines() if "notice" in l][-6:]
        check("log shows no typed search text or config values", "gap" not in re.sub(r"gap_inner|gaps", "", log) and "chord_timeout_ms = " not in log)
        release.cancel()
        gate.unlink(missing_ok=True)
        s.close()
        check("blocked: process exited after close", s.p.returncode is not None, s.p.returncode)

        # ======================================================== 2. a failed write is reported, nothing written
        s = Settings("failure", fail="config.toml")
        app = find_app(s.p.pid)
        ix = wait(lambda: (lambda i: i if "ctl:r:general:default_binds" in i else None)(index(app)), "tree")
        sw = ix["ctl:r:general:default_binds"]
        was = "checked" in states(sw)
        before = s.config.read_bytes()
        ms, r = do_action(sw, "toggle")
        lat["failure_at_toggle_reply"] = ms
        check("AT toggle accepted (the write fails later, in the worker)", r is True, [ms, r])
        got = wait(lambda: [t for t in alerts(app) if "Not saved" in t] or None, "error alert", 10)
        check("the failure is on the page as an alert, with the reason", any("Input/output error" in t for t in got), got)
        check("the switch shows the saved value again", ("checked" in states(index(app)["ctl:r:general:default_binds"])) == was)
        check("the config file is byte-identical", s.config.read_bytes() == before)
        s.close()

        # ======================================================== 3. a theme transaction failing midway changes no byte
        s = Settings("theme-rollback", pane="appearance", fail="config.toml", extra_files={"decebal/dock-settings.json": "{\n  \"appSpacing\": 3,\n  \"futureDock\": 1\n}\n"})
        files = [s.config, s.dir / "decebal" / "decebal.json", s.dir / "decebal" / "dock-settings.json"]
        sums = [sha256(f) for f in files]
        undo_before = sorted(p.name for p in (s.dir / "decebal").rglob("*"))
        wait(lambda: s.ctl("l:tcard:apply:macos"), "the macOS package card", 10)
        s.click("l:tcard:apply:macos")
        wait(lambda: ((s.layout() or {}).get("notice") or "").startswith("Not applied"), "theme failure notice", 15)
        check("theme failure is reported", True, (s.layout() or {}).get("notice"))
        check("config.toml, decebal.json and dock-settings.json are byte-identical", [sha256(f) for f in files] == sums)
        after = [p for p in (s.dir / "decebal").rglob("*")]
        undo = [p for p in after if "theme-undo" in p.parts and p.name != "theme-undo"]
        tmps = [p.name for p in after + list(s.config.parent.iterdir()) if p.name.endswith(".tmp")]
        baks = {p.name[: -len(".bak-decsettings")]: sha256(p) for p in after + list(s.config.parent.iterdir()) if p.name.endswith(".bak-decsettings")}
        check("no undo record or temp file left behind", not undo and not tmps, {"undo": [str(p) for p in undo], "tmp": tmps, "before": undo_before})
        check("the one-time .bak-decsettings copies equal the originals", all(baks.get(f.name, sums[i]) == sums[i] for i, f in enumerate(files)), sorted(baks))
        s.close()

        # ======================================================== 4. closing while a write is blocked drains it
        s = Settings("close-drain")
        before = s.config.read_text()
        gate.write_text("held\n")
        s.click("r:general:default_binds")
        wait(lambda: s.ctl("r:general:default_binds")["value"] is not None, "painted")
        pump(0.3)
        x("windowfocus", "--sync", s.wid, check_rc=False)
        t_close = time.time()
        x("key", "ctrl+w")
        timed("close_to_window_gone", lambda: not x("search", "--onlyvisible", "--class", "DecSettings", check_rc=False), "window unmapped", 10)
        alive_while_held = s.p.poll() is None
        check("process still finishing the write while the disk is held", alive_while_held)
        threading.Timer(1.0, lambda: gate.unlink(missing_ok=True)).start()
        try:
            s.p.wait(20)
        except subprocess.TimeoutExpired:
            pass
        lat["close_to_exit"] = round((time.time() - t_close) * 1000, 1)
        check("process exited by itself after the disk came back (exit 0)", s.p.returncode == 0, s.p.returncode)
        check("the edit made just before closing is on disk", s.config.read_text() != before and "default_binds = " in s.config.read_text(), s.config.read_text())
        s.close()
    except StopIteration:
        pass
    except Exception as e:  # noqa: BLE001 - record any harness or app failure, then clean up
        passed = check(f"scenario aborted: {type(e).__name__}: {e}", False)
        if s is not None and s.p.poll() is None:
            results["threads_on_failure"] = threads(s.p.pid)
        if s is not None:
            s.close()
    s = None
    try:
        families(a, Settings, find_app, index, states, do_action, alerts, toast, check, wait, pump, timed, threads, x, gate, lat, results, Atspi)
    except Exception as e:  # noqa: BLE001 - record any harness or app failure, then clean up
        passed = check(f"scenario aborted: {type(e).__name__}: {e}", False)
        if s is not None and s.p.poll() is None:
            results["threads_on_failure"] = threads(s.p.pid)
    finally:
        gate.unlink(missing_ok=True)
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
    results["x_socket_removed"] = not Path(f"/tmp/.X11-unix/X{num}").exists()
    (ev / "results.json").write_text(json.dumps(results, indent=2, default=str) + "\n")
    return 0 if results["passed"] else 1



# ---------------------------------------------------------------- one representative per write family


def families(a, Settings, find_app, index, states, do_action, alerts, toast, check, wait, pump, timed, threads, x, gate, lat, results, Atspi):
    """Every family: the action answers while every fsync is held, nothing is written yet, the newest state lands
    exactly once the disk is back, and failures (injected EIO, external edits during the held fsync) are refused
    visibly without losing anyone's data."""
    import json as _json

    want = lambda name: not a.only or name in a.only.split(",")  # noqa: E731

    def hold(secs):
        gate.write_text("held\n")
        t0 = time.time()

        def release():
            gate.unlink(missing_ok=True)
            results.setdefault("gate_releases", []).append(round(time.time() - t0, 2))

        t = threading.Timer(secs, release)
        t.daemon = True
        t.start()
        return t, t0

    def node(app, key, t=10.0):
        return wait(lambda: index(app).get(key), f"node {key}", t)

    def held_for(t0, secs):
        return time.time() - t0 < secs - 0.3

    def quick(name, ms):
        lat[name] = ms
        return ms is not None and ms < 1000

    def set_value(n, v):
        t0 = time.time()
        r = Atspi.Value.set_current_value(n, v)
        return round((time.time() - t0) * 1000, 1), r

    started = []

    class Tracked(Settings):
        def __init__(self, *args, **kw):
            super().__init__(*args, **kw)
            started.append(self)

    def scenario(name, fn):
        if not want(name):
            return
        try:
            fn()
        except Exception as e:  # noqa: BLE001 - one family failing must not hide the others
            # where each thread waits (a persistence thread in a real fsync means the host disk stalled, not the app)
            stuck = {}
            for st in started:
                if st.p.poll() is None:
                    stuck[st.name] = [t for t in threads(st.p.pid) if t["comm"] in ("decset-persist",) or t["tid"] == st.p.pid]
            results.setdefault("family_threads_on_failure", {})[name] = stuck
            check(f"{name}: aborted: {type(e).__name__}: {e}", False, stuck)
        finally:
            gate.unlink(missing_ok=True)
            while started:
                started.pop().close()

    def external_write(path, text):
        """An editor in another process: temp file + rename, no lock (like most editors)."""
        tmp = path.with_name(f".{path.name}.editor.tmp")
        tmp.write_text(text)
        os.replace(tmp, path)

    # ---- JSON: a worker write held in fsync, someone else saves another key meanwhile -> both survive
    def json_merge():
        s = Tracked("json-merge", pane="appearance/dock", extra_files={"decebal/dock-settings.json": "{\n  \"appSpacing\": 3\n}\n"})
        app = find_app(s.p.pid)
        n = node(app, "ctl:j:dock:appSpacing")
        dock = s.dir / "decebal" / "dock-settings.json"
        t, t0 = hold(4.0)
        ms, r = set_value(n, 9.0)
        check("json: the AT value change answers while the disk is held", quick("json_set_value_reply", ms) and r is True and held_for(t0, 4.0), [ms, r])
        time.sleep(0.4)
        external_write(dock, "{\n  \"appSpacing\": 3,\n  \"editor\": \"theirs\"\n}\n")
        wait(lambda: not gate.exists() and _json.loads(dock.read_text()).get("appSpacing") == 9, "committed", 15)
        got = _json.loads(dock.read_text())
        check("json: their key saved during our held fsync and ours are both on disk", got.get("editor") == "theirs" and got.get("appSpacing") == 9, got)
        return s

    # ---- JSON: someone else changes the same key while ours is held -> refused, theirs kept
    def json_same_key():
        s = Tracked("json-same-key", pane="appearance/dock", extra_files={"decebal/dock-settings.json": "{\n  \"appSpacing\": 3\n}\n"})
        app = find_app(s.p.pid)
        n = node(app, "ctl:j:dock:appSpacing")
        dock = s.dir / "decebal" / "dock-settings.json"
        t, t0 = hold(3.0)
        set_value(n, 9.0)
        time.sleep(0.4)
        theirs = "{\n  \"appSpacing\": 20,\n  \"x\": 1\n}\n"
        external_write(dock, theirs)
        got = wait(lambda: [m for m in alerts(app) if "changed on disk" in m] or None, "conflict alert", 15)
        check("json: the same key changed behind our back is refused visibly", bool(got), got)
        check("json: their value is kept byte for byte", dock.read_text() == theirs)
        return s

    # ---- TOML: an editor saves config.toml while our write is held -> refused, theirs kept
    def toml_external():
        s = Tracked("toml-external")
        app = find_app(s.p.pid)
        sw = node(app, "ctl:r:general:default_binds")
        t, t0 = hold(3.0)
        ms, r = do_action(sw, "toggle")
        check("toml: the AT toggle answers while the disk is held", quick("toml_toggle_reply", ms) and r is True, [ms, r])
        time.sleep(0.4)
        theirs = "# saved by hand while Settings was writing\n[general]\nborder_px = 7\n"
        external_write(s.config, theirs)
        got = wait(lambda: [m for m in alerts(app) if "changed on disk" in m] or None, "conflict alert", 15)
        check("toml: refused at commit with a visible error", bool(got), got)
        check("toml: the hand-saved file is kept byte for byte", s.config.read_text() == theirs)
        return s

    # ---- theme: custom palette, a new package picked and then a mode change while held -> new design's colours
    def theme_palette():
        base = {"theme": {"name": "nord", "mode": "dark", "palette": "catppuccin", "accent": "#ff9f0a"}, "future": {"kept": True}}
        s = Tracked("theme-palette", pane="appearance", extra_files={"decebal/decebal.json": _json.dumps(base, indent=2) + "\n"})
        wait(lambda: s.ctl("l:tcard:apply:macos"), "macOS card", 10)
        t, t0 = hold(5.0)
        s.click("l:tcard:apply:macos")
        app = find_app(s.p.pid)
        tabs = wait(lambda: next((n for k, n in index(app).items() if k and k.endswith("ap:tab")), None), "appearance tabs", 5)
        pal = next(i for i in range(tabs.get_child_count()) if "alette" in (tabs.get_child_at_index(i).get_name() or ""))
        Atspi.Selection.select_child(tabs, pal)
        mode = wait(lambda: index(app).get("ctl:th:mode"), "mode control", 5)
        tm = time.time()
        r = Atspi.Selection.select_child(mode, 0)  # Light
        lat["theme_mode_select_reply"] = round((time.time() - tm) * 1000, 1)
        check("theme: the mode change answers while the package is still being saved", r is True and held_for(t0, 5.0) and lat["theme_mode_select_reply"] < 1000, [r, lat["theme_mode_select_reply"]])
        shell = s.dir / "decebal" / "decebal.json"
        wait(lambda: not gate.exists() and _json.loads(shell.read_text())["theme"].get("mode") == "light", "mode committed", 20)
        th = _json.loads(shell.read_text())["theme"]
        check("theme: the new design keeps its own colours (old palette and accent did not come back)", th.get("name") == "macos" and th.get("mode") == "light" and "palette" not in th and "accent" not in th, th)
        check("theme: unknown keys survived", _json.loads(shell.read_text()).get("future") == {"kept": True})
        return s

    # ---- Config & Presets editor: save while held, typing meanwhile stays unsaved
    def editor():
        s = Tracked("editor", pane="config")
        app = find_app(s.p.pid)
        ed = wait(lambda: next((n for n in index(app).values() if n.get_role() == Atspi.Role.TEXT and "editable" in states(n)), None), "editor", 10)
        # the editor opens decebal.json; valid JSON that keeps the unknown key
        target = s.dir / "decebal" / "decebal.json"
        text = '{\n  "theme": {"name": "nord", "mode": "dark"},\n  "future": {"kept": true},\n  "edited": 1\n}\n'
        Atspi.EditableText.set_text_contents(ed, text)
        save = node(app, "ctl:cfg:save")
        t, t0 = hold(4.0)
        ms, r = do_action(save, "click")
        check("editor: Save answers while the disk is held (validation and write run in the worker)", quick("editor_save_reply", ms) and r is True, [ms, r])
        Atspi.EditableText.insert_text(ed, len(text), "late", -1)
        check("editor: nothing written while held", "edited" not in target.read_text())
        wait(lambda: not gate.exists() and target.read_text() == text, "saved", 15)
        check("editor: exactly the saved text is on disk", target.read_text() == text, target.read_text())
        pump(0.5)
        check("editor: typing done during the save is still unsaved (Save stays enabled)", "enabled" in states(node(app, "ctl:cfg:save")) and "late" not in target.read_text())
        return s

    # ---- presets: an apply failing on config.toml rolls every file back; one held answers at once
    def preset():
        s = Tracked("preset", pane="config", fail="config.toml")
        app = find_app(s.p.pid)
        Atspi.Selection.select_child(node(app, "ctl:cfg:view"), 1)
        wait(lambda: s.ctl("cfg:apply:builtin/macos"), "the preset list", 10)
        s.click("cfg:apply:builtin/macos")
        files = [s.config, s.dir / "decebal" / "decebal.json"]
        sums = [sha256(f) for f in files]
        go = node(app, "ctl:cfg:apply-do")
        t, t0 = hold(3.0)
        ms, r = do_action(go, "click")
        check("preset: Apply answers while the disk is held", quick("preset_apply_reply", ms) and r is True, [ms, r])
        got = wait(lambda: [m for m in alerts(app) if "Not applied" in m] or None, "preset failure alert", 20)
        check("preset: the failing apply is reported", bool(got), got)
        check("preset: every file is byte-identical after the rollback", [sha256(f) for f in files] == sums)
        return s

    # ---- General > Restore original, held
    def restore():
        orig = "# the original\n[general]\nborder_px = 1\n"
        s = Tracked("restore", extra_files={"cfg/config.toml.bak-decsettings": orig})
        app = find_app(s.p.pid)
        b = node(app, "ctl:a:restore")
        t, t0 = hold(3.0)
        ms, r = do_action(b, "click")
        check("restore: answers while the disk is held", quick("restore_reply", ms) and r is True and s.config.read_text() != orig, [ms, r])
        wait(lambda: not gate.exists() and s.config.read_text() == orig, "restored", 15)
        check("restore: the original is back exactly", s.config.read_text() == orig)
        return s

    # ---- session settings (Accessibility > Reduce motion), held; then a failing write is taken back
    def session():
        s = Tracked("session", pane="accessibility")
        app = find_app(s.p.pid)
        sw = node(app, "ctl:s:acc:motion:")
        sess = s.dir / "cfg" / "decebal" / "session-settings.json"
        t, t0 = hold(3.0)
        ms, r = do_action(sw, "toggle")
        check("session: the toggle answers while the disk is held", quick("session_toggle_reply", ms) and r is True, [ms, r])
        check("session: nothing written while held", not sess.exists() or "\"reduce_motion\": true" not in sess.read_text())
        script = s.dir / "cfg" / "decebal" / "session-settings.sh"
        wait(lambda: not gate.exists() and script.exists() and sess.exists() and _json.loads(sess.read_text()).get("reduce_motion") is True, "session saved", 15)
        sh = (s.dir / "cfg" / "decebal" / "session-settings.sh").read_text()
        check("session: json and login script both carry the newest value", "DECEBAL_REDUCE_MOTION=1" in sh)
        return s

    # ---- notifications.toml, failing write: shown at once, taken back with an error
    def notifications():
        s = Tracked("notifications", pane="notifications", fail="notifications.toml")
        app = find_app(s.p.pid)
        sw = node(app, "ctl:s:not:dnd:")
        ms, r = do_action(sw, "toggle")
        check("notifications: the toggle answers", quick("notifications_toggle_reply", ms) and r is True, [ms, r])
        got = wait(lambda: [m for m in alerts(app) if "not saved" in m.lower()] or None, "notifications failure alert", 15)
        check("notifications: the failed write is reported", bool(got), got)
        wait(lambda: "checked" not in states(node(app, "ctl:s:not:dnd:")), "toggle taken back", 5)
        check("notifications: the switch shows the saved value again", True)
        f = s.dir / "cfg" / "decebal" / "notifications.toml"
        check("notifications: no file was written", not f.exists() or "dnd = true" not in f.read_text())
        return s

    # ---- the Wallhaven API key: held, private from creation, never in any log
    def wallhaven_key():
        key = "AbCdEfGh12345678AbCdEfGh12345678"
        # every request goes to a closed local port: nothing leaves the machine
        s = Tracked("wallhaven-key", pane="wallhaven", env_extra={"WALLHAVEN_API_BASE": "http://127.0.0.1:9"})
        app = find_app(s.p.pid)
        Atspi.Selection.select_child(node(app, "ctl:wh:tab"), 1)
        do_action(node(app, "ctl:wh:filters"), "click")
        f = wait(lambda: index(app).get("ctl:wh:key:secret"), "key field", 15)
        Atspi.EditableText.set_text_contents(f, key)
        t, t0 = hold(3.0)
        ms, r = do_action(f, "activate")
        check("wallhaven: saving the key answers while the disk is held", quick("wallhaven_key_reply", ms) and r is True, [ms, r])
        kf = s.dir / "cfg" / "decebal" / "wallhaven.key"
        wait(lambda: not gate.exists() and kf.exists(), "key saved", 15)
        check("wallhaven: key file 0600 from creation, no backup or temp copy", (kf.stat().st_mode & 0o777) == 0o600 and sorted(p.name for p in kf.parent.iterdir() if "wallhaven" in p.name and p.name != "wallhaven.json") == ["wallhaven.key"], sorted(p.name for p in kf.parent.iterdir()))
        logs = "".join(p.read_text(errors="replace") for p in (s.dir / "logs").rglob("*") if p.is_file()) + (s.dir / "decsettings.out").read_text(errors="replace")
        check("wallhaven: the key appears in no log line", key not in logs and "AbCdEfGh" not in logs)
        return s

    # ---- close while the disk never comes back: the window goes at once, the process gives up after the limit
    def close_limit():
        s = Tracked("close-limit")
        before = s.config.read_text()
        gate.write_text("held\n")
        s.click("r:general:default_binds")
        pump(0.5)
        x("windowfocus", "--sync", s.wid, check_rc=False)
        t0 = time.time()
        x("key", "ctrl+w")
        timed("close_limit_window_gone", lambda: not x("search", "--onlyvisible", "--class", "DecSettings", check_rc=False), "window unmapped", 10)
        try:
            s.p.wait(45)
        except subprocess.TimeoutExpired:
            pass
        lat["close_limit_exit"] = round((time.time() - t0) * 1000, 1)
        gate.unlink(missing_ok=True)
        logs = "".join(p.read_text(errors="replace") for p in (s.dir / "logs").rglob("*") if p.is_file())
        check("close limit: the process exits by itself after the drain limit (~30 s)", s.p.returncode is not None and 28_000 < lat["close_limit_exit"] < 40_000, [s.p.returncode, lat["close_limit_exit"]])
        check("close limit: the lost change is logged as not written, the file untouched", "were not written within 30 s" in logs and s.config.read_text() == before)
        return s

    # ---- a theme package: someone saves config.toml while an earlier file of the transaction is in fsync
    def theme_external():
        s = Tracked("theme-external", pane="appearance", extra_files={"decebal/dock-settings.json": "{\n  \"appSpacing\": 3\n}\n"})
        files = [s.config, s.dir / "decebal" / "decebal.json", s.dir / "decebal" / "dock-settings.json"]
        before = [f.read_text() for f in files]
        wait(lambda: s.ctl("l:tcard:apply:macos"), "the macOS card", 10)
        t, t0 = hold(3.0)
        s.click("l:tcard:apply:macos")
        time.sleep(0.5)  # the transaction's first file is now held in fsync
        theirs = "# saved by hand during the theme change\n[general]\nborder_px = 9\n"
        external_write(s.config, theirs)
        app = find_app(s.p.pid)
        got = wait(lambda: [m for m in alerts(app) if "Not applied" in m] or None, "theme refusal", 20)
        check("theme external: the transaction is refused visibly", bool(got), got)
        check("theme external: the hand-saved config.toml is kept byte for byte", s.config.read_text() == theirs)
        check("theme external: every other file is back byte for byte", [f.read_text() for f in files[1:]] == before[1:], [f.read_text() for f in files[1:]])
        check("theme external: the error says nothing was changed (all of ours undone)", any("nothing was changed" in m for m in got), got)
        return s

    # ---- the launcher style, held
    def launcher():
        s = Tracked("launcher", pane="launcher", extra_files={"decebal/decebal.json": _json.dumps({"theme": {"name": "nord"}, "launcher": {"style": "theme", "future": 1}}) + "\n"})
        app = find_app(s.p.pid)
        menu = node(app, "ctl:ln:style/menu")
        shell = s.dir / "decebal" / "decebal.json"
        t, t0 = hold(3.0)
        tm = time.time()
        r = Atspi.Selection.select_child(menu, 1)
        lat["launcher_select_reply"] = round((time.time() - tm) * 1000, 1)
        check("launcher: choosing a style answers while the disk is held", r is True and lat["launcher_select_reply"] < 1000 and held_for(t0, 3.0), [r, lat["launcher_select_reply"]])
        check("launcher: nothing written while held", _json.loads(shell.read_text())["launcher"]["style"] == "theme")
        wait(lambda: not gate.exists() and _json.loads(shell.read_text())["launcher"]["style"] != "theme", "launcher saved", 15)
        got = _json.loads(shell.read_text())["launcher"]
        check("launcher: the style is saved and the object's other keys kept", got.get("future") == 1, got)
        return s

    # ---- the editor: someone saves config.toml during a slow check-config
    def editor_validation():
        ctl = gate.parent / "slow-rdwmctl"
        s = Tracked("editor-validation", pane="config", env_extra={"RDWMCTL": str(ctl)})
        theirs = "# saved by hand during validation\n[general]\nborder_px = 9\n"
        ctl.write_text(f"#!/bin/sh\nsleep 0.3\nprintf '%s' '{theirs}' > '{s.config}'\nexit 0\n")
        ctl.chmod(0o755)
        app = find_app(s.p.pid)
        tabs = node(app, "ctl:cfg:tab")
        rd = next(i for i in range(tabs.get_child_count()) if "Window manager" in (tabs.get_child_at_index(i).get_name() or ""))
        Atspi.Selection.select_child(tabs, rd)
        ed = wait(lambda: next((n for n in index(app).values() if n.get_role() == Atspi.Role.TEXT and "editable" in states(n) and "[general]" in Atspi.Text.get_text(n, 0, -1)), None), "the config.toml buffer", 10)
        Atspi.EditableText.set_text_contents(ed, "[general]\nborder_px = 3\n")
        ms, r = do_action(node(app, "ctl:cfg:save"), "click")
        check("editor validation: Save answers while check-config runs", quick("editor_validation_reply", ms) and r is True, [ms, r])
        got = wait(lambda: [m for m in alerts(app) if "changed on disk" in m] or None, "refusal", 15)
        check("editor validation: a save made during validation is refused visibly", bool(got), got)
        check("editor validation: the hand-saved file is kept", s.config.read_text() == theirs, s.config.read_text())
        return s

    # ---- an asset choice: value and ownership in one write, an editor's save meanwhile kept
    def asset_choice():
        s = Tracked("asset-choice", pane="icons", extra_files={"decebal/decebal.json": _json.dumps({"theme": {"name": "nord"}, "system": {"enabled": False}}) + "\n"})
        app = find_app(s.p.pid)
        menu = node(app, "ctl:ic:cursor/menu")
        names = [menu.get_child_at_index(i).get_name() for i in range(menu.get_child_count())]
        pick = next((i for i, n in enumerate(names) if n and n not in ("Not set",) and "not installed" not in n), None)
        if pick is None:
            check("asset choice: no cursor theme installed on this machine to choose (skipped)", True, names)
            return s
        shell = s.dir / "decebal" / "decebal.json"
        t, t0 = hold(3.0)
        tm = time.time()
        r = Atspi.Selection.select_child(menu, pick)
        lat["asset_choice_reply"] = round((time.time() - tm) * 1000, 1)
        check("asset choice: answers while the disk is held", r is True and lat["asset_choice_reply"] < 1000 and held_for(t0, 3.0), [r, lat["asset_choice_reply"]])
        time.sleep(0.4)
        external_write(shell, _json.dumps({"theme": {"name": "nord"}, "system": {"enabled": False}, "editor": 1}) + "\n")
        wait(lambda: not gate.exists() and "asset_packs" in _json.loads(shell.read_text()), "choice committed", 15)
        doc = _json.loads(shell.read_text())
        check("asset choice: value and its ownership committed together, their key kept", doc.get("system", {}).get("cursorTheme") == names[pick] and doc["asset_packs"]["cursors"] == {"user": True} and doc.get("editor") == 1, doc)
        return s

    for name, fn in [("asset-choice", asset_choice), ("theme-external", theme_external), ("launcher", launcher), ("editor-validation", editor_validation), ("json-merge", json_merge), ("json-same-key", json_same_key), ("toml-external", toml_external), ("theme-palette", theme_palette), ("editor", editor), ("preset", preset), ("restore", restore), ("session", session), ("notifications", notifications), ("wallhaven-key", wallhaven_key), ("close-limit", close_limit)]:
        scenario(name, fn)

if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", required=True)
    ap.add_argument("--evidence", required=True)
    ap.add_argument("--label", default="candidate")
    ap.add_argument("--inner", action="store_true")
    ap.add_argument("--only", help="comma list: base and/or family scenario names")
    ap.add_argument("--timeout", type=int, default=600)
    a = ap.parse_args()
    sys.exit(inner(a) if a.inner else outer(a))
