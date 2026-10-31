"""Owned-process cleanup, stubs, and a fake compositor."""

from __future__ import annotations

import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import textwrap
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from lab.display import XvfbSlot
from lab.job import run_job
from lab.policy import PolicyError
from lab.procman import kill_marked


class LifecycleTests(unittest.TestCase):
    def test_timeout_kills_only_the_owned_sleeper(self) -> None:
        other = subprocess.Popen(
            [sys.executable, "-c", "import time; time.sleep(60)"],
            env={"PATH": "/usr/bin:/bin"},
            stdin=subprocess.DEVNULL,
            start_new_session=True,
        )
        try:
            with tempfile.TemporaryDirectory(prefix="desktop-lab-time-") as raw:
                report = run_job(
                    {
                        "name": "timeout",
                        "backend": "none",
                        "session_bus": False,
                        "timeout_s": 0.4,
                        "command": [sys.executable, "-c", "import time; time.sleep(30)"],
                    },
                    out=Path(raw) / "out",
                )
            self.assertTrue(report["timed_out"])
            self.assertFalse(report["job_passed"])
            self.assertNotEqual(report["result_class"], "coordinator_cleanup")
            self.assertNotIn("kill_cause", report)
            self.assertFalse(report["containment"]["claims_marker_scan_is_sufficient"])
            self.assertTrue(report["containment"]["verified_drain"])
            command = next(item for item in report["processes"] if item["name"] == "command")
            self.assertTrue(command["pidfd"])
            self.assertFalse(command["hardware_acceptance"])
            self.assertIsNone(other.poll())
        finally:
            if other.poll() is None:
                os.killpg(other.pid, signal.SIGKILL)
                other.wait(timeout=2)

    def test_double_fork_daemon_dies_inside_the_namespace(self) -> None:
        self._contained_script(
            "double",
            textwrap.dedent(
                """\
                import os, sys, time
                started = sys.argv[1]
                if os.fork() == 0:
                    os.setsid()
                    if os.fork() > 0:
                        os._exit(0)
                    open(started, "w").write("daemon")
                    time.sleep(60)
                while not os.path.exists(started):
                    time.sleep(0.02)
                os._exit(0)
                """
            ),
        )

    def test_nested_cleared_marker_daemon_dies(self) -> None:
        self._contained_script(
            "nested",
            textwrap.dedent(
                """\
                import os, sys, time
                started = sys.argv[1]
                if os.fork() == 0:
                    if os.fork() == 0:
                        os.setsid()
                        env = {}
                        for key, value in os.environ.items():
                            if key != "DECEBAL_DESKTOP_LAB_JOB":
                                env[key] = value
                        code = "import sys, time\\nopen(sys.argv[1], 'w').write('cleared')\\ntime.sleep(60)\\n"
                        os.execve(sys.executable, [sys.executable, "-c", code, started], env)
                    time.sleep(60)
                while not os.path.exists(started):
                    time.sleep(0.02)
                os._exit(0)
                """
            ),
        )

    def test_churn_leaves_no_namespace_processes(self) -> None:
        self._contained_script(
            "churn",
            textwrap.dedent(
                """\
                import os, sys, time
                started = sys.argv[1]
                for _ in range(5):
                    if os.fork() == 0:
                        os._exit(0)
                if os.fork() == 0:
                    os.setsid()
                    if os.fork() > 0:
                        os._exit(0)
                    open(started, "w").write("survivor")
                    time.sleep(60)
                while not os.path.exists(started):
                    time.sleep(0.02)
                os._exit(0)
                """
            ),
        )

    def test_term_then_kill_reaps_a_daemon_and_spares_a_sibling(self) -> None:
        sibling = _sibling()
        try:
            with tempfile.TemporaryDirectory(prefix="desktop-lab-term-") as raw:
                root = Path(raw)
                script = root / "term.py"
                marker = root / "marker"
                script.write_text(
                    "import signal, sys, time\n"
                    "marker = sys.argv[1]\n"
                    "def on_term(signum, frame):\n"
                    "    open(marker, 'w').write('term')\n"
                    "signal.signal(signal.SIGTERM, on_term)\n"
                    "open(marker, 'w').write('started')\n"
                    "while True:\n"
                    "    time.sleep(0.1)\n"
                )
                report = run_job(
                    {
                        "name": "term-kill",
                        "backend": "none",
                        "session_bus": False,
                        "timeout_s": 2,
                        "command": [sys.executable, str(script), str(marker)],
                    },
                    out=root / "out",
                )
                self.assertTrue(report["timed_out"])
                self.assertFalse(report["job_passed"])
                self.assertEqual(marker.read_text(), "term")
                self.assertFalse(_cmdline_has(str(script).encode()))
                self.assertNotEqual(report["result_class"], "coordinator_cleanup")
                self.assertNotIn("kill_cause", report)
                self.assertTrue(report["containment"]["verified_drain"])
                self.assertFalse(report["containment"]["claims_marker_scan_is_sufficient"])
                self.assertIsNone(sibling.poll())
        finally:
            _stop_sibling(sibling)

    def test_build_timeout_is_recorded_before_stop(self) -> None:
        with tempfile.TemporaryDirectory(prefix="desktop-lab-build-time-") as raw:
            report = run_job(
                {
                    "name": "build-timeout",
                    "backend": "none",
                    "session_bus": False,
                    "timeout_s": 5,
                    "command": ["/usr/bin/true"],
                    "build": {
                        "argv": [sys.executable, "-c", "import time; time.sleep(30)"],
                        "features": [],
                        "timeout_s": 0.8,
                    },
                },
                out=Path(raw) / "out",
            )
        self.assertTrue(report["build"]["timed_out"])
        self.assertIsNotNone(report["build"]["exit_code"])
        self.assertFalse(report["timed_out"])
        self.assertFalse(report["job_passed"])
        self.assertNotEqual(report["result_class"], "coordinator_cleanup")
        self.assertNotIn("kill_cause", report)
        self.assertNotIn("queue cleanup", " ".join(report["errors"]))

    def test_unsupported_containment_does_not_start_the_command(self) -> None:
        with tempfile.TemporaryDirectory(prefix="desktop-lab-unsupported-") as raw:
            root = Path(raw)
            marker = root / "ran"
            with patch(
                "lab.job.probe_containment",
                side_effect=PolicyError("containment unsupported: pid namespace probe failed"),
            ):
                report = run_job(
                    {
                        "name": "unsupported",
                        "backend": "none",
                        "session_bus": False,
                        "timeout_s": 5,
                        "command": [sys.executable, "-c", "open(sys.argv[1], 'w').write('ran')", str(marker)],
                    },
                    out=root / "out",
                )
        self.assertFalse(marker.exists())
        self.assertFalse(report["job_passed"])
        self.assertEqual(report["containment"]["kind"], "unsupported")
        self.assertFalse(report["containment"]["claims_marker_scan_is_sufficient"])
        self.assertFalse(report["containment"]["verified_drain"])
        self.assertFalse(any(item["name"] == "command" for item in report["processes"]))
        self.assertIn("containment unsupported", " ".join(report["errors"]))

    def test_kill_marked_ignores_a_different_token(self) -> None:
        token = "a" * 32
        other = "b" * 32
        marked = subprocess.Popen(
            [sys.executable, "-c", "import time; time.sleep(60)"],
            env={"PATH": "/usr/bin:/bin", "DECEBAL_DESKTOP_LAB_JOB": token},
            stdin=subprocess.DEVNULL,
        )
        plain = subprocess.Popen(
            [sys.executable, "-c", "import time; time.sleep(60)"],
            env={"PATH": "/usr/bin:/bin", "DECEBAL_DESKTOP_LAB_JOB": other},
            stdin=subprocess.DEVNULL,
        )
        try:
            signaled = kill_marked(token)
            self.assertIn(marked.pid, signaled)
            marked.wait(timeout=2)
            self.assertIsNone(plain.poll())
            self.assertEqual(kill_marked("short"), [])
        finally:
            for proc in (marked, plain):
                if proc.poll() is None:
                    proc.kill()
                    proc.wait(timeout=2)

    def test_recorder_stubs_do_not_succeed(self) -> None:
        with tempfile.TemporaryDirectory(prefix="desktop-lab-stub-") as raw:
            root = Path(raw)
            script = root / "call.py"
            marker = root / "codes.json"
            script.write_text(
                "import json, subprocess, sys\n"
                "names = ['xclip', 'wl-copy', 'wl-paste', 'xsel', 'xdg-open', 'systemctl']\n"
                "codes = {name: subprocess.call([name, '-selection', 'clipboard']) for name in names}\n"
                "open(sys.argv[1], 'w').write(json.dumps(codes))\n"
            )
            report = run_job(
                {
                    "name": "stubs",
                    "backend": "none",
                    "session_bus": False,
                    "timeout_s": 5,
                    "command": [sys.executable, str(script), str(marker)],
                },
                out=root / "out",
            )
            self.assertTrue(report["job_passed"], report["errors"])
            codes = json.loads(marker.read_text())
            self.assertEqual(set(codes.values()), {127})
            commands = [row["command"] for row in report["recorder"]]
            self.assertEqual(commands, ["xclip", "wl-copy", "wl-paste", "xsel", "xdg-open", "systemctl"])
            self.assertTrue(report["recorder_is_not_protocol_success"])
            for row in report["recorder"]:
                self.assertNotIn("contents", row)

    def test_second_xvfb_slot_does_not_release_the_first(self) -> None:
        with tempfile.TemporaryDirectory(prefix="desktop-lab-xvfb-lock-") as raw:
            path = Path(raw) / "xvfb.lock"
            first = XvfbSlot(path)
            second = XvfbSlot(path)
            first.acquire()
            try:
                with self.assertRaises(PolicyError):
                    second.acquire()
                self.assertIsNotNone(first.fd)
            finally:
                first.release()
            self.assertIsNone(first.fd)
            again = XvfbSlot(path)
            again.acquire()
            again.release()

    def test_fake_compositor_hides_host_display(self) -> None:
        saved = os.environ.get("DISPLAY")
        os.environ["DISPLAY"] = ":0"
        try:
            with tempfile.TemporaryDirectory(prefix="desktop-lab-way-") as raw:
                root = Path(raw)
                sway = root / "fake-sway"
                marker = root / "env.json"
                sway.write_text(
                    "#!/usr/bin/env python3\n"
                    "import os, time\n"
                    "from pathlib import Path\n"
                    "Path(os.environ['XDG_RUNTIME_DIR'], 'wayland-9').touch()\n"
                    "time.sleep(30)\n"
                )
                sway.chmod(0o755)
                child = root / "child.py"
                child.write_text(
                    "import json, os, sys\n"
                    "from pathlib import Path\n"
                    "Path(sys.argv[1]).write_text(json.dumps({\n"
                    " 'display': os.environ.get('DISPLAY'),\n"
                    " 'wayland': os.environ.get('WAYLAND_DISPLAY'),\n"
                    " 'wlr': os.environ.get('WLR_BACKENDS'),\n"
                    "}))\n"
                )
                report = run_job(
                    {
                        "name": "wayland",
                        "backend": "wayland",
                        "session_bus": False,
                        "timeout_s": 5,
                        "tools": {"sway": str(sway)},
                        "command": [sys.executable, str(child), str(marker)],
                    },
                    out=root / "out",
                )
                self.assertTrue(report["job_passed"], report["errors"])
                seen = json.loads(marker.read_text())
                self.assertIsNone(seen["display"])
                self.assertEqual(seen["wayland"], "wayland-9")
                self.assertIsNone(seen["wlr"])
                self.assertNotIn("WLR_BACKENDS", report["isolation"]["env_names"])
                self.assertNotIn(":0", json.dumps(report["isolation"]))
        finally:
            if saved is None:
                os.environ.pop("DISPLAY", None)
            else:
                os.environ["DISPLAY"] = saved


    def _contained_script(self, name: str, source: str) -> None:
        sibling = _sibling()
        try:
            with tempfile.TemporaryDirectory(prefix=f"desktop-lab-{name}-") as raw:
                root = Path(raw)
                script = root / f"{name}.py"
                started = root / "started"
                script.write_text(source)
                report = run_job(
                    {
                        "name": name,
                        "backend": "none",
                        "session_bus": False,
                        "timeout_s": 5,
                        "command": [sys.executable, str(script), str(started)],
                    },
                    out=root / "out",
                )
                self.assertTrue(report["job_passed"], report["errors"])
                self.assertTrue(started.is_file())
                self.assertFalse(_cmdline_has(str(script).encode()))
                self.assertFalse(_cmdline_has(str(started).encode()))
                self.assertEqual(report["containment"]["kind"], "pid-namespace")
                self.assertTrue(report["containment"]["verified_drain"])
                self.assertFalse(report["containment"]["claims_marker_scan_is_sufficient"])
                self.assertIn("--kill-child", report["containment"]["argv_prefix"])
                self.assertIn("--mount-proc", report["containment"]["argv_prefix"])
                self.assertIn("--user", report["containment"]["argv_prefix"])
                self.assertIsNone(sibling.poll())
                self.assertNotIn(sibling.pid, report["escaped_children_signaled"])
        finally:
            _stop_sibling(sibling)


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


def _cmdline_has(marker: bytes) -> bool:
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


if __name__ == "__main__":
    unittest.main()
