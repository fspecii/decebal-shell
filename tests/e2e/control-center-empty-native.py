#!/usr/bin/env python3
"""Owned-X11 Control Center empty-selection acceptance; never uses a live session.

Run system Python with python-xlib and Pillow:
  python3 tests/e2e/control-center-empty-native.py --bin /exact/decbar --out FRESH_DIR
Each case starts a fresh process. Reload-in-place, Settings UI and hardware are
not covered; real Settings pointer/persistence coverage lives in Rust tests.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import time
import uuid

import private_x11
from PIL import Image
from Xlib import X, display
from Xlib.ext import xtest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bin', type=Path, default=os.environ.get('BAR_BINARY'))
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    if args.bin is None:
        parser.error('--bin or BAR_BINARY is required')
    binary = args.bin.resolve(strict=True)
    args.out.mkdir(mode=0o700, parents=True, exist_ok=False)
    out = args.out.resolve()
    token = uuid.uuid4().hex
    marker = ('CC_EMPTY_QA_TOKEN=' + token).encode()
    result = {'passed': False, 'cases': [], 'binary': str(binary),
              'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'fixture_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              'scope': 'fresh native overlay processes; no reload-in-place, Settings UI or hardware proof'}
    server = conn = None
    children = []
    logs = []

    def interrupted(signum, frame):
        raise SystemExit(128 + signum)

    for sig in (signal.SIGTERM, signal.SIGINT):
        signal.signal(sig, interrupted)

    def marked():
        found = []
        for proc in Path('/proc').iterdir():
            try:
                if proc.name.isdigit() and proc.stat().st_uid == os.getuid() and marker in (proc / 'environ').read_bytes().split(b'\0'):
                    found.append(int(proc.name))
            except OSError:
                pass
        return found

    def send_owned(pid, sig):
        fd = os.pidfd_open(pid)
        try:
            proc = Path('/proc') / str(pid)
            assert proc.stat().st_uid == os.getuid()
            assert marker in (proc / 'environ').read_bytes().split(b'\0')
            signal.pidfd_send_signal(fd, sig)
        finally:
            os.close(fd)

    def wait(fn, label, timeout=10):
        until = time.monotonic() + timeout
        while time.monotonic() < until:
            value = fn()
            if value:
                return value
            time.sleep(.05)
        raise AssertionError(label)

    def xcall(fn):
        assert private_x11.listeners_owned(server, int(disp[1:])), 'lost owned X server'
        return fn()

    def stop(child):
        # Direct Popen children remain unreaped until exact token/pidfd shutdown.
        try:
            send_owned(child.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            child.wait(timeout=4)
        except subprocess.TimeoutExpired:
            send_owned(child.pid, signal.SIGKILL)
            child.wait(timeout=3)
            result['forced_cleanup'] = True
            raise AssertionError('forced process cleanup')

    env = {'PATH': str(out / 'bin'), 'LANG': 'C.UTF-8', 'CC_EMPTY_QA_TOKEN': token,
           'DECEBAL_BACKEND': 'x11', 'DECEBAL_SYSTEM_AUTOMATIC': '0',
           'DECEBAL_SCALE': '1', 'DECEBAL_FONT_SCALE': '1',
           'GSETTINGS_BACKEND': 'memory', 'GTK_A11Y': 'none',
           'DBUS_SESSION_BUS_ADDRESS': 'unix:path=' + str(out / 'no-session'),
           'DBUS_SYSTEM_BUS_ADDRESS': 'unix:path=' + str(out / 'no-system'),
           'PULSE_SERVER': 'unix:' + str(out / 'no-audio')}
    if os.environ.get('E2E_PARENT_TOKEN'):
        env['E2E_PARENT_TOKEN'] = os.environ['E2E_PARENT_TOKEN']
    for key in ('HOME', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_STATE_HOME',
                'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR', 'XDG_DATA_DIRS', 'XDG_CONFIG_DIRS',
                'DECEBAL_CONFIG', 'DECEBAL_THEMES', 'DECEBAL_SYSTEM_THEMES'):
        path = out / key.lower()
        path.mkdir(mode=0o700)
        env[key] = str(path)
    tools = out / 'bin'
    tools.mkdir()
    isolated = tools / 'decbar'
    shutil.copy2(binary, isolated)
    isolated.chmod(0o555)
    assert hashlib.sha256(isolated.read_bytes()).hexdigest() == result['binary_sha256']
    # No system PATH: unlisted probes fail closed. These siblings only log and
    # fail; they cannot invoke desktop actions or reach the user's services.
    for name in ('curl', 'nmcli', 'wpctl', 'pactl', 'playerctl', 'bluetoothctl',
                 'brightnessctl', 'decutil', 'rdwmctl', 'decsettings', 'decsession',
                 'decnotify', 'omarchy', 'dunstctl', 'makoctl', 'notify-send',
                 'xdg-open', 'systemctl', 'loginctl', 'launchapp'):
        path = tools / name
        path.write_text('#!/bin/sh\nprintf "%s\\n" "' + name + ' $*" >> "$CC_EMPTY_CALLS"\nexit 1\n')
        path.chmod(0o700)
    # Font discovery is read-only and intentionally the only forwarded CLI.
    for name in ('fc-list', 'fc-match'):
        path = tools / name
        path.write_text('#!/bin/sh\nexec /usr/bin/' + name + ' "$@"\n')
        path.chmod(0o700)
    env['RDWMCTL'] = str(tools / 'rdwmctl')
    env['CC_EMPTY_CALLS'] = str(out / 'probe-calls.log')
    config = Path(env['DECEBAL_CONFIG']) / 'decebal.json'
    try:
        log = (out / 'xvfb.log').open('w')
        logs.append(log)
        server, disp = private_x11.start_private_xvfb(env=env, log=log, executable='/usr/bin/Xvfb', screen='1280x900x24')
        env['DISPLAY'] = disp
        conn = xcall(lambda: display.Display(disp))
        for style in ('card', 'fluent', 'gnome', 'dux'):
            for label, tiles in (('empty', []), ('media-unavailable', ['media']), ('actions-positive', ['actions'])):
                case_dir = out / (style + '-' + label)
                case_dir.mkdir()
                dump = case_dir / 'panel.txt'
                config.write_text(json.dumps({'theme': {'name': 'macos', 'mode': 'dark'},
                    'bar': {'enabled': False}, 'control_center': {'style': style, 'position': 'top-left', 'tiles': tiles}}))
                case_env = dict(env, DECBAR_DEBUG_PANEL=str(dump))
                log = (case_dir / 'native.log').open('w')
                logs.append(log)
                xcall(lambda: None)
                child = subprocess.Popen([str(isolated), '--control-center', 'open'], env=case_env, stdout=log, stderr=log)
                children.append(child)
                def metadata():
                    assert child.poll() is None, 'overlay exited before metadata'
                    try:
                        lines = dump.read_text().splitlines()
                        return lines if lines and lines[0].startswith('panel ') else None
                    except OSError:
                        return None
                lines = wait(metadata, 'native panel metadata')
                targets = {line.split()[1] for line in lines if line.startswith('target ')}
                if tiles == ['actions']:
                    assert {'Screenshot', 'Settings', 'StayAwake'} <= targets, (style, targets)
                else:
                    assert not targets, (style, label, targets)
                    assert 'focus none' in lines and 'hover none' in lines
                # Select the unique mapped overlay, on the server only we created.
                def window():
                    matches = []
                    for candidate in xcall(lambda: conn.screen().root.query_tree().children):
                        attrs = xcall(candidate.get_attributes)
                        if attrs.map_state == X.IsViewable and xcall(candidate.get_wm_name) == 'decebal-control-center':
                            matches.append(candidate)
                    assert len(matches) <= 1, 'ambiguous overlay windows'
                    return matches[0] if matches else None
                win = wait(window, 'unique mapped overlay')
                px, py, pw, ph = map(float, lines[0].split()[1:])
                assert pw > 0 and ph > 0
                def capture():
                    geometry = xcall(win.get_geometry)
                    raw = xcall(lambda: win.get_image(0, 0, geometry.width, geometry.height, X.ZPixmap, 0xffffffff))
                    assert conn.display.info.image_byte_order == X.LSBFirst
                    image = Image.frombytes('RGB', (geometry.width, geometry.height), raw.data, 'raw', 'BGRX')
                    crop = image.crop((int(px + 8), int(py + 8), int(px + pw - 8), int(py + ph - 8)))
                    lum = sorted(sum(rgb) / 3 for rgb in crop.getdata())
                    # A painted flat panel alone is insufficient: require visible
                    # content contrast in the interior, away from rounded edges.
                    baseline = lum[len(lum) // 2]
                    contrast = sum(value > baseline + 30 for value in lum)
                    if contrast <= 10:
                        return None
                    image.save(case_dir / 'surface.png')
                    crop.save(case_dir / 'panel.png')
                    return contrast
                contrast = wait(capture, 'visible native panel content')
                if not targets:
                    # Navigation may not manufacture a hidden keyboard target.
                    xcall(lambda: xtest.fake_input(conn, X.KeyPress, conn.keysym_to_keycode(0xff54)))
                    xcall(lambda: xtest.fake_input(conn, X.KeyRelease, conn.keysym_to_keycode(0xff54)))
                    xcall(conn.sync)
                    time.sleep(.1)
                    assert 'focus none' in metadata()
                result['cases'].append({'style': style, 'selection': label, 'targets': sorted(targets),
                    'visible_content_pixels': contrast, 'panel': [px, py, pw, ph], 'passed': True})
                stop(child)
                children.remove(child)
                wait(lambda: not xcall(lambda: conn.screen().root.query_tree().children) or window() is None,
                     'overlay removed after own process shutdown')
        result['passed'] = True
    except BaseException as exc:
        result['error'] = repr(exc)
    finally:
        cleanup_errors = []
        for child in children:
            try:
                stop(child)
            except BaseException as exc:
                cleanup_errors.append(repr(exc))
        # Probe subprocesses have parent-death shutdown; audit before rescue.
        expected = {server.pid} if server else set()
        until = time.monotonic() + 3
        while set(marked()) - expected and time.monotonic() < until:
            time.sleep(.05)
        result['detached_pids'] = sorted(set(marked()) - expected)
        if result['detached_pids']:
            result['passed'] = False
            for pid in result['detached_pids']:
                try:
                    send_owned(pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                except BaseException as exc:
                    cleanup_errors.append(repr(exc))
        if conn is not None:
            try:
                conn.close()
            except BaseException as exc:
                cleanup_errors.append(repr(exc))
        if server is not None:
            private_x11._owned.discard(server)
            try:
                stop(server)
            except BaseException as exc:
                cleanup_errors.append(repr(exc))
        until = time.monotonic() + 3
        while marked() and time.monotonic() < until:
            time.sleep(.05)
        result['leftover_pids'] = marked()
        if result['leftover_pids'] or cleanup_errors:
            result.update(passed=False, cleanup_errors=cleanup_errors)
        for log in logs:
            log.close()
        (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
        print(json.dumps(result))
    return 0 if result['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
