#!/usr/bin/env python3
"""Native Dux bar input/pixels on a private Xvfb; no live sessions or real app actions.
Usage: python3 tests/e2e/dux-bar-native.py BIN THEMES_DIR THEME_ID EVIDENCE_DIR
The supplied package must declare bar.style=dux and real native font.files.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

from PIL import Image


def wait(predicate, label):
    for _ in range(120):
        try:
            result = predicate()
            if result:
                return result
        except (OSError, ValueError, subprocess.CalledProcessError):
            pass
        time.sleep(0.05)
    raise AssertionError(label)


def main():
    binary, themes = (Path(p).resolve() for p in sys.argv[1:3])
    theme_id = sys.argv[3]
    evidence = Path(sys.argv[4]).resolve()
    evidence.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="dux-bar-native-") as private:
        root = Path(private)
        for name in ["home", "config", "cache", "state", "data", "run", "bin", "empty-themes"]:
            (root / name).mkdir(mode=0o700)
        shutil.copy2(binary, root / "bin/decbar")
        calls = root / "calls"
        calls.write_text("")
        for app in ["declaunch", "decnotify", "decclip", "decdash", "decsession"]:
            path = root / "bin" / app
            path.write_text('#!/bin/sh\nprintf "%s" "${0##*/}" >> "$DUX_CALLS"\nfor arg in "$@"; do printf " %s" "$arg" >> "$DUX_CALLS"; done\nprintf "\\n" >> "$DUX_CALLS"\n')
            path.chmod(0o755)
        # Block external probes, including weather network access. Cached/missing state is intentional.
        for app in ["curl", "nmcli", "pactl", "wpctl", "playerctl", "bluetoothctl", "brightnessctl", "decutil", "rdwmctl"]:
            path = root / "bin" / app
            path.write_text("#!/bin/sh\nexit 1\n")
            path.chmod(0o755)
        display = next(f":{n}" for n in range(310, 350) if not Path(f"/tmp/.X11-unix/X{n}").exists() and not Path(f"/tmp/.X{n}-lock").exists())
        env = dict(os.environ, HOME=str(root / "home"), XDG_CONFIG_HOME=str(root / "config"), XDG_CACHE_HOME=str(root / "cache"), XDG_STATE_HOME=str(root / "state"), XDG_DATA_HOME=str(root / "data"), XDG_RUNTIME_DIR=str(root / "run"), DECEBAL_CONFIG=str(root / "config"), DECEBAL_THEMES=str(themes), DECEBAL_SYSTEM_THEMES=str(root / "empty-themes"), DISPLAY=display, DECEBAL_BACKEND="x11", DECEBAL_SYSTEM_AUTOMATIC="0", DBUS_SESSION_BUS_ADDRESS=f"unix:path={private}/no-bus", DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={private}/no-system", RDWMCTL=str(root / "bin/rdwmctl"), DUX_CALLS=str(calls), PATH=f'{root / "bin"}:{os.environ["PATH"]}')
        for key in ["WAYLAND_DISPLAY", "RDWM_SOCK", "PULSE_SERVER", "DBUS_STARTER_ADDRESS", "DBUS_STARTER_BUS_TYPE"]:
            env.pop(key, None)
        with (evidence / "native-bar.log").open("w") as log:
            server = subprocess.Popen(["Xvfb", display, "-screen", "0", "1920x1080x24", "-nolisten", "tcp"], env=env, stdout=log, stderr=log)
            bar = None
            try:
                def run(*args, check=True):
                    assert server.poll() is None, "owned private Xvfb exited"
                    return subprocess.run(args, env=env, text=True, capture_output=True, check=check).stdout.strip()
                wait(lambda: server.poll() is None and Path(f"/tmp/.X{display[1:]}-lock").exists() and int(Path(f"/tmp/.X{display[1:]}-lock").read_text().strip()) == server.pid, "owned private Xvfb")
                wait(lambda: run("xdotool", "getdisplaygeometry", check=False), "private Xvfb ready")
                results = []
                for scale, background, mode in [(1,"band","dark"),(2,"band","dark"),(1,"band","light"),(2,"band","light"),(1,"islands","dark"),(2,"islands","dark")]:
                    calls.write_text("")
                    env["DECEBAL_FONT_SCALE"] = str(scale)
                    debug = root / f"tray-{scale}-{background}-{mode}.txt"
                    env["DECBAR_DEBUG_TRAY"] = str(debug)
                    config = {"theme": {"name": theme_id, "mode": mode}, "bar": {"background": background, "margin": 4, "spacing": 4, "radius": 14, "left": ["launcherButton"], "center": ["clock"], "right": ["notificationButton", "controlCenterButton"]}}
                    (root / "config/decebal.json").write_text(json.dumps(config))
                    bar = subprocess.Popen([str(root / "bin/decbar"), "--app", "Native fixture"], env=env, stdout=log, stderr=log)
                    window = wait(lambda: run("xdotool", "search", "--name", "decebal-bar", check=False) or run("xdotool", "search", "--class", "decbar", check=False), "native bar mapped").splitlines()[0]
                    wait(lambda: debug.exists() and "Logo" in debug.read_text() and "Control" in debug.read_text(), "Dux declarative groups painted")
                    time.sleep(0.2)
                    shot = evidence / f"bar-{background}-{mode}-{scale * 100}-native.png"
                    run("import", "-window", "root", str(shot))
                    image = Image.open(shot).convert("RGB")
                    assert image.getpixel((0, 0)) == (0, 0, 0), "bar paints unused canvas"
                    centers = dict(line.split(" ", 1) for line in debug.read_text().splitlines())
                    y = 23
                    run("xdotool", "mousemove", str(round(float(centers["Logo"]))), str(y), "click", "1")
                    wait(lambda: "declaunch" in calls.read_text().splitlines(), "native launcher action")
                    # Notification sits immediately before the Control item in this explicit list.
                    notif = round(float(centers["Control"])) - 32
                    run("xdotool", "mousemove", str(notif), str(y), "click", "1")
                    wait(lambda: "decnotify history" in calls.read_text().splitlines(), "native notifications action")
                    # True gap between islands remains click-through (the X shape excludes it).
                    gap = image.getpixel((128, y))
                    assert (gap == (0, 0, 0)) == (background == "islands"), (background, gap)
                    results.append({"background": background, "scale": scale, "calls": calls.read_text().splitlines(), "tray": centers, "gap": [128, y]})
                    if background == "band":
                        # Capture the canonical configured item lists, separately from the
                        # deliberately reduced interaction fixture above.
                        config["bar"] = {"background":"band","margin":4,"spacing":4,"radius":14,"band_radius":16}
                        (root / "config/decebal.json").write_text(json.dumps(config))
                        wait(lambda: "Weather" in debug.read_text(), "canonical default layout refreshed")
                        time.sleep(0.2)
                        full = evidence / f"native-{mode}-1920x1080-{scale * 100}.png"
                        run("import", "-window", "root", str(full))
                        results[-1].update(mode=mode, viewport=[1920,1080], canonical_capture=full.name,
                            fixture="Private Xvfb, no WM/MPRIS/weather backend; native host CPU/RAM sampler is real. Clock is private current time.")
                    bar.terminate()
                    bar.wait(timeout=10)
                    bar = None
                    wait(lambda: not run("xdotool", "search", "--class", "decbar", check=False), "native surface cleaned")
                (evidence / "native-checks.json").write_text(json.dumps(results, indent=2))
            finally:
                if bar is not None:
                    bar.terminate()
                    bar.wait(timeout=10)
                server.terminate()
                server.wait(timeout=10)
    print(f"PASS native Dux band/islands at100/200%, launcher/notifications actions and correct gap pixels: {evidence}")


if __name__ == "__main__":
    main()
