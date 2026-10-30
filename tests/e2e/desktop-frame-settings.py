#!/usr/bin/env python3
"""Real Settings control -> native rdwm frame integration, on owned private Xvfb.

SETTINGS_BIN=/pinned/decsettings RDWM_BIN_DIR=/pinned/wm/bin python3 ... /tmp/evidence
Optional OLD_RDWM_BIN=/pinned/older/rdwm also proves the upgraded-CLI/older-WM guard.
No fake WM, schema or state. No live display, bus, configuration or input.
"""
from pathlib import Path
import ctypes as C
import hashlib
import json
import os
import signal
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib
from PIL import Image, ImageGrab

settings = Path(os.environ['SETTINGS_BIN']).resolve()
bins = Path(os.environ['RDWM_BIN_DIR']).resolve()
old = Path(os.environ['OLD_RDWM_BIN']).resolve() if os.environ.get('OLD_RDWM_BIN') else None
out = Path(sys.argv[1]).resolve()
out.mkdir(mode=0o700, parents=True, exist_ok=True)

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

result = {'settings': {'path': str(settings), 'sha256': sha(settings)},
          'rdwmctl': {'path': str(bins / 'rdwmctl'), 'sha256': sha(bins / 'rdwmctl')}, 'cases': []}
build = None
if os.environ.get('RDWM_BUILD_MANIFEST'):
    manifest_path = Path(os.environ['RDWM_BUILD_MANIFEST'])
    build = json.loads(manifest_path.read_text())
    assert build['binaries']['rdwmctl'] == result['rdwmctl']['sha256']
    result['build'] = {'revision': build['head'], 'input_fingerprint': build['input_fingerprint'], 'manifest_sha256': sha(manifest_path)}

def native_stack(display):
    lib = C.CDLL('libX11.so.6')
    pointer, window = C.c_void_p, C.c_ulong
    lib.XOpenDisplay.argtypes = [C.c_char_p]
    lib.XOpenDisplay.restype = pointer
    lib.XDefaultRootWindow.argtypes = [pointer]
    lib.XDefaultRootWindow.restype = window
    lib.XQueryTree.argtypes = [pointer, window, C.POINTER(window), C.POINTER(window), C.POINTER(C.POINTER(window)), C.POINTER(C.c_uint)]
    lib.XFree.argtypes = [pointer]
    lib.XCloseDisplay.argtypes = [pointer]
    connection = lib.XOpenDisplay(display.encode())
    assert connection
    try:
        root, parent, children, count = window(), window(), C.POINTER(window)(), C.c_uint()
        assert lib.XQueryTree(connection, lib.XDefaultRootWindow(connection), C.byref(root), C.byref(parent), C.byref(children), C.byref(count))
        order = [children[i] for i in range(count.value)]
        lib.XFree(children)
        return order
    finally:
        lib.XCloseDisplay(connection)


def cpu_ticks(pid):
    fields = Path(f'/proc/{pid}/stat').read_text().split(') ', 1)[1].split()
    return int(fields[11]) + int(fields[12])


def compositor_owner(display):
    lib = C.CDLL('libX11.so.6')
    lib.XOpenDisplay.argtypes = [C.c_char_p]
    lib.XOpenDisplay.restype = C.c_void_p
    lib.XInternAtom.argtypes = [C.c_void_p, C.c_char_p, C.c_int]
    lib.XInternAtom.restype = C.c_ulong
    lib.XGetSelectionOwner.argtypes = [C.c_void_p, C.c_ulong]
    lib.XGetSelectionOwner.restype = C.c_ulong
    lib.XCloseDisplay.argtypes = [C.c_void_p]
    connection = lib.XOpenDisplay(display.encode())
    assert connection
    try:
        return lib.XGetSelectionOwner(connection, lib.XInternAtom(connection, b'_NET_WM_CM_S0', 0))
    finally:
        lib.XCloseDisplay(connection)


def wait(fn, label, seconds=15):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        try:
            value = fn()
            if value:
                return value
        except (OSError, ValueError, IndexError, subprocess.CalledProcessError):
            pass
        time.sleep(.04)
    raise AssertionError(label)

for label, wm_binary, supports in [('updated-wm', bins / 'rdwm', True)] + ([('older-wm-new-cli', old, False)] if old else []):
    case = out / label
    case.mkdir(exist_ok=True)
    record = {'case': label, 'wm': str(wm_binary), 'wm_sha256': sha(wm_binary), 'supports_frame': supports}
    if supports and build:
        assert record['wm_sha256'] == build['binaries']['rdwm'], 'candidate differs from compiled manifest'
    processes, logs = [], []
    try:
        with tempfile.TemporaryDirectory(prefix='frame-settings-') as temporary:
            root = Path(temporary)
            env = dict(os.environ)
            for key in list(env):
                if key.startswith(('DECEBAL_', 'DECSETTINGS_', 'RDWM', 'XDG_', 'DBUS_')) or key in ('HOME', 'DISPLAY', 'WAYLAND_DISPLAY', 'XAUTHORITY', 'SESSION_MANAGER', 'AT_SPI_BUS_ADDRESS', 'HYPRLAND_INSTANCE_SIGNATURE'):
                    env.pop(key)
            for key in ('HOME', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_STATE_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR', 'XDG_CONFIG_DIRS', 'XDG_DATA_DIRS', 'DECEBAL_CONFIG', 'DECEBAL_THEMES', 'DECEBAL_SYSTEM_THEMES'):
                path = root / key
                path.mkdir(mode=0o700)
                env[key] = str(path)
            tools = root / 'bin'
            tools.mkdir()
            for name in ('curl', 'gsettings', 'omarchy-theme-set', 'playerctl', 'wpctl', 'pactl', 'brightnessctl', 'nmcli', 'bluetoothctl', 'decutil', 'decbar', 'decdock', 'decwidgets', 'xdg-open'):
                path = tools / name
                path.write_text('#!/bin/sh\nexit 1\n')
                path.chmod(0o755)
            display = next(f':{n}' for n in range(880, 940) if not Path(f'/tmp/.X11-unix/X{n}').exists() and not Path(f'/tmp/.X{n}-lock').exists())
            env.update(DISPLAY=display, DECEBAL_BACKEND='x11', XDG_SESSION_TYPE='x11',
                       DECEBAL_SYSTEM_AUTOMATIC='0', RDWMCTL=str(bins / 'rdwmctl'),
                       RDWM_SOCK=str(Path(env['XDG_RUNTIME_DIR']) / 'rdwm.sock'),
                       DBUS_SESSION_BUS_ADDRESS='unix:path=' + temporary + '/no-session',
                       DBUS_SYSTEM_BUS_ADDRESS='unix:path=' + temporary + '/no-system',
                       DECSETTINGS_DEBUG_LAYOUT=str(root / 'layout.json'),
                       DECEBAL_THEME_PREVIEW_BIN=str(settings.parent / 'decthemectl'),
                       PATH=str(tools) + ':/usr/bin:/bin')
            shell_path = Path(env['DECEBAL_CONFIG']) / 'decebal.json'
            shell = {'theme': {'name': 'macos', 'mode': 'dark', 'palette': 'nord'},
                     'dock': {'enabled': False}, 'bar': {'enabled': False}, 'widgets': {'enabled': False},
                     'future_fixture': {'keep': ['untouched', 7]}}
            shell_path.write_text(json.dumps(shell, indent=2) + '\n')
            before_shell = shell_path.read_bytes()
            wallpaper = root / 'wallpaper.png'
            Image.new('RGB', (600, 400), (50, 90, 140)).save(wallpaper)
            config = root / 'config.toml'
            config.write_text(f'''# Preserve fixture comment
[general]
border_px = 0
[bar]
show = false
[decoration]
enabled = false
[wallpaper]
image = {str(wallpaper)!r}
mode = 'fit'
color = '#102030'
[[rule]]
class = 'DecSettings'
floating = true
''')
            env['RDWM_CONFIG'] = str(config)
            original = tomllib.loads(config.read_text())
            def start(argv, name):
                log = (case / (name + '.log')).open('w')
                logs.append(log)
                process = subprocess.Popen(list(map(str, argv)), env=env, stdout=log, stderr=log, start_new_session=True)
                processes.append(process)
                return process
            def run(*argv):
                return subprocess.check_output(list(map(str, argv)), env=env, text=True, stderr=subprocess.DEVNULL, timeout=8).strip()
            def state():
                return json.loads(run(bins / 'rdwmctl', 'state'))['state']
            def document():
                return json.loads((root / 'layout.json').read_text())
            def controls():
                return [c for c in document()['ctls'] if c['id'].startswith('r:wallpaper.frame:')]
            def strip_windows():
                process = subprocess.run(['xdotool', 'search', '--class', '^RdwmDesktopFrame$'], env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=5)
                return process.stdout.splitlines()
            server = start(['Xvfb', display, '-screen', '0', '1400x1800x24', '-nolisten', 'tcp', '-noreset'], 'xvfb')
            wait(lambda: server.poll() is None and Path(f'/tmp/.X{display[1:]}-lock').exists() and int(Path(f'/tmp/.X{display[1:]}-lock').read_text()) == server.pid and Path(f'/tmp/.X11-unix/X{display[1:]}').exists(), 'owned Xvfb')
            if os.environ.get('FRAME_PICOM_CONFIG'):
                picom = shutil.which('picom')
                assert picom, 'picom is required for this compositor gate'
                private_picom = root / 'picom.conf'
                shutil.copy2(os.environ['FRAME_PICOM_CONFIG'], private_picom)
                compositor = start([picom, '--config', private_picom], 'picom')
                wait(lambda: compositor_owner(display), 'private compositor selection')
                assert compositor.poll() is None
                record['compositor'] = {'binary_sha256': sha(Path(picom)), 'config_sha256': sha(private_picom), 'selection_owner': compositor_owner(display)}
            wm = start([wm_binary, '--backend', 'x11', '--config', config], 'wm')
            wait(lambda: Path(env['RDWM_SOCK']).exists(), 'private WM socket')
            wait(state, 'actual native WM state')
            assert isinstance(state()['wallpaper'].get('frame'), dict) == supports, 'baseline WM capability differs from requested test case'
            app = start([settings, '--config', config, 'wallpaper'], 'settings')
            window = wait(lambda: run('xdotool', 'search', '--class', '^DecSettings$').splitlines()[0], 'Settings window')
            wait(lambda: document()['pane'] == 'wallpaper', 'Wallpaper route')
            run('xdotool', 'windowsize', window, '1080', '1600')
            run('xdotool', 'windowmove', window, '150', '80')
            run('xdotool', 'windowactivate', '--sync', window)
            wait(lambda: len(state()['windows']) == 1, 'one managed Settings instance')
            assert app.poll() is None and wm.poll() is None
            def capture(name):
                (case / (name + '.json')).write_text(json.dumps(document(), indent=2) + '\n')
                run('import', '-window', window, str(case / (name + '.png')))
                ImageGrab.grab(xdisplay=display).save(case / (name + '-root.png'))
            if supports:
                expected = {'r:wallpaper.frame:' + key for key in ('enabled', 'radius', 'thickness', 'color')}
                wait(lambda: {c['id'] for c in controls()} == expected, 'four actual schema/runtime-gated frame controls')
                assert not strip_windows() and state()['wallpaper']['frame']['enabled'] is False
                capture('before-enable')
                clicks = []
                for enabled, count in [(True, 4), (False, 0)]:
                    target = next(c for c in controls() if c['id'] == 'r:wallpaper.frame:enabled')
                    x, y, w, h = target['rect']
                    assert target['enabled'] and x >= 0 and y >= 0 and y + h <= 1600, target
                    run('xdotool', 'mousemove', '--window', window, round(x + w / 2), round(y + h / 2), 'click', '1')
                    wait(lambda: state()['wallpaper']['frame']['enabled'] is enabled and len(strip_windows()) == count, 'actual UI toggle must update native WM and strip count')
                    wait(lambda: next(c for c in controls() if c['id'] == target['id'])['value'] is enabled, 'toggle presentation')
                    after = tomllib.loads(config.read_text())
                    assert after['wallpaper']['image'] == original['wallpaper']['image']
                    assert after['wallpaper']['mode'] == original['wallpaper']['mode']
                    assert after['wallpaper']['color'] == original['wallpaper']['color']
                    assert '# Preserve fixture comment' in config.read_text()
                    assert shell_path.read_bytes() == before_shell, 'frame UI changed theme/unknown shell preferences'
                    assert app.poll() is None and wm.poll() is None
                    clicks.append({'enabled': enabled, 'surface_count': len(strip_windows()), 'control': target})
                    capture('enabled' if enabled else 'disabled')
                # Exercise the real sibling daemons and actual embedded mpv, not
                # a fabricated desktop marker. The generated clip is a QA fixture.
                video = root / 'layer-fixture.mp4'
                subprocess.run(['ffmpeg', '-nostdin', '-v', 'error', '-f', 'lavfi', '-i',
                                'color=c=0x22aa88:s=640x360:r=10:d=2', '-an', '-c:v', 'libx264',
                                '-threads', '1', '-pix_fmt', 'yuv420p', '-y', str(video)],
                               env=env, check=True, timeout=20, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
                widget_dir = Path(env['XDG_CONFIG_HOME']) / 'decebal/widgets'
                widget_dir.mkdir(parents=True)
                (widget_dir / 'frame-edge-probe.toml').write_text("""format = 1
id = "frame-edge-probe"
name = "Frame edge layer fixture"
size = [180, 120]
refresh = 1000
[root]
type = "col"
background = "#f2c94c"
radius = 0
padding = 0
""")
                shell['widgets'] = {'enabled': True, 'allow_exec': False, 'items': [
                    {'use': 'frame-edge-probe', 'place': 'desktop', 'anchor': 'top-left', 'x': 0, 'y': 0, 'enabled': True}]}
                shell_path.write_text(json.dumps(shell, indent=2) + '\n')
                hosts_shell_before = shell_path.read_bytes()
                live_config = Path(env['XDG_CONFIG_HOME']) / 'decebal/live-wallpaper.json'
                live_config.write_text(json.dumps({'enabled': True, 'video': str(video), 'poster': str(wallpaper),
                                                  'fps': 10, 'pause_fullscreen': True}) + '\n')
                widget_host = start([settings.parent / 'decwidgets', 'daemon'], 'real-widgets')
                wall_host = start([settings.parent / 'decwall', 'daemon'], 'real-wallpaper')
                def named_windows(class_name):
                    process = subprocess.run(['xdotool', 'search', '--class', class_name], env=env, text=True,
                                             stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=5)
                    return [int(value) for value in process.stdout.splitlines()]
                widget_ids = wait(lambda: named_windows('^decwidgets$'), 'real widget host window')
                for widget_id in widget_ids:
                    assert '_NET_WM_STATE_BELOW' in run('xprop', '-id', widget_id, '_NET_WM_STATE'), 'desktop widget stacking role missing'
                live_ids = wait(lambda: named_windows('^Decwall$'), 'real live wallpaper desktop window')
                def player_running():
                    runtime = json.loads(run(settings.parent / 'decwall', 'status'))['runtime']
                    return runtime if runtime.get('players') and all(p['alive'] and p['paused'] is False for p in runtime['players']) and not runtime.get('errors') else None
                player = wait(player_running, 'actual mpv player must be alive and playing')
                record['real_hosts'] = {'widgets_sha256': sha(settings.parent / 'decwidgets'),
                                        'wallpaper_host_sha256': sha(settings.parent / 'decwall'),
                                        'video_sha256': sha(video), 'widget_windows': widget_ids,
                                        'live_windows': live_ids, 'mpv_runtime': player,
                                        'fixture_video_audio': False, 'before_frame_stack': native_stack(display)}
                before_layers_ticks = {str(process.pid): cpu_ticks(process.pid) for process in (wm, widget_host)}

                wait(lambda: ImageGrab.grab(xdisplay=display).convert('RGB').getpixel((5, 5)) == (242, 201, 76), 'edge widget pixel before enabling frame')
                target = next(c for c in controls() if c['id'] == 'r:wallpaper.frame:enabled')
                x, y, w, h = target['rect']
                run('xdotool', 'mousemove', '--window', window, round(x + w / 2), round(y + h / 2), 'click', '1')
                wait(lambda: state()['wallpaper']['frame']['enabled'] is True and len(strip_windows()) == 4, 'enable frame alongside real hosts')
                def ordered_hosts():
                    stack = native_stack(display)
                    frames = [int(value) for value in strip_windows()]
                    if not all(value in stack for value in live_ids + frames + widget_ids):
                        return None
                    return stack if (max(stack.index(value) for value in live_ids) < min(stack.index(value) for value in widget_ids)
                                     and max(stack.index(value) for value in widget_ids) < stack.index(int(window))
                                     and stack.index(int(window)) < min(stack.index(value) for value in frames)) else None
                wait(ordered_hosts, 'real video < widgets < app < shell frame stacking')
                for _ in range(3):
                    run(bins / 'rdwmctl', 'action', 'view', '2')
                    run(bins / 'rdwmctl', 'action', 'view', '1')
                    time.sleep(.2)
                    assert ordered_hosts(), 'workspace restack changed shell frame/widget ordering'
                    pixels = ImageGrab.grab(xdisplay=display).convert('RGB')
                    assert pixels.getpixel((5, 5)) == (16, 32, 48), 'shell frame does not cover outer widget edge'
                    assert pixels.getpixel((30, 30)) == (242, 201, 76), 'shell frame covers widget inside its cutout'
                run(bins / 'rdwmctl', 'action', 'view', '2')
                time.sleep(.2)
                empty_stack = native_stack(display)
                assert max(empty_stack.index(value) for value in widget_ids) < min(empty_stack.index(int(value)) for value in strip_windows()), 'empty-workspace widgets moved above the shell frame'
                assert ImageGrab.grab(xdisplay=display).convert('RGB').getpixel((5, 5)) == (16, 32, 48), 'empty-workspace frame does not cover outer card edge'
                run(bins / 'rdwmctl', 'action', 'view', '1')
                time.sleep(.2)
                start_ticks = {str(process.pid): cpu_ticks(process.pid) for process in (wm, widget_host)}
                begin = time.monotonic()
                samples = []
                for _ in range(20):
                    time.sleep(.1)
                    stack = ordered_hosts()
                    pixel = ImageGrab.grab(xdisplay=display).convert('RGB').getpixel((5, 5))
                    assert stack and pixel == (16, 32, 48), 'shell frame edge changes while actual video runs'
                    assert ImageGrab.grab(xdisplay=display).convert('RGB').getpixel((30, 30)) == (242, 201, 76), 'widget inside the cutout changed'
                    assert all(process.poll() is None for process in (wm, app, widget_host, wall_host))
                    samples.append({'seconds': round(time.monotonic() - begin, 3), 'edge_pixel': pixel, 'stack': stack})
                tick_delta = {str(process.pid): cpu_ticks(process.pid) - start_ticks[str(process.pid)] for process in (wm, widget_host)}
                assert tick_delta[str(wm.pid)] <= 30 and tick_delta[str(widget_host.pid)] <= 60, ('stack feedback CPU', tick_delta)
                assert shell_path.read_bytes() == hosts_shell_before, 'frame toggling mutated theme/widget configuration'
                assert player_running(), 'real player stopped during stacking checks'
                record['real_hosts'].update(cpu_ticks=tick_delta, sample_duration=time.monotonic() - begin,
                                            samples=samples, layer_order='video < widgets < apps < shell frame')
                capture('frame-with-real-video-and-edge-widget')
                # Join the real colour producer to the real WM consumer. The
                # bottom edge stays clear of the top bar, so it measures the
                # frame itself rather than the panel covering it.
                shell['bar']['enabled'] = True
                shell_path.write_text(json.dumps(shell, indent=2) + '\n')
                real_bar = start([settings.parent / 'decbar', '--app', 'Frame fixture'], 'real-bar')
                def themed_edge():
                    colour = json.loads(run(settings.parent / 'decthemectl', 'get', '--json'))['colors']['background']
                    expected_rgb = tuple(bytes.fromhex(colour[1:]))
                    return expected_rgb if ImageGrab.grab(xdisplay=display).convert('RGB').getpixel((700, 1795)) == expected_rgb else None
                first_colour = wait(themed_edge, 'real bar startup must colour the native frame')
                shell['theme']['palette'] = 'gruvbox'
                shell_path.write_text(json.dumps(shell, indent=2) + '\n')
                second_colour = wait(lambda: (colour if colour != first_colour else None) if (colour := themed_edge()) else None,
                                     'live palette change must colour the native frame')
                assert real_bar.poll() is None and player_running()
                record['real_hosts']['bar_theme_bridge'] = {'bar_sha256': sha(settings.parent / 'decbar'), 'startup_rgb': first_colour, 'changed_rgb': second_colour}
                capture('frame-with-real-bar-theme-colour')
                real_bar.terminate()
                real_bar.wait(timeout=4)
                target = next(c for c in controls() if c['id'] == 'r:wallpaper.frame:enabled')
                x, y, w, h = target['rect']
                run('xdotool', 'mousemove', '--window', window, round(x + w / 2), round(y + h / 2), 'click', '1')
                wait(lambda: not strip_windows() and state()['wallpaper']['frame']['enabled'] is False, 'disable frame while real hosts remain running')
                assert all(process.poll() is None for process in (widget_host, wall_host)) and player_running()
                record.update(controls=sorted(expected), clicks=clicks, wallpaper_and_shell_preserved=True)
            else:
                # Allow the actual runtime watcher to replace initial offline-only state.
                time.sleep(2)
                assert not controls(), 'new CLI must not offer frame controls to older live WM'
                assert config.read_text().find('[wallpaper.frame]') == -1
                assert shell_path.read_bytes() == before_shell
                capture('unsupported-old-session')
                record['old_live_wm_hides_controls_despite_new_offline_cli'] = True
            if os.environ.get('FRAME_PICOM_CONFIG'):
                assert compositor.poll() is None and compositor_owner(display), 'private compositor stopped during acceptance'
            record.update(passed=True, settings_pid=app.pid, wm_pid=wm.pid, display=display)
    except Exception as error:
        record.update(passed=False, failure=str(error))
        if 'real_hosts' in record:
            try:
                record['real_hosts']['failure_stack'] = native_stack(display)
                record['real_hosts']['failure_edge_pixel'] = ImageGrab.grab(xdisplay=display).convert('RGB').getpixel((5, 5))
                record['real_hosts']['failure_cpu_ticks'] = {str(process.pid): cpu_ticks(process.pid) - before_layers_ticks[str(process.pid)] for process in (wm, widget_host)}
                ImageGrab.grab(xdisplay=display).save(case / 'failure-real-hosts.png')
            except Exception:
                pass  # Keep the original test failure if diagnostic capture also fails.
        raise
    finally:
        # Stop children before their display, avoiding misleading connection-loss
        # diagnostics during fixture cleanup.
        for process in reversed(processes):
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=4)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=4)
        for log in logs:
            log.close()
        result['cases'].append(record)
        (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps({'settings_sha256': result['settings']['sha256'],
                  'cases': [{'case': case['case'], 'passed': case['passed'], 'wm_sha256': case['wm_sha256'],
                             'host_cpu_ticks': case.get('real_hosts', {}).get('cpu_ticks')} for case in result['cases']]}))
