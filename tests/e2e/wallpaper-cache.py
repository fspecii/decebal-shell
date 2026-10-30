#!/usr/bin/env python3
"""Private native 73-image viewport/cache regression; never builds or uses live IPC.

DECSETTINGS_BIN=/absolute/decsettings WALLPAPER_CACHE_EVIDENCE=/tmp/evidence \
  python3 tests/e2e/wallpaper-cache.py

All images, config, caches, HOME, pointer and X11 surfaces belong to this fixture.
No session/system bus is reachable. Exit 77 means a required tool is missing.
"""
from pathlib import Path
import json
import os
import shutil
import subprocess
import tempfile
import time
from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
MAX_BYTES = 32 * 1024 * 1024
MAX_ENTRIES = 512


def command(env, *args):
    return subprocess.check_output([str(a) for a in args], env=env, text=True, timeout=10).strip()


def proc_sample(pid):
    io = dict(line.split(':', 1) for line in Path(f'/proc/{pid}/io').read_text().splitlines())
    fields = Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()
    return {'read_chars': int(io['rchar']), 'cpu_ticks': int(fields[11]) + int(fields[12])}


def main():
    for tool in ('Xvfb', 'xdotool', 'import'):
        if not shutil.which(tool):
            return 77
    binary = Path(os.environ.get('DECSETTINGS_BIN', str(ROOT/'target/release/decsettings'))).resolve()
    if not binary.is_file():
        return 77
    evidence = Path(os.environ.get('WALLPAPER_CACHE_EVIDENCE', tempfile.mkdtemp(prefix='wallpaper-cache-evidence-'))).resolve()
    evidence.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='wallpaper-cache-private-') as temp:
        base = Path(temp)
        env = dict(os.environ)
        for key in ('WAYLAND_DISPLAY', 'RDWM_SOCK', 'DBUS_SESSION_BUS_ADDRESS', 'DBUS_STARTER_ADDRESS', 'DBUS_STARTER_BUS_TYPE', 'DECSETTINGS_CONFIG', 'HYPRLAND_INSTANCE_SIGNATURE', 'XAUTHORITY', 'AT_SPI_BUS_ADDRESS', 'SESSION_MANAGER'):
            env.pop(key, None)
        for key, name in [('HOME', 'home'), ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'), ('XDG_DATA_HOME', 'data'), ('XDG_STATE_HOME', 'state'), ('XDG_RUNTIME_DIR', 'run'), ('XDG_CONFIG_DIRS', 'no-config'), ('XDG_DATA_DIRS', 'no-data'), ('DECEBAL_THEMES', 'themes'), ('DECEBAL_SYSTEM_THEMES', 'system-themes'), ('DECEBAL_PALETTES', 'palettes'), ('DECEBAL_WIDGETS', 'widgets')]:
            path = base/name
            path.mkdir(mode=0o700)
            env[key] = str(path)
        env.update(DECEBAL_CONFIG=str(base/'config'), DECEBAL_BACKEND='x11', DECEBAL_SYSTEM_AUTOMATIC='0', RDWMCTL='/nonexistent/rdwmctl', DBUS_SESSION_BUS_ADDRESS='unix:path=/nonexistent', DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent', DECSETTINGS_DEBUG_LAYOUT=str(base/'layout.json'))
        walls = base/'config/backgrounds'
        walls.mkdir()
        for i in range(73):
            Image.new('RGB', (64, 40), (i*3 % 255, 120, 200)).save(walls/f'{i:02}.png')
        config = base/'rdwm.toml'
        config.write_text('[general]\nborder_px=2\n[wallpaper]\nimage='+json.dumps(str(walls/'00.png'))+'\n')
        (base/'config/decebal.json').write_text('{"theme":{"name":"macos","mode":"dark"}}')
        xvfb = subprocess.Popen(['Xvfb', '-displayfd', '1', '-screen', '0', '3000x2000x24', '-nolisten', 'tcp'], stdout=subprocess.PIPE, stderr=(evidence/'xvfb.log').open('w'), text=True)
        env['DISPLAY'] = ':'+xvfb.stdout.readline().strip()
        app = subprocess.Popen([str(binary), '--config', str(config), 'wallpaper'], env=env, stdout=(evidence/'settings.log').open('w'), stderr=subprocess.STDOUT)
        results = []
        try:
            until = time.monotonic()+10
            while True:
                found = subprocess.run(['xdotool', 'search', '--class', 'DecSettings'], env=env, capture_output=True, text=True)
                if found.returncode == 0:
                    wid = found.stdout.splitlines()[0]
                    break
                assert app.poll() is None, 'Settings exited during startup'
                assert time.monotonic() < until, 'Settings did not open'
                time.sleep(.05)

            def sample():
                layout = json.loads((base/'layout.json').read_text())
                assert layout['loaded_thumbs'] <= MAX_ENTRIES
                assert layout['thumb_bytes'] <= MAX_BYTES
                return layout

            for width, height in json.loads(os.environ.get('WALLPAPER_CACHE_SIZES', '[[1759,1194],[2600,1700]]')):
                command(env, 'xdotool', 'windowsize', wid, width, height)
                command(env, 'xdotool', 'windowmove', wid, 0, 0)
                command(env, 'xdotool', 'windowfocus', wid)
                time.sleep(.3)
                command(env, 'xdotool', 'mousemove', '--window', wid, width-100, height//2)
                for step in range(int(os.environ.get('WALLPAPER_CACHE_STEPS', '5'))):
                    command(env, 'xdotool', 'mousemove', '--window', wid, width-100, height//2)
                    command(env, 'xdotool', 'click', '--repeat', 6, '--delay', 30, 5)
                    # Wait for every visible thumbnail centre to be real fixture
                    # pixels, not a loading placeholder. Captures are private.
                    until = time.monotonic()+8
                    shot = evidence/f'{width}x{height}-{step}.png'
                    while True:
                        layout = sample()
                        command(env, 'import', '-window', wid, shot)
                        image = Image.open(shot).convert('RGB')
                        if image.size != (width, height):
                            assert time.monotonic() < until, 'private window capture did not reach requested geometry'
                            time.sleep(.1)
                            continue
                        visible = []
                        missing = []
                        for item in layout['thumbs']:
                            x, y, w, h = item['rect']
                            left, right = max(2, x+2), min(width-2, x+w-2)
                            top, bottom = max(2, y+2), min(height-2, y+h-2)
                            if right > left and bottom > top:
                                cx, cy = round((left+right)/2), round((top+bottom)/2)
                                visible.append(dict(item, sample_point=[cx, cy]))
                                index = int(Path(item['id']).stem)
                                if image.getpixel((cx, cy)) != (index*3 % 255, 120, 200):
                                    missing.append(index)
                        if visible and not missing:
                            quiet_before = sample()
                            time.sleep(1.0)
                            quiet_after = sample()
                            if quiet_after['thumb_pending'] == 0 and quiet_after['thumb_decodes'] == quiet_before['thumb_decodes'] and quiet_after['scroll'] == quiet_before['scroll'] and quiet_after['scroll'] == layout['scroll'] and quiet_after['thumbs'] == layout['thumbs']:
                                break
                        assert time.monotonic() < until, f'visible placeholders did not settle: {missing}'
                        time.sleep(.1)
                    before = sample()
                    (evidence/f'{width}x{height}-{step}-before.json').write_text(json.dumps(before))
                    io_before = proc_sample(app.pid)
                    for item in visible[:3]:
                        x, y, w, h = item['rect']
                        command(env, 'xdotool', 'mousemove', '--window', wid, *item['sample_point'])
                    # Keep the captured cursor away from sampled image pixels.
                    command(env, 'xdotool', 'mousemove', '--window', wid, 4, 4)
                    time.sleep(.8)
                    after = sample()
                    (evidence/f'{width}x{height}-{step}-after.json').write_text(json.dumps(after))
                    io_after = proc_sample(app.pid)
                    assert after['thumb_decodes'] == before['thumb_decodes'], ('idle viewport decoded images again', {k:before[k] for k in ('frames','loaded_thumbs','thumb_decodes','thumb_pending','thumb_bytes','scroll')}, {k:after[k] for k in ('frames','loaded_thumbs','thumb_decodes','thumb_pending','thumb_bytes','scroll')})
                    assert io_after['read_chars'] - io_before['read_chars'] < 4096, 'idle viewport kept rereading images'
                    assert after['thumb_pending'] == 0, 'idle viewport retained a decode backlog'
                    command(env, 'import', '-window', wid, shot)
                    settled_pixels = Image.open(shot).convert('RGB')
                    for item in visible:
                        x, y, w, h = item['rect']
                        index = int(Path(item['id']).stem)
                        assert settled_pixels.getpixel(tuple(item['sample_point'])) == (index*3 % 255, 120, 200), ('thumbnail changed after redraw', item['id'], item['sample_point'], settled_pixels.getpixel(tuple(item['sample_point'])), before['scroll'], after['scroll'], settled_pixels.size)
                    wanted = sum(t['rect'][1]+t['rect'][3] > -t['rect'][3] and t['rect'][1] < height+t['rect'][3] for t in after['thumbs'])
                    results.append({'size': [width, height], 'step': step, 'scroll': after['scroll'], 'visible': len(visible), 'wanted_with_margin': wanted, 'loaded': after['loaded_thumbs'], 'cache_bytes': after['thumb_bytes'], 'idle_decodes': after['thumb_decodes']-before['thumb_decodes'], 'idle_read_chars': io_after['read_chars']-io_before['read_chars'], 'idle_cpu_ticks': io_after['cpu_ticks']-io_before['cpu_ticks'], 'pending': after['thumb_pending']})
                    assert after['thumb_pending'] <= wanted+1
            for size in {tuple(case['size']) for case in results}:
                assert any(tuple(case['size']) == size and case['wanted_with_margin'] > 32 for case in results), f'{size} fixture did not exceed the old cache capacity'
            (evidence/'result.json').write_text(json.dumps({'binary': str(binary), 'cases': results, 'all_visible_images_present': True, 'idle_decode_stable': True, 'bounded_cache': True}, indent=2)+'\n')
            print(f'PASS wallpaper cache: 73 fixtures, visible pixels, >32 working set, stable idle and scrolling; {evidence}')
        finally:
            app.terminate()
            app.wait(timeout=5)
            xvfb.terminate()
            xvfb.wait(timeout=5)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
