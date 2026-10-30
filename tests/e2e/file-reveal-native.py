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
        for p in paths:
            assert run(binary, 'reveal', '--', str(p)) == 'selected'
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
                        break
                time.sleep(.1)
            assert urlsplit(actual).scheme == 'file' and unquote(urlsplit(actual).path) == str(p), (p.name, actual)
        out = Path(os.environ['REVEAL_NATIVE_EVIDENCE'])
        out.mkdir(parents=True, exist_ok=True)
        subprocess.run(['import', '-window', wid, str(out/'selection.png')], check=True)
        (out/'result.json').write_text(json.dumps({'selected_exact_items': len(paths), 'directory_and_symlink': True, 'clipboard_uri_verified': True}, indent=2)+'\n')
        print('PASS native Nautilus exact selection for six special-name/file/directory/symlink cases')
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
