#!/usr/bin/env python3
"""Real GTK icon consumer: the icon name is set once; settings changes must refresh it."""
import json,os,sys
from pathlib import Path
import gi
kit,out=sys.argv[1:3]
gi.require_version('Gtk','3.0' if kit=='gtk3' else '4.0')
gi.require_version('GdkX11','3.0' if kit=='gtk3' else '4.0')
from gi.repository import Gtk,GLib,GdkX11
out=Path(out)

def populate(win):
    win.set_title('Decebal icon probe '+kit)
    win.set_default_size(240,180)
    box=Gtk.Box(orientation=Gtk.Orientation.VERTICAL,spacing=12)
    for name in ('top','bottom','start','end'):getattr(box,'set_margin_'+name)(24)
    box.set_halign(Gtk.Align.CENTER);box.set_valign(Gtk.Align.CENTER)
    image=Gtk.Image.new_from_icon_name('decebal-probe',Gtk.IconSize.DIALOG) if kit=='gtk3' else Gtk.Image.new_from_icon_name('decebal-probe')
    image.set_pixel_size(64)
    label=Gtk.Label(label='Icon settings probe')
    if kit=='gtk3':
        box.pack_start(image,True,True,0);box.pack_start(label,False,False,0);win.add(box);win.show_all()
    else:
        box.append(image);box.append(label);win.set_child(box);win.present()
    def report():
        surface=win.get_window() if kit=='gtk3' else win.get_surface()
        if surface is None:return True
        settings=Gtk.Settings.get_default()
        xid=GdkX11.X11Window.get_xid(surface) if kit=='gtk3' else GdkX11.X11Surface.get_xid(surface)
        d={'pid':os.getpid(),'xid':xid,'kit':kit}
        for key in ('gtk-icon-theme-name','gtk-theme-name','gtk-font-name'):
            d[key]=settings.get_property(key)
        temp=out.with_suffix('.tmp');temp.write_text(json.dumps(d));temp.replace(out)
        return True
    GLib.timeout_add(100,report)

if kit=='gtk3':
    win=Gtk.Window();win.connect('destroy',Gtk.main_quit);populate(win);Gtk.main()
else:
    app=Gtk.Application(application_id='org.decebal.PrivateIconProbe')
    def activate(app):
        win=Gtk.ApplicationWindow(application=app);populate(win)
    app.connect('activate',activate);app.run([])
