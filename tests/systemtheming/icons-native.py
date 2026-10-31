#!/usr/bin/env python3
"""Manual icon publication contract on an owned X server; never applies a theme.

Requires system Python gi (GTK3 + GTK4), python-xlib, Pillow and Xvfb.
All native operations are intentionally opt-in by executing this file.
"""
import argparse
import configparser
import hashlib
import json
import os
from pathlib import Path
import signal
import shutil
import subprocess
import sys
import time
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'e2e'))
import private_x11
from PIL import Image
from Xlib import X, display
from Xlib.ext import xtest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bin-dir', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    args.out.mkdir(parents=True, mode=0o700, exist_ok=False)
    base = args.out.resolve()
    bins = args.bin_dir.resolve(strict=True)
    binaries = {name: (bins / name).resolve(strict=True) for name in ('decthemectl', 'decsettings')}
    token = uuid.uuid4().hex

    def interrupted(signum, frame):
        raise SystemExit(128 + signum)

    # Override the helper's default handler: all signals must use our token/pidfd cleanup.
    for signum in (signal.SIGTERM, signal.SIGINT):
        signal.signal(signum, interrupted)
    env = dict(PATH=str(bins) + os.pathsep + os.defpath, LANG='C.UTF-8',
               ICONS_QA_TOKEN=token, GDK_BACKEND='x11', GSK_RENDERER='cairo',
               GSETTINGS_BACKEND='memory', GTK_A11Y='none', DECEBAL_BACKEND='x11',
               XDG_SESSION_TYPE='x11', RDWMCTL='/bin/false',
               DBUS_SESSION_BUS_ADDRESS='unix:path=' + str(base / 'no-session-bus'),
               DBUS_SYSTEM_BUS_ADDRESS='unix:path=' + str(base / 'no-system-bus'))
    if os.environ.get('E2E_PARENT_TOKEN'):
        env['E2E_PARENT_TOKEN'] = os.environ['E2E_PARENT_TOKEN']
    for key, name in [('HOME', 'home'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_DATA_HOME', 'data'), ('XDG_CACHE_HOME', 'cache'),
                      ('XDG_STATE_HOME', 'state'), ('XDG_RUNTIME_DIR', 'run'),
                      ('XDG_DATA_DIRS', 'empty-data'), ('XDG_CONFIG_DIRS', 'empty-config')]:
        p = base / name
        p.mkdir(mode=0o700)
        env[key] = str(p)
    # Keep icon/theme discovery private while allowing GdkPixbuf to identify PNG.
    # An empty XDG_DATA_DIRS also hides the shared MIME database from GTK.
    shutil.copytree('/usr/share/mime', base / 'empty-data/mime')
    # Recent GdkPixbuf uses Glycin loader descriptors from XDG_DATA_DIRS too.
    if Path('/usr/share/glycin-loaders').is_dir():
        shutil.copytree('/usr/share/glycin-loaders', base / 'empty-data/glycin-loaders')
    config_dir = base / 'config/omarchy'
    config_dir.mkdir()
    for name in ('themes', 'catalogs'):
        (base / name).mkdir()
    env.update(DECEBAL_CONFIG=str(config_dir), DECEBAL_SYSTEM_THEMES=str(base / 'themes'),
               DECEBAL_ASSET_CATALOGS=str(base / 'catalogs'))
    config = config_dir / 'decebal.json'
    system = {'enabled': True, 'signals': False,
              'targets': {name: name == 'gtk' for name in
                          ('gtk', 'qt', 'omarchy', 'browser', 'editors', 'hooks', 'kvantum', 'x11', 'terminals')},
              'options': {'gtk.xsettings': True}}

    def save_config():
        value = json.loads(config.read_text()) if config.exists() else {}
        value['system'] = system
        value.setdefault('asset_packs', {})['downloads'] = 'ask'
        config.write_text(json.dumps(value))

    save_config()
    colors = {'QA-Red': (231, 37, 43), 'QA-Blue': (29, 83, 229)}
    for name, rgb in colors.items():
        pack = base / 'data/icons' / name
        (pack / '48x48/apps').mkdir(parents=True)
        (pack / 'index.theme').write_text(
            f'[Icon Theme]\nName={name}\nDirectories=48x48/apps\n\n'
            '[48x48/apps]\nSize=48\nType=Fixed\nContext=Applications\n')
        Image.new('RGB', (48, 48), rgb).save(pack / '48x48/apps/decebal-probe.png')
    css = b'/* independent GTK stylesheet sentinel */\n'
    inis = []
    for kit in ('3', '4'):
        folder = base / f'config/gtk-{kit}.0'
        folder.mkdir()
        (folder / 'gtk.css').write_bytes(css)
        ini = folder / 'settings.ini'
        ini.write_text('[Settings]\ngtk-icon-theme-name=QA-Red\ngtk-theme-name=Adwaita\ngtk-font-name=Sans 11\n')
        inis.append(ini)
    children, logs, checks = [], [], []
    server = conn = foreign = None
    phase = 'fixture setup'
    result = {'passed': False, 'checks': checks,
              'binaries': {n: {'path': str(p), 'sha256': hashlib.sha256(p.read_bytes()).hexdigest()}
                           for n, p in binaries.items()},
              'source_scope': 'manual icon assets only; no full theme apply',
              'fixture_sha256': {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in
                                 (Path(__file__), Path(__file__).with_name('gtk_icon_probe.py'))},
              'settings_ui': 'pending real Icons-pane selection'}

    revision = subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=Path(__file__).resolve().parents[2],
                              env={'PATH': os.defpath}, capture_output=True, text=True, timeout=5)
    result['source_revision'] = revision.stdout.strip() if revision.returncode == 0 else None

    def marked():
        found = []
        needle = ('ICONS_QA_TOKEN=' + token).encode()
        for p in Path('/proc').iterdir():
            try:
                if p.name.isdigit() and p.stat().st_uid == os.getuid() and needle in (p / 'environ').read_bytes().split(b'\0'):
                    found.append(int(p.name))
            except OSError:
                pass
        return found

    def send_owned(pid, sig):
        fd = os.pidfd_open(pid)
        try:
            p = Path('/proc') / str(pid)
            assert p.stat().st_uid == os.getuid()
            assert ('ICONS_QA_TOKEN=' + token).encode() in (p / 'environ').read_bytes().split(b'\0')
            signal.pidfd_send_signal(fd, sig)
        finally:
            os.close(fd)

    def prove():
        assert private_x11.listeners_owned(server, int(disp[1:])), 'private X server ownership lost'

    def xcall(fn):
        prove()
        return fn()

    def run(*argv):
        prove()
        completed = subprocess.run(list(map(str, argv)), env=env, capture_output=True, text=True, timeout=12)
        number = len(list(base.glob('command-*.json')))
        (base / f'command-{number}.json').write_text(json.dumps({
            'argv': list(map(str, argv)), 'returncode': completed.returncode,
            'stdout': completed.stdout, 'stderr': completed.stderr}, indent=2))
        assert completed.returncode == 0, completed.stderr
        return completed.stdout

    def wait(predicate, label, timeout=12):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            if predicate():
                checks.append(label)
                return
            time.sleep(.1)
        raise AssertionError(label)

    def owner():
        return xcall(lambda: getattr(conn.get_selection_owner(selection), 'id', 0))

    def metadata(kit):
        try:
            return json.loads((base / (kit + '.json')).read_text())
        except (OSError, ValueError):
            return {}

    def pixels(kit, color, label):
        data = metadata(kit)
        if not data.get('xid'):
            return False
        win = xcall(lambda: conn.create_resource_object('window', data['xid']))
        geometry = xcall(win.get_geometry)
        raw = xcall(lambda: win.get_image(0, 0, geometry.width, geometry.height, X.ZPixmap, 0xffffffff))
        # Xvfb is explicitly 24-bit TrueColor, 32 bits per pixel, little endian.
        assert conn.display.info.image_byte_order == X.LSBFirst
        image = Image.frombytes('RGB', (geometry.width, geometry.height), raw.data, 'raw', 'BGRX')
        image.save(base / f'{kit}-{label}.png')
        return sum(count for count, rgb in image.getcolors(image.width * image.height) if rgb == color) > 100

    def verify_files(name):
        for ini in inis:
            parsed = configparser.ConfigParser()
            parsed.read(ini)
            assert dict(parsed['Settings']) == {'gtk-icon-theme-name': name,
                                                'gtk-theme-name': 'Adwaita', 'gtk-font-name': 'Sans 11'}
            assert ini.with_name('gtk.css').read_bytes() == css

    try:
        log = (base / 'xvfb.log').open('w')
        logs.append(log)
        server, disp = private_x11.start_private_xvfb(env=env, log=log, screen='1400x1100x24')
        env['DISPLAY'] = disp
        conn = xcall(lambda: display.Display(disp))
        selection = xcall(lambda: conn.intern_atom('_XSETTINGS_S0'))
        assert not owner(), 'unexpected XSETTINGS owner before probes'
        for kit in ('gtk3', 'gtk4'):
            prove()
            log = (base / (kit + '.log')).open('w')
            logs.append(log)
            child = subprocess.Popen(['/usr/bin/python3', str(Path(__file__).with_name('gtk_icon_probe.py')),
                                      kit, str(base / (kit + '.json'))], env=env, stdout=log, stderr=log)
            children.append(child)
            wait(lambda: metadata(kit).get('gtk-icon-theme-name') == 'QA-Red', kit + ' starts with independent red preference')
            window = xcall(lambda: conn.create_resource_object('window', metadata(kit)['xid']))
            xcall(lambda: window.configure(x=40 if kit == 'gtk3' else 400, y=60))
            xcall(conn.sync)
            wait(lambda: pixels(kit, colors['QA-Red'], 'initial'), kit + ' initially paints red')
        identities = {kit: (metadata(kit)['pid'], metadata(kit)['xid']) for kit in ('gtk3', 'gtk4')}
        assert not owner(), 'probe unexpectedly started settings manager'
        phase = 'manual icon publication'
        run(binaries['decthemectl'], 'icons', 'set', 'QA-Blue')
        wait(owner, 'first manual icons set starts XSETTINGS without prior theme apply')
        for kit in identities:
            wait(lambda: metadata(kit).get('gtk-icon-theme-name') == 'QA-Blue', kit + ' live metadata becomes blue')
            wait(lambda: pixels(kit, colors['QA-Blue'], 'manual-blue'), kit + ' same open image paints blue')
            data = metadata(kit)
            assert (data['pid'], data['xid']) == identities[kit]
            assert data['gtk-theme-name'] == 'Adwaita' and data['gtk-font-name'] == 'Sans 11'
        verify_files('QA-Blue')
        completion = base / 'state/decebal/system/last-result.json'
        wait(lambda: completion.exists() and json.loads(completion.read_text()).get('intent') == 'assets', 'asset completion recorded')
        completed = json.loads(completion.read_text())
        assert 'gtk' in completed['applied']
        (base / 'manual-completion.json').write_text(json.dumps(completed, indent=2))
        phase = 'Settings Icons-pane publication'
        layout = base / 'settings-layout.json'
        wm_config = base / 'rdwm.toml'
        wm_config.write_text('[general]\nborder_px = 2\ngap_inner = 6\n[bar]\nshow = false\n')
        settings_env = dict(env, DECSETTINGS_DEBUG_LAYOUT=str(layout), DECSETTINGS_SCALE='1')
        log = (base / 'settings.log').open('w')
        logs.append(log)
        prove()
        settings = subprocess.Popen([str(binaries['decsettings']), '--config', str(wm_config), 'icons'],
                                    env=settings_env, stdout=log, stderr=log)
        children.append(settings)
        pid_atom = xcall(lambda: conn.intern_atom('_NET_WM_PID'))

        def settings_window():
            assert settings.poll() is None, 'Settings exited before window discovery'
            matches = []
            for candidate in xcall(lambda: conn.screen().root.query_tree().children):
                if xcall(candidate.get_wm_class) != ('decsettings', 'DecSettings'):
                    continue
                prop = xcall(lambda: candidate.get_full_property(pid_atom, X.AnyPropertyType))
                # The current native backend omits _NET_WM_PID. This server was
                # created here and only this fixture's token-marked children use it.
                if prop is not None and len(prop.value):
                    assert int(prop.value[0]) == settings.pid, 'foreign Settings PID'
                matches.append(candidate)
            assert len(matches) <= 1, 'ambiguous Settings windows on private server'
            return matches[0] if matches else None

        wait(settings_window, 'Settings starts an owned Icons-pane window')
        settings_win = settings_window()
        xcall(lambda: settings_win.configure(x=150, y=100, width=1000, height=900, stack_mode=X.Above))
        xcall(conn.sync)

        def document():
            try:
                return json.loads(layout.read_text())
            except (OSError, ValueError):
                return {}

        wait(lambda: document().get('pane') == 'icons', 'Settings opens dedicated Icons pane')

        def pointer(x, y, button=1):
            assert settings.poll() is None, 'Settings exited during interaction'
            xcall(lambda: settings_win.configure(stack_mode=X.Above))
            translated = xcall(lambda: conn.screen().root.translate_coords(settings_win, int(x), int(y)))
            xcall(lambda: xtest.fake_input(conn, X.MotionNotify, x=translated.x, y=translated.y))
            xcall(lambda: xtest.fake_input(conn, X.ButtonPress, button))
            xcall(lambda: xtest.fake_input(conn, X.ButtonRelease, button))
            xcall(conn.sync)
            time.sleep(.15)

        def click_control(identifier):
            for _ in range(35):
                doc = document()
                candidates = [c for collection in ('ctls', 'layouts') for c in doc.get(collection, [])
                              if c['id'] == identifier]
                assert candidates, 'missing Settings control ' + identifier
                control = candidates[0]
                x, y, width, height = control['rect']
                if y >= 95 and y + height < 890:
                    assert control.get('enabled', True), 'disabled Settings control ' + identifier
                    pointer(x + width / 2, y + height / 2)
                    return
                pointer(800, 500, 5 if y + height >= 890 else 4)
            raise AssertionError('Settings control cannot be revealed: ' + identifier)

        for name in ('QA-Red', 'QA-Blue'):
            previous = config.read_bytes()
            click_control('l:icons:pick:' + name)
            wait(lambda: any(c['id'] == 'ic:use' and c.get('enabled', True)
                             for c in document().get('ctls', [])), 'Settings enables Use after selecting ' + name)
            assert config.read_bytes() == previous, 'highlight alone must not apply an icon theme'
            completion.unlink(missing_ok=True)
            click_control('ic:use')
            wait(lambda: json.loads(config.read_text()).get('icons', {}).get('theme') == name,
                 'Settings saves ' + name + ' through the real Use button')
            wait(lambda: completion.exists() and json.loads(completion.read_text()).get('intent') == 'assets',
                 'Settings records completed asset publication for ' + name)
            assert 'gtk' in json.loads(completion.read_text())['applied']
            # No WM runs here: put Settings below the probe windows before capturing their visible pixels.
            xcall(lambda: settings_win.configure(stack_mode=X.Below))
            xcall(conn.sync)
            for kit in identities:
                wait(lambda: metadata(kit).get('gtk-icon-theme-name') == name,
                     kit + ' follows Settings selection ' + name)
                wait(lambda: pixels(kit, colors[name], 'settings-' + name),
                     kit + ' same open image repaints after Settings selection ' + name)
                data = metadata(kit)
                assert (data['pid'], data['xid']) == identities[kit]
                assert data['gtk-theme-name'] == 'Adwaita' and data['gtk-font-name'] == 'Sans 11'
            verify_files(name)
            (base / ('settings-' + name + '-layout.json')).write_text(json.dumps(document(), indent=2))
            (base / ('settings-' + name + '-completion.json')).write_bytes(completion.read_bytes())
        result['settings_ui'] = 'real Icons row and Use clicks publish both local packs to the same open GTK3/GTK4 consumers'
        phase = 'ownership and opt-outs'
        system['options']['gtk.xsettings'] = False
        save_config()
        run(binaries['decthemectl'], 'icons', 'set', 'QA-Red')
        wait(lambda: not owner(), 'gtk.xsettings off stops owned manager')
        wait(lambda: all('QA-Red' in ini.read_text() for ini in inis), 'xsettings-off file publication completes')
        verify_files('QA-Red')
        system['options']['gtk.xsettings'] = True
        save_config()
        foreign = xcall(lambda: conn.screen().root.create_window(-1, -1, 1, 1, 0, 0, X.InputOnly))
        xcall(lambda: foreign.set_selection_owner(selection, X.CurrentTime))
        xcall(conn.sync)
        run(binaries['decthemectl'], 'icons', 'set', 'QA-Blue')
        wait(lambda: all('QA-Blue' in ini.read_text() for ini in inis), 'foreign-owner selection publication completes')
        assert owner() == foreign.id
        checks.append('foreign fixture selection owner preserved')
        xcall(foreign.destroy)
        foreign = None
        xcall(conn.sync)
        for option in ('master', 'gtk'):
            system['enabled'] = option != 'master'
            system['targets']['gtk'] = option != 'gtk'
            save_config()
            before = {str(p): p.read_bytes() for p in (base / 'config').glob('gtk-*/*')}
            output = run(binaries['decthemectl'], 'icons', 'set', 'QA-Red')
            assert 'not published' in output
            assert not owner()
            assert before == {str(p): p.read_bytes() for p in (base / 'config').glob('gtk-*/*')}
            checks.append(option + ' opt-out produces no toolkit writes or manager')
        result['passed'] = True
    except BaseException as exc:
        result.update(error=repr(exc), failure_phase=phase)
    finally:
        def cleanup():
            if server is not None:
                try:
                    run(binaries['decthemectl'], 'system', 'xsettings', 'stop', '--display', disp)
                except BaseException as exc:
                    result.update(passed=False, stop_error=repr(exc))
            if foreign is not None:
                try:
                    xcall(foreign.destroy)
                    xcall(conn.sync)
                except BaseException as exc:
                    result.update(passed=False, foreign_cleanup_error=repr(exc))
            # GTK's sandboxed image loaders legitimately remain alive while
            # their probe owns images. Close direct clients before testing leaks.
            for child in children:
                try:
                    send_owned(child.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
            for child in children:
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    send_owned(child.pid, signal.SIGKILL)
                    child.wait(timeout=3)
                    result.update(passed=False, forced_cleanup=True)
            expected = {server.pid} if server is not None else set()
            end = time.monotonic() + 3
            while set(marked()) - expected and time.monotonic() < end:
                time.sleep(.05)
            result['detached_after_clients_closed'] = sorted(set(marked()) - expected)
            if result['detached_after_clients_closed']:
                result['passed'] = False
                for pid in result['detached_after_clients_closed']:
                    try:
                        send_owned(pid, signal.SIGTERM)
                    except ProcessLookupError:
                        pass
            if conn is not None:
                conn.close()
            if server is not None:
                # Remove helper atexit ownership before explicit verified cleanup.
                private_x11._owned.discard(server)
                try:
                    send_owned(server.pid, signal.SIGTERM)
                    server.wait(timeout=5)
                except ProcessLookupError:
                    server.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    send_owned(server.pid, signal.SIGKILL)
                    server.wait(timeout=3)
                    result.update(passed=False, forced_cleanup=True)
            end = time.monotonic() + 3
            while marked() and time.monotonic() < end:
                time.sleep(.05)
            result['leftover_pids'] = marked()
            if result['leftover_pids']:
                result['passed'] = False
        try:
            cleanup()
        except BaseException as exc:
            result.update(passed=False, cleanup_error=repr(exc))
        for log in logs:
            try:
                log.close()
            except OSError as exc:
                result.update(passed=False, log_close_error=repr(exc))
        (base / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
        print(json.dumps(result))
    return 0 if result['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
