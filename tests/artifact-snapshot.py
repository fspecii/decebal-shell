#!/usr/bin/env python3
"""Temporary files only; no builds, desktop, network or child processes."""
import importlib.util
import contextlib
import io
import stat
import sys
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('artifact_snapshot', Path(__file__).resolve().parents[1] / 'tools/dev/artifact_snapshot.py')
a = importlib.util.module_from_spec(spec)
spec.loader.exec_module(a)


class Snapshot(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / 'bin').mkdir()
        (self.root / 'bin/app').write_bytes(b'candidate')

    def test_round_trip_and_changed_bytes(self):
        manifest = a.snapshot(self.root, ['bin/app'])
        self.assertEqual(a.verify(self.root, manifest), 1)
        (self.root / 'bin/app').write_bytes(b'different')
        with self.assertRaises(ValueError):
            a.verify(self.root, manifest)

    def test_relative_normalized_paths_only(self):
        for path in ('', '../secret', '/etc/passwd', 'bin/../app', './bin/app', 'bin//app', 'bin/app/'):
            with self.subTest(path=path), self.assertRaises(ValueError):
                a.snapshot(self.root, [path])

    def test_symlinks_in_any_component_are_refused(self):
        (self.root / 'link').symlink_to('bin', target_is_directory=True)
        (self.root / 'bin/link').symlink_to('app')
        for path in ('link/app', 'bin/link'):
            with self.subTest(path=path), self.assertRaises(OSError):
                a.snapshot(self.root, [path])

    def test_fifo_does_not_block(self):
        os.mkfifo(self.root / 'pipe')
        with self.assertRaises(ValueError):
            a.snapshot(self.root, ['pipe'])

    def test_size_and_count_budgets(self):
        with patch.object(a, 'MAX_FILE_BYTES', 2), self.assertRaises(ValueError):
            a.snapshot(self.root, ['bin/app'])
        for paths in ([], ['bin/app', 'bin/app'], ['x'] * 257):
            with self.assertRaises(ValueError):
                a.snapshot(self.root, paths)

    def test_replacement_during_hash_is_refused(self):
        original = a.os.read
        replaced = False
        def read(fd, n):
            nonlocal replaced
            data = original(fd, n)
            if data and not replaced:
                replacement = self.root / 'new'
                replacement.write_bytes(data)
                replacement.replace(self.root / 'bin/app')
                replaced = True
            return data
        with patch.object(a.os, 'read', read), self.assertRaises(ValueError):
            a.snapshot(self.root, ['bin/app'])

    def test_cli_creates_private_manifest_and_never_overwrites_it(self):
        output = self.root / 'receipt.json'
        argv = ['snapshot', '--root', str(self.root), '--output', str(output), 'bin/app']
        with patch.object(sys, 'argv', ['artifact_snapshot.py', *argv]), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(a.main(), 0)
        self.assertEqual(stat.S_IMODE(output.stat().st_mode), 0o600)
        original = output.read_bytes()
        with patch.object(sys, 'argv', ['artifact_snapshot.py', *argv]), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(a.main(), 1)
        self.assertEqual(output.read_bytes(), original)
        with patch.object(sys, 'argv', ['artifact_snapshot.py', 'verify', '--root', str(self.root), '--manifest', str(output)]), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(a.main(), 0)

    def test_invalid_manifests(self):
        valid = a.snapshot(self.root, ['bin/app'])
        for doc in ({}, {'format': 2}, {'format': 1, 'files': []}, {'format': 1, 'files': valid['files'] * 2}):
            with self.assertRaises(ValueError):
                a.verify(self.root, doc)

    def test_manifest_reader_refuses_links_and_special_files(self):
        (self.root / 'manifest').write_text('{"format":1}')
        (self.root / 'link').symlink_to('manifest')
        self.assertEqual(a.load_manifest(self.root / 'manifest'), {'format': 1})
        with self.assertRaises(OSError):
            a.load_manifest(self.root / 'link')
        os.mkfifo(self.root / 'pipe')
        with self.assertRaises(ValueError):
            a.load_manifest(self.root / 'pipe')


if __name__ == '__main__':
    unittest.main()
