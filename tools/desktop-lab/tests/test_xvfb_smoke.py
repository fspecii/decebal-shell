"""One private Xvfb. This does not use the host display or a live build slot."""

from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from lab.job import run_job

_PROBE = textwrap.dedent(
    """\
    import json, os, subprocess, sys, time
    from pathlib import Path
    display = os.environ.get("DISPLAY")
    if not display or display in {":0", ":0.0", ":1", ":1.0"}:
        raise SystemExit("refusing a live display")
    number = display[1:].split(".", 1)[0]
    if not number.isdigit() or int(number) < 700:
        raise SystemExit("refusing a display below 700")
    if "WAYLAND_DISPLAY" in os.environ:
        raise SystemExit("wayland display leaked into the Xvfb job")
    system = os.environ["DBUS_SYSTEM_BUS_ADDRESS"].removeprefix("unix:path=")
    if Path(system).exists():
        raise SystemExit("system bus path exists")
    subprocess.check_call(["xsetroot", "-solid", "#336699"])
    time.sleep(0.2)
    ping = None
    dbus = Path("/usr/bin/dbus-send")
    if dbus.is_file():
        proc = subprocess.run(
            [str(dbus), "--session", "--type=method_call", "--dest=org.freedesktop.DBus",
             "/org/freedesktop/DBus", "org.freedesktop.DBus.Peer.Ping"],
            check=False,
        )
        ping = proc.returncode
        if ping != 0:
            raise SystemExit(ping)
    Path(sys.argv[1]).write_text(json.dumps({"display": display, "ping": ping, "system_exists": False}))
    """
)


class XvfbSmokeTests(unittest.TestCase):
    def test_private_xvfb_paints_one_screen(self) -> None:
        for binary in ("/usr/bin/Xvfb", "/usr/bin/import", "/usr/bin/convert", "/usr/bin/xsetroot", "/usr/bin/dbus-daemon"):
            self.assertTrue(Path(binary).is_file(), binary)
        with tempfile.TemporaryDirectory(prefix="desktop-lab-xvfb-") as raw:
            root = Path(raw)
            script = root / "probe.py"
            marker = root / "probe.json"
            script.write_text(_PROBE)
            report = run_job(
                {
                    "name": "xvfb-smoke",
                    "backend": "xvfb",
                    "session_bus": True,
                    "screenshot": True,
                    "screen": "640x480x24",
                    "timeout_s": 20,
                    "command": [sys.executable, str(script), str(marker)],
                },
                out=root / "out",
            )
            self.assertTrue(report["job_passed"], report["errors"])
            self.assertFalse(report["full_acceptance"])
            self.assertFalse(report["hardware_acceptance"])
            self.assertIsNone(report["gpu"])
            isolation = report["isolation"]
            self.assertTrue(isolation["session_bus_started"])
            self.assertFalse(isolation["system_bus_exists"])
            self.assertFalse(isolation["runtime_dir_alone_is_isolation"])
            display = isolation["display"]
            self.assertTrue(display.startswith(":"))
            self.assertNotIn(display, {":0", ":0.0", ":1", ":1.0"})
            number = int(display[1:].split(".", 1)[0])
            self.assertGreaterEqual(number, 700)
            transport = report["display_transport"]
            self.assertEqual(transport["number"], number)
            self.assertTrue(transport["filesystem_socket"])
            self.assertTrue(transport["abstract_socket"])
            self.assertTrue(transport["lock_pid_matches"])
            self.assertFalse(transport["displayfd_probe"])
            self.assertIsInstance(transport["filesystem_inode"], int)
            self.assertIsInstance(transport["abstract_inode"], int)
            self.assertNotEqual(transport["filesystem_inode"], transport["abstract_inode"])
            self.assertEqual(transport["filesystem_listener_inodes"], [transport["filesystem_inode"]])
            self.assertEqual(transport["abstract_listener_inodes"], [transport["abstract_inode"]])
            self.assertTrue(transport["inodes_owned_by_server"])
            self.assertIsNone(isolation["wayland_display"])
            probe = json.loads(marker.read_text())
            self.assertEqual(probe["display"], display)
            if Path("/usr/bin/dbus-send").is_file():
                self.assertEqual(probe["ping"], 0)
            else:
                self.assertIsNone(probe["ping"])
            image = report["screenshots"][0]
            self.assertEqual(image["width"], 640)
            self.assertEqual(image["height"], 480)
            self.assertGreater(image["bytes"], 8)
            pixel = subprocess.check_output(
                ["/usr/bin/convert", image["path"], "-format", "%[hex:p{10,10}]", "info:"],
                text=True,
            ).strip().lower()
            self.assertTrue(pixel.startswith("336699"), pixel)
            processes = {item["name"]: item for item in report["processes"]}
            xvfb = [item for item in report["processes"] if item["name"] == "xvfb"]
            self.assertEqual(len(xvfb), 1)
            self.assertNotIn("-displayfd", xvfb[0]["argv"])
            self.assertEqual(xvfb[0]["argv"][1], display)
            self.assertEqual(xvfb[0]["display_transport"], transport)
            self.assertTrue(xvfb[0]["pidfd"])
            self.assertIsNotNone(xvfb[0]["exit_code"])
            self.assertTrue(processes["command"]["pidfd"])
            self.assertFalse(xvfb[0]["hardware_acceptance"])
            self.assertFalse(Path(f"/tmp/.X{number}-lock").exists())


if __name__ == "__main__":
    unittest.main()
