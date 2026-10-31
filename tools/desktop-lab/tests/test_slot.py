"""Build-slot lock fd must not survive a double-fork daemon."""

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

from lab.display import XvfbSlot
from lab.job import run_job
from lab.policy import LIVE_SLOT_PATHS, PolicyError, check_argv
from lab.slot import acquire_build_slot, equivalent_argv, incident_record, release_build_slot

_DAEMON = textwrap.dedent(
    """\
    import json, os, sys, time
    result = sys.argv[1]
    read_fd, write_fd = os.pipe()
    pid = os.fork()
    if pid == 0:
        os.close(read_fd)
        os.setsid()
        if os.fork() > 0:
            os._exit(0)
        fds = []
        for name in os.listdir("/proc/self/fd"):
            link = f"/proc/self/fd/{name}"
            try:
                target = os.readlink(link)
            except OSError:
                continue
            fds.append({"fd": int(name), "target": target})
        with open(result, "w", encoding="utf-8") as handle:
            json.dump({"pid": os.getpid(), "fds": fds}, handle)
            handle.flush()
            os.fsync(handle.fileno())
        os.write(write_fd, b"1")
        os.close(write_fd)
        time.sleep(30)
        os._exit(0)
    os.close(write_fd)
    os.read(read_fd, 1)
    os.close(read_fd)
    os._exit(0)
    """
)


def _kill_script(script: Path) -> None:
    marker = str(script).encode()
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        pid = int(entry.name)
        if pid <= 1 or pid == os.getpid():
            continue
        try:
            command = (entry / "cmdline").read_bytes()
        except OSError:
            continue
        if marker not in command:
            continue
        fd = None
        try:
            fd = os.pidfd_open(pid)
            signal.pidfd_send_signal(fd, signal.SIGKILL)
        except OSError:
            continue
        finally:
            if fd is not None:
                os.close(fd)


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


def _targets(payload: dict) -> list[str]:
    return [item["target"] for item in payload["fds"]]


_LOCK_DAEMON = textwrap.dedent(
    """\
    import fcntl, os, signal, sys, time
    slot, started, marker = sys.argv[1:]
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    if os.fork() == 0:
        os.setsid()
        if os.fork() > 0:
            os._exit(0)
        open(started, "w").write("up")
        fd = os.open(slot, os.O_RDWR | os.O_CLOEXEC)
        while True:
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                time.sleep(0.02)
                continue
            open(marker, "w").write("acquired")
            os._exit(0)
    deadline = time.time() + 5
    while not os.path.exists(started):
        if time.time() > deadline:
            os._exit(1)
        time.sleep(0.02)
    os._exit(0)
    """
)


def _file_identity(path: Path) -> tuple:
    info = path.lstat()
    return (info.st_ino, info.st_size, info.st_mtime_ns, path.read_bytes())


def _script_running(script: Path) -> bool:
    marker = str(script).encode()
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            command = (entry / "cmdline").read_bytes()
        except OSError:
            continue
        if marker in command:
            return True
    return False


def _sibling() -> subprocess.Popen:
    return subprocess.Popen(
        [sys.executable, "-c", "import time; time.sleep(60)"],
        env={"PATH": "/usr/bin:/bin"},
        stdin=subprocess.DEVNULL,
        start_new_session=True,
    )


def _stop_sibling(proc: subprocess.Popen) -> None:
    if proc.poll() is None:
        os.killpg(proc.pid, signal.SIGKILL)
        proc.wait(timeout=2)


def _alive(pid: int) -> bool:
    try:
        text = Path(f"/proc/{pid}/status").read_text()
    except OSError:
        return False
    for line in text.splitlines():
        if line.startswith("State:"):
            return line.split()[1] not in {"Z", "X"}
    return False


class SlotTests(unittest.TestCase):
    def test_equivalent_argv_uses_close(self) -> None:
        slot = Path("/tmp/desktop-lab-example.lock")
        argv = equivalent_argv(slot, ["/usr/bin/true"])
        self.assertEqual(argv, ["flock", "--close", str(slot), "nice", "-n", "10", "--", "/usr/bin/true"])

    def test_live_slots_are_refused_before_open(self) -> None:
        for path in LIVE_SLOT_PATHS:
            with patch("lab.slot.os.open", side_effect=AssertionError("opened a live slot")):
                with self.assertRaises(PolicyError):
                    acquire_build_slot(Path(path))
            with self.assertRaises(PolicyError):
                check_argv(["flock", "--close", path, "/usr/bin/true"])
            with self.assertRaises(PolicyError):
                check_argv(["flock", path, "/usr/bin/true"])

    def test_plain_flock_command_is_refused(self) -> None:
        with self.assertRaises(PolicyError):
            check_argv(["flock", "/tmp/desktop-lab-example.lock", "/usr/bin/true"])

    def test_python_wrapper_does_not_leak_into_a_daemon(self) -> None:
        self._daemon_does_not_hold(wrapper="python")

    def test_flock_close_does_not_leak_into_a_daemon(self) -> None:
        self._daemon_does_not_hold(wrapper="flock")

    def test_plain_flock_leaks_until_the_fake_daemon_is_killed(self) -> None:
        with tempfile.TemporaryDirectory(prefix="desktop-lab-slot-") as raw:
            root = Path(raw)
            slot = root / "slot.lock"
            script = root / "fake-daemon.py"
            result = root / "daemon.json"
            script.write_text(_DAEMON)
            try:
                proc = subprocess.run(
                    ["/usr/bin/flock", str(slot), sys.executable, str(script), str(result)],
                    check=False,
                    timeout=10,
                )
                self.assertEqual(proc.returncode, 0, proc.stderr if isinstance(proc.stderr, str) else proc.returncode)
                payload = self._wait_payload(result)
                self.assertTrue(any(target == str(slot) for target in _targets(payload)))
                self.assertTrue(_alive(payload["pid"]))
                self.assertFalse(_lock_free(slot))
                self._signal_one(payload["pid"], script)
                self.assertFalse(self._wait_dead(payload["pid"], script))
                self.assertTrue(self._wait_free(slot))
            finally:
                _kill_script(script)

    def test_run_job_keeps_the_slot_fd_out_of_the_child(self) -> None:
        with tempfile.TemporaryDirectory(prefix="desktop-lab-slot-job-") as raw:
            root = Path(raw)
            slot = root / "slot.lock"
            marker = root / "fds.json"
            script = root / "child.py"
            script.write_text(
                "import json, os, sys\n"
                "from pathlib import Path\n"
                "targets = []\n"
                "for name in os.listdir('/proc/self/fd'):\n"
                "    try:\n"
                "        targets.append(os.readlink(f'/proc/self/fd/{name}'))\n"
                "    except OSError:\n"
                "        pass\n"
                "Path(sys.argv[1]).write_text(json.dumps(targets))\n"
            )
            report = run_job(
                {
                    "name": "slot-child",
                    "backend": "none",
                    "session_bus": False,
                    "timeout_s": 5,
                    "slot": str(slot),
                    "command": [sys.executable, str(script), str(marker)],
                },
                out=root / "out",
            )
            self.assertTrue(report["job_passed"], report["errors"])
            self.assertEqual(report["build_slot"]["argv"], equivalent_argv(slot, report["command"]))
            self.assertTrue(report["build_slot"]["close_on_exec"])
            self.assertFalse(report["build_slot"]["passed_to_child"])
            self.assertFalse(report["build_slot"]["live_slot"])
            targets = json.loads(marker.read_text())
            self.assertFalse(any(item == str(slot) for item in targets))
            self.assertTrue(_lock_free(slot))
            self.assertFalse(report["full_acceptance"])

    def test_sigkill_and_exit_137_are_unknown(self) -> None:
        commands = [
            [sys.executable, "-c", "import os, signal; os.kill(os.getpid(), signal.SIGKILL)"],
            [sys.executable, "-c", "import os; os._exit(137)"],
        ]
        for command in commands:
            with self.subTest(command=command[-1]):
                with tempfile.TemporaryDirectory(prefix="desktop-lab-sigkill-") as raw:
                    report = run_job(
                        {
                            "name": "sigkill",
                            "backend": "none",
                            "session_bus": False,
                            "timeout_s": 5,
                            "command": command,
                        },
                        out=Path(raw) / "out",
                    )
                self._assert_unknown_kill(report)

    def test_symlink_alias_does_not_touch_the_stand_in(self) -> None:
        with tempfile.TemporaryDirectory(prefix="desktop-lab-alias-") as raw:
            root = Path(raw)
            stand = root / "stand-in"
            stand.write_text("marker-contents")
            before = _file_identity(stand)
            alias = root / "alias.lock"
            alias.symlink_to(stand)
            with self.assertRaises(PolicyError) as caught:
                acquire_build_slot(alias)
            self.assertIn("symlink", str(caught.exception))
            self.assertEqual(_file_identity(stand), before)
            self.assertTrue(alias.is_symlink())

    def test_hardlink_is_refused_before_lock(self) -> None:
        with tempfile.TemporaryDirectory(prefix="desktop-lab-hardlink-") as raw:
            root = Path(raw)
            stand = root / "stand-in"
            stand.write_text("marker-contents")
            before = _file_identity(stand)
            link = root / "hard.lock"
            os.link(stand, link)
            with self.assertRaises(PolicyError) as caught:
                acquire_build_slot(link)
            self.assertIn("hardlink", str(caught.exception))
            self.assertEqual(_file_identity(stand), before)
            self.assertTrue(_lock_free(stand))

    def test_parent_symlink_is_not_followed(self) -> None:
        with tempfile.TemporaryDirectory(prefix="desktop-lab-parent-link-") as raw:
            root = Path(raw)
            real = root / "real"
            real.mkdir(mode=0o700)
            stand = real / "lock"
            stand.write_text("marker-contents")
            before = _file_identity(stand)
            alias_parent = root / "aliasdir"
            alias_parent.symlink_to(real, target_is_directory=True)
            with self.assertRaises(PolicyError) as caught:
                acquire_build_slot(alias_parent / "lock")
            self.assertIn("symlink", str(caught.exception))
            self.assertEqual(_file_identity(stand), before)

    def test_loose_parent_is_refused(self) -> None:
        with tempfile.TemporaryDirectory(prefix="desktop-lab-loose-") as raw:
            parent = Path(raw) / "loose"
            parent.mkdir()
            parent.chmod(0o755)
            target = parent / "slot.lock"
            with self.assertRaises(PolicyError) as caught:
                acquire_build_slot(target)
            self.assertIn("0700", str(caught.exception))
            self.assertFalse(target.exists())

    def test_xvfb_symlink_does_not_truncate_the_target(self) -> None:
        with tempfile.TemporaryDirectory(prefix="desktop-lab-xvfb-alias-") as raw:
            root = Path(raw)
            stand = root / "stand-in"
            stand.write_text("marker-contents")
            before = _file_identity(stand)
            alias = root / "xvfb.lock"
            alias.symlink_to(stand)
            slot = XvfbSlot(alias)
            with self.assertRaises(PolicyError) as caught:
                slot.acquire()
            self.assertIn("symlink", str(caught.exception))
            self.assertIsNone(slot.fd)
            self.assertEqual(_file_identity(stand), before)
            self.assertTrue(alias.is_symlink())

    def test_slot_stays_locked_until_the_daemon_is_dead(self) -> None:
        sibling = _sibling()
        try:
            with tempfile.TemporaryDirectory(prefix="desktop-lab-hold-") as raw:
                root = Path(raw)
                slot = root / "slot.lock"
                script = root / "hold.py"
                started = root / "started"
                marker = root / "acquired"
                script.write_text(_LOCK_DAEMON)
                report = run_job(
                    {
                        "name": "hold-slot",
                        "backend": "none",
                        "session_bus": False,
                        "timeout_s": 5,
                        "slot": str(slot),
                        "command": [sys.executable, str(script), str(slot), str(started), str(marker)],
                    },
                    out=root / "out",
                )
                self.assertTrue(report["job_passed"], report["errors"])
                self.assertTrue(started.is_file(), "daemon never reached the lock loop")
                self.assertFalse(marker.exists(), "daemon acquired the slot before drain finished")
                self.assertFalse(_script_running(script))
                self.assertTrue(_lock_free(slot))
                self.assertTrue(report["containment"]["verified_drain"])
                self.assertFalse(report["containment"]["claims_marker_scan_is_sufficient"])
                self.assertIn("--user", report["containment"]["argv_prefix"])
                self.assertIn("--kill-child", report["containment"]["argv_prefix"])
                self.assertIsNone(sibling.poll())
        finally:
            _stop_sibling(sibling)

    def test_incident_record_is_metadata_only(self) -> None:
        record = incident_record()
        self.assertFalse(record["clipboard_read"])
        self.assertFalse(record["helpers_killed_by_this_lane"])
        self.assertFalse(record["slot2_restarted"])
        self.assertFalse(record["runtime_dir_alone_is_isolation"])
        self.assertTrue(record["coordinator_sigkill_is_not_a_compiler_failure"])
        self.assertFalse(record["full_acceptance"])
        self.assertEqual(record["retired_name"], "slot1-retired-0200.lock")
        self.assertEqual({row["path"] for row in record["slots"]}, set(LIVE_SLOT_PATHS))
        blob = json.dumps(record)
        self.assertNotIn("mcp:launch", blob)
        for row in record["slots"]:
            self.assertFalse(row["locked_by_this_lane"])
            if row["exists"]:
                self.assertIsInstance(row["inode"], int)
                self.assertNotIn("contents", row)

    def _daemon_does_not_hold(self, wrapper: str) -> None:
        with tempfile.TemporaryDirectory(prefix="desktop-lab-slot-") as raw:
            root = Path(raw)
            slot = root / "slot.lock"
            script = root / "fake-daemon.py"
            result = root / "daemon.json"
            script.write_text(_DAEMON)
            held = None
            try:
                if wrapper == "python":
                    held = acquire_build_slot(slot)
                    self.assertFalse(_lock_free(slot))
                    proc = subprocess.Popen(
                        [sys.executable, str(script), str(result)],
                        stdin=subprocess.DEVNULL,
                        stdout=subprocess.DEVNULL,
                        stderr=subprocess.DEVNULL,
                        start_new_session=True,
                        close_fds=True,
                        pass_fds=(),
                    )
                    proc.wait(timeout=10)
                else:
                    proc = subprocess.run(
                        ["/usr/bin/flock", "--close", str(slot), sys.executable, str(script), str(result)],
                        check=False,
                        timeout=10,
                    )
                    self.assertEqual(proc.returncode, 0)
                payload = self._wait_payload(result)
                self.assertFalse(any(target == str(slot) for target in _targets(payload)))
                self.assertTrue(_alive(payload["pid"]))
                if held is not None:
                    release_build_slot(held)
                    held = None
                self.assertTrue(_lock_free(slot))
                self.assertTrue(_alive(payload["pid"]))
            finally:
                if held is not None:
                    release_build_slot(held)
                _kill_script(script)

    def _wait_payload(self, result: Path) -> dict:
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if result.exists() and result.stat().st_size:
                return json.loads(result.read_text())
            time.sleep(0.02)
        self.fail("fake daemon did not report its fds")

    def _wait_dead(self, pid: int, script: Path) -> bool:
        marker = str(script).encode()
        deadline = time.monotonic() + 2
        while time.monotonic() < deadline:
            if not _alive(pid):
                return False
            try:
                command = Path(f"/proc/{pid}/cmdline").read_bytes()
            except OSError:
                return False
            if marker not in command:
                return False
            time.sleep(0.02)
        return _alive(pid)

    def _wait_free(self, slot: Path) -> bool:
        deadline = time.monotonic() + 2
        while time.monotonic() < deadline:
            if _lock_free(slot):
                return True
            time.sleep(0.02)
        return False

    def _assert_unknown_kill(self, report: dict) -> None:
        self.assertFalse(report["job_passed"])
        self.assertEqual(report["kill_cause"], "unknown")
        self.assertEqual(report["result_class"], "killed")
        self.assertNotEqual(report["result_class"], "coordinator_cleanup")
        self.assertFalse(report["compiler_failure"])
        self.assertNotIn("retry", report)
        blob = " ".join(report["errors"])
        self.assertNotIn("queue cleanup", blob)
        self.assertNotIn("coordinator queue", blob)
        self.assertFalse(report["full_acceptance"])

    def _signal_one(self, pid: int, script: Path) -> None:
        command = Path(f"/proc/{pid}/cmdline").read_bytes()
        self.assertIn(str(script).encode(), command)
        fd = os.pidfd_open(pid)
        try:
            signal.pidfd_send_signal(fd, signal.SIGKILL)
        finally:
            os.close(fd)


if __name__ == "__main__":
    unittest.main()
