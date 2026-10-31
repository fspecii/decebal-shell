#!/usr/bin/env python3
"""Native gate for Settings > Widgets > Create widget: the real `decsettings` and `decwidgets` on an owned private X server.

Usage (see docs/widget-creator-native.md for the exact root commands and the limits of this gate):
  python3 -I tests/e2e/widget-creator-native.py --bin-dir IMMUTABLE_BIN_DIR --out NEW_SHORT_EVIDENCE_DIR [--extra-bin NAME ...]

SOURCE-ONLY CANDIDATE: reviewed with py_compile and the pure-Python self-test (widget-creator-native-test.py); the native run has NOT been executed.

What a pass proves (and the checks that make each claim):
* Navigation and form: the sidebar is clicked to open Widgets; "Create widget..." opens the form; the controls come from the real control rectangles that
  `DECSETTINGS_DEBUG_LAYOUT` writes after every paint, and every input is XTEST/xdotool (pointer clicks, wheel, keys, typing). The preview is the item painted above
  the form: it must hold real pixels and must change when the type or the toggles change.
* Create a clock (12-hour, no date) and a note (type chosen in the popup by type-ahead, large size, nudged offset): the `<id>.toml` file is parsed as TOML and has the
  typed name, the chosen options and no commands; `decebal.json` gets exactly the new card (the form's own anchor/x/y); every earlier card, unknown key and other section
  stays byte-equal as JSON; `decwidgets status --json` becomes ready with no broken card and every item without a definition error.
* Asynchronous transaction regression (the reason this gate exists): with the PRIVATE config directory made unwritable (mode 0500, the widgets directory stays writable),
  Create runs the worker transaction which writes the TOML and then fails the JSON commit. The gate waits for the visible error notice (the evidence that the worker ran
  past the TOML write; nothing is asserted before it), then requires the form to be still open with the typed draft, the new TOML to be gone, `decebal.json` byte-equal and
  no stray temp file. Restoring the mode and pressing Create again must succeed under the SAME file name (so the failed attempt really freed it).

Isolation: an owned Xvfb on a high display (private_x11.py; both listening sockets verified as this child's before EVERY X command), no window manager, dead session and
system buses, absent audio and rdwm, HOME/XDG/DECEBAL_* inside the evidence directory, a PATH of fake action tools and read-only fontconfig links only, private copies of the
binaries (the source directory is hashed before and after and must be unchanged), run-token environment on every child. Children are stopped through pidfds; SIGKILL, a leftover
token process, a changed source binary or an interrupt fails the run. SIGTERM/SIGINT unwind through the same cleanup. Nothing is ever deleted: the evidence directory must be new.
"""
import argparse
import hashlib
import importlib.util
import json
import os
import select
import shutil
import signal
import stat
import subprocess
import sys
import threading
import time
import tomllib
import traceback
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
REQUIRED_BINS = ("decsettings", "decwidgets")
FAKE_TOOLS = ("xdg-open", "gio", "peony", "notify-send", "loginctl", "systemctl")
FONT_TOOLS = ("fc-list", "fc-match")
TOKEN_VAR = "DECEBAL_NATIVE_TOKEN"
# the longest private unix socket (Settings' navigation endpoint) must fit sockaddr_un (107 bytes) or Settings falls back to /tmp, outside the private tree
SOCKET_TAIL = "r/run/decsettings-1000/ffffffffffffffff.sock"
TOP_MARGIN, BOTTOM_MARGIN = 95, 70


class Terminated(BaseException):
    """SIGTERM/SIGINT converted into an unwinding exception so the one cleanup path runs."""

    def __init__(self, signum):
        super().__init__(signum)
        self.signum = signum


def sha256_file(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def fail(message):
    raise AssertionError(message)


# ---------------------------------------------------------------------------------------------------------------- argument validation
def validate_bin_dir(arg, out=None, extra=()):
    """The explicit, immutable candidate directory: absolute, a real directory nobody else can write, regular executable files only. Returns {name: path}."""
    raw = Path(arg)
    if not raw.is_absolute():
        fail(f"--bin-dir must be an absolute path: {arg}")
    if raw.is_symlink():
        fail(f"--bin-dir must not be a symlink: {arg}")
    directory = raw.resolve()
    if not directory.is_dir():
        fail(f"--bin-dir is not a directory: {directory}")
    mode = directory.stat()
    if mode.st_mode & (stat.S_IWGRP | stat.S_IWOTH):
        fail(f"--bin-dir is writable by group/others, so it is not immutable: {directory}")
    if mode.st_uid not in (0, os.getuid()):
        fail(f"--bin-dir belongs to uid {mode.st_uid}, not to this user or root: {directory}")
    if out is not None:
        out = Path(out).resolve()
        if out == directory or out in directory.parents or directory in out.parents:
            fail(f"--out and --bin-dir must not contain one another: {out} / {directory}")
    found = {}
    for name in (*REQUIRED_BINS, *extra):
        path = directory / name
        if path.is_symlink() or not path.is_file():
            fail(f"{path} is not a regular file")
        if not os.access(path, os.X_OK):
            fail(f"{path} is not executable")
        if path.stat().st_mode & (stat.S_IWGRP | stat.S_IWOTH):
            fail(f"{path} is writable by group/others")
        found[name] = path
    return found


def fresh_out(arg):
    """A NEW evidence directory (never reused, never deleted) whose private socket paths stay short. Returns the created absolute path."""
    out = Path(arg)
    if not out.is_absolute():
        fail(f"--out must be an absolute path: {arg}")
    if os.path.lexists(out):
        fail(f"--out already exists; a fresh directory is required: {out}")
    if not out.parent.is_dir():
        fail(f"the parent of --out does not exist: {out.parent}")
    longest = len(str(out / SOCKET_TAIL).encode())
    if longest > 100:
        fail(f"--out is too long for the private Settings socket ({longest} bytes, limit 100); use a shorter path such as /var/tmp/dwc-<sha7>-<HHMMSS>")
    out.mkdir(mode=0o700)
    return out


# ---------------------------------------------------------------------------------------------------------------- process ownership
def wait_pidfd(fd, seconds):
    poller = select.poll()
    poller.register(fd, select.POLLIN)
    return bool(poller.poll(int(seconds * 1000)))


def stop_pid(pid, *, grace=6, verify=None):
    """SIGTERM through a pidfd (never a recycled pid). Returns 'gone', 'term' (exited after SIGTERM) or 'forced' (needed SIGKILL).
    `verify(pid)` runs after the pidfd pins the process; a process that is not the one expected is never signalled."""
    try:
        fd = os.pidfd_open(pid)
    except ProcessLookupError:
        return "gone"
    try:
        if verify is not None and not verify(pid):
            fail(f"pid {pid} is not the process this run owns; not signalling it")
        signal.pidfd_send_signal(fd, signal.SIGTERM)
        if wait_pidfd(fd, grace):
            return "term"
        signal.pidfd_send_signal(fd, signal.SIGKILL)
        wait_pidfd(fd, grace)
        return "forced"
    finally:
        os.close(fd)


def scan_token(token, *, proc="/proc", uid=None, exclude=()):
    """Pids of this user's processes whose environment carries the run token."""
    uid = os.getuid() if uid is None else uid
    mark = f"{TOKEN_VAR}={token}".encode()
    found = []
    for name in os.listdir(proc):
        if not name.isdigit() or int(name) in exclude:
            continue
        directory = Path(proc) / name
        try:
            if directory.stat().st_uid != uid:
                continue
            if mark in (directory / "environ").read_bytes().split(b"\0"):
                found.append(int(name))
        except OSError:
            continue
    return sorted(found)


def exe_of(pid, proc="/proc"):
    try:
        return os.readlink(Path(proc) / str(pid) / "exe")
    except OSError:
        return None


class Ledger:
    """Everything irregular that happened to the processes of one run. Any entry fails the run."""

    def __init__(self):
        self.forced, self.leftovers, self.unexpected, self.events = [], [], [], []

    def problems(self):
        out = []
        if self.forced:
            out.append(f"forced cleanup (SIGKILL) of {self.forced}")
        if self.leftovers:
            out.append(f"leftover token processes {self.leftovers}")
        if self.unexpected:
            out.append(f"unexpected token processes {self.unexpected}")
        return out


# ---------------------------------------------------------------------------------------------------------------- pure verification
def deep_copy(value):
    return json.loads(json.dumps(value))


INSTANCE_KEYS = {"use", "place", "anchor", "x", "y", "monitor", "enabled", "options"}


def check_new_item(item, *, use, anchor, x, y):
    if item.get("use") != use:
        fail(f"new card uses {item.get('use')!r}, expected {use!r}")
    if item.get("place") != "desktop":
        fail(f"new card place {item.get('place')!r}, expected 'desktop'")
    if (item.get("anchor"), item.get("x"), item.get("y")) != (anchor, x, y):
        fail(f"new card is at {(item.get('anchor'), item.get('x'), item.get('y'))}, the form showed {(anchor, x, y)}")
    if item.get("enabled") is False:
        fail("the new card was saved disabled")
    extra = set(item) - INSTANCE_KEYS
    if extra:
        fail(f"the new card carries unexpected keys {sorted(extra)}")


def verify_config(before, after, new_cards):
    """`after` is `before` plus exactly the `new_cards` ({use, anchor, x, y}) appended to widgets.items; widgets.enabled is now true; nothing else changed
    (unknown top-level keys, other sections, unknown widgets keys, and every earlier card with its unknown fields are equal)."""
    b, a = deep_copy(before), deep_copy(after)
    bw, aw = b.get("widgets", {}), a.get("widgets")
    if not isinstance(aw, dict):
        fail("the widgets object is missing after Create")
    prior, items = bw.get("items", []), aw.get("items", [])
    if len(items) != len(prior) + len(new_cards):
        fail(f"{len(items)} cards after Create, expected {len(prior)} + {len(new_cards)}")
    if items[: len(prior)] != prior:
        fail("an earlier card changed")
    for item, spec in zip(items[len(prior):], new_cards):
        check_new_item(item, **spec)
    if aw.get("enabled") is not True:
        fail(f"widgets.enabled is {aw.get('enabled')!r} after Create, expected true")
    for doc in (bw, aw):
        doc.pop("items", None)
        doc.pop("enabled", None)
    if bw != aw:
        fail(f"the other widgets settings changed: {bw} -> {aw}")
    b.pop("widgets", None)
    a.pop("widgets", None)
    if a != b:
        fail(f"unknown top-level keys or other sections changed: {b} -> {a}")


def walk(value):
    """Every (key, value) pair of a parsed TOML document, recursively."""
    if isinstance(value, dict):
        for key, inner in value.items():
            yield key, inner
            yield from walk(inner)
    elif isinstance(value, list):
        for inner in value:
            yield from walk(inner)


def check_widget_toml(text, stem, name, *, have=(), lack=(), widths=()):
    data = tomllib.loads(text)
    if data.get("format") != 1 or data.get("id") != stem or data.get("name") != name:
        fail(f"widget file header is {(data.get('format'), data.get('id'), data.get('name'))}, expected (1, {stem!r}, {name!r})")
    for word in have:
        if word not in text:
            fail(f"widget file lacks {word!r}")
    for word in lack:
        if word in text:
            fail(f"widget file unexpectedly holds {word!r}")
    keys = {key for key, _ in walk(data)}
    if keys & {"exec", "command", "commands"}:
        fail(f"widget file runs commands: {sorted(keys & {'exec', 'command', 'commands'})}")
    for width in widths:
        if not any(k == "width" and isinstance(v, (int, float)) and abs(v - width) < 0.05 for k, v in walk(data)):
            fail(f"no node of width {width} (the chosen size) in the widget file")
    return data


def initial_config():
    """Cards and keys the gate must preserve: unknown fields everywhere, a disabled card, other sections."""
    return {
        "zz_unknown_top": {"keep": ["me", 1, {"deep": True}]},
        "theme": {"name": "windows11", "palette": "nord", "future": "keep"},
        "dock": {"size": 48, "future_dock": "keep"},
        "widgets": {
            "enabled": False,
            "allow_exec": False,
            "future": {"nested": [1, 2, {"a": None}]},
            "items": [
                {"use": "builtin:clock-card", "anchor": "bottom-left", "x": 21, "y": -9, "options": {"sentinel": "keep"}, "future_item": 17},
                {"use": "builtin:storage-card", "anchor": "top-left", "x": 40, "y": 40, "enabled": False, "future_item": ["keep"]},
            ],
        },
    }


def changed_area(a, b):
    """Changed bounding-box area in the common prefix, allowing preview heights to change."""
    from PIL import ImageChops
    w, h = min(a.width, b.width), min(a.height, b.height)
    assert w > 0 and h > 0, "empty preview comparison"
    a, b = a.crop((0, 0, w, h)), b.crop((0, 0, w, h))
    box = ImageChops.difference(a, b).convert("L").point(lambda v: 255 if v > 3 else 0).getbbox()
    return 0 if box is None else (box[2] - box[0]) * (box[3] - box[1])


def region_has_content(image, minimum=6):
    colours = image.getcolors(maxcolors=1 << 22)
    return colours is None or len(colours) >= minimum


# ---------------------------------------------------------------------------------------------------------------- the gate
def load_helper():
    path = Path(__file__).resolve().parent / "private_x11.py"
    spec = importlib.util.spec_from_file_location("private_x11", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    for name in ("start_private_xvfb", "listeners_owned", "close"):
        assert hasattr(module, name), f"{path} lacks {name}"
    return module, path


class Gate:
    def __init__(self, x11, out, token):
        self.x11, self.out, self.token = x11, out, token
        self.ledger = Ledger()
        self.server = self.number = self.display = self.settings = None
        self.env = {}
        self.layout_path = None
        self.private = None
        self.steps = []
        self.tools = {name: shutil.which(name) for name in ("xdotool", "import", "Xvfb")}
        missing = [n for n, p in self.tools.items() if not p]
        if missing:
            fail(f"missing host tools: {missing}")
        self.logs = []
        self.wid = None

    # -- evidence
    def step(self, name, **info):
        self.steps.append({"step": name, "t": round(time.monotonic(), 3), **info})
        print("STEP", name, flush=True)

    # -- X access: every command re-verifies that this run's Xvfb child is alive and owns both listening sockets
    def owned(self):
        if self.server is None or self.server.poll() is not None:
            fail("the owned private Xvfb is not running")
        if not self.x11.listeners_owned(self.server, self.number):
            fail("the X listeners (filesystem and abstract) are no longer this run's Xvfb child")
        if self.number < 700:
            fail(f"display :{self.number} is not a reserved high display")

    def x(self, *argv, check=True, timeout=30):
        self.owned()
        argv = (self.tools.get(argv[0], argv[0]), *argv[1:])
        done = subprocess.run(argv, env=self.env, capture_output=True, text=True, timeout=timeout)
        if check and done.returncode != 0:
            fail(f"{Path(argv[0]).name} failed ({done.returncode}): {done.stderr.strip()}")
        return done.stdout.strip()

    # -- observation
    def layout(self):
        return json.loads(self.layout_path.read_text())

    def ctl(self, cid, state=None):
        for c in (state or self.layout())["ctls"]:
            if c["id"] == cid:
                return c
        return None

    def wait(self, test, label, seconds=15, pause=0.05):
        end = time.monotonic() + seconds
        last = None
        while time.monotonic() < end:
            try:
                value = test()
                if value:
                    return value
            except (OSError, ValueError, KeyError, StopIteration, TypeError, IndexError) as error:
                last = error
            time.sleep(pause)
        fail(f"timed out after {seconds}s: {label}" + (f" (last read error: {last})" if last else ""))

    def window_height(self):
        out = self.x("xdotool", "getwindowgeometry", "--shell", self.wid)
        return int(dict(line.split("=") for line in out.splitlines())["HEIGHT"])

    def window_width(self):
        out = self.x("xdotool", "getwindowgeometry", "--shell", self.wid)
        return int(dict(line.split("=") for line in out.splitlines())["WIDTH"])

    # -- input (XTEST through xdotool)
    def mouse(self, x, y, button=1):
        self.x("xdotool", "mousemove", "--window", self.wid, str(int(x)), str(int(y)), "click", str(button))
        time.sleep(0.2)

    def key(self, *keys):
        self.x("xdotool", "key", "--clearmodifiers", *keys)
        time.sleep(0.15)

    def type(self, text):
        self.x("xdotool", "type", "--clearmodifiers", "--delay", "40", text)
        time.sleep(0.15)

    def reveal(self, cid):
        """Scrolls with the wheel until the control's rectangle is fully inside the visible page; returns its (current) rectangle."""
        for _ in range(40):
            c = self.ctl(cid)
            if c is None:
                fail(f"control {cid} is not on the page")
            x, y, w, h = c["rect"]
            height = self.window_height()
            if y >= TOP_MARGIN and y + h <= height - BOTTOM_MARGIN:
                return c["rect"]
            below = y + h > height - BOTTOM_MARGIN
            self.mouse(self.content_x(), height / 2, 5 if below else 4)
            time.sleep(0.2)
        fail(f"control could not be scrolled into view: {cid}")

    def content_x(self):
        state = self.layout()
        sidebar = max(p["rect"][0] + p["rect"][2] for p in state["panes"])
        return sidebar + (self.window_width() - sidebar) / 2

    def click(self, cid, part=None):
        x, y, w, h = self.reveal(cid)
        if part is not None:
            x, y, w, h = self.ctl(cid)["parts"][part]
        self.mouse(x + w / 2, y + h / 2)

    def scroll_top(self):
        for _ in range(40):
            if self.layout()["scroll"] <= 0.5:
                return
            self.mouse(self.content_x(), self.window_height() / 2, 4)
            time.sleep(0.2)
        fail("could not scroll the page to the top")

    def set_text(self, cid, text):
        self.click(cid)
        time.sleep(0.2)
        self.key("ctrl+u")          # clears the field (ctrl+a is "home" in Settings text fields)
        self.type(text)
        self.key("Return")
        self.wait(lambda: self.ctl(cid)["value"] == text, f"{cid} did not take the typed text {text!r}")

    def shot(self, name):
        path = self.out / "shots" / f"{name}.png"
        self.x("import", "-window", self.wid, str(path))
        return path

    def preview_shot(self, name):
        from PIL import Image
        # At the pinned 1x UI scale, the enabled toggle is followed by the rest
        # of its row, the card gap and the 28px Create header. The Type row is
        # below the preview and its caption. Insets exclude header/caption ink.
        # Recompute after every template
        # change: clocks and notes have different preview heights.
        layout = self.layout()
        sidebar_right = max(p["rect"][0] + p["rect"][2] for p in layout["panes"])
        _, enabled_y, _, enabled_h = self.ctl("wg:enabled")["rect"]
        template_y = self.ctl("wg:new:template")["rect"][1]
        box = (int(sidebar_right) + 8, int(enabled_y + enabled_h + 68),
               self.window_width() - 8, int(template_y - 56))
        assert box[3] - box[1] >= 40, f"preview band is too short: {box}"
        assert 0 <= box[1] < box[3] <= self.window_height(), f"preview is clipped: {box}"
        return Image.open(self.shot(name)).convert("RGB").crop(box)

    # -- cleanup
    def stop_popen(self, name, proc, grace=6):
        if proc is None or proc.poll() is not None:
            return
        result = stop_pid(proc.pid, grace=grace)
        if result == "forced":
            self.ledger.forced.append(name)
        try:
            proc.wait(timeout=grace)
        except subprocess.TimeoutExpired:
            self.ledger.forced.append(name + " (not reaped)")

    def host_pids(self):
        return scan_token(self.token, exclude=(os.getpid(),))

    def close(self):
        """One cleanup path: Settings, then every other token process (the desktop host it started), then Xvfb, then proof nothing is left."""
        try:
            try:
                self.stop_popen("decsettings", self.settings)
            except Exception as error:
                self.ledger.unexpected.append(f"Settings cleanup: {error}")
            for pid in self.host_pids():
                if self.server is not None and pid == self.server.pid:
                    continue  # Xvfb inherits the token; its owned handle is stopped below.
                try:
                    exe = exe_of(pid)
                    expected = exe is not None and Path(exe) == self.private / "bin" / "decwidgets"
                    if exe is None:
                        continue
                    if not expected:
                        self.ledger.unexpected.append(f"{pid}:{exe}")
                    self.ledger.events.append(f"stopped {'host' if expected else 'stray'} {pid} exe={exe}")
                    if stop_pid(pid, verify=self.token_matches) == "forced":
                        self.ledger.forced.append(f"pid {pid}")
                except Exception as error:
                    self.ledger.unexpected.append(f"pid {pid} cleanup: {error}")
        finally:
            if self.server is not None:
                try:
                    self.stop_popen("Xvfb", self.server)
                finally:
                    self.x11.close(self.server)
        deadline = time.monotonic() + 3
        while scan_token(self.token, exclude=(os.getpid(),)) and time.monotonic() < deadline:
            time.sleep(0.05)
        self.ledger.leftovers = scan_token(self.token, exclude=(os.getpid(),))
        for handle in self.logs:
            handle.close()

    def token_matches(self, pid):
        return pid in scan_token(self.token, exclude=(os.getpid(),))


# ---------------------------------------------------------------------------------------------------------------- private environment
def build_private(private, bins, extra):
    """Directories, copied binaries, fake action tools and read-only font tools. Everything lives under `private` (inside the evidence directory)."""
    for d in ("home", "cfg", "xdgcfg", "cache", "data", "state", "run", "widgets", "themes", "bin", "no-data"):
        (private / d).mkdir(parents=True, exist_ok=True)
    (private / "run").chmod(0o700)
    hashes = {}
    for name, source in bins.items():
        shutil.copy2(source, private / "bin" / name)
        hashes[name] = {"source": str(source), "sha256": sha256_file(source), "copy_sha256": sha256_file(private / "bin" / name)}
    actions = private / "actions"
    actions.write_text("")
    for tool in FAKE_TOOLS:
        path = private / "bin" / tool
        path.write_text(f'#!/bin/sh\necho "{tool} $*" >> "{actions}"\nexit 0\n')
        path.chmod(0o755)
    for tool in FONT_TOOLS:
        found = shutil.which(tool)
        if not found:
            fail(f"{tool} is required for font discovery")
        (private / "bin" / tool).symlink_to(found)
    return actions, hashes


def base_env(private, token):
    """Built from nothing: no host DISPLAY/WAYLAND/XAUTHORITY/RDWM/audio, both buses dead, PATH holds only the private bin."""
    return {
        "HOME": str(private / "home"), "XDG_CONFIG_HOME": str(private / "xdgcfg"), "XDG_CACHE_HOME": str(private / "cache"), "XDG_DATA_HOME": str(private / "data"),
        "XDG_DATA_DIRS": str(private / "no-data"), "XDG_CONFIG_DIRS": str(private / "no-data"), "XDG_STATE_HOME": str(private / "state"), "XDG_RUNTIME_DIR": str(private / "run"),
        "DECEBAL_CONFIG": str(private / "cfg"), "DECEBAL_WIDGETS": str(private / "widgets"), "DECEBAL_THEMES": str(private / "themes"), "DECEBAL_SYSTEM_THEMES": str(private / "themes"),
        "DECEBAL_BACKEND": "x11", "DECEBAL_SYSTEM_AUTOMATIC": "0", "DECSETTINGS_SCALE": "1", "DECSETTINGS_DEBUG_LAYOUT": str(private / "layout.json"),
        "DBUS_SESSION_BUS_ADDRESS": f"unix:path={private}/no-session-bus", "DBUS_SYSTEM_BUS_ADDRESS": f"unix:path={private}/no-system-bus",
        "PULSE_SERVER": f"unix:{private}/no-pulse", "PIPEWIRE_RUNTIME_DIR": str(private / "no-pipewire"), "RDWMCTL": str(private / "no-rdwmctl"),
        "PATH": str(private / "bin"), "LANG": "C.UTF-8", TOKEN_VAR: token,
    }


def status(gate):
    """The desktop host's own report, queried with the copied decwidgets under the same private environment."""
    done = subprocess.run([str(gate.private / "bin" / "decwidgets"), "status", "--json"], env=gate.env, capture_output=True, text=True, timeout=20)
    if done.returncode != 0:
        fail(f"decwidgets status failed: {done.stderr.strip()}")
    return json.loads(done.stdout)


def directory_listing(path):
    return sorted(p.name for p in Path(path).iterdir())


def shell_json(gate):
    path = gate.private / "cfg" / "decebal.json"
    return path.read_bytes()


class Watcher(threading.Thread):
    """Records every file name that ever appears in the widgets directory (the TOML written before the JSON commit fails). Informational: a fast worker may be missed."""

    def __init__(self, directory, baseline):
        super().__init__(daemon=True)
        self.directory, self.baseline, self.seen, self.stop_event = Path(directory), set(baseline), set(), threading.Event()

    def run(self):
        while not self.stop_event.is_set():
            try:
                self.seen |= set(os.listdir(self.directory)) - self.baseline
            except OSError:
                pass
            time.sleep(0.0005)

    def finish(self):
        self.stop_event.set()
        self.join(timeout=2)
        return sorted(self.seen)


def run_flow(gate, hashes):
    from PIL import Image
    private, widgets, cfg = gate.private, gate.private / "widgets", gate.private / "cfg"
    config_path = cfg / "decebal.json"
    original = initial_config()
    config_path.write_text(json.dumps(original, indent=2) + "\n")
    config_path.chmod(0o600)
    report = {"cases": {}}

    # ---- the desktop is up and Settings opens on its default pane
    gate.step("start decsettings")
    log = (gate.out / "logs" / "decsettings.log").open("w")
    gate.logs.append(log)
    gate.settings = subprocess.Popen([str(private / "bin" / "decsettings")], env=gate.env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    gate.wid = gate.wait(lambda: (lambda found: found[0] if len(found) == 1 else None)(gate.x("xdotool", "search", "--onlyvisible", "--name", "^System Settings$", check=False).split()), "exactly one visible System Settings window")
    owner = gate.x("xdotool", "getwindowpid", gate.wid, check=False)
    if owner:
        assert int(owner) == gate.settings.pid, f"the Settings window belongs to pid {owner}, not this run's decsettings {gate.settings.pid}"
    report["window"] = {"id": gate.wid, "pid_property": owner or None}
    gate.x("xdotool", "windowfocus", gate.wid)   # there is no window manager to give keyboard focus
    gate.layout_path = private / "layout.json"
    gate.wait(lambda: gate.layout()["panes"], "the sidebar was laid out")
    report["host_before"] = status(gate)
    assert report["host_before"]["state"] == "down", f"the desktop host was already up before the first Create: {report['host_before']}"

    # ---- sidebar navigation to Widgets by pointer
    gate.step("open Widgets from the sidebar")
    for _ in range(30):
        entry = next(p for p in gate.layout()["panes"] if p["id"] == "widgets")
        if entry["visible"]:
            break
        # Startup follows the selected General row, so Widgets can start
        # above the viewport. Never assume the sidebar begins at its top.
        search = gate.layout()["search"]
        above = entry["rect"][1] < search[1] + search[3] + 16
        gate.mouse(entry["rect"][2] / 2, gate.window_height() / 2, 4 if above else 5)
    else:
        fail("the Widgets sidebar entry could not be revealed")
    x, y, w, h = entry["rect"]
    gate.mouse(x + w / 2, y + h / 2)
    gate.wait(lambda: gate.layout()["pane"] == "widgets" and gate.ctl("wg:new:open"), "the sidebar click did not open the Widgets pane")
    assert gate.ctl("wg:new:create") is None, "the creator form was open before Create widget... was pressed"
    assert gate.layout()["warnings"] == [], "warnings on the closed creator"

    # ---- open the form
    gate.step("open the creator form")
    gate.click("wg:new:open")
    gate.wait(lambda: gate.ctl("wg:new:create"), "the creator form did not open")
    for needed in ("wg:new:template", "wg:new:name", "wg:new:size", "wg:new:anchor", "wg:new:x", "wg:new:y", "wg:new:cancel", "wg:new:h24", "wg:new:date"):
        assert gate.ctl(needed), f"form control {needed} is missing"
    state = gate.layout()
    assert gate.ctl("wg:new:template")["value"] == "clock" and gate.ctl("wg:new:name")["value"] == "My clock" and gate.ctl("wg:new:create")["enabled"], "unexpected default draft"
    assert state["warnings"] == [], state["warnings"]
    gate.scroll_top()
    preview_default = gate.preview_shot("form-clock-default")
    assert region_has_content(preview_default), "the preview area holds no painted content"

    # ---- case 1: a 12-hour clock without the date
    gate.step("create the clock")
    gate.set_text("wg:new:name", "Desk Clock")
    gate.click("wg:new:h24")
    gate.wait(lambda: gate.ctl("wg:new:h24")["value"] is False, "24-hour toggle did not turn off")
    gate.click("wg:new:date")
    gate.wait(lambda: gate.ctl("wg:new:date")["value"] is False, "date toggle did not turn off")
    gate.scroll_top()
    time.sleep(0.4)
    preview_toggled = gate.preview_shot("form-clock-12h")
    assert changed_area(preview_default, preview_toggled) >= 200, "the preview did not visibly change when the clock options changed"
    clock = {k: gate.ctl(f"wg:new:{k}")["value"] for k in ("anchor", "x", "y")}
    clock_before = json.loads(config_path.read_text())
    gate.click("wg:new:create")
    gate.wait(lambda: gate.ctl("wg:new:create") is None and gate.ctl("wg:new:open"), "the form did not close after Create")
    gate.wait(lambda: len(json.loads(config_path.read_text())["widgets"]["items"]) == 3, "decebal.json did not gain the clock card")
    after_clock = json.loads(config_path.read_text())
    verify_config(clock_before, after_clock, [{"use": "desk-clock", "anchor": clock["anchor"], "x": int(clock["x"]), "y": int(clock["y"])}])
    verify_config(original, after_clock, [{"use": "desk-clock", "anchor": clock["anchor"], "x": int(clock["x"]), "y": int(clock["y"])}])
    assert directory_listing(widgets) == ["desk-clock.toml"], directory_listing(widgets)
    check_widget_toml((widgets / "desk-clock.toml").read_text(), "desk-clock", "Desk Clock", have=("{clock:%I:%M %p}",), lack=("%H:%M", "%A"))
    ready = gate.wait(lambda: (lambda s: s if s["running"] and s["state"] == "ready" else None)(status(gate)), "the desktop host did not become ready after the clock was created", seconds=25, pause=0.3)
    assert ready["cards"]["broken"] == 0 and all(i["error"] is None for i in ready["items"]), ready
    assert any(i["use"] == "desk-clock" for i in ready["items"]), ready
    report["cases"]["clock"] = {"form": clock, "host": ready}

    # ---- case 2: a note, with the failing JSON commit first
    gate.step("open the form again for the note")
    gate.click("wg:new:open")
    gate.wait(lambda: gate.ctl("wg:new:create"), "the creator form did not reopen")
    gate.click("wg:new:template")
    time.sleep(0.4)
    gate.type("static")
    gate.key("Return")
    gate.wait(lambda: gate.ctl("wg:new:template")["value"] == "note", "the Type popup did not select Static note")
    gate.wait(lambda: gate.ctl("wg:new:text") and gate.ctl("wg:new:name")["value"] == "My note", "the note form did not appear with its default name")
    assert gate.ctl("wg:new:create")["enabled"] is False and any("Type the note" in w for w in gate.layout()["warnings"]), "Create was not blocked for an empty note"
    gate.set_text("wg:new:name", "Field Note")
    gate.set_text("wg:new:text", "Greetings from XTEST")
    gate.click("wg:new:size", part=2)
    gate.wait(lambda: gate.ctl("wg:new:size")["value"] == "large", "size Large was not chosen")
    x_before = gate.ctl("wg:new:x")["value"]
    gate.click("wg:new:x", part=1)
    gate.wait(lambda: gate.ctl("wg:new:x")["value"] > x_before, "the horizontal offset stepper did not move")
    gate.wait(lambda: gate.ctl("wg:new:create")["enabled"] and gate.layout()["warnings"] == [], "Create did not become available")
    gate.scroll_top()
    time.sleep(0.4)
    preview_note = gate.preview_shot("form-note")
    assert region_has_content(preview_note) and changed_area(preview_toggled, preview_note) >= 200, "the note preview is empty or equals the clock preview"
    note = {k: gate.ctl(f"wg:new:{k}")["value"] for k in ("anchor", "x", "y")}
    draft = {k: gate.ctl(f"wg:new:{k}")["value"] for k in ("template", "name", "text", "size")}

    # failure: the config directory refuses new files (the widgets directory stays writable); uid != 0 is required for this to bite
    gate.step("make the private config directory unwritable and press Create")
    before_json = shell_json(gate)
    before_widgets, before_cfg = directory_listing(widgets), directory_listing(cfg)
    assert not (gate.layout()["notice"] or "").startswith("Could not create widget"), "a failure notice was already showing"
    cfg.chmod(0o500)
    try:
        try:
            (cfg / "probe").touch(exist_ok=False)
            fail("the config directory is still writable; the failure injection is ineffective (running as root?)")
        except PermissionError:
            pass
        assert os.access(widgets, os.W_OK), "the widgets directory is not writable"
        watcher = Watcher(widgets, before_widgets)
        watcher.start()
        try:
            gate.click("wg:new:create")
            notice = gate.wait(lambda: (lambda n: n if n and n.startswith("Could not create widget") else None)(gate.layout()["notice"]), "no failure notice: the worker transaction never reported", seconds=20)
        finally:
            seen = watcher.finish()
        assert "Permission denied" in notice, f"the failure was not the permission failure that was injected: {notice}"
        time.sleep(1.0)    # a late reappearance of the rolled-back file or a late success would still show here
        assert directory_listing(widgets) == before_widgets, f"widgets directory after the failed Create: {directory_listing(widgets)} (before {before_widgets})"
        assert shell_json(gate) == before_json, "decebal.json changed although its commit failed"
        assert directory_listing(cfg) == before_cfg, f"temp or backup files appeared in the config directory: {directory_listing(cfg)}"
        assert gate.ctl("wg:new:create") and gate.ctl("wg:new:create")["enabled"], "the draft form did not stay open after the failure"
        kept = {k: gate.ctl(f"wg:new:{k}")["value"] for k in ("template", "name", "text", "size")}
        assert kept == draft, f"the draft changed after the failure: {kept} (was {draft})"
        assert gate.settings.poll() is None, "Settings exited during the failed Create"
        gate.shot("form-after-failure")
        report["cases"]["failure"] = {"notice": notice, "transient_files_seen": seen, "json_sha256": hashlib.sha256(before_json).hexdigest(), "draft": kept}
    finally:
        cfg.chmod(0o700)

    # retry with the directory writable again: the same name must be free
    gate.step("restore the directory and press Create again")
    gate.click("wg:new:create")
    gate.wait(lambda: gate.ctl("wg:new:create") is None and gate.ctl("wg:new:open"), "the form did not close after the retried Create")
    gate.wait(lambda: len(json.loads(config_path.read_text())["widgets"]["items"]) == 4, "decebal.json did not gain the note card")
    after_note = json.loads(config_path.read_text())
    spec = {"use": "field-note", "anchor": note["anchor"], "x": int(note["x"]), "y": int(note["y"])}
    verify_config(after_clock, after_note, [spec])
    verify_config(original, after_note, [{"use": "desk-clock", "anchor": clock["anchor"], "x": int(clock["x"]), "y": int(clock["y"])}, spec])
    assert directory_listing(widgets) == ["desk-clock.toml", "field-note.toml"], f"the retry did not reuse the freed file name: {directory_listing(widgets)}"
    check_widget_toml((widgets / "field-note.toml").read_text(), "field-note", "Field Note", have=("Greetings", "XTEST"), widths=(338.0,))
    ready = gate.wait(lambda: (lambda s: s if s["running"] and s["state"] == "ready" and s["cards"]["wanted"] >= 3 else None)(status(gate)), "the desktop host did not become ready with the note", seconds=25, pause=0.3)
    assert ready["cards"]["broken"] == 0 and all(i["error"] is None for i in ready["items"]), ready
    assert {"desk-clock", "field-note"} <= {i["use"] for i in ready["items"]}, ready
    gate.shot("after-retry")
    report["cases"]["note"] = {"form": note, "host": ready}
    assert gate.settings.poll() is None, "Settings exited during the run"
    actions = (private / "actions").read_text().splitlines()
    assert not actions, f"an external action tool was invoked: {actions}"
    report["fake_tool_calls"] = actions
    return report


# ---------------------------------------------------------------------------------------------------------------- main
def parse_args(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--bin-dir", required=True, help="absolute, immutable directory holding the candidate decsettings and decwidgets")
    ap.add_argument("--out", required=True, help="a NEW absolute evidence directory (short path)")
    ap.add_argument("--extra-bin", action="append", default=[], metavar="NAME", help="another binary of --bin-dir to copy (a renderer, if the candidate uses one)")
    return ap.parse_args(argv)


def main(argv=None):
    args = parse_args(argv)
    if os.getuid() == 0:
        fail("run as the unprivileged user: permission bits do not stop root, so the failure injection would not bite")
    bins = validate_bin_dir(args.bin_dir, args.out, args.extra_bin)
    out = fresh_out(args.out)
    (out / "shots").mkdir()
    (out / "logs").mkdir()
    x11, helper = load_helper()
    gate = Gate(x11, out, os.urandom(8).hex())
    private = out / "r"
    gate.private = private
    outcome = {"passed": False, "error": None, "interrupted": None}
    report = {}

    def terminated(signum, frame):
        raise Terminated(signum)

    # handlers first: private_x11 keeps caller handlers, so SIGTERM/SIGINT unwind through our cleanup instead of exiting underneath it
    for signum in (signal.SIGTERM, signal.SIGINT):
        signal.signal(signum, terminated)
    actions, hashes = build_private(private, bins, args.extra_bin)
    hashes["harness"] = sha256_file(__file__)
    hashes["private_x11.py"] = sha256_file(helper)
    gate.env = base_env(private, gate.token)
    try:
        gate.step("start the owned private Xvfb")
        log = (out / "logs" / "xvfb.log").open("w")
        gate.logs.append(log)
        gate.server, display = x11.start_private_xvfb(env=gate.env, log=log, screen="1280x800x24", executable=gate.tools["Xvfb"])
        gate.display, gate.number = display, int(display[1:])
        gate.env["DISPLAY"] = display
        gate.owned()
        report = run_flow(gate, hashes)
        outcome["passed"] = True
    except Terminated as stop:
        outcome["interrupted"] = signal.Signals(stop.signum).name
        outcome["error"] = f"interrupted by {outcome['interrupted']}"
    except BaseException as error:           # noqa: BLE001 - every failure must still reach the one cleanup path
        outcome["error"] = f"{type(error).__name__}: {error}"
        outcome["trace"] = traceback.format_exc()
    finally:
        for signum in (signal.SIGTERM, signal.SIGINT):
            signal.signal(signum, signal.SIG_IGN)      # cleanup is not interruptible
        try:
            os.chmod(private / "cfg", 0o700)           # the one directory this gate makes unwritable; always put it back
        except OSError:
            pass
        try:
            gate.close()
        except BaseException as error:               # noqa: BLE001
            gate.ledger.events.append(f"cleanup raised {type(error).__name__}: {error}")
            gate.ledger.leftovers.append("cleanup-error")
        problems = gate.ledger.problems()
        changed = [n for n, p in bins.items() if sha256_file(p) != hashes[n]["sha256"]]
        if changed:
            problems.append(f"the source binaries changed during the run: {changed}")
        if problems:
            outcome["passed"] = False
        (out / "widget-creator-native.json").write_text(json.dumps({
            "note": "Settings > Widgets > Create driven natively (XTEST) on an owned private Xvfb with no window manager; synthetic config; private copies of the binaries",
            "token": gate.token, "display": gate.display, "outcome": outcome, "cleanup_problems": problems, "cleanup_events": gate.ledger.events,
            "inputs": hashes, "steps": gate.steps, "report": report}, indent=1, default=str) + "\n")
    if outcome["interrupted"]:
        print(f"FAIL {outcome['error']}; cleanup problems: {problems}", file=sys.stderr)
        return 128 + getattr(signal, outcome["interrupted"])
    if not outcome["passed"]:
        print(f"FAIL {outcome['error'] or ''} cleanup problems: {problems}; evidence: {out}", file=sys.stderr)
        return 1
    print(f"PASS widget creator: clock + note created through the form, async JSON-failure rollback and retry, preserved config, host ready; evidence: {out}")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except AssertionError as error:
        print(f"FAIL {error}", file=sys.stderr)
        sys.exit(2)
