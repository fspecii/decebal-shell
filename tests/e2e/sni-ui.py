#!/usr/bin/python3
"""Real native surfaces + isolated SNI fixture. Never builds or uses a user bus."""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import dbus
from PIL import Image

parser = argparse.ArgumentParser(description=__doc__)
modes = parser.add_mutually_exclusive_group()
modes.add_argument('--probe-real-app', action='store_true', help='probe installed OBS protocol without native SNI UI assertions')
modes.add_argument('--tiny-rail', action='store_true', help='verify both apps remain accessible on a 600px-tall rail')
args = parser.parse_args()
repo = Path(os.environ['SNI_UI_REPO'])
binaries = Path(os.environ['MS_BIN_DIR'])
wm_bin = Path(os.environ['RDWM_BIN_DIR'])
for command in ('Xvfb', 'xdotool', 'xprop', 'import', 'dbus-daemon'):
    if not shutil.which(command):
        print('SKIP: missing ' + command); sys.exit(77)
for binary in (binaries / 'decbar', binaries / 'decdock', wm_bin / 'rdwm'):
    if not os.access(binary, os.X_OK):
        print('SKIP: missing executable ' + str(binary)); sys.exit(77)
match = re.fullmatch(r':([0-9]{1,4})', os.environ.get('E2E_DISPLAY', ':345'))
if not match or int(match[1]) < 2:
    raise SystemExit('Refusing live or invalid X display')
number = int(match[1])
if Path('/tmp/.X11-unix/X' + str(number)).exists() or Path('/tmp/.X' + str(number) + '-lock').exists():
    print('SKIP: occupied display'); sys.exit(77)
root = Path(tempfile.mkdtemp(prefix='decebal-sni-ui.'))
evidence = Path(os.environ.get('SNI_UI_EVIDENCE_DIR', root / 'evidence')).resolve()
evidence.mkdir(parents=True, exist_ok=True)
env = os.environ.copy()
for key in ('WAYLAND_DISPLAY', 'RDWM_SOCK', 'HYPRLAND_INSTANCE_SIGNATURE', 'DBUS_SESSION_BUS_PID', 'SESSION_MANAGER', 'PULSE_SERVER', 'PIPEWIRE_REMOTE', 'AT_SPI_BUS_ADDRESS', 'XAUTHORITY'):
    env.pop(key, None)
for key in list(env):
    if key.startswith(('DECBAR_DEBUG_', 'DECDOCK_DEBUG_')):
        env.pop(key)
for key, value in {'HOME':'home', 'XDG_CONFIG_HOME':'cfg', 'XDG_RUNTIME_DIR':'runtime',
        'XDG_CACHE_HOME':'cache', 'XDG_STATE_HOME':'state', 'XDG_DATA_HOME':'data',
        'XDG_DATA_DIRS':'nodata', 'XDG_CONFIG_DIRS':'noconfig', 'DECEBAL_SYSTEM_THEMES':'system-themes', 'DECEBAL_CONFIG':'cfg/omarchy', 'DECEBAL_THEMES':'cfg/themes'}.items():
    env[key] = str(root / value); Path(env[key]).mkdir(parents=True, exist_ok=True)
os.chmod(env['XDG_RUNTIME_DIR'], 0o700)
env.update(DISPLAY=':' + str(number), DECEBAL_BACKEND='x11', DECEBAL_SYSTEM_AUTOMATIC='0',
    DECEBAL_COLOR_SCHEME='dark', DBUS_SESSION_BUS_ADDRESS='unix:path=' + str(root / 'bus'),
    DBUS_SYSTEM_BUS_ADDRESS='unix:path=' + str(root / 'no-system-bus'),
    RDWMCTL=str(wm_bin / 'rdwmctl'), DECSETTINGS_CONFIG=str(root / 'cfg/rdwm.toml'),
    FAKE_STATE=str(root / 'fake'), DECBAR_POWER_SUPPLY=str(root / 'ps'))
env['PATH'] = ':'.join((str(repo / 'tests/fakebin'), str(wm_bin), str(binaries), env['PATH']))
Path(env['FAKE_STATE']).mkdir()
for name, value in {'vol':'.50', 'mute':'0', 'wifi':'enabled', 'btpower':'yes', 'bright':'50', 'log':''}.items():
    (Path(env['FAKE_STATE']) / name).write_text(value + '\n')
battery = root / 'ps/BAT0'; battery.mkdir(parents=True)
for name, value in {'type':'Battery', 'capacity':'64', 'status':'Discharging'}.items():
    (battery / name).write_text(value + '\n')
Path(env['DECSETTINGS_CONFIG']).write_text('[general]\ngap_inner = 6\n[decoration]\nenabled = true\n[bar]\nshow = false\n')
processes = []
logs = []
failed = False

def start(args, label, extra=None):
    output = (root / (label + '.log')).open('w'); logs.append(output)
    child_env = dict(env); child_env.update(extra or {})
    process = subprocess.Popen(list(map(str, args)), env=child_env, stdout=output,
        stderr=subprocess.STDOUT, start_new_session=True)
    processes.append(process)
    return process

def stop(process):
    if process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
        try: process.wait(timeout=4)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL); process.wait(timeout=2)

def run(*args):
    return subprocess.check_output(list(map(str, args)), env=env, stderr=subprocess.DEVNULL, text=True).strip()

def wait(check, reason, seconds=12):
    deadline = time.monotonic() + seconds
    last = None
    while time.monotonic() < deadline:
        try:
            result = check()
            if result: return result
        except (OSError, ValueError, KeyError, dbus.DBusException, subprocess.CalledProcessError) as error:
            last = error
        time.sleep(.05)
    raise AssertionError(reason + (': ' + str(last) if last else ''))

def read_debug(path):
    return json.loads(path.read_text())

def items(path):
    return read_debug(path).get('items', [])

def find_item(path, title):
    return next((item for item in items(path) if item['title'] == title), None)

def events(method=None, index=None):
    path = root / 'events.jsonl'
    result = [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []
    return [event for event in result if (method is None or event['method'] == method)
        and (index is None or event.get('item') == index)]

def window(name):
    return run('xdotool', 'search', '--onlyvisible', '--name', '^' + name + '$').splitlines()[0]

def origin(win):
    geo = dict(line.split('=', 1) for line in run('xdotool', 'getwindowgeometry', '--shell', win).splitlines())
    return int(geo['X']), int(geo['Y'])

def click_rect(win, rect, button=1, absolute=False):
    x, y, width, height = map(float, rect)
    ox, oy = (0, 0) if absolute else origin(win)
    run('xdotool', 'mousemove', '--sync', int(ox + x + width / 2), int(oy + y + height / 2), 'click', button)

def screenshot(name, win=None):
    time.sleep(.08)  # debug layout is written during draw, before the X11 upload
    destination = evidence / (name + '.png')
    run('import', '-window', win or 'root', destination)
    return destination

def assert_color(win, rect, color, label):
    # Independent native pixels, not a layout-only assertion. Opaque flat fixture
    # icon should contain its actual RGB after upload/premultiplication/scaling.
    def present():
        image = Image.open(screenshot(label, win)).convert('RGB')
        x, y, width, height = map(int, rect)
        crop = image.crop((max(0, x), max(0, y), min(image.width, x + width), min(image.height, y + height)))
        expected = tuple(bytes.fromhex(color))
        pixels = crop.get_flattened_data() if hasattr(crop, 'get_flattened_data') else crop.getdata()
        return sum(1 for pixel in pixels if max(abs(a-b) for a,b in zip(pixel, expected)) <= 3) >= 12
    wait(present, label + ': actual icon pixels absent')

def menu_rows(path):
    return (read_debug(path).get('menu') or {}).get('rows', [])

def menu_click(path, win, label):
    row = wait(lambda: next((r for r in menu_rows(path) if r['label'] == label), None), 'menu row ' + label)
    debug_menu = read_debug(path)['menu']
    rect = list(row['rect'])
    if 'origin' in debug_menu:
        rect[0] += debug_menu['origin'][0]; rect[1] += debug_menu['origin'][1]
        click_rect(win, rect, absolute=True)
    else:
        click_rect(win, rect)
    return row

def surface(style):
    config = Path(env['DECEBAL_CONFIG'])
    (config / 'decebal.json').write_text(json.dumps({'theme': 'caelestia' if style == 'rail' else 'macos',
        'bar': {'enabled': style != 'windows', 'position': 'left'}, 'dock': {'enabled': style == 'windows'}}))
    (config / 'dock-settings.json').write_text(json.dumps({'style':'windows','windowsAlign':'center','windowsSearch':'button'}))
    debug = root / (style + '-indicators.json')
    debug.unlink(missing_ok=True)
    if style == 'windows':
        args = [binaries / 'decdock']; name = 'decebal-dock'
    elif style == 'rail':
        args = [binaries / 'decbar', '--position', 'left']; name = 'decebal-bar'
    else:
        args = [binaries / 'decbar']; name = 'decebal-bar'
    process = start(args, style, {'DECBAR_DEBUG_INDICATORS': str(debug)})
    win = wait(lambda: window(name), style + ' native surface did not map')
    return process, win, debug

try:
    daemon = start(['dbus-daemon', '--session', '--nofork', '--address=' + env['DBUS_SESSION_BUS_ADDRESS']], 'bus')
    wait(lambda: (root / 'bus').is_socket(), 'private D-Bus did not start')
    xvfb = start(['Xvfb', env['DISPLAY'], '-screen', '0', ('1280x600x24' if args.tiny_rail else '1280x900x24'), '-nolisten', 'tcp'], 'xvfb')
    wait(lambda: Path('/tmp/.X11-unix/X' + str(number)).is_socket() and xvfb.poll() is None, 'private Xvfb did not start')
    wm = start([wm_bin / 'rdwm', '--backend', 'x11', '--config', env['DECSETTINGS_CONFIG']], 'rdwm')
    wait(lambda: 'window id' in run('xprop', '-root', '_NET_SUPPORTING_WM_CHECK'), 'private WM did not start')
    fixture = start(['/usr/bin/python3', repo / 'tests/e2e/sni-fixture.py', '--root', root, '--watcher'], 'fixture')
    wait(lambda: (root / 'ready').exists(), 'fixture did not start')
    bus = dbus.bus.BusConnection(env['DBUS_SESSION_BUS_ADDRESS'])
    control = dbus.Interface(bus.get_object('org.decebal.TestSNI', '/TestControl'), 'org.decebal.TestSNI')
    watcher_owner = str(bus.get_name_owner('org.kde.StatusNotifierWatcher'))
    def start_real_app():
        obs = shutil.which('obs')
        if not obs:
            print('SKIP real app: OBS is not installed'); raise SystemExit(77)
        obs_dir = Path(env['XDG_CONFIG_HOME']) / 'obs-studio'
        obs_dir.mkdir(parents=True, exist_ok=True)
        (obs_dir / 'global.ini').write_text('[General]\nFirstRun=true\n[BasicWindow]\nSysTrayEnabled=true\nSysTrayWhenStarted=true\n')
        real = start([obs, '--safe-mode', '--minimize-to-tray', '--disable-updater', '--disable-missing-files-check'],
            'obs-real', {'QT_QPA_PLATFORM':'xcb', 'QT_OPENGL':'software', 'LIBGL_ALWAYS_SOFTWARE':'1'})
        watcher = bus.get_object('org.kde.StatusNotifierWatcher', '/StatusNotifierWatcher')
        def real_item():
            registered = watcher.Get('org.kde.StatusNotifierWatcher', 'RegisteredStatusNotifierItems',
                dbus_interface='org.freedesktop.DBus.Properties')
            return next((str(item) for item in registered if not str(item).startswith('org.decebal.TestSNI.')), None)
        address = wait(real_item, 'OBS did not register an SNI item', seconds=30)
        service, path = address.split('/', 1)
        props = bus.get_object(service, '/' + path).GetAll('org.kde.StatusNotifierItem',
            dbus_interface='org.freedesktop.DBus.Properties')
        assert props.get('IconName') or props.get('IconPixmap'), 'real OBS item has no icon'
        (evidence / 'real-app.json').write_text(json.dumps({'app':'OBS', 'address':address,
            'title':str(props.get('Title')), 'status':str(props.get('Status')), 'menu':str(props.get('Menu'))}, indent=2))
        screenshot('real-obs')
        print('PASS real installed OBS protocol registration:', address, str(props.get('Title')))
        return real, address, props
    if args.probe_real_app:
        real, _, _ = start_real_app()
        stop(real)
        raise SystemExit(0)
    if args.tiny_rail:
        tiny, tiny_win, tiny_debug = surface('rail')
        wait(lambda: len(items(tiny_debug)) >= 1 or read_debug(tiny_debug).get('overflow'), '600px rail has no app access')
        debug = read_debug(tiny_debug)
        for entry in debug['items']:
            x, y, width, height = entry['rect']
            assert y >= 0 and y + height <= 600, 'tiny rail app target outside screen'
        if debug.get('overflow'):
            x, y, width, height = debug['overflow']
            assert y >= 0 and y + height <= 600, 'tiny rail overflow outside screen'
            click_rect(tiny_win, debug['overflow'])
            wait(lambda: all(any(row['label'] == title for row in menu_rows(tiny_debug))
                for title in ('Fixture red', 'Fixture cyan')), 'tiny rail overflow cannot access both apps')
            menu_click(tiny_debug, tiny_win, 'Fixture red')
        else:
            assert len(debug['items']) == 2, 'tiny rail silently lost an app'
            click_rect(tiny_win, find_item(tiny_debug, 'Fixture red')['rect'], 3)
        wait(lambda: any(row.get('id') == 1 for row in menu_rows(tiny_debug)), 'tiny rail app menu did not load')
        screenshot('tiny-rail-appmenu')
        stop(tiny)
        print('PASS 600px vertical rail: bounded targets and both apps accessible')
        raise SystemExit(0)
    for style in ('horizontal', 'rail', 'windows'):
        process, win, debug = surface(style)
        red = wait(lambda: find_item(debug, 'Fixture red'), style + ' missing first external item')
        cyan = wait(lambda: find_item(debug, 'Fixture cyan'), style + ' missing second external item')
        assert red['key'] != cyan['key'] and len(items(debug)) == 2
        assert_color(win, red['rect'], 'f03c5a', style + '-red')
        assert_color(win, cyan['rect'], '18c7e8', style + '-cyan')
        for button, method in ((1, 'Activate'), (2, 'SecondaryActivate'), (4, 'Scroll')):
            count = len(events(method, 1)); click_rect(win, red['rect'], button)
            wait(lambda: len(events(method, 1)) > count, style + ': ' + method + ' not delivered')
            event = events(method, 1)[-1]
            if button != 4:
                ox, oy = origin(win)
                x, y, width, height = red['rect']
                assert abs(event['x'] - int(ox+x+width/2)) <= 1 and abs(event['y'] - int(oy+y+height/2)) <= 1, 'activation did not use global coordinates'
            else:
                assert event['orientation'] == 'vertical' and abs(event['delta']) == 120
        click_rect(win, red['rect'], 3)
        wait(lambda: any(row['label'] == 'Disabled action' for row in menu_rows(debug)), style + ' loaded menu absent')
        rows = menu_rows(debug)
        disabled = next(r for r in rows if r['label'] == 'Disabled action')
        checked = next(r for r in rows if r['label'] == 'Checked option')
        assert not disabled['enabled'] and checked.get('checked')
        before = len(events('Event', 1)); menu_click(debug, win, 'Disabled action'); time.sleep(.15)
        assert len(events('Event', 1)) == before, 'disabled action was dispatched'
        before_nested = len([e for e in events('Event', 1) if e.get('id') == 6])
        menu_click(debug, win, 'More')
        menu_click(debug, win, 'Nested action')
        wait(lambda: len([e for e in events('Event', 1) if e.get('id') == 6]) > before_nested, 'nested menu event absent')
        click_rect(win, red['rect'], 3)
        before_check = len([e for e in events('Event', 1) if e.get('id') == 2])
        menu_click(debug, win, 'Checked option')
        wait(lambda: len([e for e in events('Event', 1) if e.get('id') == 2]) > before_check, 'check menu event absent')
        click_rect(win, red['rect'], 3)
        unchecked = wait(lambda: next((r for r in menu_rows(debug) if r['label'] == 'Checked option'), None), 'updated check menu absent')
        assert not unchecked.get('checked'), 'menu check state did not update'
        menu_click(debug, win, 'Checked option')  # restore fixture baseline for next surface
        # The popup can resize/reposition the native surface (especially taskbar).
        # Synthetic Open app must retain the original icon's screen anchor.
        time.sleep(.12)
        closed_red = find_item(debug, 'Fixture red')
        anchor_x, anchor_y = origin(win)
        rx, ry, rw, rh = closed_red['rect']
        expected_anchor = (int(anchor_x + rx + rw/2), int(anchor_y + ry + rh/2))
        before_open_app = len(events('Activate', 1))
        click_rect(win, closed_red['rect'], 3)
        menu_click(debug, win, 'Open app')
        wait(lambda: len(events('Activate', 1)) > before_open_app, 'synthetic Open app did not activate')
        activated = events('Activate', 1)[-1]
        assert abs(activated['x'] - expected_anchor[0]) <= 1 and abs(activated['y'] - expected_anchor[1]) <= 1, 'Open app lost original screen anchor after popup resize'
        # Real lazy DBusMenu providers advertise children-display before children exist.
        control.Update(1, 'menu', 'dynamic')
        menu_wire = dbus.Interface(bus.get_object('org.decebal.TestSNI.Item1', '/Menu'), 'com.canonical.dbusmenu')
        tree = menu_wire.GetLayout(0, -1, [])[1]
        assert not next(child for child in tree[2] if child[0] == 5)[2], 'dynamic fixture already populated'
        click_rect(win, red['rect'], 3)
        more = wait(lambda: next((row for row in menu_rows(debug) if row.get('id') == 5), None), 'dynamic submenu affordance absent')
        assert more['submenu'], 'empty advertised submenu lost its navigation affordance'
        before_dynamic = len([event for event in events('AboutToShow', 1) if event.get('id') == 5])
        menu_click(debug, win, 'More')
        wait(lambda: len([event for event in events('AboutToShow', 1) if event.get('id') == 5]) > before_dynamic,
            'dynamic submenu AboutToShow(5) was not dispatched')
        menu_click(debug, win, 'Nested action')
        control.Update(1, 'menu', '')
        count = len(events('AboutToShow', 2))
        click_rect(win, cyan['rect'], 1)
        wait(lambda: len(events('AboutToShow', 2)) > count, 'ItemIsMenu left click did not open DBusMenu')
        run('xdotool', 'key', 'Escape')
        time.sleep(.3)
        before_paint = len(events('GetAll'))
        ox, oy = origin(win)
        moves = []
        x, y, width, height = red['rect']
        for _ in range(20):
            moves += ['mousemove', str(int(ox+x+width/2)), str(int(oy+y+height/2)), 'mousemove', '640', '450']
        run('xdotool', *moves)
        time.sleep(.3)
        assert len(events('GetAll')) - before_paint <= 2, 'pointer paints made repeated synchronous property reads'
        control.Update(1, 'title', 'Updated fixture')
        updated = wait(lambda: find_item(debug, 'Updated fixture'), 'title signal not rendered')
        assert updated['key'] == red['key']
        control.Update(1, 'color', '4fe187')
        assert_color(win, updated['rect'], '4fe187', style + '-updated')
        control.Update(1, 'status', 'NeedsAttention')
        wait(lambda: find_item(debug, 'Updated fixture')['status'] == 'NeedsAttention', 'attention status absent')
        control.Update(1, 'status', 'Passive')
        wait(lambda: not find_item(debug, 'Updated fixture'), 'Passive item still visible')
        control.Update(1, 'status', 'Active')
        wait(lambda: find_item(debug, 'Updated fixture'), 'reactivated item absent')
        for malformed in ('oversize', 'truncated'):
            control.Update(1, 'bad', malformed)
            wait(lambda: find_item(debug, 'Updated fixture').get('icon_size') != [24, 24], 'malformed pixmap retained the previous valid image')
            size = find_item(debug, 'Updated fixture').get('icon_size')
            assert size is None or max(size) <= 512, 'oversized pixmap reached native rendering'
            assert process.poll() is None, 'malformed pixmap killed native surface'
        old_key = find_item(debug, 'Updated fixture')['key']
        old_owner = json.loads(str(control.Snapshot()))['1']['owner']
        control.Drop(1)
        wait(lambda: not find_item(debug, 'Updated fixture'), 'dead item remained visible')
        control.Spawn(1)
        red = wait(lambda: find_item(debug, 'Fixture red'), 'reowned item did not return')
        assert red['key'] != old_key
        assert json.loads(str(control.Snapshot()))['1']['owner'] != old_owner
        assert str(bus.get_name_owner('org.kde.StatusNotifierWatcher')) == watcher_owner, 'native host stole external watcher'
        screenshot(style + '-complete')
        stop(process)
        print('PASS native ' + style + ': pixels, input, lazy submenus, signals, malformed survival, death/reown')
    overflow_process, overflow_win, overflow_debug = surface('horizontal')
    for index in range(3, 19):
        control.Spawn(index)
        control.Update(index, 'title', 'Overflow ' + str(index))
    overflow_rect = wait(lambda: read_debug(overflow_debug).get('overflow'), 'many apps have no overflow affordance')
    click_rect(overflow_win, overflow_rect)
    first_page = wait(lambda: next((row for row in menu_rows(overflow_debug) if row['label'] == 'More…'), None), 'overflow first page absent')
    click_rect(overflow_win, first_page['rect'], 5)
    wait(lambda: any(row['label'] == 'Previous' for row in menu_rows(overflow_debug)), 'wheel did not page overflow forward')
    previous = next(row for row in menu_rows(overflow_debug) if row['label'] == 'Previous')
    click_rect(overflow_win, previous['rect'], 4)
    wait(lambda: not any(row['label'] == 'Previous' for row in menu_rows(overflow_debug)), 'wheel did not page overflow backward')
    for _ in range(3):
        rows = wait(lambda: menu_rows(overflow_debug), 'overflow list absent')
        if any(row['label'] == 'Overflow 18' for row in rows): break
        menu_click(overflow_debug, overflow_win, 'More…')
    menu_click(overflow_debug, overflow_win, 'Overflow 18')
    wait(lambda: any(row['label'] == 'Nested action' or row['label'] == 'More' for row in menu_rows(overflow_debug)),
        'overflow item cannot reach app menu')
    screenshot('horizontal-overflow-menu')
    run('xdotool', 'key', 'Escape')
    for index in range(3, 19): control.Drop(index)
    wait(lambda: len(items(overflow_debug)) == 2 and not read_debug(overflow_debug).get('overflow'), 'overflow did not clear')
    stop(overflow_process)
    print('PASS native overflow wheel/button pagination and hidden app access')
    if shutil.which('obs'):
        native, real_win, real_debug = surface('horizontal')
        wait(lambda: len(items(real_debug)) == 2, 'native surface missing before real app')
        real, real_address, real_props = start_real_app()
        actual = wait(lambda: next((item for item in items(real_debug) if item['title'] == str(real_props['Title'])), None),
            'real installed OBS icon absent in native Decebal')
        assert actual['rect'][2] > 0 and actual['rect'][3] > 0
        screenshot('real-obs-native')
        click_rect(real_win, actual['rect'], 3)
        wait(lambda: any(row.get('id') is not None for row in menu_rows(real_debug)), 'real installed OBS DBusMenu actions did not load natively')
        assert any(row.get('label') and row.get('id') is not None for row in menu_rows(real_debug)), 'OBS menu has no native remote actions'
        screenshot('real-obs-native-menu')
        submenu = next((row for row in menu_rows(real_debug) if row.get('submenu') and row.get('id') is not None), None)
        assert submenu, 'OBS projector submenu affordance absent'
        menu_click(real_debug, real_win, submenu['label'])
        wait(lambda: any(row['label'] == '‹ Back' for row in menu_rows(real_debug)), 'OBS submenu did not navigate')
        wait(lambda: any(row.get('id') is not None or row['label'] == 'No menu actions' for row in menu_rows(real_debug)),
            'OBS submenu never completed loading')
        screenshot('real-obs-native-submenu')
        run('xdotool', 'key', 'Escape')
        stop(real)
        wait(lambda: len(items(real_debug)) == 2, 'terminated real app icon remained')
        stop(native)
        print('PASS installed OBS native item/menu/lifecycle')
    else:
        print('SKIP real-app compatibility: no installed OBS candidate')
    # Two independent native hosts share the pre-existing watcher without stealing.
    first, win1, debug1 = surface('horizontal')
    wait(lambda: len(items(debug1)) == 2, 'first multi-host icons absent')
    config = Path(env['DECEBAL_CONFIG'])
    (config / 'decebal.json').write_text(json.dumps({'theme':'macos', 'bar':{'enabled':True}, 'dock':{'enabled':True}}))
    debug2 = root / 'multihost-indicators.json'
    second = start([binaries / 'decdock'], 'multihost', {'DECBAR_DEBUG_INDICATORS':str(debug2)})
    win2 = wait(lambda: window('decebal-dock'), 'second native host did not map')
    wait(lambda: len(items(debug2)) == 2 and len(items(debug1)) == 2, 'concurrent host icons absent')
    assert first.poll() is None and second.poll() is None
    assert str(bus.get_name_owner('org.kde.StatusNotifierWatcher')) == watcher_owner
    stop(second); stop(first)
    # No session bus is a valid desktop startup condition, never a native crash.
    env['DBUS_SESSION_BUS_ADDRESS'] = 'unix:path=' + str(root / 'missing-bus')
    missing, missing_win, missing_debug = surface('horizontal')
    wait(lambda: missing_debug.exists(), 'missing-bus surface never painted')
    time.sleep(.4)
    assert missing.poll() is None and not items(missing_debug)
    screenshot('missing-bus')
    stop(missing)
    print('PASS: private native SNI UI stages, concurrent hosts, missing-bus survival')
except Exception:
    failed = True
    print('Evidence/log directory:', root, file=sys.stderr)
    raise
finally:
    for process in reversed(processes): stop(process)
    for output in logs: output.close()
    for pattern in ('*.log', '*-indicators.json', 'events.jsonl'):
        for source in root.glob(pattern): shutil.copy2(source, evidence / source.name)
    if failed:
        print('Failed run retained at ' + str(root), file=sys.stderr)
    else:
        if evidence.is_relative_to(root):
            print('Temporary evidence removed; set SNI_UI_EVIDENCE_DIR to retain it')
        shutil.rmtree(root)
