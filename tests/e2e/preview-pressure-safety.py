#!/usr/bin/env python3
"""Safety fixtures for the preview-pressure harness.

Existing evidence is refused and left byte-for-byte unchanged.
A PID whose live parent is not the owned Popen is never pidfd-signaled.
"""

from __future__ import annotations

import importlib.util
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parent / "preview-pressure.py"
SENTINEL = b"sentinel-keep\n"


def load_harness():
    spec = importlib.util.spec_from_file_location("preview_pressure_harness", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def ppid(pid: int) -> int:
    stat = Path(f"/proc/{pid}/stat").read_text(encoding="utf-8")
    return int(stat[stat.rfind(")") + 2:].split()[1])


class PreviewPressureSafety(unittest.TestCase):
    def test_existing_evidence_is_refused_and_sentinel_stays(self):
        with tempfile.TemporaryDirectory(prefix="preview-pressure-sentinel-", dir="/var/tmp") as raw:
            root = Path(raw)
            sentinel = root / "sentinel.txt"
            nested = root / "rapid_nav" / "root" / "keep.txt"
            nested.parent.mkdir(parents=True)
            sentinel.write_bytes(SENTINEL)
            nested.write_bytes(SENTINEL)
            stamps = {
                sentinel: sentinel.stat().st_mtime_ns,
                nested: nested.stat().st_mtime_ns,
            }
            env = os.environ.copy()
            env["PREVIEW_PRESSURE_EVIDENCE"] = str(root)
            completed = subprocess.run(
                [sys.executable, str(SCRIPT)],
                env=env,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=20,
                check=False,
            )
            self.assertEqual(completed.returncode, 2, completed.stderr)
            self.assertIn("already exists", completed.stderr)
            self.assertEqual(sentinel.read_bytes(), SENTINEL)
            self.assertEqual(nested.read_bytes(), SENTINEL)
            self.assertEqual(sentinel.stat().st_mtime_ns, stamps[sentinel])
            self.assertEqual(nested.stat().st_mtime_ns, stamps[nested])
            self.assertFalse((root / "summary.json").exists())
            self.assertFalse((root / "min-repro.json").exists())

    def test_existing_case_directory_is_not_replaced(self):
        harness = load_harness()
        with tempfile.TemporaryDirectory(prefix="preview-pressure-case-", dir="/var/tmp") as raw:
            evidence = Path(raw) / "evidence"
            case_dir = evidence / "rapid_nav"
            marker = case_dir / "root" / "keep.txt"
            marker.parent.mkdir(parents=True)
            marker.write_bytes(SENTINEL)
            stamp = marker.stat().st_mtime_ns
            with self.assertRaises(FileExistsError):
                harness.Case("rapid_nav", evidence)
            self.assertEqual(marker.read_bytes(), SENTINEL)
            self.assertEqual(marker.stat().st_mtime_ns, stamp)

    def test_foreign_parent_is_never_pidfd_signaled(self):
        harness = load_harness()
        owned = subprocess.Popen(["/bin/sleep", "60"])
        bystander = subprocess.Popen(["/bin/sleep", "60"])
        try:
            real_parent = ppid(bystander.pid)
            foreign_parent = 1 if real_parent != 1 else 2
            self.assertNotEqual(foreign_parent, real_parent)
            status = harness.signal_pidfd(bystander.pid, signal.SIGTERM, parent=foreign_parent)
            self.assertEqual(status, "not_our_child")
            self.assertIsNone(bystander.poll())
            with patch.object(harness, "proc_children", return_value=[bystander.pid]):
                info = harness.stop_process(owned)
            self.assertEqual(info["children"][0]["pid"], bystander.pid)
            self.assertEqual(info["children"][0]["term"], "not_our_child")
            self.assertIsNone(bystander.poll())
            self.assertIsNotNone(owned.poll())
        finally:
            for proc in (owned, bystander):
                if proc.poll() is None:
                    proc.kill()
                proc.wait(timeout=3)

    def test_artifact_override_must_match_pinned_sha256(self):
        with tempfile.TemporaryDirectory(prefix="preview-pressure-artifact-", dir="/var/tmp") as raw:
            root = Path(raw)
            art = root / "art"
            (art / "bin").mkdir(parents=True)
            (art / "bin" / "decsettings").write_bytes(b"not-the-binary")
            (art / "bin" / "decthemectl").write_bytes(b"not-the-binary")
            (art / "SHA256SUMS").write_text(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  decsettings\n"
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  decthemectl\n",
                encoding="utf-8",
            )
            evidence = root / "evidence-new"
            env = os.environ.copy()
            env["PREVIEW_PRESSURE_ARTIFACT"] = str(art)
            env["PREVIEW_PRESSURE_EVIDENCE"] = str(evidence)
            completed = subprocess.run(
                [sys.executable, str(SCRIPT)],
                env=env,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=20,
                check=False,
            )
            self.assertEqual(completed.returncode, 2, completed.stderr)
            self.assertIn("pinned", completed.stderr)
            repro = (evidence / "min-repro.json").read_text(encoding="utf-8")
            self.assertIn("pinned", repro)
            self.assertEqual((art / "bin" / "decsettings").read_bytes(), b"not-the-binary")


if __name__ == "__main__":
    unittest.main(verbosity=1)
