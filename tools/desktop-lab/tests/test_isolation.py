"""A private runtime directory does not isolate a host HOME or DISPLAY."""

from __future__ import annotations

import json
import os
from pathlib import Path
import stat
import sys
import tempfile
import textwrap
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from lab.isolation import IsolatedEnv, host_snapshot
from lab.job import run_job, validate_job
from lab.policy import PolicyError

_SECRET = "supersecret-unique-value"
_SENTINELS = (
    "host-session-bus-sentinel-9f3a",
    "host-system-bus-sentinel-9f3a",
    "wayland-host-sentinel",
    "rdwm-host-sentinel.sock",
    "gtk-host-sentinel",
    _SECRET,
)

_CHILD = textwrap.dedent(
    """\
    import json, os, stat, sys
    from pathlib import Path
    system = os.environ["DBUS_SYSTEM_BUS_ADDRESS"].removeprefix("unix:path=")
    session = os.environ["DBUS_SESSION_BUS_ADDRESS"].removeprefix("unix:path=")
    payload = {
        "display": os.environ.get("DISPLAY"),
        "wayland": os.environ.get("WAYLAND_DISPLAY"),
        "xauthority": "XAUTHORITY" in os.environ,
        "session_manager": "SESSION_MANAGER" in os.environ,
        "rdwm": any(key.startswith("RDWM_") for key in os.environ),
        "gtk": any(key.startswith("GTK_") for key in os.environ),
        "gdk": any(key.startswith("GDK_") for key in os.environ),
        "gsk": any(key.startswith("GSK_") for key in os.environ),
        "qt": any(key.startswith("QT_") for key in os.environ),
        "secret": "AWS_SECRET_ACCESS_KEY" in os.environ,
        "home": os.environ.get("HOME"),
        "runtime_mode": stat.S_IMODE(Path(os.environ["XDG_RUNTIME_DIR"]).stat().st_mode),
        "system_exists": Path(system).exists(),
        "session_exists": Path(session).exists(),
    }
    Path(sys.argv[1]).write_text(json.dumps(payload))
    """
)


class IsolationTests(unittest.TestCase):
    def test_host_context_is_refused(self) -> None:
        for key in ("host_context", "inherit_host"):
            with self.assertRaises(PolicyError) as caught:
                validate_job({"command": ["/usr/bin/true"], "session_bus": False, key: True})
            self.assertIn("private XDG_RUNTIME_DIR alone is not isolation", str(caught.exception))

    def test_runtime_dir_alone_is_not_isolation(self) -> None:
        with tempfile.TemporaryDirectory(prefix="desktop-lab-iso-") as raw:
            iso = IsolatedEnv(Path(raw))
            iso.env["DISPLAY"] = ":0"
            iso.env["HOME"] = str(Path.home())
            with self.assertRaises(PolicyError) as caught:
                iso.assert_private(host_snapshot(), backend="none")
            self.assertIn("private XDG_RUNTIME_DIR alone is not isolation", str(caught.exception))

    def test_xvfb_display_must_be_at_least_700(self) -> None:
        with tempfile.TemporaryDirectory(prefix="desktop-lab-iso-") as raw:
            iso = IsolatedEnv(Path(raw))
            iso.env["DISPLAY"] = ":20"
            with self.assertRaises(PolicyError) as caught:
                iso.assert_private({}, backend="xvfb")
            self.assertIn("700", str(caught.exception))
            iso.env["DISPLAY"] = ":700.0"
            with self.assertRaises(PolicyError) as caught:
                iso.assert_private({}, backend="xvfb")
            self.assertIn("session bus", str(caught.exception))

    def test_job_scrubs_host_session(self) -> None:
        saved = dict(os.environ)
        try:
            os.environ["DISPLAY"] = ":0"
            os.environ["WAYLAND_DISPLAY"] = "wayland-host-sentinel"
            os.environ["XAUTHORITY"] = str(Path.home() / ".Xauthority")
            os.environ["RDWM_SOCK"] = "rdwm-host-sentinel.sock"
            os.environ["SESSION_MANAGER"] = "local/host-session-manager"
            os.environ["GTK_MODULES"] = "gtk-host-sentinel"
            os.environ["DBUS_SESSION_BUS_ADDRESS"] = "unix:path=/run/user/host-session-bus-sentinel-9f3a"
            os.environ["DBUS_SYSTEM_BUS_ADDRESS"] = "unix:path=/run/dbus/host-system-bus-sentinel-9f3a"
            os.environ["AWS_SECRET_ACCESS_KEY"] = _SECRET
            os.environ["HOME"] = str(Path.home())
            with tempfile.TemporaryDirectory(prefix="desktop-lab-iso-") as raw:
                root = Path(raw)
                script = root / "child.py"
                marker = root / "marker.json"
                script.write_text(_CHILD)
                report = run_job(
                    {
                        "name": "scrub",
                        "backend": "none",
                        "session_bus": False,
                        "timeout_s": 5,
                        "command": [sys.executable, str(script), str(marker)],
                    },
                    out=root / "out",
                )
                blob = (root / "out" / "report.json").read_text()
                logs = root / "out" / "logs"
                if logs.is_dir():
                    blob += "\n".join(path.read_text() for path in logs.glob("*.log"))
                seen = json.loads(marker.read_text())
                marker_text = marker.read_text()
        finally:
            os.environ.clear()
            os.environ.update(saved)
        self.assertTrue(report["job_passed"], report["errors"])
        self.assertIsNone(seen["display"])
        self.assertIsNone(seen["wayland"])
        self.assertFalse(seen["xauthority"])
        self.assertFalse(seen["session_manager"])
        self.assertFalse(seen["rdwm"])
        self.assertFalse(seen["gtk"])
        self.assertFalse(seen["gdk"])
        self.assertFalse(seen["gsk"])
        self.assertFalse(seen["qt"])
        self.assertFalse(seen["secret"])
        self.assertNotEqual(seen["home"], str(Path.home()))
        self.assertEqual(seen["runtime_mode"], 0o700)
        self.assertFalse(seen["system_exists"])
        self.assertFalse(seen["session_exists"])
        isolation = report["isolation"]
        self.assertFalse(isolation["host_home_reused"])
        self.assertFalse(isolation["host_display_reused"])
        self.assertFalse(isolation["host_bus_reused"])
        self.assertFalse(isolation["runtime_dir_alone_is_isolation"])
        self.assertFalse(isolation["system_bus_exists"])
        for sentinel in _SENTINELS:
            self.assertNotIn(sentinel, blob)
            self.assertNotIn(sentinel, marker_text)


if __name__ == "__main__":
    unittest.main()
