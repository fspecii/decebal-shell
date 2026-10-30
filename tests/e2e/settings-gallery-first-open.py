#!/usr/bin/env python3
"""Native first-open regression, using exactly one Appearance click.

SETTINGS_BIN=/absolute/decsettings SETTINGS_EVIDENCE_DIR=/tmp/evidence python3 ...
SETTINGS_EXPECT_FAILURE=1 records an expected broken baseline (exit zero only on failure).
No input is sent after the sole sidebar click. Requires Xvfb, xdotool, import.
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
    expected = os.environ.get('SETTINGS_EXPECT_FAILURE') == '1'
    installed = Path('/home/ambsd/.local/share/rdwm-decebal/bin')
    result = {'binary': str(binary), 'sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'expected_failure': expected, 'click_count': 0, 'samples': []}
    processes = []
    failure = None
    with tempfile.TemporaryDirectory(prefix='gallery-first-open-') as tmp:
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
                   DECEBAL_THEME_PREVIEW_BIN=str(installed / 'decthemectl'),
                   DECEBAL_DECO_RENDERER=str(installed / 'decebal-deco-preview'))
        result.update(display=display, renderer=env['DECEBAL_THEME_PREVIEW_BIN'], deco_renderer=env['DECEBAL_DECO_RENDERER'])
        def run(*args):
            return subprocess.check_output(args, env=env, text=True, stderr=subprocess.DEVNULL).strip()
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
            # From here on: filesystem reads and exact-XID captures only. No X input.
            start = time.monotonic()
            previous = None
            first_gallery = None
            gallery_selected = False
            first_png = None
            post_png = None
            deadline = start + float(os.environ.get('SETTINGS_WAIT_SECONDS', '25'))
            while time.monotonic() < deadline:
                current = doc()
                gallery_selected |= current['pane'] == 'appearance'
                pngs = list((root / 'cache/decebal/theme-previews').glob('*.png'))
                sample = {'seconds': round(time.monotonic() - start, 3), 'frames': current['frames'],
                          'loaded_thumbs': current.get('loaded_thumbs', 0),
                          'theme_previews_painted': current.get('theme_previews_painted', 0), 'pngs': len(pngs),
                          'cards': sum(c['id'].startswith('l:tcard:') for c in current['ctls'])}
                state = tuple(sample[k] for k in ('frames', 'theme_previews_painted', 'pngs', 'cards'))
                if state != previous:
                    result['samples'].append(sample)
                    previous = state
                if first_gallery is None and sample['cards']:
                    first_gallery = sample
                    capture('gallery-first-frame', current)
                if first_png is None and pngs:
                    first_png = sample
                    capture('preview-first-completion', current)
                if first_png and sample['frames'] > first_png['frames'] and sample['theme_previews_painted'] > 0:
                    post_png = sample
                    break
                time.sleep(.05)
            # Layout dumps are written during paint; let the native surface present.
            time.sleep(.4)
            final = doc()
            capture('after-idle', final)
            result.update(first_gallery=first_gallery, first_png=first_png, post_png=post_png,
                          final_pane=final['pane'], final_frames=final['frames'])
            assert gallery_selected, 'sole click did not navigate to Appearance'
            assert first_gallery, 'gallery controls never appeared after the sole Appearance click'
            assert first_png, 'native renderer never completed a PNG'
            assert post_png, 'native preview completion did not cause a further frame with painted theme previews'
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
    if expected:
        assert failure in ('gallery controls never appeared after the sole Appearance click',
                           'native preview completion did not cause a further frame with painted theme previews'), \
            f'baseline must fail a regression assertion, not setup: {failure}'
    else:
        assert failure is None, failure


if __name__ == '__main__':
    main()
