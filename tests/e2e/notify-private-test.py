#!/usr/bin/env python3
"""Pure, mocked checks of tests/e2e/notify_private.py and of the two scripts that use it.

No child process, X server, bus or GUI is started: signals, pidfds and process groups are fakes, sockets are local
socket objects of this process, and the filesystem is a temporary directory.

    python3 tests/e2e/notify-private-test.py
"""
import contextlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import signal
import socket
import sys
import tempfile
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import notify_private as np


class FakeProc:
    def __init__(self, pid):
        self.pid, self.returncode = pid, None

    def poll(self):
        return self.returncode

    def wait(self, timeout=None):
        if self.returncode is None:
            self.returncode = 0
        return self.returncode


class FakeKernel:
    """pidfd/process-group fakes. A pid exits on SIGTERM unless it is listed as stubborn."""

    def __init__(self, stubborn=(), fail_signal=()):
        self.stubborn, self.fail_signal = set(stubborn), set(fail_signal)
        self.sent, self.groups, self.order, self.exited = [], [], [], set()
        self.fds = {}

    def pidfd_open(self, pid):
        self.fds[1000 + pid] = pid
        return 1000 + pid

    def pidfd_send_signal(self, fd, signum):
        pid = self.fds[fd]
        if pid in self.fail_signal:
            raise PermissionError(1, 'Operation not permitted')
        self.sent.append((pid, signum))
        if signum == signal.SIGKILL or (signum == signal.SIGTERM and pid not in self.stubborn):
            self.exited.add(pid)

    def killpg(self, pgid, signum):
        self.groups.append((pgid, signum))

    def waitid(self, kind, fd, options):
        return object() if self.fds[fd] in self.exited else None

    def patches(self):
        return (patch.object(np.os, 'pidfd_open', self.pidfd_open),
                patch.object(np.signal, 'pidfd_send_signal', self.pidfd_send_signal),
                patch.object(np.os, 'killpg', self.killpg),
                patch.object(np.os, 'getpgid', lambda pid: pid),
                patch.object(np.os, 'getpgrp', lambda: 1),
                patch.object(np.os, 'waitid', self.waitid),
                patch.object(np.os, 'close', lambda fd: None),
                patch.object(np, 'group_members', lambda pgid: []))


@contextlib.contextmanager
def faked(kernel):
    with contextlib.ExitStack() as stack:
        for item in kernel.patches():
            stack.enter_context(item)
        yield


def ledger_with(kernel, pids):
    ledger = np.Ledger()
    children = [ledger.adopt(FakeProc(pid), f'child{pid}') for pid in pids]
    return ledger, children


class Environment(unittest.TestCase):
    def test_nothing_is_inherited_and_buses_are_dead(self):
        root = Path(tempfile.mkdtemp(prefix='np-env-'))
        self.addCleanup(shutil.rmtree, root)
        polluted = {'DISPLAY': ':0', 'WAYLAND_DISPLAY': 'wayland-1', 'DBUS_SESSION_BUS_ADDRESS': 'unix:path=/run/user/1/bus',
                    'XDG_SESSION_TYPE': 'wayland', 'DECEBAL_BACKEND': 'wayland', 'DECEBAL_SYSTEM_SYNC': '1',
                    'RDWM_SOCK': '/run/live', 'PULSE_SERVER': 'unix:/run/pulse', 'SECRET_TOKEN': 'do-not-leak', 'HOME': '/home/live'}
        with patch.dict(os.environ, polluted):
            env = np.private_environment(root, ['/abs/bin'], 'cfg/omarchy', {'DECNOTIFY_FORCE': '1'})
        for key in ('DISPLAY', 'WAYLAND_DISPLAY', 'XAUTHORITY', 'XDG_SESSION_TYPE', 'DECEBAL_SYSTEM_SYNC', 'RDWM_SOCK', 'PULSE_SERVER', 'SECRET_TOKEN'):
            self.assertNotIn(key, env)
        self.assertEqual(env['DECEBAL_SYSTEM_AUTOMATIC'], '0')
        self.assertEqual(env['DECNOTIFY_FORCE'], '1')
        self.assertTrue(env['PATH'].startswith('/abs/bin' + os.pathsep))
        for key in ('HOME', 'XDG_CONFIG_HOME', 'XDG_RUNTIME_DIR', 'XDG_STATE_HOME', 'XDG_DATA_HOME', 'XDG_CACHE_HOME', 'DECEBAL_CONFIG', 'DECEBAL_THEMES', 'DECEBAL_SYSTEM_THEMES', 'TMPDIR'):
            self.assertTrue(Path(env[key]).is_relative_to(root), key)
            self.assertEqual(Path(env[key]).stat().st_mode & 0o777, 0o700, key)
        for key in ('DBUS_SESSION_BUS_ADDRESS', 'DBUS_SYSTEM_BUS_ADDRESS'):
            self.assertTrue(env[key].startswith('unix:path=' + str(root)), key)
            self.assertFalse(Path(env[key][10:]).exists(), key)
        self.assertNotIn('do-not-leak', json.dumps(env))

    def test_explicit_directories_have_no_default(self):
        with patch.dict(os.environ, {}, clear=True):
            with self.assertRaises(np.Usage):
                np.explicit_directory('MS_BIN_DIR')
        with patch.dict(os.environ, {'MS_BIN_DIR': 'target/debug'}):
            with self.assertRaises(np.Usage):
                np.explicit_directory('MS_BIN_DIR')
        with patch.dict(os.environ, {'MS_BIN_DIR': '/abs/bin'}):
            self.assertEqual(np.explicit_directory('MS_BIN_DIR'), Path('/abs/bin'))

    def test_display_namespace(self):
        self.assertIsNone(np.parse_display(None))
        self.assertIsNone(np.parse_display(''))
        self.assertEqual(np.parse_display(':700'), 700)
        self.assertEqual(np.parse_display(':65535'), 65535)
        for bad in (':0', ':348', ':699', ':65536', '700', ':x', ':7 00', ':700.0'):
            with self.assertRaises(np.Usage, msg=bad):
                np.parse_display(bad)


class SocketCredentials(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp(prefix='np-sock-'))
        self.addCleanup(shutil.rmtree, self.dir)
        self.path = self.dir / 'bus'
        self.server = socket.socket(socket.AF_UNIX)
        self.addCleanup(self.server.close)
        self.server.bind(str(self.path))
        self.server.listen()

    def test_peer_credentials_must_name_the_exact_child(self):
        mine = FakeProc(os.getpid())
        self.assertTrue(np.socket_owned(mine, self.path))
        self.assertFalse(np.socket_owned(FakeProc(os.getpid() + 1), self.path), 'a different pid must not own the socket')
        exited = FakeProc(os.getpid())
        exited.returncode = 0
        self.assertFalse(np.socket_owned(exited, self.path))
        self.assertFalse(np.socket_owned(mine, self.dir / 'missing'))
        (self.dir / 'plain').write_text('')
        self.assertFalse(np.socket_owned(mine, self.dir / 'plain'), 'a regular file is not a socket')


class Signals(unittest.TestCase):
    def test_immediate_raise_and_deferral(self):
        guard = np.SignalGuard()
        with self.assertRaises(np.Interrupted) as raised:
            guard.handler(signal.SIGTERM, None)
        self.assertEqual(raised.exception.signum, signal.SIGTERM)
        with self.assertRaises(np.Interrupted) as raised:
            with guard.deferred():
                with guard.deferred():
                    guard.handler(signal.SIGTERM, None)
                    guard.handler(signal.SIGINT, None)
                self.assertEqual(guard.pending, signal.SIGTERM, 'nested exit must not release early')
        self.assertEqual(raised.exception.signum, signal.SIGTERM)
        self.assertIsNone(guard.pending)
        with guard.deferred():
            pass

    def test_interrupted_is_not_swallowed_by_exception_handlers(self):
        self.assertFalse(issubclass(np.Interrupted, Exception))
        self.assertFalse(issubclass(np.OwnershipLost, np.RETRYABLE))
        with self.assertRaises(np.OwnershipLost):
            np.wait(lambda: (_ for _ in ()).throw(np.OwnershipLost('x')), 'never', 1)
        with self.assertRaises(AssertionError) as raised:
            np.wait(lambda: False, 'timed out', .05)
        self.assertIn('timed out', str(raised.exception))


class LedgerCleanup(unittest.TestCase):
    def test_all_children_are_signalled_together_before_waiting(self):
        kernel = FakeKernel()
        with faked(kernel):
            ledger, children = ledger_with(kernel, [11, 12, 13])
            ledger.stop_all(grace=1, kill_wait=1)
        terms = [(pid, sig) for pid, sig in kernel.sent if sig == signal.SIGTERM]
        self.assertEqual({pid for pid, _ in terms}, {11, 12, 13})
        self.assertFalse([1 for _, sig in kernel.sent if sig == signal.SIGKILL])
        self.assertEqual(ledger.failures, [])
        self.assertEqual(ledger.forced, [])
        self.assertTrue(all(child.closed and child.proc.returncode is not None for child in children))
        self.assertEqual({pgid for pgid, _ in kernel.groups}, {11, 12, 13})

    def test_ignoring_sigterm_is_killed_reaped_and_recorded(self):
        kernel = FakeKernel(stubborn={12})
        with faked(kernel):
            ledger, children = ledger_with(kernel, [11, 12])
            ledger.stop_all(grace=.05, kill_wait=1)
        self.assertIn((12, signal.SIGKILL), kernel.sent)
        self.assertNotIn((11, signal.SIGKILL), kernel.sent)
        self.assertEqual(ledger.forced, ['child12'])
        self.assertTrue(any('child12' in failure and 'killed' in failure for failure in ledger.failures))
        self.assertTrue(all(child.closed and child.proc.returncode is not None for child in children))

    def test_cleanup_continues_after_one_error(self):
        kernel = FakeKernel(fail_signal={11})
        with faked(kernel):
            ledger, children = ledger_with(kernel, [11, 12, 13])
            ledger.stop_all(grace=.05, kill_wait=1)
        self.assertTrue(any('child11' in failure for failure in ledger.failures))
        self.assertTrue({12, 13} <= {pid for pid, sig in kernel.sent if sig == signal.SIGTERM})
        self.assertTrue(all(child.closed for child in children), 'every child is reaped even after an error')

    def test_never_signals_a_reaped_group_or_our_own(self):
        kernel = FakeKernel()
        with faked(kernel):
            ledger, (reaped, leader) = ledger_with(kernel, [21, 22])
            reaped.proc.returncode = 0
            ledger.stop_all(grace=.5, kill_wait=1)
            self.assertNotIn(21, {pgid for pgid, _ in kernel.groups}, 'reaped leader: its pgid may belong to a stranger')
            self.assertIn(22, {pgid for pgid, _ in kernel.groups})
            kernel.groups.clear()
            with patch.object(np.os, 'getpgrp', lambda: 31):
                ledger2, _ = ledger_with(kernel, [31])
                ledger2.stop_all(grace=.5, kill_wait=1)
            self.assertEqual(kernel.groups, [], 'our own process group must never be signalled')
            kernel.groups.clear()
            with patch.object(np.os, 'getpgid', lambda pid: 1):
                ledger3, _ = ledger_with(kernel, [41])
                ledger3.stop_all(grace=.5, kill_wait=1)
            self.assertEqual(kernel.groups, [], 'a child that is not its own group leader must not be group-signalled')

    def test_descendants_outliving_the_leader_are_a_failure(self):
        kernel = FakeKernel()
        remaining = [[777]]
        def members(pgid):
            return list(remaining[0])
        def kill_group(pgid, signum):
            kernel.groups.append((pgid, signum))
            if signum == signal.SIGKILL:
                remaining[0] = []
        with faked(kernel), patch.object(np, 'group_members', members), patch.object(np.os, 'killpg', kill_group):
            ledger, _ = ledger_with(kernel, [51])
            ledger.stop_all(grace=.5, kill_wait=1)
        self.assertIn((51, signal.SIGKILL), kernel.groups)
        self.assertTrue(any('outlived the leader' in failure for failure in ledger.failures))

    def test_single_stop_leaves_other_children_alone(self):
        kernel = FakeKernel()
        with faked(kernel):
            ledger, (first, second) = ledger_with(kernel, [61, 62])
            ledger.stop([first], grace=.5, kill_wait=1)
        self.assertEqual({pid for pid, _ in kernel.sent}, {61})
        self.assertFalse(second.closed)

    def test_group_members_scans_proc_without_zombies_or_leader(self):
        proc = Path(tempfile.mkdtemp(prefix='np-proc-'))
        self.addCleanup(shutil.rmtree, proc)
        for pid, state, pgrp in ((100, 'S', 100), (101, 'S', 100), (102, 'Z', 100), (103, 'S', 999)):
            (proc / str(pid)).mkdir()
            (proc / str(pid) / 'stat').write_text(f'{pid} (name with ) paren) {state} 1 {pgrp} {pgrp} 0 0\n')
        (proc / 'self').mkdir()
        self.assertEqual(np.group_members(100, str(proc)), [101])


class SessionBehaviour(unittest.TestCase):
    def session(self, **options):
        defaults = dict(tools=(), binaries={}, evidence_env='NP_TEST_EVIDENCE', decebal_config='cfg/unit')
        defaults.update(options)
        return np.Session('np-unit', **defaults)

    def test_prerequisites_are_skips_not_passes(self):
        with patch.dict(os.environ, {}, clear=True):
            with self.assertRaises(np.Skip):
                self.session(tools=('definitely-not-a-real-tool-xyz',))
            with self.assertRaises(np.Skip):
                self.session(binaries={'decnotify': '/nonexistent/decnotify'})
            if os.getuid() == 0:
                with self.assertRaises(np.Skip):
                    self.session(require_non_root=True)
        with patch.dict(os.environ, {'E2E_DISPLAY': ':348'}):
            with self.assertRaises(np.Usage):
                self.session()
        with patch.dict(os.environ, {'E2E_DISPLAY': ':700'}), patch.object(np.private_x11, 'available', return_value=False):
            with self.assertRaises(np.Skip) as raised:
                self.session()
            self.assertIn('never adopted', str(raised.exception))

    def test_symlinked_binaries_are_refused(self):
        directory = Path(tempfile.mkdtemp(prefix='np-bin-'))
        self.addCleanup(shutil.rmtree, directory)
        real = directory / 'real'
        real.write_text('#!/bin/sh\n')
        real.chmod(0o700)
        link = directory / 'link'
        link.symlink_to(real)
        with patch.dict(os.environ, {}, clear=True):
            self.session(binaries={'real': real})
            with self.assertRaises(np.Skip):
                self.session(binaries={'link': link})

    def test_evidence_must_be_fresh(self):
        directory = Path(tempfile.mkdtemp(prefix='np-evidence-'))
        self.addCleanup(shutil.rmtree, directory)
        with patch.dict(os.environ, {'NP_TEST_EVIDENCE': str(directory)}):
            with self.assertRaises(np.Usage):
                self.session()
        with patch.dict(os.environ, {'NP_TEST_EVIDENCE': str(directory / 'new')}):
            self.session()

    def test_x_tools_reprove_display_ownership_before_running(self):
        session = self.session(tools=())
        session.tools = {'xdotool': '/usr/bin/xdotool', 'dbus-daemon': '/usr/bin/dbus-daemon'}
        session.xvfb, session.number = type('X', (), {'proc': object()})(), 701
        with patch.object(np.private_x11, 'listeners_owned', return_value=False) as owned, patch.object(np.subprocess, 'run') as run:
            with self.assertRaises(np.OwnershipLost):
                session.run('xdotool', 'getdisplaygeometry')
            owned.assert_called_once()
            run.assert_not_called()
        completed = type('R', (), {'stdout': ' 1280 900\n'})()
        with patch.object(np.private_x11, 'listeners_owned', return_value=True), patch.object(np.subprocess, 'run', return_value=completed) as run:
            self.assertEqual(session.run('xdotool', 'getdisplaygeometry'), '1280 900')
            self.assertEqual(run.call_args[0][0][0], '/usr/bin/xdotool', 'tools run by absolute path')
            self.assertNotIn('shell', run.call_args[1])
        with patch.object(np.private_x11, 'listeners_owned') as owned, patch.object(np.subprocess, 'run', return_value=completed):
            session.run('/abs/bin/decthemectl', 'get')
            owned.assert_not_called()

    def run_session(self, body, **options):
        session = self.session(**options)
        output = io.StringIO()
        error = None
        with contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
            try:
                with session:
                    body(session)
            except BaseException as caught:
                error = caught
        if session.root:
            self.addCleanup(shutil.rmtree, session.root, True)
        return session, error, output.getvalue()

    def test_success_removes_the_tree_but_failure_and_interrupt_retain_evidence(self):
        with patch.dict(os.environ, {}, clear=True):
            session, error, _ = self.run_session(lambda s: None)
            self.assertIsNone(error)
            self.assertFalse(session.root.exists())

            ran = []
            def failing(s):
                (s.root / 'daemon.log').write_text('log')
                s.before_cleanup(lambda: ran.append('hook'))
                raise AssertionError('scenario failed')
            session, error, text = self.run_session(failing)
            self.assertIsInstance(error, AssertionError)
            self.assertEqual(ran, ['hook'])
            self.assertTrue((session.evidence / 'cleanup.json').exists())
            self.assertIn('retained', text)

            def interrupted(s):
                (s.root / 'daemon.log').write_text('log')
                raise np.Interrupted(signal.SIGTERM)
            session, error, _ = self.run_session(interrupted)
            self.assertIsInstance(error, np.Interrupted)
            record = json.loads((session.evidence / 'cleanup.json').read_text())
            self.assertEqual(record['interrupted_signal'], signal.SIGTERM)
            self.assertTrue(session.root.exists())

    def test_forced_cleanup_fails_an_otherwise_passing_run(self):
        with patch.dict(os.environ, {}, clear=True):
            def body(s):
                s.ledger.failures.append('daemon: ignored SIGTERM for 4s and was killed')
                s.ledger.forced.append('daemon')
            session, error, text = self.run_session(body)
            self.assertIsInstance(error, np.CleanupFailed)
            self.assertTrue(session.root.exists(), 'forced cleanup keeps the evidence')
            self.assertIn('CLEANUP FAILURE', text)
            self.assertEqual(json.loads((session.evidence / 'cleanup.json').read_text())['forced'], ['daemon'])

    def test_external_evidence_receives_logs_and_leaves_no_env(self):
        outer = Path(tempfile.mkdtemp(prefix='np-outer-'))
        self.addCleanup(shutil.rmtree, outer)
        with patch.dict(os.environ, {'NP_TEST_EVIDENCE': str(outer / 'ev'), 'SECRET_TOKEN': 'do-not-leak'}, clear=True):
            def body(s):
                (s.root / 'daemon-normal.log').write_text('synthetic')
                s.extra_evidence.append(s.root / 'debug.json')
                (s.root / 'debug.json').write_text('{}')
                raise AssertionError('x')
            session, error, _ = self.run_session(body)
        self.assertEqual((outer / 'ev' / 'daemon-normal.log').read_text(), 'synthetic')
        self.assertTrue((outer / 'ev' / 'debug.json').exists())
        for path in (outer / 'ev').iterdir():
            self.assertNotIn('do-not-leak', path.read_text())

    def test_modified_binary_is_a_failure(self):
        directory = Path(tempfile.mkdtemp(prefix='np-bin-'))
        self.addCleanup(shutil.rmtree, directory)
        binary = directory / 'decnotify'
        binary.write_text('#!/bin/sh\n')
        binary.chmod(0o700)
        with patch.dict(os.environ, {}, clear=True):
            session, error, _ = self.run_session(lambda s: binary.write_text('#!/bin/sh\necho changed\n'), binaries={'decnotify': binary})
        self.assertIsInstance(error, np.CleanupFailed)
        self.assertIn('changed during the run', str(error))

    def test_permission_repair_makes_trees_removable(self):
        root = Path(tempfile.mkdtemp(prefix='np-perm-'))
        self.addCleanup(shutil.rmtree, root, True)
        locked = root / 'state' / 'decebal'
        locked.mkdir(parents=True)
        (locked / 'file').write_text('x')
        locked.chmod(0o500)
        link = root / 'link'
        link.symlink_to(root / 'elsewhere')
        np.repair_permissions(root)
        self.assertEqual(locked.stat().st_mode & 0o777, 0o700)


class ScriptSources(unittest.TestCase):
    """The native scripts must keep their coverage and must not regress to unsafe process or display handling."""
    history = (HERE / 'notify-history.py').read_text()
    theme = (HERE / 'notify-theme.py').read_text()

    def test_no_raw_process_management_or_display_adoption(self):
        for name, text in (('history', self.history), ('theme', self.theme)):
            for forbidden in ('subprocess.Popen', 'os.killpg', 'os.kill(', 'shutil.rmtree', '/tmp/.X', 'os.environ.copy', 'dict(os.environ)'):
                self.assertNotIn(forbidden, text, f'{name}: {forbidden}')
            self.assertNotRegex(text, r"start\(\[?'?Xvfb|'Xvfb',\s*env\[", name)
            self.assertIn('private.run_main(main)', text)

    def test_history_keeps_its_coverage(self):
        for marker in ("rdwm", "--backend", "xprop", "_NET_SUPPORTING_WM_CHECK", "chmod(0o500)", "retry_rect", "clear_rect",
                       "for mode in ('symlink', 'corrupt', 'unreadable')", "Retry did not rekey conflicting old record",
                       "native Retry did not advance daemon ID above merged history", "restart did not restore exactly durable items",
                       "read-only restart modified history", "transient notification persisted", "stat.S_IMODE(history.stat().st_mode) == 0o600",
                       "ClearHistory returned success despite durable failure", "notification contents leaked into diagnostic log",
                       "Retry lost repaired disk entry or duplicated IDs", "post-Retry Notify overwrote a merged old entry",
                       "failed Retry falsely cleared the warning", "native Clear button failed without visible warning"):
            self.assertIn(marker, self.history, marker)

    def test_theme_keeps_its_coverage(self):
        for marker in ("toast matches resolved Nord", "same existing toast repainted", "history uses same resolved palette",
                       "independent-palette", "'dynamic'", "'light'", "missing-theme", "same existing banner follows B",
                       "same missing banner gains icon", "image hint survives icon refresh", "subsequent matching notification uses B",
                       "history retries previous missing icon", "existing history follows A", "history.read_bytes() == original",
                       "after[item]['deadline']", "result.json"):
            self.assertIn(marker, self.theme, marker)

    def test_wrappers_require_explicit_binaries(self):
        history = (HERE / 'notify-history.sh').read_text()
        theme = (HERE / 'notify-theme.sh').read_text()
        self.assertRegex(history, r':\s*"\$\{MS_BIN_DIR:\?')
        self.assertRegex(history, r':\s*"\$\{RDWM_BIN_DIR:\?')
        self.assertRegex(theme, r':\s*"\$\{MS_BIN_DIR:\?')
        for text in (history, theme):
            self.assertNotRegex(text, r'\$HOME|target/(debug|release)\}|CARGO_TARGET_DIR|cargo ')


if __name__ == '__main__':
    unittest.main()
