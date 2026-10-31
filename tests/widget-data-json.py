#!/usr/bin/env python3
"""Private-HOME fixtures for `decwidgets data --json` (schema decebal-widget-data/1).
DECWIDGETS_BIN must point at a built decwidgets. Every case runs in an env-from-nothing
private HOME/XDG with invalid buses and a recording notify-send, and asserts that the
whole private tree is unchanged afterwards (no state, lock, claim, log or migration)."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest

BIN = Path(os.environ.get('DECWIDGETS_BIN', 'target/debug/decwidgets')).resolve()
NOTE_ID = '0123456789abcdef0123456789abcd{:02x}'


def tree(root):
    return {str(p.relative_to(root)): (p.stat().st_mode, p.stat().st_size, p.stat().st_mtime_ns, p.read_bytes() if p.is_file() else None)
            for p in sorted(root.rglob('*')) if not p.is_symlink()}


class DataJson(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='decwidgets-data-json-')
        self.root = Path(self.tmp.name)
        tools = self.root / 'bin'
        tools.mkdir()
        (tools / 'notify-send').write_text('#!/bin/sh\necho "$*" >> "$HOME/notified"\n')
        (tools / 'notify-send').chmod(0o700)
        self.env = {'PATH': f'{tools}:/usr/bin:/bin', 'LANG': 'C.UTF-8', 'HOME': str(self.root / 'home'),
                    'XDG_DATA_HOME': str(self.root / 'data'), 'XDG_STATE_HOME': str(self.root / 'state'),
                    'XDG_CONFIG_HOME': str(self.root / 'config'), 'XDG_RUNTIME_DIR': str(self.root / 'run'),
                    'DBUS_SESSION_BUS_ADDRESS': 'unix:path=/nonexistent', 'DBUS_SYSTEM_BUS_ADDRESS': 'unix:path=/nonexistent'}
        (self.root / 'home').mkdir()

    def tearDown(self):
        self.tmp.cleanup()

    def data(self, *args, code=0):
        before = tree(self.root)
        run = subprocess.run([str(BIN), 'data', '--json', *args], env=self.env, capture_output=True, text=True, timeout=10)
        self.assertEqual(run.returncode, code, run.stderr)
        self.assertEqual(tree(self.root), before, 'decwidgets data --json wrote to the private tree')
        self.assertFalse((self.root / 'home/notified').exists(), 'a notification was sent')
        if code:
            return run.stderr
        self.assertLess(len(run.stdout), 4096, 'output is not bounded')
        out = json.loads(run.stdout)
        self.assertEqual((out['schema'], out['snapshot'], out['live']), ('decebal-widget-data/1', True, False))
        return out

    def timer(self, status, duration, remaining, deadline=None, title='Focus', claimed=False, mode=0o600):
        folder = self.root / 'state/decebal/timer'
        folder.mkdir(parents=True, exist_ok=True)
        folder.chmod(0o700)
        value = dict(status=status, duration=duration, remaining=remaining, title=title, deadline=deadline,
                     claimed=claimed, notification_error=None, version=1)
        path = folder / 'timer.json'
        path.write_text(json.dumps(value))
        path.chmod(mode)
        return path

    def note(self, index, title, body, updated_ms):
        folder = self.root / 'data/decebal/notes'
        folder.mkdir(parents=True, exist_ok=True)
        folder.chmod(0o700)
        note_id = NOTE_ID.format(index)
        (folder / f'{note_id}.json').write_text(json.dumps(dict(id=note_id, revision=1, updated_ms=updated_ms, title=title, body=body)))
        return folder

    def test_empty_home_reports_defaults_and_creates_nothing(self):
        out = self.data()
        self.assertEqual(out['groups'], ['notes', 'timer'])
        self.assertEqual(out['fields']['notes.count'], 0)
        self.assertEqual(out['fields']['timer.status'], 'Ready')
        self.assertFalse((self.root / 'state').exists() or (self.root / 'data').exists(), 'state or data folders were created')

    def test_timer_running_paused_and_expired_are_resolved_without_claiming(self):
        title = 'Focus {cpu.usage} "quoted" <b>'
        self.timer('Running', 300, 300, deadline=int(time.time()) + 125, title=title)
        fields = self.data('--group', 'timer')['fields']
        self.assertEqual(set(fields), {'timer.remaining', 'timer.title', 'timer.status', 'timer.progress'})
        self.assertEqual((fields['timer.status'], fields['timer.title']), ('Running', title))
        self.assertIn(fields['timer.remaining'], {'00:02:05', '00:02:04', '00:02:03'})
        self.assertAlmostEqual(fields['timer.progress'], 1 - 125 / 300, delta=0.02)
        self.timer('Paused', 300, 90, title='Paused task')
        fields = self.data('--group', 'timer')['fields']
        self.assertEqual((fields['timer.status'], fields['timer.remaining'], fields['timer.title']), ('Paused', '00:01:30', 'Paused task'))
        self.assertAlmostEqual(fields['timer.progress'], 0.7)
        state = self.timer('Running', 60, 60, deadline=int(time.time()) - 5, title='Expired task')
        raw = state.read_bytes()
        fields = self.data('--group', 'timer')['fields']
        self.assertEqual(fields['timer.remaining'], '00:00:00')
        self.assertIn(fields['timer.status'], {'Complete', 'Running'})
        self.assertEqual(state.read_bytes(), raw, 'an expired timer was claimed or rewritten')

    def test_unsafe_timer_state_is_reported_not_repaired(self):
        state = self.timer('Running', 60, 60, deadline=int(time.time()) + 30, mode=0o644)
        fields = self.data('--group', 'timer')['fields']
        self.assertEqual(fields['timer.status'], 'Timer unavailable')
        self.assertEqual(state.stat().st_mode & 0o777, 0o644)

    def test_notes_counts_corrupt_status_and_literal_bounded_content(self):
        now = int(time.time() * 1000)
        self.note(1, 'Older', 'older body', now - 2000)
        self.note(2, 'Middle', 'middle body', now - 1000)
        latest_title = '{cpu.usage} "q" <b>' + 'T' * 60
        folder = self.note(3, latest_title, 'Line one\n\n  {clock:%H}   ' + 'x' * 400, now)
        (folder / (NOTE_ID.format(4) + '.json')).write_text('{not json')
        (folder / 'README.txt').write_text('not a note')
        fields = self.data('--group', 'notes')['fields']
        self.assertEqual(fields['notes.count'], 3)
        self.assertEqual(fields['notes.status'], 'Some unreadable notes preserved')
        self.assertTrue(fields['notes.title'].startswith('{cpu.usage} "q" <b>'), fields['notes.title'])
        self.assertLessEqual(len(fields['notes.title']), 39)
        self.assertTrue(fields['notes.excerpt'].startswith('Line one {clock:%H} x'), fields['notes.excerpt'])
        self.assertLessEqual(len(fields['notes.excerpt']), 111)
        self.assertNotIn('timer.status', fields)
        self.note(5, '   ', 'untitled body', now + 1000)
        self.assertEqual(self.data('--group', 'notes')['fields']['notes.title'], 'Untitled note')

    def test_errors_are_explicit(self):
        self.assertIn('unsupported data group "cpu"', self.data('--group', 'cpu', code=1))
        self.assertIn('--group needs', self.data('--group', code=1))
        self.assertIn('unexpected argument', self.data('--live', code=1))


if __name__ == '__main__':
    unittest.main()
