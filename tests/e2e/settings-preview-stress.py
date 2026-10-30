#!/usr/bin/env python3
"""Private native Settings preview stress and disappearance diagnostic. No Escape or Apply input.

SETTINGS_BIN=/absolute/decsettings SETTINGS_EVIDENCE_DIR=/tmp/evidence python3 ...
SETTINGS_CORRUPT_CACHE=1 damages one private cached PNG before a warm relaunch.
Requires Xvfb, xdotool, import. Copies repo themes (or SETTINGS_THEMES_DIR) into the fixture.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time


def main():
    binary = Path(os.environ['SETTINGS_BIN'])
    assert binary.is_absolute() and binary.is_file(), 'SETTINGS_BIN must be an absolute binary path'
    out = Path(os.environ['SETTINGS_EVIDENCE_DIR']).resolve()
    out.mkdir(parents=True, exist_ok=True)
    installed = Path('/home/ambsd/.local/share/rdwm-decebal/bin')
    result = {'binary': str(binary), 'sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'click_count': 0, 'samples': []}
    processes = []
    failure = None
    with tempfile.TemporaryDirectory(prefix='settings-preview-stress-') as tmp:
        root = Path(tmp)
        for name in ('home', 'config', 'cache', 'data', 'state', 'run', 'empty'):
            (root / name).mkdir(mode=0o700)
        env = {k: v for k, v in os.environ.items() if not k.startswith(('DECEBAL_', 'DECSETTINGS_', 'RDWM', 'DBUS_', 'XDG_'))}
        for key in ('WAYLAND_DISPLAY', 'XAUTHORITY', 'SESSION_MANAGER', 'AT_SPI_BUS_ADDRESS', 'HYPRLAND_INSTANCE_SIGNATURE'):
            env.pop(key, None)
        display = next(f':{n}' for n in range(460, 560)
                       if not Path(f'/tmp/.X11-unix/X{n}').exists() and not Path(f'/tmp/.X{n}-lock').exists())
        env.update(HOME=str(root / 'home'), DISPLAY=display, XDG_CONFIG_HOME=str(root / 'config'),
                   XDG_CACHE_HOME=str(root / 'cache'), XDG_DATA_HOME=str(root / 'data'), XDG_STATE_HOME=str(root / 'state'),
                   XDG_RUNTIME_DIR=str(root / 'run'), XDG_CONFIG_DIRS=str(root / 'empty'), XDG_DATA_DIRS=str(root / 'empty'),
                   XDG_SESSION_TYPE='x11', DECEBAL_CONFIG=str(root / 'config'), DECEBAL_BACKEND='x11',
                   DECEBAL_SYSTEM_AUTOMATIC='0', DBUS_SESSION_BUS_ADDRESS=f'unix:path={tmp}/no-session-bus',
                   DBUS_SYSTEM_BUS_ADDRESS=f'unix:path={tmp}/no-system-bus', RDWM_SOCK=str(root / 'no-wm'),
                   RDWMCTL='/nonexistent/rdwmctl', DECSETTINGS_DEBUG_LAYOUT=str(root / 'layout.json'),
                   DECEBAL_THEME_PREVIEW_BIN=os.environ.get('SETTINGS_RENDERER_BIN', str(installed / 'decthemectl')),
                   DECEBAL_DECO_RENDERER=str(installed / 'decebal-deco-preview'))
        theme_source = Path(os.environ.get('SETTINGS_THEMES_DIR', str(Path(__file__).resolve().parents[2] / 'themes')))
        shutil.copytree(theme_source, root / 'themes', symlinks=False)
        env['DECEBAL_THEMES'] = str(root / 'themes')
        env['DECEBAL_SYSTEM_THEMES'] = str(root / 'empty')
        result['theme_fixture_source'] = str(theme_source)
        fake = root / 'fake-bin'
        fake.mkdir()
        for tool in ('curl', 'wget', 'nmcli', 'pactl', 'wpctl', 'playerctl', 'bluetoothctl', 'brightnessctl', 'decutil', 'rdwmctl'):
            stub = fake / tool
            stub.write_text('#!/bin/sh\nexit 1\n')
            stub.chmod(0o700)
        env['PATH'] = str(fake) + ':' + env.get('PATH', '/usr/bin:/bin')
        result['renderer_sha256'] = hashlib.sha256(Path(env['DECEBAL_THEME_PREVIEW_BIN']).read_bytes()).hexdigest()
        result.update(display=display, renderer=env['DECEBAL_THEME_PREVIEW_BIN'], deco_renderer=env['DECEBAL_DECO_RENDERER'])
        def run(*args):
            return subprocess.check_output(args, env=env, text=True, stderr=subprocess.DEVNULL, timeout=8).strip()
        def wait(fn, timeout=15):
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                try:
                    value = fn()
                    if value:
                        return value
                except (OSError, ValueError, subprocess.CalledProcessError, IndexError):
                    pass
                time.sleep(.05)
            raise AssertionError('timed out waiting for private native window/layout')
        def doc():
            return json.loads((root / 'layout.json').read_text())
        def capture(name, layout):
            (out / f'{name}.json').write_text(json.dumps(layout, indent=2))
            run('import', '-window', window, str(out / f'{name}.png'))
        try:
            with (out / 'xvfb.log').open('w') as log:
                xvfb = subprocess.Popen(['Xvfb', display, '-screen', '0', '1400x1800x24', '-nolisten', 'tcp'], env=env, stdout=log, stderr=log)
            processes.append(xvfb)
            def owned():
                assert xvfb.poll() is None, 'private Xvfb exited (possible display collision)'
                lock = Path(f'/tmp/.X{display[1:]}-lock')
                return lock.exists() and int(lock.read_text().strip()) == xvfb.pid and Path(f'/tmp/.X11-unix/X{display[1:]}').exists()
            wait(owned)
            cfg = root / 'config.toml'
            cfg.write_text('[general]\nborder_px = 2\ngap_inner = 6\n')
            with (out / 'settings.log').open('w') as log:
                app = subprocess.Popen([str(binary), '--config', str(cfg)], env=env, stdout=log, stderr=log)
            processes.append(app)
            window = wait(lambda: run('xdotool', 'search', '--class', 'DecSettings').splitlines()[0])
            result['window_xid'] = window
            result['window_title'] = run('xdotool', 'getwindowname', window)
            run('xdotool', 'windowsize', window, '1040', '1600')
            wait(doc)
            # Drain initial native exposure and discovery workers before interacting.
            time.sleep(float(os.environ.get('SETTINGS_INITIAL_SETTLE', '4')))
            before = doc()
            assert before['pane'] == 'general', before['pane']
            capture('before-click', before)
            pane = next(p for p in before['panes'] if p['id'] == 'appearance')
            assert pane['visible'], 'Appearance sidebar target must be visible before the sole click'
            rect = pane['rect']
            x, y, w, h = rect
            run('xdotool', 'mousemove', '--window', window, str(int(x + w / 2)), str(int(y + h / 2)), 'click', '1')
            result['click_count'] = 1
            # Re-enter after discovery to bypass the old first-open wake bug; this
            # diagnostic must exercise previews rather than merely their wake bug.
            time.sleep(2)
            general = next(p for p in before['panes'] if p['id'] == 'general')['rect']
            gx, gy, gw, gh = general
            run('xdotool', 'mousemove', '--window', window, str(int(gx + gw / 2)), str(int(gy + gh / 2)), 'click', '1')
            run('xdotool', 'mousemove', '--window', window, str(int(x + w / 2)), str(int(y + h / 2)), 'click', '1')
            result['click_count'] = 3
            # Exercise browsing only. No Escape, Apply, or live desktop interaction.
            start = time.monotonic()
            cache = root / 'cache/decebal/theme-previews'
            corrupted = False
            deadline = start + float(os.environ.get('SETTINGS_WAIT_SECONDS', '45'))
            while time.monotonic() < deadline:
                assert app.poll() is None, f'Settings exited unexpectedly: status {app.returncode}'
                current = doc()
                status = Path(f'/proc/{app.pid}/status').read_text()
                fields = dict(line.split(':', 1) for line in status.splitlines() if ':' in line)
                pngs = list(cache.glob('*.png'))
                result['samples'].append({
                    'seconds': round(time.monotonic() - start, 2), 'frames': current['frames'],
                    'pane': current['pane'], 'pngs': len(pngs), 'rss': fields.get('VmRSS', '').strip(),
                    'threads': fields.get('Threads', '').strip(),
                    'fds': len(list(Path(f'/proc/{app.pid}/fd').iterdir())), 'exit_status': app.poll()})
                # Alternating scroll bursts keep asynchronous discovery/completion and
                # paint active, including on the old missing-wake baseline.
                direction = '5' if int(time.monotonic() - start) % 8 < 4 else '4'
                run('xdotool', 'mousemove', '--window', window, '850', '600',
                    'click', '--repeat', '3', '--delay', '35', direction)
                if os.environ.get('SETTINGS_CORRUPT_CACHE') == '1' and pngs and not corrupted:
                    pngs[0].write_bytes(b'not a PNG: private diagnostic fixture')
                    corrupted = True
                time.sleep(.25)
            assert any(s['pngs'] for s in result['samples']), 'No native previews generated; not a valid preview stress run'
            final = doc()
            capture('after-stress', final)
            result.update(final_pane=final['pane'], final_frames=final['frames'],
                          unexpected_exit=app.poll(), corrupted_private_png=corrupted)
            # Reload the same private cache in a fresh process, so corrupt data is
            # decoded again rather than hidden by the first process's pixmap cache.
            app.terminate()
            result['controlled_cold_shutdown'] = app.wait(timeout=5)
            (root / 'layout.json').unlink(missing_ok=True)
            with (out / 'settings-warm.log').open('w') as log:
                warm = subprocess.Popen([str(binary), '--config', str(cfg), 'appearance'], env=env, stdout=log, stderr=log)
            processes.append(warm)
            window = wait(lambda: run('xdotool', 'search', '--class', 'DecSettings').splitlines()[0])
            wait(doc)
            for _ in range(12):
                assert warm.poll() is None, f'Warm Settings exited unexpectedly: status {warm.returncode}'
                run('xdotool', 'mousemove', '--window', window, '700', '500', 'click', '5')
                time.sleep(.2)
            result['warm_unexpected_exit'] = warm.poll()
            result['warm_frames'] = doc()['frames']
            capture('after-warm-cache', doc())
        except Exception as exc:
            failure = str(exc)
        finally:
            for proc in reversed(processes):
                proc.terminate()
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait()
            if (root / 'layout.json').exists():
                shutil.copyfile(root / 'layout.json', out / 'last-layout.json')
    result.update(passed=failure is None, failure=failure)
    (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({k: v for k, v in result.items() if k != 'samples'}, indent=2))
    assert failure is None, failure


if __name__ == '__main__':
    main()
