#!/usr/bin/env python3
"""Private native Dux full/compact Applications-list query, navigation and launch gate.
python3 tests/e2e/dux-launcher-native.py BIN PACKAGE EVIDENCE
Desktop entries and typed app actions are fixtures; fonts and native renderer are real.
"""
import json
import os
from pathlib import Path
import re
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
    binary, package, evidence = (Path(s).resolve() for s in sys.argv[1:4])
    evidence.mkdir(parents=True, exist_ok=True)
    theme_id = tomllib.loads((package / "theme.toml").read_text())["id"]
    with tempfile.TemporaryDirectory(prefix="dux-launcher-native-") as private:
        root = Path(private)
        for name in ("bin", "home", "config", "cache", "data/applications", "state", "run", "themes"):
            (root / name).mkdir(parents=True, exist_ok=True)
        (root / "run").chmod(0o700)
        target = root / "themes" / theme_id
        shutil.copytree(package, target)
        source = (target / "theme.toml").read_text()
        match = re.search(r"(?ms)^\[launcher\]\n(.*?)(?=^\[|\Z)", source)
        if match:
            body = re.sub(r'(?m)^style\s*=.*$', 'style = "dux"', match.group(1))
            if not re.search(r'(?m)^style\s*=', body):
                body += 'style = "dux"\n'
            source = source[:match.start(1)] + body + source[match.end(1):]
        else:
            source += '\n[launcher]\nstyle = "dux"\n'
        (target / "theme.toml").write_text(source)
        shutil.copy2(binary, root / "bin/declaunch")
        calls = root / "calls"
        calls.write_text("")
        app = root / "bin/fixture-app"
        app.write_text('#!/bin/sh\nprintf "%s\\n" "$*" >> "$DUX_APP_CALLS"\n')
        app.chmod(0o755)
        icon = root / "fixture.svg"
        icon.write_text('<svg xmlns="http://www.w3.org/2000/svg" width="36" height="36"><rect x="2" y="2" width="32" height="32" rx="8" fill="#6750a4"/><path d="M10 12h16v12H10z" fill="#eaddff"/></svg>')
        for index in range(30):
            path = root / "data/applications" / f"fixture-{index:02}.desktop"
            path.write_text(f'[Desktop Entry]\nType=Application\nName=Dux Fixture {index:02}\nComment=Native desktop-entry description {index:02}\nIcon={icon}\nExec="{app}" {index:02}\nTerminal=false\n')
        env = dict(os.environ, HOME=str(root / "home"), XDG_CONFIG_HOME=str(root / "config"), XDG_CACHE_HOME=str(root / "cache"), XDG_DATA_HOME=str(root / "data"), XDG_DATA_DIRS=str(root / "data"), XDG_STATE_HOME=str(root / "state"), XDG_RUNTIME_DIR=str(root / "run"), DECEBAL_CONFIG=str(root / "config"), DECEBAL_THEMES=str(root / "themes"), DECEBAL_SYSTEM_THEMES=str(root / "themes"), DECEBAL_BACKEND="x11", DECEBAL_SYSTEM_AUTOMATIC="0", DBUS_SESSION_BUS_ADDRESS=f"unix:path={private}/no-session", DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={private}/no-system", DUX_APP_CALLS=str(calls), PATH=f'{root / "bin"}:{os.environ["PATH"]}')
        for key in ("WAYLAND_DISPLAY", "RDWM_SOCK", "PULSE_SERVER"):
            env.pop(key, None)
        ledger = []
        with (evidence / "native.log").open("w") as log:
            for screen_height in (720, 420):
                display = next(f":{n}" for n in range(440, 470) if not Path(f"/tmp/.X11-unix/X{n}").exists() and not Path(f"/tmp/.X{n}-lock").exists())
                env["DISPLAY"] = display
                server = subprocess.Popen(["Xvfb", display, "-screen", "0", f"960x{screen_height}x24", "-nolisten", "tcp"], env=env, stdout=log, stderr=log)
                launcher = None
                try:
                    def run(*args, check=True):
                        assert server.poll() is None, "owned private Xvfb exited"
                        return subprocess.run(args, env=env, check=check, capture_output=True, text=True).stdout.strip()
                    wait(lambda: server.poll() is None and Path(f"/tmp/.X{display[1:]}-lock").exists() and int(Path(f"/tmp/.X{display[1:]}-lock").read_text().strip()) == server.pid, "owned private Xvfb")
                    wait(lambda: run("xdotool", "getdisplaygeometry", check=False), "private display ready")
                    for scale in (1, 2):
                        env["DECEBAL_FONT_SCALE"] = str(scale)
                        for mode in ("dark", "light"):
                            (root / "config/decebal.json").write_text(json.dumps({"theme": {"name": theme_id, "mode": mode}}))
                            launcher = subprocess.Popen([str(root / "bin/declaunch")], env=env, stdout=log, stderr=log)
                            window = wait(lambda: run("xdotool", "search", "--class", "^declaunch$", check=False), "native launcher mapped").splitlines()[0]
                            time.sleep(.3)
                            def capture(suffix):
                                path = evidence / f"launcher-{screen_height}-{scale * 100}-{mode}-{suffix}.png"
                                run("import", "-window", "root", str(path))
                                return Image.open(path).convert("RGB")
                            top = capture("top")
                            ox, oy, height = 170, 50, min(620, screen_height - 100)
                            geometry = dict(line.split("=", 1) for line in run("xdotool", "getwindowgeometry", "--shell", window).splitlines() if "=" in line)
                            assert tuple(int(geometry[key]) for key in ("X", "Y", "WIDTH", "HEIGHT")) == (ox, oy, 620, height), geometry
                            run("xdotool", "key", "--window", window, "Next")
                            time.sleep(.1)
                            scrolled = capture("page-down")
                            assert ImageChops.difference(top, scrolled).getbbox(), "list selection/page did not change"
                            # No arbitrary-command provider: unmatched text has no executable result.
                            run("xdotool", "type", "--window", window, "--delay", "1", "unmatched ; echo forbidden")
                            time.sleep(.1)
                            capture("empty")
                            run("xdotool", "key", "--window", window, "Return")
                            time.sleep(.1)
                            assert launcher.poll() is None and not calls.read_text(), "unmatched query executed or closed"
                            # Source clear accessory is an actual native hit target.
                            run("xdotool", "mousemove", str(ox + 620 - 32), str(oy + 36), "click", "1")
                            run("xdotool", "type", "--window", window, "--delay", "1", "Dux Fixture 19")
                            time.sleep(.1)
                            capture("filtered")
                            before = len(calls.read_text().splitlines())
                            # Launch closes on key press; xdotool's key release can see BadWindow.
                            run("xdotool", "key", "--window", window, "Return", check=False)
                            wait(lambda: len(calls.read_text().splitlines()) > before, "typed desktop app fixture launched")
                            assert calls.read_text().splitlines()[-1] == "19", calls.read_text()
                            launcher.wait(timeout=10)
                            launcher = None
                            calls.write_text("")
                            ledger.append({"screen": [960, screen_height], "panel": [ox, oy, 620, height], "scale": scale, "mode": mode, "typed_launch": "19", "unmatched_exec": False})
                finally:
                    if launcher is not None:
                        launcher.terminate()
                        launcher.wait(timeout=10)
                    server.terminate()
                    server.wait(timeout=10)
        (evidence / "checks.json").write_text(json.dumps(ledger, indent=2))
    print(f"PASS native Dux Applications list/query/clear/page/typed launch at100/200 light/dark normal/short: {evidence}")


if __name__ == "__main__":
    main()
