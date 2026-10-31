#!/usr/bin/env python3
"""Lifecycle checks of tests/e2e/universal-panels-native.py: setup that fails or stalls cleans up what it started, and
the descendants of a child that exited are ended too. Only the private processes these tests start are involved: each
check looks for processes carrying the case's own marker in /proc. Needs Xvfb and dbus-daemon (private, owned).

    python3 tests/e2e/universal-panels-native-test.py
"""
import importlib.util
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import time
import unittest

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("upn", HERE / "universal-panels-native.py")
upn = importlib.util.module_from_spec(spec)
spec.loader.exec_module(upn)


def script(dir_, name, body):
    path = Path(dir_) / name
    path.write_text("#!/bin/sh\n" + body + "\n")
    path.chmod(path.stat().st_mode | stat.S_IXUSR)
    return str(path)


def alive_marked(marker):
    """Live (not zombie) processes of this uid carrying `marker`, looked up independently of Case.owned."""
    out = []
    for d in os.listdir("/proc"):
        if not d.isdigit():
            continue
        try:
            if os.stat(f"/proc/{d}").st_uid != os.getuid():
                continue
            if marker not in Path(f"/proc/{d}/environ").read_bytes().split(b"\0"):
                continue
            state = Path(f"/proc/{d}/stat").read_text().rsplit(")", 1)[1].split()[0]
            if state != "Z":
                out.append(int(d))
        except OSError:
            continue
    return out


class Lifecycle(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="upn-lifecycle-", dir="/var/tmp")
        self.out = Path(self.tmp.name)
        self.saved = (upn.DBUS, upn.Case.BUS_TIMEOUT, upn.private_x11.start_private_xvfb)

    def tearDown(self):
        upn.DBUS, upn.Case.BUS_TIMEOUT, upn.private_x11.start_private_xvfb = self.saved
        self.tmp.cleanup()

    def test_a_stalled_bus_is_a_bounded_setup_error_and_leaves_nothing(self):
        upn.DBUS = script(self.out, "stalled-bus", "exec /usr/bin/sleep 300")
        upn.Case.BUS_TIMEOUT = 1.0
        c = upn.Case(self.out, "stall", 320, 240)
        start = time.monotonic()
        with self.assertRaisesRegex(RuntimeError, "no session bus address"):
            c.__enter__()  # what `with` does: when __enter__ raises, __exit__ is not called
        self.assertLess(time.monotonic() - start, 15.0)
        self.assertEqual(alive_marked(c.marker), [], "the Xvfb and the stalled bus are gone")
        self.assertTrue(c.log.closed)

    def test_a_bus_that_exits_early_is_a_setup_error_and_leaves_nothing(self):
        upn.DBUS = script(self.out, "dying-bus", "exit 3")
        c = upn.Case(self.out, "dying", 320, 240)
        with self.assertRaisesRegex(RuntimeError, "exited before"):
            c.__enter__()
        self.assertEqual(alive_marked(c.marker), [])

    def test_a_failing_x_server_start_cleans_up(self):
        def boom(**kw):
            raise RuntimeError("injected Xvfb failure")
        upn.private_x11.start_private_xvfb = boom
        c = upn.Case(self.out, "noxvfb", 320, 240)
        with self.assertRaisesRegex(RuntimeError, "injected"):
            c.__enter__()
        self.assertEqual(alive_marked(c.marker), [])
        self.assertTrue(c.log.closed)

    def test_a_flooded_bus_address_is_bounded(self):
        with upn.Case(self.out, "bus-flood", 320, 240) as c:
            child = subprocess.Popen([sys.executable, "-c", "import sys,time;sys.stdout.write('x'*8192);sys.stdout.flush();time.sleep(300)"],
                                     env=c.env, stdout=subprocess.PIPE, start_new_session=True)
            c.procs.append(child)
            with self.assertRaisesRegex(RuntimeError, "exceeds 4096"):
                c.read_line(child, 2.0)

    def test_the_descendants_of_an_exited_leader_are_ended(self):
        with upn.Case(self.out, "orphans", 320, 240) as c:
            p = c.spawn(["/bin/sh", "-c", "/usr/bin/sleep 300 & exit 0"], "orphan")
            p.wait(timeout=5)
            time.sleep(0.2)
            group = c.owned(pgid=p.pid)
            self.assertTrue(group, "the leader exited, its child lives on in its process group")
            c.stop(p)
            self.assertEqual([q for q in group if q in alive_marked(c.marker)], [], "stop() ended the leftover child")
            # a descendant that left the group (setsid) is still ended when the case closes
            q = c.spawn(["/bin/sh", "-c", "/usr/bin/setsid /usr/bin/sleep 300 & exit 0"], "escaped")
            q.wait(timeout=5)
            time.sleep(0.2)
            self.assertTrue(any(pid not in c.owned(pgid=q.pid) for pid in c.owned()), "a marked process outside the group")
            marker = c.marker
        self.assertEqual(alive_marked(marker), [], "nothing carrying the case's marker is left")

    def test_unrelated_processes_are_never_signalled(self):
        # a process of ours without the case's marker survives the case's cleanup
        other = subprocess.Popen(["/usr/bin/sleep", "300"], start_new_session=True)
        try:
            with upn.Case(self.out, "unrelated", 320, 240) as c:
                self.assertNotIn(other.pid, c.owned())
            self.assertIsNone(other.poll(), "the unrelated process is still running")
        finally:
            other.terminate()
            other.wait(timeout=5)


if __name__ == "__main__":
    sys.exit(0 if unittest.main(verbosity=2, exit=False).result.wasSuccessful() else 1)
