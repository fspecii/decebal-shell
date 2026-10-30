#!/usr/bin/env python3
"""Native popup startup/current-focus regression, with a private Xvfb and buses.

Usage: python3 tests/e2e/popup-focus.py /path/to/decdash /tmp/evidence
The PointerRoot -> popup startup transition failed before the current-focus fix.
Also verifies genuine focus loss and Escape still close the popup.
"""
import ctypes as C
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


class Focus(C.Structure):
    _fields_ = [("type", C.c_int), ("serial", C.c_ulong), ("send_event", C.c_int),
                ("display", C.c_void_p), ("window", C.c_ulong),
                ("mode", C.c_int), ("detail", C.c_int)]


class Event(C.Union):
    _fields_ = [("focus", Focus), ("pad", C.c_long * 24)]


def main():
    binary, evidence = (Path(p).resolve() for p in sys.argv[1:3])
    evidence.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="decebal-popup-focus-") as tmp:
        root = Path(tmp)
        for name in ("home", "config", "cache", "data", "state", "run", "bin", "themes"):
            (root / name).mkdir(mode=0o700)
        for name in ("curl", "nmcli", "pactl", "playerctl", "brightnessctl", "bluetoothctl"):
            path = root / "bin" / name
            path.write_text("#!/bin/sh\nexit 1\n")
            path.chmod(0o755)
        (root / "config/decebal.json").write_text(json.dumps({"theme": {"name": "nord", "mode": "dark"}}))
        display = next(f":{n}" for n in range(460, 480) if not Path(f"/tmp/.X11-unix/X{n}").exists() and not Path(f"/tmp/.X{n}-lock").exists())
        env = dict(os.environ, HOME=str(root / "home"), DISPLAY=display,
                   XDG_CONFIG_HOME=str(root / "config"), XDG_CACHE_HOME=str(root / "cache"),
                   XDG_DATA_HOME=str(root / "data"), XDG_STATE_HOME=str(root / "state"),
                   XDG_RUNTIME_DIR=str(root / "run"), DECEBAL_CONFIG=str(root / "config"),
                   DECEBAL_THEMES=str(root / "themes"), DECEBAL_SYSTEM_THEMES=str(root / "themes"),
                   DBUS_SESSION_BUS_ADDRESS=f"unix:path={tmp}/no-session",
                   DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={tmp}/no-system",
                   DECEBAL_SYSTEM_AUTOMATIC="0", DECEBAL_BACKEND="x11",
                   DECEBAL_DEBUG_POPUP_EVENTS="1", PATH=f'{root / "bin"}:{os.environ["PATH"]}')
        for key in ("WAYLAND_DISPLAY", "RDWM_SOCK", "PULSE_SERVER"):
            env.pop(key, None)
        x = C.CDLL("libX11.so.6")
        x.XOpenDisplay.argtypes, x.XOpenDisplay.restype = [C.c_char_p], C.c_void_p
        x.XDefaultRootWindow.argtypes, x.XDefaultRootWindow.restype = [C.c_void_p], C.c_ulong
        x.XCreateSimpleWindow.argtypes = [C.c_void_p, C.c_ulong, C.c_int, C.c_int, C.c_uint, C.c_uint, C.c_uint, C.c_ulong, C.c_ulong]
        x.XCreateSimpleWindow.restype = C.c_ulong
        x.XMapWindow.argtypes = [C.c_void_p, C.c_ulong]
        x.XSetInputFocus.argtypes = [C.c_void_p, C.c_ulong, C.c_int, C.c_ulong]
        x.XGetInputFocus.argtypes = [C.c_void_p, C.POINTER(C.c_ulong), C.POINTER(C.c_int)]
        x.XSendEvent.argtypes = [C.c_void_p, C.c_ulong, C.c_int, C.c_long, C.POINTER(Event)]
        x.XSync.argtypes = [C.c_void_p, C.c_int]
        x.XCloseDisplay.argtypes = [C.c_void_p]
        child = None
        with (evidence / "popup-focus.log").open("w") as log:
            server = subprocess.Popen(["Xvfb", display, "-screen", "0", "960x720x24", "-nolisten", "tcp"], env=env, stdout=log, stderr=log)
            connection = None
            try:
                for _ in range(100):
                    assert server.poll() is None, "private Xvfb exited"
                    lock = Path(f"/tmp/.X{display[1:]}-lock")
                    if lock.exists() and int(lock.read_text().strip()) == server.pid and Path(f"/tmp/.X11-unix/X{display[1:]}").exists():
                        connection = x.XOpenDisplay(display.encode())
                        if connection:
                            break
                    time.sleep(.05)
                assert connection, "private Xvfb unavailable"
                # Explicitly use PointerRoot, so mapping the popup queues the
                # same startup focus transition even when the pointer is inside.
                x.XSetInputFocus(connection, 1, 1, 0)
                x.XSync(connection, 0)
                child = subprocess.Popen([str(binary), "--top", "48"], env=env, stdout=log, stderr=log)
                window = None
                for _ in range(100):
                    result = subprocess.run(["xdotool", "search", "--class", "^decdash$"], env=env, text=True, capture_output=True)
                    if result.returncode == 0 and result.stdout.strip():
                        window = int(result.stdout.splitlines()[0])
                        break
                    time.sleep(.05)
                (evidence / "startup.json").write_text(json.dumps({"window": window, "process_returncode": child.poll(), "binary": str(binary)}, indent=2))
                assert window, "popup did not map"
                time.sleep(1)
                assert child.poll() is None, "stale startup FocusOut closed popup"
                other = x.XCreateSimpleWindow(connection, x.XDefaultRootWindow(connection), 10, 600, 100, 50, 0, 0, 0)
                x.XMapWindow(connection, other)
                x.XSetInputFocus(connection, other, 1, 0)
                x.XSync(connection, 0)
                current, revert = C.c_ulong(), C.c_int()
                x.XGetInputFocus(connection, C.byref(current), C.byref(revert))
                assert current.value == other
                # A grab can report WhileGrabbed; explicitly deliver a normal
                # notification while server focus really belongs elsewhere.
                event = Event()
                event.focus = Focus(10, 0, 1, connection, window, 0, 3)
                x.XSendEvent(connection, window, 0, 1 << 21, C.byref(event))
                x.XSync(connection, 0)
                assert child.wait(timeout=5) == 0, "genuine focus loss did not close"
                child = subprocess.Popen([str(binary), "--top", "48"], env=env, stdout=log, stderr=log)
                time.sleep(1)
                assert child.poll() is None
                subprocess.run(["xdotool", "key", "Escape"], env=env, check=True)
                assert child.wait(timeout=5) == 0, "Escape did not close"
                (evidence / "checks.json").write_text(json.dumps({"startup_stays_open": True, "foreign_focus_closes": True, "escape_closes": True}, indent=2))
            finally:
                if child is not None and child.poll() is None:
                    child.terminate()
                    child.wait(timeout=5)
                if connection:
                    x.XCloseDisplay(connection)
                server.terminate()
                server.wait(timeout=5)
    print(f"PASS native popup stale startup, genuine focus loss and Escape: {evidence}")


if __name__ == "__main__":
    main()
