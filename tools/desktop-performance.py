#!/usr/bin/env python3
"""Bounded read-only recorder for explicitly nominated Linux processes.

Samples utime/stime, RSS, thread count, file-descriptor count, and (when the
kernel permits) PSS. 100% CPU means one core over the measured monotonic
interval. The process total can exceed 100. Machine share is not reported.

The tool never signals, attaches to, or scans processes. It reads only the
allowlisted /proc files below and only for the PIDs named on the command line.
"""

from __future__ import annotations

import argparse
import errno
import json
import os
import select
import signal
import stat
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path

SCHEMA = "decebal.desktop-performance.v1"
TOOL_NAME = "tools/desktop-performance.py"

MAX_PIDS = 32
MAX_OBSERVATIONS = 600
MIN_DURATION_S = 1
MAX_DURATION_S = 60
MIN_PERIOD_MS = 100
MAX_PERIOD_MS = 2000
MAX_PID = 1 << 22

COMM_MAX = 64
STAT_MAX = 65536
SMAPS_MAX = 8192
BOOT_ID_MAX = 128
FD_CAP = 65536
COMPARE_MAX_BYTES = 8 * 1024 * 1024
UINT64_MAX = (1 << 64) - 1

# Relative names opened under a proc root. Nothing else is opened.
STAT_NAME = "stat"
COMM_NAME = "comm"
FD_NAME = "fd"
SMAPS_NAME = "smaps_rollup"
BOOT_ID_REL = "sys/kernel/random/boot_id"
READ_NAMES = frozenset({STAT_NAME, COMM_NAME, FD_NAME, SMAPS_NAME})

EXIT_OK = 0
EXIT_USAGE = 2
EXIT_OUTPUT = 3
EXIT_CANCELLED = 4
EXIT_INCOMPLETE = 5

SAMPLE_KEYS = (
    "index",
    "monotonic_ns",
    "unix_ns",
    "status",
    "detail",
    "state",
    "comm",
    "comm_status",
    "comm_truncated",
    "utime_ticks",
    "stime_ticks",
    "starttime_ticks",
    "rss_pages",
    "rss_bytes",
    "rss_bytes_status",
    "threads",
    "fd_count",
    "fd_status",
    "pss_bytes",
    "pss_status",
)

CPU_SEMANTICS = {
    "basis": "one_core",
    "one_hundred_percent": "one core busy for the measured monotonic elapsed time",
    "process_total": (
        "utime and stime are summed across the process, still in one-core units, "
        "so a process running on several cores can exceed 100"
    ),
    "machine_share": "not reported",
    "online_cpus_role": "informational_only_not_a_divisor",
}

NOT_A_BENCHMARK = (
    "Difference between two recordings from this tool when duration and period match. "
    "Not a desktop-environment benchmark and not a GNOME or KDE comparison."
)


class UsageError(Exception):
    """The request is outside the recorder's bounds."""


class OutputError(Exception):
    """An output or comparison path cannot be used safely."""


@dataclass
class Request:
    pids: list[int]
    duration_s: int
    period_ms: int
    output: Path
    force: bool
    compare_doc: dict | None


@dataclass(frozen=True)
class Units:
    clk_tck: int | None
    page_size: int | None


class CancelFlag:
    """Set from a signal handler. The handler only stores a reason string."""

    def __init__(self) -> None:
        self.reason: str | None = None
        self._prev_int = None
        self._prev_term = None

    def requested(self) -> bool:
        return self.reason is not None

    def install(self) -> None:
        self._prev_int = signal.signal(signal.SIGINT, self._on_signal)
        self._prev_term = signal.signal(signal.SIGTERM, self._on_signal)

    def restore(self) -> None:
        if self._prev_int is not None:
            signal.signal(signal.SIGINT, self._prev_int)
        if self._prev_term is not None:
            signal.signal(signal.SIGTERM, self._prev_term)

    def _on_signal(self, signum: int, _frame: object) -> None:
        if signum == signal.SIGINT:
            self.reason = "SIGINT"
        elif signum == signal.SIGTERM:
            self.reason = "SIGTERM"
        else:
            self.reason = f"signal-{signum}"


class SystemClock:
    def monotonic_ns(self) -> int:
        return time.monotonic_ns()

    def time_ns(self) -> int:
        return time.time_ns()

    def sleep_until(self, target_ns: int, cancel: CancelFlag) -> None:
        while not cancel.requested():
            now = time.monotonic_ns()
            if now >= target_ns:
                return
            time.sleep(min((target_ns - now) / 1_000_000_000, 0.05))


@dataclass
class Pin:
    pid: int
    pidfd: int | None
    pidfd_status: str
    starttime: int | None = None
    terminal: str | None = None
    identity_lost: bool = False
    boot_id: str | None = None


def observation_count(duration_s: int, period_ms: int) -> int:
    """Snapshots from t=0 through t=duration inclusive, step period."""
    return (duration_s * 1000) // period_ms + 1


def validate_bounds(pids: list[int], duration_s: int, period_ms: int) -> int:
    if not pids:
        raise UsageError("at least one --pid is required")
    if len(pids) > MAX_PIDS:
        raise UsageError(f"at most {MAX_PIDS} pids")
    if len(set(pids)) != len(pids):
        raise UsageError("duplicate pid")
    for pid in pids:
        if isinstance(pid, bool) or not isinstance(pid, int) or pid < 1 or pid > MAX_PID:
            raise UsageError(f"pid out of range: {pid}")
    if isinstance(duration_s, bool) or not isinstance(duration_s, int):
        raise UsageError("duration must be a whole number of seconds from 1 to 60")
    if duration_s < MIN_DURATION_S or duration_s > MAX_DURATION_S:
        raise UsageError("duration must be a whole number of seconds from 1 to 60")
    if isinstance(period_ms, bool) or not isinstance(period_ms, int):
        raise UsageError("period must be a whole number of milliseconds from 100 to 2000")
    if period_ms < MIN_PERIOD_MS or period_ms > MAX_PERIOD_MS:
        raise UsageError("period must be a whole number of milliseconds from 100 to 2000")
    count = observation_count(duration_s, period_ms)
    if count > MAX_OBSERVATIONS:
        raise UsageError(
            f"observation count would be {count}; the maximum is {MAX_OBSERVATIONS} "
            f"(a sample is one snapshot, and the count includes both endpoints)"
        )
    return count


def _u64(token: str) -> int:
    if not token.isdigit() or len(token) > 20:
        raise ValueError("fields")
    value = int(token)
    if value > UINT64_MAX:
        raise ValueError("fields")
    return value


def clean_comm(text: str) -> tuple[str | None, bool]:
    raw = text.split("\n", 1)[0]
    truncated = len(raw.encode("utf-8", errors="replace")) > COMM_MAX or len(raw) > COMM_MAX
    raw = raw[:COMM_MAX]
    cleaned = []
    for ch in raw:
        if ch.isprintable() and ch != "\x7f":
            cleaned.append(ch)
        else:
            cleaned.append("?")
            truncated = True
    comm = "".join(cleaned).strip()
    return (comm or None), truncated


class StatParseError(Exception):
    def __init__(self, detail: str) -> None:
        super().__init__(detail)
        self.detail = detail


@dataclass(frozen=True)
class Stat:
    pid: int
    comm: str | None
    comm_truncated: bool
    state: str
    utime: int
    stime: int
    threads: int
    starttime: int
    rss_pages: int


def parse_stat(text: str) -> Stat:
    """Parse one /proc/<pid>/stat line. The comm field may contain spaces and parentheses."""
    line = text.strip()
    if not line or len(line) > STAT_MAX:
        raise StatParseError("stat_shape")
    open_at = line.find("(")
    close_at = line.rfind(")")
    if open_at < 1 or close_at <= open_at:
        raise StatParseError("stat_shape")
    try:
        pid = _u64(line[:open_at].strip())
    except ValueError as exc:
        raise StatParseError("stat_fields") from exc
    if pid < 1 or pid > MAX_PID:
        raise StatParseError("stat_fields")
    comm, truncated = clean_comm(line[open_at + 1 : close_at])
    fields = line[close_at + 1 :].split()
    # state ppid pgrp session tty tpgid flags minflt cminflt majflt cmajflt
    # utime stime cutime cstime priority nice num_threads itrealvalue starttime vsize rss
    if len(fields) < 22:
        raise StatParseError("stat_fields")
    state = fields[0]
    if len(state) != 1 or not state.isalpha():
        raise StatParseError("stat_fields")
    try:
        utime = _u64(fields[11])
        stime = _u64(fields[12])
        threads = _u64(fields[17])
        starttime = _u64(fields[19])
        rss_pages = _u64(fields[21])
    except ValueError as exc:
        raise StatParseError("stat_fields") from exc
    return Stat(pid, comm, truncated, state, utime, stime, threads, starttime, rss_pages)


def parse_pss_bytes(text: str) -> int:
    """Return PSS in bytes from smaps_rollup. Raises ValueError with a short reason."""
    if len(text) > SMAPS_MAX:
        raise ValueError("too_large")
    for line in text.splitlines():
        if not line.startswith("Pss:"):
            continue
        parts = line.split()
        if len(parts) != 3 or parts[2] != "kB":
            raise ValueError("malformed")
        try:
            kilobytes = _u64(parts[1])
        except ValueError as exc:
            raise ValueError("malformed") from exc
        if kilobytes > UINT64_MAX // 1024:
            raise ValueError("malformed")
        return kilobytes * 1024
    raise ValueError("missing")


def cpu_percent(delta_ticks: int, clk_tck: int, elapsed_ns: int) -> float:
    """Percent of one core. Does not divide by the number of online CPUs."""
    elapsed_s = elapsed_ns / 1_000_000_000
    return (delta_ticks / clk_tck) / elapsed_s * 100.0


def _round(value: float) -> float:
    return round(value, 4)


def blank_sample(status: str, detail: str | None) -> dict:
    return {
        "index": None,
        "monotonic_ns": None,
        "unix_ns": None,
        "status": status,
        "detail": detail,
        "state": None,
        "comm": None,
        "comm_status": "not_read",
        "comm_truncated": False,
        "utime_ticks": None,
        "stime_ticks": None,
        "starttime_ticks": None,
        "rss_pages": None,
        "rss_bytes": None,
        "rss_bytes_status": "not_read",
        "threads": None,
        "fd_count": None,
        "fd_status": "not_read",
        "pss_bytes": None,
        "pss_status": "not_read",
    }


def _apply_rss(sample: dict, pages: int, units: Units) -> None:
    sample["rss_pages"] = pages
    if units.page_size is None or units.page_size <= 0:
        sample["rss_bytes"] = None
        sample["rss_bytes_status"] = "unsupported"
    else:
        sample["rss_bytes"] = pages * units.page_size
        sample["rss_bytes_status"] = "ok"


def classify_stat(pin: Pin, raw: dict, units: Units) -> dict:
    """Turn one stat read into a sample. Metrics stay null unless the pinned task is alive."""
    status = raw.get("read_status")
    if status == "missing":
        gone = pin.starttime is not None or pin.pidfd_status == "open" or raw.get("pidfd_exited")
        return blank_sample("exited" if gone else "missing", raw.get("detail") or "stat")
    if status == "denied":
        return blank_sample("denied", "stat")
    if status != "ok":
        kind = "malformed" if status == "malformed" else "unsupported"
        return blank_sample(kind, raw.get("detail") or status)

    if raw.get("pid_in_stat") != pin.pid:
        return blank_sample("malformed", "stat_pid_mismatch")

    state = raw.get("state")
    starttime = raw.get("starttime")
    if not isinstance(starttime, int) or not isinstance(state, str):
        return blank_sample("malformed", "stat_fields")

    if pin.starttime is None and raw.get("pidfd_exited"):
        if state == "Z":
            pin.starttime = starttime
            sample = blank_sample("zombie", "state")
            sample["state"] = "Z"
            return sample
        # The pidfd refers to a task that already exited. A live stat is another process.
        pin.identity_lost = True
        return blank_sample("reused", "pidfd")

    if pin.starttime is None:
        pin.starttime = starttime
    elif starttime != pin.starttime:
        pin.identity_lost = True
        return blank_sample("reused", "starttime")

    if state == "Z" or raw.get("pidfd_exited"):
        kind = "zombie" if state == "Z" else "exited"
        sample = blank_sample(kind, "state" if state == "Z" else "pidfd")
        sample["state"] = state if state == "Z" else None
        return sample

    sample = blank_sample("alive", None)
    sample["state"] = state
    sample["comm"] = raw.get("comm")
    sample["comm_status"] = raw.get("comm_status") or "fallback_stat"
    sample["comm_truncated"] = bool(raw.get("comm_truncated"))
    sample["utime_ticks"] = raw.get("utime")
    sample["stime_ticks"] = raw.get("stime")
    sample["starttime_ticks"] = starttime
    sample["threads"] = raw.get("threads")
    pages = raw.get("rss_pages")
    if isinstance(pages, int):
        _apply_rss(sample, pages, units)
    else:
        sample["rss_bytes_status"] = "malformed"
    return sample


def attach_extras(sample: dict, extras: dict) -> dict:
    if extras.get("comm_status") == "ok" and extras.get("comm"):
        sample["comm"] = extras["comm"]
        sample["comm_status"] = "ok"
        sample["comm_truncated"] = bool(extras.get("comm_truncated"))
    sample["fd_count"] = extras.get("fd_count")
    sample["fd_status"] = extras.get("fd_status") or "unsupported"
    sample["pss_bytes"] = extras.get("pss_bytes")
    sample["pss_status"] = extras.get("pss_status") or "unsupported"
    return sample


def observe_once(proc: object, pin: Pin, units: Units) -> dict:
    raw = proc.read_stat(pin)  # type: ignore[attr-defined]
    sample = classify_stat(pin, raw, units)
    if sample["status"] != "alive":
        return sample
    if pin.pidfd is not None and proc.pidfd_exited(pin.pidfd):  # type: ignore[attr-defined]
        sample = blank_sample("exited", "pidfd")
        return sample
    extras = proc.read_extras(pin.pid)  # type: ignore[attr-defined]
    confirmed = classify_stat(pin, proc.read_stat(pin), units)  # type: ignore[attr-defined]
    if confirmed["status"] != "alive":
        return confirmed
    return attach_extras(confirmed, extras)


def build_interval(previous: dict, current: dict, units: Units) -> dict:
    elapsed_ns = int(current["monotonic_ns"]) - int(previous["monotonic_ns"])
    interval = {
        "from_index": previous["index"],
        "to_index": current["index"],
        "elapsed_ns": elapsed_ns,
        "elapsed_seconds": round(elapsed_ns / 1_000_000_000, 6),
        "status": "not_alive",
        "from_status": previous["status"],
        "to_status": current["status"],
        "utime_ticks_delta": None,
        "stime_ticks_delta": None,
        "cpu_percent_one_core": None,
        "user_percent_one_core": None,
        "system_percent_one_core": None,
        "basis": "one_core",
    }
    if previous["status"] != "alive" or current["status"] != "alive":
        return interval
    if units.clk_tck is None or units.clk_tck <= 0:
        interval["status"] = "unsupported_clock"
        return interval
    if elapsed_ns < 1_000_000:
        interval["status"] = "elapsed_too_small"
        return interval
    utime_prev = previous["utime_ticks"]
    stime_prev = previous["stime_ticks"]
    utime_next = current["utime_ticks"]
    stime_next = current["stime_ticks"]
    if None in (utime_prev, stime_prev, utime_next, stime_next):
        interval["status"] = "malformed"
        return interval
    if utime_next < utime_prev or stime_next < stime_prev:
        # A backwards counter is not treated as unsigned wrap: a guessed modulus
        # would publish a fabricated rate.
        interval["status"] = "counter_regressed"
        return interval
    delta_user = utime_next - utime_prev
    delta_system = stime_next - stime_prev
    interval["status"] = "ok"
    interval["utime_ticks_delta"] = delta_user
    interval["stime_ticks_delta"] = delta_system
    interval["user_percent_one_core"] = _round(cpu_percent(delta_user, units.clk_tck, elapsed_ns))
    interval["system_percent_one_core"] = _round(cpu_percent(delta_system, units.clk_tck, elapsed_ns))
    interval["cpu_percent_one_core"] = _round(
        cpu_percent(delta_user + delta_system, units.clk_tck, elapsed_ns)
    )
    return interval


def _series(samples: list[dict], value_key: str, status_key: str) -> dict:
    usable = [sample for sample in samples if sample["status"] == "alive" and sample.get(status_key) == "ok"]
    alive = [sample for sample in samples if sample["status"] == "alive"]
    if not alive:
        return {"status": "no_measurement", "min": None, "max": None, "last": None}
    if not usable:
        statuses = sorted({sample.get(status_key) or "unavailable" for sample in alive})
        status = statuses[0] if len(statuses) == 1 else "unavailable"
        return {"status": status, "min": None, "max": None, "last": None, "sample_statuses": statuses}
    values = [sample[value_key] for sample in usable]
    body = {"status": "ok", "min": min(values), "max": max(values), "last": values[-1]}
    if len(usable) != len(alive):
        body["status"] = "partial"
        body["sample_statuses"] = sorted({sample.get(status_key) or "unavailable" for sample in alive})
    return body


def _cpu_summary(intervals: list[dict], units: Units) -> dict:
    ok = [item for item in intervals if item["status"] == "ok"]
    if not ok or units.clk_tck is None:
        return {
            "status": "unsupported_clock" if units.clk_tck is None else "no_measured_intervals",
            "intervals": 0,
            "cpu_percent_one_core": None,
            "cpu_percent_one_core_min": None,
            "cpu_percent_one_core_max": None,
            "cpu_percent_one_core_mean_unweighted": None,
            "basis": "one_core",
            "machine_share_percent": None,
            "machine_share_status": "not_reported",
        }
    user_ticks = sum(item["utime_ticks_delta"] for item in ok)
    system_ticks = sum(item["stime_ticks_delta"] for item in ok)
    elapsed_ns = sum(item["elapsed_ns"] for item in ok)
    percents = [item["cpu_percent_one_core"] for item in ok]
    return {
        "status": "ok",
        "intervals": len(ok),
        "cpu_percent_one_core": _round(cpu_percent(user_ticks + system_ticks, units.clk_tck, elapsed_ns)),
        "user_percent_one_core": _round(cpu_percent(user_ticks, units.clk_tck, elapsed_ns)),
        "system_percent_one_core": _round(cpu_percent(system_ticks, units.clk_tck, elapsed_ns)),
        "cpu_percent_one_core_min": min(percents),
        "cpu_percent_one_core_max": max(percents),
        "cpu_percent_one_core_mean_unweighted": _round(sum(percents) / len(percents)),
        "basis": "one_core",
        "machine_share_percent": None,
        "machine_share_status": "not_reported",
    }


def _outcome(samples: list[dict], intervals: list[dict], cancelled: bool) -> str:
    if any(item["status"] == "ok" for item in intervals):
        return "measured"
    seen = [sample["status"] for sample in samples]
    for name in ("reused", "denied", "malformed", "unsupported", "zombie", "exited", "missing"):
        if name in seen:
            return name
    if samples and all(sample["status"] == "alive" for sample in samples):
        return "no_interval"
    if cancelled:
        return "cancelled"
    return "unmeasured"


def _last_comm(samples: list[dict]) -> tuple[str | None, str, bool]:
    for sample in reversed(samples):
        if sample["status"] == "alive" and sample.get("comm"):
            return sample["comm"], sample["comm_status"], bool(sample["comm_truncated"])
    return None, "unavailable", False


def build_process(pin: Pin, samples: list[dict], units: Units, cancelled: bool) -> dict:
    intervals = [
        build_interval(samples[index], samples[index + 1], units) for index in range(len(samples) - 1)
    ]
    for sample in samples:
        if list(sample.keys()) != list(SAMPLE_KEYS):
            missing = [key for key in SAMPLE_KEYS if key not in sample]
            extra = [key for key in sample if key not in SAMPLE_KEYS]
            raise RuntimeError(f"sample keys drifted missing={missing} extra={extra}")
    comm, comm_status, truncated = _last_comm(samples)
    rss = _series(samples, "rss_bytes", "rss_bytes_status")
    thread_values = [
        sample["threads"]
        for sample in samples
        if sample["status"] == "alive" and isinstance(sample["threads"], int)
    ]
    if thread_values:
        threads = {"status": "ok", "min": min(thread_values), "max": max(thread_values), "last": thread_values[-1]}
    else:
        threads = {"status": "no_measurement", "min": None, "max": None, "last": None}
    return {
        "pid": pin.pid,
        "comm": comm,
        "comm_status": comm_status,
        "comm_truncated": truncated,
        "identity": {
            "boot_id": pin.boot_id,
            "starttime_ticks": pin.starttime,
            "pidfd_status": pin.pidfd_status,
        },
        "outcome": _outcome(samples, intervals, cancelled),
        "ended": any(sample["status"] in {"exited", "zombie"} for sample in samples),
        "identity_lost": pin.identity_lost or any(sample["status"] == "reused" for sample in samples),
        "samples": samples,
        "intervals": intervals,
        "summary": {
            "cpu": _cpu_summary(intervals, units),
            "rss_bytes": rss,
            "threads": threads,
            "fd_count": _series(samples, "fd_count", "fd_status"),
            "pss_bytes": _series(samples, "pss_bytes", "pss_status"),
        },
    }


def _stamp(sample: dict, index: int, monotonic_ns: int, unix_ns: int) -> dict:
    sample["index"] = index
    sample["monotonic_ns"] = monotonic_ns
    sample["unix_ns"] = unix_ns
    return sample


def record(
    request: Request,
    proc: object,
    clock: object,
    cancel: CancelFlag,
    units: Units,
    git: tuple[str | None, str],
    online_cpus: int | None,
) -> dict:
    planned = validate_bounds(request.pids, request.duration_s, request.period_ms)
    boot_id, boot_status = proc.boot()  # type: ignore[attr-defined]
    pins: list[Pin] = []
    warnings: list[str] = []
    if boot_status != "ok":
        warnings.append(f"boot_id_{boot_status}")
    if units.clk_tck is None:
        warnings.append("clk_tck_unsupported")
    if units.page_size is None:
        warnings.append("page_size_unsupported")
    try:
        for pid in request.pids:
            pidfd, pidfd_status = proc.open_pidfd(pid)  # type: ignore[attr-defined]
            pin = Pin(pid=pid, pidfd=pidfd, pidfd_status=pidfd_status, boot_id=boot_id if boot_status == "ok" else None)
            pins.append(pin)
            if pidfd_status not in {"open", "skipped"}:
                warnings.append(f"pid {pid} pidfd_{pidfd_status}")
        start_mono = clock.monotonic_ns()  # type: ignore[attr-defined]
        start_unix = clock.time_ns()  # type: ignore[attr-defined]
        period_ns = request.period_ms * 1_000_000
        taken: list[list[dict]] = [[] for _ in pins]
        for index in range(planned):
            if cancel.requested():
                break
            clock.sleep_until(start_mono + index * period_ns, cancel)  # type: ignore[attr-defined]
            if cancel.requested():
                break
            mono = clock.monotonic_ns()  # type: ignore[attr-defined]
            unix = clock.time_ns()  # type: ignore[attr-defined]
            for slot, pin in enumerate(pins):
                if pin.terminal is not None:
                    sample = blank_sample(pin.terminal, "carried")
                else:
                    sample = observe_once(proc, pin, units)
                    if sample["status"] != "alive":
                        pin.terminal = sample["status"]
                taken[slot].append(_stamp(sample, index, mono, unix))
        end_mono = clock.monotonic_ns()  # type: ignore[attr-defined]
        end_unix = clock.time_ns()  # type: ignore[attr-defined]
        observations = len(taken[0]) if taken else 0
        if cancel.requested():
            warnings.append(f"cancelled_{cancel.reason}")
        processes = [
            build_process(pin, samples, units, cancel.requested()) for pin, samples in zip(pins, taken)
        ]
        git_hash, git_status = git
        return {
            "schema": SCHEMA,
            "tool": TOOL_NAME,
            "cpu_semantics": CPU_SEMANTICS,
            "limits": {
                "max_pids": MAX_PIDS,
                "max_samples": MAX_OBSERVATIONS,
                "max_observations": MAX_OBSERVATIONS,
                "duration_s": {"min": MIN_DURATION_S, "max": MAX_DURATION_S},
                "period_ms": {"min": MIN_PERIOD_MS, "max": MAX_PERIOD_MS},
                "comm_max_bytes": COMM_MAX,
                "compare_max_bytes": COMPARE_MAX_BYTES,
            },
            "recording": {
                "git_hash": git_hash,
                "git_hash_status": git_status,
                "duration_requested_s": request.duration_s,
                "period_requested_ms": request.period_ms,
                "observations_planned": planned,
                "observations_taken": observations,
                "sample_count": observations,
                "clk_tck": units.clk_tck,
                "clk_tck_status": "ok" if units.clk_tck else "unsupported",
                "page_size": units.page_size,
                "page_size_status": "ok" if units.page_size else "unsupported",
                "boot_id": boot_id if boot_status == "ok" else None,
                "boot_id_status": boot_status,
                "started_unix_ns": start_unix,
                "ended_unix_ns": end_unix,
                "started_monotonic_ns": start_mono,
                "ended_monotonic_ns": end_mono,
                "cancelled": cancel.requested(),
                "cancel_reason": cancel.reason,
                "online_cpus": online_cpus,
                "online_cpus_role": "informational_only_not_a_divisor",
            },
            "processes": processes,
            "comparison": None,
            "warnings": warnings,
        }
    finally:
        for pin in pins:
            if pin.pidfd is not None:
                proc.close_pidfd(pin.pidfd)  # type: ignore[attr-defined]
                pin.pidfd = None


def _same_identity(current: dict, previous: dict) -> bool:
    left = current.get("identity") or {}
    right = previous.get("identity") or {}
    return bool(
        left.get("boot_id")
        and left.get("boot_id") == right.get("boot_id")
        and left.get("starttime_ticks") is not None
        and left.get("starttime_ticks") == right.get("starttime_ticks")
        and current.get("pid") == previous.get("pid")
    )


def _metric_pair(current: object, previous: object) -> dict:
    if isinstance(current, (int, float)) and not isinstance(current, bool) and isinstance(previous, (int, float)) and not isinstance(previous, bool):
        return {"current": current, "previous": previous, "delta": _round(float(current) - float(previous)), "status": "ok"}
    return {"current": current if isinstance(current, (int, float)) and not isinstance(current, bool) else None,
            "previous": previous if isinstance(previous, (int, float)) and not isinstance(previous, bool) else None,
            "delta": None,
            "status": "not_measured"}


def compare_recordings(current: dict, previous: dict) -> dict:
    reasons: list[str] = []
    if not isinstance(previous, dict) or previous.get("schema") != SCHEMA:
        reasons.append("schema")
    prev_rec = previous.get("recording") if isinstance(previous, dict) else None
    cur_rec = current.get("recording") or {}
    if not isinstance(prev_rec, dict):
        reasons.append("recording")
        prev_rec = {}
    if cur_rec.get("duration_requested_s") != prev_rec.get("duration_requested_s"):
        reasons.append("duration")
    if cur_rec.get("period_requested_ms") != prev_rec.get("period_requested_ms"):
        reasons.append("period")
    matched = not reasons
    pairs: list[dict] = []
    if matched:
        used: set[int] = set()
        previous_processes = previous.get("processes") if isinstance(previous.get("processes"), list) else []
        for proc in current.get("processes") or []:
            match = None
            how = None
            for index, other in enumerate(previous_processes):
                if index in used or not isinstance(other, dict):
                    continue
                if _same_identity(proc, other):
                    match = other
                    how = "boot_pid_starttime"
                    used.add(index)
                    break
            if match is None and proc.get("comm"):
                for index, other in enumerate(previous_processes):
                    if index in used or not isinstance(other, dict):
                        continue
                    if other.get("comm") and other.get("comm") == proc.get("comm"):
                        match = other
                        how = "comm_only"
                        used.add(index)
                        break
            if match is None:
                pairs.append({
                    "pid": proc.get("pid"),
                    "match": "unmatched",
                    "comm": proc.get("comm"),
                    "cpu_percent_one_core": _metric_pair(None, None),
                })
                continue
            current_cpu = ((proc.get("summary") or {}).get("cpu") or {}).get("cpu_percent_one_core")
            previous_cpu = ((match.get("summary") or {}).get("cpu") or {}).get("cpu_percent_one_core")
            current_rss = ((proc.get("summary") or {}).get("rss_bytes") or {}).get("last")
            previous_rss = ((match.get("summary") or {}).get("rss_bytes") or {}).get("last")
            pairs.append({
                "pid": proc.get("pid"),
                "previous_pid": match.get("pid"),
                "match": how,
                "comm": proc.get("comm"),
                "cpu_percent_one_core": _metric_pair(current_cpu, previous_cpu),
                "rss_bytes_last": _metric_pair(current_rss, previous_rss),
            })
    return {
        "desktop_environment_benchmark": False,
        "note": NOT_A_BENCHMARK,
        "matched_run": matched,
        "incomparable_reasons": reasons,
        "processes": pairs,
    }


def format_summary(doc: dict) -> str:
    recording = doc["recording"]
    lines = [
        (
            f"desktop-performance: {len(doc['processes'])} pid(s), "
            f"{recording['observations_taken']} observation(s), "
            f"duration {recording['duration_requested_s']}s, period {recording['period_requested_ms']} ms"
        ),
        "cpu: 100% = one core; process total can exceed 100; machine share is not reported",
    ]
    if recording.get("online_cpus"):
        lines.append(
            f"online_cpus: {recording['online_cpus']} (informational only, not a divisor)"
        )
    if recording.get("cancelled"):
        lines.append(f"cancelled: {recording.get('cancel_reason')}")
    for proc in doc["processes"]:
        cpu = proc["summary"]["cpu"]
        if cpu["status"] != "ok":
            cpu_text = cpu["status"]
        else:
            cpu_text = (
                f"{cpu['cpu_percent_one_core']}% of one core "
                f"(min {cpu['cpu_percent_one_core_min']}, max {cpu['cpu_percent_one_core_max']})"
            )
        pss = proc["summary"]["pss_bytes"]
        pss_text = pss["status"] if pss["status"] != "ok" else f"{pss['last']} bytes"
        comm = proc["comm"] or "-"
        lines.append(
            f"pid {proc['pid']} comm={comm} outcome={proc['outcome']} cpu={cpu_text} pss={pss_text}"
        )
    comparison = doc.get("comparison")
    if comparison:
        if comparison.get("matched_run"):
            lines.append("comparison: matched duration and period. Not a desktop-environment benchmark.")
            for pair in comparison.get("processes") or []:
                cpu = pair.get("cpu_percent_one_core") or {}
                lines.append(
                    f"compare pid {pair.get('pid')} match={pair.get('match')} "
                    f"cpu_one_core current={cpu.get('current')} previous={cpu.get('previous')} "
                    f"delta={cpu.get('delta')} status={cpu.get('status')}"
                )
        else:
            why = ", ".join(comparison.get("incomparable_reasons") or ["unspecified"])
            lines.append(
                f"comparison: incomparable ({why}). Not a desktop-environment benchmark."
            )
    return "\n".join(lines) + "\n"


def exit_code(doc: dict) -> int:
    if doc["recording"].get("cancelled"):
        return EXIT_CANCELLED
    if any(proc["outcome"] != "measured" for proc in doc["processes"]):
        return EXIT_INCOMPLETE
    return EXIT_OK


def dumps(doc: dict) -> bytes:
    return (json.dumps(doc, indent=2, ensure_ascii=True, allow_nan=False) + "\n").encode("ascii")


def _parent_dir(path: Path) -> Path:
    parent = path.parent
    if parent.as_posix() in ("", "."):
        parent = Path.cwd()
    try:
        st = os.stat(parent)
    except FileNotFoundError as exc:
        raise OutputError("output directory does not exist") from exc
    if not stat.S_ISDIR(st.st_mode):
        raise OutputError("output parent is not a directory")
    return parent


def _lstat_output(path: Path):
    try:
        return os.lstat(path)
    except FileNotFoundError:
        return None


def _unlink_quiet(path: Path) -> None:
    try:
        os.unlink(path)
    except FileNotFoundError:
        pass
    except OSError:
        pass


def describe_output(path: Path, force: bool) -> None:
    """Reject a symlink or non-regular final path before any process is sampled."""
    _parent_dir(path)
    st = _lstat_output(path)
    if st is None:
        return
    if stat.S_ISLNK(st.st_mode):
        raise OutputError("refusing to write through a symlink")
    if not stat.S_ISREG(st.st_mode):
        raise OutputError("refusing to write a non-regular output")
    if not force:
        raise OutputError("output exists; pass --force to replace a regular file")


def _write_temp(parent: Path, name: str, data: bytes) -> Path:
    token = os.urandom(8).hex()
    tmp = parent / f".{name}.{os.getpid()}.{token}.tmp"
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC
    fd = os.open(tmp, flags, 0o600)
    try:
        os.fchmod(fd, 0o600)
        view = memoryview(data)
        while view:
            written = os.write(fd, view)
            if written <= 0:
                raise OSError("short write")
            view = view[written:]
        os.fsync(fd)
    except Exception:
        os.close(fd)
        try:
            os.unlink(tmp)
        except FileNotFoundError:
            pass
        raise
    os.close(fd)
    return tmp


def _fsync_dir(parent: Path) -> None:
    fd = os.open(parent, os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def publish_bytes(path: Path, data: bytes, force: bool) -> None:
    """Publish a complete file. Exclusive create unless force replaces a regular file."""
    parent = _parent_dir(path)
    st = _lstat_output(path)
    if st is not None:
        if stat.S_ISLNK(st.st_mode):
            raise OutputError("refusing to write through a symlink")
        if not stat.S_ISREG(st.st_mode):
            raise OutputError("refusing to write a non-regular output")
        if not force:
            raise OutputError("output exists; pass --force to replace a regular file")
    tmp = _write_temp(parent, path.name, data)
    try:
        st_now = _lstat_output(path)
        if st_now is not None and (stat.S_ISLNK(st_now.st_mode) or not stat.S_ISREG(st_now.st_mode)):
            raise OutputError("refusing to replace a symlink or non-regular output")
        if st_now is not None and not force:
            raise OutputError("output exists; pass --force to replace a regular file")
        if not force and st_now is None:
            try:
                os.link(tmp, path)
            except FileExistsError as exc:
                raise OutputError("output exists; pass --force to replace a regular file") from exc
            _unlink_quiet(tmp)
        else:
            os.replace(tmp, path)
        _fsync_dir(parent)
    except Exception:
        _unlink_quiet(tmp)
        raise


def _open_compare(path: Path) -> dict:
    try:
        st = os.lstat(path)
    except FileNotFoundError as exc:
        raise OutputError("comparison file does not exist") from exc
    if stat.S_ISLNK(st.st_mode):
        raise OutputError("refusing to read a symlink comparison file")
    if not stat.S_ISREG(st.st_mode):
        raise OutputError("comparison file is not a regular file")
    if st.st_size > COMPARE_MAX_BYTES:
        raise OutputError(f"comparison file exceeds {COMPARE_MAX_BYTES} bytes")
    flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC
    try:
        fd = os.open(path, flags)
    except OSError as exc:
        if exc.errno == errno.ELOOP:
            raise OutputError("refusing to read a symlink comparison file") from exc
        raise OutputError("comparison file is not readable") from exc
    try:
        chunks = []
        remaining = COMPARE_MAX_BYTES + 1
        while remaining > 0:
            block = os.read(fd, min(1024 * 1024, remaining))
            if not block:
                break
            chunks.append(block)
            remaining -= len(block)
        blob = b"".join(chunks)
    finally:
        os.close(fd)
    if len(blob) > COMPARE_MAX_BYTES:
        raise OutputError(f"comparison file exceeds {COMPARE_MAX_BYTES} bytes")
    try:
        loaded = json.loads(blob.decode("utf-8"))
    except (UnicodeError, json.JSONDecodeError) as exc:
        raise OutputError("comparison file is not utf-8 JSON") from exc
    if not isinstance(loaded, dict):
        raise OutputError("comparison file is not a JSON object")
    return loaded


def system_units() -> Units:
    clk = os.sysconf("SC_CLK_TCK") if hasattr(os, "sysconf") else -1
    page = os.sysconf("SC_PAGE_SIZE") if hasattr(os, "sysconf") else -1
    try:
        clk_value = int(clk)
    except (TypeError, ValueError):
        clk_value = -1
    try:
        page_value = int(page)
    except (TypeError, ValueError):
        page_value = -1
    return Units(
        clk_tck=clk_value if clk_value > 0 else None,
        page_size=page_value if page_value > 0 else None,
    )


def git_hash(repo: Path) -> tuple[str | None, str]:
    try:
        completed = subprocess.run(
            ["git", "-C", str(repo), "rev-parse", "HEAD"],
            check=False,
            capture_output=True,
            text=True,
            timeout=2,
            shell=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None, "unavailable"
    if completed.returncode != 0:
        return None, "unavailable"
    line = completed.stdout.strip()
    if len(line) != 40 or any(ch not in "0123456789abcdef" for ch in line):
        return None, "unavailable"
    return line, "ok"


def _read_bounded(fd: int, limit: int) -> tuple[bytes | None, str]:
    chunks = []
    total = 0
    while total <= limit:
        block = os.read(fd, min(65536, limit - total + 1))
        if not block:
            break
        chunks.append(block)
        total += len(block)
        if total > limit:
            return None, "too_large"
    return b"".join(chunks), "ok"


class Procfs:
    """Read-only view of an allowlisted proc tree. pidfd is off unless explicitly enabled."""

    def __init__(self, root: Path, allow_pidfd: bool) -> None:
        self.root = Path(root)
        self.allow_pidfd = allow_pidfd

    def _path(self, rel: str) -> str:
        parts = rel.split("/")
        if not parts or any(part in ("", ".", "..") for part in parts):
            raise PermissionError(rel)
        if parts[-1] not in READ_NAMES and rel != BOOT_ID_REL:
            raise PermissionError(rel)
        root = os.path.abspath(self.root)
        path = os.path.abspath(os.path.join(root, rel))
        if os.path.commonpath([root, path]) != root:
            raise PermissionError(rel)
        return path

    def _open(self, rel: str, directory: bool = False) -> tuple[int | None, str]:
        flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC
        if directory:
            flags |= os.O_DIRECTORY
        try:
            return os.open(self._path(rel), flags), "ok"
        except FileNotFoundError:
            return None, "missing"
        except PermissionError:
            return None, "denied"
        except OSError as exc:
            if exc.errno in (errno.EACCES, errno.EPERM):
                return None, "denied"
            if exc.errno == errno.ELOOP:
                return None, "symlink"
            return None, "unsupported"

    def _read_bytes(self, rel: str, limit: int) -> tuple[bytes | None, str]:
        fd, status = self._open(rel)
        if fd is None:
            return None, status
        try:
            return _read_bounded(fd, limit)
        finally:
            os.close(fd)

    def boot(self) -> tuple[str | None, str]:
        data, status = self._read_bytes(BOOT_ID_REL, BOOT_ID_MAX)
        if status != "ok" or data is None:
            return None, status
        text = data.decode("ascii", errors="replace").strip()
        if not text or len(text) > 64 or any(ch not in "0123456789abcdef-" for ch in text.lower()):
            return None, "malformed"
        return text.lower(), "ok"

    def open_pidfd(self, pid: int) -> tuple[int | None, str]:
        if not self.allow_pidfd:
            return None, "skipped"
        if not hasattr(os, "pidfd_open"):
            return None, "unsupported"
        try:
            return os.pidfd_open(pid, 0), "open"
        except OSError as exc:
            if exc.errno == errno.ENOSYS:
                return None, "unsupported"
            if exc.errno in (errno.EPERM, errno.EACCES):
                return None, "denied"
            if exc.errno == errno.ESRCH:
                return None, "missing"
            return None, "unsupported"

    def close_pidfd(self, fd: int) -> None:
        os.close(fd)

    def pidfd_exited(self, fd: int) -> bool:
        readable, _, _ = select.select([fd], [], [], 0)
        return bool(readable)

    def read_stat(self, pin: Pin) -> dict:
        exited = bool(pin.pidfd is not None and self.pidfd_exited(pin.pidfd))
        data, status = self._read_bytes(f"{pin.pid}/{STAT_NAME}", STAT_MAX)
        raw = {
            "read_status": "ok",
            "detail": None,
            "pid_in_stat": None,
            "state": None,
            "comm": None,
            "comm_status": "unavailable",
            "comm_truncated": False,
            "utime": None,
            "stime": None,
            "starttime": None,
            "rss_pages": None,
            "threads": None,
            "pidfd_exited": exited,
        }
        if status == "too_large":
            raw["read_status"] = "malformed"
            raw["detail"] = "stat_too_large"
            return raw
        if status != "ok" or data is None:
            raw["read_status"] = "malformed" if status == "symlink" else status
            raw["detail"] = "stat_symlink" if status == "symlink" else "stat"
            if status == "symlink":
                raw["read_status"] = "unsupported"
            return raw
        try:
            parsed = parse_stat(data.decode("utf-8", errors="replace"))
        except StatParseError as exc:
            raw["read_status"] = "malformed"
            raw["detail"] = exc.detail
            return raw
        raw.update(
            {
                "pid_in_stat": parsed.pid,
                "state": parsed.state,
                "comm": parsed.comm,
                "comm_status": "fallback_stat",
                "comm_truncated": parsed.comm_truncated,
                "utime": parsed.utime,
                "stime": parsed.stime,
                "starttime": parsed.starttime,
                "rss_pages": parsed.rss_pages,
                "threads": parsed.threads,
            }
        )
        return raw

    def read_extras(self, pid: int) -> dict:
        comm, comm_status, truncated = self._read_comm(pid)
        fd_count, fd_status = self._count_fds(pid)
        pss_bytes, pss_status = self._read_pss(pid)
        return {
            "comm": comm,
            "comm_status": comm_status,
            "comm_truncated": truncated,
            "fd_count": fd_count,
            "fd_status": fd_status,
            "pss_bytes": pss_bytes,
            "pss_status": pss_status,
        }

    def _read_comm(self, pid: int) -> tuple[str | None, str, bool]:
        data, status = self._read_bytes(f"{pid}/{COMM_NAME}", COMM_MAX + 8)
        if status != "ok" or data is None:
            return None, status, False
        text = data.decode("utf-8", errors="replace")
        comm, truncated = clean_comm(text)
        if len(data) > COMM_MAX:
            truncated = True
        return comm, "ok", truncated

    def _count_fds(self, pid: int) -> tuple[int | None, str]:
        fd, status = self._open(f"{pid}/{FD_NAME}", directory=True)
        if fd is None:
            mapped = {"missing": "missing", "denied": "denied", "symlink": "symlink"}.get(status, "unsupported")
            return None, mapped
        try:
            # Names only. Do not readlink: descriptor targets can be paths or sockets.
            names = os.listdir(fd)
        finally:
            os.close(fd)
        if len(names) > FD_CAP:
            return None, "capped"
        return len(names), "ok"

    def _read_pss(self, pid: int) -> tuple[int | None, str]:
        data, status = self._read_bytes(f"{pid}/{SMAPS_NAME}", SMAPS_MAX)
        if status == "missing":
            return None, "unsupported"
        if status != "ok" or data is None:
            if status == "too_large":
                return None, "too_large"
            if status == "symlink":
                return None, "unsupported"
            return None, status if status in {"denied", "unsupported"} else "unsupported"
        try:
            return parse_pss_bytes(data.decode("utf-8", errors="replace")), "ok"
        except ValueError as exc:
            reason = str(exc)
            if reason == "missing":
                return None, "malformed"
            if reason == "too_large":
                return None, "too_large"
            return None, "malformed"


def record_live(request: Request, cancel: CancelFlag, clock: SystemClock) -> dict:
    if not sys.platform.startswith("linux"):
        raise UsageError("this recorder reads Linux /proc and is unsupported on this platform")
    proc = Procfs(Path("/proc"), allow_pidfd=True)
    repo = Path(__file__).resolve().parents[1]
    return record(
        request,
        proc,
        clock,
        cancel,
        system_units(),
        git_hash(repo),
        os.cpu_count(),
    )


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="desktop-performance.py",
        description=(
            "Record read-only /proc stats for explicitly nominated PIDs. "
            "CPU is a percent of one core (100 = one core). "
            "This is not a GNOME or KDE benchmark."
        ),
    )
    parser.add_argument("--pid", type=int, action="append", required=True, help="nominated PID; repeat up to 32")
    parser.add_argument("--duration", type=int, required=True, help="whole seconds, 1..60")
    parser.add_argument("--period-ms", type=int, required=True, dest="period_ms", help="sample period, 100..2000 ms")
    parser.add_argument("--output", type=Path, required=True, help="JSON path; created exclusively unless --force")
    parser.add_argument("--force", action="store_true", help="replace a regular output file")
    parser.add_argument("--compare", type=Path, default=None, help="optional earlier JSON from this tool")
    return parser


def parse_request(argv: list[str] | None) -> Request:
    parser = _build_parser()
    try:
        args = parser.parse_args(argv)
    except SystemExit as exc:
        code = exc.code
        if code not in (0, None):
            raise UsageError("invalid arguments") from exc
        raise
    pids = list(args.pid or [])
    validate_bounds(pids, args.duration, args.period_ms)
    output = Path(args.output)
    if args.compare is not None and Path(args.compare) == output:
        raise UsageError("comparison path and output path must differ")
    describe_output(output, args.force)
    compare_doc = _open_compare(Path(args.compare)) if args.compare is not None else None
    return Request(pids, args.duration, args.period_ms, output, bool(args.force), compare_doc)


def main(argv: list[str] | None = None) -> int:
    try:
        request = parse_request(argv)
    except UsageError as exc:
        print(f"desktop-performance: {exc}", file=sys.stderr)
        return EXIT_USAGE
    except OutputError as exc:
        print(f"desktop-performance: {exc}", file=sys.stderr)
        return EXIT_OUTPUT
    except SystemExit as exc:
        return int(exc.code or 0)
    cancel = CancelFlag()
    cancel.install()
    try:
        try:
            doc = record_live(request, cancel, SystemClock())
        except UsageError as exc:
            print(f"desktop-performance: {exc}", file=sys.stderr)
            return EXIT_USAGE
        except OutputError as exc:
            print(f"desktop-performance: {exc}", file=sys.stderr)
            return EXIT_OUTPUT
        if request.compare_doc is not None:
            doc["comparison"] = compare_recordings(doc, request.compare_doc)
            if not doc["comparison"]["matched_run"]:
                doc["warnings"].append("comparison_incomparable")
        try:
            publish_bytes(request.output, dumps(doc), request.force)
        except OutputError as exc:
            print(f"desktop-performance: {exc}", file=sys.stderr)
            return EXIT_OUTPUT
    finally:
        cancel.restore()
    sys.stdout.write(format_summary(doc))
    return exit_code(doc)


if __name__ == "__main__":
    sys.exit(main())
