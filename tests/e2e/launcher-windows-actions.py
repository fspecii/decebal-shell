#!/usr/bin/env python3
"""Windows Start search actions on a short 2x screen, natively, with one executable per app so identity is proven.

    LAUNCHER_ACTIONS_EVIDENCE=/var/tmp/launcher-actions python3 tests/e2e/launcher-windows-actions.py

A private Xvfb (800x480, Xft.dpi 192) with a 64 px panel strip on top: the Start menu is short, so the best-match
pane is at its most compact. For the second search result it opens and pins (keyboard: Down, Tab, Enter / Down,
Enter; pointer: hover the result, click Open / Pin found on screen) and checks that exactly that app's own executable
ran and that exactly its desktop id was pinned. Two apps both called "Terminal" check that usage history follows the
desktop id: the one with history ranks first and is the one that runs. Never touches the live desktop.
LAUNCHER_BIN_DIR selects declaunch (default: $CARGO_TARGET_DIR or target, debug profile).
"""
from pathlib import Path
import hashlib, json, os, shutil, signal, subprocess, sys, tempfile, time
from PIL import Image
sys.path.insert(0, str(Path(__file__).resolve().parent))
from private_x11 import start_private_xvfb

repo = Path(__file__).resolve().parents[2]
target = Path(os.environ.get("CARGO_TARGET_DIR", repo / "target"))
bins = Path(os.environ.get("LAUNCHER_BIN_DIR", target / "debug"))
evidence = Path(os.environ.get("LAUNCHER_ACTIONS_EVIDENCE", "/var/tmp/launcher-actions-evidence"))
evidence.mkdir(parents=True, exist_ok=True)
root = Path(tempfile.mkdtemp(prefix="decebal-launcher-actions-", dir=os.environ.get("LAUNCHER_ACTIONS_WORK", "/var/tmp")))
root.chmod(0o700)
env = dict(os.environ)
for k in list(env):
    if k in ("DISPLAY", "WAYLAND_DISPLAY", "XAUTHORITY", "RDWM_SOCK", "SESSION_MANAGER", "AT_SPI_BUS_ADDRESS", "DBUS_SESSION_BUS_ADDRESS") or k.startswith(("DECEBAL_", "DECSPOT_", "DECSETTINGS_")):
        env.pop(k, None)
for k, v in {"HOME": "home", "XDG_CONFIG_HOME": "cfg", "XDG_CONFIG_DIRS": "nocfg", "XDG_DATA_HOME": "data", "XDG_DATA_DIRS": "data", "XDG_CACHE_HOME": "cache",
             "XDG_STATE_HOME": "state", "XDG_RUNTIME_DIR": "run", "DECEBAL_CONFIG": "shell", "DECEBAL_THEMES": "themes", "DECEBAL_SYSTEM_THEMES": "systemthemes"}.items():
    env[k] = str(root / v)
    Path(env[k]).mkdir(parents=True, exist_ok=True)
os.chmod(env["XDG_RUNTIME_DIR"], 0o700)
env.update(DECEBAL_BACKEND="x11", DECEBAL_SYSTEM_AUTOMATIC="0", DBUS_SYSTEM_BUS_ADDRESS="unix:path=" + str(root / "no-system-bus"))
calls = root / "calls"
calls.write_text("")
env["ACTION_CALLS"] = str(calls)
fixture_bin = root / "bin"
fixture_bin.mkdir()
apps = Path(env["XDG_DATA_HOME"]) / "applications"
apps.mkdir()
ACCENT = (0xFF, 0x00, 0xFF)  # the theme accent override: only the pane's glyphs, the search underline and the avatar use it
# id, Name, icon colour; every app runs its own executable, which records its own file name
FIXTURES = [("recorder-one", "Recorder One", "#2e7d32"), ("recorder-six", "Recorder Six", "#c62828"), ("recorder-two", "Recorder Two", "#1565c0"),
            ("term-a", "Terminal", "#6d4c41"), ("term-b", "Terminal", "#00838f")]
for ident, name, colour in FIXTURES:
    exe = fixture_bin / ident
    exe.write_text('#!/bin/sh\nprintf "%s\\n" "$(basename "$0")" >> "$ACTION_CALLS"\n')
    exe.chmod(0o755)
    icon = root / f"{ident}.svg"
    icon.write_text(f'<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><rect x="2" y="2" width="60" height="60" rx="12" fill="{colour}"/></svg>')
    (apps / f"{ident}.desktop").write_text(f"[Desktop Entry]\nType=Application\nName={name}\nIcon={icon}\nExec={exe}\nCategories=Utility;\n")
state = Path(env["XDG_STATE_HOME"]) / "decebal"
state.mkdir(parents=True)
now = int(time.time())
# "app:Terminal" is legacy and ambiguous (most recent, many uses): it must help neither Terminal.
HISTORY = f"9\t{now}\tapp:Terminal\n2\t{now - 60}\tappid:term-b\n"
shell = Path(env["DECEBAL_CONFIG"])
(shell / "decebal.json").write_text(json.dumps({"theme": {"name": "macos", "mode": "dark", "accent": "#ff00ff"}, "launcher": {"style": "windows"}}))
(shell / "launcher-pinned.json").write_text(json.dumps({"pinned": ["recorder-one"]}))
shutil.copy2(bins / "declaunch", fixture_bin / "declaunch")
launcher = fixture_bin / "declaunch"
owned = []
records = {"commit": subprocess.run(["git", "-C", str(repo), "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip(),
           "dirty": bool(subprocess.run(["git", "-C", str(repo), "status", "--porcelain"], capture_output=True, text=True).stdout.strip()),
           "declaunch_sha256": hashlib.sha256(launcher.read_bytes()).hexdigest(), "screen": [800, 480], "xft_dpi": 192, "panel": "top 64 px", "steps": []}


def wait(fn, what, seconds=10):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        v = fn()
        if v:
            return v
        time.sleep(0.03)
    raise AssertionError(what)


def run(*args, check=True):
    return subprocess.run(list(map(str, args)), env=env, check=check, capture_output=True, text=True, timeout=20).stdout.strip()


def window():
    return run("xdotool", "search", "--onlyvisible", "--class", "declaunch", check=False).split()


def capture(out):
    last = None
    for _ in range(40):
        run("import", "-window", "root", out)
        data = Image.open(out).convert("RGB").tobytes()
        if data == last:
            return Image.open(out).convert("RGB")
        last = data
        time.sleep(0.15)
    raise AssertionError(f"{out}: the picture never settled")


def ran():
    return [l for l in calls.read_text().splitlines() if l]


def pins():
    return json.loads((shell / "launcher-pinned.json").read_text())["pinned"]


def open_and_search(query):
    # every step starts from the same history, so a launch in one step cannot reorder the next one's results
    (state / "spotlight-history").write_text(HISTORY)
    p = subprocess.Popen([str(launcher), "open", "--edge", "top", "--anchor", "400,32", "--thickness", "64"], env=env, stdout=(root / "declaunch.log").open("a"), stderr=subprocess.STDOUT, start_new_session=True)
    owned.append(p)
    wait(window, "the launcher did not map")
    time.sleep(0.3)
    run("xdotool", "type", "--delay", "25", query)
    time.sleep(0.3)
    return p


def find(img, colour, leftmost=True):
    px = img.load()
    hits = [(x, y) for y in range(img.size[1]) for x in range(img.size[0]) if px[x, y] == colour]
    assert hits, f"colour {colour} not on screen"
    return min(hits) if leftmost else hits[0]


def actions(img):
    """The Open and Pin glyphs of the pane: small accent-coloured blobs, in reading order (Open first)."""
    px = img.load()
    w, h = img.size
    # glyph edges blend with the button, so accept near-accent pixels
    near = lambda c: c[0] >= 230 and c[1] <= 40 and c[2] >= 230
    pts = {(x, y) for y in range(h) for x in range(w) if near(px[x, y])}
    blobs, large = [], []
    while pts:
        seed = pts.pop()
        blob, todo = [seed], [seed]
        while todo:
            x, y = todo.pop()
            for n in ((x + dx, y + dy) for dx in (-2, -1, 0, 1, 2) for dy in (-2, -1, 0, 1, 2)):
                if n in pts:
                    pts.remove(n)
                    blob.append(n)
                    todo.append(n)
        xs, ys = [p[0] for p in blob], [p[1] for p in blob]
        bw, bh = max(xs) - min(xs) + 1, max(ys) - min(ys) + 1
        # glyphs are small; the search underline is long and thin, the avatar a large disc
        box = (min(xs), min(ys), max(xs), max(ys))
        (blobs if bw <= 40 and bh <= 40 and len(blob) < 900 else large).append(box)
    # bits inside a large accent shape (the avatar disc around its initial) are not glyphs
    blobs = [b for b in blobs if not any(l[0] <= b[0] and l[1] <= b[1] and b[2] <= l[2] and b[3] <= l[3] for l in large)]
    blobs.sort(key=lambda b: (round(b[1] / 12), b[0]))
    assert len(blobs) == 2, f"expected the Open and Pin glyphs, found {blobs}"
    return blobs


try:
    with (root / "xvfb.log").open("w") as log:
        x, env["DISPLAY"] = start_private_xvfb(env=env, log=log, screen="800x480x24")
    owned.append(x)
    subprocess.run(["xrdb", "-merge"], input="Xft.dpi: 192\n", env=env, text=True, check=True)
    desk = Image.new("RGB", (800, 480), (0x5A, 0x6A, 0x7A))
    desk.paste((0x1E, 0x22, 0x28), (0, 0, 800, 64))
    desk.save(root / "desk.png")
    owned.append(subprocess.Popen(["display", "-immutable", "-borderwidth", "0", "-geometry", "+0+0", "-title", "desk", str(root / "desk.png")], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True))
    wait(lambda: run("xdotool", "search", "--onlyvisible", "--name", "desk", check=False), "desktop stand-in")

    # A. keyboard: second result, Tab to Open, Enter
    p = open_and_search("recorder")
    run("xdotool", "key", "Down")
    run("xdotool", "key", "Tab")
    capture(evidence / "a-keyboard-open-focused.png")
    run("xdotool", "key", "Return")
    assert p.wait(timeout=8) == 0
    wait(lambda: ran(), "nothing ran")
    assert ran() == ["recorder-six"], ran()
    records["steps"].append({"step": "keyboard Open on result 2", "ran": ran()[-1]})

    # B. keyboard: second result, Tab, then to Pin (Right: the short pane puts Open and Pin side by side), Enter
    p = open_and_search("recorder")
    run("xdotool", "key", "Down")
    run("xdotool", "key", "Tab")
    run("xdotool", "key", "Right")
    capture(evidence / "b-keyboard-pin-focused.png")
    run("xdotool", "key", "Return")
    wait(lambda: "recorder-six" in pins(), "Pin did not pin the second result")
    assert pins() == ["recorder-one", "recorder-six"], pins()
    run("xdotool", "key", "Escape")
    assert p.wait(timeout=8) == 0
    records["steps"].append({"step": "keyboard Pin on result 2", "pinned": pins()})

    # C. pointer: hover the second result (its own icon colour), click Open as drawn on screen
    p = open_and_search("recorder")
    img = capture(evidence / "c-before-wheel.png")
    # a short menu shows one result row: wheel the list down to the second result, as a user would
    rx, ry = find(img, (0x2E, 0x7D, 0x32))
    run("xdotool", "mousemove", rx + 6, ry + 6)
    run("xdotool", "click", "5")
    time.sleep(0.3)
    img = capture(evidence / "c-after-wheel.png")
    sx, sy = find(img, (0xC6, 0x28, 0x28))
    run("xdotool", "mousemove", sx + 6, sy + 6)
    time.sleep(0.3)
    img = capture(evidence / "c-pointer-hover-second.png")
    open_glyph, pin_glyph = actions(img)
    run("xdotool", "mousemove", open_glyph[2] + 24, (open_glyph[1] + open_glyph[3]) // 2)
    time.sleep(0.2)
    run("xdotool", "click", "1")
    assert p.wait(timeout=8) == 0
    wait(lambda: len(ran()) == 2, "the pointer Open ran nothing")
    assert ran()[-1] == "recorder-six", ran()
    records["steps"].append({"step": "pointer Open on result 2", "ran": ran()[-1], "open_glyph": open_glyph, "pin_glyph": pin_glyph})

    # D. pointer: Pin (now "Unpin") toggles exactly that app back off
    p = open_and_search("recorder")
    img = capture(evidence / "d-before-wheel.png")
    rx, ry = find(img, (0x2E, 0x7D, 0x32))
    run("xdotool", "mousemove", rx + 6, ry + 6)
    run("xdotool", "click", "5")
    time.sleep(0.3)
    img = capture(evidence / "d-after-wheel.png")
    sx, sy = find(img, (0xC6, 0x28, 0x28))
    run("xdotool", "mousemove", sx + 6, sy + 6)
    time.sleep(0.3)
    img = capture(evidence / "d-pointer-hover-second.png")
    _, pin_glyph = actions(img)
    run("xdotool", "mousemove", pin_glyph[2] + 24, (pin_glyph[1] + pin_glyph[3]) // 2)
    time.sleep(0.2)
    run("xdotool", "click", "1")
    wait(lambda: "recorder-six" not in pins(), "the pointer Unpin did not unpin the second result")
    assert pins() == ["recorder-one"], pins()
    run("xdotool", "key", "Escape")
    assert p.wait(timeout=8) == 0
    records["steps"].append({"step": "pointer Unpin on result 2", "pinned": pins()})

    # E. two apps named Terminal: history by desktop id ranks term-b first; Enter runs term-b, not term-a
    p = open_and_search("terminal")
    capture(evidence / "e-duplicate-names.png")
    run("xdotool", "key", "Return")
    assert p.wait(timeout=8) == 0
    wait(lambda: len(ran()) == 3, "Enter on Terminal ran nothing")
    assert ran()[-1] == "term-b", ran()
    history = (state / "spotlight-history").read_text()
    assert "appid:term-b" in history and history.count("app:Terminal") == 1, history
    records["steps"].append({"step": "duplicate Name=Terminal follows the desktop-id history", "ran": ran()[-1]})
    records["passed"] = True
finally:
    for p in reversed(owned):
        if p.poll() is None:
            p.send_signal(signal.SIGTERM)
            try:
                p.wait(timeout=5)
            except subprocess.TimeoutExpired:
                p.kill()
    records["ran"] = ran()
    (evidence / "result.json").write_text(json.dumps(records, indent=1) + "\n")
    print(json.dumps(records, indent=1))
    shutil.rmtree(root, ignore_errors=True)
