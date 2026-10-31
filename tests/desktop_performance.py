#!/usr/bin/env python3
"""Tests for tools/desktop-performance.py.

Fixture tests build a private proc tree and do not read live processes.
The owned-helper tests at the bottom start processes themselves, record only
those PIDs, and reap them on failure, SIGTERM, and timeout.
"""

from __future__ import annotations

import ast
import errno
import importlib.util
import io
import json
import os
import signal
import stat
import subprocess
import sys
import tempfile
import time
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TOOL_PATH = ROOT / "tools" / "desktop-performance.py"

SENTINEL_CMDLINE = "SENTINEL_CMDLINE_SHOULD_NOT_APPEAR"
SENTINEL_ENV = "SENTINEL_ENV_SHOULD_NOT_APPEAR"
SENTINEL_OTHER = "SENTINEL_OTHER_PID"
SENTINEL_FD = "SENTINEL_FD_TARGET"
SENTINEL_REUSED = "SENTINEL_REUSED_COMM"

HELPER_CAP_S = 5
IDLE_CODE = "import time\ntime.sleep(5)\n"
BUSY_CODE = (
    f"sentinel = {SENTINEL_CMDLINE!r}\n"
    "import time\n"
    "end = time.monotonic() + 5\n"
    "x = 0\n"
    "while time.monotonic() < end:\n"
    "    x = (x * 1664525 + 1013904223) & 0x7fffffff\n"
)


def load_tool():
    spec = importlib.util.spec_from_file_location("desktop_performance_tool", TOOL_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {TOOL_PATH}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


tool = load_tool()


def stat_line(pid, comm, state, utime, stime, start, threads=1, rss=10, ppid=1):
    return (
        f"{pid} ({comm}) {state} {ppid} {pid} {pid} 0 -1 0 0 0 0 0 "
        f"{utime} {stime} 0 0 20 0 {threads} 0 {start} 0 {rss}\n"
    )


RUST_STAT = (
    "1234 (fire fox (x)) S 1 1234 1234 0 -1 4194560 100 0 0 0 250 50 0 0 20 0 42 0 "
    "98765 1000000 2500 18446744073709551615 0 0 0 0 0 0 0 0 0 0 0 0 17 3 0 0 0 0 0"
)


class ManualClock:
    def __init__(self):
        self.mono = 0
        self.wall = 1_700_000_000_000_000_000

    def monotonic_ns(self):
        return self.mono

    def time_ns(self):
        return self.wall

    def sleep_until(self, target_ns, cancel):
        if cancel.requested():
            return
        if target_ns > self.mono:
            self.wall += target_ns - self.mono
            self.mono = target_ns


class StepClock(ManualClock):
    def __init__(self, steps):
        super().__init__()
        self.steps = list(steps)
        self.n = 0

    def sleep_until(self, target_ns, cancel):
        super().sleep_until(target_ns, cancel)
        if cancel.requested():
            return
        if self.n < len(self.steps):
            self.steps[self.n]()
        self.n += 1


class QueueProc:
    """Returns scripted stat reads. Two reads are consumed for each alive sample."""

    def __init__(self, stats, extras=None, boot=("boot-1", "ok"), pidfd_status="skipped"):
        self.stats = {pid: list(rows) for pid, rows in stats.items()}
        self.extras = dict(extras or {})
        self.boot_value = boot
        self.pidfd_status = pidfd_status
        self.stat_reads = []
        self.extra_reads = []
        self.opened = []
        self.closed = []

    def boot(self):
        return self.boot_value

    def open_pidfd(self, pid):
        self.opened.append(pid)
        return None, self.pidfd_status

    def close_pidfd(self, fd):
        self.closed.append(fd)

    def pidfd_exited(self, fd):
        return False

    def read_stat(self, pin):
        self.stat_reads.append(pin.pid)
        queue = self.stats[pin.pid]
        if not queue:
            return {"read_status": "missing", "detail": "stat", "pidfd_exited": False}
        return queue.pop(0)

    def read_extras(self, pid):
        self.extra_reads.append(pid)
        return self.extras.get(pid, {
            "comm": "app",
            "comm_status": "ok",
            "comm_truncated": False,
            "fd_count": 3,
            "fd_status": "ok",
            "pss_bytes": 4096,
            "pss_status": "ok",
        })


def raw_alive(pid, start, utime, stime=0, rss=10, threads=1, state="S", comm="app"):
    return {
        "read_status": "ok",
        "detail": None,
        "pid_in_stat": pid,
        "state": state,
        "comm": comm,
        "comm_status": "fallback_stat",
        "comm_truncated": False,
        "utime": utime,
        "stime": stime,
        "starttime": start,
        "rss_pages": rss,
        "threads": threads,
        "pidfd_exited": False,
    }


def pair(row):
    return [json.loads(json.dumps(row)), json.loads(json.dumps(row))]


class TempCase(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmp.name)
        self._live = tool.record_live

    def tearDown(self):
        tool.record_live = self._live
        self._tmp.cleanup()

    def block_live(self):
        def banned(*_args, **_kwargs):
            raise AssertionError("record_live was called")
        tool.record_live = banned

    def run_main(self, argv):
        stdout = io.StringIO()
        stderr = io.StringIO()
        with redirect_stdout(stdout), redirect_stderr(stderr):
            code = tool.main(argv)
        return code, stdout.getvalue(), stderr.getvalue()


class ParseTests(TempCase):
    def test_parses_the_activity_monitor_stat_fixture(self):
        parsed = tool.parse_stat(RUST_STAT)
        self.assertEqual(parsed.pid, 1234)
        self.assertEqual(parsed.comm, "fire fox (x)")
        self.assertEqual(parsed.state, "S")
        self.assertEqual((parsed.utime, parsed.stime, parsed.threads), (250, 50, 42))
        self.assertEqual((parsed.starttime, parsed.rss_pages), (98765, 2500))

    def test_truncated_and_non_numeric_stats_are_malformed(self):
        for text in (
            "garbage",
            "1 (no-close S 1",
            "1 (ok) S 1",
            "x (a) S 1 1 1 0 -1 0 0 0 0 0 1 1 0 0 20 0 1 0 5 0 0",
            stat_line(1, "ok", "S", "+10", 0, 5),
            stat_line(1, "ok", "S", -1, 0, 5),
            stat_line(1, "ok", "S", "1" * 21, 0, 5),
            stat_line(1, "ok", "ZZ", 1, 0, 5),
        ):
            with self.subTest(text=text[:40]):
                with self.assertRaises(tool.StatParseError):
                    tool.parse_stat(text)

    def test_missing_rss_is_not_zero(self):
        short = stat_line(4, "ok", "S", 1, 1, 9, rss=12).rsplit(" ", 1)[0]
        with self.assertRaises(tool.StatParseError) as caught:
            tool.parse_stat(short)
        self.assertEqual(caught.exception.detail, "stat_fields")

    def test_uint64_bounds(self):
        biggest = stat_line(4, "ok", "S", 2**64 - 1, 0, 9)
        self.assertEqual(tool.parse_stat(biggest).utime, 2**64 - 1)
        with self.assertRaises(tool.StatParseError):
            tool.parse_stat(stat_line(4, "ok", "S", 2**64, 0, 9))

    def test_long_comm_is_bounded_and_fields_still_parse(self):
        comm = "n" * 80
        parsed = tool.parse_stat(stat_line(8, comm, "R", 3, 4, 5, threads=2, rss=6))
        self.assertEqual(parsed.comm, "n" * tool.COMM_MAX)
        self.assertTrue(parsed.comm_truncated)
        self.assertEqual((parsed.utime, parsed.stime, parsed.rss_pages), (3, 4, 6))

    def test_pss_ignores_pss_dirty_and_keeps_a_real_zero(self):
        text = "Rss: 10 kB\nPss_Dirty: 99 kB\nPss: 0 kB\n"
        self.assertEqual(tool.parse_pss_bytes(text), 0)
        self.assertEqual(tool.parse_pss_bytes("Pss: 4 kB\n"), 4096)
        with self.assertRaises(ValueError):
            tool.parse_pss_bytes("Rss: 4 kB\n")
        with self.assertRaises(ValueError):
            tool.parse_pss_bytes("Pss: 4 MB\n")
        with self.assertRaises(ValueError):
            tool.parse_pss_bytes("Pss: -1 kB\n")


class RateTests(TempCase):
    def test_one_core_percent_uses_actual_elapsed(self):
        units = tool.Units(100, 4096)
        previous = tool.blank_sample("alive", None)
        current = tool.blank_sample("alive", None)
        previous.update({
            "index": 0, "monotonic_ns": 0, "unix_ns": 1,
            "utime_ticks": 0, "stime_ticks": 0, "threads": 1,
        })
        current.update({
            "index": 1, "monotonic_ns": 500_000_000, "unix_ns": 2,
            "utime_ticks": 50, "stime_ticks": 25, "threads": 1,
        })
        interval = tool.build_interval(previous, current, units)
        self.assertEqual(interval["status"], "ok")
        self.assertEqual(interval["cpu_percent_one_core"], 150.0)
        self.assertEqual(interval["user_percent_one_core"], 100.0)
        self.assertEqual(interval["system_percent_one_core"], 50.0)
        self.assertEqual(interval["basis"], "one_core")

    def test_multicore_total_is_not_divided_by_online_cpus(self):
        units = tool.Units(100, 4096)
        previous = tool.blank_sample("alive", None)
        current = tool.blank_sample("alive", None)
        previous.update({"index": 0, "monotonic_ns": 0, "unix_ns": 1, "utime_ticks": 0, "stime_ticks": 0})
        current.update({
            "index": 1, "monotonic_ns": 1_000_000_000, "unix_ns": 2,
            "utime_ticks": 200, "stime_ticks": 50,
        })
        interval = tool.build_interval(previous, current, units)
        self.assertEqual(interval["cpu_percent_one_core"], 250.0)
        self.assertIsNone(tool._cpu_summary([interval], units)["machine_share_percent"])
        summary = tool.format_summary({
            "recording": {
                "observations_taken": 2,
                "duration_requested_s": 1,
                "period_requested_ms": 1000,
                "cancelled": False,
                "online_cpus": 24,
            },
            "processes": [{
                "pid": 1,
                "comm": "busy",
                "outcome": "measured",
                "summary": {
                    "cpu": tool._cpu_summary([interval], units),
                    "pss_bytes": {"status": "denied", "last": None},
                },
            }],
            "comparison": None,
        })
        self.assertIn("250.0% of one core", summary)
        self.assertIn("not a divisor", summary)
        self.assertNotIn("10.4167", summary)
        self.assertIn("pss=denied", summary)
        self.assertNotIn("pss=0", summary)

    def test_counter_regression_is_not_reported_as_a_wrap_rate(self):
        units = tool.Units(100, 4096)
        previous = tool.blank_sample("alive", None)
        current = tool.blank_sample("alive", None)
        previous.update({
            "index": 0, "monotonic_ns": 0, "unix_ns": 1,
            "utime_ticks": 2**64 - 10, "stime_ticks": 0,
        })
        current.update({
            "index": 1, "monotonic_ns": 1_000_000_000, "unix_ns": 2,
            "utime_ticks": 5, "stime_ticks": 0,
        })
        interval = tool.build_interval(previous, current, units)
        self.assertEqual(interval["status"], "counter_regressed")
        self.assertIsNone(interval["cpu_percent_one_core"])
        self.assertIsNone(interval["utime_ticks_delta"])

    def test_tiny_elapsed_and_missing_clock_are_not_zero(self):
        previous = tool.blank_sample("alive", None)
        current = tool.blank_sample("alive", None)
        previous.update({"index": 0, "monotonic_ns": 5, "unix_ns": 1, "utime_ticks": 0, "stime_ticks": 0})
        current.update({"index": 1, "monotonic_ns": 10, "unix_ns": 2, "utime_ticks": 5, "stime_ticks": 0})
        tiny = tool.build_interval(previous, current, tool.Units(100, 4096))
        self.assertEqual(tiny["status"], "elapsed_too_small")
        self.assertIsNone(tiny["cpu_percent_one_core"])
        missing = tool.build_interval(previous, current, tool.Units(None, 4096))
        self.assertEqual(missing["status"], "unsupported_clock")
        self.assertIsNone(missing["cpu_percent_one_core"])

    def test_overall_rate_is_time_weighted(self):
        units = tool.Units(100, 4096)
        samples = []
        points = [(0, 0), (500_000_000, 100), (2_000_000_000, 100)]
        for index, (mono, utime) in enumerate(points):
            sample = tool.blank_sample("alive", None)
            sample.update({
                "index": index,
                "monotonic_ns": mono,
                "unix_ns": index,
                "utime_ticks": utime,
                "stime_ticks": 0,
                "threads": 1,
                "rss_pages": 1,
                "rss_bytes": 4096,
                "rss_bytes_status": "ok",
                "fd_count": 1,
                "fd_status": "ok",
                "pss_bytes": 0,
                "pss_status": "ok",
                "comm": "worker",
                "comm_status": "ok",
            })
            samples.append(sample)
        pin = tool.Pin(pid=7, pidfd=None, pidfd_status="skipped", starttime=1, boot_id="boot")
        proc = tool.build_process(pin, samples, units, cancelled=False)
        cpu = proc["summary"]["cpu"]
        self.assertEqual(cpu["cpu_percent_one_core"], 50.0)
        self.assertEqual(cpu["cpu_percent_one_core_mean_unweighted"], 100.0)
        self.assertEqual(proc["summary"]["pss_bytes"]["last"], 0)
        self.assertEqual(proc["summary"]["pss_bytes"]["status"], "ok")

    def test_denied_and_zombie_metrics_stay_null(self):
        units = tool.Units(100, 4096)
        denied = tool.classify_stat(
            tool.Pin(pid=3, pidfd=None, pidfd_status="skipped"),
            {"read_status": "denied", "detail": "stat", "pidfd_exited": False},
            units,
        )
        self.assertEqual(denied["status"], "denied")
        self.assertIsNone(denied["utime_ticks"])
        self.assertIsNone(denied["rss_bytes"])
        zombie = tool.classify_stat(
            tool.Pin(pid=3, pidfd=None, pidfd_status="skipped"),
            raw_alive(3, 10, utime=555, state="Z"),
            units,
        )
        self.assertEqual(zombie["status"], "zombie")
        self.assertIsNone(zombie["utime_ticks"])
        self.assertIsNone(zombie["rss_bytes"])
        pin = tool.Pin(pid=3, pidfd=None, pidfd_status="skipped", starttime=10)
        proc = tool.build_process(pin, [tool._stamp(denied, 0, 0, 1), tool._stamp(zombie, 1, 1_000_000_000, 2)], units, False)
        self.assertIsNone(proc["summary"]["cpu"]["cpu_percent_one_core"])
        self.assertNotEqual(proc["summary"]["cpu"]["cpu_percent_one_core"], 0)


class LimitTests(TempCase):
    def test_rejects_pid_duration_period_and_sample_cap(self):
        with self.assertRaises(tool.UsageError):
            tool.validate_bounds([], 1, 100)
        with self.assertRaises(tool.UsageError):
            tool.validate_bounds([1, 1], 1, 100)
        with self.assertRaises(tool.UsageError):
            tool.validate_bounds([0], 1, 100)
        with self.assertRaises(tool.UsageError):
            tool.validate_bounds(list(range(1, 34)), 1, 100)
        with self.assertRaises(tool.UsageError):
            tool.validate_bounds([1], 0, 100)
        with self.assertRaises(tool.UsageError):
            tool.validate_bounds([1], 61, 100)
        with self.assertRaises(tool.UsageError):
            tool.validate_bounds([1], 1, 99)
        with self.assertRaises(tool.UsageError):
            tool.validate_bounds([1], 1, 2001)
        with self.assertRaises(tool.UsageError) as caught:
            tool.validate_bounds([1], 60, 100)
        self.assertIn("601", str(caught.exception))
        self.assertEqual(tool.validate_bounds([1], 60, 101), 595)
        self.assertEqual(tool.validate_bounds([1], 1, 2000), 1)
        self.assertEqual(tool.validate_bounds([1], 1, 1000), 2)

    def test_cli_rejects_bounds_before_any_live_read(self):
        self.block_live()
        out = self.tmp / "out.json"
        code, _stdout, stderr = self.run_main(["--duration", "1", "--period-ms", "100", "--output", str(out)])
        self.assertEqual(code, tool.EXIT_USAGE)
        self.assertFalse(out.exists())
        code, _stdout, stderr = self.run_main([
            "--pid", "1", "--duration", "60", "--period-ms", "100", "--output", str(out),
        ])
        self.assertEqual(code, tool.EXIT_USAGE)
        self.assertIn("601", stderr)
        self.assertFalse(out.exists())
        code, _stdout, stderr = self.run_main([
            "--pid", "1", "--pid", "1", "--duration", "1", "--period-ms", "200", "--output", str(out),
        ])
        self.assertEqual(code, tool.EXIT_USAGE)
        self.assertIn("duplicate", stderr)


class IdentityTests(TempCase):
    def test_reuse_drops_the_new_process_and_stops_reading(self):
        pid = 77
        huge = raw_alive(pid, start=2, utime=999_999, comm=SENTINEL_REUSED)
        proc = QueueProc({
            pid: pair(raw_alive(pid, start=1, utime=0))
            + pair(raw_alive(pid, start=1, utime=50))
            + [huge],
        })
        doc = tool.record(
            tool.Request([pid], 1, 500, self.tmp / "out.json", False, None),
            proc,
            ManualClock(),
            tool.CancelFlag(),
            tool.Units(100, 4096),
            ("abc", "ok"),
            24,
        )
        body = tool.dumps(doc).decode()
        self.assertNotIn(SENTINEL_REUSED, body)
        self.assertNotIn("999999", body)
        process = doc["processes"][0]
        self.assertEqual(process["outcome"], "measured")
        self.assertTrue(process["identity_lost"])
        self.assertEqual(process["summary"]["cpu"]["cpu_percent_one_core"], 100.0)
        self.assertEqual(process["samples"][2]["status"], "reused")
        self.assertIsNone(process["samples"][2]["utime_ticks"])
        self.assertIsNone(process["intervals"][1]["cpu_percent_one_core"])
        self.assertEqual(proc.extra_reads, [pid, pid])
        self.assertEqual(len(proc.stat_reads), 5)

    def test_confirm_read_discards_extras_from_a_reused_pid(self):
        pid = 88
        pin = tool.Pin(pid=pid, pidfd=None, pidfd_status="skipped")
        proc = QueueProc({
            pid: [
                raw_alive(pid, start=1, utime=10),
                raw_alive(pid, start=2, utime=12345, comm=SENTINEL_REUSED),
            ],
        }, extras={pid: {
            "comm": SENTINEL_REUSED,
            "comm_status": "ok",
            "comm_truncated": False,
            "fd_count": 7,
            "fd_status": "ok",
            "pss_bytes": 12345,
            "pss_status": "ok",
        }})
        sample = tool.observe_once(proc, pin, tool.Units(100, 4096), ManualClock())
        self.assertEqual(sample["status"], "reused")
        self.assertIsNone(sample["fd_count"])
        self.assertIsNone(sample["pss_bytes"])
        self.assertNotEqual(sample["comm"], SENTINEL_REUSED)
        self.assertEqual(proc.extra_reads, [pid])

    def test_first_sample_pidfd_exit_does_not_adopt_a_new_task(self):
        pin = tool.Pin(pid=9, pidfd=4, pidfd_status="open")
        raw = raw_alive(9, start=50, utime=80, state="S")
        raw["pidfd_exited"] = True
        sample = tool.classify_stat(pin, raw, tool.Units(100, 4096))
        self.assertEqual(sample["status"], "reused")
        self.assertIsNone(pin.starttime)
        self.assertIsNone(sample["utime_ticks"])

    def test_cancel_writes_no_zero_filled_tail(self):
        pid = 15
        cancel = tool.CancelFlag()

        class StopClock(ManualClock):
            def __init__(self):
                super().__init__()
                self.calls = 0

            def sleep_until(self, target_ns, cancel_flag):
                self.calls += 1
                if self.calls > 1:
                    cancel_flag.reason = "SIGTERM"
                    return
                super().sleep_until(target_ns, cancel_flag)

        proc = QueueProc({pid: pair(raw_alive(pid, start=4, utime=1)) * 6})
        doc = tool.record(
            tool.Request([pid], 1, 200, self.tmp / "out.json", False, None),
            proc,
            StopClock(),
            cancel,
            tool.Units(100, 4096),
            (None, "unavailable"),
            None,
        )
        self.assertTrue(doc["recording"]["cancelled"])
        self.assertEqual(doc["recording"]["cancel_reason"], "SIGTERM")
        self.assertEqual(doc["recording"]["observations_taken"], 1)
        self.assertEqual(doc["recording"]["observations_planned"], 6)
        self.assertEqual(doc["processes"][0]["samples"][0]["status"], "alive")
        self.assertEqual(doc["processes"][0]["outcome"], "no_interval")
        self.assertIsNone(doc["processes"][0]["summary"]["cpu"]["cpu_percent_one_core"])
        self.assertEqual(tool.exit_code(doc), tool.EXIT_CANCELLED)


class ProcfsFixtureTests(TempCase):
    def write_pid(self, root, pid, line, comm="worker", pss="Pss:                   4 kB\nPss_Dirty: 99 kB\n"):
        base = root / str(pid)
        base.mkdir(parents=True, exist_ok=True)
        (base / "stat").write_text(line)
        (base / "comm").write_text(comm + "\n")
        fd_dir = base / "fd"
        fd_dir.mkdir(exist_ok=True)
        for name in fd_dir.iterdir():
            name.unlink()
        os.symlink(f"/nowhere/{SENTINEL_FD}", fd_dir / "0")
        os.symlink(f"/nowhere/{SENTINEL_FD}-b", fd_dir / "1")
        (base / "smaps_rollup").write_text(pss)
        (base / "cmdline").write_bytes(SENTINEL_CMDLINE.encode() + b"\x00")
        (base / "environ").write_bytes(f"{SENTINEL_ENV}=1\x00".encode())
        boot = root / "sys" / "kernel" / "random"
        boot.mkdir(parents=True, exist_ok=True)
        (boot / "boot_id").write_text("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee\n")
        return base

    def test_fixture_reader_allowlist_rates_and_reuse(self):
        root = self.tmp / "proc"
        pid = 4_000_001
        state = {"utime": 0, "start": 1000, "comm": "worker"}

        def publish():
            self.write_pid(
                root,
                pid,
                stat_line(pid, state["comm"], "S", state["utime"], 0, state["start"], threads=2, rss=8),
                comm=state["comm"],
            )

        publish()
        self.write_pid(root, 4_000_002, stat_line(4_000_002, SENTINEL_OTHER, "S", 1, 1, 1), comm=SENTINEL_OTHER)

        def bump():
            state["utime"] = 50
            publish()

        def reuse():
            state["start"] = 2000
            state["utime"] = 999_999
            state["comm"] = SENTINEL_REUSED
            publish()

        proc = tool.Procfs(root, allow_pidfd=False)
        doc = tool.record(
            tool.Request([pid], 1, 500, self.tmp / "out.json", False, None),
            proc,
            StepClock([lambda: None, bump, reuse]),
            tool.CancelFlag(),
            tool.Units(100, 4096),
            ("deadbeef" * 5, "ok"),
            8,
        )
        body = tool.dumps(doc).decode()
        for secret in (SENTINEL_CMDLINE, SENTINEL_ENV, SENTINEL_OTHER, SENTINEL_FD, SENTINEL_REUSED, "999999"):
            self.assertNotIn(secret, body)
        process = doc["processes"][0]
        self.assertEqual(process["outcome"], "measured")
        self.assertEqual(process["comm"], "worker")
        self.assertEqual(process["summary"]["cpu"]["cpu_percent_one_core"], 100.0)
        self.assertEqual(process["summary"]["rss_bytes"]["last"], 8 * 4096)
        self.assertEqual(process["summary"]["threads"]["last"], 2)
        self.assertEqual(process["summary"]["fd_count"]["last"], 2)
        self.assertEqual(process["summary"]["pss_bytes"]["last"], 4096)
        self.assertEqual(process["identity"]["boot_id"], "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
        self.assertEqual(process["identity"]["pidfd_status"], "skipped")
        self.assertEqual(process["samples"][2]["status"], "reused")
        self.assertTrue(process["identity_lost"])
        self.assertEqual([sample["index"] for sample in process["samples"]], [0, 1, 2])
        self.assertEqual(list(process["samples"][0].keys()), list(tool.SAMPLE_KEYS))

    def test_denied_malformed_symlink_and_real_pss_zero(self):
        if os.geteuid() == 0:
            self.skipTest("root bypasses mode bits")
        root = self.tmp / "proc"
        pid = 4_000_003
        base = self.write_pid(root, pid, stat_line(pid, "worker", "S", 1, 1, 5), pss="Pss: 0 kB\nPss_Dirty: 8 kB\n")
        proc = tool.Procfs(root, allow_pidfd=False)
        pin = tool.Pin(pid=pid, pidfd=None, pidfd_status="skipped")
        extras = proc.read_extras(pid)
        self.assertEqual(extras["pss_bytes"], 0)
        self.assertEqual(extras["pss_status"], "ok")
        self.assertEqual(extras["fd_count"], 2)
        os.chmod(base / "smaps_rollup", 0)
        denied = proc.read_extras(pid)
        self.assertIsNone(denied["pss_bytes"])
        self.assertEqual(denied["pss_status"], "denied")
        os.chmod(base / "smaps_rollup", 0o600)
        (base / "smaps_rollup").write_text("Rss: 1 kB\n")
        malformed = proc.read_extras(pid)
        self.assertIsNone(malformed["pss_bytes"])
        self.assertEqual(malformed["pss_status"], "malformed")
        (base / "smaps_rollup").unlink()
        missing = proc.read_extras(pid)
        self.assertEqual(missing["pss_status"], "unsupported")
        os.chmod(base / "stat", 0)
        stat_raw = proc.read_stat(pin)
        self.assertEqual(stat_raw["read_status"], "denied")
        sample = tool.classify_stat(pin, stat_raw, tool.Units(100, 4096))
        self.assertEqual(sample["status"], "denied")
        self.assertIsNone(sample["utime_ticks"])
        os.chmod(base / "stat", 0o600)
        leak = base / "leak-stat"
        leak.write_text(stat_line(pid, "SENTINEL_FOLLOWED_STAT", "S", 9, 9, 5))
        (base / "stat").unlink()
        os.symlink(leak, base / "stat")
        linked = proc.read_stat(pin)
        self.assertEqual(linked["read_status"], "unsupported")
        self.assertNotIn("SENTINEL_FOLLOWED_STAT", json.dumps(linked))
        (base / "stat").unlink()
        (base / "stat").write_text("this is not a stat line\n")
        malformed = proc.read_stat(tool.Pin(pid=pid, pidfd=None, pidfd_status="skipped"))
        self.assertEqual(malformed["read_status"], "malformed")
        self.assertIsNone(malformed["utime"])

    def test_zombie_fixture_keeps_metrics_null(self):
        root = self.tmp / "proc"
        pid = 4_000_005
        self.write_pid(root, pid, stat_line(pid, "worker", "Z", 40, 5, 9, rss=3))
        proc = tool.Procfs(root, allow_pidfd=False)
        pin = tool.Pin(pid=pid, pidfd=None, pidfd_status="skipped")
        sample = tool.classify_stat(pin, proc.read_stat(pin), tool.Units(100, 4096))
        self.assertEqual(sample["status"], "zombie")
        self.assertEqual(sample["state"], "Z")
        self.assertIsNone(sample["utime_ticks"])
        self.assertIsNone(sample["rss_bytes"])
        self.assertIsNone(sample["threads"])

    def test_pidfd_flag_does_not_open_the_caller(self):
        proc = tool.Procfs(self.tmp, allow_pidfd=False)
        fd, status = proc.open_pidfd(os.getpid())
        self.assertIsNone(fd)
        self.assertEqual(status, "skipped")

    def test_path_rejects_cmdline_and_escape(self):
        proc = tool.Procfs(self.tmp, allow_pidfd=False)
        with self.assertRaises(PermissionError):
            proc._path("12/cmdline")
        with self.assertRaises(PermissionError):
            proc._path("12/environ")
        with self.assertRaises(PermissionError):
            proc._path("12/../12/stat")
        self.assertTrue(proc._path("12/stat").endswith("/12/stat"))

    def test_fd_cap_does_not_publish_a_partial_count(self):
        root = self.tmp / "proc"
        pid = 4_000_004
        base = self.write_pid(root, pid, stat_line(pid, "worker", "S", 1, 0, 1))
        fd_dir = base / "fd"
        for name in ("2", "3"):
            os.symlink("/dev/null", fd_dir / name)
        original = tool.FD_CAP
        tool.FD_CAP = 2
        try:
            count, status = tool.Procfs(root, allow_pidfd=False)._count_fds(pid)
        finally:
            tool.FD_CAP = original
        self.assertIsNone(count)
        self.assertEqual(status, "capped")


class OutputTests(TempCase):
    def test_refuses_symlink_fifo_directory_and_existing_file(self):
        self.block_live()
        target = self.tmp / "secret.json"
        target.write_text("SECRET_TARGET")
        link = self.tmp / "out.json"
        link.symlink_to(target)
        code, _stdout, stderr = self.run_main([
            "--pid", "1", "--duration", "1", "--period-ms", "500", "--output", str(link), "--force",
        ])
        self.assertEqual(code, tool.EXIT_OUTPUT)
        self.assertIn("symlink", stderr)
        self.assertEqual(target.read_text(), "SECRET_TARGET")
        fifo = self.tmp / "fifo.json"
        os.mkfifo(fifo)
        code, _stdout, stderr = self.run_main([
            "--pid", "1", "--duration", "1", "--period-ms", "500", "--output", str(fifo),
        ])
        self.assertEqual(code, tool.EXIT_OUTPUT)
        self.assertTrue(stat.S_ISFIFO(fifo.lstat().st_mode))
        directory = self.tmp / "dir.json"
        directory.mkdir()
        code, _stdout, _stderr = self.run_main([
            "--pid", "1", "--duration", "1", "--period-ms", "500", "--output", str(directory),
        ])
        self.assertEqual(code, tool.EXIT_OUTPUT)
        self.assertTrue(directory.is_dir())
        existing = self.tmp / "keep.json"
        existing.write_text("ORIGINAL")
        code, _stdout, stderr = self.run_main([
            "--pid", "1", "--duration", "1", "--period-ms", "500", "--output", str(existing),
        ])
        self.assertEqual(code, tool.EXIT_OUTPUT)
        self.assertEqual(existing.read_text(), "ORIGINAL")

    def test_force_replaces_regular_file_and_failed_replace_keeps_it(self):
        path = self.tmp / "out.json"
        path.write_text("ORIGINAL")
        tool.record_live = lambda request, _cancel, _clock: _measured_document(request)
        original_replace = tool.os.replace

        def boom(_src, _dst):
            raise OSError(errno.ENOSPC, "No space left on device")

        tool.os.replace = boom
        try:
            code, stdout, stderr = self.run_main([
                "--pid", "7", "--duration", "1", "--period-ms", "500",
                "--output", str(path), "--force",
            ])
        finally:
            tool.os.replace = original_replace
        self.assertEqual(code, tool.EXIT_OUTPUT)
        self.assertIn("output write failed", stderr)
        self.assertIn("No space left on device", stderr)
        self.assertNotIn("Traceback", stderr)
        self.assertEqual(stdout, "")
        self.assertEqual(path.read_text(), "ORIGINAL")
        self.assertEqual(list(self.tmp.glob(".*tmp")), [])
        tool.publish_bytes(path, b'{"complete": true}\n', force=True)
        self.assertEqual(json.loads(path.read_text()), {"complete": True})
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)

    def test_create_is_exclusive_and_mode_0600(self):
        path = self.tmp / "fresh.json"
        old = os.umask(0o000)
        try:
            tool.publish_bytes(path, b'{"a": 1}\n', force=False)
        finally:
            os.umask(old)
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)
        with self.assertRaises(tool.OutputError):
            tool.publish_bytes(path, b'{"a": 2}\n', force=False)
        self.assertEqual(json.loads(path.read_text())["a"], 1)

    def test_compare_input_bounds(self):
        self.block_live()
        output = self.tmp / "out.json"
        secret = self.tmp / "secret.json"
        secret.write_text("SECRET_COMPARE")
        link = self.tmp / "compare.json"
        link.symlink_to(secret)
        code, _stdout, stderr = self.run_main([
            "--pid", "1", "--duration", "1", "--period-ms", "500",
            "--output", str(output), "--compare", str(link),
        ])
        self.assertEqual(code, tool.EXIT_OUTPUT)
        self.assertIn("symlink", stderr)
        self.assertEqual(secret.read_text(), "SECRET_COMPARE")
        self.assertFalse(output.exists())
        fifo = self.tmp / "compare-fifo.json"
        os.mkfifo(fifo)
        code, _stdout, _stderr = self.run_main([
            "--pid", "1", "--duration", "1", "--period-ms", "500",
            "--output", str(output), "--compare", str(fifo),
        ])
        self.assertEqual(code, tool.EXIT_OUTPUT)
        huge = self.tmp / "huge.json"
        huge.write_text("{}")
        original = tool.COMPARE_MAX_BYTES
        tool.COMPARE_MAX_BYTES = 1
        try:
            code, _stdout, stderr = self.run_main([
                "--pid", "1", "--duration", "1", "--period-ms", "500",
                "--output", str(output), "--compare", str(huge),
            ])
        finally:
            tool.COMPARE_MAX_BYTES = original
        self.assertEqual(code, tool.EXIT_OUTPUT)
        self.assertIn("exceeds", stderr)
        code, _stdout, stderr = self.run_main([
            "--pid", "1", "--duration", "1", "--period-ms", "500",
            "--output", str(output), "--compare", str(output),
        ])
        self.assertEqual(code, tool.EXIT_USAGE)


class CompareTests(TempCase):
    def measured(self, pid, comm, cpu, rss, boot="boot", start=10, duration=1, period=500):
        return {
            "schema": tool.SCHEMA,
            "recording": {"duration_requested_s": duration, "period_requested_ms": period},
            "processes": [{
                "pid": pid,
                "comm": comm,
                "identity": {"boot_id": boot, "starttime_ticks": start, "pidfd_status": "open"},
                "summary": {
                    "cpu": {"cpu_percent_one_core": cpu},
                    "rss_bytes": {"last": rss},
                },
            }],
        }

    def test_matched_delta_and_incomparable_period(self):
        current = self.measured(5, "helper", 80.0, 1000)
        previous = self.measured(5, "helper", 20.0, 400)
        compared = tool.compare_recordings(current, previous)
        self.assertFalse(compared["desktop_environment_benchmark"])
        self.assertTrue(compared["matched_run"])
        self.assertIn("Not a desktop-environment benchmark", compared["note"])
        self.assertEqual(compared["processes"][0]["cpu_percent_one_core"]["delta"], 60.0)
        self.assertEqual(compared["processes"][0]["match"], "boot_pid_starttime")
        shifted = self.measured(5, "helper", 20.0, 400, period=1000)
        incomparable = tool.compare_recordings(current, shifted)
        self.assertFalse(incomparable["matched_run"])
        self.assertEqual(incomparable["processes"], [])
        self.assertIn("period", incomparable["incomparable_reasons"])
        summary = tool.format_summary({
            "recording": {
                "observations_taken": 2,
                "duration_requested_s": 1,
                "period_requested_ms": 500,
                "cancelled": False,
                "online_cpus": None,
            },
            "processes": [{
                "pid": 5,
                "comm": "gnome-shell",
                "outcome": "measured",
                "summary": {
                    "cpu": {"status": "ok", "cpu_percent_one_core": 1, "cpu_percent_one_core_min": 1, "cpu_percent_one_core_max": 1},
                    "pss_bytes": {"status": "ok", "last": 1},
                },
            }],
            "comparison": incomparable,
        })
        self.assertIn("incomparable", summary)
        self.assertIn("Not a desktop-environment benchmark", summary)
        self.assertNotIn("faster", summary.lower())

    def test_missing_metric_is_not_compared_to_zero(self):
        current = self.measured(5, "helper", 80.0, None)
        previous = self.measured(5, "helper", None, 10)
        pair = tool.compare_recordings(current, previous)["processes"][0]
        self.assertIsNone(pair["cpu_percent_one_core"]["delta"])
        self.assertEqual(pair["cpu_percent_one_core"]["status"], "not_measured")
        self.assertIsNone(pair["rss_bytes_last"]["delta"])
        self.assertNotEqual(pair["cpu_percent_one_core"]["delta"], 80.0)


def _measured_document(request):
    """A finished recording used when a test must not read a live process."""
    return {
        "schema": tool.SCHEMA,
        "tool": tool.TOOL_NAME,
        "cpu_semantics": tool.CPU_SEMANTICS,
        "recording": {
            "duration_requested_s": request.duration_s,
            "period_requested_ms": request.period_ms,
            "observations_taken": 2,
            "cancelled": False,
            "cancel_reason": None,
            "online_cpus": None,
        },
        "processes": [{
            "pid": request.pids[0],
            "comm": "helper",
            "outcome": "measured",
            "identity": {"boot_id": "boot", "starttime_ticks": 10, "pidfd_status": "skipped"},
            "summary": {
                "cpu": {
                    "status": "ok",
                    "cpu_percent_one_core": 10.0,
                    "cpu_percent_one_core_min": 10.0,
                    "cpu_percent_one_core_max": 10.0,
                },
                "rss_bytes": {"status": "ok", "last": 4096},
                "pss_bytes": {"status": "ok", "last": 4096},
            },
        }],
        "comparison": None,
        "warnings": [],
    }


class DelayProc(QueueProc):
    """Advances the clock while extras are collected, after the first stat read."""

    def __init__(self, clock, extra_delays, stats):
        super().__init__(stats)
        self.clock = clock
        self.extra_delays = {pid: list(delays) for pid, delays in extra_delays.items()}
        self.confirm_ns = {}
        self._stat_n = {}

    def read_stat(self, pin):
        result = super().read_stat(pin)
        seen = self._stat_n.get(pin.pid, 0)
        if seen % 2 == 1:
            self.confirm_ns.setdefault(pin.pid, []).append(self.clock.monotonic_ns())
        self._stat_n[pin.pid] = seen + 1
        return result

    def read_extras(self, pid):
        result = super().read_extras(pid)
        delays = self.extra_delays.get(pid) or []
        if delays:
            delay = delays.pop(0)
            self.clock.mono += delay
            self.clock.wall += delay
        return result


class ReviewFixTests(TempCase):
    def write_pid(self, root, pid):
        base = root / str(pid)
        base.mkdir(parents=True)
        (base / "stat").write_text(stat_line(pid, "worker", "S", 0, 0, 50))
        (base / "comm").write_text("worker\n")
        fd_dir = base / "fd"
        fd_dir.mkdir()
        os.symlink("/dev/null", fd_dir / "0")
        os.symlink("/dev/null", fd_dir / "1")
        (base / "smaps_rollup").write_text("Pss:                   4 kB\n")
        boot = root / "sys" / "kernel" / "random"
        boot.mkdir(parents=True, exist_ok=True)
        (boot / "boot_id").write_text("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee\n")
        return base

    def record(self, proc, pids):
        return tool.record(
            tool.Request(pids, 1, 1000, self.tmp / "out.json", False, None),
            proc,
            ManualClock(),
            tool.CancelFlag(),
            tool.Units(100, 4096),
            (None, "unavailable"),
            4,
        )

    def test_post_open_proc_errors_publish_the_other_pid(self):
        root = self.tmp / "proc"
        dead = 4_000_011
        live = 4_000_012
        denied = 4_000_013
        for pid in (dead, live, denied):
            self.write_pid(root, pid)
        proc = tool.Procfs(root, allow_pidfd=False)
        real_read = tool.os.read

        def raise_esrch(_fd, _size):
            raise OSError(errno.ESRCH, "No such process")

        def raise_eacces(_fd, _size):
            raise OSError(errno.EACCES, "Permission denied")

        tool.os.read = raise_esrch
        try:
            data, status = proc._read_bytes(f"{dead}/stat", tool.STAT_MAX)
        finally:
            tool.os.read = real_read
        self.assertIsNone(data)
        self.assertEqual(status, "missing")
        tool.os.read = raise_eacces
        try:
            data, status = proc._read_bytes(f"{dead}/stat", tool.STAT_MAX)
        finally:
            tool.os.read = real_read
        self.assertIsNone(data)
        self.assertEqual(status, "denied")

        real_scan = tool.os.scandir

        def scan_denied(_fd):
            raise OSError(errno.EACCES, "Permission denied")

        def scan_missing(_fd):
            raise OSError(errno.ESRCH, "No such process")

        tool.os.scandir = scan_denied
        try:
            count, status = proc._count_fds(live)
        finally:
            tool.os.scandir = real_scan
        self.assertIsNone(count)
        self.assertEqual(status, "denied")
        tool.os.scandir = scan_missing
        try:
            count, status = proc._count_fds(live)
        finally:
            tool.os.scandir = real_scan
        self.assertIsNone(count)
        self.assertEqual(status, "missing")

        proc._post_open_faults.append((f"{dead}/stat", errno.ESRCH))
        doc = self.record(proc, [dead, live])
        gone = doc["processes"][0]
        kept = doc["processes"][1]
        self.assertEqual(gone["samples"][0]["status"], "missing")
        self.assertIsNone(gone["samples"][0]["utime_ticks"])
        self.assertIsNone(gone["samples"][0]["stime_ticks"])
        self.assertIsNone(gone["samples"][0]["rss_bytes"])
        self.assertIsNone(gone["samples"][0]["fd_count"])
        self.assertEqual(gone["samples"][1]["status"], "missing")
        self.assertEqual(gone["samples"][1]["detail"], "carried")
        self.assertEqual(kept["outcome"], "measured")
        self.assertEqual(kept["samples"][0]["status"], "alive")
        self.assertEqual(kept["samples"][0]["utime_ticks"], 0)
        self.assertEqual(kept["summary"]["cpu"]["cpu_percent_one_core"], 0.0)

        other = tool.Procfs(root, allow_pidfd=False)
        other._post_open_faults.append((f"{denied}/fd", errno.EACCES))
        doc = self.record(other, [denied, live])
        sample = doc["processes"][0]["samples"][0]
        self.assertEqual(sample["status"], "alive")
        self.assertEqual(sample["utime_ticks"], 0)
        self.assertIsNotNone(sample["rss_bytes"])
        self.assertIsNone(sample["fd_count"])
        self.assertEqual(sample["fd_status"], "denied")
        self.assertEqual(sample["pss_bytes"], 4096)
        self.assertEqual(sample["pss_status"], "ok")
        self.assertEqual(doc["processes"][1]["pid"], live)
        self.assertEqual(doc["processes"][1]["outcome"], "measured")

    def test_counter_timestamps_follow_collection_delay_and_pid_order(self):
        slow = 4_000_021
        fast = 4_000_022

        def run(order):
            clock = ManualClock()
            stats = {
                slow: pair(raw_alive(slow, 10, 0)) + pair(raw_alive(slow, 10, 0)),
                fast: pair(raw_alive(fast, 20, 0)) + pair(raw_alive(fast, 20, 100)),
            }
            proc = DelayProc(
                clock,
                {slow: [400_000_000, 0], fast: [0, 0]},
                stats,
            )
            doc = tool.record(
                tool.Request(order, 1, 1000, self.tmp / "times.json", False, None),
                proc,
                clock,
                tool.CancelFlag(),
                tool.Units(100, 4096),
                (None, "unavailable"),
                24,
            )
            return doc, proc

        def process(doc, pid):
            return next(item for item in doc["processes"] if item["pid"] == pid)

        slow_first, slow_proc = run([slow, fast])
        fast_first, fast_proc = run([fast, slow])
        delayed = process(slow_first, fast)
        leading = process(fast_first, fast)
        self.assertEqual(delayed["intervals"][0]["utime_ticks_delta"], 100)
        self.assertEqual(leading["intervals"][0]["utime_ticks_delta"], 100)
        self.assertEqual(delayed["intervals"][0]["elapsed_ns"], 600_000_000)
        self.assertEqual(leading["intervals"][0]["elapsed_ns"], 1_000_000_000)
        self.assertEqual(delayed["summary"]["cpu"]["cpu_percent_one_core"], 166.6667)
        self.assertEqual(leading["summary"]["cpu"]["cpu_percent_one_core"], 100.0)
        slow_sample = process(slow_first, slow)["samples"][0]
        self.assertEqual(slow_sample["monotonic_ns"], 400_000_000)
        self.assertEqual(slow_sample["index"], 0)
        self.assertNotEqual(slow_sample["monotonic_ns"], slow_first["recording"]["started_monotonic_ns"])
        self.assertEqual(slow_sample["monotonic_ns"], slow_proc.confirm_ns[slow][0])
        self.assertEqual(delayed["samples"][0]["monotonic_ns"], slow_proc.confirm_ns[fast][0])
        self.assertEqual(delayed["samples"][0]["monotonic_ns"], 400_000_000)
        self.assertEqual(leading["samples"][0]["monotonic_ns"], fast_proc.confirm_ns[fast][0])
        self.assertEqual(leading["samples"][0]["monotonic_ns"], 0)
        self.assertEqual(process(slow_first, slow)["summary"]["cpu"]["cpu_percent_one_core"], 0.0)
        self.assertEqual(process(fast_first, slow)["summary"]["cpu"]["cpu_percent_one_core"], 0.0)

    def test_fd_enumeration_stops_at_cap_plus_one(self):
        root = self.tmp / "proc"
        pid = 4_000_031
        fd_dir = root / str(pid) / "fd"
        fd_dir.mkdir(parents=True)
        os.symlink("/dev/null", fd_dir / "0")
        yielded = {"n": 0}

        class Bounded:
            def __iter__(self):
                return self

            def __next__(self):
                if yielded["n"] >= 50:
                    raise StopIteration
                yielded["n"] += 1
                return object()

            def close(self):
                return None

        real_scan = tool.os.scandir
        original_cap = tool.FD_CAP
        tool.os.scandir = lambda _fd: Bounded()
        tool.FD_CAP = 4
        try:
            count, status = tool.Procfs(root, allow_pidfd=False)._count_fds(pid)
        finally:
            tool.os.scandir = real_scan
            tool.FD_CAP = original_cap
        self.assertEqual(yielded["n"], 5)
        self.assertLess(yielded["n"], 50)
        self.assertIsNone(count)
        self.assertEqual(status, "capped")

    def test_compare_rejects_nested_types_and_nonfinite_values(self):
        def document(cpu, rss=10):
            return {
                "schema": tool.SCHEMA,
                "recording": {"duration_requested_s": 1, "period_requested_ms": 500},
                "processes": [{
                    "pid": 5,
                    "comm": "helper",
                    "identity": {"boot_id": "boot", "starttime_ticks": 10},
                    "summary": {
                        "cpu": {"cpu_percent_one_core": cpu},
                        "rss_bytes": {"last": rss},
                    },
                }],
            }

        current = document(80.0, 1000)
        identity_list = document(20.0, 400)
        identity_list["processes"][0]["identity"] = [1]
        summary_list = document(20.0, 400)
        summary_list["processes"][0]["summary"] = [1]
        for previous in (identity_list, summary_list):
            compared = tool.compare_recordings(current, previous)
            self.assertFalse(compared["matched_run"])
            self.assertEqual(compared["processes"], [])
            self.assertIn("structure", compared["incomparable_reasons"])
            self.assertFalse(compared["desktop_environment_benchmark"])
            self.assertIn("Not a desktop-environment benchmark", compared["note"])
        self.assertFalse(tool._same_identity(
            {"pid": 5, "identity": [1]},
            {"pid": 5, "identity": {"boot_id": "boot", "starttime_ticks": 10}},
        ))
        for bad in (float("nan"), float("inf")):
            compared = tool.compare_recordings(current, document(bad))
            self.assertFalse(compared["matched_run"])
            self.assertEqual(compared["processes"], [])
            self.assertIn("values", compared["incomparable_reasons"])
            json.dumps(compared, allow_nan=False)
        pair = tool._metric_pair(float("nan"), 1.0)
        self.assertEqual(pair["status"], "not_measured")
        self.assertIsNone(pair["current"])
        self.assertIsNone(pair["delta"])
        json.dumps(pair, allow_nan=False)

    def test_comparison_error_still_publishes_the_current_sample(self):
        output = self.tmp / "out.json"
        previous = self.tmp / "previous.json"
        previous.write_text(json.dumps({
            "schema": tool.SCHEMA,
            "recording": {"duration_requested_s": 1, "period_requested_ms": 500},
            "processes": [{
                "pid": 7,
                "comm": "helper",
                "identity": [1],
                "summary": [1],
            }],
        }))
        tool.record_live = lambda request, _cancel, _clock: _measured_document(request)
        code, stdout, stderr = self.run_main([
            "--pid", "7", "--duration", "1", "--period-ms", "500",
            "--output", str(output), "--compare", str(previous),
        ])
        self.assertEqual(code, tool.EXIT_OK)
        self.assertEqual(stderr, "")
        self.assertNotIn("Traceback", stdout)
        self.assertIn("incomparable", stdout)
        self.assertNotIn("faster", stdout.lower())
        saved = json.loads(output.read_text())
        self.assertEqual(saved["processes"][0]["pid"], 7)
        self.assertEqual(saved["processes"][0]["outcome"], "measured")
        self.assertEqual(saved["processes"][0]["summary"]["cpu"]["cpu_percent_one_core"], 10.0)
        self.assertFalse(saved["comparison"]["matched_run"])
        self.assertEqual(saved["comparison"]["processes"], [])
        self.assertIn("structure", saved["comparison"]["incomparable_reasons"])
        self.assertFalse(saved["comparison"]["desktop_environment_benchmark"])

        guard_output = self.tmp / "guard.json"
        guard_previous = self.tmp / "guard-previous.json"
        guard_previous.write_text('{"schema": "decebal.desktop-performance.v1"}')
        real_compare = tool.compare_recordings

        def explode(_current, _previous):
            raise AttributeError("identity")

        tool.compare_recordings = explode
        try:
            code, stdout, stderr = self.run_main([
                "--pid", "7", "--duration", "1", "--period-ms", "500",
                "--output", str(guard_output), "--compare", str(guard_previous),
            ])
        finally:
            tool.compare_recordings = real_compare
        self.assertEqual(code, tool.EXIT_OK)
        self.assertNotIn("Traceback", stderr)
        self.assertNotIn("Traceback", stdout)
        saved = json.loads(guard_output.read_text())
        self.assertEqual(saved["processes"][0]["outcome"], "measured")
        self.assertEqual(saved["processes"][0]["summary"]["cpu"]["cpu_percent_one_core"], 10.0)
        self.assertEqual(saved["comparison"]["incomparable_reasons"], ["comparison_error"])
        self.assertEqual(saved["comparison"]["processes"], [])
        self.assertFalse(saved["comparison"]["desktop_environment_benchmark"])
        self.assertIn("comparison_error", saved["warnings"])

    def test_publish_filesystem_errors_are_output_errors(self):
        path = self.tmp / "out.json"
        path.write_text("ORIGINAL")
        real_fsync = tool.os.fsync

        def fail_fsync(_fd):
            raise OSError(errno.EIO, "Input/output error")

        tool.os.fsync = fail_fsync
        try:
            with self.assertRaises(tool.OutputError) as caught:
                tool.publish_bytes(path, b'{"a": 1}\n', force=True)
        finally:
            tool.os.fsync = real_fsync
        self.assertIn("output write failed", str(caught.exception))
        self.assertIn("Input/output error", str(caught.exception))
        self.assertEqual(path.read_text(), "ORIGINAL")
        self.assertEqual(list(self.tmp.glob(".*tmp")), [])

        fresh = self.tmp / "fresh.json"
        real_open = tool.os.open

        def fail_open(*_args, **_kwargs):
            raise OSError(errno.EACCES, "Permission denied")

        tool.os.open = fail_open
        try:
            with self.assertRaises(tool.OutputError) as caught:
                tool.publish_bytes(fresh, b'{"a": 1}\n', force=False)
        finally:
            tool.os.open = real_open
        self.assertIn("output write failed", str(caught.exception))
        self.assertIn("Permission denied", str(caught.exception))
        self.assertFalse(fresh.exists())
        self.assertEqual(list(self.tmp.glob(".*tmp")), [])

        real_stat = tool.os.stat

        def fail_stat(*_args, **_kwargs):
            raise OSError(errno.EACCES, "Permission denied")

        tool.os.stat = fail_stat
        try:
            with self.assertRaises(tool.OutputError) as caught:
                tool.describe_output(self.tmp / "elsewhere.json", False)
        finally:
            tool.os.stat = real_stat
        self.assertIn("output write failed", str(caught.exception))
        self.assertIn("Permission denied", str(caught.exception))


class SourceContractTests(unittest.TestCase):
    def test_tool_source_has_no_signals_shell_or_command_line_reads(self):
        source = TOOL_PATH.read_text(encoding="utf-8")
        tree = ast.parse(source)
        for node in ast.walk(tree):
            if isinstance(node, ast.Attribute) and node.attr in {"kill", "killpg", "system", "popen"}:
                self.fail(f"forbidden call {node.attr}")
            if isinstance(node, ast.Constant) and isinstance(node.value, str):
                self.assertNotIn(node.value, {"cmdline", "environ"})
            if isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute) and node.func.attr == "run":
                values = []
                for elt in node.args[0].elts:
                    if isinstance(elt, ast.Constant):
                        values.append(elt.value)
                self.assertEqual(values, ["git", "-C", "rev-parse", "HEAD"])
                for keyword in node.keywords:
                    if keyword.arg == "shell":
                        self.assertIs(keyword.value.value, False)
        self.assertEqual(tool.READ_NAMES, frozenset({"stat", "comm", "fd", "smaps_rollup"}))


class OwnedHelper:
    def __init__(self, code):
        env = os.environ.copy()
        env["DESKTOP_PERF_SENTINEL"] = SENTINEL_ENV
        self.p = subprocess.Popen(
            [sys.executable, "-c", code],
            start_new_session=True,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            env=env,
        )
        self.born = time.monotonic()
        self.died = None

    def alive(self):
        return self.p.poll() is None

    def reap(self):
        if self.p.poll() is None:
            try:
                os.killpg(self.p.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        try:
            self.p.wait(timeout=2)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(self.p.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            self.p.wait(timeout=2)
        self.died = time.monotonic()

    def lifetime(self):
        end = self.died if self.died is not None else time.monotonic()
        return end - self.born


def reap_popen(proc):
    if proc.poll() is None:
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    try:
        proc.wait(timeout=2)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        proc.wait(timeout=2)


def assert_reaped(case, owned):
    owned.reap()
    with case.assertRaises(ProcessLookupError):
        os.kill(owned.p.pid, 0)


class OwnedProbeTests(TempCase):
    def spawn(self, code):
        helper = OwnedHelper(code)
        self.addCleanup(helper.reap)
        return helper

    def test_term_does_not_kill_targets_or_leave_them(self):
        first = self.spawn(IDLE_CODE)
        second = self.spawn(IDLE_CODE)
        out = self.tmp / "term.json"
        recorder = subprocess.Popen(
            [
                sys.executable,
                str(TOOL_PATH),
                "--pid", str(first.p.pid),
                "--pid", str(second.p.pid),
                "--duration", "5",
                "--period-ms", "200",
                "--output", str(out),
            ],
            start_new_session=True,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        self.addCleanup(lambda: reap_popen(recorder))
        time.sleep(0.4)
        self.assertIsNone(recorder.poll(), "recorder exited before SIGTERM")
        os.kill(recorder.pid, signal.SIGTERM)
        try:
            stdout, stderr = recorder.communicate(timeout=3)
        except subprocess.TimeoutExpired:
            reap_popen(recorder)
            self.fail("recorder did not exit after SIGTERM")
        self.assertEqual(recorder.returncode, tool.EXIT_CANCELLED, stderr)
        self.assertTrue(first.alive())
        self.assertTrue(second.alive())
        doc = json.loads(out.read_text())
        self.assertTrue(doc["recording"]["cancelled"])
        self.assertEqual(doc["recording"]["cancel_reason"], "SIGTERM")
        self.assertEqual([proc["pid"] for proc in doc["processes"]], [first.p.pid, second.p.pid])
        self.assertFalse(doc["cpu_semantics"]["machine_share"] == "reported")
        for proc in doc["processes"]:
            self.assertIsNone(proc["summary"]["cpu"]["machine_share_percent"])
        assert_reaped(self, first)
        assert_reaped(self, second)
        self.assertLessEqual(first.lifetime(), HELPER_CAP_S + 0.5)
        self.assertLessEqual(second.lifetime(), HELPER_CAP_S + 0.5)
        text = stdout + stderr + out.read_text()
        self.assertNotIn(SENTINEL_ENV, text)

    def test_busy_and_idle_probe_stays_inside_the_helper_cap(self):
        idle = self.spawn(IDLE_CODE)
        busy = self.spawn(BUSY_CODE)
        out = self.tmp / "probe.json"
        recorder = subprocess.Popen(
            [
                sys.executable,
                str(TOOL_PATH),
                "--pid", str(busy.p.pid),
                "--pid", str(idle.p.pid),
                "--duration", "2",
                "--period-ms", "200",
                "--output", str(out),
            ],
            start_new_session=True,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        self.addCleanup(lambda: reap_popen(recorder))
        time.sleep(0.25)
        self.assertIsNone(recorder.poll(), "recorder finished before the duration")
        children = Path(f"/proc/{recorder.pid}/task/{recorder.pid}/children")
        if children.exists():
            self.assertEqual(children.read_text().split(), [])
        try:
            stdout, stderr = recorder.communicate(timeout=4)
        except subprocess.TimeoutExpired:
            reap_popen(recorder)
            self.fail("recorder exceeded the duration bound")
        self.assertEqual(recorder.returncode, 0, stdout + stderr)
        assert_reaped(self, idle)
        assert_reaped(self, busy)
        self.assertLessEqual(busy.lifetime(), HELPER_CAP_S + 0.5)
        self.assertLessEqual(idle.lifetime(), HELPER_CAP_S + 0.5)
        doc = json.loads(out.read_text())
        text = stdout + stderr + out.read_text()
        self.assertNotIn(SENTINEL_CMDLINE, text)
        self.assertNotIn(SENTINEL_ENV, text)
        self.assertEqual([proc["pid"] for proc in doc["processes"]], [busy.p.pid, idle.p.pid])
        self.assertEqual(doc["cpu_semantics"]["basis"], "one_core")
        self.assertEqual(doc["recording"]["observations_taken"], 11)
        self.assertEqual(doc["recording"]["git_hash_status"], "ok")
        self.assertEqual(len(doc["recording"]["git_hash"]), 40)
        by_pid = {proc["pid"]: proc for proc in doc["processes"]}
        busy_cpu = by_pid[busy.p.pid]["summary"]["cpu"]["cpu_percent_one_core"]
        idle_cpu = by_pid[idle.p.pid]["summary"]["cpu"]["cpu_percent_one_core"]
        self.assertGreater(busy_cpu, 15, msg=json.dumps(by_pid[busy.p.pid]["summary"]["cpu"]))
        self.assertLess(idle_cpu, 10, msg=json.dumps(by_pid[idle.p.pid]["summary"]["cpu"]))
        self.assertGreater(busy_cpu, idle_cpu * 3)
        for proc in doc["processes"]:
            ok = [item for item in proc["intervals"] if item["status"] == "ok"]
            ticks = sum(item["utime_ticks_delta"] + item["stime_ticks_delta"] for item in ok)
            elapsed_ns = sum(item["elapsed_ns"] for item in ok)
            expected = ticks / doc["recording"]["clk_tck"] / (elapsed_ns / 1_000_000_000) * 100
            self.assertAlmostEqual(proc["summary"]["cpu"]["cpu_percent_one_core"], round(expected, 4), places=3)
            self.assertIsNone(proc["summary"]["cpu"]["machine_share_percent"])
            self.assertEqual(proc["outcome"], "measured")
        self.assertIn("100% = one core", stdout)
        self.assertIn("machine share is not reported", stdout)
        evidence = os.environ.get("DESKTOP_PERF_EVIDENCE_DIR")
        if evidence:
            self.write_evidence(Path(evidence), doc, busy_cpu, idle_cpu)

    def write_evidence(self, directory, doc, busy_cpu, idle_cpu):
        directory.mkdir(parents=True, exist_ok=True)
        (directory / "probe.json").write_text(json.dumps(doc, indent=2) + "\n", encoding="ascii")
        lines = [
            "pid,comm,outcome,cpu_percent_one_core,cpu_status,rss_bytes_last,rss_status,"
            "threads_last,fd_last,fd_status,pss_last,pss_status"
        ]
        for proc in doc["processes"]:
            cpu = proc["summary"]["cpu"]
            rss = proc["summary"]["rss_bytes"]
            threads = proc["summary"]["threads"]
            fd_count = proc["summary"]["fd_count"]
            pss = proc["summary"]["pss_bytes"]

            def cell(value):
                return "" if value is None else str(value)

            lines.append(",".join([
                str(proc["pid"]),
                proc["comm"] or "",
                proc["outcome"],
                cell(cpu.get("cpu_percent_one_core")),
                cpu["status"],
                cell(rss.get("last")),
                rss["status"],
                cell(threads.get("last")),
                cell(fd_count.get("last")),
                fd_count["status"],
                cell(pss.get("last")),
                pss["status"],
            ]))
        (directory / "probe.csv").write_text("\n".join(lines) + "\n", encoding="ascii")
        meta = {
            "git_hash": doc["recording"]["git_hash"],
            "git_hash_status": doc["recording"]["git_hash_status"],
            "not_a_hardware_claim": True,
            "not_a_desktop_environment_benchmark": True,
            "cpu_basis": "one_core",
            "helper_cap_seconds": HELPER_CAP_S,
            "duration_requested_s": doc["recording"]["duration_requested_s"],
            "period_requested_ms": doc["recording"]["period_requested_ms"],
            "busy_cpu_percent_one_core": busy_cpu,
            "idle_cpu_percent_one_core": idle_cpu,
            "limits": doc["limits"],
        }
        (directory / "meta.json").write_text(json.dumps(meta, indent=2) + "\n", encoding="ascii")


if __name__ == "__main__":
    unittest.main(verbosity=1)
