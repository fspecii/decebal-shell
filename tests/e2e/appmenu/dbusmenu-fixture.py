#!/usr/bin/python3 -I
"""Real libdbusmenu-glib exporter for the app-menu tests (the library appmenu-gtk-module and many
GTK/Electron exporters build on). Registers --window with the registrar on the private bus named
by DBUS_SESSION_BUS_ADDRESS and appends "activated <name> <timestamp>" to --record for every
item-activated signal libdbusmenu delivers."""
import argparse
import gi

gi.require_version("Dbusmenu", "0.4")
from gi.repository import Dbusmenu, Gio, GLib  # noqa: E402

p = argparse.ArgumentParser()
p.add_argument("--window", type=int, required=True)
p.add_argument("--record", required=True)
p.add_argument("--ready", required=True)
a = p.parse_args()

PATH = "/org/decebal/fixture/menu"
server = Dbusmenu.Server.new(PATH)
root = Dbusmenu.Menuitem.new()


def item(parent, name, label, **props):
    mi = Dbusmenu.Menuitem.new()
    mi.property_set(Dbusmenu.MENUITEM_PROP_LABEL, label)
    for k, v in props.items():
        key = k.replace("_", "-")
        if isinstance(v, bool):
            mi.property_set_bool(key, v)
        elif isinstance(v, int):
            mi.property_set_int(key, v)
        else:
            mi.property_set(key, v)

    def activated(_mi, ts):
        with open(a.record, "a") as f:
            f.write(f"activated {name} {ts}\n")

    mi.connect("item-activated", activated)
    parent.child_append(mi)
    return mi


file = item(root, "file", "_File", children_display="submenu")
item(file, "new", "_New")
sep = Dbusmenu.Menuitem.new()
sep.property_set(Dbusmenu.MENUITEM_PROP_TYPE, "separator")
file.child_append(sep)
item(file, "check", "Show _Toolbar", toggle_type="checkmark", toggle_state=1)
item(file, "off", "Disabled", enabled=False)
sub = item(file, "recent", "Recent", children_display="submenu")
item(sub, "doc", "doc.txt")
help_ = item(root, "help", "_Help", children_display="submenu")
item(help_, "about", "_About")
server.set_root(root)

bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
bus.call_sync("com.canonical.AppMenu.Registrar", "/com/canonical/AppMenu/Registrar", "com.canonical.AppMenu.Registrar",
              "RegisterWindow", GLib.Variant("(uo)", (a.window, PATH)), None, Gio.DBusCallFlags.NO_AUTO_START, 3000, None)
with open(a.ready, "w") as f:
    f.write(bus.get_unique_name())
GLib.MainLoop().run()
