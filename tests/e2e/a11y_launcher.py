#!/usr/bin/env python3
"""DRAFT native AT-SPI2 probe for declaunch. Private only. NOT RUN: written source-only, review before first use.

Runs the real declaunch binary, once per launcher style, inside a private HOME/XDG tree, a private
`dbus-run-session` (real at-spi-bus-launcher; the real at-spi2-registryd is started on the private accessibility bus,
as tests/e2e/a11y_settings.py does) and a private Xvfb on a reserved high display, and drives it as a screen reader
would: GI Atspi only (read the tree, EditableText on the search field, the Action interface). No pointer events and
no synthetic keys. The fixture apps are three private `.desktop` files whose `Exec=` writes a marker file in the
private tree, so a launch is proven by *which* marker appeared, never by a real application starting.

    python3 -I tests/e2e/a11y_launcher.py --bin target/debug/declaunch --evidence ~/.cache/decebal-a11y-evidence/launcher

Writes <evidence>/results.json. Same outer/inner shape and cleanup rules as a11y_settings.py (it never touches the
live session bus, display, input, configuration or processes; only processes carrying the private root in their
environment are ever signalled). Not claimed: Orca speech, Wayland, bar or dock.
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve()
STYLES = ["default", "dux", "windows", "gnome", "kde", "xfce", "blackberry", "ukui"]
APPS = {"probe-alpha": "Probe Alpha", "probe-beta": "Probe Beta", "probe-gamma": "Probe Gamma"}


class Fail(Exception):
    pass


def outer(a):
    ev = Path(a.evidence).expanduser().resolve()
    ev.mkdir(parents=True, exist_ok=True)
    root = ev / "root"
    if root.exists():
        shutil.rmtree(root)
    for d in ["home", "cfg", "data/applications", "cache", "state", "run", "tmp", "marks"]:
        (root / d).mkdir(parents=True)
    os.chmod(root / "run", 0o700)
    for stem, name in APPS.items():
        (root / "data/applications" / f"{stem}.desktop").write_text(f"[Desktop Entry]\nType=Application\nName={name}\nComment=Fixture {name}\nExec=sh -c 'echo {stem} > {root}/marks/{stem}'\nCategories=Utility;\n")
    drop = ("DBUS_", "AT_SPI", "ATSPI_", "DECEBAL_", "GTK_", "GDK_", "QT_", "XDG_", "WAYLAND_", "GSETTINGS_", "NO_AT_BRIDGE", "RDWM")
    env = {k: v for k, v in os.environ.items() if not k.startswith(drop) and k not in ("DISPLAY", "XAUTHORITY", "SESSION_MANAGER", "PULSE_SERVER")}
    env.update(HOME=str(root / "home"), XDG_CONFIG_HOME=str(root / "cfg"), XDG_DATA_HOME=str(root / "data"), XDG_DATA_DIRS=str(root / "data"), XDG_CACHE_HOME=str(root / "cache"), XDG_STATE_HOME=str(root / "state"), XDG_RUNTIME_DIR=str(root / "run"), TMPDIR=str(root / "tmp"), GSETTINGS_BACKEND="memory", LANG="C.UTF-8")
    cmd = ["dbus-run-session", "--", sys.executable, "-I", str(HERE), "--inner", "--bin", str(Path(a.bin).resolve()), "--evidence", str(ev)]
    with open(ev / "session.log", "w") as log:
        r = subprocess.run(cmd, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=a.timeout)
    res = json.loads((ev / "results.json").read_text()) if (ev / "results.json").exists() else {"passed": False, "error": "no results"}
    res["session_exit"] = r.returncode
    left = []
    for p in Path("/proc").iterdir():
        if p.name.isdigit():
            try:
                if str(root).encode() in (p / "environ").read_bytes():
                    left.append(int(p.name))
            except OSError:
                pass
    for pid in left:
        try:
            os.kill(pid, 15)  # provably ours: the private root is in their environment
        except OSError:
            pass
    res["leftover_private_processes"] = left
    (ev / "results.json").write_text(json.dumps(res, indent=2) + "\n")
    print(json.dumps({k: res.get(k) for k in ("passed", "failed", "session_exit", "leftover_private_processes")}, indent=2))
    return 0 if res.get("passed") and not left else 1


def inner(a):
    import gi

    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi, Gio, GLib

    ev = Path(a.evidence)
    root = ev / "root"
    checks, owned = [], []

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
            time.sleep(0.01)

    def wait(fn, why, t=8.0):
        end, last = time.time() + t, None
        while time.time() < end:
            try:
                v = fn()
                if v:
                    return v
            except Exception as e:  # noqa: BLE001 - keep polling, report the last error
                last = e
            pump(0.05)
        raise Fail(f"timeout: {why} (last error: {last!r})")

    # reserved high display, never :0 (see the Xvfb notes in tests/e2e/a11y_settings.py)
    num = next(n for n in range(700, 760) if not Path(f"/tmp/.X11-unix/X{n}").exists() and not Path(f"/tmp/.X{n}-lock").exists())
    display = f":{num}"
    xvfb = subprocess.Popen(["Xvfb", display, "-screen", "0", "1920x1080x24", "-nolisten", "tcp", "-noreset"], stdout=open(ev / "xvfb.log", "w"), stderr=subprocess.STDOUT, start_new_session=True)
    owned.append(xvfb)
    os.environ["DISPLAY"] = display
    session = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    session.call_sync("org.a11y.Bus", "/org/a11y/bus", "org.freedesktop.DBus.Properties", "Set", GLib.Variant("(ssv)", ("org.a11y.Status", "IsEnabled", GLib.Variant("b", True))), None, 0, 5000, None)
    addr = session.call_sync("org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus", "GetAddress", None, GLib.VariantType("(s)"), 0, 5000, None).unpack()[0]
    owned.append(subprocess.Popen(["/usr/lib/at-spi2-registryd"], env={**os.environ, "AT_SPI_BUS_ADDRESS": addr}, stdout=open(ev / "registry.log", "w"), stderr=subprocess.STDOUT, start_new_session=True))

    def index(acc, limit=4000):
        out, queue = {}, [acc]
        while queue and len(out) < limit:
            n = queue.pop(0)
            try:
                out[n.get_accessible_id()] = n
                queue.extend(c for c in (n.get_child_at_index(i) for i in range(n.get_child_count())) if c is not None)
            except GLib.Error:
                continue
        return out

    def states(n):
        s = n.get_state_set()
        return {st.value_nick for st in Atspi.StateType.__enum_values__.values() if s.contains(st)}

    def act(n, name):
        for i in range(Atspi.Action.get_n_actions(n)):
            if Atspi.Action.get_action_name(n, i) == name:
                return Atspi.Action.do_action(n, i)
        raise Fail(f"{n.get_accessible_id()} has no action {name}")

    def start(style, scenario):
        d = root / "apps" / f"{style}-{scenario}"
        (d / "decebal").mkdir(parents=True, exist_ok=True)
        (d / "decebal" / "decebal.json").write_text(json.dumps({"launcher": {"style": style}}))
        env = {**os.environ, "DECEBAL_CONFIG": str(d / "decebal"), "DECEBAL_BACKEND": "x11", "DECEBAL_A11Y": "1", "DECEBAL_LOG": "debug", "DECEBAL_LOG_DIR": str(d / "logs")}
        p = subprocess.Popen([a.bin, "open"], env=env, stdout=open(d / "declaunch.log", "w"), stderr=subprocess.STDOUT, start_new_session=True)
        owned.append(p)
        app = wait(lambda: next((c for c in (Atspi.get_desktop(0).get_child_at_index(i) for i in range(Atspi.get_desktop(0).get_child_count())) if c is not None and c.get_name() == "Launcher" and c.get_process_id() == p.pid), None), f"{style}: the Launcher application")
        return p, app

    passed = True
    try:
        for style in STYLES:
            # --- the tree: search field, lists, exact bounds, stable ids bound to desktop ids
            p, app = start(style, "tree")
            tree = wait(lambda: (lambda t: t if any(k.endswith("app:probe-beta") for k in t) else None)(index(app)), f"{style}: the fixture apps are described")
            check(f"{style}: search is an editable entry", "editable" in states(tree["search"]) and tree["search"].get_role_name() == "entry")
            beta = next(n for k, n in tree.items() if k.endswith("app:probe-beta"))
            check(f"{style}: the app node is named and described", beta.get_name() == "Probe Beta" and "Fixture Probe Beta" in beta.get_description(), beta.get_description())
            ext = Atspi.Component.get_extents(beta, Atspi.CoordType.WINDOW)
            check(f"{style}: the app node has real bounds", ext.width > 0 and ext.height > 0, [ext.x, ext.y, ext.width, ext.height])
            before = beta.get_accessible_id()
            # --- search through EditableText: results follow, ids stay
            Atspi.EditableText.set_text_contents(tree["search"], "gamma")
            after = wait(lambda: next((n for k, n in index(app).items() if k.endswith("app:probe-gamma")), None), f"{style}: Gamma among the results")
            check(f"{style}: a search keeps desktop-id keyed nodes", after.get_name() == "Probe Gamma" and before.endswith("app:probe-beta"))
            Atspi.EditableText.set_text_contents(tree["search"], "zzzz")
            wait(lambda: "announce:empty" in index(app), f"{style}: the empty search is announced")
            Atspi.EditableText.set_text_contents(tree["search"], "")
            wait(lambda: "announce:empty" not in index(app), f"{style}: the announcement goes away")
            p.terminate()
            p.wait(5)
            # --- activation launches exactly that app and closes the (single) launcher
            p, app = start(style, "launch")
            tree = wait(lambda: (lambda t: t if any(k.endswith("app:probe-beta") for k in t) else None)(index(app)), f"{style}: apps described again")
            act(next(n for k, n in tree.items() if k.endswith("app:probe-beta")), "activate")
            wait(lambda: (root / "marks" / "probe-beta").exists(), f"{style}: Beta was started")
            check(f"{style}: only the activated app was started", sorted(x.name for x in (root / "marks").iterdir()) == ["probe-beta"])
            check(f"{style}: the launcher closed after the launch", wait(lambda: p.poll() is not None, f"{style}: launcher exit") is not None)
            (root / "marks" / "probe-beta").unlink()
    except Fail as e:
        passed = False
        check("scenario", False, str(e))
    finally:
        for p in owned:
            if p.poll() is None:
                p.terminate()
        results = {"passed": passed and all(c["ok"] for c in checks), "failed": [c["name"] for c in checks if not c["ok"]], "checks": checks}
        (ev / "results.json").write_text(json.dumps(results, indent=2) + "\n")
    return 0 if results["passed"] else 1


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", required=True)
    ap.add_argument("--evidence", required=True)
    ap.add_argument("--inner", action="store_true")
    ap.add_argument("--timeout", type=int, default=600)
    a = ap.parse_args()
    return inner(a) if a.inner else outer(a)


if __name__ == "__main__":
    sys.exit(main())
