#!/usr/bin/env python3
"""Private Source/painter + actual X11 Dashboard clock QA.
Usage: dux-clock-native.py BIN_DIR PACKAGE TEST_BIN EVIDENCE
Copies only the package; the fifth font entry is activated only in this fixture.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time
from PIL import ImageGrab, ImageStat


def set_key(text, section, key, value):
    match = re.search(rf"(?ms)^\[{re.escape(section)}\]\n(.*?)(?=^\[|\Z)", text)
    if not match:
        return text + f"\n[{section}]\n{key} = {value}\n"
    body = match.group(1)
    line = rf"(?m)^{re.escape(key)}\s*=.*$"
    body = re.sub(line, f"{key} = {value}", body) if re.search(line, body) else body + f"{key} = {value}\n"
    return text[:match.start(1)] + body + text[match.end(1):]


def main():
    binaries, package, test, evidence = [Path(p).resolve() for p in sys.argv[1:5]]
    evidence.mkdir(parents=True, exist_ok=True)
    checks = []
    with tempfile.TemporaryDirectory(prefix="dux-clock-native-") as private:
        root = Path(private)
        for name in ("home", "config", "cache", "state", "data", "run", "themes", "bin"):
            (root / name).mkdir()
        (root / "run").chmod(0o700)
        target = root / "themes/dux-lavender"
        shutil.copytree(package, target)
        variable = "fonts/GoogleSansFlex-Variable.ttf"
        if not (target / variable).is_file():
            asset=Path(os.environ["DUX_CLOCK_VARIABLE_FONT"]).resolve()
            shutil.copy2(asset,target / variable)
        assert (target / variable).is_file(), "original variable asset required"
        assert hashlib.sha256((target / variable).read_bytes()).hexdigest() == "c6d53424121196b81de816b8daccf200e285dd506df43766db3d7e8cdf06ee30", "exact pinned source font required for evidence"
        manifest = target / "theme.toml"
        source = manifest.read_text()
        import tomllib
        files = tomllib.loads(source)["font"]["files"]
        four_files=[file for file in files if file!=variable]
        if variable not in files:
            files.append(variable)
        source = set_key(source, "font", "files", json.dumps(files))
        source = set_key(source, "dashboard", "style", '"dux"')
        source = set_key(source, "dashboard", "profile", '"readme"')
        manifest.write_text(source)
        shutil.copy2(binaries / "decdash", root / "bin/decdash")
        curl = root / "bin/curl"
        curl.write_text("#!/bin/sh\nexit 1\n")
        curl.chmod(0o755)
        env = dict(os.environ, HOME=str(root / "home"), XDG_CONFIG_HOME=str(root / "config"),
                   XDG_CACHE_HOME=str(root / "cache"), XDG_STATE_HOME=str(root / "state"),
                   XDG_DATA_HOME=str(root / "data"), XDG_RUNTIME_DIR=str(root / "run"),
                   DECEBAL_CONFIG=str(root / "config"), DECEBAL_THEMES=str(root / "themes"),
                   DECEBAL_SYSTEM_THEMES=str(root / "themes"), DECEBAL_SYSTEM_AUTOMATIC="0",
                   DBUS_SESSION_BUS_ADDRESS=f"unix:path={private}/no-session-bus",
                   DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={private}/no-system-bus",
                   PATH=str(root / "bin") + os.pathsep + os.environ.get("PATH", ""),
                   DUX_CLOCK_EVIDENCE_DIR=str(evidence / "painter"))
        for key in ("DISPLAY", "WAYLAND_DISPLAY", "RDWM_SOCK", "PULSE_SERVER", "XAUTHORITY", "DBUS_STARTER_ADDRESS", "DBUS_STARTER_BUS_TYPE", "SWAYSOCK", "HYPRLAND_INSTANCE_SIGNATURE", "SESSION_MANAGER", "XDG_SESSION_ID"):
            env.pop(key, None)
        config = root / "config/decebal.json"
        config.write_text(json.dumps({"theme": {"name": "dux-lavender", "mode": "dark"}}))
        for motion in ("0", "1"):
            env["DECEBAL_REDUCE_MOTION"] = motion
            folder = evidence / ("painter" if motion == "0" else "painter-reduced")
            env["DUX_CLOCK_EVIDENCE_DIR"] = str(folder)
            with (evidence / f"painter-motion-{motion}.log").open("w") as log:
                subprocess.run([str(test), "clockface::tests::native_variable_clock_evidence", "--exact", "--ignored", "--test-threads=1"], env=env, check=True, stdout=log, stderr=log, timeout=60)
        for image in (evidence / "painter").glob("*.png"):
            assert image.read_bytes() == (evidence / "painter-reduced" / image.name).read_bytes(), "reduced-motion must preserve steady clock pixels"
        for screen_width, screen_height in ((1920,1080),(960,720),(640,420)):
            display = next(f":{n}" for n in range(440, 490) if not Path(f"/tmp/.X11-unix/X{n}").exists() and not Path(f"/tmp/.X{n}-lock").exists())
            env.update(DISPLAY=display, DECEBAL_BACKEND="x11", XDG_SESSION_TYPE="x11")
            with (evidence / f"native-{screen_height}.log").open("w") as log:
                server = subprocess.Popen(["Xvfb", display, "-screen", "0", f"{screen_width}x{screen_height}x24", "-nolisten", "tcp"], env=env, stdout=log, stderr=log)
                child = None
                try:
                    def run(*args, check=True):
                        assert server.poll() is None, "owned Xvfb exited"
                        return subprocess.run(args, env=env, check=check, capture_output=True, text=True, timeout=5).stdout.strip()
                    def wait(action, label):
                        until = time.monotonic() + 8
                        while time.monotonic() < until:
                            value = action()
                            if value:
                                return value
                            time.sleep(.1)
                        raise AssertionError(label)
                    wait(lambda: run("xdotool", "getdisplaygeometry", check=False), "private display")
                    for scale in (1, 2):
                        for mode in ("dark", "light"):
                            env.update(DECEBAL_FONT_SCALE=str(scale), DECEBAL_REDUCE_MOTION="1")
                            config.write_text(json.dumps({"theme": {"name": "dux-lavender", "mode": mode}}))
                            child = subprocess.Popen([str(root / "bin/decdash"), "--top", "48"], env=env, stdout=log, stderr=log)
                            window = wait(lambda: run("xdotool", "search", "--class", "^decdash$", check=False), "native clock host").splitlines()[0]
                            time.sleep(.8)
                            assert child.poll() is None, "dashboard must stay open"
                            thread_names=[path.read_text().strip() for path in Path(f"/proc/{child.pid}/task").glob("*/comm")]
                            assert thread_names.count("dux-clock")==1,"one preparation worker required"
                            geo = dict(line.split("=", 1) for line in run("xdotool", "getwindowgeometry", "--shell", window).splitlines() if "=" in line)
                            x, y, w, h = (int(geo[key]) for key in ("X", "Y", "WIDTH", "HEIGHT"))
                            assert x >= 0 and y >= 0 and x + w <= screen_width and y + h <= screen_height
                            image = ImageGrab.grab(xdisplay=display).convert("RGB").crop((x, y, x + w, y + h))
                            assert max(ImageStat.Stat(image).stddev) > 8
                            name = f"dashboard-{screen_height}-{mode}-{scale*100}"
                            image.save(evidence / f"{name}.png")
                            # Clock card is top-left in both wide/reflow layouts.
                            s = scale
                            cell = (w-24*s-5*8*s)/6 if w >= 780*s else (w-24*s-8*s)/2
                            clock = image.crop((12*s,92*s,min(w,round(12*s+cell)),min(h,(92+200)*s)))
                            assert max(ImageStat.Stat(clock).stddev) > 12, "actual clock card must contain ink"
                            pixels=clock.load()
                            assert sum(1 for y in range(clock.height) for x in range(clock.width) if pixels[x,y][2]-pixels[x,y][1]>=30 and pixels[x,y][0]>60)>200,"clock must contain real primary-coloured digits, not a loading/missing message"
                            clock.save(evidence / f"{name}-clock.png")
                            scrolled = False
                            if scale == 2:
                                run("xdotool", "key", "--window", window, "Next")
                                if screen_height == 420:
                                    run("xdotool", "key", "--window", window, "Next")
                                time.sleep(.2)
                                shifted = ImageGrab.grab(xdisplay=display).convert("RGB").crop((x, y, x+w, y+h))
                                assert shifted.tobytes() != image.tobytes(), "clock must remain reachable by keyboard scrolling"
                                shifted.save(evidence / f"{name}-scrolled.png")
                                scrolled = True
                            checks.append({"case": name, "geometry": geo, "reduced_motion": True, "keyboard_scrolled": scrolled})
                            run("xdotool", "key", "--window", window, "Escape", check=False)
                            child.wait(timeout=5)
                            child = None
                    if screen_width==1920:
                        # Asset presence alone does not activate a capability:
                        # the deployed four-entry manifest keeps its old clock.
                        manifest.write_text(set_key(source,"font","files",json.dumps(four_files)))
                        env.update(DECEBAL_FONT_SCALE="1")
                        config.write_text(json.dumps({"theme":{"name":"dux-lavender","mode":"dark"}}))
                        child=subprocess.Popen([str(root/"bin/decdash"),"--top","48"],env=env,stdout=log,stderr=log)
                        window=wait(lambda:run("xdotool","search","--class","^decdash$",check=False),"four-font compatibility host").splitlines()[0]
                        time.sleep(.8)
                        assert child.poll() is None
                        assert "dux-clock" not in [path.read_text().strip() for path in Path(f"/proc/{child.pid}/task").glob("*/comm")]
                        ImageGrab.grab(xdisplay=display).save(evidence/"four-font-compatibility.png")
                        run("xdotool","key","--window",window,"Escape",check=False);child.wait(timeout=5);child=None
                        manifest.write_text(source)
                finally:
                    if child is not None and child.poll() is None:
                        child.terminate()
                        child.wait(timeout=5)
                    if server.poll() is None:
                        server.terminate()
                        server.wait(timeout=5)
    (evidence / "checks.json").write_text(json.dumps(checks, indent=2))
    print(f"PASS native variable clock:28 painter +4 missing-state renders (both motion modes),12 private variable-clock Dashboard windows + four-font compatibility: {evidence}")


if __name__ == "__main__":
    main()
