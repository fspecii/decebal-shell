#!/usr/bin/env python3
"""Same-process private native Windows taskbar palette regression.
DOCK_BIN=/absolute/decdock PALETTE_EVIDENCE_DIR=/tmp/proof python3 this-file
No live display/config, network/system tools, input events, or process-name kills.
"""
import os, pathlib, tempfile, subprocess, time, json, hashlib
from PIL import Image
out = pathlib.Path(os.environ['PALETTE_EVIDENCE_DIR']).resolve()
out.mkdir(parents=True, exist_ok=True)
binary = pathlib.Path(os.environ['DOCK_BIN']).resolve()
procs = []
with tempfile.TemporaryDirectory(prefix='windows-palette-private-') as td:
    r = pathlib.Path(td)
    for n in ['home', 'cfg', 'cache', 'data', 'state', 'run', 'empty', 'fake']:
        (r / n).mkdir(mode=0o700)
    env = {k: v for k, v in os.environ.items() if not k.startswith(('DECEBAL', 'DECDOCK', 'DECSETTINGS', 'RDWM', 'DBUS', 'XDG'))}
    for k in ['WAYLAND_DISPLAY', 'XAUTHORITY', 'SESSION_MANAGER', 'AT_SPI_BUS_ADDRESS']:
        env.pop(k, None)
    n = next((n for n in range(470, 560) if not pathlib.Path(f'/tmp/.X11-unix/X{n}').exists() and (not pathlib.Path(f'/tmp/.X{n}-lock').exists())))
    env.update(HOME=str(r / 'home'), DISPLAY=f':{n}', XDG_CONFIG_HOME=str(r / 'cfg'), XDG_DATA_HOME=str(r / 'data'), XDG_CACHE_HOME=str(r / 'cache'), XDG_STATE_HOME=str(r / 'state'), XDG_RUNTIME_DIR=str(r / 'run'), XDG_CONFIG_DIRS=str(r / 'empty'), XDG_DATA_DIRS=str(r / 'empty'), DECEBAL_CONFIG=str(r / 'cfg'), DECEBAL_THEMES=str(r / 'empty'), DECEBAL_SYSTEM_THEMES=str(r / 'empty'), DECEBAL_BACKEND='x11', DECEBAL_SYSTEM_AUTOMATIC='0', DBUS_SESSION_BUS_ADDRESS=f'unix:path={r}/no-session', DBUS_SYSTEM_BUS_ADDRESS=f'unix:path={r}/no-system', RDWM_SOCK=str(r / 'no-wm'), RDWMCTL='/nonexistent', DECDOCK_DEBUG_LAYOUT=str(r / 'layout'))
    for t in ['curl', 'wget', 'pactl', 'wpctl', 'nmcli', 'playerctl', 'bluetoothctl', 'brightnessctl', 'rdwmctl']:
        p = r / 'fake' / t
        p.write_text('#!/bin/sh\nexit 1\n')
        p.chmod(0o700)
    env['PATH'] = str(r / 'fake') + ':' + env.get('PATH', '/usr/bin:/bin')

    def run(*a):
        return subprocess.check_output(a, env=env, text=True, stderr=subprocess.DEVNULL, timeout=5).strip()
    try:
        log = (out / 'xvfb.log').open('w')
        x = subprocess.Popen(['Xvfb', f':{n}', '-screen', '0', '1280x800x24', '-nolisten', 'tcp'], env=env, stdout=log, stderr=log)
        procs.append(x)
        for _ in range(100):
            lock = pathlib.Path(f'/tmp/.X{n}-lock')
            if lock.exists() and int(lock.read_text()) == x.pid:
                break
            time.sleep(0.05)
        assert x.poll() is None and int(lock.read_text()) == x.pid
        (r / 'cfg/decebal.json').write_text(json.dumps({'theme': {'name': 'windows11', 'mode': 'dark', 'palette': 'catppuccin'}, 'dock': {'enabled': True}}))
        (r / 'cfg/dock-pinned-macos.json').write_text('{"version":1,"pinned":[],"order":[]}')
        results = []
        (r / 'cfg/dock-settings.json').write_text(json.dumps({'style': 'windows', 'windowsTheme': 'auto', 'transparency': 1.0}))
        lg = (out / 'dock.log').open('w')
        p = subprocess.Popen([str(binary)], env=env, stdout=lg, stderr=lg)
        procs.append(p)
        w = ''
        for _ in range(100):
            try:
                w = run('xdotool', 'search', '--name', '^decebal-dock$').splitlines()[0]
                break
            except Exception:
                time.sleep(0.05)
        assert w and p.poll() is None
        for mode in ['auto', 'light', 'dark']:
            for palette in ['catppuccin', 'nord', 'gruvbox']:
                for name, doc in [('decebal.json', {'theme': {'name': 'windows11', 'mode': 'dark', 'palette': palette}, 'dock': {'enabled': True}}), ('dock-settings.json', {'style': 'windows', 'windowsTheme': mode, 'transparency': 1.0})]:
                    temp = r / 'cfg' / (name + '.tmp')
                    temp.write_text(json.dumps(doc))
                    temp.replace(r / 'cfg' / name)
                time.sleep(float(os.environ.get('PALETTE_SETTLE_SECONDS', '2')))
                assert p.poll() is None, 'Dock exited while recolouring'
                assert run('xdotool', 'search', '--name', '^decebal-dock$').splitlines() == [w], 'Dock window changed'
                png = out / f'{mode}-{palette}.png'
                run('import', '-window', w, str(png))
                im = Image.open(png).convert('RGB')
                start = next(line.split() for line in (r / 'layout').read_text().splitlines() if line.startswith('start '))
                start_pixel = im.getpixel((round(float(start[1]) - 5), round(float(start[2]) - 5)))
                results.append({'mode': mode, 'palette': palette, 'pid': p.pid, 'window': w, 'pixel_2_20': im.getpixel((2, 20)), 'start_accent_pixel': start_pixel, 'size': im.size})
        (out / 'result.json').write_text(json.dumps({'binary': str(binary), 'sha256': hashlib.sha256(binary.read_bytes()).hexdigest(), 'fixture': {'name': 'windows11', 'mode': 'dark', 'palette': 'catppuccin'}, 'samples': results}, indent=2))
        print((out / 'result.json').read_text())
        assert all((len({(tuple(v['pixel_2_20']), tuple(v['start_accent_pixel'])) for v in results if v['mode'] == mode}) == 3 for mode in ['auto', 'light', 'dark'])), 'Palette changes did not recolour the same taskbar in every mode'
    finally:
        for p in reversed(procs):
            if p.poll() is None:
                p.terminate()
                p.wait(timeout=5)
