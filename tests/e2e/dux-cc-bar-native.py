#!/usr/bin/env python3
"""Private native main-bar Dux CC clipping, scroll, focus-reveal and translated input.
Usage: python3 tests/e2e/dux-cc-bar-native.py BIN PACKAGE EVIDENCE
All apps/actions, HOME/XDG/buses and Xvfb are private; neither :0 nor :430 is used.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib
from PIL import Image, ImageChops


def wait(fn, label):
    for _ in range(120):
        try:
            value = fn()
            if value:
                return value
        except (OSError, ValueError, subprocess.CalledProcessError):
            pass
        time.sleep(.05)
    raise AssertionError(label)


def main():
    binary, package, evidence = (Path(p).resolve() for p in sys.argv[1:4])
    evidence.mkdir(parents=True, exist_ok=True)
    theme_id = tomllib.loads((package / "theme.toml").read_text())["id"]
    with tempfile.TemporaryDirectory(prefix="dux-cc-bar-") as private:
        root = Path(private)
        for name in ("bin", "home", "config", "cache", "data", "state", "run", "themes"):
            (root / name).mkdir(mode=0o700)
        shutil.copytree(package, root / "themes" / theme_id)
        shutil.copy2(binary, root / "bin/decbar")
        calls = root / "calls"
        calls.write_text("")
        fixture = root / "bin/decsettings"
        fixture.write_text('#!/bin/sh\nprintf "decsettings %s\\n" "$*" >> "$DUX_SETTINGS_CALLS"\n')
        fixture.chmod(0o755)
        for app in ("curl", "nmcli", "pactl", "wpctl", "playerctl", "bluetoothctl", "brightnessctl", "decutil", "rdwmctl"):
            path = root / "bin" / app
            path.write_text("#!/bin/sh\nexit 1\n")
            path.chmod(0o755)
        env = dict(os.environ, HOME=str(root / "home"), XDG_CONFIG_HOME=str(root / "config"), XDG_CACHE_HOME=str(root / "cache"), XDG_DATA_HOME=str(root / "data"), XDG_STATE_HOME=str(root / "state"), XDG_RUNTIME_DIR=str(root / "run"), DECEBAL_CONFIG=str(root / "config"), DECEBAL_THEMES=str(root / "themes"), DECEBAL_SYSTEM_THEMES=str(root / "themes"), DBUS_SESSION_BUS_ADDRESS=f"unix:path={private}/no-session", DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={private}/no-system", DECEBAL_SYSTEM_AUTOMATIC="0", DECEBAL_BACKEND="x11", DUX_SETTINGS_CALLS=str(calls), PATH=f'{root / "bin"}:{os.environ["PATH"]}')
        for key in ("WAYLAND_DISPLAY", "RDWM_SOCK", "PULSE_SERVER", "DBUS_STARTER_ADDRESS", "DBUS_STARTER_BUS_TYPE"):
            env.pop(key, None)
        ledger = []
        with (evidence / "native.log").open("w") as log:
            for width, screen_height in ((1920, 1080), (960, 720)):
                display = next(f":{n}" for n in range(380, 420) if not Path(f"/tmp/.X11-unix/X{n}").exists())
                env["DISPLAY"] = display
                server = subprocess.Popen(["Xvfb", display, "-screen", "0", f"{width}x{screen_height}x24", "-nolisten", "tcp"], env=env, stdout=log, stderr=log)
                bar = None
                try:
                    def run(*args, check=True):
                        return subprocess.run(args, env=env, check=check, text=True, capture_output=True).stdout.strip()
                    wait(lambda: run("xdotool", "getdisplaygeometry", check=False), "private display ready")
                    for scale in (1, 2):
                        env["DECEBAL_FONT_SCALE"] = str(scale)
                        for mode in ("dark", "light"):
                            config = {"theme": {"name": theme_id, "mode": mode}, "control_center": {"style": "dux"}, "bar": {"left": ["launcherButton"], "center": ["clock"], "right": ["controlCenterButton"], "margin": 4}}
                            (root / "config/decebal.json").write_text(json.dumps(config))
                            tray = root / "tray"
                            tray.unlink(missing_ok=True)
                            env["DECBAR_DEBUG_TRAY"] = str(tray)
                            bar = subprocess.Popen([str(root / "bin/decbar"), "--app", "Main CC fixture"], env=env, stdout=log, stderr=log)
                            window = wait(lambda: run("xdotool", "search", "--name", "^decebal-bar$", check=False), "main bar mapped").splitlines()[0]
                            wait(lambda: tray.exists() and "Control " in tray.read_text(), "main bar painted")
                            cx = float(next(line.split()[1] for line in tray.read_text().splitlines() if line.startswith("Control ")))
                            run("xdotool", "mousemove", str(round(cx)), "23", "click", "1")
                            time.sleep(.7)
                            # Shared source placement: Control's right edge anchors a 600px panel.
                            ox, oy = cx + 14 - 600, 50
                            geo = dict(line.split("=", 1) for line in run("xdotool", "getwindowgeometry", "--shell", window).splitlines() if "=" in line)
                            assert int(geo["HEIGHT"]) <= screen_height, geo
                            def capture(suffix):
                                path = evidence / f"main-cc-{width}-{scale * 100}-{mode}-{suffix}.png"
                                run("import", "-window", "root", str(path))
                                return Image.open(path).convert("RGB")
                            before = capture("top")
                            run("xdotool", "mousemove", str(round(ox + 594)), str(oy + 200), "click", "--repeat", "40", "--delay", "10", "5")
                            time.sleep(.2)
                            bottom = capture("bottom")
                            row = 64 + (scale - 1) * 36
                            # At the end of scrolling the footer remains 16px above content bottom.
                            # Its Settings action verifies pointer Y was translated to canonical content.
                            if screen_height == 720 and scale == 2:
                                assert ImageChops.difference(before.crop((round(ox), oy, width - 8, screen_height - 8)), bottom.crop((round(ox), oy, width - 8, screen_height - 8))).getbbox(), "wheel failed to change clipped content"
                                previous = len(calls.read_text().splitlines())
                                run("xdotool", "mousemove", str(round(ox + 372)), str(screen_height - 8 - 16 - row // 2), "click", "1")
                                wait(lambda: len(calls.read_text().splitlines()) > previous, "translated footer Settings target")
                                run("xdotool", "mousemove", str(round(cx)), "23", "click", "1")
                                time.sleep(.4)
                                run("xdotool", "mousemove", str(round(ox + 594)), str(oy + 200), "click", "--repeat", "40", "--delay", "10", "5")
                            previous = len(calls.read_text().splitlines())
                            keyboard_bottom = capture("keyboard-bottom")
                            # First stable target is header Settings. Down reveals it from the
                            # scrolled bottom and Enter uses its identity, not a stale rectangle.
                            run("xdotool", "key", "--window", window, "Down")
                            time.sleep(.2)
                            keyboard_reveal = capture("keyboard-reveal")
                            if screen_height == 720 and scale == 2:
                                assert ImageChops.difference(keyboard_bottom.crop((round(ox), oy, width - 8, screen_height - 8)), keyboard_reveal.crop((round(ox), oy, width - 8, screen_height - 8))).getbbox(), "keyboard did not reveal scrolled target"
                            run("xdotool", "key", "--window", window, "Return")
                            wait(lambda: len(calls.read_text().splitlines()) > previous, "stable keyboard Settings action")
                            assert calls.read_text().splitlines()[-1].startswith("decsettings"), calls.read_text()
                            ledger.append({"screen": [width, screen_height], "scale": scale, "mode": mode, "origin": [ox, oy], "window": geo, "settings_calls": len(calls.read_text().splitlines())})
                            bar.terminate()
                            bar.wait(timeout=10)
                            bar = None
                finally:
                    if bar is not None:
                        bar.terminate()
                        bar.wait(timeout=10)
                    server.terminate()
                    server.wait(timeout=10)
        (evidence / "checks.json").write_text(json.dumps(ledger, indent=2))
    print(f"PASS main-bar Dux CC clipped native input/focus at1920/960,100/200,dark/light: {evidence}")


if __name__ == "__main__":
    main()
