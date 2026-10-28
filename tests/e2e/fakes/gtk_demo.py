#!/usr/bin/env python3
"""A small GTK window used by tests/e2e/system-theme.sh to see the generated gtk.css at work.

usage: gtk_demo.py 4|3 [title]   (4 = GTK 4 + libadwaita, 3 = plain GTK 3)
Shows a header bar, a sidebar, a list, buttons (suggested / destructive), an entry, a switch and a progress bar.
"""
import sys

ver = sys.argv[1] if len(sys.argv) > 1 else "4"
title = sys.argv[2] if len(sys.argv) > 2 else "decebal GTK demo"
import gi

if ver == "4":
    gi.require_version("Gtk", "4.0")
    gi.require_version("Adw", "1")
    from gi.repository import Adw, Gtk

    class App(Adw.Application):
        def __init__(self):
            super().__init__(application_id="org.decebal.GtkDemo")

        def do_activate(self):
            win = Adw.ApplicationWindow(application=self, title=title, default_width=820, default_height=520)
            side = Gtk.ListBox(css_classes=["navigation-sidebar"])
            for n in ("Recent", "Home", "Documents", "Downloads", "Trash"):
                side.append(Gtk.Label(label=n, xalign=0, margin_top=8, margin_bottom=8, margin_start=12))
            side.select_row(side.get_row_at_index(1))
            sidebar = Adw.ToolbarView()
            sidebar.add_top_bar(Adw.HeaderBar(title_widget=Gtk.Label(label="Places")))
            sidebar.set_content(side)
            box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12, margin_top=18, margin_start=18, margin_end=18)
            box.append(Gtk.Label(label="Window text on the window background", xalign=0))
            row = Gtk.Box(spacing=8)
            for label, cls in (("Suggested", "suggested-action"), ("Destructive", "destructive-action"), ("Normal", None)):
                b = Gtk.Button(label=label)
                if cls:
                    b.add_css_class(cls)
                row.append(b)
            row.append(Gtk.Switch(active=True, valign=Gtk.Align.CENTER))
            box.append(row)
            box.append(Gtk.Entry(text="An entry (view colours)"))
            box.append(Gtk.ProgressBar(fraction=0.6))
            card = Gtk.Box(css_classes=["card"], margin_top=6)
            card.append(Gtk.Label(label="A card", margin_top=14, margin_bottom=14, margin_start=14))
            box.append(card)
            lst = Gtk.ListBox(css_classes=["boxed-list"])
            for n in ("alpha.txt", "beta.png", "gamma.pdf"):
                lst.append(Adw.ActionRow(title=n, subtitle="boxed list row"))
            box.append(lst)
            content = Adw.ToolbarView()
            content.add_top_bar(Adw.HeaderBar(title_widget=Gtk.Label(label=title)))
            content.set_content(box)
            split = Adw.OverlaySplitView(sidebar=sidebar, content=content, show_sidebar=True, sidebar_width_fraction=0.28)
            win.set_content(split)
            win.present()

    App().run(None)
else:
    gi.require_version("Gtk", "3.0")
    from gi.repository import Gtk

    win = Gtk.Window(title=title, default_width=620, default_height=380)
    hb = Gtk.HeaderBar(title=title, show_close_button=True)
    win.set_titlebar(hb)
    box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10, margin=16)
    box.add(Gtk.Label(label="GTK 3 window text", xalign=0))
    row = Gtk.Box(spacing=8)
    b = Gtk.Button(label="Suggested")
    b.get_style_context().add_class("suggested-action")
    row.add(b)
    d = Gtk.Button(label="Destructive")
    d.get_style_context().add_class("destructive-action")
    row.add(d)
    row.add(Gtk.Button(label="Normal"))
    box.add(row)
    box.add(Gtk.Entry(text="An entry"))
    box.add(Gtk.ProgressBar(fraction=0.6))
    win.add(box)
    win.connect("destroy", Gtk.main_quit)
    win.show_all()
    Gtk.main()
