#!/usr/bin/env python3
"""Private Nautilus selection acceptance; no live bus, files or pointer use."""
from pathlib import Path
import json, os, shutil, subprocess, sys, tempfile, time
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[2]


def run(*args, **kw):
    return subprocess.check_output(args, text=True, timeout=8, **kw).strip()


def private():
    home = Path.home()
    fixtures = home / 'Reveal fixtures'
    fixtures.mkdir()
    paths = []
    for name in ['space name.txt', 'café #100%.txt', "quote ' and $.txt", '-leading.txt']:
        p = fixtures / name
        p.write_text('private fixture')
        paths.append(p)
    target = fixtures / 'link.txt'
    target.symlink_to(paths[0].name)
    paths.append(target)
    folder = fixtures / 'Child folder'
    folder.mkdir()
    paths.append(folder)
    binary = os.environ.get('DECUTIL_BIN', str(ROOT / 'target/release/decutil'))
    wm = subprocess.Popen([os.environ['RDWM_BIN'], '--backend', 'x11', '--config', os.environ['PRIVATE_CONFIG']], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        time.sleep(.3)
        def selected_item(p):
            until = time.monotonic() + 6
            actual = ''
            while time.monotonic() < until:
                found = subprocess.run(['xdotool', 'search', '--onlyvisible', '--class', 'org.gnome.Nautilus'], capture_output=True, text=True)
                if found.returncode == 0:
                    wid = found.stdout.splitlines()[-1]
                    run('xdotool', 'windowfocus', wid)
                    run('xdotool', 'key', '--clearmodifiers', 'ctrl+c')
                    time.sleep(.1)
                    clip = subprocess.run(['xclip', '-selection', 'clipboard', '-o', '-t', 'text/uri-list'], capture_output=True, text=True, timeout=2)
                    actual = clip.stdout.strip()
                    if urlsplit(actual).scheme == 'file' and unquote(urlsplit(actual).path) == str(p):
                        return wid
                time.sleep(.1)
            raise AssertionError((p.name, actual))

        for p in paths:
            assert run(binary, 'reveal', '--', str(p)) == 'selected'
            wid = selected_item(p)

        # Exercise the actual search -> Ctrl+Enter -> detached utility route.
        # Only the private search producer is faked; both consumer binaries and
        # the file manager are real. The unique query has no app/command match,
        # so the result order is Packages, File, Web.
        spot_binary = Path(os.environ.get('DECSPOT_BIN', str(Path(binary).parent/'decspot'))).resolve()
        assert (spot_binary.parent/'decutil').resolve() == Path(binary).resolve(), 'Spotlight needs the matching sibling decutil'
        needle = fixtures/'revealneedle-native #%.txt'
        needle.write_text('actual Spotlight route fixture')
        tools = home/'search-tools'
        tools.mkdir()
        producer = tools/'plocate'
        marker = tools/'query-finished'
        producer.write_text('#!'+sys.executable+'\nfrom pathlib import Path\nimport sys\n'
                            'Path('+repr(str(marker))+').write_text(sys.argv[-1])\n'
                            'print('+repr(str(needle))+')\n')
        producer.chmod(0o700)
        spot_env = dict(os.environ, PATH=str(tools)+':'+os.environ['PATH'])
        spot_log = home/'spotlight.log'
        with spot_log.open('w') as log:
            spot = subprocess.Popen([str(spot_binary)], env=spot_env, stdout=log, stderr=log)
            try:
                deadline = time.monotonic()+5
                spot_wid = None
                while time.monotonic()<deadline:
                    result = subprocess.run(['xdotool','search','--onlyvisible','--name','^Spotlight$'], capture_output=True, text=True)
                    if result.returncode == 0:
                        spot_wid = result.stdout.splitlines()[-1]
                        break
                    time.sleep(.05)
                assert spot_wid, 'Spotlight did not open'
                run('xdotool','type','--clearmodifiers','--delay','20','revealneedle')
                deadline = time.monotonic()+4
                while time.monotonic()<deadline and (not marker.exists() or marker.read_text()!='revealneedle'):
                    time.sleep(.05)
                assert marker.exists() and marker.read_text()=='revealneedle', 'search producer was not queried'
                time.sleep(.3)  # Allow the completed worker snapshot to reach the UI.
                run('xdotool','key','--clearmodifiers','Down','ctrl+Return')
                spot.wait(timeout=4)
                assert spot.returncode == 0, spot_log.read_text()
                assert subprocess.run(['xdotool','search','--onlyvisible','--name','^Spotlight$'], capture_output=True).returncode != 0
                wid = selected_item(needle)
            finally:
                if spot.poll() is None:
                    spot.terminate()
                    spot.wait(timeout=3)
        out = Path(os.environ['REVEAL_NATIVE_EVIDENCE'])
        out.mkdir(parents=True, exist_ok=True)
        subprocess.run(['import', '-window', wid, str(out/'selection.png')], check=True)
        (out/'result.json').write_text(json.dumps({'selected_exact_items': len(paths), 'directory_and_symlink': True, 'clipboard_uri_verified': True, 'spotlight_ctrl_enter_exact_selection': True, 'spotlight_closed': True}, indent=2)+'\n')
        print('PASS native Nautilus exact selection for six special-name/file/directory/symlink cases and actual Spotlight Ctrl+Enter')
    finally:
        subprocess.run(['nautilus', '--quit'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        wm.terminate()
        wm.wait(timeout=3)


if __name__ == '__main__':
    if '--private' in sys.argv:
        private()
    else:
        for tool in ['Xvfb', 'dbus-run-session', 'xdotool', 'xclip', 'nautilus', 'import']:
            if not shutil.which(tool):
                sys.exit(77)
        with tempfile.TemporaryDirectory(prefix='file-reveal-native-') as temp:
            base = Path(temp)
            env = dict(os.environ)
            for key in ['WAYLAND_DISPLAY','RDWM_SOCK','DBUS_SESSION_BUS_ADDRESS','DBUS_STARTER_ADDRESS','DBUS_STARTER_BUS_TYPE','DECEBAL_OPEN','GTK_THEME','GTK_PATH','GTK_MODULES','XAUTHORITY']:
                env.pop(key, None)
            for key, name in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('XDG_STATE_HOME','state'),('XDG_DATA_HOME','data'),('XDG_RUNTIME_DIR','run')]:
                path = base/name
                path.mkdir(mode=0o700)
                env[key] = str(path)
            config = base/'config/rdwm.toml'
            config.write_text('[general]\nlayouts = ["floating"]\n[decoration]\nenabled = true\n')
            env.update(DECEBAL_SYSTEM_AUTOMATIC='0', GSETTINGS_BACKEND='memory', GTK_A11Y='none', NO_AT_BRIDGE='1', DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent', PRIVATE_CONFIG=str(config), RDWM_BIN=os.environ.get('RDWM_BIN', str(ROOT.parent/'rdwm/target/release/rdwm')), REVEAL_NATIVE_EVIDENCE=os.environ.get('REVEAL_NATIVE_EVIDENCE','/tmp/decebal-file-reveal-native'))
            x = subprocess.Popen(['Xvfb','-displayfd','1','-screen','0','1280x900x24','-nolisten','tcp'], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
            env['DISPLAY'] = ':'+x.stdout.readline().strip()
            try:
                subprocess.run(['dbus-run-session','--',sys.executable,str(Path(__file__).resolve()),'--private'], env=env, check=True, timeout=60)
            finally:
                x.terminate()
                x.wait(timeout=3)
