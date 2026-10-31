#!/usr/bin/env python3
"""Rapid launcher open/close regression in a private X11 session; never touches the live desktop.

    LAUNCHER_RAPID_EVIDENCE=/var/tmp/launcher-rapid python3 tests/e2e/launcher-rapid-toggle.py

Several toggles in quick succession (each started as soon as the previous one either exited or took the lock, before
any window maps) must leave exactly the state the last one asked for, no stray declaunch process, every intentional
close with status 0 and no false "EXITED EARLY" report from the shared launch monitor. It also checks the
open/close verbs of the panel contract and that a genuine SIGTERM is still a failure.
LAUNCHER_BIN_DIR picks the binaries, LAUNCHAPP_BIN the monitored launch helper.
"""
from pathlib import Path
import fcntl, hashlib, json, os, signal, subprocess, tempfile, time

repo = Path(__file__).resolve().parents[2]
target = Path(os.environ.get("CARGO_TARGET_DIR", repo / "target"))
bins = Path(os.environ.get("LAUNCHER_BIN_DIR", target / "debug"))
helper = Path(os.environ.get("LAUNCHAPP_BIN", bins / "examples/launchapp"))
evidence = Path(os.environ.get("LAUNCHER_RAPID_EVIDENCE", "/var/tmp/launcher-rapid-evidence"))
evidence.mkdir(parents=True, exist_ok=True)
# -I omits the script directory; import only this trusted fixture directory.
import sys
sys.path.insert(0, str(Path(__file__).resolve().parent))
from private_x11 import start_private_xvfb
root = Path(tempfile.mkdtemp(prefix="decebal-launcher-rapid-", dir=os.environ.get("LAUNCHER_RAPID_WORK", "/var/tmp")))
root.chmod(0o700)
env = dict(os.environ)
for k in list(env):
    if k in ("DISPLAY", "WAYLAND_DISPLAY", "XAUTHORITY", "RDWM_SOCK", "SESSION_MANAGER", "AT_SPI_BUS_ADDRESS", "DBUS_SESSION_BUS_ADDRESS") or k.startswith(("DECEBAL_", "DECSPOT_", "DECSETTINGS_")):
        env.pop(k, None)
for k, v in {"HOME": "home", "XDG_CONFIG_HOME": "cfg", "XDG_CONFIG_DIRS": "nocfg", "XDG_DATA_HOME": "data", "XDG_DATA_DIRS": "nodata", "XDG_CACHE_HOME": "cache",
             "XDG_STATE_HOME": "state", "XDG_RUNTIME_DIR": "run", "DECEBAL_CONFIG": "shell", "DECEBAL_THEMES": "themes", "DECEBAL_SYSTEM_THEMES": "systemthemes"}.items():
    env[k] = str(root / v)
    Path(env[k]).mkdir(parents=True, exist_ok=True)
os.chmod(env["XDG_RUNTIME_DIR"], 0o700)
env.update(DECEBAL_BACKEND="x11", DECEBAL_SYSTEM_AUTOMATIC="0", DBUS_SYSTEM_BUS_ADDRESS="unix:path=" + str(root / "no-system-bus"))
notice = root / "notify.sh"
notice.write_text('#!/bin/sh\nprintf "%s\\n" "$@" >> "$XDG_STATE_HOME/notices"\n')
notice.chmod(0o755)
env["DECEBAL_NOTIFY"] = str(notice)
binary = bins / "declaunch"
apps = Path(env["XDG_DATA_HOME"]) / "applications"
apps.mkdir()
owned, logs = [], []
records = {"declaunch_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(), "commit": subprocess.run(["git", "-C", str(repo), "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip()}


def start(args, label):
    f = (root / (label + ".log")).open("w")
    logs.append(f)
    p = subprocess.Popen(list(map(str, args)), env=env, stdout=f, stderr=subprocess.STDOUT, start_new_session=True)
    owned.append(p)
    return p


def wait(fn, what, seconds=8):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        v = fn()
        if v:
            return v
        time.sleep(0.005)
    raise AssertionError(what)


def holder():
    path = Path(env["XDG_RUNTIME_DIR"]) / "declaunch.lock"
    try:
        with path.open("r") as f:
            try:
                fcntl.flock(f, fcntl.LOCK_EX | fcntl.LOCK_NB)
                return None
            except BlockingIOError:
                return int(f.read().strip() or 0) or None
    except FileNotFoundError:
        return None


def ours():
    """declaunch processes of this private session (matched by its runtime dir), zombies excluded."""
    found = []
    for d in Path("/proc").iterdir():
        if not d.name.isdigit():
            continue
        try:
            if (d / "comm").read_text().strip() != "declaunch" or "Z" in (d / "stat").read_text().split(")")[1].split()[0]:
                continue
            if (b"XDG_RUNTIME_DIR=" + env["XDG_RUNTIME_DIR"].encode() + b"\0") in (d / "environ").read_bytes():
                found.append(int(d.name))
        except (FileNotFoundError, ProcessLookupError, PermissionError, IndexError):
            pass
    return found


def mapped():
    return subprocess.run(["xdotool", "search", "--onlyvisible", "--class", "declaunch"], env=env, capture_output=True, text=True, timeout=5).stdout.split()


def invoke(args, label):
    """Starts one invocation and returns once it has exited or holds the lock (no waiting for a window)."""
    p = start([binary, *args], label)
    wait(lambda: p.poll() is not None or holder() == p.pid, f"{label}: neither exited nor opened")
    return p


def settle(expect_open, label):
    time.sleep(0.4)
    if expect_open:
        wait(mapped, f"{label}: the launcher should be open")
    else:
        wait(lambda: not mapped(), f"{label}: the launcher should be closed")
    procs = ours()
    assert len(procs) == (1 if expect_open else 0), (label, "stray or missing declaunch", procs)
    assert (holder() is not None) == expect_open, (label, "lock state")
    return procs


def launchlog():
    p = Path(env["XDG_STATE_HOME"]) / "decebal/launch.log"
    return p.read_text() if p.exists() else ""


try:
    log = (root / "xvfb.log").open("w"); logs.append(log)
    xvfb, env["DISPLAY"] = start_private_xvfb(env=env, log=log); owned.append(xvfb)
    panel = ["toggle", "--edge", "bottom", "--anchor", "640,782", "--thickness", "36"]
    rounds = []
    # A: bursts of N toggles, each one fired as soon as the previous has exited or taken the lock.
    for n in [2, 3, 4, 5, 7, 8]:
        procs = [invoke(panel if k % 2 == 0 else ["toggle"], f"burst{n}-{k}") for k in range(n)]
        state = settle(n % 2 == 1, f"burst of {n}")
        closers = [p for p in procs if p.poll() is not None]
        assert all(p.returncode == 0 for p in closers), [(p.args, p.returncode) for p in closers]
        if n % 2 == 1:
            invoke(["toggle"], f"burst{n}-final-close")
            settle(False, f"after burst {n}")
        for p in procs:
            p.wait(timeout=8)
            assert p.returncode == 0, ("an intentional close must exit 0", p.args, p.returncode)
        rounds.append({"toggles": n, "ended_open": n % 2 == 1, "all_exit_0": True, "stray_processes": 0})
    records["bursts"] = rounds

    # B: the same through the shared launch monitor (what the panel's Start button uses): no false crash report.
    entry = apps / "declaunch-toggle.desktop"
    entry.write_text(f"[Desktop Entry]\nType=Application\nName=Toggle fixture\nExec={binary} toggle --edge bottom --anchor 640,782 --thickness 36\n")
    before = launchlog().count("EXITED EARLY")
    for k in range(6):
        m = start([helper, entry.stem, "2"], f"monitored-{k}")
        wait(lambda: holder() is not None if k % 2 == 0 else holder() is None, f"monitored toggle {k}")
    for p in owned:
        if p.args and str(p.args[0]) == str(helper):
            p.wait(timeout=15)
    settle(False, "monitored toggles")
    records["monitor_false_reports"] = launchlog().count("EXITED EARLY") - before
    assert records["monitor_false_reports"] == 0, launchlog()

    # C: the open / close verbs.
    a = invoke(["open"], "open")
    first = settle(True, "open")
    began = time.monotonic()
    b = invoke(["open"], "open-again")
    assert b.wait(timeout=8) == 0
    records["open_on_running_returns_s"] = round(time.monotonic() - began, 3)
    assert records["open_on_running_returns_s"] < 1.0, "open must not wait for the running launcher to exit"
    assert settle(True, "open again") == first, "open leaves the running launcher alone"
    c = invoke(["close"], "close")
    assert c.wait(timeout=8) == 0
    settle(False, "close")
    assert a.wait(timeout=8) == 0, "closed by `close`: status 0"
    d = invoke(["close"], "close-again")
    assert d.wait(timeout=8) == 0
    settle(False, "close never opens")
    records["verbs"] = {"open_is_idempotent": True, "close_never_opens": True}

    # D: a genuine SIGTERM is still a failure status (only the intentional USR1 close is clean).
    e = invoke(["open"], "sigterm")
    wait(mapped, "sigterm target mapped")
    e.send_signal(signal.SIGTERM)
    assert e.wait(timeout=8) == -signal.SIGTERM, e.returncode
    settle(False, "after sigterm")
    records["genuine_sigterm_status"] = e.returncode
    records["passed"] = True
finally:
    for pid in ours():
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    for p in reversed(owned):
        if p.poll() is None:
            p.kill()
        try:
            p.wait(timeout=5)
        except subprocess.TimeoutExpired:
            pass
    for f in logs:
        f.close()
    (evidence / "result.json").write_text(json.dumps(records, indent=1) + "\n")
    print(json.dumps(records, indent=1))
