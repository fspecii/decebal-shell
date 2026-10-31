#!/usr/bin/env python3
"""Lightweight resource-runner regressions. No Cargo invocation or desktop access."""
import fcntl
import json
import os
import signal
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest

RUNNER = Path(__file__).resolve().parents[1] / 'tools/dev/desktop_job.py'


class DesktopJobTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.slots = self.root / 'slots'

    def argv(self, *args):
        return [sys.executable, str(RUNNER), '--slots-dir', str(self.slots), *args]

    def call(self, *args):
        return subprocess.run(self.argv(*args), capture_output=True, text=True, timeout=10)

    def wait_file(self, path):
        deadline = time.monotonic() + 3
        while not path.exists() and time.monotonic() < deadline:
            time.sleep(.01)
        self.assertTrue(path.exists(), f'{path} was not written')

    def test_low_priorities_jobs_and_lean_defaults_reach_child(self):
        code = 'import os,json;print(json.dumps({"nice":os.getpriority(os.PRIO_PROCESS,0),"jobs":os.environ["CARGO_BUILD_JOBS"],"threads":os.environ["RUST_TEST_THREADS"],"debug":os.environ["CARGO_PROFILE_DEV_DEBUG"],"target":os.environ["CARGO_TARGET_DIR"]}))'
        env = {k: v for k, v in os.environ.items() if k not in ('CARGO_BUILD_JOBS', 'RUST_TEST_THREADS', 'CARGO_PROFILE_DEV_DEBUG', 'CARGO_TARGET_DIR')}
        r = subprocess.run(self.argv('--', sys.executable, '-c', code), env=env, capture_output=True, text=True, timeout=10)
        self.assertEqual(r.returncode, 0, r.stderr)
        d = json.loads(r.stdout)
        self.assertEqual((d['nice'], d['jobs'], d['threads'], d['debug']), (19, '1', '1', '0'))
        self.assertTrue(Path(d['target']).is_absolute())
        r = self.call('--', '/usr/bin/ionice', '-p', '0')
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn('idle', r.stdout)

    def test_pause_prevents_start_and_resume_preserves_lock_files(self):
        marker = self.root / 'started'
        self.assertEqual(self.call('--pause').returncode, 0)
        r = self.call('--', sys.executable, '-c', f'from pathlib import Path;Path({str(marker)!r}).touch()')
        self.assertEqual(r.returncode, 75)
        self.assertFalse(marker.exists())
        self.assertEqual(self.call('--resume').returncode, 0)
        self.assertEqual(self.call('--', sys.executable, '-c', 'pass').returncode, 0)
        self.assertTrue((self.slots / 'slot1.lock').is_file())
        self.assertTrue((self.slots / 'slot2.lock').is_file())

    def test_legacy_second_slot_blocks_without_leaking_first_lock(self):
        self.slots.mkdir()
        with (self.slots / 'slot2.lock').open('w') as second:
            fcntl.flock(second, fcntl.LOCK_EX)
            r = self.call('--queue-timeout', '.05', '--', sys.executable, '-c', 'raise SystemExit(99)')
            self.assertEqual(r.returncode, 75, r.stderr)
            with (self.slots / 'slot1.lock').open('w') as first:
                fcntl.flock(first, fcntl.LOCK_EX | fcntl.LOCK_NB)

    def test_two_runners_serialize_and_do_not_leak_locks_to_child(self):
        started, finished, second = (self.root / n for n in ('start', 'finish', 'second'))
        code = f'from pathlib import Path;import time;Path({str(started)!r}).touch();time.sleep(.3);Path({str(finished)!r}).touch()'
        p = subprocess.Popen(self.argv('--', sys.executable, '-c', code), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            self.wait_file(started)
            r = self.call('--', sys.executable, '-c', f'from pathlib import Path;assert Path({str(finished)!r}).exists();Path({str(second)!r}).touch()')
            self.assertEqual(r.returncode, 0, r.stderr)
            self.assertTrue(second.exists())
            self.assertEqual(p.wait(timeout=3), 0)
        finally:
            if p.poll() is None:
                p.kill()
                p.wait()

    def test_pause_stops_active_command_and_releases_slots(self):
        started = self.root / 'started'
        code = f'from pathlib import Path;import time;Path({str(started)!r}).touch();time.sleep(60)'
        p = subprocess.Popen(self.argv('--', sys.executable, '-c', code), stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            self.wait_file(started)
            self.assertEqual(self.call('--pause').returncode, 0)
            _, stderr = p.communicate(timeout=4)
            self.assertEqual(p.returncode, 130, stderr)
            self.call('--resume')
            self.assertEqual(self.call('--', sys.executable, '-c', 'pass').returncode, 0)
        finally:
            if p.poll() is None:
                p.kill()
                p.wait()

    def test_execution_deadline_ends_non_detached_grandchild(self):
        pidfile = self.root / 'grandchild'
        code = f'import subprocess,time;from pathlib import Path;p=subprocess.Popen(["/usr/bin/sleep","60"]);Path({str(pidfile)!r}).write_text(str(p.pid));time.sleep(60)'
        r = self.call('--timeout', '.5', '--', sys.executable, '-c', code)
        self.assertEqual(r.returncode, 124, r.stderr)
        pid = int(pidfile.read_text())
        stat = Path(f'/proc/{pid}/stat')
        self.assertTrue(not stat.exists() or stat.read_text().rsplit(')', 1)[1].split()[0] == 'Z')

    def test_detached_child_cannot_inherit_supervisor_slot_locks(self):
        marker = self.root / 'detached'
        code = f'import subprocess,json;from pathlib import Path;p=subprocess.Popen(["/usr/bin/sleep","60"],start_new_session=True,stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL);s=Path(f"/proc/{{p.pid}}/stat").read_text().rsplit(")",1)[1].split()[19];Path({str(marker)!r}).write_text(json.dumps({{"pid":p.pid,"start":s}}))'
        fd = None
        try:
            r = self.call('--', sys.executable, '-c', code)
            self.assertEqual(r.returncode, 0, r.stderr)
            data = json.loads(marker.read_text())
            fd = os.pidfd_open(data['pid'])
            self.assertEqual(Path(f"/proc/{data['pid']}/stat").read_text().rsplit(')', 1)[1].split()[19], data['start'])
            # The deliberately detached fixture is alive; a new job must nevertheless get both locks.
            r = self.call('--queue-timeout', '.1', '--', sys.executable, '-c', 'pass')
            self.assertEqual(r.returncode, 0, r.stderr)
        finally:
            if fd is None and marker.exists():
                data = json.loads(marker.read_text())
                try:
                    candidate = os.pidfd_open(data['pid'])
                    actual = Path(f"/proc/{data['pid']}/stat").read_text().rsplit(')', 1)[1].split()[19]
                    if actual == data['start']:
                        fd = candidate
                    else:
                        os.close(candidate)
                except ProcessLookupError:
                    pass
            if fd is not None:
                try:
                    signal.pidfd_send_signal(fd, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                os.close(fd)

    def test_oversubscribed_cargo_and_nonfinite_deadlines_are_refused(self):
        for args in [('cargo', 'test', '-j2'), ('cargo', 'build', '--jobs', '8')]:
            r = self.call('--', *args)
            self.assertEqual(r.returncode, 75)
            self.assertIn('-j1', r.stderr)
        for value in ('nan', 'inf', '-1'):
            self.assertEqual(self.call('--timeout', value, '--', sys.executable, '-c', 'pass').returncode, 75)

    def test_symlinked_lock_is_refused(self):
        self.slots.mkdir()
        target = self.root / 'foreign'
        target.write_text('leave alone')
        (self.slots / 'slot1.lock').symlink_to(target)
        self.assertEqual(self.call('--', sys.executable, '-c', 'pass').returncode, 75)
        self.assertEqual(target.read_text(), 'leave alone')


if __name__ == '__main__':
    unittest.main()
