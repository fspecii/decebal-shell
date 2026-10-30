#!/usr/bin/env python3
"""Headless real-CLI recovery checks. TIMER_BIN must point at a built dectimer."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest

BIN = Path(os.environ.get('TIMER_BIN', 'target/debug/dectimer')).resolve()


class Recovery(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='decebal-timer-recovery-')
        self.root = Path(self.tmp.name)
        self.state = self.root / 'state/decebal/timer'
        self.log = self.root / 'notifications'
        tools = self.root / 'bin'
        tools.mkdir()
        notifier = tools / 'notify-send'
        notifier.write_text('#!/bin/sh\nprintf "%s\\n" "$*" >> "$TIMER_NOTIFY_LOG"\n')
        notifier.chmod(0o700)
        self.env = dict(os.environ, HOME=str(self.root), XDG_STATE_HOME=str(self.root / 'state'),
                        XDG_CONFIG_HOME=str(self.root / 'config'), XDG_DATA_HOME=str(self.root / 'data'),
                        XDG_RUNTIME_DIR=str(self.root / 'run'), DBUS_SESSION_BUS_ADDRESS='unix:path=/nonexistent',
                        PATH=str(tools), TIMER_NOTIFY_LOG=str(self.log))
        self.env.pop('DISPLAY', None)
        self.env.pop('WAYLAND_DISPLAY', None)
        self.children = []

    def tearDown(self):
        for child in self.children:
            if child.poll() is None:
                child.kill()
            child.communicate(timeout=5)
        self.tmp.cleanup()

    def seed(self, status='Running', deadline=None, claimed=False):
        self.state.mkdir(parents=True, exist_ok=True, mode=0o700)
        self.state.chmod(0o700)
        value = dict(status=status, duration=10, remaining=0 if status == 'Complete' else 10, title='Recovery fixture',
                     deadline=deadline if deadline is not None else (int(time.time()) - 1 if status == 'Running' else None),
                     claimed=claimed, notification_error=None, version=1)
        self.file = self.state / 'timer.json'
        self.file.write_text(json.dumps(value))
        self.file.chmod(0o600)

    def run_cli(self, *args):
        return subprocess.run([str(BIN), *args], env=self.env, capture_output=True, timeout=8)

    def start(self, *args):
        child = subprocess.Popen([str(BIN), *args], env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.children.append(child)
        return child

    def wait(self, condition):
        until = time.monotonic() + 5
        while time.monotonic() < until:
            if condition():
                return
            time.sleep(.02)
        self.fail('private timer condition timed out')

    def test_missing_state_exits_without_creating_state_or_window(self):
        result = self.run_cli('autostart')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.state.exists())

    def test_idle_paused_claimed_state_is_readonly(self):
        for status, claimed in [('Ready', False), ('Paused', False), ('Complete', True)]:
            with self.subTest(status=status):
                self.seed(status=status, claimed=claimed)
                before = self.file.read_bytes()
                self.assertEqual(self.run_cli('autostart').returncode, 0)
                self.assertEqual(self.file.read_bytes(), before)
                self.assertFalse((self.state / 'worker.lock').exists())
                self.assertFalse(self.log.exists())

    def test_corruption_and_bad_arguments_fail_without_overwrite(self):
        self.seed()
        self.file.write_text('invalid saved state')
        out = self.run_cli('autostart')
        self.assertEqual(out.returncode, 1)
        self.assertIn(b'Corrupt', out.stderr)
        self.assertEqual(self.file.read_text(), 'invalid saved state')
        self.assertEqual(self.run_cli('autostart', 'unexpected').returncode, 1)

    def test_elapsed_timer_delivers_once_across_repeated_logins(self):
        self.seed()
        for _ in range(3):
            out = self.run_cli('autostart')
            self.assertEqual(out.returncode, 0, out.stderr)
        self.assertEqual(len(self.log.read_text().splitlines()), 1)
        saved = json.loads(self.file.read_text())
        self.assertEqual(saved['status'], 'Complete')
        self.assertTrue(saved['claimed'])

    def test_logout_recovery_and_duplicate_normal_worker_have_one_owner(self):
        self.seed(deadline=int(time.time()) + 4)
        old = self.start('--timer-worker', str(self.state))
        self.wait(lambda: (self.state / 'worker.lock').exists())
        time.sleep(.1)
        old.terminate()
        old.wait(timeout=3)
        # Model a new login plus a duplicate login and normal UI's worker spawn.
        workers = [self.start('autostart'), self.start('autostart'), self.start('--timer-worker', str(self.state))]
        for child in workers:
            stdout, stderr = child.communicate(timeout=8)
            self.assertEqual(child.returncode, 0, (stdout, stderr))
        self.assertEqual(len(self.log.read_text().splitlines()), 1)
        self.assertTrue(json.loads(self.file.read_text())['claimed'])

    def test_missing_notifier_failure_is_durable_and_not_retried(self):
        self.seed()
        self.env['PATH'] = str(self.root / 'missing-tools')
        out = self.run_cli('autostart')
        self.assertEqual(out.returncode, 0, out.stderr)
        saved = json.loads(self.file.read_text())
        self.assertTrue(saved['claimed'])
        self.assertIn('Notification unavailable', saved['notification_error'])
        before = self.file.read_bytes()
        self.assertEqual(self.run_cli('autostart').returncode, 0)
        self.assertEqual(self.file.read_bytes(), before)


if __name__ == '__main__':
    if not BIN.is_file():
        raise SystemExit('Build dectimer or set TIMER_BIN before running this test')
    unittest.main()
