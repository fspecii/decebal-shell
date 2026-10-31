#!/usr/bin/env python3
"""Dock process lifecycle on an owned private X server and private rdwm session."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parent))
from private_x11 import start_private_xvfb, listeners_owned, close

ap = argparse.ArgumentParser()
ap.add_argument('--bin-dir', type=Path, required=True)
ap.add_argument('--themes', type=Path, required=True)
ap.add_argument('--wm', type=Path, required=True)
ap.add_argument('--ctl', type=Path, required=True)
ap.add_argument('--out', type=Path, required=True)
a = ap.parse_args()
a.out.mkdir(parents=True, mode=0o700, exist_ok=False)
root = a.out.resolve()
bins = a.bin_dir.resolve(strict=True)
wm, ctl = a.wm.resolve(strict=True), a.ctl.resolve(strict=True)
token = uuid.uuid4().hex
env = {'PATH': str(bins) + os.pathsep + os.defpath, 'HOME': str(root / 'home'), 'LANG': 'C.UTF-8',
       'SMOKE_QA_TOKEN': token, 'DECEBAL_SYSTEM_AUTOMATIC': '0', 'DECEBAL_BACKEND': 'x11',
       'DBUS_SESSION_BUS_ADDRESS': 'unix:path=' + str(root / 'no-bus'),
       'DBUS_SYSTEM_BUS_ADDRESS': 'unix:path=' + str(root / 'no-system-bus')}
for key, name in [('HOME', 'home'), ('XDG_CONFIG_HOME', 'config'), ('XDG_DATA_HOME', 'data'),
                  ('XDG_CACHE_HOME', 'cache'), ('XDG_STATE_HOME', 'state'), ('XDG_RUNTIME_DIR', 'run'),
                  ('XDG_DATA_DIRS', 'no-data'), ('XDG_CONFIG_DIRS', 'no-config')]:
    path = root / name
    path.mkdir(mode=0o700)
    env[key] = str(path)
env.update(DECEBAL_CONFIG=str(root / 'config/omarchy'), DECEBAL_SYSTEM_THEMES=str(a.themes.resolve(strict=True)),
           RDWMCTL=str(ctl), RDWM_SOCK=str(root / 'run/rdwm.sock'))
(root / 'config/omarchy/backgrounds').mkdir(parents=True)
config = root / 'config/rdwm.toml'
config.write_text('[general]\nborder_px = 2\ngap_inner = 6\n[bar]\nshow = false\n')
owned = []
server = None
checks = []
result = {'checks': checks, 'binaries': {n: hashlib.sha256((bins / n).read_bytes()).hexdigest()
                                      for n in ('decdock', 'decthemectl')},
          'wm_sha256': hashlib.sha256(wm.read_bytes()).hexdigest()}


def marked():
    found = []
    needle = ('SMOKE_QA_TOKEN=' + token).encode()
    for proc in Path('/proc').iterdir():
        try:
            if proc.name.isdigit() and proc.stat().st_uid == os.getuid() and needle in (proc / 'environ').read_bytes().split(b'\0'):
                found.append(int(proc.name))
        except (OSError, ValueError):
            pass
    return found


def docks():
    out = []
    for pid in marked():
        try:
            if Path(os.readlink('/proc/%d/exe' % pid)) == bins / 'decdock':
                out.append(pid)
        except OSError:
            pass
    return out


def prove():
    assert listeners_owned(server, int(display[1:])), 'private display ownership lost'


def run(*argv):
    prove()
    p = subprocess.run(list(map(str, argv)), env=env, capture_output=True, text=True, timeout=10)
    assert p.returncode == 0, (argv, p.returncode, p.stderr)
    return p.stdout


def count(want, label):
    end = time.monotonic() + 8
    while time.monotonic() < end:
        if len(docks()) == want:
            checks.append({'check': label, 'dock_pids': docks()})
            return
        time.sleep(.05)
    raise AssertionError((label, 'expected', want, 'actual', docks()))


try:
    with (root / 'xvfb.log').open('w') as log:
        server, display = start_private_xvfb(env=env, log=log, screen='1600x1000x24')
    env['DISPLAY'] = display
    prove()
    with (root / 'wm.log').open('w') as log:
        child = subprocess.Popen([str(wm), '--backend', 'x11', '--config', str(config)], env=env, stdout=log, stderr=log)
    owned.append(child)
    end = time.monotonic() + 5
    while not Path(env['RDWM_SOCK']).exists() and child.poll() is None and time.monotonic() < end:
        time.sleep(.05)
    assert child.poll() is None and Path(env['RDWM_SOCK']).is_socket(), 'private WM did not become ready'
    off = root / 'run/rdwm-decebal/decdock.off'
    off.parent.mkdir(exist_ok=True)
    off.touch()
    run(bins / 'decthemectl', 'theme', 'apply', 'macos')
    count(1, 'macOS starts one dock despite stale off marker')
    assert not off.exists()
    run(bins / 'decdock')
    count(1, 'duplicate start preserves one dock')
    for theme, expected in [('orbit-glass', 0), ('macos', 1), ('caelestia', 0), ('windows11', 1)]:
        run(bins / 'decthemectl', 'theme', 'apply', theme)
        count(expected, theme + ' owns the expected dock process count')
        if expected == 0:
            assert off.exists()
            run(bins / 'decdock')
            count(0, theme + ' disabled dock exits without hidden process')
    result['passed'] = True
except BaseException as exc:
    result.update(passed=False, error=repr(exc))
    raise
finally:
    needle = ('SMOKE_QA_TOKEN=' + token).encode()
    for pid in marked():
        fd = None
        try:
            fd = os.pidfd_open(pid)
            if needle in Path('/proc/%d/environ' % pid).read_bytes().split(b'\0'):
                signal.pidfd_send_signal(fd, signal.SIGTERM)
        except (OSError, ProcessLookupError):
            pass
        finally:
            if fd is not None:
                os.close(fd)
    for child in owned:
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait(timeout=5)
            result['passed'] = False
            result['forced_child_cleanup'] = True
    if server is not None:
        close(server)
    end = time.monotonic() + 3
    while marked() and time.monotonic() < end:
        time.sleep(.05)
    result['leftover_pids'] = marked()
    result['passed'] = result.get('passed', False) and not result['leftover_pids']
    (root / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result))
    if not result['passed']:
        sys.exit(1)
