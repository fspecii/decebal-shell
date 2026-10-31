#!/usr/bin/env python3
"""Owned high-display collision, readiness, timeout, and cleanup checks."""
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
sys.path.insert(0, str(Path(__file__).resolve().parent))
import private_x11 as fixture


class PrivateX11(unittest.TestCase):
    def setUp(self):
        self.root = tempfile.TemporaryDirectory(prefix='private-x11-test-')
        root = Path(self.root.name)
        self.env = {'PATH': os.defpath, 'HOME': str(root), 'DBUS_SESSION_BUS_ADDRESS': 'unix:path='+str(root/'no-bus'), 'DBUS_SYSTEM_BUS_ADDRESS': 'unix:path='+str(root/'no-system-bus')}
        for name in ('CONFIG', 'DATA', 'CACHE', 'STATE', 'RUNTIME'):
            path = root/name; path.mkdir(mode=0o700)
            self.env['XDG_'+name+'_HOME' if name != 'RUNTIME' else 'XDG_RUNTIME_DIR'] = str(path)
        self.number = next(n for n in range(51000, 60000) if fixture.available(n))
        self.log = open(os.devnull, 'w')

    def tearDown(self):
        fixture._cleanup(); self.log.close(); self.root.cleanup()

    def test_collisions(self):
        n = self.number
        path = Path(f'/tmp/.X11-unix/X{n}')
        lock = Path(f'/tmp/.X{n}-lock')
        for mode in ('filesystem', 'abstract', 'lock', 'symlink'):
            server = None
            try:
                if mode in ('filesystem', 'abstract'):
                    server = socket.socket(socket.AF_UNIX)
                    server.bind(str(path) if mode == 'filesystem' else '\0'+str(path))
                    server.listen()
                elif mode == 'lock': lock.write_text('foreign sentinel\n')
                else: lock.symlink_to('/nonexistent/private-x11-target')
                before = os.lstat(lock if mode in ('lock','symlink') else path) if mode != 'abstract' else None
                with patch.object(fixture.subprocess, 'Popen') as spawn:
                    with self.assertRaises(RuntimeError): fixture.start_private_xvfb(env=self.env, log=self.log, candidates=[n])
                    spawn.assert_not_called()
                if before: self.assertEqual(before.st_ino, os.lstat(lock if mode in ('lock','symlink') else path).st_ino)
                if mode == 'lock': self.assertEqual(lock.read_text(), 'foreign sentinel\n')
                if server: self.assertGreaterEqual(server.fileno(), 0)
            finally:
                if server: server.close()
                if mode == 'filesystem': path.unlink()
                if mode in ('lock','symlink'): lock.unlink()

    def test_failure_and_timeout(self):
        for executable in ('/bin/false', '/bin/sleep'):
            with self.assertRaises(RuntimeError):
                fixture.start_private_xvfb(env=self.env, log=self.log, candidates=[self.number], executable=executable, timeout=.1)
            self.assertFalse(fixture._owned)
        # An actual long-lived fake child exercises timeout termination/reaping.
        with patch.object(fixture, 'listeners_owned', return_value=False):
            with self.assertRaises(RuntimeError):
                fixture.start_private_xvfb(env=self.env, log=self.log, candidates=[self.number], timeout=.1)
        self.assertFalse(fixture._owned)

    def test_signal_cleanup(self):
        code = ("import sys,os,signal;sys.path.insert(0,sys.argv[1]);"
                "import private_x11 as f;"
                "p,d=f.start_private_xvfb(env=dict(os.environ),log=open(os.devnull,'w'),candidates=[int(sys.argv[2])]);"
                "os.kill(os.getpid(),signal.SIGTERM)")
        result = subprocess.run([sys.executable, '-I', '-c', code, str(Path(__file__).resolve().parent), str(self.number)], env=self.env, timeout=8)
        self.assertEqual(result.returncode, 128+signal.SIGTERM)
        self.assertTrue(fixture.available(self.number))

    def test_real_smoke(self):
        child, display = fixture.start_private_xvfb(env=self.env, log=self.log, candidates=[self.number])
        self.assertEqual(display, ':'+str(self.number))
        self.assertTrue(fixture.listeners_owned(child, self.number))
        env = dict(self.env, DISPLAY=display)
        subprocess.run(['xdotool', 'getdisplaygeometry'], env=env, check=True, stdout=subprocess.DEVNULL, timeout=3)
        fixture.close(child)
        self.assertIsNotNone(child.returncode)
        self.assertTrue(fixture.available(self.number))
        fixture.close(child)


if __name__ == '__main__': unittest.main()
