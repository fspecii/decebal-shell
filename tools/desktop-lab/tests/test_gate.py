"""Source-frozen gate, feature checks, and read-only trailer scans."""

from __future__ import annotations

from datetime import datetime, timezone
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import textwrap
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from lab.fingerprint import changed, fingerprint
from lab.gate import run_gate
from lab.policy import PolicyError
from lab.records import check_commits, check_message, index_commits

_ROOT_DATE = datetime(2026, 10, 31, 1, 0, tzinfo=timezone.utc)


def _git(repo: Path, *args: str, home: Path) -> None:
    subprocess.run(
        ["git", "-C", str(repo), "-c", "commit.gpgsign=false", "-c", "user.email=dev@example.com", "-c", "user.name=Desktop Lab", *args],
        check=True,
        env={
            "PATH": "/usr/bin:/bin",
            "HOME": str(home),
            "GIT_CONFIG_GLOBAL": "/dev/null",
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_CONFIG_SYSTEM": "/dev/null",
            "GIT_AUTHOR_DATE": "2026-10-31T01:00:00Z",
            "GIT_COMMITTER_DATE": "2026-10-31T01:00:00Z",
        },
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
    )


class GateTests(unittest.TestCase):
    def test_fingerprint_sees_a_change(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            target = root / "src.txt"
            target.write_text("one")
            before = fingerprint(root, ["src.txt"])
            target.write_text("two")
            after = fingerprint(root, ["src.txt"])
            self.assertEqual(changed(before, after), ["src.txt"])
            self.assertNotEqual(before["sha256"], after["sha256"])

    def test_frozen_fake_compiler_passes_and_dirty_source_fails(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            (root / "src.txt").write_text("frozen")
            compiler = root / "compile.py"
            compiler.write_text("from pathlib import Path\nPath('built-bin').write_text('bin')\n")
            spec = {
                "name": "frozen",
                "source_root": str(root),
                "source_paths": ["src.txt"],
                "build": {"argv": [sys.executable, str(compiler)], "features": []},
                "bins": ["built-bin"],
                "tests": [{"name": "true", "command": ["/usr/bin/true"], "backend": "none", "session_bus": False, "timeout_s": 5}],
            }
            report = run_gate(spec, out=root / "out", now=_ROOT_DATE)
            self.assertTrue(report["gate_passed"], report["errors"])
            self.assertTrue(report["source"]["frozen"])
            self.assertFalse(report["full_acceptance"])
            self.assertFalse(report["hardware_acceptance"])
            self.assertEqual(len(report["build"]["binaries"][0]["sha256"]), 64)
            dirty = root / "dirty.py"
            dirty.write_text("from pathlib import Path\nPath('src.txt').write_text('changed')\n")
            spec["build"]["argv"] = [sys.executable, str(dirty)]
            spec.pop("bins")
            failed = run_gate(spec, out=root / "dirty-out", now=_ROOT_DATE)
            self.assertFalse(failed["gate_passed"])
            self.assertFalse(failed["source"]["frozen"])
            self.assertIn("src.txt", failed["source"]["changed"])

    def test_feature_mismatch_does_not_run_the_compiler(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            (root / "src.txt").write_text("src")
            stamp = root / "stamp"
            compiler = root / "compile.py"
            compiler.write_text(f"from pathlib import Path\nPath({str(stamp)!r}).write_text('ran')\n")
            report = run_gate(
                {
                    "name": "features",
                    "source_root": str(root),
                    "source_paths": ["src.txt"],
                    "build": {"argv": [sys.executable, str(compiler)], "features": ["x11"]},
                },
                out=root / "out",
                now=_ROOT_DATE,
            )
            self.assertFalse(report["gate_passed"])
            self.assertFalse(stamp.exists())
            self.assertIn("declared features do not match the build command", report["errors"])
            self.assertIsNone(report["build"]["exit_code"])

    def test_cargo_is_refused_before_a_report(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            (root / "src.txt").write_text("src")
            with self.assertRaises(PolicyError):
                run_gate(
                    {
                        "name": "cargo",
                        "source_root": str(root),
                        "source_paths": ["src.txt"],
                        "build": {"argv": ["cargo", "test", "--features", "x11"], "features": ["x11"]},
                    },
                    out=root / "out",
                    now=_ROOT_DATE,
                )
            self.assertFalse((root / "out").exists())

    def test_trailer_check_does_not_rewrite_history(self) -> None:
        self.assertIn("Claude Code", check_message("Subject\n\nCo-authored-by: Claude Code <noreply@anthropic.com>\n"))
        self.assertIn("Co-authored-by", check_message("Subject\n\nCo-authored-by: Someone <dev@example.com>\n"))
        self.assertIsNone(check_message("Subject\n\nNo trailer here.\n"))
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            home = root / "home"
            home.mkdir()
            repo = root / "repo"
            repo.mkdir()
            _git(repo, "init", "-b", "master", home=home)
            (repo / "file.txt").write_text("one")
            _git(repo, "add", "file.txt", home=home)
            _git(repo, "commit", "-m", "first", home=home)
            first = subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"], text=True).strip()
            message = root / "message.txt"
            message.write_text("second\n\nCo-authored-by: Claude Code <noreply@anthropic.com>\n")
            (repo / "file.txt").write_text("two")
            _git(repo, "add", "file.txt", home=home)
            _git(repo, "commit", "-F", str(message), home=home)
            head = subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"], text=True).strip()
            body = subprocess.check_output(["git", "-C", str(repo), "log", "-1", "--format=%B"], text=True)
            errors = check_commits(repo, first)
            self.assertEqual(subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"], text=True).strip(), head)
            self.assertEqual(subprocess.check_output(["git", "-C", str(repo), "log", "-1", "--format=%B"], text=True), body)
            self.assertTrue(any("Claude Code" in item for item in errors))

    def test_index_matches_a_feature_report(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            home = root / "home"
            home.mkdir()
            repo = root / "repo"
            (repo / "src").mkdir(parents=True)
            _git(repo, "init", "-b", "master", home=home)
            (repo / "src" / "foo.rs").write_text("fn main() {}\n")
            _git(repo, "add", "src/foo.rs", home=home)
            _git(repo, "commit", "-m", "first", home=home)
            first = subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"], text=True).strip()
            (repo / "src" / "foo.rs").write_text("fn main() { let _ = 1; }\n")
            _git(repo, "add", "src/foo.rs", home=home)
            _git(repo, "commit", "-m", "update foo", home=home)
            sha = subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"], text=True).strip()
            evidence = root / "evidence"
            evidence.mkdir()
            (evidence / "report.json").write_text(json.dumps({"commit": sha, "feature": "foo", "job_passed": True}))
            report = index_commits(repo, first, evidence)
            self.assertFalse(report["history_rewritten"])
            self.assertFalse(report["full_acceptance"])
            self.assertEqual(report["commits"][0]["sha"], sha)
            self.assertIn("foo", report["commits"][0]["features"])
            self.assertEqual(report["commits"][0]["evidence"][0]["feature"], "foo")
            self.assertFalse(report["commits"][0]["evidence"][0]["full_acceptance"])


if __name__ == "__main__":
    unittest.main()
