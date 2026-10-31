"""Coordinator lifetime for a contained job.

SIGKILL cannot run a cleanup handler. The contained command is exec'd by a
fresh single-threaded launcher that sets PR_SET_PDEATHSIG and rechecks the
parent before exec. That is not a Python preexec_fn. --kill-child then binds
the namespace init to that unshare process, so the coordinator's death
cascades. A guardian that is not tied to the coordinator's death holds a
duplicate of the slot lock until the owned tree is gone. An unverified drain
does not release the lock. A missing, truncated, or malformed drain is
unverified. Each contained track is reserved pending before it is spawned.
Release requires every such track to be proven on its own. A drain proves a
track only when its JSON says namespace_empty true and its invocation matches
that track's nonce. A file left behind by an earlier run does not match. The
other proof is that the track is no longer pending and each of its own init
pids has an integer start and is dead. A completed build does not prove a
later command. An unknown start keeps the lock.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import signal
import subprocess
import time

_SIGNALS = (signal.SIGTERM, signal.SIGHUP)

LAUNCHER_SOURCE = """\
import ctypes, ctypes.util, os, signal, sys
PR_SET_PDEATHSIG = 1
args = sys.argv[1:]
if len(args) < 3 or "--" not in args:
    os._exit(2)
parent = int(args[0])
cmd = args[args.index("--") + 1 :]
if not cmd:
    os._exit(2)
libc = ctypes.CDLL(ctypes.util.find_library("c"), use_errno=True)
libc.prctl.restype = ctypes.c_int
if libc.prctl(PR_SET_PDEATHSIG, signal.SIGKILL, 0, 0, 0) != 0:
    os._exit(127)
if os.getppid() != parent:
    os.kill(os.getpid(), signal.SIGKILL)
    os._exit(128 + 9)
os.execvpe(cmd[0], cmd, os.environ)
os._exit(127)
"""

GUARDIAN_SOURCE = """\
import json, os, signal, sys, time
args = sys.argv[1:]
if args[:1] != ["--fd"] or "--watch" not in args or "--ack" not in args:
    os._exit(2)
lock_fd = int(args[1])
watch_path = args[args.index("--watch") + 1]
ack_path = args[args.index("--ack") + 1]
os.set_inheritable(lock_fd, False)


def start_of(pid):
    try:
        stat = open(f"/proc/{pid}/stat", encoding="utf-8").read()
        rest = stat[stat.rfind(")") + 2 :].split()
        return int(rest[19])
    except (OSError, IndexError, ValueError):
        return None


def alive_match(pid, start):
    if not isinstance(start, int) or start_of(pid) != start:
        return False
    try:
        text = open(f"/proc/{pid}/status", encoding="utf-8").read()
    except OSError:
        return False
    for line in text.splitlines():
        if line.startswith("State:"):
            return line.split()[1] not in {"Z", "X"}
    return False


def kill_match(pid, start):
    if not alive_match(pid, start):
        return
    try:
        fd = os.pidfd_open(int(pid))
    except OSError:
        return
    try:
        if alive_match(pid, start):
            signal.pidfd_send_signal(fd, signal.SIGKILL)
    except OSError:
        pass
    finally:
        os.close(fd)


def load():
    try:
        payload = json.loads(open(watch_path, encoding="utf-8").read())
    except (OSError, json.JSONDecodeError, UnicodeError):
        return None
    if not isinstance(payload, dict):
        return None
    if not isinstance(payload.get("parent"), int) or not isinstance(payload.get("parent_start"), int):
        return None
    return payload


def row_known(row):
    return isinstance(row, dict) and isinstance(row.get("pid"), int) and isinstance(row.get("start"), int)


def verified_drains(paths):
    rows = list(paths or [])
    if not rows:
        return False
    for path in rows:
        try:
            payload = json.loads(open(path, encoding="utf-8").read())
        except (OSError, json.JSONDecodeError, UnicodeError):
            return False
        if not isinstance(payload, dict) or payload.get("namespace_empty") is not True:
            return False
    return True


def init_death_proven(inits):
    rows = list(inits or [])
    if not rows:
        return False
    for row in rows:
        if not row_known(row) or alive_match(row.get("pid"), row.get("start")):
            return False
    return True


def unknown_identity(hosts, inits):
    for row in list(hosts or []) + list(inits or []):
        if not row_known(row):
            return True
    return False


def track_rows(track):
    rows = []
    if not isinstance(track, dict):
        return rows
    host = track.get("host")
    if isinstance(host, dict):
        rows.append(host)
    inits = track.get("inits")
    if isinstance(inits, list):
        for row in inits:
            if isinstance(row, dict):
                rows.append(row)
    return rows


def bound_drain(path, invocation):
    if not isinstance(path, str) or not path or not isinstance(invocation, str) or not invocation:
        return False
    try:
        payload = json.loads(open(path, encoding="utf-8").read())
    except (OSError, json.JSONDecodeError, UnicodeError):
        return False
    if not isinstance(payload, dict) or payload.get("namespace_empty") is not True:
        return False
    return payload.get("invocation") == invocation


def track_proven(track):
    if not isinstance(track, dict):
        return False
    host = track.get("host")
    if host is not None and not row_known(host):
        return False
    inits = track.get("inits")
    if inits is None:
        inits = []
    if not isinstance(inits, list):
        return False
    for row in inits:
        if not row_known(row):
            return False
    drain = track.get("drain")
    if bound_drain(drain, track.get("invocation")):
        return True
    if track.get("pending") is not False:
        return False
    if not row_known(host):
        return False
    if not inits:
        return False
    for row in inits:
        if alive_match(row.get("pid"), row.get("start")):
            return False
    return True


def covered_tracks(tracks):
    if not isinstance(tracks, list) or not tracks:
        return False
    for track in tracks:
        if not track_proven(track):
            return False
    return True


while True:
    state = load()
    if state is None:
        time.sleep(0.02)
        continue
    parent = state.get("parent")
    parent_start = state.get("parent_start")
    parent_dead = start_of(parent) != parent_start
    raw_tracks = state.get("tracks")
    use_tracks = isinstance(raw_tracks, list) and len(raw_tracks) > 0
    malformed_tracks = "tracks" in state and raw_tracks not in (None, []) and not isinstance(raw_tracks, list)
    if use_tracks or malformed_tracks:
        rows = []
        if use_tracks:
            for track in raw_tracks:
                rows.extend(track_rows(track))
        proven = use_tracks and covered_tracks(raw_tracks)
    else:
        hosts = list(state.get("hosts") or [])
        inits = list(state.get("inits") or [])
        rows = hosts + inits
        drains = list(state.get("drains") or [])
        unknown = unknown_identity(hosts, inits)
        verified = verified_drains(drains)
        death = init_death_proven(inits)
        asked = bool(state.get("release"))
        unarmed = asked and not drains and not hosts and not inits and not unknown
        proven = (not unknown) and (verified or death or unarmed)
    if parent_dead:
        for row in rows:
            if row_known(row):
                kill_match(row.get("pid"), row.get("start"))
    survivors = False
    for row in rows:
        if row_known(row) and alive_match(row.get("pid"), row.get("start")):
            survivors = True
            break
    if (parent_dead or bool(state.get("release"))) and not survivors and proven:
        os.close(lock_fd)
        temporary = ack_path + ".tmp"
        with open(temporary, "w", encoding="utf-8") as handle:
            handle.write("released\\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, ack_path)
        os._exit(0)
    time.sleep(0.02)
"""


class CoordinatorStop(BaseException):
    def __init__(self, signum: int) -> None:
        self.signum = signum
        super().__init__(f"coordinator received signal {signum}")


def _on_coordinator_signal(signum: int, _frame) -> None:
    if _on_coordinator_signal.cleaning:
        return
    raise CoordinatorStop(signum)


_on_coordinator_signal.cleaning = False


def push_coordinator_signals():
    _on_coordinator_signal.cleaning = False
    saved = []
    for sig in _SIGNALS:
        saved.append((sig, signal.getsignal(sig)))
        signal.signal(sig, _on_coordinator_signal)
    return saved


def pop_coordinator_signals(saved) -> None:
    for sig, previous in saved:
        signal.signal(sig, previous)


def begin_cleanup() -> None:
    _on_coordinator_signal.cleaning = True


def parent_death_argv(parent_pid: int, command: list[str]) -> list[str]:
    return [os.fsdecode(os.fsencode(sys_executable())), str(launcher_path()), str(parent_pid), "--", *command]


def sys_executable() -> str:
    import sys

    return sys.executable


def launcher_path() -> Path:
    path = Path(os.environ.get("DECEBAL_DESKTOP_LAB_LAUNCHER", ""))
    if path.is_file():
        return path
    import tempfile

    fd, name = tempfile.mkstemp(prefix="desktop-lab-pdeath-", suffix=".py")
    os.write(fd, LAUNCHER_SOURCE.encode())
    os.close(fd)
    os.chmod(name, 0o700)
    os.environ["DECEBAL_DESKTOP_LAB_LAUNCHER"] = name
    return Path(name)


def lock_release_allowed(*, armed: bool, verified_drain: bool, survivors: bool) -> bool:
    """An error report is not containment. Hold the slot while any owned tree remains."""

    if survivors:
        return False
    if armed and not verified_drain:
        return False
    return True


class SlotGuard:
    """Holds a duplicate of the slot lock until the owned tree is verified gone."""

    def __init__(self) -> None:
        self.fd: int | None = None
        self.proc: subprocess.Popen | None = None
        self.watch: Path | None = None
        self.ack: Path | None = None
        self.pid: int | None = None
        self.starttime: int | None = None
        self.armed = False
        self.state = {
            "parent": os.getpid(),
            "parent_start": _start_of(os.getpid()),
            "release": False,
            "drains": [],
            "hosts": [],
            "inits": [],
            "tracks": [],
        }

    def start(self, lock_fd: int, directory: Path) -> None:
        import fcntl

        directory.mkdir(mode=0o700, parents=True, exist_ok=True)
        script = directory / "slot-guardian.py"
        script.write_text(GUARDIAN_SOURCE)
        script.chmod(0o700)
        self.watch = directory / "slot-guard.json"
        self.ack = directory / "slot-guard.ack"
        self.fd = lock_fd
        self.publish()
        flags = fcntl.fcntl(lock_fd, fcntl.F_GETFD)
        self.proc = subprocess.Popen(
            [sys_executable(), str(script), "--fd", str(lock_fd), "--watch", str(self.watch), "--ack", str(self.ack)],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
            close_fds=True,
            pass_fds=(lock_fd,),
            env={"PATH": "/usr/bin:/bin"},
        )
        fcntl.fcntl(lock_fd, fcntl.F_SETFD, flags | fcntl.FD_CLOEXEC)
        os.set_inheritable(lock_fd, False)
        self.pid = self.proc.pid
        self.starttime = _start_of(self.proc.pid)
        if self.proc.poll() is not None:
            raise OSError("slot guardian exited before the job started")

    def publish(self) -> None:
        if self.watch is None:
            return
        temporary = self.watch.with_suffix(".json.tmp")
        temporary.write_text(json.dumps(self.state))
        os.replace(temporary, self.watch)

    def note_host(self, pid: int, start: int | None, drain: str) -> None:
        self.armed = True
        _upsert(self.state["hosts"], pid, start)
        if drain not in self.state["drains"]:
            self.state["drains"].append(drain)
        self.publish()

    def note_inits(self, rows: list[dict]) -> None:
        for row in rows:
            _upsert(self.state["inits"], row.get("pid"), row.get("start"))
        self.publish()

    def reserve_track(self, track_id: str, drain: str, invocation: str) -> None:
        """Publish a pending track before spawn so a missing init fails closed."""

        self.armed = True
        tracks = self.state.setdefault("tracks", [])
        for row in tracks:
            if isinstance(row, dict) and row.get("id") == track_id:
                row["drain"] = drain
                row["invocation"] = invocation
                row["pending"] = True
                row["host"] = None
                row["inits"] = []
                self.publish()
                return
        tracks.append({
            "id": track_id,
            "drain": drain,
            "invocation": invocation,
            "pending": True,
            "host": None,
            "inits": [],
        })
        if drain not in self.state["drains"]:
            self.state["drains"].append(drain)
        self.publish()

    def note_track(
        self,
        track_id: str,
        pid: object,
        start: object,
        drain: str,
        inits: list[dict],
        invocation: str | None = None,
    ) -> None:
        """Record one track's host and init identities in a single publish."""

        self.armed = True
        rows = []
        for item in inits:
            if isinstance(item, dict):
                rows.append({"pid": item.get("pid"), "start": item.get("start")})
        host = {"pid": pid, "start": start} if isinstance(pid, int) else None
        complete = isinstance(pid, int) and isinstance(start, int) and _identities_complete(rows)
        tracks = self.state.setdefault("tracks", [])
        found = None
        for row in tracks:
            if isinstance(row, dict) and row.get("id") == track_id:
                found = row
                break
        if found is None:
            found = {
                "id": track_id,
                "drain": drain,
                "invocation": invocation,
                "pending": True,
                "host": None,
                "inits": [],
            }
            tracks.append(found)
        found["drain"] = drain
        if isinstance(invocation, str) and invocation:
            found["invocation"] = invocation
        found["host"] = host
        found["inits"] = rows
        found["pending"] = not complete
        _upsert(self.state["hosts"], pid, start)
        if drain not in self.state["drains"]:
            self.state["drains"].append(drain)
        for item in rows:
            _upsert(self.state["inits"], item.get("pid"), item.get("start"))
        self.publish()

    def finish(self, allowed: bool, timeout: float = 3.0) -> bool:
        if self.fd is None:
            return True
        if self.proc is None:
            if not allowed:
                return False
            os.close(self.fd)
            self.fd = None
            return True
        if not allowed:
            self.state["release"] = False
            self.publish()
            return False
        self.state["release"] = True
        self.publish()
        if not self._wait_ack(timeout):
            self.state["release"] = False
            self.publish()
            return False
        os.close(self.fd)
        self.fd = None
        return True

    def _wait_ack(self, timeout: float) -> bool:
        if self.ack is None or self.proc is None:
            return False
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.ack.is_file() and self.proc.poll() is not None:
                return True
            time.sleep(0.02)
        return False


def sync_guard(guard: SlotGuard | None, tracks: list[dict]) -> None:
    if guard is None:
        return
    for track in tracks:
        drain = track.get("drain_file")
        name = track.get("name")
        if not drain or not isinstance(name, str) or not name:
            continue
        rows = []
        starts = track.get("namespace_init_start") or {}
        for pid in track.get("namespace_init") or []:
            rows.append({"pid": pid, "start": starts.get(pid)})
        guard.note_track(
            name,
            track.get("pid"),
            track.get("leader_starttime"),
            str(drain),
            rows,
            track.get("invocation"),
        )


def tracks_have_survivors(tracks: list[dict]) -> bool:
    for track in tracks:
        if not track.get("drain_file"):
            continue
        process = track.get("process")
        if process is not None and process.poll() is None:
            return True
        starts = track.get("namespace_init_start") or {}
        for pid in track.get("namespace_init") or []:
            if _alive_match(pid, starts.get(pid)):
                return True
    return False


def _identities_complete(rows: list[dict]) -> bool:
    if not rows:
        return False
    for item in rows:
        if not isinstance(item.get("pid"), int) or not isinstance(item.get("start"), int):
            return False
    return True


def _upsert(rows: list[dict], pid: object, start: object) -> None:
    if not isinstance(pid, int):
        return
    for row in rows:
        if row.get("pid") == pid:
            row["start"] = start
            return
    rows.append({"pid": pid, "start": start})


def _start_of(pid: int) -> int | None:
    try:
        stat = Path(f"/proc/{pid}/stat").read_text()
        rest = stat[stat.rfind(")") + 2 :].split()
        return int(rest[19])
    except (OSError, IndexError, ValueError):
        return None


def _alive_match(pid: object, start: object) -> bool:
    if not isinstance(pid, int) or not isinstance(start, int):
        return False
    if _start_of(pid) != start:
        return False
    try:
        text = Path(f"/proc/{pid}/status").read_text()
    except OSError:
        return False
    for line in text.splitlines():
        if line.startswith("State:"):
            return line.split()[1] not in {"Z", "X"}
    return False
