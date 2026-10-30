#!/usr/bin/env python3
"""Private native Settings capture matrix (second-pass redesign evidence).

SETTINGS_BIN=/abs/decsettings python3 tests/e2e/settings-capture.py OUTDIR [CASES.json]

Starts one Xvfb on a free display (both the socket and lock file must be absent), private HOME/XDG paths and unreachable
D-Bus addresses; never touches the desktop, window manager or live config. Cases: name, pane, scale (UI), text (font
scale), mode, w, h, search, keys (xdotool key chords, "click:x,y", "type:text", "sleep:s"), scroll (wheel notches).
Writes NAME.png and the layout dump NAME.json for each case.
"""
import json, os, subprocess, sys, tempfile, time
from pathlib import Path

def run(*a, env, **k):
    return subprocess.check_output(a, env=env, text=True, **k).strip()

def wait_for(fn, n=100):
    for _ in range(n):
        try:
            v = fn()
            if v:
                return v
        except (OSError, ValueError, subprocess.CalledProcessError, IndexError):
            pass
        time.sleep(0.1)
    raise AssertionError("not ready")

def free_display():
    for n in range(300, 360):
        if not Path(f"/tmp/.X11-unix/X{n}").exists() and not Path(f"/tmp/.X{n}-lock").exists():
            return f":{n}"
    raise SystemExit("no free display")

DEFAULT_CASES = [
    {"name": f"{pane.replace('/', '-')}-{mode}-ui{int(scale * 100)}-text{int(text * 100)}-{size}", "pane": pane, "mode": mode, "scale": scale, "text": text,
     "w": int((1040 if size == "normal" else 900) * scale), "h": int((740 if size == "normal" else 420) * scale)}
    for pane in ("appearance", "appearance/fonts", "appearance/window", "notifications")
    for mode in ("dark", "light")
    for scale, text in ((1, 1), (1.25, 1.25), (2, 1), (1, 2))
    for size in ("normal", "short")
]


def main():
    binary, out = Path(os.environ["SETTINGS_BIN"]).resolve(), Path(sys.argv[1])
    cases = json.load(open(sys.argv[2])) if len(sys.argv) > 2 else DEFAULT_CASES
    out.mkdir(parents=True, exist_ok=True)
    display = free_display()
    with tempfile.TemporaryDirectory(prefix="dux2-") as tmp:
        root = Path(tmp)
        for d in ("home", "config", "cache", "data", "state", "run"):
            (root / d).mkdir(mode=0o700)
        env = dict(os.environ, HOME=str(root / "home"), DISPLAY=display, XDG_CONFIG_HOME=str(root / "config"),
                   XDG_CACHE_HOME=str(root / "cache"), XDG_DATA_HOME=str(root / "data"), XDG_STATE_HOME=str(root / "state"),
                   XDG_RUNTIME_DIR=str(root / "run"), DECEBAL_CONFIG=str(root / "config"),
                   DBUS_SESSION_BUS_ADDRESS=f"unix:path={tmp}/no-bus", DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={tmp}/no-sys",
                   DECEBAL_BACKEND="x11", DECEBAL_SYSTEM_AUTOMATIC="0")
        for k in ("WAYLAND_DISPLAY", "RDWM_SOCK", "DECSETTINGS_CONFIG", "HYPRLAND_INSTANCE_SIGNATURE"):
            env.pop(k, None)
        # Inherited overrides must not route this capture into the user's assets or IPC.
        for k in ("XAUTHORITY", "SESSION_MANAGER", "DBUS_STARTER_ADDRESS", "DBUS_STARTER_BUS_TYPE", "AT_SPI_BUS_ADDRESS"):
            env.pop(k, None)
        for k, d in {"XDG_CONFIG_DIRS": "no-config", "XDG_DATA_DIRS": "no-data", "DECEBAL_THEMES": "themes",
                     "DECEBAL_SYSTEM_THEMES": "system-themes", "DECEBAL_PALETTES": "palettes", "DECEBAL_WIDGETS": "widgets"}.items():
            path = root / d
            path.mkdir(exist_ok=True)
            env[k] = str(path)
        env.update(XDG_SESSION_TYPE="x11", RDWMCTL="/nonexistent/rdwmctl", RDWM_SOCK=str(root / "no-wm.sock"))
        cfg = root / "config.toml"
        cfg.write_text("[general]\nborder_px = 2\ngap_inner = 6\n")
        with (root / "xvfb.log").open("w") as log:
            xvfb = subprocess.Popen(["Xvfb", display, "-screen", "0", "2600x1700x24", "-nolisten", "tcp"], env=env, stdout=log, stderr=log)
        try:
            def owned_display():
                if xvfb.poll() is not None:
                    raise RuntimeError("Private Xvfb exited before becoming ready")
                lock = Path(f"/tmp/.X{display[1:]}-lock")
                return lock.exists() and int(lock.read_text().strip()) == xvfb.pid and Path(f"/tmp/.X11-unix/X{display[1:]}").exists()
            wait_for(owned_display)
            for c in cases:
                e = dict(env)
                mode = c.get("mode", "dark")
                (root / "config" / "decebal.json").write_text(json.dumps({"theme": {"name": c.get("theme", "macos"), "mode": mode}}))
                layout = root / f"layout-{c['name']}.json"
                e.update(DECSETTINGS_SCALE=str(c.get("scale", 1)), DECSETTINGS_DEBUG_LAYOUT=str(layout), DECEBAL_COLOR_SCHEME=mode)
                if c.get("text", 1) != 1:
                    e["DECEBAL_FONT_SCALE"] = str(c["text"])
                else:
                    e.pop("DECEBAL_FONT_SCALE", None)
                w, h = c.get("w", 1040), c.get("h", 740)
                with (out / f"{c['name']}.log").open("w") as lg:
                    app = subprocess.Popen([str(binary), "--config", str(cfg), c.get("pane", "appearance")], env=e, stdout=lg, stderr=lg)
                try:
                    win = wait_for(lambda: run("xdotool", "search", "--class", "DecSettings", env=e).splitlines()[0])
                    run("xdotool", "windowsize", win, str(w), str(h), env=e)
                    time.sleep(0.8)
                    wait_for(lambda: json.loads(layout.read_text()))
                    if c.get("search"):
                        run("xdotool", "key", "ctrl+f", env=e); time.sleep(0.2)
                        run("xdotool", "type", "--delay", "40", c["search"], env=e); time.sleep(0.5)
                    for k in c.get("keys", []):
                        if k.startswith("click:"):
                            x, y = k[6:].split(","); run("xdotool", "mousemove", "--window", win, x, y, "click", "1", env=e)
                        elif k.startswith("type:"):
                            run("xdotool", "type", "--delay", "40", k[5:], env=e)
                        elif k.startswith("sleep:"):
                            time.sleep(float(k[6:]))
                        else:
                            run("xdotool", "key", "--delay", "80", *k.split(), env=e)
                        time.sleep(0.25)
                    if c.get("scroll"):
                        run("xdotool", "mousemove", "--window", win, str(w * 2 // 3), str(h // 2), "click", "--repeat", str(c["scroll"]), "--delay", "30", "5", env=e)
                    time.sleep(0.6)
                    run("import", "-window", win, str(out / f"{c['name']}.png"), env=e)
                    try:
                        (out / f"{c['name']}.json").write_text(layout.read_text())
                    except OSError:
                        pass
                    print("ok", c["name"], flush=True)
                finally:
                    app.terminate()
                    try: app.wait(timeout=10)
                    except Exception: app.kill()
        finally:
            xvfb.terminate(); xvfb.wait(timeout=10)

main()
