#!/usr/bin/env python3
"""Registry-fixture regressions; no GTK, bus, MIME utility or application is launched."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('openfix', Path(__file__).with_name('openfix.py'))
registry = importlib.util.module_from_spec(spec)
spec.loader.exec_module(registry)

class RegistryTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='decfiles-registry-unit-', dir='/var/tmp')
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        for name in ('data', 'config', 'system-mime'):
            (self.root / name).mkdir()
        self.session = SimpleNamespace(root=self.root, env={
            'XDG_DATA_HOME': str(self.root / 'data'), 'XDG_CONFIG_HOME': str(self.root / 'config'),
            'XDG_DATA_DIRS': '/usr/share:/usr/local/share', 'XDG_CONFIG_DIRS': '/etc/xdg'})

    def make(self, stdout='No default applications for application/x-decfiles-none'):
        result = subprocess.CompletedProcess([], 0, stdout=stdout, stderr='')
        with patch.object(registry, 'SYSTEM_MIME', self.root / 'system-mime'), patch.object(registry.subprocess, 'run', return_value=result) as run:
            value = registry.Registry(self.session)
        return value, run.call_args_list

    def test_host_applications_are_hidden_and_b_is_explicitly_associated(self):
        value, calls = self.make()
        self.assertEqual(self.session.env['XDG_DATA_DIRS'], str(self.root / 'registry-data'))
        self.assertEqual(self.session.env['XDG_CONFIG_DIRS'], str(self.root / 'registry-config'))
        self.assertTrue((self.root / 'registry-data/mime').is_symlink())
        self.assertFalse((self.root / 'registry-data/applications').exists())
        mimeapps = (value.config / 'mimeapps.list').read_text()
        self.assertIn('[Added Associations]', mimeapps)
        self.assertIn('decfiles-fake-a.desktop;decfiles-fake-b.desktop;', mimeapps)
        self.assertEqual(calls[-1].args[0], ['gio', 'mime', 'application/x-decfiles-none'])
        self.assertEqual(calls[-1].kwargs['env']['LC_ALL'], 'C')
        value.unchanged()

    def test_unexpected_default_is_rejected_before_any_native_app_launch(self):
        with self.assertRaisesRegex(AssertionError, 'no-handler precondition failed'):
            self.make('Default application for application/x-decfiles-none: foreign.desktop')

    def test_default_or_association_mutation_is_detected(self):
        value, _ = self.make()
        with (value.config / 'mimeapps.list').open('a') as stream:
            stream.write('\napplication/octet-stream=other.desktop;\n')
        with self.assertRaises(AssertionError):
            value.unchanged()

if __name__ == '__main__':
    unittest.main()
