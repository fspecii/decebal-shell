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
import fcntl, hashlib, json, os, signal, subprocess, tempfile, time, shutil, uuid

repo = Path(os.environ.get('DECEBAL_QA_REPO', Path(__file__).resolve().parents[2])).resolve()
target = Path(os.environ.get("CARGO_TARGET_DIR", repo / "target"))
bins = Path(os.environ.get("LAUNCHER_BIN_DIR", target / "debug"))
helper = Path(os.environ.get("LAUNCHAPP_BIN", bins / "examples/launchapp"))
evidence = Path(os.environ.get("LAUNCHER_RAPID_EVIDENCE", "/var/tmp/launcher-rapid-evidence"))
evidence.mkdir(mode=0o700, parents=True, exist_ok=False)
# -I omits the script directory; import only this trusted fixture directory.
import sys
sys.path.insert(0, str(repo / "tests/e2e"))
import private_x11
from private_x11 import start_private_xvfb
root = Path(tempfile.mkdtemp(prefix="decebal-launcher-rapid-", dir=os.environ.get("LAUNCHER_RAPID_WORK", "/var/tmp")))
root.chmod(0o700)
token = uuid.uuid4().hex
marker = ('LAUNCHER_RAPID_QA_TOKEN=' + token).encode()
env = {'LANG': 'C.UTF-8', 'LAUNCHER_RAPID_QA_TOKEN': token}
if os.environ.get('E2E_PARENT_TOKEN'):
    env['E2E_PARENT_TOKEN'] = os.environ['E2E_PARENT_TOKEN']
for k, v in {"HOME": "home", "XDG_CONFIG_HOME": "cfg", "XDG_CONFIG_DIRS": "nocfg", "XDG_DATA_HOME": "data", "XDG_DATA_DIRS": "nodata", "XDG_CACHE_HOME": "cache",
             "XDG_STATE_HOME": "state", "XDG_RUNTIME_DIR": "run", "DECEBAL_CONFIG": "shell", "DECEBAL_THEMES": "themes", "DECEBAL_SYSTEM_THEMES": "systemthemes"}.items():
    env[k] = str(root / v)
    Path(env[k]).mkdir(parents=True, exist_ok=True)
os.chmod(env["XDG_RUNTIME_DIR"], 0o700)
env.update(DECEBAL_BACKEND="x11", DECEBAL_SYSTEM_AUTOMATIC="0",
           DBUS_SYSTEM_BUS_ADDRESS="unix:path=" + str(root / "no-system-bus"),
           DBUS_SESSION_BUS_ADDRESS="unix:path=" + str(root / "no-session-bus"),
           PULSE_SERVER="unix:" + str(root / "no-audio"))
source_binary = (bins / 'declaunch').resolve(strict=True)
source_helper = helper.resolve(strict=True)
isolated = root / 'bin'; isolated.mkdir()
binary = isolated / 'declaunch'; helper = isolated / 'launchapp'
for src, dst in ((source_binary, binary), (source_helper, helper)):
    shutil.copy2(src, dst); dst.chmod(0o555)
    assert hashlib.sha256(src.read_bytes()).digest() == hashlib.sha256(dst.read_bytes()).digest()
for name in ('decnotify','decsettings','rdwmctl','decutil','xdg-open','systemctl','loginctl','nmcli','pactl','wpctl','playerctl','gsettings','curl'):
    p = isolated / name; p.write_text('#!/bin/sh\nexit 1\n'); p.chmod(0o700)
for name in ('fc-list','fc-match'):
    p = isolated / name; p.write_text('#!/bin/sh\nexec /usr/bin/'+name+' "$@"\n'); p.chmod(0o700)
env['PATH'] = str(isolated)
env['RDWMCTL'] = str(isolated / 'rdwmctl')
notice = root / "notify.sh"
notice.write_text('#!/bin/sh\nprintf "%s\\n" "$@" >> "$XDG_STATE_HOME/notices"\n')
notice.chmod(0o755)
env["DECEBAL_NOTIFY"] = str(notice)
apps = Path(env["XDG_DATA_HOME"]) / "applications"
apps.mkdir()
owned, logs = [], []
records = {"passed": False, "declaunch_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
           "helper_sha256": hashlib.sha256(helper.read_bytes()).hexdigest(),
           "fixture_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
           "commit": subprocess.run(["/usr/bin/git", "-C", str(repo), "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip(),
           "work_dir": str(root), "scope": "owned private X11 rapid launcher lifecycle; no physical input or native Start-button click"}
xvfb = None

def interrupted(signum, frame):
    raise SystemExit(128 + signum)
for sig in (signal.SIGINT, signal.SIGTERM):
    signal.signal(sig, interrupted)

def marked():
    found = []
    for p in Path('/proc').iterdir():
        try:
            if p.name.isdigit() and p.stat().st_uid == os.getuid() and marker in (p/'environ').read_bytes().split(b'\0'):
                found.append(int(p.name))
        except OSError:
            pass
    return found

def send_owned(pid, sig):
    fd = os.pidfd_open(pid)
    try:
        p = Path('/proc')/str(pid)
        assert p.stat().st_uid == os.getuid()
        assert marker in (p/'environ').read_bytes().split(b'\0')
        signal.pidfd_send_signal(fd, sig)
    finally:
        os.close(fd)

def prove_x():
    assert xvfb is not None and private_x11.listeners_owned(xvfb, int(env['DISPLAY'][1:])), 'lost private X server ownership'



def start(args, label):
    prove_x()
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
    """Exact fixture token and copied executable, never process-name matching."""
    found = []
    want = binary.stat()
    for pid in marked():
        try:
            st = (Path('/proc')/str(pid)/'exe').stat()
            if (st.st_dev, st.st_ino) == (want.st_dev, want.st_ino):
                found.append(pid)
        except OSError:
            pass
    return found


def mapped():
    prove_x()
    return subprocess.run(["/usr/bin/xdotool", "search", "--onlyvisible", "--class", "declaunch"], env=env, capture_output=True, text=True, timeout=5).stdout.split()


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
    xvfb, env["DISPLAY"] = start_private_xvfb(env=env, log=log, executable="/usr/bin/Xvfb"); owned.append(xvfb)
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
    send_owned(e.pid, signal.SIGTERM)
    assert e.wait(timeout=8) == -signal.SIGTERM, e.returncode
    settle(False, "after sigterm")
    records["genuine_sigterm_status"] = e.returncode
    records["passed"] = True
except BaseException as exc:
    records.update(passed=False, error=repr(exc))
finally:
    cleanup_errors = []
    # Ask the holder to close through its own typed command before terminating clients.
    if xvfb is not None:
        try:
            prove_x()
            if holder() is not None:
                q = subprocess.run([str(binary), 'close'], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5)
                assert q.returncode == 0
                wait(lambda: holder() is None, 'typed close during cleanup')
        except BaseException as exc:
            cleanup_errors.append('typed close: ' + repr(exc))
    for p in reversed(owned):
        if p is xvfb:
            continue
        # poll may reap; never signal a reaped Popen's number without token revalidation.
        if p.poll() is None:
            try:
                send_owned(p.pid, signal.SIGTERM)
                p.wait(timeout=5)
            except ProcessLookupError:
                pass
            except subprocess.TimeoutExpired:
                records['forced_cleanup'] = True
                try:
                    send_owned(p.pid, signal.SIGKILL); p.wait(timeout=3)
                except BaseException as exc:
                    cleanup_errors.append(repr(exc))
            except BaseException as exc:
                cleanup_errors.append(repr(exc))
    expected = {xvfb.pid} if xvfb is not None else set()
    until = time.monotonic() + 3
    while set(marked()) - expected and time.monotonic() < until:
        time.sleep(.05)
    records['detached_pids'] = sorted(set(marked()) - expected)
    if records['detached_pids']:
        records['passed'] = False
        for pid in records['detached_pids']:
            try:
                send_owned(pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            except BaseException as exc:
                cleanup_errors.append(repr(exc))
    if xvfb is not None:
        private_x11._owned.discard(xvfb)
        try:
            send_owned(xvfb.pid, signal.SIGTERM); xvfb.wait(timeout=5)
        except ProcessLookupError:
            pass
        except subprocess.TimeoutExpired:
            records['forced_cleanup'] = True
            try:
                send_owned(xvfb.pid, signal.SIGKILL); xvfb.wait(timeout=3)
            except BaseException as exc:
                cleanup_errors.append(repr(exc))
        except BaseException as exc:
            cleanup_errors.append(repr(exc))
    until = time.monotonic() + 3
    while marked() and time.monotonic() < until:
        time.sleep(.05)
    records['leftover_pids'] = marked()
    records['cleanup_errors'] = cleanup_errors
    if cleanup_errors or records['leftover_pids'] or records.get('forced_cleanup'):
        records['passed'] = False
    for f in logs:
        try:
            f.close()
        except BaseException as exc:
            cleanup_errors.append(repr(exc)); records['passed'] = False
    (evidence / 'result.json').write_text(json.dumps(records, indent=2) + '\n')
    print(json.dumps(records, indent=2))
sys.exit(0 if records['passed'] else 1)
