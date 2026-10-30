#!/usr/bin/env python3
"""Private-bus real-CLI acceptance for decutil reveal (DECUTIL_BIN selects build).

Never contacts the live session bus or opens a real file manager. The fixture
implements the actual FileManager1 method signature and records received values.
"""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest

BIN = Path(os.environ.get('DECUTIL_BIN', 'target/debug/decutil')).resolve()
HERE = Path(__file__).resolve()


def serve(address, root, mode):
    from gi.repository import Gio, GLib
    flags = Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION
    connection = Gio.DBusConnection.new_for_address_sync(address, flags, None, None)
    interface = Gio.DBusNodeInfo.new_for_xml('''<node><interface name="org.freedesktop.FileManager1">
      <method name="ShowItems"><arg type="as" direction="in"/><arg type="s" direction="in"/></method>
    </interface></node>''').interfaces[0]

    pending = []

    def method(conn, sender, path, iface, member, parameters, invocation):
        with (root / 'requests.jsonl').open('a') as stream:
            stream.write(json.dumps({'path': path, 'interface': iface, 'member': member,
                                     'signature': parameters.get_type_string(),
                                     'arguments': parameters.unpack()}) + '\n')
        def reply():
            if mode == 'error':
                invocation.return_dbus_error('org.freedesktop.DBus.Error.NotSupported', 'fixture unsupported')
            else:
                invocation.return_value(GLib.Variant('()', ()))
            return False
        if mode == 'timeout':
            pending.append(invocation)
            return  # Hold the invocation until the caller's total deadline expires.
        if mode == 'delay':
            GLib.timeout_add(500, reply)
        else:
            reply()

    connection.register_object('/org/freedesktop/FileManager1', interface, method, None, None)
    result = connection.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus',
                                  'org.freedesktop.DBus', 'RequestName',
                                  GLib.Variant('(su)', ('org.freedesktop.FileManager1', 0)),
                                  GLib.VariantType.new('(u)'), Gio.DBusCallFlags.NONE, 2000, None)
    assert result.unpack()[0] == 1
    (root / 'service-ready').touch()
    GLib.MainLoop().run()


class Reveal(unittest.TestCase):
    def setUp(self):
        if not BIN.is_file():
            self.skipTest('build decutil or set DECUTIL_BIN')
        self.temp = tempfile.TemporaryDirectory(prefix='decebal-file-reveal-')
        self.root = Path(self.temp.name)
        self.children = []
        self.tools = self.root / 'tools'
        self.tools.mkdir()
        for name in ('xdg-open', 'custom-open'):
            script = self.tools / name
            script.write_text('#!' + sys.executable + '\nimport json,os,sys\n'
                              'with open(os.environ["REVEAL_OPEN_LOG"], "a") as f:'
                              ' f.write(json.dumps(sys.argv) + "\\n")\n')
            script.chmod(0o700)
        self.env = dict(os.environ, HOME=str(self.root), XDG_CONFIG_HOME=str(self.root / 'config'),
                        XDG_CACHE_HOME=str(self.root / 'cache'), XDG_DATA_HOME=str(self.root / 'data'),
                        XDG_STATE_HOME=str(self.root / 'state'), XDG_RUNTIME_DIR=str(self.root / 'run'),
                        PATH=str(self.tools), DBUS_SESSION_BUS_ADDRESS='unix:path=/nonexistent',
                        REVEAL_OPEN_LOG=str(self.root / 'opens.jsonl'))
        for name in ('DISPLAY', 'WAYLAND_DISPLAY', 'RDWM_SOCK', 'DECEBAL_OPEN'):
            self.env.pop(name, None)
        self.folder = self.root / 'parent folder'
        self.folder.mkdir()
        self.target = self.folder / 'ordinary.txt'
        self.target.write_text('private fixture')

    def tearDown(self):
        for child in reversed(self.children):
            if child.poll() is None:
                child.terminate()
            try:
                child.communicate(timeout=3)
            except subprocess.TimeoutExpired:
                child.kill()
                child.communicate(timeout=3)
        self.temp.cleanup()

    def wait(self, predicate, message, seconds=5):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if predicate():
                return
            time.sleep(.02)
        self.fail(message)

    def bus(self, mode='success'):
        # No service directories: the private bus cannot activate installed apps.
        config = self.root / 'bus.conf'
        config.write_text('''<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen>
          <auth>EXTERNAL</auth><policy context="default"><allow send_destination="*"/>
          <allow receive_sender="*"/><allow own="*"/></policy></busconfig>''')
        bus = subprocess.Popen(['/usr/bin/dbus-daemon', '--nofork', '--print-address=1',
                                '--config-file=' + str(config)], stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, text=True)
        self.children.append(bus)
        address = bus.stdout.readline().strip()
        self.assertTrue(address.startswith('unix:'), address)
        self.env['DBUS_SESSION_BUS_ADDRESS'] = address
        service = subprocess.Popen([sys.executable, str(HERE), '--service', address, str(self.root), mode],
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        self.children.append(service)
        self.wait(lambda: (self.root / 'service-ready').exists(), 'private service did not register')

    def requests(self):
        path = self.root / 'requests.jsonl'
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def opens(self):
        path = self.root / 'opens.jsonl'
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def reveal(self, path):
        return subprocess.run([str(BIN), 'reveal', '--', str(path)], env=self.env,
                              capture_output=True, text=True, timeout=8)

    def assert_folder(self, result, parent):
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), 'folder')
        self.wait(lambda: bool(self.opens()), 'fallback opener not started')
        self.assertEqual(len(self.opens()), 1, 'fallback must run exactly once')
        self.assertEqual(self.opens()[0][-1], str(parent))

    def test_typed_exact_paths_with_special_names_symlink_and_directory(self):
        self.bus()
        names = ['space here.txt', '日本語 café.txt', 'hash#percent%.txt', 'quote\'"$(touch NEVER).txt', '-leading.txt']
        paths = []
        for name in names:
            path = self.folder / name
            path.write_text('not executable')
            paths.append(path)
        link = self.folder / 'link # identity'
        link.symlink_to(paths[0])
        executable = self.folder / 'must-not-run'
        executable.write_text('#!' + sys.executable + '\nfrom pathlib import Path\nPath(' + repr(str(self.root / 'executed')) + ').touch()\n')
        executable.chmod(0o700)
        paths.extend([link, self.folder, executable])
        for path in paths:
            with self.subTest(path=path.name):
                result = self.reveal(path)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout.strip(), 'selected')
                request = self.requests()[-1]
                self.assertEqual(request['path'], '/org/freedesktop/FileManager1')
                self.assertEqual(request['interface'], 'org.freedesktop.FileManager1')
                self.assertEqual(request['member'], 'ShowItems')
                self.assertEqual(request['signature'], '(ass)')
                self.assertEqual(request['arguments'], [[path.as_uri()], ''])
        self.assertEqual(len(self.requests()), len(paths))
        self.assertEqual(self.opens(), [])
        self.assertFalse((self.root / 'NEVER').exists())
        self.assertFalse((self.root / 'executed').exists(), 'reveal must never execute the selected file')

    def test_missing_bus_falls_back_to_parent_once(self):
        self.assert_folder(self.reveal(self.target), self.folder)

    def test_service_error_falls_back_once(self):
        self.bus('error')
        self.assert_folder(self.reveal(self.target), self.folder)
        self.assertEqual(len(self.requests()), 1)

    def test_total_request_timeout_is_bounded_and_falls_back_once(self):
        self.bus('timeout')
        start = time.monotonic()
        self.assert_folder(self.reveal(self.target), self.folder)
        elapsed = time.monotonic() - start
        self.assertGreater(elapsed, 2)
        self.assertLess(elapsed, 6, 'total service deadline must include connection and request')
        self.assertEqual(len(self.requests()), 1)

    def test_custom_opener_is_honoured_without_contacting_other_manager(self):
        self.bus()
        self.env['DECEBAL_OPEN'] = 'custom-open --profile "two words"'
        self.assert_folder(self.reveal(self.target), self.folder)
        self.assertEqual(self.opens()[0][1:], ['--profile', 'two words', str(self.folder)])
        self.assertEqual(self.requests(), [])

    def test_request_finishes_after_detached_requesting_launcher_exits(self):
        self.bus('delay')
        output = self.root / 'detached-output'
        launcher = 'import subprocess,sys; f=open(sys.argv[2],"w"); p=subprocess.Popen([sys.argv[1],"reveal","--",sys.argv[3]],stdout=f,stderr=f,start_new_session=True); print(p.pid)'
        parent = subprocess.run([sys.executable, '-c', launcher, str(BIN), str(output), str(self.target)],
                                env=self.env, capture_output=True, text=True, timeout=3)
        self.assertEqual(parent.returncode, 0, parent.stderr)
        pid = int(parent.stdout.strip())
        try:
            self.wait(lambda: output.exists() and output.read_text().strip() == 'selected',
                      'detached helper did not finish after launcher exited')
            self.assertEqual(len(self.requests()), 1)
            self.assertEqual(self.opens(), [])
        finally:
            # Only kill our private helper if it is unexpectedly still running.
            try:
                args = Path('/proc/' + str(pid) + '/cmdline').read_bytes().split(b'\0')
                if args and args[0] == os.fsencode(BIN):
                    os.kill(pid, signal.SIGTERM)
            except (FileNotFoundError, ProcessLookupError):
                pass

    def test_owner_death_stops_typed_worker_without_fallback(self):
        self.bus('timeout')
        owner = subprocess.Popen([str(BIN), 'reveal', '--', str(self.target)], env=self.env,
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        self.children.append(owner)
        self.wait(lambda: bool(self.requests()), 'typed worker never reached private service')
        children = Path('/proc/' + str(owner.pid) + '/task/' + str(owner.pid) + '/children')
        worker_pids = [int(pid) for pid in children.read_text().split()]
        self.assertEqual(len(worker_pids), 1, 'reveal must own one bounded typed worker')
        worker = worker_pids[0]
        args = Path('/proc/' + str(worker) + '/cmdline').read_bytes().split(b'\0')
        self.assertTrue(os.path.samefile('/proc/' + str(worker) + '/exe', BIN))
        self.assertIn(b'--file-manager-show-items', args)
        owner.terminate()
        owner.communicate(timeout=3)
        def stopped():
            try:
                # A reparented zombie is not executing and is waiting for init.
                status = Path('/proc/' + str(worker) + '/stat').read_text()
                return status.rsplit(') ', 1)[1].split()[0] == 'Z'
            except FileNotFoundError:
                return True
        self.wait(stopped, 'typed worker survived its owner', seconds=2)
        self.assertEqual(self.opens(), [], 'owner death must not launch a fallback')

    def test_vanished_item_opens_existing_parent(self):
        vanished = self.folder / 'removed.txt'
        self.assert_folder(self.reveal(vanished), self.folder)


if __name__ == '__main__':
    if len(sys.argv) > 1 and sys.argv[1] == '--service':
        serve(sys.argv[2], Path(sys.argv[3]), sys.argv[4])
    else:
        unittest.main()
