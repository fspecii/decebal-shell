#!/usr/bin/python3
"""Native notification persistence failures/retry; private resources only."""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import signal
import stat
import subprocess
import tempfile
import time
import dbus

argparse.ArgumentParser(description=__doc__).parse_args()
repo = Path(os.environ['NOTIFY_HISTORY_REPO'])
binary = Path(os.environ['MS_BIN_DIR']) / 'decnotify'
wm_binary = Path(os.environ['RDWM_BIN_DIR']) / 'rdwm'
for command in ('Xvfb', 'dbus-daemon', 'xdotool', 'xprop', 'import'):
    if not shutil.which(command):
        print('SKIP: missing ' + command); raise SystemExit(77)
for executable in (binary, wm_binary):
    if not os.access(executable, os.X_OK):
        print('SKIP: missing ' + str(executable)); raise SystemExit(77)
if os.getuid() == 0:
    print('SKIP: actual permission-denied regression requires non-root uid'); raise SystemExit(77)
match = re.fullmatch(r':([0-9]{1,4})', os.environ.get('E2E_DISPLAY', ':348'))
if not match or int(match[1]) < 2:
    raise SystemExit('Refusing live or invalid display')
number = int(match[1])
for path in (Path('/tmp/.X11-unix/X' + str(number)), Path('/tmp/.X' + str(number) + '-lock')):
    if path.exists(): print('SKIP: occupied display'); raise SystemExit(77)
root = Path(tempfile.mkdtemp(prefix='decebal-notify-history.'))
evidence = Path(os.environ.get('NOTIFY_HISTORY_EVIDENCE_DIR', root / 'evidence')).resolve()
evidence.mkdir(parents=True, exist_ok=True)
env = dict(os.environ)
for key in list(env):
    if key in ('WAYLAND_DISPLAY','RDWM_SOCK','HYPRLAND_INSTANCE_SIGNATURE','DBUS_SESSION_BUS_PID',
            'SESSION_MANAGER','AT_SPI_BUS_ADDRESS','XAUTHORITY','PULSE_SERVER','PIPEWIRE_REMOTE') or key.startswith(('DECBAR_DEBUG_', 'DECDOCK_DEBUG_', 'DECNOTIFY_DEBUG_')):
        env.pop(key, None)
for key, child in {'HOME':'home','XDG_CONFIG_HOME':'cfg','XDG_CONFIG_DIRS':'noconfig',
        'XDG_RUNTIME_DIR':'run','XDG_STATE_HOME':'state','XDG_DATA_HOME':'data',
        'XDG_DATA_DIRS':'nodata','XDG_CACHE_HOME':'cache','DECEBAL_CONFIG':'cfg/omarchy',
        'DECEBAL_THEMES':'themes','DECEBAL_SYSTEM_THEMES':'system-themes'}.items():
    env[key] = str(root / child); Path(env[key]).mkdir(parents=True, exist_ok=True)
os.chmod(env['XDG_RUNTIME_DIR'], 0o700)
env.update(DISPLAY=':' + str(number), XDG_SESSION_TYPE='x11', DECNOTIFY_FORCE='1',
    DECEBAL_BACKEND='x11', DECEBAL_SYSTEM_AUTOMATIC='0',
    DBUS_SESSION_BUS_ADDRESS='unix:path=' + str(root / 'bus'),
    DBUS_SYSTEM_BUS_ADDRESS='unix:path=' + str(root / 'no-system-bus'))
env['PATH'] = str(Path(os.environ['MS_BIN_DIR'])) + ':' + env['PATH']
config = root / 'cfg/rdwm.toml'
config.write_text('[general]\ngap_inner = 6\n[decoration]\nenabled = true\n[bar]\nshow = false\n')
notify_config = root / 'cfg/decebal'; notify_config.mkdir()
(notify_config / 'notifications.toml').write_text('dnd = false\nbanner_seconds = 60\n')
history = root / 'state/decebal/notification-history.json'
history.parent.mkdir(mode=0o700)
debug = root / 'notify-debug.json'
env['DECNOTIFY_DEBUG_HISTORY'] = str(debug)
processes = []
streams = []
secrets = []
failed = False
counter = 0
B = 'org.freedesktop.Notifications'
P = '/org/freedesktop/Notifications'
E = 'org.decebal.Notifications'

def start(args, label):
    output = (root / (label + '.log')).open('w'); streams.append(output)
    process = subprocess.Popen(list(map(str, args)), env=env, stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
    processes.append(process); return process

def stop(process):
    if process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
        try: process.wait(timeout=4)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL); process.wait(timeout=2)

def run(*args):
    return subprocess.check_output(list(map(str,args)), env=env, stderr=subprocess.DEVNULL, text=True).strip()

def wait(check, reason, seconds=10):
    end = time.monotonic() + seconds
    last = None
    while time.monotonic() < end:
        try:
            result = check()
            if result: return result
        except (OSError, ValueError, KeyError, dbus.DBusException, subprocess.CalledProcessError) as error:
            last = error
        time.sleep(.04)
    raise AssertionError(reason + (': ' + str(last) if last else ''))

def state(): return json.loads(debug.read_text())
def stored(): return json.loads(history.read_text())
def persisted(identifier): return any(item['id'] == identifier for item in stored())
def ids(): return [item['id'] for item in state().get('items', [])]
def banner_windows():
    try: return set(run('xdotool','search','--onlyvisible','--classname','^decnotify$').splitlines())
    except subprocess.CalledProcessError: return set()
def history_visible():
    win = state().get('window')
    if win:
        try:
            run('xdotool','getwindowgeometry','--shell',str(win))
            return win
        except subprocess.CalledProcessError:
            return None
    return None
def error_visible():
    value = state()
    return history_visible() and value.get('error') and value.get('retry_rect')

def screenshot(name):
    time.sleep(.1)
    destination = evidence / (name + '.png')
    run('import', '-window', 'root', destination)
    return destination

def click(rect):
    win = str(state()['window'])
    x,y,w,h = rect
    run('xdotool','mousemove','--window',win,int(x+w/2),int(y+h/2),'click','1')

def show_history():
    extension.ShowHistory()
    wait(history_visible, 'native history panel absent')

def close_history():
    if history_visible():
        extension.ToggleHistory()
        wait(lambda: not history_visible(), 'native history panel did not close')

def send(label, transient=False):
    title = 'NH_PRIVATE_TITLE_' + label
    body = 'NH_PRIVATE_BODY_' + label
    secrets.extend((title,body))
    hints = dbus.Dictionary({'transient':dbus.Boolean(transient)}, signature='sv')
    before_windows = banner_windows()
    identifier = int(notifications.Notify('Native history QA', dbus.UInt32(0), '', title, body,
        dbus.Array([], signature='s'), hints, dbus.Int32(60000)))
    assert identifier > 0
    wait(lambda: banner_windows() - before_windows, 'Notify did not create a native banner')
    return identifier

def launch(label):
    global notifications, extension
    debug.unlink(missing_ok=True)
    process = start([binary], 'daemon-' + label)
    wait(lambda: bus.name_has_owner(B), 'notification daemon did not own private bus')
    proxy = bus.get_object(B,P)
    notifications = dbus.Interface(proxy,B)
    extension = dbus.Interface(proxy,E)
    assert notifications.GetServerInformation()[0] == 'decnotify'
    return process

def shutdown(process):
    stop(process)
    wait(lambda: not bus.name_has_owner(B), 'notification name remained after daemon shutdown')

def assert_private_logs():
    for path in root.glob('daemon-*.log'):
        contents = path.read_text()
        assert 'NH_PRIVATE_' not in contents and not any(secret in contents for secret in secrets), 'notification contents leaked into diagnostic log ' + path.name

try:
    daemon_bus = start(['dbus-daemon','--session','--nofork','--address='+env['DBUS_SESSION_BUS_ADDRESS']], 'bus')
    wait(lambda: (root/'bus').is_socket(), 'private bus failed')
    xvfb = start(['Xvfb',env['DISPLAY'],'-screen','0','1280x900x24','-nolisten','tcp'], 'xvfb')
    wait(lambda: Path('/tmp/.X11-unix/X'+str(number)).is_socket() and xvfb.poll() is None, 'private Xvfb failed')
    wait(lambda: run('xdotool','getdisplaygeometry') == '1280 900', 'private X server not ready')
    wm = start([wm_binary,'--backend','x11','--config',config], 'rdwm')
    wait(lambda: 'window id' in run('xprop','-root','_NET_SUPPORTING_WM_CHECK'), 'private WM failed')
    bus = dbus.bus.BusConnection(env['DBUS_SESSION_BUS_ADDRESS'])
    daemon = launch('normal')
    first = send('normal')
    wait(lambda: persisted(first), 'normal notification did not persist')
    assert stat.S_IMODE(history.stat().st_mode) == 0o600
    before = history.read_bytes()
    transient = send('transient', True)
    assert history.read_bytes() == before and not persisted(transient), 'transient notification persisted'
    screenshot('normal-banners')
    show_history()
    assert not state().get('error')
    screenshot('normal-history')
    shutdown(daemon)
    daemon = launch('restart')
    show_history()
    assert first in ids() and transient not in ids(), 'restart did not restore exactly durable items'
    assert history.read_bytes() == before, 'read-only restart modified history'
    screenshot('restored-history')
    close_history()
    print('PASS native banners, transient exclusion and restart restoration', flush=True)
    # Force actual EACCES while preserving a readable valid durable file.
    history.parent.chmod(0o500)
    pending = send('permission')
    pending_transient = send('permission-transient', True)
    assert history.read_bytes() == before
    screenshot('permission-banner-usable')
    show_history(); wait(error_visible, 'write failure was not visibly recoverable')
    assert pending in ids() and pending_transient not in ids()
    screenshot('permission-history-error')
    click(state()['retry_rect'])
    wait(error_visible, 'failed Retry falsely cleared the warning')
    history.parent.chmod(0o700)
    click(state()['retry_rect'])
    wait(lambda: not state().get('error') and persisted(pending), 'Retry did not persist pending entries after permission repair')
    screenshot('permission-recovered')
    print('PASS actual write-denial warning and native Retry recovery', flush=True)
    # Failed clear may clear memory, but must preserve durable data and say it failed.
    before_clear = history.read_bytes()
    history.parent.chmod(0o500)
    remote_clear_failed = False
    try: extension.ClearHistory()
    except dbus.DBusException as error:
        assert error.get_dbus_name() == 'org.freedesktop.DBus.Error.Failed'
        assert 'NH_PRIVATE_' not in str(error), 'clear error response leaked notification content'
        remote_clear_failed = True
    wait(error_visible, 'failed clear has no visible durability warning')
    assert history.read_bytes() == before_clear, 'failed clear changed durable history'
    assert remote_clear_failed, 'ClearHistory returned success despite durable failure'
    screenshot('clear-failure')
    history.parent.chmod(0o700)
    click(state()['retry_rect'])
    wait(lambda: not state().get('error') and stored() == [], 'Retry did not durably apply pending clear')
    screenshot('clear-recovered')
    close_history()
    clear_button_item = send('native-clear')
    wait(lambda: persisted(clear_button_item), 'native-clear fixture did not persist')
    before_native_clear = history.read_bytes()
    show_history()
    history.parent.chmod(0o500)
    click(state()['clear_rect'])
    wait(error_visible, 'native Clear button failed without visible warning')
    assert history.read_bytes() == before_native_clear
    screenshot('native-clear-failure')
    history.parent.chmod(0o700)
    click(state()['retry_rect'])
    wait(lambda: not state().get('error') and stored() == [], 'native Clear Retry failed')
    shutdown(daemon)
    print('PASS D-Bus/native Clear failures preserve durable content until Retry', flush=True)
    # Untrusted existing sources are preserved until repaired explicitly.
    for mode in ('symlink','corrupt','unreadable'):
        history.unlink(missing_ok=True)
        target = root / ('preserved-' + mode + '.json')
        old_summary = 'NH_PRIVATE_OLD_' + mode
        secrets.append(old_summary)
        old_record = {'id':1,'app':'Old private QA','icon':'','summary':old_summary,'body':'NH_PRIVATE_OLD_BODY_' + mode,'entry':'','time':0}
        original = json.dumps([old_record]).encode() if mode == 'unreadable' else ('NH_PRIVATE_DISK_' + mode).encode()
        secrets.append('NH_PRIVATE_DISK_' + mode)
        if mode == 'symlink':
            target.write_bytes(original)
            history.symlink_to(target)
        else:
            history.write_bytes(original)
            if mode == 'unreadable': history.chmod(0o000)
        daemon = launch(mode)
        identifier = send(mode)
        screenshot(mode + '-banner-usable')
        show_history(); wait(error_visible, mode + ' source failure not visible')
        assert identifier in ids()
        if mode == 'unreadable':
            assert stat.S_IMODE(history.stat().st_mode) == 0
            history.chmod(0o600)  # explicit external repair, before Retry
        assert history.read_bytes() == original
        if mode == 'symlink': assert history.is_symlink() and target.read_bytes() == original
        screenshot(mode + '-preserved-error')
        if mode != 'unreadable':
            history.unlink()
            old_record['id'] = identifier  # collide with active unsaved ID intentionally
            history.write_text(json.dumps([old_record]))
            history.chmod(0o600)
        click(state()['retry_rect'])
        wait(lambda: not state().get('error') and persisted(identifier), mode + ' repair+Retry failed')
        records = stored()
        assert len(records) == 2 and len({item['id'] for item in records}) == 2, 'Retry lost repaired disk entry or duplicated IDs'
        assert any(item['summary'] == old_summary and item['id'] != identifier for item in records), 'Retry did not rekey conflicting old record'
        assert any(item['id'] == identifier and item['summary'] == 'NH_PRIVATE_TITLE_' + mode for item in records), 'Retry changed active pending ID'
        if mode == 'symlink': assert target.read_bytes() == original and not history.is_symlink()
        screenshot(mode + '-recovered')
        merged_ids = {item['id'] for item in records}
        close_history()
        followup = send(mode + '-after-retry')
        assert followup not in merged_ids, 'native Retry did not advance daemon ID above merged history'
        wait(lambda: persisted(followup) and len(stored()) == 3, 'post-Retry Notify overwrote a merged old entry')
        shutdown(daemon)
        daemon = launch(mode + '-restart')
        show_history()
        assert identifier in ids() and followup in ids() and len(ids()) == 3 and not state().get('error')
        shutdown(daemon)
        print('PASS ' + mode + ' preservation, repaired merge/rekey and restart', flush=True)
    assert_private_logs()
    print('PASS native Notify banners/history, restart durability, transient exclusion, actual EACCES warning/retry, failed-clear durability, symlink/corrupt/unreadable preservation and merging repair, content-free diagnostics')
except Exception:
    failed = True
    print('Failed private run retained at',root)
    raise
finally:
    history.parent.chmod(0o700)
    if history.exists() and not history.is_symlink(): history.chmod(0o600)
    for process in reversed(processes): stop(process)
    for stream in streams: stream.close()
    for path in root.glob('*.log'): shutil.copy2(path,evidence/path.name)
    if debug.exists(): shutil.copy2(debug,evidence/debug.name)
    if not failed:
        if evidence.is_relative_to(root): print('Temporary evidence removed; set NOTIFY_HISTORY_EVIDENCE_DIR to retain it')
        shutil.rmtree(root)
