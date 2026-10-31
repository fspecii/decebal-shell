#!/usr/bin/env python3
"""Owned-X11 Control Center live-reload acceptance; never uses a live session.

Run system Python with python-xlib and Pillow:
  python3 tests/e2e/control-center-reload-native.py --bin /exact/decbar --out FRESH_DIR
One overlay process per style reloads actions -> empty -> actions -> unavailable
media. Settings UI and hardware are not covered.
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

REPO = Path(os.environ.get('DECEBAL_QA_REPO', Path(__file__).resolve().parents[2])).resolve()
sys.path.insert(0, str(REPO / 'tests/e2e'))
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
    marker = ('CC_RELOAD_QA_TOKEN=' + token).encode()
    result = {'passed': False, 'cases': [], 'binary': str(binary),
              'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'fixture_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              'scope': 'same PID/XID configuration reload per style; no Settings UI or hardware proof',
              'repository_revision': subprocess.check_output(['/usr/bin/git', 'rev-parse', 'HEAD'], cwd=REPO, text=True).strip(),
              'source_sha256': {name: hashlib.sha256((REPO / name).read_bytes()).hexdigest() for name in
                  ('src/menubar/overlay.rs', 'src/menubar/embed.rs', 'src/menubar/mod.rs', 'src/theme/live.rs',
                   'src/controlcenter/mod.rs', 'src/controlcenter/settings.rs', 'src/controlcenter/gnome.rs', 'src/controlcenter/dux.rs')}}
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

    env = {'PATH': str(out / 'bin'), 'LANG': 'C.UTF-8', 'CC_RELOAD_QA_TOKEN': token,
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
        path.write_text('#!/bin/sh\nprintf "%s\\n" "' + name + ' $*" >> "$CC_RELOAD_CALLS"\nexit 1\n')
        path.chmod(0o700)
    # Font discovery is read-only and intentionally the only forwarded CLI.
    for name in ('fc-list', 'fc-match'):
        path = tools / name
        path.write_text('#!/bin/sh\nexec /usr/bin/' + name + ' "$@"\n')
        path.chmod(0o700)
    env['RDWMCTL'] = str(tools / 'rdwmctl')
    env['CC_RELOAD_CALLS'] = str(out / 'probe-calls.log')
    config = Path(env['DECEBAL_CONFIG']) / 'decebal.json'
    try:
        log = (out / 'xvfb.log').open('w')
        logs.append(log)
        server, disp = private_x11.start_private_xvfb(env=env, log=log, executable='/usr/bin/Xvfb', screen='1280x900x24')
        env['DISPLAY'] = disp
        conn = xcall(lambda: display.Display(disp))
        for style in ('card', 'fluent', 'gnome', 'dux'):
            case_dir = out / style
            case_dir.mkdir()
            dump = case_dir / 'panel.txt'
            def write_config(tiles):
                # live::Source stamps the JSON path and its metadata; replace the
                # file just as Settings' atomic persistence does. No signal/IPC
                # refresh is injected: the 500ms live watcher must wake overlay.
                body = json.dumps({'theme': {'name': 'macos', 'mode': 'dark'},
                    'bar': {'enabled': False}, 'control_center': {'style': style,
                    'position': 'top-left', 'tiles': tiles}}).encode()
                temporary = config.with_name('decebal.json.reload-new')
                with temporary.open('xb') as handle:
                    handle.write(body)
                    handle.flush()
                    os.fsync(handle.fileno())
                temporary.replace(config)
                return hashlib.sha256(body).hexdigest()
            initial_config_sha = write_config(['actions'])
            case_env = dict(env, DECBAR_DEBUG_PANEL=str(dump))
            log = (case_dir / 'native.log').open('w')
            logs.append(log)
            xcall(lambda: None)
            child = subprocess.Popen([str(isolated), '--control-center', 'open'], env=case_env, stdout=log, stderr=log)
            children.append(child)
            process_start = Path('/proc', str(child.pid), 'stat').read_text().rsplit(')', 1)[1].split()[19]
            def metadata():
                assert child.poll() is None, 'overlay exited during reload'
                assert Path('/proc', str(child.pid), 'stat').read_text().rsplit(')', 1)[1].split()[19] == process_start
                try:
                    lines = dump.read_text().splitlines()
                    return lines if lines and lines[0].startswith('panel ') else None
                except OSError:
                    return None
            def window():
                matches = []
                for candidate in xcall(lambda: conn.screen().root.query_tree().children):
                    attrs = xcall(candidate.get_attributes)
                    if attrs.map_state == X.IsViewable and xcall(candidate.get_wm_name) == 'decebal-control-center':
                        matches.append(candidate)
                assert len(matches) <= 1, 'ambiguous overlay windows'
                return matches[0] if matches else None
            win = wait(window, 'unique initial mapped overlay')
            original_xid = win.id
            previous_pixels = None
            for label, tiles in (('actions-initial', ['actions']), ('empty-reload', []),
                                 ('actions-restored', ['actions']), ('media-unavailable-reload', ['media'])):
                started = time.monotonic()
                config_sha = initial_config_sha if label == 'actions-initial' else write_config(tiles)
                def expected_metadata():
                    lines = metadata()
                    if lines is None:
                        return None
                    targets = {line.split()[1] for line in lines if line.startswith('target ')}
                    if tiles == ['actions']:
                        return lines if {'Screenshot', 'Settings', 'StayAwake'} <= targets else None
                    return lines if not targets and 'focus none' in lines and 'hover none' in lines else None
                lines = wait(expected_metadata, style + '/' + label + ' target transition', timeout=12)
                same = window()
                assert same is not None and same.id == original_xid, 'reload replaced the overlay window'
                targets = {line.split()[1] for line in lines if line.startswith('target ')}
                px, py, pw, ph = map(float, lines[0].split()[1:])
                assert pw > 0 and ph > 0
                stable_candidate = None
                def capture():
                    nonlocal stable_candidate
                    # Dump generation precedes presentation; require a different
                    # painted crop from the previous selection, not just new metadata.
                    same = window()
                    assert same is not None and same.id == original_xid
                    geometry = xcall(win.get_geometry)
                    raw = xcall(lambda: win.get_image(0, 0, geometry.width, geometry.height, X.ZPixmap, 0xffffffff))
                    assert conn.display.info.image_byte_order == X.LSBFirst
                    image = Image.frombytes('RGB', (geometry.width, geometry.height), raw.data, 'raw', 'BGRX')
                    crop = image.crop((int(px + 8), int(py + 8), int(px + pw - 8), int(py + ph - 8)))
                    pixels_sha = hashlib.sha256(crop.tobytes()).hexdigest()
                    if pixels_sha == previous_pixels:
                        return None
                    pixels = crop.get_flattened_data() if hasattr(crop, 'get_flattened_data') else crop.getdata()
                    lum = sorted(sum(rgb) / 3 for rgb in pixels)
                    if not lum:
                        return None
                    baseline = lum[len(lum) // 2]
                    contrast = sum(value > baseline + 30 for value in lum)
                    if contrast <= 10:
                        return None
                    now = time.monotonic()
                    if stable_candidate is None or stable_candidate[0] != pixels_sha:
                        stable_candidate = (pixels_sha, now)
                        return None
                    if now - stable_candidate[1] < .15:
                        return None
                    image.save(case_dir / (label + '-surface.png'))
                    crop.save(case_dir / (label + '-panel.png'))
                    return contrast, pixels_sha
                contrast, pixels_sha = wait(capture, style + '/' + label + ' newly painted content', timeout=12)
                previous_pixels = pixels_sha
                if not targets:
                    xcall(lambda: xtest.fake_input(conn, X.KeyPress, conn.keysym_to_keycode(0xff54)))
                    xcall(lambda: xtest.fake_input(conn, X.KeyRelease, conn.keysym_to_keycode(0xff54)))
                    xcall(conn.sync)
                    time.sleep(.1)
                    assert 'focus none' in metadata(), 'reload left a hidden keyboard target'
                (case_dir / (label + '-panel.txt')).write_text('\n'.join(lines) + '\n')
                result['cases'].append({'style': style, 'selection': label, 'targets': sorted(targets),
                    'pid': child.pid, 'process_start_ticks': process_start, 'xid': original_xid,
                    'config_sha256': config_sha, 'pixels_sha256': pixels_sha,
                    'visible_content_pixels': contrast, 'panel': [px, py, pw, ph],
                    'elapsed_s': time.monotonic() - started, 'passed': True})
            stop(child)
            children.remove(child)
            wait(lambda: window() is None, 'overlay removed after own process shutdown')
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
