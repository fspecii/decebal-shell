#!/usr/bin/env python3
"""Private Xvfb actual native host smoke at 100/200% on a short display.
Usage: python3 tests/e2e/dux-widgets-native.py /absolute/path/to/decwidgets /tmp/evidence
No live display, config, bus, or application is used. Allowlisted app opens are
captured by private sibling fixtures; those fixtures do not expand widget trust.
"""
import json
import math
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

binary = Path(sys.argv[1]).resolve()
evidence = Path(sys.argv[2]).resolve()
evidence.mkdir(parents=True, exist_ok=True)


def wait(predicate, label):
    deadline = time.monotonic() + 8
    while time.monotonic() < deadline:
        result = predicate()
        if result:
            return result
        time.sleep(.05)
    raise AssertionError(label)


with tempfile.TemporaryDirectory(prefix="dux-native-") as private:
    root = Path(private)
    for name in ["bin", "home", "config/omarchy", "data", "state", "cache", "run", "themes"]:
        (root / name).mkdir(parents=True)
    (root / "run").chmod(0o700)
    shutil.copy2(binary, root / "bin/decwidgets")
    calls = root / "calls"
    for app in ["decdash", "dectimer", "decnotes"]:
        fake = root / "bin" / app
        fake.write_text('#!/bin/sh\nprintf "%s" "${0##*/}" >> "$DUX_CALLS"\nfor arg in "$@"; do printf " %s" "$arg" >> "$DUX_CALLS"; done\nprintf "\\n" >> "$DUX_CALLS"\n')
        fake.chmod(0o755)
    display = next(f":{i}" for i in range(289, 310) if not Path(f"/tmp/.X11-unix/X{i}").exists())
    env = dict(os.environ, HOME=str(root / "home"), DISPLAY=display, DECEBAL_BACKEND="x11", XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), XDG_STATE_HOME=str(root / "state"), XDG_CACHE_HOME=str(root / "cache"), XDG_RUNTIME_DIR=str(root / "run"), DECEBAL_CONFIG=str(root / "config/omarchy"), DECEBAL_THEMES=str(root / "themes"), DECEBAL_SYSTEM_THEMES=str(root / "themes"), DECEBAL_SYSTEM_AUTOMATIC="0", DBUS_SESSION_BUS_ADDRESS="unix:path=/nonexistent-dux-native-bus", DUX_CALLS=str(calls))
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("RDWM_SOCK", None)
    log = (evidence / "native-host.log").open("w")
    server = subprocess.Popen(["Xvfb", display, "-screen", "0", "640x480x24", "-nolisten", "tcp"], env=env, stdout=log, stderr=log)
    host = None
    try:
        def run(*args, check=True):
            return subprocess.run(args, env=env, text=True, capture_output=True, check=check).stdout.strip()
        wait(lambda: run("xdotool", "getdisplaygeometry", check=False), "private Xvfb ready")
        checks = []
        for scale in [1, 2]:
            env["DECEBAL_FONT_SCALE"] = str(scale)
            for widget, height in [("resource-overview", 248 if scale == 1 else 324), ("focus-hub", 292 if scale == 1 else 363)]:
                config = {"theme": {"name": "nord", "mode": "dark"}, "widgets": {"enabled": True, "allow_exec": False, "items": [{"use": f"builtin:{widget}", "enabled": True, "anchor": "top-left", "x": 20, "y": 20}]}}
                (root / "config/omarchy/decebal.json").write_text(json.dumps(config))
                host = subprocess.Popen([str(root / "bin/decwidgets"), "daemon"], env=env, stdout=log, stderr=log)
                wid = wait(lambda: run("xdotool", "search", "--class", "^decwidgets$", check=False), "native widget mapped").splitlines()[0]
                wait(lambda: json.loads(run(str(root / "bin/decwidgets"), "status", "--json"))["running"], "painted readiness")
                geom = dict(line.split("=", 1) for line in run("xdotool", "getwindowgeometry", "--shell", wid).splitlines() if "=" in line)
                assert int(geom["WIDTH"]) == 320 and int(geom["HEIGHT"]) == height, geom
                assert int(geom["Y"]) >= 0 and int(geom["Y"]) + height <= 480, geom
                run("import", "-window", "root", str(evidence / f"{widget}-{scale * 100}-short.png"))
                calls.write_text("")
                def click(x, y, expected):
                    run("xdotool", "mousemove", "--window", wid, str(x), str(y), "click", "1")
                    wait(lambda: expected in calls.read_text().splitlines(), f"allowlisted {expected}")
                if widget == "resource-overview":
                    click(100, 60, "decdash")
                else:
                    lh = lambda size: math.ceil(size * scale * 1.25)
                    panel_y = 18 + lh(11) + 10 + lh(28) + 10 + lh(13) + 10 + 4 + 10
                    panel_h = 24 + 8 + lh(10) + lh(14)
                    buttons_y = panel_y + panel_h + 10 + 10 + lh(12) // 2
                    click(70, buttons_y, "dectimer")
                    click(230, buttons_y, "decnotes --new")
                    click(100, panel_y + panel_h // 2, "decnotes")
                checks.append({"widget": widget, "scale": scale, "geometry": geom, "calls": calls.read_text().splitlines()})
                host.terminate()
                host.wait(timeout=5)
                host = None
                wait(lambda: not run("xdotool", "search", "--class", "^decwidgets$", check=False), "native windows gone")
        (evidence / "checks.json").write_text(json.dumps(checks, indent=2))
        print("PASS four actual native windows at100/200% on640x480; eight safe app clicks; all private processes cleaned")
    finally:
        if host is not None:
            host.terminate()
            host.wait(timeout=5)
        server.terminate()
        server.wait(timeout=5)
        log.close()
