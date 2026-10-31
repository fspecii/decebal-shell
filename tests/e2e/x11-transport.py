#!/usr/bin/env python3
"""Private native X11 transport/auth proof. No host display or session access."""
import os
import pathlib
import secrets
import select
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time

probe = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else 'target/debug/examples/x11-connect-check').resolve()


def free_number():
    for _ in range(100):
        n = 20000 + secrets.randbelow(30000)
        if pathlib.Path(f'/tmp/.X11-unix/X{n}').exists() or pathlib.Path(f'/tmp/.X{n}-lock').exists():
            continue
        s = socket.socket(socket.AF_UNIX)
        try:
            s.bind(f'\0/tmp/.X11-unix/X{n}')
        except OSError:
            s.close()
            continue
        s.close()
        return n
    raise RuntimeError('no private display available')


def authority(path, cookie):
    # FamilyWild, empty address/display: valid only in this private auth file.
    fields = [b'', b'', b'MIT-MAGIC-COOKIE-1', cookie]
    path.write_bytes(struct.pack('!H', 65535) + b''.join(struct.pack('!H', len(v)) + v for v in fields))
    path.chmod(0o600)


class Proxy:
    def __init__(self, address, upstream):
        self.listener = socket.socket(socket.AF_UNIX)
        self.listener.bind(address)
        self.listener.listen()
        self.listener.settimeout(.2)
        self.upstream = upstream
        self.count = 0
        self.stop = False
        self.thread = threading.Thread(target=self.accept, daemon=True)
        self.thread.start()

    def accept(self):
        while not self.stop:
            try:
                client, _ = self.listener.accept()
            except (TimeoutError, OSError):
                continue
            self.count += 1
            threading.Thread(target=self.relay, args=(client,), daemon=True).start()

    def relay(self, client):
        server = socket.socket(socket.AF_UNIX)
        try:
            server.connect(self.upstream)
            while True:
                ready, _, _ = select.select([client, server], [], [], 3)
                if not ready:
                    return
                for source in ready:
                    data = source.recv(65536)
                    if not data:
                        return
                    (server if source is client else client).sendall(data)
        finally:
            client.close()
            server.close()

    def close(self):
        self.stop = True
        self.listener.close()
        self.thread.join(1)


with tempfile.TemporaryDirectory(prefix='decebal-x11-transport-') as tmp:
    root = pathlib.Path(tmp)
    for directory in ['home', 'runtime', 'config', 'cache', 'data']:
        (root / directory).mkdir(mode=0o700)
    auth = root / 'authority'
    wrong = root / 'wrong-authority'
    authority(auth, secrets.token_bytes(16))
    authority(wrong, secrets.token_bytes(16))
    env = {'PATH': '/usr/bin:/bin', 'HOME': str(root/'home'),
           'XDG_RUNTIME_DIR': str(root/'runtime'), 'XDG_CONFIG_HOME': str(root/'config'),
           'XDG_CACHE_HOME': str(root/'cache'), 'XDG_DATA_HOME': str(root/'data'),
           'DBUS_SESSION_BUS_ADDRESS': 'unix:path=/nonexistent/decebal-test-session',
           'DBUS_SYSTEM_BUS_ADDRESS': 'unix:path=/nonexistent/decebal-test-system',
           'XAUTHORITY': str(auth)}
    server_n = free_number()
    server_path = pathlib.Path(f'/tmp/.X11-unix/X{server_n}')
    proxies = []
    owned_paths = []
    server = subprocess.Popen([shutil.which('Xvfb'), f':{server_n}', '-screen', '0', '320x240x24',
                               '-screen', '1', '320x240x24', '-auth', str(auth), '-noreset', '-nolisten', 'tcp'],
                              env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(100):
            if server_path.exists():
                break
            if server.poll() is not None:
                raise RuntimeError('private Xvfb exited')
            time.sleep(.05)
        else:
            raise RuntimeError('private Xvfb did not start')
        abstract_n, filesystem_n = free_number(), free_number()
        assert len({server_n, abstract_n, filesystem_n}) == 3
        upstream = str(server_path)
        abstract = Proxy(f'\0/tmp/.X11-unix/X{abstract_n}', upstream)
        proxies.append(abstract)
        fs_path = pathlib.Path(f'/tmp/.X11-unix/X{filesystem_n}')
        filesystem = Proxy(str(fs_path), upstream)
        proxies.append(filesystem)
        owned_paths.append((fs_path, fs_path.stat().st_ino))

        def check(number, success, direct=False, bad=False, screen=0):
            current = dict(env, DISPLAY=f':{number}.{screen}', XAUTHORITY=str(wrong if bad else auth))
            result = subprocess.run([str(probe)] + (['--upstream'] if direct else []), env=current,
                                    capture_output=True, text=True, timeout=10)
            assert (result.returncode == 0) == success, result.stderr
            if success:
                assert f'screen={screen}' in result.stdout, result.stdout

        check(abstract_n, False, direct=True)
        check(abstract_n, True, screen=1)
        check(filesystem_n, True)
        check(filesystem_n, True, direct=True)
        # A usable filesystem endpoint at the same display must never be tried
        # after an accepted abstract transport rejects authentication.
        guard_path = pathlib.Path(f'/tmp/.X11-unix/X{abstract_n}')
        guard = Proxy(str(guard_path), upstream)
        proxies.append(guard)
        owned_paths.append((guard_path, guard_path.stat().st_ino))
        check(abstract_n, False, bad=True)
        assert guard.count == 0, 'authentication failure fell back to filesystem'
        print('PASS: upstream abstract failure; native abstract + screen 1; filesystem fallback; wrong cookie rejected without fallback')
        print(f'private fixture displays={server_n},{abstract_n},{filesystem_n}; probe={probe.name}')
    finally:
        for proxy in proxies:
            proxy.close()
        if server.poll() is None:
            server.terminate()
            try:
                server.wait(timeout=5)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait(timeout=5)
        for path, inode in owned_paths:
            if path.exists() and path.stat().st_ino == inode:
                path.unlink()
