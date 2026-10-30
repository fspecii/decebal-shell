#!/usr/bin/env python3
"""Native notification layout/input regression. Never uses the live session.

NOTIFY_BINARY=/absolute/decnotify NOTIFY_LARGE_TEXT_EVIDENCE=/tmp/new-directory python3 ...
Set NOTIFY_EXPECT_BASELINE=1 only to record a pre-fix binary (no repair assertions).
"""
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time

import dbus
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib
from PIL import Image

DBusGMainLoop(set_as_default=True)
repo = Path(__file__).resolve().parents[2]
binary = Path(os.environ.get('NOTIFY_BINARY', repo / 'target/debug/decnotify')).resolve()
evidence = Path(os.environ.get('NOTIFY_LARGE_TEXT_EVIDENCE') or tempfile.mkdtemp(prefix='notify-large-text.')).resolve()
evidence.mkdir(parents=True, exist_ok=True)
baseline = os.environ.get('NOTIFY_EXPECT_BASELINE') == '1'
results = []
B, P, E = 'org.freedesktop.Notifications', '/org/freedesktop/Notifications', 'org.decebal.Notifications'


def wait(fn, message, seconds=12):
    end = time.monotonic() + seconds
    last = None
    while time.monotonic() < end:
        while GLib.MainContext.default().pending():
            GLib.MainContext.default().iteration(False)
        try:
            value = fn()
            if value:
                return value
        except (OSError, ValueError, KeyError, IndexError, dbus.DBusException, subprocess.CalledProcessError) as error:
            last = error
        time.sleep(.04)
    raise AssertionError(message + (': ' + str(last) if last else ''))


class Case:
    def __init__(self, label, mode, text, screen):
        self.root = evidence / label
        self.root.mkdir()  # Fresh paths prevent a prior history from changing this case.
        self.label, self.mode, self.text, self.screen = label, mode, text, screen
        self.processes, self.logs = [], []
        self.env = dict(os.environ)
        for key in list(self.env):
            if key in ('DISPLAY', 'WAYLAND_DISPLAY', 'XAUTHORITY', 'SESSION_MANAGER', 'AT_SPI_BUS_ADDRESS', 'DBUS_STARTER_ADDRESS', 'DBUS_STARTER_BUS_TYPE', 'RDWM_SOCK') or key.startswith(('DECNOTIFY_DEBUG_', 'DECBAR_DEBUG_')):
                self.env.pop(key, None)
        for key, directory in {'HOME': 'home', 'XDG_CONFIG_HOME': 'cfg', 'XDG_CONFIG_DIRS': 'nocfg', 'XDG_DATA_HOME': 'data', 'XDG_DATA_DIRS': 'nodata', 'XDG_RUNTIME_DIR': 'run', 'XDG_STATE_HOME': 'state', 'XDG_CACHE_HOME': 'cache', 'DECEBAL_CONFIG': 'shell', 'DECEBAL_THEMES': 'themes', 'DECEBAL_SYSTEM_THEMES': 'system-themes', 'DECEBAL_WIDGETS': 'widgets', 'DECEBAL_PALETTES': 'palettes'}.items():
            path = self.root / directory
            path.mkdir()
            self.env[key] = str(path)
        os.chmod(self.env['XDG_RUNTIME_DIR'], 0o700)
        self.number = next(n for n in range(520, 580) if not Path(f'/tmp/.X11-unix/X{n}').exists() and not Path(f'/tmp/.X{n}-lock').exists())
        self.env.update(DISPLAY=f':{self.number}', DBUS_SESSION_BUS_ADDRESS='unix:path=' + str(self.root / 'bus'), DBUS_SYSTEM_BUS_ADDRESS='unix:path=' + str(self.root / 'no-system'), DECEBAL_HIGH_CONTRAST='0', DECEBAL_REDUCE_MOTION='1', DECEBAL_SYSTEM_AUTOMATIC='0', DECEBAL_COLOR_SCHEME=mode, DECNOTIFY_FORCE='1', DECEBAL_BACKEND='x11', XDG_SESSION_TYPE='x11', DECNOTIFY_DEBUG_HISTORY=str(self.root / 'history-layout.json'), DECNOTIFY_DEBUG_BANNERS=str(self.root / 'banner-layout.json'))
        self.env.pop('DECEBAL_FONT_SCALE', None)
        if text != 1:
            self.env['DECEBAL_FONT_SCALE'] = str(text)
        self.selection(1)
        (self.root / 'cfg/decebal').mkdir()
        (self.root / 'cfg/decebal/notifications.toml').write_text('dnd = false\nbanner_seconds = 120\n')

    def selection(self, scale):
        target = self.root / 'shell/decebal.json'
        temporary = target.with_suffix('.new')
        temporary.write_text(json.dumps({'theme': {'name': 'macos', 'mode': self.mode, 'font': {'scale': scale}}}))
        os.replace(temporary, target)

    def start(self, args, name):
        log = (self.root / (name + '.log')).open('w')
        self.logs.append(log)
        process = subprocess.Popen(list(map(str, args)), env=self.env, stdout=log, stderr=log, start_new_session=True)
        self.processes.append(process)
        return process

    def run(self, *args):
        p = subprocess.run(list(map(str, args)), env=self.env, text=True, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        if p.returncode and not ('xdotool' in args and 'search' in args):
            raise subprocess.CalledProcessError(p.returncode, args)
        return p.stdout.strip()

    def launch(self, name):
        process = self.start([binary], name)
        wait(lambda: self.bus.name_has_owner(B), 'private notification name')
        proxy = self.bus.get_object(B, P)
        self.notify, self.extension = dbus.Interface(proxy, B), dbus.Interface(proxy, E)
        return process

    def stop(self, process):
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()

    def setup(self):
        self.start(['dbus-daemon', '--session', '--nofork', '--address=' + self.env['DBUS_SESSION_BUS_ADDRESS']], 'bus')
        wait(lambda: (self.root / 'bus').is_socket(), 'private bus')
        server = self.start(['Xvfb', self.env['DISPLAY'], '-screen', '0', f'{self.screen[0]}x{self.screen[1]}x24', '-nolisten', 'tcp'], 'xvfb')
        def owned_display():
            if server.poll() is not None:
                raise RuntimeError('Private Xvfb exited')
            lock = Path(f'/tmp/.X{self.number}-lock')
            return lock.exists() and int(lock.read_text().strip()) == server.pid and self.run('xdotool', 'getdisplaygeometry') == f'{self.screen[0]} {self.screen[1]}'
        wait(owned_display, 'owned private display')
        self.run('xdotool', 'mousemove', 0, 0)
        self.bus = dbus.bus.BusConnection(self.env['DBUS_SESSION_BUS_ADDRESS'])
        self.actions = []
        self.bus.add_signal_receiver(lambda identifier, key: self.actions.append((int(identifier), str(key))), signal_name='ActionInvoked', dbus_interface=B)
        self.daemon = self.launch('daemon')

    def read_layout(self, name):
        # The optional production diagnostics are rewritten after native redraws.
        # A reader may briefly observe truncate-before-write; retry that observation.
        for _ in range(100):
            try:
                return json.loads((self.root / name).read_text())
            except (OSError, ValueError):
                time.sleep(.005)
        return json.loads((self.root / name).read_text())

    def history(self):
        return self.read_layout('history-layout.json')

    def banners(self):
        return self.read_layout('banner-layout.json')

    def capture(self, win, label):
        path = self.root / (label + '.png')
        self.run('import', '-window', win, path)
        return Image.open(path).convert('RGB')

    def geometry(self, win):
        return {key: int(value) for key, value in (line.split('=', 1) for line in self.run('xdotool', 'getwindowgeometry', '--shell', win).splitlines())}

    def send(self, index, timeout=60000):
        return int(self.notify.Notify('Long application name for notification testing', dbus.UInt32(0), '', f'{index}. A long notification title that needs enough space to remain readable', 'First body line has useful details.\nSecond body line should not overlap.\nThird body line stays above action buttons.', dbus.Array(['open', 'Open details', 'later', 'Remind me later', 'dismiss', 'Dismiss'], signature='s'), dbus.Dictionary({'resident': dbus.Boolean(True)}, signature='sv'), dbus.Int32(timeout)))

    def key(self, *keys):
        self.run('xdotool', 'key', '--clearmodifiers', *keys)

    def click(self, win, rect):
        x, y, w, h = rect
        self.run('xdotool', 'mousemove', '--window', win, int(x + w / 2), int(y + h / 2), 'click', 1)
        self.run('xdotool', 'mousemove', 0, 0)

    def assert_rect(self, rect):
        x, y, w, h = rect
        assert min(x, y) >= -1 and x + w <= self.screen[0] + 1 and y + h <= self.screen[1] + 1, rect

    def reveal(self, key):
        for _ in range(40):
            state = self.history()
            rect = state[key]
            if rect[1] >= 0 and rect[1] + rect[3] <= state['size'][1]:
                return rect
            self.run('xdotool', 'mousemove', '--window', state['window'], min(100, state['size'][0] // 2), max(1, state['size'][1] // 2), 'click', 4 if rect[1] < 0 else 5)
            time.sleep(.04)
        raise AssertionError('Cannot reveal ' + key)

    def check(self):
        ids = [self.send(i) for i in range(1, 4)]
        windows = wait(lambda: self.run('xdotool', 'search', '--onlyvisible', '--classname', '^decnotify$').splitlines(), 'mapped banners')
        time.sleep(.12)
        self.capture(windows[0], 'banner')
        self.capture('root', 'banners-root')
        banner_geometry = self.geometry(windows[0])
        if not baseline:
            self.assert_rect([banner_geometry['X'], banner_geometry['Y'], banner_geometry['WIDTH'], banner_geometry['HEIGHT']])
        dynamic = None
        if not baseline:
            state = wait(lambda: self.banners() if {i['id'] for i in self.banners()['active'] + self.banners()['queued']} == set(ids) else None, 'complete banner layout')
            assert {i['id'] for i in state['active'] + state['queued']} == set(ids)
            for item in state['active']:
                self.assert_rect(item['rect'])
                bottom = 0
                for rect in item['body_rects']:
                    assert rect[1] >= bottom
                    bottom = rect[1] + rect[3]
                for button in item['buttons']:
                    assert button['rect'][1] >= bottom
                    assert button['rect'][1] + button['rect'][3] <= item['rect'][3] + 1
            if self.text == 1:
                before = state
                self.selection(1.4)
                changed = wait(lambda: self.banners() if self.banners()['font_scale'] > 1.39 else None, 'font-only toast reflow')
                assert {i['id'] for i in changed['active'] + changed['queued']} == set(ids)
                old = {i['id']: i for i in before['active']}
                for item in changed['active']:
                    if item['id'] in old:
                        assert item['window'] == old[item['id']]['window']
                        assert item['deadline'] == old[item['id']]['deadline']
                for item in changed['queued']:
                    if item['id'] in old:
                        assert item['started'] and item['deadline'] == old[item['id']]['deadline']
                assert changed['active'][0]['rect'][3] != before['active'][0]['rect'][3] or changed['active'][0]['buttons'] != before['active'][0]['buttons']
                self.capture('root', 'font-only-reflow')
                self.selection(1)
                wait(lambda: self.banners()['font_scale'] == 1, 'restore font')
                restored = self.banners()
                for item in restored['active']:
                    if item['id'] in old:
                        assert item['deadline'] == old[item['id']]['deadline']
                dynamic = True
            # Real action at the shared native rectangle, with typed signal verification.
            active = self.banners()['active'][0]
            self.click(active['window'], active['buttons'][0]['rect'])
            wait(lambda: (active['id'], active['buttons'][0]['key']) in self.actions, 'native banner action signal')
            state = self.banners()
            queued_before = {i['id'] for i in state['queued']}
            if queued_before:
                self.click(active['window'], active['close_rect'])
                wait(lambda: any(i['id'] in queued_before for i in self.banners()['active']), 'queued banner promoted into available space')
        self.extension.ShowHistory()
        wait(lambda: (self.root / 'history-layout.json').exists(), 'history debug')
        history = wait(lambda: self.history() if len(self.history()['items']) == 3 else None, 'complete notification history')
        win = history['window']
        self.capture(win, 'history')
        self.capture('root', 'history-root')
        history_geometry = self.geometry(win)
        if not baseline:
            self.assert_rect([history_geometry['X'], history_geometry['Y'], history_geometry['WIDTH'], history_geometry['HEIGHT']])
            if dynamic:
                history_bytes = (self.root / 'state/decebal/notification-history.json').read_bytes()
                self.selection(1.4)
                changed = wait(lambda: self.history() if self.history()['font_scale'] > 1.39 else None, 'font-only history reflow')
                assert changed['window'] == win and [i['id'] for i in changed['items']] == [i['id'] for i in history['items']]
                assert changed['size'] != history['size'] or changed['row_rects'] != history['row_rects']
                assert (self.root / 'state/decebal/notification-history.json').read_bytes() == history_bytes
                self.capture(win, 'history-font-only-reflow')
                self.selection(1)
                wait(lambda: self.history()['font_scale'] == 1, 'restore history font')
            self.key('Tab')
            wait(lambda: self.history()['focus'] == 'clear', 'Clear keyboard focus')
            self.key('Tab', 'Return')
            wait(lambda: bool(self.extension.GetDnd()), 'Enter toggles DND')
            self.key('space')
            wait(lambda: not bool(self.extension.GetDnd()), 'Space toggles DND')
            self.key('Tab')
            wait(lambda: str(self.history()['focus']).startswith('item:'), 'notification keyboard focus')
            self.capture(win, 'keyboard-row-focus')
            if dynamic:
                focus = self.history()['focus']
                self.selection(1.4)
                changed = wait(lambda: self.history() if self.history()['font_scale'] > 1.39 else None, 'focused row reflow')
                assert changed['focus'] == focus
                index = next(i for i, item in enumerate(changed['items']) if focus == 'item:' + str(item['id']))
                rect = changed['row_rects'][index - changed['scroll']]
                assert rect[1] >= 0 and rect[1] + rect[3] <= changed['size'][1]
                self.capture(win, 'keyboard-row-font-reflow')
                self.selection(1)
                wait(lambda: self.history()['font_scale'] == 1 and self.history()['focus'] == focus, 'focused row restore')
            self.key('Delete')
            wait(lambda: len(self.history()['items']) == 2, 'Delete dismisses focused notification')
            row = self.history()['row_rects'][0]
            # Wheel-pan a genuinely tiny viewport until the row centre is visible.
            for _ in range(30):
                row = self.history()['row_rects'][0]
                if row[1] >= 0 and row[1] + row[3] <= self.history()['size'][1]:
                    break
                self.run('xdotool', 'mousemove', '--window', win, 100, 100, 'click', 5)
                time.sleep(.03)
            self.click(win, row)
            wait(lambda: len(self.history()['items']) == 1, 'mouse shares row dismiss action')
            self.key('shift+Tab')
            wait(lambda: self.history()['focus'] == 'dnd', 'Shift-Tab returns to DND')
            self.key('shift+Tab', 'Return')
            wait(lambda: not self.history()['items'], 'keyboard Clear action')
        self.key('Escape')
        wait(lambda: not self.run('xdotool', 'search', '--onlyvisible', '--classname', '^decnotify$'), 'Escape closes history')
        if not baseline:
            # Filling the viewport queues the next timer without starting its clock.
            closed = []
            receiver = self.bus.add_signal_receiver(lambda identifier, reason: closed.append((int(identifier), int(reason))), signal_name='NotificationClosed', dbus_interface=B)
            blockers = [self.send(i) for i in range(10, 15)]
            timed = self.send(99, timeout=250)
            wait(lambda: any(item['id'] == timed and not item['started'] for item in self.banners()['queued']), 'timer is queued and not started')
            time.sleep(.35)
            while GLib.MainContext.default().pending():
                GLib.MainContext.default().iteration(False)
            assert (timed, 1) not in closed
            assert any(item['id'] == timed and not item['started'] for item in self.banners()['queued'])
            for identifier in blockers:
                self.notify.CloseNotification(dbus.UInt32(identifier))
            wait(lambda: (timed, 1) in closed, 'queued timer expires after promotion')
            wait(lambda: timed not in {item['id'] for item in self.banners()['active'] + self.banners()['queued']}, 'expired notification leaves the UI queue')
            receiver.remove()
        self.stop(self.daemon)
        wait(lambda: not self.bus.name_has_owner(B), 'private daemon shutdown')
        history_path = self.root / 'state/decebal/notification-history.json'
        history_path.write_text('not valid json')
        self.daemon = self.launch('daemon-error')
        pending_id = self.send(4)
        self.extension.ShowHistory()
        error = wait(lambda: self.history() if self.history().get('error') and self.history().get('retry_rect') else None, 'history error/Retry')
        win = error['window']
        self.capture(win, 'retry-error')
        self.capture('root', 'retry-error-root')
        old_error = error['error']
        history_path.unlink()
        history_path.mkdir()
        self.click(win, error['retry_rect'] if baseline else self.reveal('retry_rect'))
        wait(lambda: self.history()['error'] != old_error, 'mouse Retry re-reads repaired target')
        history_path.rmdir()
        if baseline:
            self.click(win, self.history()['retry_rect'])
        else:
            for _ in range(len(self.history()['items']) + 4):
                previous = self.history()['focus']
                self.key('Tab')
                wait(lambda: self.history()['focus'] != previous, 'keyboard focus advances')
                if self.history()['focus'] == 'retry':
                    break
            assert self.history()['focus'] == 'retry', 'keyboard Retry focus'
            self.capture(win, 'keyboard-retry-focus')
            self.key('Return')
        recovered = wait(lambda: self.history() if not self.history().get('error') else None, 'Retry clears repaired history error')
        assert pending_id in [item['id'] for item in recovered['items']], 'pending history retained through recovery'
        self.capture(win, 'retry-recovered')
        return {'case': self.label, 'screen': self.screen, 'font_scale': self.text, 'banner_geometry': banner_geometry, 'history_geometry': history_geometry, 'font_only_reflow': dynamic, 'mouse_and_keyboard': not baseline, 'history_recovery_preserves_pending': True}

    def cleanup(self):
        for process in reversed(self.processes):
            self.stop(process)
        for log in self.logs:
            log.close()
        if hasattr(self, 'bus'):
            self.bus.close()


for mode in ('dark', 'light'):
    for text in (1, 2):
        sizes = [('normal', (1280, 900)), ('narrow', (320, 360))]
        if os.environ.get('NOTIFY_TINY_VIEWPORT') == '1':
            sizes.append(('tiny', (320, 180)))
        for size, screen in sizes:
            label = f'{mode}-text{text*100}-{size}'
            if os.environ.get('NOTIFY_CASE_FILTER', '') not in label:
                continue
            case = Case(label, mode, text, screen)
            try:
                case.setup()
                result = case.check()
                results.append(result)
                (case.root / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
                print('PASS', case.label, flush=True)
            finally:
                case.cleanup()
assert results, 'No notification QA cases matched'
(evidence / 'results.json').write_text(json.dumps({'binary': str(binary), 'sha256': hashlib.sha256(binary.read_bytes()).hexdigest(), 'baseline': baseline, 'cases': results}, indent=2) + '\n')
print('PASS native notification usability matrix:', evidence)
