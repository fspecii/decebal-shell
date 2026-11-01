#!/usr/bin/env python3
import hashlib
import importlib.util
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('health', Path(__file__).resolve().parents[1] / 'tools/dev/shell_health.py')
h = importlib.util.module_from_spec(spec)
spec.loader.exec_module(h)


class HealthTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.proc = self.root / '123'
        self.proc.mkdir()
        self.binary = self.root / 'decbar'
        self.binary.write_bytes(b'test executable')
        (self.proc / 'exe').symlink_to(self.binary)
        self.write_stat()
        self.entry = dict(name='decbar', pid=123, target=str(self.binary), starttime='456',
                          sha256=hashlib.sha256(self.binary.read_bytes()).hexdigest())

    def write_stat(self, state='S', start='456'):
        fields = [state] + ['0'] * 18 + [start]
        (self.proc / 'stat').write_text('123 (name with ) parentheses) ' + ' '.join(fields))

    def check(self):
        return h.inspect_component(self.entry, self.root)['state']

    def test_pinned_process_is_running(self):
        self.assertEqual(self.check(), 'running')

    def test_missing_process(self):
        self.entry['pid'] = 999
        self.assertEqual(self.check(), 'absent')

    def test_reused_pid_never_counts_as_running(self):
        self.write_stat(start='789')
        self.assertEqual(self.check(), 'pid-reused')

    def test_unpinned_receipt_is_explicitly_inconclusive(self):
        del self.entry['starttime']
        self.assertEqual(self.check(), 'running-unpinned')

    def test_zombie_is_not_running(self):
        self.write_stat(state='Z')
        self.assertEqual(self.check(), 'exited')

    def test_foreign_owner_is_not_inspected(self):
        self.assertEqual(h.inspect_component(self.entry, self.root, os.getuid() + 1)['state'], 'foreign-owner')

    def test_changed_executable_and_bytes(self):
        self.entry['target'] = '/another/program'
        self.assertEqual(self.check(), 'executable-changed')
        self.entry['target'] = str(self.binary)
        self.binary.write_bytes(b'changed')
        self.assertEqual(self.check(), 'binary-changed')

    def test_malformed_stat_is_unknown(self):
        (self.proc / 'stat').write_text('broken')
        self.assertEqual(self.check(), 'unknown')

    def test_untrusted_dock_probe_never_executes(self):
        with patch.object(h.subprocess, 'run') as run:
            with self.assertRaises(ValueError):
                h.dock_wanted(self.binary, 'wrong')
            run.assert_not_called()


if __name__ == '__main__':
    unittest.main()
