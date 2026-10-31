#!/usr/bin/env python3
"""A small real GTK 3 / GTK 4 / libadwaita client for the GTK theming harness (not a test by itself).

    gtk_probe.py gtk3|gtk4|adw TITLE OUT.json

It builds the same control set in each toolkit (header bar and header button, sidebar with a selected row, content view,
entry, normal / suggested / destructive / insensitive buttons, check, switch, scale, body and dim labels, a card) and,
once the window is drawn, writes OUT.json: the X11 window id, the scale factor and each control's box in surface-local
logical pixels. Clicks are appended to OUT.json's sibling `.events` file so a harness can prove real actions arrived.
It reads no theme of its own: everything it shows comes from the toolkit, settings.ini, gsettings and the user gtk.css.
"""
import json
import os
import sys

kit, title, out = sys.argv[1], sys.argv[2], sys.argv[3]
import gi

if kit == "gtk3":
    gi.require_version("Gtk", "3.0")
    gi.require_version("GdkX11", "3.0")
else:
    gi.require_version("Gtk", "4.0")
    gi.require_version("GdkX11", "4.0")
    if kit == "adw":
        gi.require_version("Adw", "1")
from gi.repository import GLib, Gtk  # noqa: E402

if kit == "adw":
    from gi.repository import Adw  # noqa: E402

events = out + ".events"
named = {}


def log(name):
    with open(events, "a") as f:
        f.write(name + "\n")


def keep(name, w):
    named[name] = w
    if hasattr(w, "set_name"):
        w.set_name("probe-" + name)
    return w


def button(name, label, cls=None, sensitive=True):
    b = keep(name, Gtk.Button(label=label))
    if cls:
        b.get_style_context().add_class(cls) if kit == "gtk3" else b.add_css_class(cls)
    b.set_sensitive(sensitive)
    b.connect("clicked", lambda *_: log("clicked:" + name))
    return b


def add(box, child):
    box.pack_start(child, False, False, 0) if kit == "gtk3" else box.append(child)


def content():
    page = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
    for m in ("set_margin_top", "set_margin_bottom", "set_margin_start", "set_margin_end"):
        getattr(page, m)(14)
    add(page, keep("label", Gtk.Label(label="Readable body text in the window colour")))
    dim = keep("dim", Gtk.Label(label="Secondary text for hints and subtitles"))
    dim.get_style_context().add_class("dim-label") if kit == "gtk3" else dim.add_css_class("dim-label")
    add(page, dim)
    entry = keep("entry", Gtk.Entry())
    entry.set_text("Editable entry text")
    add(page, entry)
    row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
    for b in (button("btn", "Normal button"), button("btn_suggested", "Suggested", "suggested-action"), button("btn_destructive", "Delete", "destructive-action"), button("btn_disabled", "Disabled", sensitive=False)):
        add(row, b)
    add(page, row)
    toggles = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=16)
    check = keep("check", Gtk.CheckButton(label="Check option"))
    check.set_active(True)
    check.connect("toggled", lambda w: log("toggled:check:%d" % w.get_active()))
    add(toggles, check)
    sw = keep("switch", Gtk.Switch())
    sw.set_active(True)
    sw.connect("notify::active", lambda w, _p: log("toggled:switch:%d" % w.get_active()))
    add(toggles, sw)
    add(page, toggles)
    scale = keep("scale", Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, 0, 100, 1))
    scale.set_value(60)
    scale.set_draw_value(False)
    add(page, scale)
    view = keep("view", Gtk.TextView())
    view.get_buffer().set_text("Content view text.\nSelection and caret use the accent.")
    view.set_size_request(-1, 90)
    add(page, view)
    if kit == "adw":
        group = Adw.PreferencesGroup()
        card = keep("card", Adw.ActionRow(title="Card row", subtitle="libadwaita boxed list"))
        group.add(card)
        add(page, group)
    return page


def sidebar():
    lb = keep("sidebar", Gtk.ListBox())
    for i, text in enumerate(["Selected place", "Documents", "Downloads", "Pictures"]):
        lab = Gtk.Label(label=text, xalign=0)
        for m in ("set_margin_top", "set_margin_bottom", "set_margin_start", "set_margin_end"):
            getattr(lab, m)(8)
        if kit == "gtk3":
            r = Gtk.ListBoxRow()
            r.add(lab)
            lb.add(r)
        else:
            lb.append(lab)
        if i == 0:
            keep("sidebar_selected", lb.get_row_at_index(0))
        if i == 2:
            keep("sidebar_row", lb.get_row_at_index(2))
    lb.select_row(lb.get_row_at_index(0))
    lb.set_size_request(200, -1)
    if kit == "gtk3":
        lb.get_style_context().add_class("sidebar")
    else:
        lb.add_css_class("navigation-sidebar")
    return lb


def build(app=None):
    if kit == "adw":
        win = Adw.ApplicationWindow(application=app, title=title)
        split = Adw.NavigationSplitView()
        side_view = Adw.ToolbarView()
        side_view.add_top_bar(Adw.HeaderBar())
        side_view.set_content(sidebar())
        split.set_sidebar(Adw.NavigationPage(child=side_view, title="Places"))
        main = Adw.ToolbarView()
        hb = keep("headerbar", Adw.HeaderBar())
        hb.pack_end(button("header_btn", "Header"))
        main.add_top_bar(hb)
        main.set_content(content())
        split.set_content(Adw.NavigationPage(child=main, title=title))
        win.set_content(split)
    else:
        win = Gtk.ApplicationWindow(application=app, title=title) if app else Gtk.Window(title=title)
        hb = keep("headerbar", Gtk.HeaderBar())
        hb.pack_end(button("header_btn", "Header"))
        if kit == "gtk3":
            hb.set_show_close_button(True)
            hb.set_title(title)
        win.set_titlebar(hb)
        paned = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        add(paned, sidebar())
        sep = Gtk.Separator(orientation=Gtk.Orientation.VERTICAL)
        add(paned, sep)
        c = content()
        c.set_hexpand(True)
        add(paned, c)
        win.add(paned) if kit == "gtk3" else win.set_child(paned)
    keep("window", win)
    win.set_default_size(920, 640)
    return win


def boxes(win):
    out_boxes = {}
    if kit == "gtk3":
        from gi.repository import GdkX11

        top = win.get_window()
        _, ox, oy = top.get_origin()
        for name, w in named.items():
            if w is win:
                a = w.get_allocation()
                out_boxes[name] = [0, 0, top.get_width(), top.get_height()]
                continue
            gw = Gtk.Widget.get_window(w)  # TextView overrides get_window(type)
            _, wx, wy = gw.get_origin()
            a = w.get_allocation()
            x, y = (wx - ox + a.x, wy - oy + a.y) if not w.get_has_window() else (wx - ox, wy - oy)
            out_boxes[name] = [x, y, a.width, a.height]
        return GdkX11.X11Window.get_xid(top), top.get_scale_factor(), out_boxes
    from gi.repository import GdkX11

    surface = win.get_surface()
    tx, ty = win.get_surface_transform()
    for name, w in named.items():
        ok, r = w.compute_bounds(win)
        if ok:
            out_boxes[name] = [r.get_x() + tx, r.get_y() + ty, r.get_width(), r.get_height()]
    return GdkX11.X11Surface.get_xid(surface), surface.get_scale_factor(), out_boxes


def report(win):
    xid, scale, b = boxes(win)
    settings = Gtk.Settings.get_default()
    info = {"kit": kit, "xid": xid, "scale": scale, "boxes": b, "pid": os.getpid()}
    for key in ("gtk-theme-name", "gtk-icon-theme-name", "gtk-font-name", "gtk-decoration-layout", "gtk-application-prefer-dark-theme"):
        try:
            info[key] = settings.get_property(key)
        except TypeError:
            pass
    if kit != "gtk3":
        try:
            info["gtk-interface-color-scheme"] = int(settings.get_property("gtk-interface-color-scheme"))
        except TypeError:
            pass
    if kit == "adw":
        sm = Adw.StyleManager.get_default()
        info["adw_dark"] = sm.get_dark()
        info["adw_accent"] = sm.get_accent_color_rgba().to_string()
    tmp = out + ".tmp"
    with open(tmp, "w") as f:
        json.dump(info, f)
    os.replace(tmp, out)
    return False


def shown(win):
    # Two seconds lets fonts, icons and the first frames settle; re-report on demand (SIGUSR1) after scale/theme changes.
    GLib.timeout_add(1500, report, win)
    import signal

    try:
        from gi.repository import GLibUnix

        GLibUnix.signal_add(GLib.PRIORITY_DEFAULT, signal.SIGUSR1, lambda: report(win) or True)
    except ImportError:
        GLib.unix_signal_add(GLib.PRIORITY_DEFAULT, signal.SIGUSR1, lambda: report(win) or True)


if kit == "gtk3":
    w = build()
    w.connect("destroy", Gtk.main_quit)
    w.show_all()
    shown(w)
    Gtk.main()
else:
    App = Adw.Application if kit == "adw" else Gtk.Application
    try:
        from gi.repository import Gio

        flags = Gio.ApplicationFlags.NON_UNIQUE
    except Exception:
        flags = 0
    app = App(application_id="org.decebal.GtkProbe" + kit.capitalize(), flags=flags)

    def activate(a):
        w = build(a)
        w.present()
        shown(w)

    app.connect("activate", activate)
    app.run([])
