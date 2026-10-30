#!/usr/bin/env python3
"""Staged optional-session prefix checks; never installs or starts a desktop."""
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]


class SessionInstall(unittest.TestCase):
    def run_install(self, prefix, stage, command_line=False):
        # Environment values are literal even when they contain make/shell syntax.
        command = ["make", "install-session"]
        if command_line:
            command += ["PREFIX=" + prefix, "DESTDIR=" + str(stage)]
        return subprocess.run(command, cwd=ROOT,
                              env=dict(os.environ, PREFIX=prefix, DESTDIR=str(stage)),
                              capture_output=True, text=True)

    def test_runtime_paths_follow_prefix_and_never_contain_destdir(self):
        for prefix in ("/usr", "/usr/local"):
            with self.subTest(prefix=prefix), tempfile.TemporaryDirectory(prefix="session stage ") as temp:
                stage = Path(temp)
                result = self.run_install(prefix, stage, command_line=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                base = stage / prefix.lstrip("/")
                wrapper = (base / "bin/decebal-session").read_text()
                self.assertIn(f'PATH="{prefix}/lib/decebal/shim:$PATH"', wrapper)
                self.assertIn('exec /usr/bin/start-hyprland "$@"', wrapper)
                desktop = (base / "share/wayland-sessions/hyprland-decebal.desktop").read_text()
                self.assertIn(f"Exec={prefix}/bin/decebal-session", desktop)
                for path in base.rglob("*"):
                    if path.is_file():
                        self.assertNotIn(str(stage), path.read_text())
                self.assertTrue((base / "bin/decebal-session").stat().st_mode & 0o111)
                self.check_shims(base, prefix, stage)

    def check_shims(self, base, prefix, stage):
        tools = stage / "fake-tools"
        tools.mkdir()
        log = stage / "calls"
        def tool(name, body):
            path = tools / name
            path.write_text("#!/bin/sh\n" + body + "\n")
            path.chmod(0o755)
        tool("systemctl", 'printf "systemctl:%s\\n" "$*" >> "$CALL_LOG"')
        tool("pgrep", "exit 1")
        tool("pidof", "exit 1")
        tool("setsid", 'exec "$@"')
        tool("mako", 'printf "mako\\n" >> "$CALL_LOG"')
        tool("hypridle", 'printf "hypridle:%s\\n" "$*" >> "$CALL_LOG"')
        tool("hyprlock", 'printf "hyprlock:%s\\n" "$*" >> "$CALL_LOG"')
        env = dict(os.environ, PATH=str(tools), CALL_LOG=str(log))
        for name, args in (("omarchy-launch-shell", []), ("omarchy-shell", ["lock"])):
            subprocess.run(["/bin/sh", str(base / "lib/decebal/shim" / name), *args],
                           env=env, check=True, capture_output=True, timeout=5)
        deadline = time.monotonic() + 2
        while time.monotonic() < deadline:
            calls = log.read_text().splitlines() if log.exists() else []
            if any(line.startswith("hypridle:") for line in calls):
                break
            time.sleep(.01)
        self.assertIn(f"hypridle:-c {prefix}/share/decebal/hypridle.conf", calls)
        self.assertIn(f"hyprlock:-c {prefix}/share/decebal/hyprlock.conf", calls)
        idle = (base / "share/decebal/hypridle.conf").read_text()
        self.assertIn(f"hyprlock -c {prefix}/share/decebal/hyprlock.conf", idle)

    def test_unsafe_prefix_is_rejected_before_writes(self):
        for prefix in ("relative", "/opt/space here", "/opt/a;touch injected", "/opt/$(touch injected)", "/opt/a:b", "/opt/a\nother", "/usr/../../tmp", "/usr/./local"):
            with self.subTest(prefix=prefix), tempfile.TemporaryDirectory() as temp:
                stage = Path(temp) / "not-created"
                result = self.run_install(prefix, stage)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("PREFIX must be absolute", result.stderr)
                self.assertFalse(stage.exists())
                self.assertFalse((ROOT / "injected").exists())


if __name__ == "__main__":
    unittest.main()
