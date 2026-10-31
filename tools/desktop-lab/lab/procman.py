"""Owned child processes, pidfd signalling, and /proc samples.

killpg is used only for the session this object created, and only while that
leader's starttime still matches. A recycled pid is not signaled. A pidfd
signal does not follow a recycled pid. Host marker scans are not containment.
"""

from __future__ import annotations

import os
from pathlib import Path
import select
import signal
import subprocess
import time

try:
    _CLK = os.sysconf("SC_CLK_TCK")
except (AttributeError, OSError, ValueError):
    _CLK = 100


class Owned:
    def __init__(self) -> None:
        self.tracks: list[dict] = []
        self.guard = None

    def spawn(
        self,
        argv: list[str],
        *,
        env: dict[str, str],
        name: str,
        cwd: Path | None = None,
        stdout=None,
        stderr=None,
        pass_fds: tuple[int, ...] = (),
        parent_death: bool = True,
    ) -> dict:
        launched = list(argv)
        if parent_death:
            from .lifetime import parent_death_argv

            launched = parent_death_argv(os.getpid(), argv)
        proc = subprocess.Popen(
            launched,
            env=env,
            cwd=str(cwd) if cwd else None,
            stdin=subprocess.DEVNULL,
            stdout=stdout if stdout is not None else subprocess.DEVNULL,
            stderr=stderr if stderr is not None else subprocess.DEVNULL,
            start_new_session=True,
            close_fds=True,
            pass_fds=pass_fds,
        )
        try:
            os.setpriority(os.PRIO_PROCESS, proc.pid, 10)
        except OSError:
            pass
        pidfd = None
        try:
            pidfd = os.pidfd_open(proc.pid)
        except OSError:
            pidfd = None
        try:
            pgid = os.getpgid(proc.pid)
        except OSError:
            pgid = proc.pid
        track = {
            "name": name,
            "argv": list(argv),
            "process": proc,
            "pid": proc.pid,
            "pgid": pgid,
            "leader_starttime": _proc_starttime(proc.pid),
            "pidfd": pidfd,
            "pidfd_used": pidfd is not None,
            "samples": [],
            "exit_code": None,
            "started": time.monotonic(),
        }
        self.tracks.append(track)
        return track

    def sample(self, track: dict) -> dict | None:
        row = read_sample(track["pid"])
        if row is not None and "error" not in row:
            previous = next((item for item in reversed(track["samples"]) if "ticks" in item), None)
            if previous and row["wall"] > previous["wall"]:
                ticks = row["ticks"] - previous["ticks"]
                wall = row["wall"] - previous["wall"]
                row["process_cpu_percent"] = (ticks / _CLK) / wall * 100
            else:
                row["process_cpu_percent"] = None
            track["samples"].append(row)
        return row

    def stop_one(self, track: dict, timeout: float = 2.0) -> None:
        self._signal_group(track, signal.SIGTERM)
        if not self._wait(track, timeout) and track["process"].poll() is None:
            self._signal_group(track, signal.SIGKILL)
            self._wait(track, 1)

    def stop_all(self, timeout: float = 2.0) -> None:
        for track in reversed(self.tracks):
            self._signal_group(track, signal.SIGTERM)
        deadline = time.monotonic() + timeout
        for track in reversed(self.tracks):
            remaining = max(0.0, deadline - time.monotonic())
            self._wait(track, remaining)
        for track in reversed(self.tracks):
            if track["process"].poll() is None or self._same_leader(track):
                self._signal_group(track, signal.SIGKILL)
                self._wait(track, 1)
        self.close()

    def close(self) -> None:
        for track in self.tracks:
            fd = track.get("pidfd")
            if fd is not None:
                os.close(fd)
                track["pidfd"] = None

    def _same_leader(self, track: dict) -> bool:
        expected = track.get("leader_starttime")
        if not isinstance(expected, int):
            return False
        return _proc_starttime(track["pid"]) == expected

    def _signal_group(self, track: dict, sig: int) -> None:
        pgid = track.get("pgid")
        # An exited leader can still own a session. Signal that recorded group
        # only while its starttime matches, so a reused pid is left alone.
        if pgid == track.get("pid") and isinstance(pgid, int) and pgid > 1 and self._same_leader(track):
            try:
                os.killpg(pgid, sig)
                return
            except ProcessLookupError:
                return
        if track["process"].poll() is not None:
            return
        self._signal_pid(track, sig)

    def _signal_pid(self, track: dict, sig: int) -> None:
        fd = track.get("pidfd")
        if fd is not None:
            try:
                signal.pidfd_send_signal(fd, sig)
                return
            except OSError:
                pass
        try:
            os.kill(track["process"].pid, sig)
        except ProcessLookupError:
            pass

    def _wait(self, track: dict, timeout: float) -> bool:
        proc = track["process"]
        if proc.poll() is not None:
            track["exit_code"] = proc.returncode
            return True
        fd = track.get("pidfd")
        if fd is not None and timeout > 0:
            select.select([fd], [], [], timeout)
            timeout = 0
        try:
            proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            return False
        track["exit_code"] = proc.returncode
        return True


def _proc_starttime(pid: int) -> int | None:
    try:
        stat = Path(f"/proc/{pid}/stat").read_text()
        rest = stat[stat.rfind(")") + 2 :].split()
        return int(rest[19])
    except (OSError, IndexError, ValueError):
        return None


def read_sample(pid: int) -> dict | None:
    status_path = Path(f"/proc/{pid}/status")
    stat_path = Path(f"/proc/{pid}/stat")
    try:
        status = status_path.read_text()
        stat = stat_path.read_text()
        fds = len(list(Path(f"/proc/{pid}/fd").iterdir()))
    except (FileNotFoundError, ProcessLookupError, PermissionError):
        return {"pid": pid, "error": "process is not readable"}
    fields = {}
    for line in status.splitlines():
        if ":" in line:
            key, value = line.split(":", 1)
            fields[key] = value.strip()
    try:
        rest = stat[stat.rfind(")") + 2 :].split()
        utime = int(rest[11])
        stime = int(rest[12])
        starttime = int(rest[19])
        rss = int(fields["VmRSS"].split()[0])
        threads = int(fields["Threads"])
    except (IndexError, KeyError, ValueError):
        return {"pid": pid, "error": "proc fields were incomplete"}
    return {
        "pid": pid,
        "rss_kib": rss,
        "threads": threads,
        "fds": fds,
        "ticks": utime + stime,
        "starttime": starttime,
        "wall": time.monotonic(),
    }


def public_process(track: dict) -> dict:
    samples = []
    for row in track["samples"]:
        samples.append(
            {
                "rss_kib": row["rss_kib"],
                "threads": row["threads"],
                "fds": row["fds"],
                "starttime": row["starttime"],
                "process_cpu_percent": row["process_cpu_percent"],
            }
        )
    rss = [row["rss_kib"] for row in samples]
    cpu = [row["process_cpu_percent"] for row in samples if row["process_cpu_percent"] is not None]
    public = {
        "name": track["name"],
        "argv": track["argv"],
        "pid": track["pid"],
        "pidfd": track["pidfd_used"],
        "exit_code": track["exit_code"],
        "sample_count": len(samples),
        "rss_kib_max": max(rss) if rss else None,
        "process_cpu_percent_max": max(cpu) if cpu else None,
        "samples": samples,
        "hardware_acceptance": False,
    }
    transport = track.get("display_transport")
    if isinstance(transport, dict):
        public["display_transport"] = transport
    return public


def become_subreaper() -> bool:
    """Adopt orphaned descendants of this job only. Callers clear it before returning."""

    return _prctl_subreaper(True)


def clear_subreaper() -> None:
    _prctl_subreaper(False)


def kill_marked(token: str) -> list[int]:
    """SIGKILL direct children whose environ has this job token.

    This is not containment. It misses nested, late, and cleared-marker
    processes, and it does not claim those are gone. The environ block is not
    stored. Processes without the exact marker stay untouched.
    """

    if not token or len(token) < 16 or not token.isalnum():
        return []
    marker = f"DECEBAL_DESKTOP_LAB_JOB={token}".encode()
    me = os.getpid()
    signaled: list[int] = []
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        pid = int(entry.name)
        if pid <= 1 or pid == me or _ppid(pid) != me:
            continue
        if not _environ_has(pid, marker):
            continue
        fd = None
        try:
            fd = os.pidfd_open(pid)
            signal.pidfd_send_signal(fd, signal.SIGKILL)
            signaled.append(pid)
            _reap(pid, fd)
        except OSError:
            continue
        finally:
            if fd is not None:
                os.close(fd)
    return signaled


def _reap(pid: int, pidfd: int) -> None:
    try:
        os.waitid(os.P_PIDFD, pidfd, os.WEXITED)
        return
    except (AttributeError, ChildProcessError, OSError):
        pass
    try:
        os.waitpid(pid, os.WNOHANG)
    except (ChildProcessError, OSError):
        pass


def _prctl_subreaper(enabled: bool) -> bool:
    import ctypes

    libc = ctypes.CDLL(None, use_errno=True)
    libc.prctl.argtypes = [ctypes.c_int, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_ulong]
    libc.prctl.restype = ctypes.c_int
    return libc.prctl(36, 1 if enabled else 0, 0, 0, 0) == 0


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


def _environ_has(pid: int, marker: bytes) -> bool:
    try:
        data = Path(f"/proc/{pid}/environ").read_bytes()
    except OSError:
        return False
    matched = False
    start = 0
    while start <= len(data):
        end = data.find(b"\0", start)
        if end < 0:
            end = len(data)
        if data[start:end] == marker:
            matched = True
            break
        if end >= len(data):
            break
        start = end + 1
    del data
    return matched
