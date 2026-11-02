#!/usr/bin/python3
"""Persistent native toast/history repaint from resolved themes; private owned Xvfb/D-Bus only.

Needs an explicit, already built binary directory (nothing is built, installed or defaulted):
  MS_BIN_DIR   absolute directory with decnotify and decthemectl
Optional:
  E2E_DISPLAY                a private display :700..:65535 that must be unoccupied (default: allocated)
  NOTIFY_THEME_EVIDENCE_DIR  a NEW directory for screenshots/logs/result.json/cleanup.json (default: inside the private tree)
  KEEP_TMP=1                 keep the private tree after a pass
Exit: 0 pass, 1 failure or missing prerequisite or occupied explicit display, 2 invalid setting, 128+N after signal N.
Private Xvfb, session bus and every child are owned by tests/e2e/notify_private.py (see docs/notification-history.md).
"""
import json
import os
from pathlib import Path
import sys
import time

import dbus
from PIL import Image

sys.path.insert(0, str(Path(__file__).resolve().parent))
import notify_private as private

B = 'org.freedesktop.Notifications'
P = '/org/freedesktop/Notifications'
E = 'org.decebal.Notifications'
CATCH = private.RETRYABLE + (dbus.DBusException, AssertionError)
RPC_TIMEOUT = 5  # a hung daemon must not delay SIGTERM cleanup


def wait(check, message):
    return private.wait(check, message, 10, CATCH, .06)


def scenario(s, bins):
    root, evidence = s.root, s.evidence
    settings = root / 'shell/decebal.json'
    history = root / 'state/decebal/notification-history.json'
    (root / 'cfg/decebal').mkdir(mode=0o700)
    (root / 'cfg/decebal/notifications.toml').write_text('dnd = false\nbanner_seconds = 120\n')
    s.env.update(DECNOTIFY_DEBUG_HISTORY=str(root / 'history-debug.json'), DECNOTIFY_DEBUG_BANNERS=str(root / 'banner-debug.json'))
    s.extra_evidence.extend((root / 'history-debug.json', root / 'banner-debug.json'))
    live = {}
    s.before_cleanup(lambda: live['bus'].close() if 'bus' in live else None)

    def selection(value):
        tmp = settings.with_suffix('.new'); tmp.write_text(json.dumps({'theme': value})); os.replace(tmp, settings)
    def resolved(): return json.loads(s.run(bins / 'decthemectl', 'get', '--json'))
    def expected():
        h = resolved()['colors']['popup']; return tuple(bytes.fromhex(h.lstrip('#')))
    def capture(win, label):
        p = evidence / (label + '.png'); s.run('import', '-window', str(win), p); return Image.open(p).convert('RGB')
    def has_background(win, wanted, label):
        image = capture(win, label)
        # An untouched interior fill, away from icon/text/buttons and rounded border.
        return image.getpixel((4, image.height // 2)) == wanted

    s.start_bus()
    s.start_xvfb()
    wait(lambda: s.run('xdotool', 'getdisplaygeometry') == '1280 900', 'private Xvfb')
    bus = live['bus'] = dbus.bus.BusConnection(s.env['DBUS_SESSION_BUS_ADDRESS'])
    selection({'name': 'nord', 'mode': 'dark'})
    daemon = s.start([bins / 'decnotify'], 'daemon'); wait(lambda: bus.name_has_owner(B), 'daemon owns private name')
    proxy = bus.get_object(B, P); notify = dbus.Interface(proxy, B); extension = dbus.Interface(proxy, E)
    ident = int(notify.Notify('Theme QA', dbus.UInt32(0), '', 'Theme coherence', 'Synthetic message', dbus.Array([], signature='s'), dbus.Dictionary({}, signature='sv'), dbus.Int32(0), timeout=RPC_TIMEOUT))
    banner = int(wait(lambda: s.run('xdotool', 'search', '--onlyvisible', '--classname', '^decnotify$'), 'toast visible').splitlines()[0])
    wait(lambda: history.exists(), 'history saved'); original = history.read_bytes()
    first = expected(); wait(lambda: has_background(banner, first, 'toast-nord'), 'toast matches resolved Nord')
    selection({'name': 'rose-pine', 'mode': 'dark'})
    second = expected(); assert first != second
    wait(lambda: has_background(banner, second, 'toast-rose-pine'), 'same existing toast repainted')
    assert s.alive(daemon) and history.read_bytes() == original
    print('PASS persistent native toast repaints Nord→Rose Pine without replacement/history writes', flush=True)
    extension.ShowHistory(timeout=RPC_TIMEOUT)
    debug = root / 'history-debug.json'
    panel = int(wait(lambda: json.loads(debug.read_text()).get('window'), 'history opens'))
    wait(lambda: has_background(panel, second, 'history-rose-pine'), 'history uses same resolved palette')
    extension.SetDnd(dbus.Boolean(True), timeout=RPC_TIMEOUT); assert extension.GetDnd(timeout=RPC_TIMEOUT)
    for label, value in [
        ('independent-palette', {'name': 'macos', 'mode': 'dark', 'palette': 'nord'}),
        ('dynamic', {'name': 'macos', 'mode': 'dark', 'dynamic': {'enabled': True, 'surfaces': 'full', 'strength': 1, 'scheme': 'vibrant', 'source': '#c51772', 'seed': '#c51772', 'tone': 35, 'mode': 'keep'}}),
        ('light', {'name': 'macos', 'mode': 'light'}),
        ('missing-theme', {'name': 'missing-private-theme', 'mode': 'dark'})]:
        selection(value); want = expected()
        wait(lambda: has_background(panel, want, 'history-' + label), 'persistent history ' + label + ' matches resolved popup')
        assert s.alive(daemon) and extension.GetDnd(timeout=RPC_TIMEOUT) and history.read_bytes() == original
        assert json.loads(debug.read_text())['window'] == panel
        assert ident in [item['id'] for item in json.loads(debug.read_text())['items']]
        assert not extension.GetHistoryStatus(timeout=RPC_TIMEOUT)[0]
        assert notify.GetServerInformation(timeout=RPC_TIMEOUT)[0] == 'decnotify' and 'body' in notify.GetCapabilities(timeout=RPC_TIMEOUT)
        if label == 'missing-theme': assert resolved()['problem']
        print('PASS persistent history ' + label + '; DND/history/window identity preserved', flush=True)
    # Synthetic installed icon packs make pack selection observable without
    # depending on any machine-wide theme, app icon or notification artwork.
    extension.ToggleHistory(timeout=RPC_TIMEOUT)
    extension.SetDnd(dbus.Boolean(False), timeout=RPC_TIMEOUT)
    notify.CloseNotification(dbus.UInt32(ident), timeout=RPC_TIMEOUT)
    colors = {'a': (231, 17, 123), 'b': (17, 231, 123), 'new': (231, 123, 17), 'raw': (17, 123, 231)}
    for pack in ('a', 'b'):
        directory = root / 'data/icons' / ('NotifyQA-' + pack)
        (directory / '36x36/apps').mkdir(parents=True)
        (directory / 'index.theme').write_text('[Icon Theme]\nName=NotifyQA-' + pack + '\nDirectories=36x36/apps\n\n[36x36/apps]\nSize=36\nType=Fixed\nContext=Applications\n')
        Image.new('RGB', (36, 36), colors[pack]).save(directory / '36x36/apps/notify-qa.png')
        if pack == 'b':
            Image.new('RGB', (36, 36), colors['new']).save(directory / '36x36/apps/notify-missing-qa.png')
    def icon_selection(pack):
        tmp = settings.with_suffix('.new')
        tmp.write_text(json.dumps({'theme': {'name': 'nord', 'mode': 'dark'}, 'icons': {'theme': 'NotifyQA-' + pack, 'fallback': 'none'}}))
        os.replace(tmp, settings)
    def send_icon(app, icon, hints=None):
        return int(notify.Notify(app, dbus.UInt32(0), icon, 'Icon refresh', 'Synthetic icon test', dbus.Array([], signature='s'), dbus.Dictionary(hints or {}, signature='sv'), dbus.Int32(90000), timeout=RPC_TIMEOUT))
    def active():
        return {item['id']: item for item in json.loads((root / 'banner-debug.json').read_text())['active']}
    def icon_color(window, wanted, label):
        return sum(count for count, pixel in capture(window, label).getcolors(1 << 24) if pixel == wanted) > 300
    icon_selection('a')
    # Wait for the watcher to load A before populating its positive/negative cache.
    wait(lambda: json.loads(s.run(bins / 'decthemectl', 'icons', 'status', '--json'))['active'] == 'NotifyQA-a', 'icon pack A resolves')
    time.sleep(1.0)
    pack_id = send_icon('Icon Pack QA', 'notify-qa')
    missing_id = send_icon('Missing Icon QA', 'notify-missing-qa')
    rgba = dbus.ByteArray(bytes(colors['raw'] + (255,)) * 64)
    image_hint = dbus.Struct((dbus.Int32(8), dbus.Int32(8), dbus.Int32(32), dbus.Boolean(True), dbus.Int32(8), dbus.Int32(4), rgba), signature='iiibiiay')
    raw_id = send_icon('Image Hint QA', 'notify-qa', {'image-data': image_hint})
    before = wait(lambda: active() if {pack_id, missing_id, raw_id} <= active().keys() else None, 'three icon banners')
    wait(lambda: icon_color(before[pack_id]['window'], colors['a'], 'icon-pack-a'), 'initial A artwork')
    wait(lambda: icon_color(before[raw_id]['window'], colors['raw'], 'icon-raw-a'), 'image hint overrides A')
    assert not icon_color(before[missing_id]['window'], colors['new'], 'icon-missing-a')
    wait(lambda: {pack_id, missing_id, raw_id} <= {item['id'] for item in json.loads(history.read_text())}, 'icon notifications persisted')
    saved_icons = history.read_bytes()
    icon_selection('b')
    wait(lambda: icon_color(before[pack_id]['window'], colors['b'], 'icon-pack-b'), 'same existing banner follows B')
    wait(lambda: icon_color(before[missing_id]['window'], colors['new'], 'icon-missing-b'), 'same missing banner gains icon')
    wait(lambda: icon_color(before[raw_id]['window'], colors['raw'], 'icon-raw-b'), 'image hint survives icon refresh')
    after = active()
    for item in (pack_id, missing_id, raw_id):
        assert (after[item]['window'], after[item]['deadline']) == (before[item]['window'], before[item]['deadline'])
    assert history.read_bytes() == saved_icons
    next_id = send_icon('Icon Pack QA', 'notify-qa')
    next_banner = wait(lambda: active().get(next_id), 'new banner after icon refresh')
    wait(lambda: icon_color(next_banner['window'], colors['b'], 'icon-next-b'), 'subsequent matching notification uses B')
    # Opening history deliberately dismisses banners; verify its cache separately
    # rather than pretending banner identities survive that existing UI action.
    wait(lambda: next_id in {item['id'] for item in json.loads(history.read_text())}, 'subsequent notification persisted')
    saved_icons = history.read_bytes()
    old_panel = panel
    extension.ShowHistory(timeout=RPC_TIMEOUT)
    panel = int(wait(lambda: json.loads(debug.read_text()).get('window') if json.loads(debug.read_text()).get('window') != old_panel else None, 'fresh icon history opens'))
    wait(lambda: icon_color(panel, colors['b'], 'icon-history-b'), 'history initially uses B')
    icon_selection('a')
    wait(lambda: icon_color(panel, colors['a'], 'icon-history-a'), 'existing history follows A')
    assert not icon_color(panel, colors['new'], 'icon-history-missing-a')
    icon_selection('b')
    wait(lambda: icon_color(panel, colors['b'], 'icon-history-b-again'), 'existing history follows B again')
    wait(lambda: icon_color(panel, colors['new'], 'icon-history-new'), 'history retries previous missing icon')
    assert history.read_bytes() == saved_icons
    assert s.alive(daemon)
    print('PASS icon A→B refreshes retained banners/history, missing lookup, subsequent notifications; image hint/IDs/deadlines/history preserved', flush=True)
    (evidence / 'result.json').write_text(json.dumps({'daemon_sha256': s.hashes['decnotify'], 'icon_pack_refresh': True, 'negative_cache_refresh': True, 'supplied_image_preserved': True, 'banner_identity_deadline_preserved': True, 'daemon_pid': daemon.proc.pid, 'toast_window': banner, 'history_window': panel, 'notification_id': ident, 'history_unchanged': True, 'dnd_preserved': True}, indent=2) + '\n')
    print('PASS notification theme native QA; evidence ' + str(evidence), flush=True)


def main():
    try:
        bins = private.explicit_directory('MS_BIN_DIR')
        session = private.Session(
            'decebal-notify-theme', tools=('Xvfb', 'dbus-daemon', 'xdotool', 'import'),
            binaries={'decnotify': bins / 'decnotify', 'decthemectl': bins / 'decthemectl'},
            evidence_env='NOTIFY_THEME_EVIDENCE_DIR', decebal_config='shell', path_dirs=[bins],
            extra_env={'DECNOTIFY_FORCE': '1', 'DECEBAL_BACKEND': 'x11'})
    except private.Skip as reason:
        raise SystemExit('Missing prerequisite: ' + str(reason))
    except private.Usage as problem:
        print('ERROR: ' + str(problem), file=sys.stderr)
        return 2
    with session as s:
        scenario(s, bins)
    return 0


if __name__ == '__main__':
    private.run_main(main)
