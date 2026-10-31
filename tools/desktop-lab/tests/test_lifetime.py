"""Coordinator death and a stuck drain. Private temp locks only.

These tests signal the desktop-lab process with a pidfd. They do not signal a
live X server, open a live build slot, or unlink a socket.
"""

from __future__ import annotations

import fcntl
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import textwrap
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from lab.job import run_job
from lab.lifetime import SlotGuard, launcher_path, lock_release_allowed
from lab.slot import acquire_build_slot

_LAB = Path(__file__).resolve().parents[1] / "desktop-lab"

_PARENT = textwrap.dedent(
    """\
    import os, sys, time
    slot, ready, held, early, problem, grandchild = sys.argv[1:]
    pid = os.fork()
    if pid == 0:
        try:
            os.setsid()
            os.execve(
                sys.executable,
                [sys.executable, grandchild, slot, ready, held, early, problem],
                {},
            )
        except OSError as exc:
            with open(problem, "w", encoding="utf-8") as handle:
                handle.write(f"exec failed: {exc}")
            os._exit(127)
    while True:
        time.sleep(30)
    """
)

_GRANDCHILD = textwrap.dedent(
    """\
    import fcntl, os, signal, sys, time
    slot, ready, held, early, problem = sys.argv[1:]
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    signal.signal(signal.SIGHUP, signal.SIG_IGN)

    def _write(path, text):
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(text)
            handle.flush()
            os.fsync(handle.fileno())

    while True:
        try:
            fd = os.open(slot, os.O_RDWR | os.O_CLOEXEC)
        except OSError as exc:
            with open(problem, "w", encoding="utf-8") as handle:
                handle.write(str(exc))
            time.sleep(0.02)
            continue
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            if not os.path.exists(held):
                _write(held, "held")
            if not os.path.exists(ready):
                _write(ready, "ready")
        else:
            fcntl.flock(fd, fcntl.LOCK_UN)
            _write(early, "early")
        os.close(fd)
        time.sleep(0.02)
    """
)


def _write(path: Path, text: str) -> None:
    path.write_text(text)


class LifetimeTests(unittest.TestCase):
    def test_lock_release_allowed_fails_closed(self) -> None:
        self.assertFalse(lock_release_allowed(armed=True, verified_drain=False, survivors=False))
        self.assertFalse(lock_release_allowed(armed=True, verified_drain=True, survivors=True))
        self.assertFalse(lock_release_allowed(armed=True, verified_drain=False, survivors=True))
        self.assertTrue(lock_release_allowed(armed=False, verified_drain=False, survivors=False))
        self.assertTrue(lock_release_allowed(armed=True, verified_drain=True, survivors=False))

    def test_unverified_drain_keeps_the_slot(self) -> None:
        with tempfile.TemporaryDirectory(prefix="desktop-lab-stuck-") as raw:
            root = Path(raw)
            root.chmod(0o700)
            slot = root / "slot.lock"
            report: dict = {}
            try:
                with patch("lab.job.verify_drain", return_value=False):
                    report = run_job(
                        {
                            "name": "stuck-drain",
                            "backend": "none",
                            "session_bus": False,
                            "screenshot": False,
                            "timeout_s": 10,
                            "allow_after_deadline": True,
                            "slot": str(slot),
                            "command": ["/usr/bin/true"],
                        },
                        out=root / "out",
                    )
                self.assertFalse(report["job_passed"], report.get("errors"))
                self.assertFalse(report["build_slot"]["released"])
                self.assertFalse(_lock_free(slot))
                self.assertTrue(any("not verified" in item for item in report["errors"]))
                self.assertTrue(any("remains locked" in item for item in report["errors"]))
                guardian = report["build_slot"]["guardian_pid"]
                start = report["build_slot"]["guardian_start"]
                self.assertEqual(_start_of(guardian), start)
            finally:
                _stop_guardian(report.get("build_slot") or {})
                _close_matching(slot)

    def test_guardian_holds_until_the_drain_clears(self) -> None:
        self._guardian_drain(parent_dead=False)

    def test_guardian_holds_after_the_parent_dies_until_the_drain_clears(self) -> None:
        self._guardian_drain(parent_dead=True)

    def test_launcher_does_not_exec_for_a_dead_parent(self) -> None:
        with tempfile.TemporaryDirectory(prefix="desktop-lab-pdeath-test-") as raw:
            throw = subprocess.Popen(
                ["/bin/sleep", "30"],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            dead = throw.pid
            start = _start_of(dead)
            self.assertNotEqual(dead, os.getpid())
            self.assertIsInstance(start, int)
            _signal_match(dead, start, signal.SIGKILL)
            throw.wait(timeout=3)
            self.assertNotEqual(_start_of(dead), start)
            before = _exact_cmd_pids(["/bin/sleep", "30"])
            proc = subprocess.run(
                [sys.executable, str(launcher_path()), str(dead), "--", "/bin/sleep", "30"],
                cwd=raw,
                check=False,
                timeout=5,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            self.assertNotEqual(proc.returncode, 0)
            self.assertEqual(_exact_cmd_pids(["/bin/sleep", "30"]), before)

    def test_coordinator_signals_kill_the_grandchild_and_hold_the_slot(self) -> None:
        for sig in (signal.SIGTERM, signal.SIGHUP, signal.SIGKILL):
            with self.subTest(signal=sig):
                self._coordinator_death(sig)

    def test_incomplete_proof_holds_the_lock_after_coordinator_sigkill(self) -> None:
        cases = (
            ("missing", "unrecorded"),
            ("truncated", "unrecorded"),
            ("malformed", "unrecorded"),
            ("missing", "unknown"),
            ("truncated", "unknown"),
            ("malformed", "unknown"),
        )
        for drain_kind, init_kind in cases:
            with self.subTest(drain=drain_kind, init=init_kind):
                self._sigkill_incomplete_proof(drain_kind, init_kind)

    def test_complete_build_does_not_cover_an_incomplete_command(self) -> None:
        for drain_kind in ("missing", "malformed"):
            with self.subTest(drain=drain_kind):
                self._mixed_track_gap(drain_kind)

    def test_reused_output_stale_drain_keeps_the_lock(self) -> None:
        for proof in ("init-death", "new-drain"):
            with self.subTest(proof=proof):
                self._reused_output_stale_drain(proof)

    def _sigkill_incomplete_proof(self, drain_kind: str, init_kind: str) -> None:
        with tempfile.TemporaryDirectory(prefix="desktop-lab-proof-") as raw:
            root = Path(raw)
            root.chmod(0o700)
            slot = root / "slot.lock"
            out = root / "out"
            spec = {
                "name": "incomplete-proof",
                "backend": "none",
                "session_bus": False,
                "screenshot": False,
                "timeout_s": 30,
                "allow_after_deadline": True,
                "slot": str(slot),
                "command": ["/bin/sleep", "60"],
            }
            spec_path = root / "spec.json"
            spec_path.write_text(json.dumps(spec))
            stdout = open(root / "stdout.txt", "w", encoding="utf-8")
            stderr = open(root / "stderr.txt", "w", encoding="utf-8")
            proc = subprocess.Popen(
                [sys.executable, str(_LAB), "run", "--spec", str(spec_path), "--out", str(out)],
                stdin=subprocess.DEVNULL,
                stdout=stdout,
                stderr=stderr,
                start_new_session=True,
            )
            coord_start = _start_of(proc.pid)
            sleeper = None
            sleeper_start = None
            guard_pid = None
            guard_start = None
            try:
                found = self._wait_guardian(proc, out, slot)
                self.assertIsNotNone(found, self._detail(root, proc, slot))
                guard_pid, guard_start = found
                _signal_match(proc.pid, coord_start, signal.SIGSTOP)
                self.assertTrue(_wait_state(proc.pid, coord_start, {"T", "t"}, 2), self._detail(root, proc, slot))
                watch = out / "slot-guard.json"
                state = json.loads(watch.read_text(encoding="utf-8"))
                self.assertEqual(state.get("parent"), proc.pid)
                self.assertIsInstance(state.get("parent_start"), int)
                sleeper = subprocess.Popen(
                    ["/bin/sleep", "60"],
                    stdin=subprocess.DEVNULL,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                    start_new_session=True,
                )
                sleeper_start = _start_of(sleeper.pid)
                self.assertIsInstance(sleeper_start, int)
                drain = root / "bad-drain.json"
                if drain_kind == "truncated":
                    drain.write_text('{"namespace_empty": tru', encoding="utf-8")
                elif drain_kind == "malformed":
                    drain.write_text("{", encoding="utf-8")
                state["release"] = False
                state["hosts"] = []
                state["drains"] = [str(drain)]
                if init_kind == "unrecorded":
                    state["inits"] = []
                    command_track = {
                        "id": "command",
                        "drain": str(drain),
                        "pending": True,
                        "host": None,
                        "inits": [],
                    }
                else:
                    state["inits"] = [{"pid": sleeper.pid}]
                    command_track = {
                        "id": "command",
                        "drain": str(drain),
                        "pending": False,
                        "host": None,
                        "inits": [{"pid": sleeper.pid}],
                    }
                state["tracks"] = [command_track]
                _atomic_json(watch, state)
                if drain.exists():
                    body = drain.read_text(encoding="utf-8")
                    self.assertNotIn('"namespace_empty": false', body)
                    self.assertNotIn('"namespace_empty":false', body)
                else:
                    self.assertEqual(drain_kind, "missing")
                _signal_match(proc.pid, coord_start, signal.SIGKILL)
                deadline = time.monotonic() + 1.0
                while time.monotonic() < deadline:
                    released = (out / "slot-guard.ack").is_file() or _lock_free(slot)
                    if released or sleeper.poll() is not None or _start_of(guard_pid) != guard_start:
                        self.fail(self._detail(root, proc, slot))
                    self.assertEqual(_start_of(sleeper.pid), sleeper_start)
                    time.sleep(0.05)
                self.assertIsNone(sleeper.poll())
                self.assertFalse(_lock_free(slot))
                self.assertFalse((out / "slot-guard.ack").is_file())
                self.assertEqual(_start_of(guard_pid), guard_start)
            finally:
                if proc.poll() is None:
                    _signal_match(proc.pid, coord_start, signal.SIGKILL)
                if guard_pid is not None:
                    _signal_match(guard_pid, guard_start, signal.SIGKILL)
                if sleeper is not None and sleeper.poll() is None:
                    _signal_match(sleeper.pid, sleeper_start, signal.SIGKILL)
                    sleeper.wait(timeout=3)
                _kill_cmdline(str(root).encode())
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    pass
                stdout.close()
                stderr.close()

    def _mixed_track_gap(self, drain_kind: str) -> None:
        with tempfile.TemporaryDirectory(prefix="desktop-lab-tracks-") as raw:
            root = Path(raw)
            root.chmod(0o700)
            slot = root / "slot.lock"
            out = root / "out"
            spec = {
                "name": "mixed-track",
                "backend": "none",
                "session_bus": False,
                "screenshot": False,
                "timeout_s": 30,
                "allow_after_deadline": True,
                "slot": str(slot),
                "build": {"argv": ["/bin/sleep", "0.6"], "timeout_s": 10},
                "command": ["/bin/sleep", "60"],
            }
            spec_path = root / "spec.json"
            spec_path.write_text(json.dumps(spec))
            stdout = open(root / "stdout.txt", "w", encoding="utf-8")
            stderr = open(root / "stderr.txt", "w", encoding="utf-8")
            proc = subprocess.Popen(
                [sys.executable, str(_LAB), "run", "--spec", str(spec_path), "--out", str(out)],
                stdin=subprocess.DEVNULL,
                stdout=stdout,
                stderr=stderr,
                start_new_session=True,
            )
            coord_start = _start_of(proc.pid)
            sleeper = None
            sleeper_start = None
            guard_pid = None
            guard_start = None
            try:
                seen_pending = False
                samples: list[dict] = []
                ready = None
                deadline = time.monotonic() + 15
                while time.monotonic() < deadline:
                    if proc.poll() is not None:
                        break
                    found = self._guardian_of(proc.pid, out)
                    watch = out / "slot-guard.json"
                    state = _read_json(watch)
                    if isinstance(state, dict):
                        tracks = [item for item in state.get("tracks") or [] if isinstance(item, dict)]
                        by_id = {item.get("id"): item for item in tracks}
                        command = by_id.get("command")
                        build = by_id.get("build")
                        if isinstance(command, dict):
                            if len(samples) < 8:
                                samples.append({
                                    "pending": command.get("pending"),
                                    "host": command.get("host"),
                                    "inits": command.get("inits"),
                                })
                            if command.get("pending") is True and not (command.get("inits") or []):
                                seen_pending = True
                        if (
                            seen_pending
                            and found is not None
                            and isinstance(build, dict)
                            and _known_dead_track(build)
                            and isinstance(command, dict)
                            and not _lock_free(slot)
                        ):
                            ready = (found, json.loads(json.dumps(build)))
                            break
                    time.sleep(0.002)
                self.assertIsNotNone(
                    ready,
                    f"pending={seen_pending} samples={samples}\n" + self._detail(root, proc, slot),
                )
                (guard_pid, guard_start), build = ready
                _signal_match(proc.pid, coord_start, signal.SIGSTOP)
                self.assertTrue(_wait_state(proc.pid, coord_start, {"T", "t"}, 2), self._detail(root, proc, slot))
                watch = out / "slot-guard.json"
                stopped = _read_json(watch)
                self.assertIsInstance(stopped, dict)
                stopped_build = next(
                    (item for item in stopped.get("tracks") or [] if isinstance(item, dict) and item.get("id") == "build"),
                    None,
                )
                self.assertTrue(_known_dead_track(stopped_build), self._detail(root, proc, slot))
                build = json.loads(json.dumps(stopped_build))
                sleeper = subprocess.Popen(
                    ["/bin/sleep", "60"],
                    stdin=subprocess.DEVNULL,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                    start_new_session=True,
                )
                sleeper_start = _start_of(sleeper.pid)
                self.assertIsInstance(sleeper_start, int)
                drain = root / "bad-drain.json"
                if drain_kind == "malformed":
                    drain.write_text("{", encoding="utf-8")
                else:
                    self.assertEqual(drain_kind, "missing")
                    self.assertFalse(drain.exists())
                stopped["release"] = False
                stopped["hosts"] = [build["host"]]
                stopped["inits"] = list(build["inits"])
                stopped["drains"] = [str(drain)]
                stopped["tracks"] = [
                    build,
                    {
                        "id": "command",
                        "drain": str(drain),
                        "pending": True,
                        "host": None,
                        "inits": [],
                    },
                ]
                _atomic_json(watch, stopped)
                if drain.exists():
                    body = drain.read_text(encoding="utf-8")
                    self.assertNotIn('"namespace_empty": false', body)
                    self.assertNotIn('"namespace_empty":false', body)
                _signal_match(proc.pid, coord_start, signal.SIGKILL)
                deadline = time.monotonic() + 1.0
                while time.monotonic() < deadline:
                    released = (out / "slot-guard.ack").is_file() or _lock_free(slot)
                    if released or sleeper.poll() is not None or _start_of(guard_pid) != guard_start:
                        self.fail(self._detail(root, proc, slot))
                    self.assertEqual(_start_of(sleeper.pid), sleeper_start)
                    for row in build["inits"]:
                        self.assertTrue(_identity_gone(row["pid"], row["start"]))
                    time.sleep(0.05)
                self.assertIsNone(sleeper.poll())
                self.assertFalse(_lock_free(slot))
                self.assertFalse((out / "slot-guard.ack").is_file())
                self.assertEqual(_start_of(guard_pid), guard_start)
                _signal_match(sleeper.pid, sleeper_start, signal.SIGKILL)
                sleeper.wait(timeout=3)
                self.assertTrue(_identity_gone(sleeper.pid, sleeper_start))
                fresh = _read_json(watch)
                self.assertIsInstance(fresh, dict)
                replaced = False
                tracks = []
                for track in fresh.get("tracks") or []:
                    if isinstance(track, dict) and track.get("id") == "command":
                        tracks.append({
                            "id": "command",
                            "drain": str(drain),
                            "pending": False,
                            "host": {"pid": sleeper.pid, "start": sleeper_start},
                            "inits": [{"pid": sleeper.pid, "start": sleeper_start}],
                        })
                        replaced = True
                    else:
                        tracks.append(track)
                self.assertTrue(replaced, self._detail(root, proc, slot))
                fresh["tracks"] = tracks
                fresh["release"] = False
                _atomic_json(watch, fresh)
                self.assertTrue(_wait_file(out / "slot-guard.ack", 3), self._detail(root, proc, slot))
                self.assertTrue(_wait_free(slot, 3), self._detail(root, proc, slot))
                self.assertTrue(_wait_dead(guard_pid, guard_start, 3), self._detail(root, proc, slot))
                sleeper = None
            finally:
                if proc.poll() is None:
                    _signal_match(proc.pid, coord_start, signal.SIGKILL)
                if guard_pid is not None and _start_of(guard_pid) == guard_start:
                    _signal_match(guard_pid, guard_start, signal.SIGKILL)
                if sleeper is not None and sleeper.poll() is None:
                    _signal_match(sleeper.pid, sleeper_start, signal.SIGKILL)
                    sleeper.wait(timeout=3)
                _kill_cmdline(str(root).encode())
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    pass
                stdout.close()
                stderr.close()

    def _reused_output_stale_drain(self, proof: str) -> None:
        with tempfile.TemporaryDirectory(prefix="desktop-lab-reuse-") as raw:
            root = Path(raw)
            root.chmod(0o700)
            slot = root / "slot.lock"
            out = root / "out"
            out.mkdir(mode=0o700)
            prior = '{"namespace_empty": true}\n'
            old_command = out / "containment-command.json"
            old_build = out / "containment-build.json"
            old_command.write_text(prior, encoding="utf-8")
            old_build.write_text(prior, encoding="utf-8")
            spec = {
                "name": "reused-out",
                "backend": "none",
                "session_bus": False,
                "screenshot": False,
                "timeout_s": 30,
                "allow_after_deadline": True,
                "slot": str(slot),
                "build": {"argv": ["/bin/sleep", "0.6"], "timeout_s": 10},
                "command": ["/bin/sleep", "60"],
            }
            spec_path = root / "spec.json"
            spec_path.write_text(json.dumps(spec))
            stdout = open(root / "stdout.txt", "w", encoding="utf-8")
            stderr = open(root / "stderr.txt", "w", encoding="utf-8")
            proc = subprocess.Popen(
                [sys.executable, str(_LAB), "run", "--spec", str(spec_path), "--out", str(out)],
                stdin=subprocess.DEVNULL,
                stdout=stdout,
                stderr=stderr,
                start_new_session=True,
            )
            coord_start = _start_of(proc.pid)
            sleeper = None
            sleeper_start = None
            guard_pid = None
            guard_start = None
            try:
                seen_pending = False
                ready = None
                deadline = time.monotonic() + 15
                while time.monotonic() < deadline:
                    if proc.poll() is not None:
                        break
                    found = self._guardian_of(proc.pid, out)
                    state = _read_json(out / "slot-guard.json")
                    if isinstance(state, dict):
                        tracks = [item for item in state.get("tracks") or [] if isinstance(item, dict)]
                        by_id = {item.get("id"): item for item in tracks}
                        command = by_id.get("command")
                        build = by_id.get("build")
                        if isinstance(command, dict) and _pending_unique_drain(command, out, old_command):
                            seen_pending = True
                        if (
                            seen_pending
                            and found is not None
                            and isinstance(build, dict)
                            and isinstance(command, dict)
                            and _known_dead_track(build)
                            and _same_invocation(build, command)
                            and not _lock_free(slot)
                        ):
                            ready = (found, json.loads(json.dumps(command)))
                            break
                    time.sleep(0.002)
                self.assertIsNotNone(ready, f"pending={seen_pending}\n" + self._detail(root, proc, slot))
                (guard_pid, guard_start), command = ready
                _signal_match(proc.pid, coord_start, signal.SIGSTOP)
                self.assertTrue(_wait_state(proc.pid, coord_start, {"T", "t"}, 2), self._detail(root, proc, slot))
                watch = out / "slot-guard.json"
                stopped = _read_json(watch)
                self.assertIsInstance(stopped, dict)
                stopped_tracks = [item for item in stopped.get("tracks") or [] if isinstance(item, dict)]
                by_id = {item.get("id"): item for item in stopped_tracks}
                build = by_id.get("build")
                current = by_id.get("command")
                self.assertTrue(_known_dead_track(build), self._detail(root, proc, slot))
                self.assertTrue(_same_invocation(build, current), self._detail(root, proc, slot))
                invocation = current["invocation"]
                drain_path = Path(current["drain"])
                self.assertEqual(drain_path, out / "invocations" / invocation / "containment-command.json")
                self.assertNotEqual(drain_path, old_command)
                self.assertNotEqual(Path(build["drain"]), old_build)
                build = json.loads(json.dumps(build))
                command_track = {
                    "id": "command",
                    "drain": str(drain_path),
                    "invocation": invocation,
                    "pending": True,
                    "host": None,
                    "inits": [],
                }
                sleeper = subprocess.Popen(
                    ["/bin/sleep", "60"],
                    stdin=subprocess.DEVNULL,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                    start_new_session=True,
                )
                sleeper_start = _start_of(sleeper.pid)
                self.assertIsInstance(sleeper_start, int)
                stale = '{"namespace_empty": true}\n'
                drain_path.write_text(stale, encoding="utf-8")
                stopped["release"] = False
                stopped["hosts"] = [build["host"]]
                stopped["inits"] = list(build["inits"])
                stopped["drains"] = [str(old_command), str(drain_path)]
                stopped["tracks"] = [build, command_track]
                _atomic_json(watch, stopped)
                self.assertEqual(old_command.read_text(encoding="utf-8"), prior)
                self.assertEqual(old_build.read_text(encoding="utf-8"), prior)
                self.assertNotIn("invocation", drain_path.read_text(encoding="utf-8"))
                _signal_match(proc.pid, coord_start, signal.SIGKILL)
                proc.wait(timeout=5)
                self.assertIsNotNone(proc.poll())
                deadline = time.monotonic() + 1.0
                while time.monotonic() < deadline:
                    released = (out / "slot-guard.ack").is_file() or _lock_free(slot)
                    if released or sleeper.poll() is not None or _start_of(guard_pid) != guard_start:
                        self.fail(self._detail(root, proc, slot))
                    self.assertEqual(_start_of(sleeper.pid), sleeper_start)
                    self.assertEqual(old_command.read_text(encoding="utf-8"), prior)
                    self.assertNotIn("invocation", drain_path.read_text(encoding="utf-8"))
                    time.sleep(0.05)
                self.assertFalse(_lock_free(slot))
                self.assertFalse((out / "slot-guard.ack").is_file())
                if proof == "init-death":
                    _signal_match(sleeper.pid, sleeper_start, signal.SIGKILL)
                    sleeper.wait(timeout=3)
                    self.assertTrue(_identity_gone(sleeper.pid, sleeper_start))
                    fresh = _read_json(watch)
                    self.assertIsInstance(fresh, dict)
                    tracks = []
                    replaced = False
                    for track in fresh.get("tracks") or []:
                        if isinstance(track, dict) and track.get("id") == "command":
                            tracks.append({
                                "id": "command",
                                "drain": str(drain_path),
                                "invocation": invocation,
                                "pending": False,
                                "host": {"pid": sleeper.pid, "start": sleeper_start},
                                "inits": [{"pid": sleeper.pid, "start": sleeper_start}],
                            })
                            replaced = True
                        else:
                            tracks.append(track)
                    self.assertTrue(replaced, self._detail(root, proc, slot))
                    fresh["tracks"] = tracks
                    fresh["release"] = False
                    _atomic_json(watch, fresh)
                    sleeper = None
                elif proof == "new-drain":
                    _atomic_json(drain_path, {"namespace_empty": True, "invocation": invocation})
                else:
                    self.fail(proof)
                self.assertTrue(_wait_file(out / "slot-guard.ack", 3), self._detail(root, proc, slot))
                self.assertTrue(_wait_free(slot, 3), self._detail(root, proc, slot))
                self.assertTrue(_wait_dead(guard_pid, guard_start, 3), self._detail(root, proc, slot))
                self.assertEqual(old_command.read_text(encoding="utf-8"), prior)
                self.assertEqual(old_build.read_text(encoding="utf-8"), prior)
                if proof == "new-drain":
                    self.assertIsNone(sleeper.poll())
                    self.assertEqual(drain_path.read_text(encoding="utf-8").count(invocation), 1)
                else:
                    self.assertNotIn("invocation", drain_path.read_text(encoding="utf-8"))
            finally:
                if proc.poll() is None:
                    _signal_match(proc.pid, coord_start, signal.SIGKILL)
                if guard_pid is not None and _start_of(guard_pid) == guard_start:
                    _signal_match(guard_pid, guard_start, signal.SIGKILL)
                if sleeper is not None and sleeper.poll() is None:
                    _signal_match(sleeper.pid, sleeper_start, signal.SIGKILL)
                    sleeper.wait(timeout=3)
                _kill_cmdline(str(root).encode())
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    pass
                stdout.close()
                stderr.close()

    def _wait_guardian(self, proc: subprocess.Popen, out: Path, slot: Path):
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if proc.poll() is not None:
                return None
            found = self._guardian_of(proc.pid, out)
            if found is not None and not _lock_free(slot):
                return found
            time.sleep(0.02)
        return None

    def _guardian_of(self, root_pid: int, out: Path):
        marker = str(out).encode()
        for pid, start in _descendants(root_pid):
            command = _cmdline(pid)
            if b"slot-guardian.py" in command and marker in command:
                return pid, start
        return None

    def _guardian_drain(self, *, parent_dead: bool) -> None:
        with tempfile.TemporaryDirectory(prefix="desktop-lab-guard-") as raw:
            root = Path(raw)
            root.chmod(0o700)
            slot = root / "slot.lock"
            fd = acquire_build_slot(slot)
            guard = SlotGuard()
            sleeper = None
            sleeper_start = None
            throw = None
            try:
                guard.start(fd, root / "out")
                drain = root / "drain.json"
                _atomic_json(drain, {"namespace_empty": False})
                guard.state["drains"] = [str(drain)]
                guard.state["release"] = not parent_dead
                if parent_dead:
                    sleeper = subprocess.Popen(
                        ["/bin/sleep", "60"],
                        stdin=subprocess.DEVNULL,
                        stdout=subprocess.DEVNULL,
                        stderr=subprocess.DEVNULL,
                    )
                    sleeper_start = _start_of(sleeper.pid)
                    throw = subprocess.Popen(
                        ["/bin/sleep", "30"],
                        stdin=subprocess.DEVNULL,
                        stdout=subprocess.DEVNULL,
                        stderr=subprocess.DEVNULL,
                    )
                    dead = throw.pid
                    dead_start = _start_of(throw.pid)
                    _signal_match(dead, dead_start, signal.SIGKILL)
                    throw.wait(timeout=3)
                    throw = None
                    self.assertNotEqual(_start_of(dead), dead_start)
                    guard.state["parent"] = dead
                    guard.state["parent_start"] = dead_start
                    guard.state["hosts"] = [{"pid": sleeper.pid, "start": sleeper_start}]
                    guard.state["release"] = False
                guard.publish()
                if sleeper is not None:
                    deadline = time.monotonic() + 3
                    while time.monotonic() < deadline and sleeper.poll() is None:
                        time.sleep(0.02)
                    watch = (root / "out" / "slot-guard.json").read_text(encoding="utf-8")
                    self.assertIsNotNone(
                        sleeper.poll(),
                        f"guardian left the recorded sleeper alive poll={guard.proc.poll()} watch={watch}",
                    )
                    sleeper = None
                time.sleep(0.4)
                self.assertFalse(guard.ack.is_file())
                self.assertFalse(_lock_free(slot))
                _atomic_json(drain, {"namespace_empty": True})
                self.assertTrue(_wait_file(guard.ack, 3), "guardian did not release after the drain cleared")
                self.assertFalse(_lock_free(slot))
                os.close(fd)
                fd = -1
                self.assertTrue(_wait_free(slot, 2))
                try:
                    guard.proc.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    self.fail("guardian did not exit after releasing the slot")
            finally:
                if sleeper is not None and sleeper.poll() is None:
                    _signal_match(sleeper.pid, sleeper_start, signal.SIGKILL)
                    sleeper.wait(timeout=3)
                if throw is not None and throw.poll() is None:
                    throw.kill()
                    throw.wait(timeout=3)
                if guard.proc is not None and guard.proc.poll() is None:
                    _signal_match(guard.pid, guard.starttime, signal.SIGKILL)
                    guard.proc.wait(timeout=3)
                if fd >= 0:
                    os.close(fd)

    def _coordinator_death(self, sig: int) -> None:
        with tempfile.TemporaryDirectory(prefix="desktop-lab-life-") as raw:
            root = Path(raw)
            root.chmod(0o700)
            slot = root / "slot.lock"
            ready = root / "ready"
            held = root / "held"
            early = root / "early"
            problem = root / "problem"
            parent = root / "parent.py"
            grandchild = root / "grandchild.py"
            parent.write_text(_PARENT)
            grandchild.write_text(_GRANDCHILD)
            spec = {
                "name": "coord-death",
                "backend": "none",
                "session_bus": False,
                "screenshot": False,
                "timeout_s": 30,
                "allow_after_deadline": True,
                "slot": str(slot),
                "command": [
                    sys.executable,
                    str(parent),
                    str(slot),
                    str(ready),
                    str(held),
                    str(early),
                    str(problem),
                    str(grandchild),
                ],
            }
            spec_path = root / "spec.json"
            spec_path.write_text(json.dumps(spec))
            out = root / "out"
            stdout = open(root / "stdout.txt", "w", encoding="utf-8")
            stderr = open(root / "stderr.txt", "w", encoding="utf-8")
            proc = subprocess.Popen(
                [sys.executable, str(_LAB), "run", "--spec", str(spec_path), "--out", str(out)],
                stdin=subprocess.DEVNULL,
                stdout=stdout,
                stderr=stderr,
                start_new_session=True,
            )
            coord_start = _start_of(proc.pid)
            try:
                found = self._wait_cleared(proc, root, grandchild)
                self.assertIsNotNone(found, self._detail(root, proc, slot))
                cleared, cleared_start = found
                self.assertEqual(Path(f"/proc/{cleared}/environ").read_bytes(), b"")
                self.assertEqual(_session(cleared), cleared)
                self.assertEqual(_argv1(cleared), str(grandchild).encode())
                parent_pid = self._find_argv(proc.pid, parent)
                self.assertIsNotNone(parent_pid, self._detail(root, proc, slot))
                self.assertFalse(early.exists(), self._detail(root, proc, slot))
                self.assertTrue(held.is_file())
                self.assertFalse(_lock_free(slot))
                self.assertTrue(self._non_guardian(proc.pid, out))
                observed = True
                time.sleep(0.5)
                self.assertEqual(_start_of(proc.pid), coord_start)
                self.assertEqual(_start_of(cleared), cleared_start)
                self.assertFalse(_lock_free(slot))
                _signal_match(proc.pid, coord_start, sig)
                deadline = time.monotonic() + 20
                while time.monotonic() < deadline:
                    if early.exists():
                        self.fail("lock became free while the grandchild was still probing\n" + self._detail(root, proc, slot))
                    others = self._non_guardian(proc.pid, out)
                    if others:
                        self.assertFalse(_lock_free(slot), self._detail(root, proc, slot))
                        observed = True
                    if proc.poll() is not None and not others:
                        break
                    time.sleep(0.02)
                self.assertTrue(observed or not _lock_free(slot) or proc.poll() is not None)
                self.assertTrue(observed, self._detail(root, proc, slot))
                self.assertIsNotNone(proc.poll(), self._detail(root, proc, slot))
                self.assertFalse(self._non_guardian(proc.pid, out), self._detail(root, proc, slot))
                self.assertNotEqual(_start_of(cleared), cleared_start)
                self.assertFalse(early.exists(), self._detail(root, proc, slot))
                self.assertTrue(held.is_file())
                self.assertTrue(_wait_free(slot, 5), self._detail(root, proc, slot))
                if sig != signal.SIGKILL:
                    report = json.loads((out / "report.json").read_text())
                    self.assertEqual(report.get("coordinator_signal"), sig)
                    self.assertFalse(report["job_passed"])
                    self.assertFalse(report["full_acceptance"])
            finally:
                if proc.poll() is None:
                    _signal_match(proc.pid, coord_start, signal.SIGKILL)
                _kill_cmdline(str(root).encode())
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    pass
                stdout.close()
                stderr.close()

    def _wait_cleared(self, proc: subprocess.Popen, root: Path, grandchild: Path):
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            if proc.poll() is not None:
                return None
            if (root / "ready").is_file() and (root / "held").is_file():
                found = self._find_argv(proc.pid, grandchild)
                if found is not None:
                    pid, start = found
                    try:
                        environ = Path(f"/proc/{pid}/environ").read_bytes()
                    except OSError:
                        environ = b"?"
                    if environ == b"" and _session(pid) == pid:
                        return pid, start
            time.sleep(0.05)
        return None

    def _find_argv(self, root_pid: int, script: Path):
        wanted = str(script).encode()
        for pid, start in _descendants(root_pid):
            if _argv1(pid) == wanted and _start_of(pid) == start:
                return pid, start
        return None

    def _non_guardian(self, root_pid: int, out: Path) -> list[int]:
        marker = str(out).encode()
        found = []
        for pid, start in _descendants(root_pid):
            if _start_of(pid) != start:
                continue
            command = _cmdline(pid)
            if b"slot-guardian.py" in command and marker in command:
                continue
            found.append(pid)
        return found

    def _detail(self, root: Path, proc: subprocess.Popen, slot: Path) -> str:
        chunks = [f"returncode={proc.poll()}"]
        try:
            chunks.append(f"lock_free={_lock_free(slot)}")
        except OSError as exc:
            chunks.append(f"lock_free_error={exc}")
        for name in ("stderr.txt", "stdout.txt", "problem"):
            path = root / name
            if path.is_file():
                chunks.append(f"{name}: {path.read_text(encoding='utf-8', errors='replace')[-1500:]}")
        out = root / "out"
        if out.is_dir():
            for path in sorted(out.rglob("*")):
                if not path.is_file():
                    continue
                if path.suffix not in {".json", ".log", ".ack"} and "guard" not in path.name:
                    continue
                try:
                    chunks.append(f"{path.name}: {path.read_text(encoding='utf-8', errors='replace')[:800]}")
                except OSError:
                    continue
        return "\n".join(chunks)[-4000:]


def _read_json(path: Path) -> dict | None:
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError, UnicodeError):
        return None
    if not isinstance(payload, dict):
        return None
    return payload


def _row_known(row: object) -> bool:
    return isinstance(row, dict) and isinstance(row.get("pid"), int) and isinstance(row.get("start"), int)


def _identity_gone(pid: int, start: int) -> bool:
    if _start_of(pid) != start:
        return True
    return _proc_state(pid) in {"Z", "X"}


def _pending_unique_drain(track: dict, out: Path, old_command: Path) -> bool:
    if track.get("pending") is not True or track.get("inits"):
        return False
    invocation = track.get("invocation")
    drain = track.get("drain")
    if not _nonce(invocation) or not isinstance(drain, str):
        return False
    path = Path(drain)
    return path == out / "invocations" / invocation / "containment-command.json" and path != old_command


def _same_invocation(build: object, command: object) -> bool:
    if not isinstance(build, dict) or not isinstance(command, dict):
        return False
    invocation = command.get("invocation")
    if not _nonce(invocation) or build.get("invocation") != invocation:
        return False
    build_drain = build.get("drain")
    return isinstance(build_drain, str) and Path(build_drain) == Path(command["drain"]).parent / "containment-build.json"


def _nonce(value: object) -> bool:
    return isinstance(value, str) and len(value) == 32 and all(ch in "0123456789abcdef" for ch in value)


def _known_dead_track(track: object) -> bool:
    if not isinstance(track, dict) or track.get("pending") is not False:
        return False
    if not _row_known(track.get("host")):
        return False
    inits = track.get("inits")
    if not isinstance(inits, list) or not inits:
        return False
    for row in inits:
        if not _row_known(row) or not _identity_gone(row["pid"], row["start"]):
            return False
    return True


def _start_of(pid: int) -> int | None:
    try:
        stat_text = Path(f"/proc/{pid}/stat").read_text()
        rest = stat_text[stat_text.rfind(")") + 2 :].split()
        return int(rest[19])
    except (OSError, IndexError, ValueError):
        return None


def _session(pid: int) -> int | None:
    try:
        stat_text = Path(f"/proc/{pid}/stat").read_text()
        rest = stat_text[stat_text.rfind(")") + 2 :].split()
        return int(rest[3])
    except (OSError, IndexError, ValueError):
        return None


def _cmdline(pid: int) -> bytes:
    try:
        return Path(f"/proc/{pid}/cmdline").read_bytes()
    except OSError:
        return b""


def _argv1(pid: int) -> bytes:
    parts = _cmdline(pid).split(b"\0")
    if len(parts) < 2:
        return b""
    return parts[1]


def _descendants(root_pid: int) -> list[tuple[int, int]]:
    table: dict[int, tuple[int, int, str]] = {}
    proc = Path("/proc")
    for entry in proc.iterdir():
        if not entry.name.isdigit():
            continue
        pid = int(entry.name)
        try:
            stat_text = (entry / "stat").read_text()
            rest = stat_text[stat_text.rfind(")") + 2 :].split()
            table[pid] = (int(rest[1]), int(rest[19]), rest[0])
        except (OSError, IndexError, ValueError):
            continue
    children: dict[int, list[int]] = {}
    for pid, (ppid, _start, _state) in table.items():
        children.setdefault(ppid, []).append(pid)
    found = []
    stack = list(children.get(root_pid, []))
    seen = {root_pid}
    while stack:
        pid = stack.pop()
        if pid in seen or pid not in table:
            continue
        seen.add(pid)
        _ppid, start, state = table[pid]
        if state not in {"Z", "X"}:
            found.append((pid, start))
        stack.extend(children.get(pid, []))
    return found


def _signal_match(pid: int, start: int | None, sig: int) -> None:
    if not isinstance(pid, int) or not isinstance(start, int) or _start_of(pid) != start:
        return
    fd = None
    try:
        fd = os.pidfd_open(pid)
        if _start_of(pid) != start:
            return
        signal.pidfd_send_signal(fd, sig)
    except OSError:
        return
    finally:
        if fd is not None:
            os.close(fd)


def _kill_cmdline(marker: bytes) -> None:
    me = os.getpid()
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        pid = int(entry.name)
        if pid <= 1 or pid == me:
            continue
        try:
            command = (entry / "cmdline").read_bytes()
        except OSError:
            continue
        if marker not in command:
            continue
        _signal_match(pid, _start_of(pid), signal.SIGKILL)


def _lock_free(path: Path) -> bool:
    fd = os.open(path, os.O_RDWR | os.O_CLOEXEC)
    os.set_inheritable(fd, False)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        os.close(fd)
        return False
    fcntl.flock(fd, fcntl.LOCK_UN)
    os.close(fd)
    return True


def _wait_free(path: Path, seconds: float) -> bool:
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if _lock_free(path):
            return True
        time.sleep(0.02)
    return False


def _wait_file(path: Path, seconds: float) -> bool:
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if path.is_file():
            return True
        time.sleep(0.02)
    return False


def _proc_state(pid: int) -> str:
    try:
        text = Path(f"/proc/{pid}/status").read_text(encoding="utf-8")
    except OSError:
        return ""
    for line in text.splitlines():
        if line.startswith("State:"):
            parts = line.split()
            return parts[1] if len(parts) > 1 else ""
    return ""


def _wait_state(pid: int, start: int | None, wanted: set[str], seconds: float) -> bool:
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if _start_of(pid) != start:
            return False
        if _proc_state(pid) in wanted:
            return True
        time.sleep(0.02)
    return _start_of(pid) == start and _proc_state(pid) in wanted


def _wait_dead(pid: int, start: int | None, seconds: float) -> bool:
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if _start_of(pid) != start:
            return True
        time.sleep(0.02)
    return _start_of(pid) != start


def _close_matching(path: Path) -> None:
    target = os.fsencode(path)
    numbers = []
    for number in range(3, 256):
        try:
            link = os.readlink(f"/proc/self/fd/{number}")
        except OSError:
            continue
        if os.fsencode(link) == target:
            numbers.append(number)
    for number in numbers:
        os.close(number)


def _stop_guardian(slot_info: dict) -> None:
    pid = slot_info.get("guardian_pid")
    start = slot_info.get("guardian_start")
    if isinstance(pid, int) and isinstance(start, int):
        _signal_match(pid, start, signal.SIGKILL)
        _wait_dead(pid, start, 2)


def _exact_cmd_pids(argv: list[str]) -> set[int]:
    needle = b"\0".join(part.encode() for part in argv)
    found = set()
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            command = (entry / "cmdline").read_bytes().rstrip(b"\0")
        except OSError:
            continue
        if command == needle:
            found.add(int(entry.name))
    return found


def _atomic_json(path: Path, payload: dict) -> None:
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(payload))
    os.replace(temporary, path)


if __name__ == "__main__":
    unittest.main()
