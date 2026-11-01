#!/usr/bin/python3
"""Native notification persistence failures/retry; private owned resources only.

Needs explicit, already built binaries (nothing is built, installed or defaulted):
  MS_BIN_DIR    absolute directory with decnotify (and the decebal helpers it finds beside itself)
  RDWM_BIN_DIR  absolute directory with rdwm
Optional:
  E2E_DISPLAY                   a private display :700..:65535 that must be unoccupied (default: allocated)
  NOTIFY_HISTORY_EVIDENCE_DIR   a NEW directory for screenshots/logs/cleanup.json (default: inside the private tree)
Exit: 0 pass, 77 missing prerequisite or occupied explicit display, 2 invalid setting, 128+N after signal N.
Private Xvfb, session bus and every child are owned by tests/e2e/notify_private.py (see docs/notification-history.md).
"""
import argparse
import json
from pathlib import Path
import stat
import sys
import time
from types import SimpleNamespace

import dbus

sys.path.insert(0, str(Path(__file__).resolve().parent))
import notify_private as private

B = 'org.freedesktop.Notifications'
P = '/org/freedesktop/Notifications'
E = 'org.decebal.Notifications'
CATCH = private.RETRYABLE + (dbus.DBusException,)
RPC_TIMEOUT = 5  # a hung daemon must not delay SIGTERM cleanup


def wait(check, reason, seconds=10):
    return private.wait(check, reason, seconds, CATCH)


def scenario(s):
    root, env, evidence = s.root, s.env, s.evidence
    config = root / 'cfg/rdwm.toml'
    config.write_text('[general]\ngap_inner = 6\n[decoration]\nenabled = true\n[bar]\nshow = false\n')
    notify_config = root / 'cfg/decebal'
    notify_config.mkdir(mode=0o700)
    (notify_config / 'notifications.toml').write_text('dnd = false\nbanner_seconds = 60\n')
    history = root / 'state/decebal/notification-history.json'
    history.parent.mkdir(mode=0o700)
    debug = root / 'notify-debug.json'
    env['DECNOTIFY_DEBUG_HISTORY'] = str(debug)
    s.extra_evidence.append(debug)
    secrets = []
    live = SimpleNamespace(bus=None, notifications=None, extension=None)

    def repair():
        history.parent.chmod(0o700)
        if history.exists() and not history.is_symlink():
            history.chmod(0o600)
        if live.bus is not None:
            live.bus.close()
    s.before_cleanup(repair)

    def state(): return json.loads(debug.read_text())
    def stored(): return json.loads(history.read_text())
    def persisted(identifier): return any(item['id'] == identifier for item in stored())
    def ids(): return [item['id'] for item in state().get('items', [])]
    def banner_windows():
        try: return set(s.run('xdotool', 'search', '--onlyvisible', '--classname', '^decnotify$').splitlines())
        except private.subprocess.CalledProcessError: return set()
    def history_visible():
        win = state().get('window')
        if win:
            try:
                s.run('xdotool', 'getwindowgeometry', '--shell', str(win))
                return win
            except private.subprocess.CalledProcessError:
                return None
        return None
    def error_visible():
        value = state()
        return history_visible() and value.get('error') and value.get('retry_rect')

    def screenshot(name):
        time.sleep(.1)
        destination = evidence / (name + '.png')
        s.run('import', '-window', 'root', destination)
        return destination

    def click(rect):
        win = str(state()['window'])
        x, y, w, h = rect
        s.run('xdotool', 'mousemove', '--window', win, int(x + w / 2), int(y + h / 2), 'click', '1')

    def show_history():
        live.extension.ShowHistory(timeout=RPC_TIMEOUT)
        wait(history_visible, 'native history panel absent')

    def close_history():
        if history_visible():
            live.extension.ToggleHistory(timeout=RPC_TIMEOUT)
            wait(lambda: not history_visible(), 'native history panel did not close')

    def send(label, transient=False):
        title = 'NH_PRIVATE_TITLE_' + label
        body = 'NH_PRIVATE_BODY_' + label
        secrets.extend((title, body))
        hints = dbus.Dictionary({'transient': dbus.Boolean(transient)}, signature='sv')
        before_windows = banner_windows()
        identifier = int(live.notifications.Notify('Native history QA', dbus.UInt32(0), '', title, body,
            dbus.Array([], signature='s'), hints, dbus.Int32(60000), timeout=RPC_TIMEOUT))
        assert identifier > 0
        wait(lambda: banner_windows() - before_windows, 'Notify did not create a native banner')
        return identifier

    def launch(label):
        debug.unlink(missing_ok=True)
        child = s.start([s.binaries['decnotify']], 'daemon-' + label)
        wait(lambda: live.bus.name_has_owner(B), 'notification daemon did not own private bus')
        proxy = live.bus.get_object(B, P)
        live.notifications = dbus.Interface(proxy, B)
        live.extension = dbus.Interface(proxy, E)
        assert live.notifications.GetServerInformation(timeout=RPC_TIMEOUT)[0] == 'decnotify'
        return child

    def shutdown(child):
        s.stop(child)
        wait(lambda: not live.bus.name_has_owner(B), 'notification name remained after daemon shutdown')

    def assert_private_logs():
        for path in root.glob('daemon-*.log'):
            contents = path.read_text()
            assert 'NH_PRIVATE_' not in contents and not any(secret in contents for secret in secrets), 'notification contents leaked into diagnostic log ' + path.name

    s.start_bus()
    s.start_xvfb()
    wait(lambda: s.run('xdotool', 'getdisplaygeometry') == '1280 900', 'private X server not ready')
    s.start([s.binaries['rdwm'], '--backend', 'x11', '--config', config], 'rdwm')
    wait(lambda: 'window id' in s.run('xprop', '-root', '_NET_SUPPORTING_WM_CHECK'), 'private WM failed')
    live.bus = dbus.bus.BusConnection(env['DBUS_SESSION_BUS_ADDRESS'])
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
    try: live.extension.ClearHistory(timeout=RPC_TIMEOUT)
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
    for mode in ('symlink', 'corrupt', 'unreadable'):
        history.unlink(missing_ok=True)
        target = root / ('preserved-' + mode + '.json')
        old_summary = 'NH_PRIVATE_OLD_' + mode
        secrets.append(old_summary)
        old_record = {'id': 1, 'app': 'Old private QA', 'icon': '', 'summary': old_summary, 'body': 'NH_PRIVATE_OLD_BODY_' + mode, 'entry': '', 'time': 0}
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


def main():
    argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter).parse_args()
    try:
        ms_bin = private.explicit_directory('MS_BIN_DIR')
        rdwm_bin = private.explicit_directory('RDWM_BIN_DIR')
        session = private.Session(
            'decebal-notify-history', tools=('Xvfb', 'dbus-daemon', 'xdotool', 'xprop', 'import'),
            binaries={'decnotify': ms_bin / 'decnotify', 'rdwm': rdwm_bin / 'rdwm'},
            evidence_env='NOTIFY_HISTORY_EVIDENCE_DIR', decebal_config='cfg/omarchy', path_dirs=[ms_bin],
            extra_env={'XDG_SESSION_TYPE': 'x11', 'DECNOTIFY_FORCE': '1', 'DECEBAL_BACKEND': 'x11'},
            require_non_root=True)
    except private.Skip as reason:
        print('SKIP: ' + str(reason))
        return 77
    except private.Usage as problem:
        print('ERROR: ' + str(problem), file=sys.stderr)
        return 2
    with session as s:
        scenario(s)
        if s.evidence.is_relative_to(s.root):
            print('Temporary evidence removed; set NOTIFY_HISTORY_EVIDENCE_DIR to a new directory to retain it')
    return 0


if __name__ == '__main__':
    private.run_main(main)
