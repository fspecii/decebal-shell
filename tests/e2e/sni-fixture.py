#!/usr/bin/python3
"""Private, real SNI + DBusMenu fixture.

Run bash tests/e2e/sni-fixture.sh for isolated protocol verification.
For native integration, set private HOME/XDG directories first. Choose a
fresh ROOT temporary directory, export DBUS_SESSION_BUS_ADDRESS to
unix:path=$ROOT/bus, start dbus-daemon --session --nofork with that --address,
then run this file with --root "$ROOT" [--watcher]. Start native clients on
that same bus. Omit --watcher to exercise native watcher ownership.
Never point this fixture at a live session bus. Terminate owned children
and remove the temporary directory afterward.

Control service/interface org.decebal.TestSNI at /TestControl supports:
Snapshot(), Update(index, field, value), Drop(index), Spawn(index), Quit().
Update fields: title, status, color (six hex digits), bad (oversize,
truncated, or empty string). JSON action logs: ROOT/events.jsonl.
"""
import argparse
import json
import os
from pathlib import Path
import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

SNI = 'org.kde.StatusNotifierItem'
WATCH = 'org.kde.StatusNotifierWatcher'
MENU = 'com.canonical.dbusmenu'
PROP = 'org.freedesktop.DBus.Properties'
CONTROL = 'org.decebal.TestSNI'
DBusGMainLoop(set_as_default=True)
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--root', required=True)
parser.add_argument('--watcher', action='store_true')
a = parser.parse_args()
root = Path(a.root).resolve()
address = os.environ.get('DBUS_SESSION_BUS_ADDRESS', '')
if address != 'unix:path=' + str(root / 'bus'):
    raise SystemExit('Refusing anything except the explicitly private fixture bus')
root.mkdir(parents=True, exist_ok=True)

def log(method, **kw):
    with (root / 'events.jsonl').open('a') as f:
        f.write(json.dumps(dict(method=method, **kw)) + '\n')

class Properties(dbus.service.Object):
    @dbus.service.method(PROP, in_signature='s', out_signature='a{sv}')
    def GetAll(self, interface):
        log('GetAll', interface=str(interface))
        return dbus.Dictionary(self.properties(str(interface)), signature='sv')

    @dbus.service.method(PROP, in_signature='ss', out_signature='v')
    def Get(self, interface, name):
        return self.properties(str(interface))[str(name)]

class Menu(Properties):
    def __init__(self, connection, index):
        self.index = index
        self.checked = True
        super().__init__(connection, '/Menu')

    def properties(self, interface):
        return {'Version': dbus.Int32(3), 'Status': dbus.String('normal'),
                'TextDirection': dbus.String('ltr'),
                'IconThemePath': dbus.Array([], signature='s')}

    def node(self, ident, label='', children=(), **props):
        values = {'label': dbus.String(label), 'enabled': dbus.Boolean(True),
                  'visible': dbus.Boolean(True)}
        values.update(props)
        return dbus.Struct((dbus.Int32(ident), dbus.Dictionary(values, signature='sv'),
                            dbus.Array(children, signature='v')), signature='ia{sv}av', variant_level=1)

    @dbus.service.method(MENU, in_signature='iias', out_signature='u(ia{sv}av)')
    def GetLayout(self, parent, depth, names):
        log('GetLayout', item=self.index, parent=int(parent), depth=int(depth))
        nested = self.node(5, 'More', [self.node(6, 'Nested action')], **{'children-display': dbus.String('submenu')})
        tree = self.node(0, children=[self.node(1, 'Open window'),
            self.node(2, 'Checked option', **{'toggle-type': dbus.String('checkmark'), 'toggle-state': dbus.Int32(int(self.checked))}),
            self.node(3, 'Disabled action', enabled=dbus.Boolean(False)),
            self.node(4, type=dbus.String('separator')), nested])
        return dbus.UInt32(1), nested if int(parent) == 5 else tree

    @dbus.service.method(MENU, in_signature='i', out_signature='b')
    def AboutToShow(self, ident):
        log('AboutToShow', item=self.index, id=int(ident))
        return False

    @dbus.service.method(MENU, in_signature='isvu', out_signature='')
    def Event(self, ident, event, data, timestamp):
        log('Event', item=self.index, id=int(ident), event=str(event), timestamp=int(timestamp))
        if int(ident) == 2 and str(event) == 'clicked':
            self.checked = not self.checked
            self.ItemsPropertiesUpdated([(2, {'toggle-state': dbus.Int32(int(self.checked))})], [])

    @dbus.service.signal(MENU, signature='a(ia{sv})a(ias)')
    def ItemsPropertiesUpdated(self, changed, removed): pass

    @dbus.service.signal(MENU, signature='ui')
    def LayoutUpdated(self, revision, parent): pass

class Item(Properties):
    def __init__(self, index):
        self.index = index
        self.bus = dbus.SessionBus(private=True)
        self.name = dbus.service.BusName(CONTROL + '.Item' + str(index), self.bus, do_not_queue=True)
        self.title = 'Fixture ' + ('red' if index == 1 else 'cyan')
        self.status = 'Active'
        self.color = 'f03c5a' if index == 1 else '18c7e8'
        self.bad = ''
        self.menu = Menu(self.bus, index)
        super().__init__(self.bus, '/StatusNotifierItem')
        self.register()

    def register(self):
        try:
            proxy = self.bus.get_object(WATCH, '/StatusNotifierWatcher', introspect=False)
            proxy.RegisterStatusNotifierItem(self.name.get_name(), dbus_interface=WATCH,
                reply_handler=lambda: None, error_handler=lambda e: None)
        except dbus.DBusException:
            pass

    def properties(self, interface):
        size = 513 if self.bad == 'oversize' else 24
        pixels = bytes.fromhex('ff' + self.color) * (24 * 24)
        if self.bad: pixels = pixels[:4]
        pixmap = dbus.Array([dbus.Struct((dbus.Int32(size), dbus.Int32(size), dbus.ByteArray(pixels)), signature='iiay')], signature='(iiay)')
        empty = dbus.Array([], signature='(iiay)')
        return {'Category': dbus.String('ApplicationStatus'), 'Id': dbus.String('fixture-' + str(self.index)),
            'Title': dbus.String(self.title), 'Status': dbus.String(self.status), 'WindowId': dbus.Int32(0),
            'IconThemePath': dbus.String(''), 'IconName': dbus.String(''), 'IconPixmap': pixmap,
            'OverlayIconName': dbus.String(''), 'OverlayIconPixmap': empty,
            'AttentionIconName': dbus.String(''), 'AttentionIconPixmap': pixmap,
            'AttentionMovieName': dbus.String(''), 'Menu': dbus.ObjectPath('/Menu'),
            'ItemIsMenu': dbus.Boolean(self.index == 2),
            'ToolTip': dbus.Struct((dbus.String(''), empty, dbus.String(self.title), dbus.String('Private fixture')), signature='sa(iiay)ss')}

    @dbus.service.method(SNI, in_signature='ii', out_signature='')
    def Activate(self, x, y): log('Activate', item=self.index, x=int(x), y=int(y))
    @dbus.service.method(SNI, in_signature='ii', out_signature='')
    def SecondaryActivate(self, x, y): log('SecondaryActivate', item=self.index, x=int(x), y=int(y))
    @dbus.service.method(SNI, in_signature='ii', out_signature='')
    def ContextMenu(self, x, y): log('ContextMenu', item=self.index, x=int(x), y=int(y))
    @dbus.service.method(SNI, in_signature='is', out_signature='')
    def Scroll(self, delta, orientation): log('Scroll', item=self.index, delta=int(delta), orientation=str(orientation))
    @dbus.service.signal(SNI, signature='')
    def NewIcon(self): pass
    @dbus.service.signal(SNI, signature='')
    def NewTitle(self): pass
    @dbus.service.signal(SNI, signature='s')
    def NewStatus(self, status): pass

class Watcher(Properties):
    def __init__(self, bus):
        self.name = dbus.service.BusName(WATCH, bus, do_not_queue=True)
        self.items = []
        self.hosts = []
        super().__init__(bus, '/StatusNotifierWatcher')

    def properties(self, interface):
        return {'RegisteredStatusNotifierItems': dbus.Array(self.items, signature='s'),
            'IsStatusNotifierHostRegistered': dbus.Boolean(bool(self.hosts)), 'ProtocolVersion': dbus.Int32(0)}

    @dbus.service.method(WATCH, in_signature='s', out_signature='', sender_keyword='sender')
    def RegisterStatusNotifierItem(self, service, sender=None):
        key = str(sender) + str(service) if str(service).startswith('/') else str(service) + '/StatusNotifierItem'
        if key not in self.items:
            self.items.append(key)
            self.StatusNotifierItemRegistered(key)

    @dbus.service.method(WATCH, in_signature='s', out_signature='')
    def RegisterStatusNotifierHost(self, service):
        if str(service) not in self.hosts:
            self.hosts.append(str(service))
            self.StatusNotifierHostRegistered()

    @dbus.service.signal(WATCH, signature='s')
    def StatusNotifierItemRegistered(self, key): pass
    @dbus.service.signal(WATCH, signature='s')
    def StatusNotifierItemUnregistered(self, key): pass
    @dbus.service.signal(WATCH, signature='')
    def StatusNotifierHostRegistered(self): pass

bus = dbus.SessionBus(private=True)
watcher = Watcher(bus) if a.watcher else None
items = {}
loop = GLib.MainLoop()

class Control(dbus.service.Object):
    @dbus.service.method(CONTROL, in_signature='', out_signature='s')
    def Snapshot(self):
        return json.dumps({str(i): {'owner': item.bus.get_unique_name(), 'title': item.title,
            'status': item.status, 'color': item.color} for i, item in items.items()})

    @dbus.service.method(CONTROL, in_signature='iss', out_signature='')
    def Update(self, index, field, value):
        item = items[int(index)]
        if field == 'status': item.status = str(value); item.NewStatus(item.status)
        elif field == 'title': item.title = str(value); item.NewTitle()
        elif field == 'color':
            if len(value) != 6: raise ValueError('Expected six hexadecimal digits')
            bytes.fromhex(str(value))
            item.color = str(value); item.bad = ''; item.NewIcon()
        elif field == 'bad':
            if value not in ('oversize', 'truncated', ''): raise ValueError('Invalid malformed mode')
            item.bad = str(value); item.NewIcon()
        else: raise ValueError('Unknown field')

    @dbus.service.method(CONTROL, in_signature='i', out_signature='')
    def Drop(self, index):
        item = items.pop(int(index))
        key = item.name.get_name() + '/StatusNotifierItem'
        if watcher and key in watcher.items:
            watcher.items.remove(key); watcher.StatusNotifierItemUnregistered(key)
        item.menu.remove_from_connection(); item.remove_from_connection(); item.bus.close()

    @dbus.service.method(CONTROL, in_signature='i', out_signature='')
    def Spawn(self, index):
        if int(index) in items: raise ValueError('Already present')
        items[int(index)] = Item(int(index))

    @dbus.service.method(CONTROL, in_signature='', out_signature='')
    def Quit(self): GLib.idle_add(loop.quit)

control_name = dbus.service.BusName(CONTROL, bus, do_not_queue=True)
control = Control(bus, '/TestControl')
items.update({i: Item(i) for i in (1, 2)})
def owner_changed(name, old, new):
    if name == WATCH and new:
        for item in items.values(): item.register()
bus.add_signal_receiver(owner_changed, signal_name='NameOwnerChanged', dbus_interface='org.freedesktop.DBus')
(root / 'ready').write_text(bus.get_unique_name())
loop.run()
