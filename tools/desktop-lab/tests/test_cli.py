"""CLI exit codes. Exit 0 is a check result, never full acceptance."""

from __future__ import annotations

from datetime import datetime, timezone
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from lab.cli import _baseline
from lab.job import run_job
from lab.policy import PolicyError

_SCRIPT = Path(__file__).resolve().parents[1] / "desktop-lab"
_BASELINE = Path(__file__).resolve().parents[1] / "BASELINE"


class CliTests(unittest.TestCase):
    def test_true_is_not_full_acceptance(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            out = Path(raw) / "out"
            proc = subprocess.run(
                [sys.executable, str(_SCRIPT), "run", "--out", str(out), "--timeout", "5", "--", "/usr/bin/true"],
                check=False,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            self.assertEqual(proc.returncode, 0, proc.stderr)
            self.assertIn("full_acceptance: false", proc.stdout)
            report = json.loads((out / "report.json").read_text())
            self.assertTrue(report["job_passed"])
            self.assertFalse(report["full_acceptance"])
            self.assertIsNone(report["gpu"])

    def test_git_push_is_a_policy_refusal(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            proc = subprocess.run(
                [sys.executable, str(_SCRIPT), "run", "--out", str(Path(raw) / "out"), "--", "git", "push"],
                check=False,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            self.assertEqual(proc.returncode, 2)
            self.assertIn("refusing", proc.stderr)

    def test_past_deadline_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            with self.assertRaises(PolicyError):
                run_job(
                    {"name": "late", "backend": "none", "session_bus": False, "command": ["/usr/bin/true"]},
                    out=Path(raw) / "out",
                    now=datetime(2026, 10, 31, 11, 36, 33, tzinfo=timezone.utc),
                )

    def test_missing_baseline_does_not_scan_all_history(self) -> None:
        if _BASELINE.is_file():
            self.assertGreaterEqual(len(_baseline()), 7)
        else:
            with self.assertRaises(PolicyError):
                _baseline()


if __name__ == "__main__":
    unittest.main()
