#!/usr/bin/env python3
"""Isolated native Settings captures at UI scales 1/2 and short heights.

SETTINGS_BIN=/path/to/decsettings python3 tests/e2e/settings-dux-native.py
Requires Xvfb, xdotool, ImageMagick and Pillow; never uses the desktop bus/WM.
"""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

from PIL import Image, ImageStat


def run(*args, env):
    return subprocess.check_output(args, env=env, text=True).strip()


def wait_for(fn):
    for _ in range(100):
        try:
            value = fn()
            if value:
                return value
        except (OSError, ValueError, subprocess.CalledProcessError):
            pass
        time.sleep(0.1)
    raise AssertionError("native window/layout did not become ready")


def main():
    binary = Path(os.environ.get("SETTINGS_BIN", "target/debug/decsettings")).resolve()
    evidence = Path(os.environ.get("SETTINGS_EVIDENCE_DIR", "/tmp/decebal-dux-settings-evidence"))
    evidence.mkdir(parents=True, exist_ok=True)
    display = next(f":{n}" for n in range(260, 300) if not Path(f"/tmp/.X11-unix/X{n}").exists())
    with tempfile.TemporaryDirectory(prefix="dux-settings-") as tmp:
        root = Path(tmp)
        for name in ("home", "config", "cache", "data", "state", "run"):
            (root / name).mkdir(mode=0o700)
        env = dict(os.environ, HOME=str(root / "home"), DISPLAY=display,
                   XDG_CONFIG_HOME=str(root / "config"), XDG_CACHE_HOME=str(root / "cache"),
                   XDG_DATA_HOME=str(root / "data"), XDG_STATE_HOME=str(root / "state"),
                   XDG_RUNTIME_DIR=str(root / "run"), DECEBAL_CONFIG=str(root / "config"),
                   DBUS_SESSION_BUS_ADDRESS=f"unix:path={tmp}/no-bus", DECEBAL_BACKEND="x11",
                   DECEBAL_SYSTEM_AUTOMATIC="0")
        for key in ("WAYLAND_DISPLAY", "RDWM_SOCK", "DECSETTINGS_CONFIG"):
            env.pop(key, None)
        config = root / "config.toml"
        config.write_text('[general]\nborder_px = 2\ngap_inner = 6\n')
        with (root / "xvfb.log").open("w") as log:
            xvfb = subprocess.Popen(["Xvfb", display, "-screen", "0", "2400x1600x24", "-nolisten", "tcp"], env=env, stdout=log, stderr=log)
            try:
                wait_for(lambda: Path(f"/tmp/.X11-unix/X{display[1:]}").exists())
                for scale, mode in ((1, "dark"), (1, "light"), (2, "dark"), (2, "light")):
                    (root / "config" / "decebal.json").write_text(json.dumps({"theme": {"name": "macos", "mode": mode}}))
                    layout = root / f"layout-{scale}-{mode}.json"
                    env.update(DECSETTINGS_SCALE=str(scale), DECSETTINGS_DEBUG_LAYOUT=str(layout), DECEBAL_COLOR_SCHEME=mode)
                    with (evidence / f"settings-{scale}-{mode}.log").open("w") as app_log:
                        app = subprocess.Popen([str(binary), "--config", str(config), "appearance/fonts"], env=env, stdout=app_log, stderr=app_log)
                        try:
                            window = wait_for(lambda: run("xdotool", "search", "--class", "DecSettings", env=env).splitlines()[0])
                            for name, width, height in (("normal", 1040 * scale, 740 * scale), ("short", 900 * scale, 420 * scale)):
                                run("xdotool", "windowsize", window, str(width), str(height), env=env)
                                time.sleep(0.7)
                                run("xdotool", "mousemove", "--window", window, str(width * 3 // 4), str(height // 2), "click", "--repeat", "30", "--delay", "10", "4", env=env)
                                time.sleep(0.3)
                                doc = wait_for(lambda: json.loads(layout.read_text()))
                                assert doc["pane"] == "appearance", doc["pane"]
                                assert doc["section"] == "fonts", doc["section"]
                                assert doc["ctls"], "missing native controls"
                                shot = evidence / f"settings-{scale}00-{mode}-{name}.png"
                                run("import", "-window", window, str(shot), env=env)
                                image = Image.open(shot).convert("RGB")
                                assert image.size == (width, height), image.size
                                assert max(ImageStat.Stat(image).stddev) > 8, "blank native frame"
                                background = sum(image.getpixel((width - 24, height - 24))) / 3
                                assert (background > 150) == (mode == "light"), (mode, background)
                                (evidence / f"settings-{scale}00-{mode}-{name}.json").write_text(json.dumps(doc, indent=2))
                                # Exercise real scrolling in the short content viewport.
                                run("xdotool", "mousemove", "--window", window, str(width * 3 // 4), str(height // 2), "click", "--repeat", "8", "--delay", "50", "5", env=env)
                                time.sleep(0.3)
                                scrolled = json.loads(layout.read_text())
                                assert scrolled["scroll"] > doc["scroll"], "content cannot scroll"
                                run("import", "-window", window, str(evidence / f"settings-{scale}00-{mode}-{name}-scrolled.png"), env=env)
                        finally:
                            app.terminate()
                            app.wait(timeout=10)
            finally:
                xvfb.terminate()
                xvfb.wait(timeout=10)
    print(f"PASS native Settings 100/200%, light/dark, normal/short and content scrolling: {evidence}")


if __name__ == "__main__":
    main()
