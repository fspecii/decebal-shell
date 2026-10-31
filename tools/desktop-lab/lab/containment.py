"""Per-job pid namespace. A host /proc marker scan is not containment.

`unshare --pid` without a user namespace is EPERM for this user. The probe
tries that form, then a user namespace that maps the current user to itself.
The working prefix is what the report records. A transient systemd scope is
not used: stopping one left a double-fork daemon alive. If no probe works,
the job does not start.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
import time

from .policy import PolicyError

_CACHE: dict | None = None
_GRACE = "0.35"

_SUPERVISOR = """\
import json, os, signal, sys, time

args = sys.argv[1:]
if (
    len(args) < 8
    or args[0] != "--drain"
    or args[2] != "--grace"
    or args[4] != "--invocation"
    or args[6] != "--"
):
    raise SystemExit(2)
drain_path = args[1]
grace = float(args[3])
invocation = args[5]
cmd = args[7:]
if not cmd or not invocation or "/" in invocation or invocation in {".", ".."}:
    raise SystemExit(2)
stopping = False


def on_term(signum, frame):
    global stopping
    stopping = True


signal.signal(signal.SIGTERM, on_term)
signal.signal(signal.SIGINT, on_term)
signal.signal(signal.SIGHUP, on_term)


def others():
    me = os.getpid()
    found = []
    for name in os.listdir("/proc"):
        if not name.isdigit():
            continue
        pid = int(name)
        if pid > 1 and pid != me:
            found.append(pid)
    return found


def signal_others(sig):
    for pid in others():
        try:
            os.kill(pid, sig)
        except OSError:
            pass


def reap():
    while True:
        try:
            wpid, _status = os.waitpid(-1, os.WNOHANG)
        except ChildProcessError:
            return
        if wpid == 0:
            return


pid = os.fork()
if pid == 0:
    try:
        os.setsid()
        os.execvpe(cmd[0], cmd, os.environ)
    except OSError:
        os._exit(127)
status = None
while not stopping:
    try:
        wpid, code = os.waitpid(-1, os.WNOHANG)
    except ChildProcessError:
        break
    if wpid == 0:
        time.sleep(0.02)
        continue
    if wpid == pid:
        status = code
        break
signal_others(signal.SIGTERM)
deadline = time.monotonic() + grace
while time.monotonic() < deadline:
    reap()
    if not others():
        break
    time.sleep(0.02)
signal_others(signal.SIGKILL)
reap()
time.sleep(0.05)
reap()
left = others()
payload = {"namespace_empty": left == [], "left": left, "invocation": invocation}
temporary = drain_path + ".tmp"
with open(temporary, "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
    handle.write("\\n")
    handle.flush()
    os.fsync(handle.fileno())
os.replace(temporary, drain_path)
if status is None:
    raise SystemExit(128 + 15)
if os.WIFEXITED(status):
    raise SystemExit(os.WEXITSTATUS(status))
if os.WIFSIGNALED(status):
    raise SystemExit(128 + os.WTERMSIG(status))
raise SystemExit(1)
"""


def probe_containment() -> dict:
    """Return the argv prefix that actually exits 0. Cache only a success."""

    global _CACHE
    if _CACHE is not None:
        return {
            "kind": _CACHE["kind"],
            "argv_prefix": list(_CACHE["argv_prefix"]),
            "kill_child": True,
            "claims_marker_scan_is_sufficient": False,
        }
    binary = _unshare_binary()
    candidates = [
        [binary, "--pid", "--fork", "--mount-proc", "--kill-child", "--forward-signals"],
        [binary, "--user", "--map-current-user", "--pid", "--fork", "--mount-proc", "--kill-child", "--forward-signals"],
        [binary, "--user", "--map-root-user", "--pid", "--fork", "--mount-proc", "--kill-child", "--forward-signals"],
    ]
    errors: list[str] = []
    for prefix in candidates:
        ok, detail = _probe(prefix)
        if ok:
            _CACHE = {"kind": "pid-namespace", "argv_prefix": prefix}
            return {
                "kind": "pid-namespace",
                "argv_prefix": list(prefix),
                "kill_child": True,
                "claims_marker_scan_is_sufficient": False,
            }
        errors.append(detail)
    joined = "; ".join(item for item in errors if item)
    raise PolicyError(
        "containment unsupported: pid namespace probe failed "
        f"({joined}). a transient user scope does not contain a double-fork and is not used"
    )


def write_supervisor(path: Path) -> None:
    path.write_text(_SUPERVISOR)
    path.chmod(0o700)


def contained_argv(user_argv: list[str], *, drain_file: Path, supervisor: Path, invocation: str) -> list[str]:
    info = probe_containment()
    return [
        *info["argv_prefix"],
        "--",
        sys.executable,
        str(supervisor),
        "--drain",
        str(drain_file),
        "--grace",
        _GRACE,
        "--invocation",
        invocation,
        "--",
        *user_argv,
    ]


def note_namespace_init(track: dict) -> None:
    """Record host pids of the unshare child while that parent is still alive."""

    if track.get("namespace_init"):
        return
    pid = track.get("pid")
    if not isinstance(pid, int) or pid <= 1:
        return
    path = Path(f"/proc/{pid}/task/{pid}/children")
    try:
        text = path.read_text().split()
    except OSError:
        return
    found = [int(item) for item in text if item.isdigit()]
    if found:
        track["namespace_init"] = found
        track["namespace_init_start"] = {pid: _proc_starttime(pid) for pid in found}


def verify_drain(
    drain_file: Path,
    host_inits: list[int] | None,
    starts: dict | None = None,
    invocation: str | None = None,
) -> bool:
    """True when this invocation's supervisor saw an empty namespace, or its inits are gone.

    This reads /proc. It does not signal. A reused pid does not count as the
    original init. namespace_empty true counts only when the file's invocation
    matches this run. A missing file is accepted only when every recorded init
    pid is already dead and has no children.
    """

    flagged = _drain_flag(drain_file)
    if flagged is False:
        return False
    bound = _drain_bound(drain_file, invocation)
    inits = list(host_inits or [])
    known = starts or {}
    deadline = time.monotonic() + 0.5
    while True:
        if not _survivors(inits, known) and (bound or (flagged is None and bool(inits))):
            return True
        if time.monotonic() >= deadline:
            return False
        time.sleep(0.02)


def _unshare_binary() -> str:
    for candidate in ("/usr/bin/unshare", "/bin/unshare"):
        if os.access(candidate, os.X_OK):
            return candidate
    raise PolicyError("containment unsupported: unshare is missing")


def _probe(prefix: list[str]) -> tuple[bool, str]:
    try:
        result = subprocess.run(
            [*prefix, "--", "/bin/true"],
            check=False,
            timeout=5,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        return False, str(exc)
    if result.returncode == 0:
        return True, ""
    lines = (result.stderr or "").strip().splitlines()
    detail = lines[-1] if lines else f"exit {result.returncode}"
    return False, detail


def _drain_bound(path: Path, invocation: str | None) -> bool:
    if not isinstance(invocation, str) or not invocation:
        return False
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError, UnicodeError):
        return False
    if not isinstance(payload, dict) or payload.get("namespace_empty") is not True:
        return False
    return payload.get("invocation") == invocation


def _drain_flag(path: Path) -> bool | None:
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError, UnicodeError):
        return None
    if not isinstance(payload, dict) or "namespace_empty" not in payload:
        return None
    return payload.get("namespace_empty") is True


def _survivors(host_inits: list[int], starts: dict) -> bool:
    for pid in host_inits:
        current = _proc_starttime(pid)
        expected = starts.get(pid)
        if current is None:
            continue
        if isinstance(expected, int) and current != expected:
            continue
        if _pid_alive(pid) or _host_children(pid):
            return True
    return False


def _proc_starttime(pid: int) -> int | None:
    try:
        stat = Path(f"/proc/{pid}/stat").read_text()
        rest = stat[stat.rfind(")") + 2 :].split()
        return int(rest[19])
    except (OSError, IndexError, ValueError):
        return None


def _pid_alive(pid: int) -> bool:
    try:
        text = Path(f"/proc/{pid}/status").read_text()
    except OSError:
        return False
    for line in text.splitlines():
        if line.startswith("State:"):
            return line.split()[1] not in {"Z", "X"}
    return False


def _host_children(parent: int) -> list[int]:
    found = []
    proc = Path("/proc")
    try:
        names = list(proc.iterdir())
    except OSError:
        return found
    for entry in names:
        if not entry.name.isdigit():
            continue
        pid = int(entry.name)
        if _ppid(pid) == parent:
            found.append(pid)
    return found


def _ppid(pid: int) -> int | None:
    try:
        text = Path(f"/proc/{pid}/status").read_text()
    except OSError:
        return None
    for line in text.splitlines():
        if line.startswith("PPid:"):
            parts = line.split()
            if len(parts) == 2 and parts[1].isdigit():
                return int(parts[1])
    return None
