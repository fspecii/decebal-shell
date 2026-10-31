#!/usr/bin/env python3
"""The one EWMH hint GTK needs for :backdrop, on a private X display without a window manager (harness helper, not a test).

GDK treats every toplevel as focused unless the window manager lists `_NET_WM_STATE_FOCUSED` in `_NET_SUPPORTED` (and proves it
is running with `_NET_SUPPORTING_WM_CHECK`); only then does it follow `_NET_WM_STATE_FOCUSED` on the window to switch :backdrop.
This helper claims just that on the display in $DISPLAY (which the harness has proved it owns) and then reads commands on stdin:

    focus XID   sets _NET_WM_STATE_FOCUSED on XID (and clears it on the previously focused window)
    blur XID    clears it on XID
It answers "ok" per command. It never manages, moves or maps client windows. This is a synthetic stand-in for a window manager:
it proves the GTK themes' :backdrop styles, never a real window manager's focus handling (rdwm's support is proved by rdwm).
"""
import sys

from Xlib import X, Xatom, display

d = display.Display()
root = d.screen().root
atom = lambda n: d.intern_atom(n)  # noqa: E731
STATE, FOCUSED, SUPPORTED, CHECK = atom("_NET_WM_STATE"), atom("_NET_WM_STATE_FOCUSED"), atom("_NET_SUPPORTED"), atom("_NET_SUPPORTING_WM_CHECK")
check = root.create_window(-10, -10, 1, 1, 0, X.CopyFromParent)
check.change_property(CHECK, Xatom.WINDOW, 32, [check.id])
check.change_property(atom("_NET_WM_NAME"), atom("UTF8_STRING"), 8, b"decebal-ewmh-focus-fixture")
root.change_property(CHECK, Xatom.WINDOW, 32, [check.id])
root.change_property(SUPPORTED, Xatom.ATOM, 32, [STATE, FOCUSED])
d.sync()
print("ready", flush=True)
current = None


def set_state(xid, focused):
    w = d.create_resource_object("window", xid)
    w.change_property(STATE, Xatom.ATOM, 32, [FOCUSED] if focused else [])


for line in sys.stdin:
    parts = line.split()
    if len(parts) != 2:
        continue
    cmd, xid = parts[0], int(parts[1])
    if cmd == "focus":
        if current is not None and current != xid:
            set_state(current, False)
        set_state(xid, True)
        current = xid
    elif cmd == "blur":
        set_state(xid, False)
        if current == xid:
            current = None
    d.sync()
    print("ok", flush=True)
