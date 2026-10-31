#!/usr/bin/env python3
"""Private-X11 ownership and atomic-file update contract; never uses the live display."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'e2e'))
import private_x11
from Xlib import X, display

ap = argparse.ArgumentParser()
ap.add_argument('binary', type=Path)
ap.add_argument('out', type=Path)
args = ap.parse_args()
binary = args.binary.resolve(strict=True)
args.out.mkdir(mode=0o700, parents=True, exist_ok=False)
base = args.out.resolve()
env = {'PATH': os.defpath, 'HOME': str(base), 'DECEBAL_SYSTEM_AUTOMATIC': '0',
       'DBUS_SESSION_BUS_ADDRESS': 'unix:path=' + str(base / 'no-bus'),
       'DBUS_SYSTEM_BUS_ADDRESS': 'unix:path=' + str(base / 'no-system-bus')}
for key in ('CONFIG', 'DATA', 'CACHE', 'STATE', 'RUNTIME'):
    p = base / key
    p.mkdir(mode=0o700)
    env['XDG_' + key + '_HOME' if key != 'RUNTIME' else 'XDG_RUNTIME_DIR'] = str(p)
ini = base / 'CONFIG' / 'gtk-3.0' / 'settings.ini'
ini.parent.mkdir()
ini.write_text('[Settings]\ngtk-theme-name=Fixture-A\n')
children = []
conn = None
server = None
checks = []


def wait(predicate, label):
    end = time.monotonic() + 5
    while time.monotonic() < end:
        if predicate():
            checks.append(label)
            return
        time.sleep(.02)
    raise AssertionError(label)


def prove():
    assert private_x11.listeners_owned(server, int(disp[1:])), 'private server ownership lost'


def command(*argv):
    prove()
    return subprocess.run([str(binary), 'system', 'xsettings', *argv, '--display', disp],
                          env=env, capture_output=True, text=True, timeout=8)


def start():
    prove()
    log = (base / ('manager-%d.log' % len(children))).open('w')
    p = subprocess.Popen([str(binary), 'system', 'xsettings', 'serve', '--display', disp, '--ini', str(ini)],
                         env=env, stdout=log, stderr=subprocess.STDOUT)
    children.append((p, log))
    return p


result = {'binary': str(binary), 'sha256': hashlib.sha256(binary.read_bytes()).hexdigest(), 'checks': checks}
try:
    with (base / 'xvfb.log').open('w') as log:
        server, disp = private_x11.start_private_xvfb(env=env, log=log)
    prove()
    conn = display.Display(disp)
    root = conn.screen().root
    selection = conn.intern_atom('_XSETTINGS_S0')
    settings = conn.intern_atom('_XSETTINGS_SETTINGS')
    foreign = root.create_window(-1, -1, 1, 1, 0, 0, X.InputOnly)
    foreign.set_selection_owner(selection, X.CurrentTime)
    conn.sync()
    owner = lambda: conn.get_selection_owner(selection).id
    blocked = start()
    assert blocked.wait(timeout=5) != 0
    assert owner() == foreign.id
    checks.append('foreign owner preserved on serve')
    assert command('stop').returncode == 0 and owner() == foreign.id
    checks.append('stop leaves foreign owner untouched')
    # Destroying this fixture-owned window releases its selection without replacing another client.
    foreign.destroy()
    conn.sync()
    first, second = start(), start()
    wait(lambda: bool(conn.get_selection_owner(selection)), 'one concurrent manager claims the selection')
    winner = owner()
    marker = conn.intern_atom('_DECEBAL_XSETTINGS_OWNER')
    assert conn.create_resource_object('window', winner).get_full_property(marker, X.AnyPropertyType)
    wait(lambda: sum(p.poll() is None for p in (first, second)) == 1, 'concurrent managers leave exactly one server')
    assert owner() == winner
    checks.append('concurrent start does not replace the winner')
    win = conn.create_resource_object('window', winner)
    wait(lambda: b'Fixture-A' in bytes(win.get_full_property(settings, X.AnyPropertyType).value), 'initial settings published')
    replacement = ini.with_suffix('.new')
    replacement.write_text('[Settings]\ngtk-theme-name=Fixture-B\n')
    replacement.replace(ini)
    wait(lambda: b'Fixture-B' in bytes(win.get_full_property(settings, X.AnyPropertyType).value), 'atomic replacement published')
    takeover = root.create_window(-1, -1, 1, 1, 0, 0, X.InputOnly)
    takeover.set_selection_owner(selection, X.CurrentTime)
    conn.sync()
    wait(lambda: first.poll() is not None and second.poll() is not None, 'selection loss ends serving manager')
    assert owner() == takeover.id
    assert command('stop').returncode == 0 and owner() == takeover.id
    checks.append('replacement foreign owner survives stop')
    takeover.destroy()
    conn.sync()
    final = start()
    wait(lambda: bool(conn.get_selection_owner(selection)), 'manager can start after foreign owner leaves')
    stopped = command('stop')
    assert stopped.returncode == 0 and 'stopped' in stopped.stdout
    assert final.wait(timeout=5) == 0
    assert not conn.get_selection_owner(selection)
    checks.append('typed stop releases owned selection and exits')
    result['passed'] = True
except BaseException as exc:
    result.update(passed=False, error=repr(exc))
    raise
finally:
    for child, log in children:
        if child.poll() is None:
            child.terminate()
            try:
                child.wait(timeout=3)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait(timeout=3)
        log.close()
    if conn is not None:
        conn.close()
    if server is not None:
        private_x11.close(server)
    (base / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result))
