#!/usr/bin/python3
"""Assert fixture wire types and lifecycle; invoked by sni-fixture.sh."""
import json
import os
import sys
import time
from pathlib import Path
import dbus
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib
DBusGMainLoop(set_as_default=True)
root = Path(sys.argv[1]).resolve()
assert os.environ['DBUS_SESSION_BUS_ADDRESS'] == 'unix:path=' + str(root / 'bus')
bus = dbus.SessionBus(private=True)
P = 'org.freedesktop.DBus.Properties'
S = 'org.kde.StatusNotifierItem'
W = 'org.kde.StatusNotifierWatcher'
C = 'org.decebal.TestSNI'
M = 'com.canonical.dbusmenu'
control = dbus.Interface(bus.get_object(C, '/TestControl'), C)
watch = bus.get_object(W, '/StatusNotifierWatcher')
owner = str(bus.get_name_owner(W))
signals = []
def record_signal(*args, **kwargs):
    signals.append(kwargs['member'])
bus.add_signal_receiver(record_signal, dbus_interface=S, member_keyword='member')
for _ in range(100):
    registered = watch.Get(W, 'RegisteredStatusNotifierItems', dbus_interface=P)
    if len(registered) == 2: break
    time.sleep(.02)
assert len(registered) == 2, registered
assert set(map(str, registered)) == {C + ".Item1/StatusNotifierItem", C + ".Item2/StatusNotifierItem"}, registered
for i, color in ((1, 'f03c5a'), (2, '18c7e8')):
    item = bus.get_object(C + '.Item' + str(i), '/StatusNotifierItem')
    p = item.GetAll(S, dbus_interface=P)
    width, height, pixels = p['IconPixmap'][0]
    assert (width, height, len(pixels)) == (24, 24, 2304)
    assert bytes(pixels)[:4] == bytes.fromhex('ff' + color)
    assert p['Menu'] == '/Menu' and bool(p['ItemIsMenu']) == (i == 2)
    item.Activate(12, 34, dbus_interface=S)
    item.SecondaryActivate(12, 34, dbus_interface=S)
    item.Scroll(-120, 'vertical', dbus_interface=S)
    item.ContextMenu(12, 34, dbus_interface=S)
    menu = dbus.Interface(bus.get_object(C + '.Item' + str(i), '/Menu'), M)
    assert not menu.AboutToShow(0)
    revision, layout = menu.GetLayout(0, -1, [])
    assert revision == 1 and layout[0] == 0
    children = layout[2]
    assert len(children) == 5 and not children[2][1]['enabled']
    assert children[1][1]['toggle-state'] == 1
    assert children[4][2][0][1]['label'] == 'Nested action'
    menu.Event(2, 'clicked', dbus.Int32(0), dbus.UInt32(123))
    assert menu.GetLayout(0, -1, [])[1][2][1][1]['toggle-state'] == 0
    menu.Event(6, 'clicked', dbus.Int32(0), dbus.UInt32(124))
control.Update(1, 'title', 'Changed asynchronously')
control.Update(1, 'status', 'NeedsAttention')
control.Update(1, 'color', '4fe187')
item = bus.get_object(C + '.Item1', '/StatusNotifierItem')
p = item.GetAll(S, dbus_interface=P)
assert p['Title'] == 'Changed asynchronously' and p['Status'] == 'NeedsAttention'
assert bytes(p['IconPixmap'][0][2])[:4] == bytes.fromhex('ff4fe187')
for mode, width in (('truncated', 24), ('oversize', 513)):
    control.Update(1, 'bad', mode)
    pixmap = item.GetAll(S, dbus_interface=P)['IconPixmap'][0]
    assert pixmap[0] == width and len(pixmap[2]) == 4
control.Update(1, 'bad', '')
for _ in range(100):
    while GLib.MainContext.default().iteration(False):
        pass
    if {'NewTitle', 'NewStatus', 'NewIcon'}.issubset(signals):
        break
    time.sleep(.01)
assert {'NewTitle', 'NewStatus', 'NewIcon'}.issubset(signals), signals
old = json.loads(str(control.Snapshot()))['1']['owner']
control.Drop(1)
assert len(watch.Get(W, 'RegisteredStatusNotifierItems', dbus_interface=P)) == 1
control.Spawn(1)
new = json.loads(str(control.Snapshot()))['1']['owner']
assert old != new
for host in ('org.decebal.TestHost1', 'org.decebal.TestHost2'):
    watch.RegisterStatusNotifierHost(host, dbus_interface=W)
assert watch.Get(W, 'IsStatusNotifierHostRegistered', dbus_interface=P)
assert str(bus.get_name_owner(W)) == owner
records = [json.loads(line) for line in (root / 'events.jsonl').read_text().splitlines()]
for method in ('Activate', 'SecondaryActivate', 'Scroll', 'ContextMenu', 'GetLayout', 'AboutToShow', 'Event'):
    assert {r['item'] for r in records if r['method'] == method} == {1, 2}, method
control.Quit()
print('PASS: two real D-Bus items, typed ARGB pixmaps, menu nesting/disabled/check state, actions, updates, malformed payloads, drop/reown, two host registrations and stable watcher owner')
print('Native backend rejection, signal consumption, pixels, input, missing-bus fallback and nonblocking paint checks require UI integration; not run by this protocol self-test.')
