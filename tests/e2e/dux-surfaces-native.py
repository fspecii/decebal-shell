#!/usr/bin/env python3
"""Private Xvfb real Dux dashboard/Control Center input and capture gate.

python3 tests/e2e/dux-surfaces-native.py /path/to/debug /path/to/dux-package /tmp/evidence
Only private package copies are changed. No live/nested display or desktop bus
is used. Settings opens are captured by a private sibling fixture.
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

from PIL import Image, ImageGrab, ImageStat, features


def wait(predicate, label):
    deadline = time.monotonic() + 12
    while time.monotonic() < deadline:
        try:
            value = predicate()
            if value:
                return value
        except (OSError, ValueError, subprocess.CalledProcessError):
            pass
        time.sleep(.1)
    raise AssertionError(label)


def set_key(text, section, key, value):
    pattern = rf"(?ms)^\[{re.escape(section)}\]\n(.*?)(?=^\[|\Z)"
    found = re.search(pattern, text)
    if not found:
        return text + f"\n[{section}]\n{key} = {value}\n"
    body = found.group(1)
    line = rf"(?m)^{re.escape(key)}\s*=.*$"
    body = re.sub(line, f"{key} = {value}", body) if re.search(line, body) else body + f"{key} = {value}\n"
    return text[:found.start(1)] + body + text[found.end(1):]


def main():
    binaries, package, evidence = map(lambda s: Path(s).resolve(), sys.argv[1:4])
    evidence.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="dux-surfaces-native-") as private:
        root = Path(private)
        for name in ("bin", "home", "config/omarchy", "cache", "data", "state", "run", "themes"):
            (root / name).mkdir(parents=True)
        (root / "run").chmod(0o700)
        for name in ("decbar", "decdash"):
            shutil.copy2(binaries / name, root / "bin" / name)
        calls = root / "settings-calls"
        fixture = root / "bin/decsettings"
        fixture.write_text('#!/bin/sh\necho entered >> "$DUX_SETTINGS_CALLS.entered"\nif [ -n "$DUX_SETTINGS_FIXTURE_DELAY" ]; then sleep "$DUX_SETTINGS_FIXTURE_DELAY"; fi\nprintf "%s\\n" "$*" >> "$DUX_SETTINGS_CALLS"\n')
        fixture.chmod(0o755)
        curl = root / "bin/curl"
        curl.write_text("#!/bin/sh\nexit 1\n")
        curl.chmod(0o755)
        theme = tomllib.loads((package / "theme.toml").read_text())
        theme_id = theme["id"]
        copy = root / "themes" / theme_id
        shutil.copytree(package, copy)
        source = (copy / "theme.toml").read_text()
        for section, key, value in (("dashboard", "style", '"dux"'), ("dashboard", "profile", '"readme"'), ("control_center", "style", '"dux"'), ("bar", "style", '"dux"')):
            source = set_key(source, section, key, value)
        (copy / "theme.toml").write_text(source)
        display = next(f":{n}" for n in range(320, 380) if not Path(f"/tmp/.X11-unix/X{n}").exists() and not Path(f"/tmp/.X{n}-lock").exists())
        env = dict(os.environ, HOME=str(root / "home"), DISPLAY=display, DECEBAL_BACKEND="x11",
                   XDG_CONFIG_HOME=str(root / "config"), XDG_CACHE_HOME=str(root / "cache"),
                   XDG_DATA_HOME=str(root / "data"), XDG_STATE_HOME=str(root / "state"),
                   XDG_RUNTIME_DIR=str(root / "run"), DECEBAL_CONFIG=str(root / "config/omarchy"),
                   DECEBAL_THEMES=str(root / "themes"), DECEBAL_SYSTEM_THEMES=str(root / "themes"),
                   DBUS_SESSION_BUS_ADDRESS=f"unix:path={private}/no-session-bus",
                   DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={private}/no-system-bus",
                   DECEBAL_SYSTEM_AUTOMATIC="0", DUX_SETTINGS_CALLS=str(calls),
                   DECEBAL_LOG="debug", DECEBAL_LOG_DIR=str(root / "diagnostic-logs"))
        for key in ("WAYLAND_DISPLAY", "RDWM_SOCK", "PULSE_SERVER", "DBUS_STARTER_ADDRESS", "DBUS_STARTER_BUS_TYPE", "XAUTHORITY"):
            env.pop(key, None)
        env["PATH"] = str(root / "bin") + os.pathsep + env.get("PATH", "")
        processes = []
        with (evidence / "native.log").open("w") as log:
            server = subprocess.Popen(["Xvfb", display, "-screen", "0", "960x720x24", "-nolisten", "tcp"], env=env, stdout=log, stderr=log)
            try:
                def run(*args, check=True):
                    assert server.poll() is None, "owned private Xvfb exited"
                    return subprocess.run(args, env=env, check=check, text=True, capture_output=True, timeout=8).stdout.strip()

                def capture(window, name):
                    path = evidence / f"{name}.png"
                    assert features.check_feature("xcb"), "Pillow native XCB capture required"
                    deadline=time.monotonic()+8
                    while True:
                        image = ImageGrab.grab(xdisplay=display).convert("RGB")
                        if window != "root":
                            geometry = dict(line.split("=", 1) for line in run("xdotool", "getwindowgeometry", "--shell", window).splitlines() if "=" in line)
                            x, y, width, height = (int(geometry[k]) for k in ("X", "Y", "WIDTH", "HEIGHT"))
                            image = image.crop((x, y, x + width, y + height))
                        if max(ImageStat.Stat(image).stddev)>8:
                            image.save(path)
                            return image
                        assert time.monotonic()<deadline, f"native frame remained blank: {path}"
                        time.sleep(.1)

                wait(lambda: server.poll() is None and Path(f"/tmp/.X{display[1:]}-lock").exists() and int(Path(f"/tmp/.X{display[1:]}-lock").read_text().strip()) == server.pid, "owned private Xvfb")

                wait(lambda: run("xdotool", "getdisplaygeometry", check=False), "private display ready")
                for scale in (1, 2):
                    env["DECEBAL_FONT_SCALE"] = str(scale)
                    for mode in ("dark", "light"):
                        if os.environ.get("DUX_SURFACE_CASE") and os.environ["DUX_SURFACE_CASE"] != f"{scale*100}-{mode}":
                            continue
                        config = {"theme": {"name": theme_id, "mode": mode}, "control_center": {"style": "dux"}, "bar": {"enabled": True}}
                        (root / "config/omarchy/decebal.json").write_text(json.dumps(config))
                        if os.environ.get("DUX_SURFACE_HOST_PART") not in ("control", "bar"):
                            dash = subprocess.Popen([str(root / "bin/decdash"), "--top", "48"], env=env, stdout=log, stderr=log)
                            processes.append(dash)
                            window = wait(lambda: run("xdotool", "search", "--class", "^decdash$", check=False), "native dashboard mapped").splitlines()[0]
                            time.sleep(.8)
                            image = capture(window, f"dash-{scale * 100}-{mode}-overview")
                            assert image.width <= 960 and image.height <= 672, image.size
                            for page in ("media", "weather"):
                                run("xdotool", "key", "--window", window, "Right")
                                time.sleep(.2)
                                capture(window, f"dash-{scale * 100}-{mode}-{page}")
                            run("xdotool", "key", "--window", window, "Right", "Next")
                            time.sleep(.2)
                            capture(window, f"dash-{scale * 100}-{mode}-overview-scrolled")
                            # Fourth navigation item is the real source Settings action.
                            previous_calls = len(calls.read_text().splitlines()) if calls.exists() else 0
                            run("xdotool", "mousemove", "--window", window, str(image.width * 7 // 8), str(40 * scale), "click", "1")
                            wait(lambda: calls.exists() and len(calls.read_text().splitlines()) > previous_calls and calls.read_text().splitlines()[-1] == "appearance", "native Settings sibling action")
                            dash.wait(timeout=5)
                            processes.remove(dash)
                        if os.environ.get("DUX_SURFACE_HOST_PART") != "bar":
                            panel_dump = root / "panel.json"
                            env["DECBAR_DEBUG_PANEL"] = str(panel_dump)
                            panel = subprocess.Popen([str(root / "bin/decbar"), "--control-center"], env=env, stdout=log, stderr=log)
                            processes.append(panel)
                            wait(lambda: panel_dump.exists() and panel_dump.read_text().startswith("panel "), "native Control Center painted")
                            dump = panel_dump.read_text()
                            rect = next(line.split()[1:] for line in dump.splitlines() if line.startswith("panel "))
                            x, y, width, height = map(float, rect)
                            state = {"panel": [x, y, width, height], "dump": dump}
                            (evidence / f"control-{scale * 100}-{mode}.json").write_text(json.dumps(state, indent=2))
                            assert x >= 0 and y >= 0 and x + width <= 960 and y + height <= 720, state
                            capture("root", f"control-{scale * 100}-{mode}")
                            panel_window = wait(lambda: run("xdotool", "search", "--name", "^decebal-control-center$", check=False), "native Control Center window").splitlines()[0]
                            run("xdotool", "mousemove", "--window", panel_window, str(round(x + width / 2)), str(round(y + height / 2)), "click", "--repeat", "15", "--delay", "20", "5")
                            time.sleep(.3)
                            scrolled_dump = panel_dump.read_text()
                            scroll_line = next(line.split() for line in scrolled_dump.splitlines() if line.startswith("scroll "))
                            scroll, content = float(scroll_line[1]), float(scroll_line[3])
                            if content > height:
                                assert scroll > 0, scrolled_dump
                            capture("root", f"control-{scale * 100}-{mode}-scrolled")
                            # Navigation changes focus/reveals controls without invoking hardware actions.
                            run("xdotool", "key", "--window", panel_window, *(["Down"] * 12))
                            time.sleep(.2)
                            capture("root", f"control-{scale * 100}-{mode}-keyboard")
                            run("xdotool", "mousemove", "--window", panel_window, str(round(x + width / 2)), str(round(y + height / 2)), "click", "--repeat", "100", "--delay", "5", "4")
                            wait(lambda: panel_dump.exists() and any(line.startswith("scroll 0 ") for line in panel_dump.read_text().splitlines()), "Control Center returned to top before header action")
                            capture("root", f"control-{scale * 100}-{mode}-before-header")
                            previous_calls = len(calls.read_text().splitlines()) if calls.exists() else 0
                            header = 72 + (scale - 1) * 24
                            click_start=time.monotonic()
                            diagnostic={"before_click_dump":panel_dump.read_text(),"window_geometry":run("xdotool","getwindowgeometry","--shell",panel_window),"pointer":run("xdotool","getmouselocation","--shell"),"click_window_relative":[round(x+556),round(y+16+header/2)],"exit_before":panel.poll()}
                            (evidence/f"control-{scale*100}-{mode}-click-diagnostic.json").write_text(json.dumps(diagnostic,indent=2))
                            run("xdotool", "mousemove", "--window", panel_window, str(round(x + 556)), str(round(y + 16 + header / 2)), "click", "1")
                            def header_action_arrived():
                                if panel.poll() is not None and "exit_observed_seconds" not in diagnostic:
                                    diagnostic["exit_observed_seconds"]=time.monotonic()-click_start
                                return calls.exists() and len(calls.read_text().splitlines()) > previous_calls
                            try:
                                wait(header_action_arrived, "Control Center native Settings action")
                            except AssertionError:
                                diagnostic.update(fixture_entered=Path(str(calls)+".entered").read_text() if Path(str(calls)+".entered").exists() else None,fixture_calls=calls.read_text() if calls.exists() else None,exit_after=panel.poll(),seconds_after_click=time.monotonic()-click_start,after_click_dump=panel_dump.read_text(),remaining_windows=run("xdotool","search","--name","^decebal-control-center$",check=False),pointer_after=run("xdotool","getmouselocation","--shell"))
                                (evidence/f"control-{scale*100}-{mode}-click-diagnostic.json").write_text(json.dumps(diagnostic,indent=2))
                                (evidence / f"control-{scale*100}-{mode}-failure.txt").write_text(f"exit={panel.poll()}\n"+panel_dump.read_text())
                                raise
                            panel.wait(timeout=5)
                            processes.remove(panel)
                            panel_dump.unlink(missing_ok=True)
                        if os.environ.get("DUX_SURFACE_HOST_PART") != "control":
                            config["bar"]={"enabled":True,"background":"band","margin":4,"spacing":4,"radius":14,"left":["launcherButton"],"center":["clock"],"right":["controlCenterButton"]}
                            (root / "config/omarchy/decebal.json").write_text(json.dumps(config))
                            tray=root/"bar-tray.txt"
                            tray.unlink(missing_ok=True)
                            env["DECBAR_DEBUG_TRAY"]=str(tray)
                            bar=subprocess.Popen([str(root/"bin/decbar"),"--app","Native fixture"],env=env,stdout=log,stderr=log)
                            processes.append(bar)
                            try:
                                wait(lambda:tray.exists() and "Control " in tray.read_text(),"native mainbar tray painted")
                            except AssertionError:
                                (evidence / "failed-mainbar-state.json").write_text(json.dumps({"returncode": bar.poll(), "scale": scale, "mode": mode, "tray": tray.read_text() if tray.exists() else None, "config": config}, indent=2))
                                capture("root", "failed-mainbar-root")
                                raise
                            bar_window=wait(lambda:run("xdotool","search","--name","^decebal-bar$",check=False),"native bar window").splitlines()[0]
                            control=float(dict(line.split(" ",1) for line in tray.read_text().splitlines())["Control"])
                            run("xdotool","mousemove","--window",bar_window,str(round(control)),"23","click","1")
                            wait(lambda:sum(ImageGrab.grab(xdisplay=display).convert("RGB").getpixel((900,100)))>20,"mainbar Control Center presented")
                            image=capture("root",f"mainbar-control-{scale*100}-{mode}")
                            assert image.getpixel((900,719))==(0,0,0),"mainbar panel exceeds short viewport"
                            run("xdotool","mousemove","--window",bar_window,"650","360","click","--repeat","15","--delay","20","5")
                            run("xdotool","key","--window",bar_window,*(["Down"]*12))
                            time.sleep(.3)
                            capture("root",f"mainbar-control-{scale*100}-{mode}-keyboard")
                            run("xdotool","mousemove","--window",bar_window,"650","360","click","--repeat","100","--delay","5","4")
                            previous_calls=len(calls.read_text().splitlines()) if calls.exists() else 0
                            run("xdotool","mousemove","--window",bar_window,"908",str(54+16+(72+(scale-1)*24)//2),"click","1")
                            wait(lambda:calls.exists() and len(calls.read_text().splitlines())>previous_calls,"mainbar Control Center Settings action")
                            bar.terminate();bar.wait(timeout=5);processes.remove(bar)

            finally:
                if (root/"diagnostic-logs").exists():
                    shutil.copytree(root/"diagnostic-logs",evidence/"diagnostic-logs",dirs_exist_ok=True)
                for process in processes:
                    process.terminate()
                    process.wait(timeout=5)
                server.terminate()
                server.wait(timeout=5)
    print(f"PASS private native Dux {'Control Center' if os.environ.get('DUX_SURFACE_HOST_PART')=='control' else ('mainbar Control Center' if os.environ.get('DUX_SURFACE_HOST_PART')=='bar' else 'dashboard pages and Control Center')} scroll/keyboard/Settings action at100/200 light/dark: {evidence}")


if __name__ == "__main__":
    main()
