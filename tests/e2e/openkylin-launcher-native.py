#!/usr/bin/env python3
"""Native gate for the UKUI start menu: the real `declaunch` binary on an owned private X server, no panel and no theme apply.

Usage:
  python3 tests/e2e/openkylin-launcher-native.py --bin PATH_TO_declaunch --out NEW_EVIDENCE_DIR --fixture-dir DIR_WITH_private_x11.py [--package themes/openkylin] [--quick]

What it proves (and what it does not):
* It drives the candidate exactly as a panel would: `declaunch open --edge E --anchor X,Y --thickness T`, with the UKUI style and the `openkylin` package chosen only by
  `decebal.json` (`launcher.style = "ukui"`, `theme = {name, mode}`). No CLI style argument is passed, so an unknown-style fallback cannot pass: only the UKUI style has the
  expand / restore control, and the run requires Shift+Tab from the initial application list to land on it, Return to expand and Return again to restore.
* Per run (screen x scale x light/dark x panel edge): the same PID and the same X window id in the normal, expanded and restored states; real pixel changes between them
  (diff against a baseline of the empty private root); the card inside the output, the panel strip uncovered on every edge, the full-screen card covering exactly what the
  strip leaves; typing a query ("Green") and Return launches that coloured fixture app (recorded by a fake-only tool). On 1600x900 it also clicks the expand and restore buttons and
  a coloured fixture app icon found by its colour. On 1600x900 the empty card area must be the package's window colour (no compositor: opaque).
* It is **not** an upstream pixel comparison and says nothing about the compositor blur; fixture apps and colours are synthetic.

Isolation: an owned high-numbered Xvfb (helper `private_x11.py`: free high display, both listening sockets verified as this child's before every X command), both D-Bus
buses dead, HOME/XDG private, a PATH holding only fake action tools (`peony gio xdg-open decsession decsettings`) and read-only fontconfig links, a private copy of the exact
`declaunch` so installed siblings cannot run, no real installed applications (a private XDG_DATA_DIRS of synthetic desktop entries). Every child carries a run token and is
stopped through a pidfd; a SIGKILL, a leftover token process or any retry ("rescue") fails the run. Source-only candidate: reviewed with py_compile, not yet executed.
"""
import argparse
import hashlib
import importlib.util
import json
import os
import random
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import tomllib
from pathlib import Path

from PIL import Image, ImageChops

ROOT = Path(__file__).resolve().parents[2]
BACKDROP = {"light": (0xB4, 0xC0, 0xD0), "dark": (0x38, 0x40, 0x4C)}
TITLE = "UKUI Menu"          # the window title only the UKUI style sets
COLOURS = {"red": (0xE6, 0x19, 0x4B), "green": (0x3C, 0xB4, 0x4B), "blue": (0x43, 0x63, 0xD8)}
SCREENS = [(320, 240), (400, 300), (1600, 900)]
SCALES = [1, 1.25, 2]
EDGES = ["bottom", "top", "left", "right"]
FAKE_TOOLS = ("peony", "gio", "xdg-open", "decsession", "decsettings")


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


class Gate:
    """Owns every process of one run and refuses to continue after any irregular event."""

    def __init__(self, args, x11):
        self.args, self.x11 = args, x11
        self.token = os.urandom(8).hex()
        self.forced, self.rescues, self.events = [], [], []
        self.server = self.display = self.number = None
        self.children = []
        self.env = {}
        # the child environment's PATH holds only the fake tools, so the X tools and the server are resolved once, here, to absolute paths
        self.tools = {name: shutil.which(name) for name in ("xdotool", "import", "Xvfb")}
        missing = [n for n, p in self.tools.items() if not p]
        assert not missing, f"missing host tools: {missing}"

    # -- process ownership -------------------------------------------------------------------------------------------------------
    def start(self, name, argv, **kw):
        proc = subprocess.Popen(argv, start_new_session=True, **kw)
        self.children.append((name, proc))
        return proc

    def stop(self, name, proc, grace=6):
        """SIGTERM through a pidfd (never a recycled pid); a process that needs SIGKILL is recorded as a forced cleanup, which fails the run."""
        if proc.poll() is not None:
            return proc.returncode
        try:
            fd = os.pidfd_open(proc.pid)
        except ProcessLookupError:
            return proc.poll()
        try:
            signal.pidfd_send_signal(fd, signal.SIGTERM)
            try:
                return proc.wait(timeout=grace)
            except subprocess.TimeoutExpired:
                self.forced.append(name)
                signal.pidfd_send_signal(fd, signal.SIGKILL)
                return proc.wait(timeout=grace)
        finally:
            os.close(fd)

    def owned(self):
        assert self.server is not None and self.server.poll() is None, "the owned private Xvfb exited"
        assert self.x11.listeners_owned(self.server, self.number), "the X listeners are not this run's Xvfb child any more"

    def x(self, *argv, check=True, timeout=30):
        """Every X-facing command first re-verifies the listener ownership."""
        self.owned()
        argv = (self.tools.get(argv[0], argv[0]), *argv[1:])
        done = subprocess.run(argv, env=self.env, capture_output=True, text=True, timeout=timeout)
        if check and done.returncode != 0:
            raise AssertionError(f"{argv[0]} failed: {done.stderr.strip()}")
        return done.stdout.strip()

    def leftovers(self):
        mark = f"DECEBAL_NATIVE_TOKEN={self.token}".encode()
        found = []
        for pid in (p for p in os.listdir("/proc") if p.isdigit()):
            if int(pid) == os.getpid():
                continue
            try:
                if Path(f"/proc/{pid}").stat().st_uid == os.getuid() and mark in Path(f"/proc/{pid}/environ").read_bytes().split(b"\0"):
                    found.append(int(pid))
            except OSError:
                continue
        return found

    def close(self):
        for name, proc in reversed(self.children):
            if name != "Xvfb":
                self.stop(name, proc)
        if self.server is not None:
            self.stop("Xvfb", self.server)
            self.x11.close(self.server)
        left = self.leftovers()
        if self.forced or left or self.rescues:
            raise AssertionError(f"irregular cleanup: forced={self.forced} leftover_pids={left} rescues={self.rescues}")


def load_helper(fixture_dir):
    path = Path(fixture_dir).resolve() / "private_x11.py"
    spec = importlib.util.spec_from_file_location("private_x11", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    for name in ("start_private_xvfb", "listeners_owned", "close"):
        assert hasattr(module, name), f"{path} lacks {name}"
    return module, path


def svg(colour, label=None):
    r, g, b = colour
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64"><rect x="4" y="4" width="56" height="56" rx="12" fill="#{r:02x}{g:02x}{b:02x}"/>'
            f'</svg>')


def build_fixture(private):
    """Synthetic apps only: three coloured ones sort first ("Aaa Red", "Aab Green", "Aac Blue"), then grey fillers; Exec is a fake recorder."""
    for d in ("bin", "home", "config", "cache", "data/applications", "state", "run", "themes", "icons"):
        (private / d).mkdir(parents=True, exist_ok=True)
    (private / "run").chmod(0o700)
    calls = private / "calls"
    calls.write_text("")
    recorder = private / "bin" / "fixture-app"
    recorder.write_text(f'#!/bin/sh\nprintf "%s\\n" "$1" >> "{calls}"\n')
    recorder.chmod(0o755)
    entries = [(f"Aa{'abc'[i]} {name.title()}", name, svg(rgb, name)) for i, (name, rgb) in enumerate(COLOURS.items())]
    entries += [(f"Filler {n:02}", f"filler{n:02}", svg((0x80, 0x80, 0x80), f"filler{n:02}")) for n in range(24)]
    names = {}
    for title, key, drawing in entries:
        icon = private / "icons" / f"{key}.svg"
        icon.write_text(drawing)
        (private / "data/applications" / f"{key}.desktop").write_text(
            f'[Desktop Entry]\nType=Application\nName={title}\nIcon={icon}\nExec="{recorder}" {key}\nTerminal=false\n')
        names[key] = title
    return calls, names


def build_bin(private, declaunch):
    """A private bin: the exact candidate, fake-only action tools and read-only fontconfig links; nothing else is on PATH."""
    shutil.copy2(declaunch, private / "bin" / "declaunch")
    actions = private / "actions"
    actions.write_text("")
    for tool in FAKE_TOOLS:
        path = private / "bin" / tool
        path.write_text(f'#!/bin/sh\necho "{tool} $*" >> "{actions}"\nexit 0\n')
        path.chmod(0o755)
    for tool in ("fc-list", "fc-match"):
        found = shutil.which(tool)
        assert found, f"{tool} is required for font discovery"
        (private / "bin" / tool).symlink_to(found)
    return actions


def free_area(edge, thick, w, h):
    return {"bottom": (0, 0, w, h - thick), "top": (0, thick, w, h - thick), "left": (thick, 0, w - thick, h), "right": (0, 0, w - thick, h)}[edge]


def anchor_point(edge, scale, thick, w, h):
    first = round(24 * scale)
    return {"bottom": (first, h - thick // 2), "top": (first, thick // 2), "left": (thick // 2, first), "right": (w - thick // 2, first)}[edge]


def bbox(base, shot, threshold=3):
    diff = ImageChops.difference(base, shot).convert("L").point(lambda v: 255 if v > threshold else 0)
    return diff.getbbox()


def near(a, b, tol):
    return all(abs(x - y) <= tol for x, y in zip(a, b))


def set_backdrop(gate, colour):
    """Paints the empty private root (there is no compositor and no wallpaper host)."""
    from Xlib import X, display as xdisplay
    gate.owned()
    d = xdisplay.Display(gate.display)
    try:
        root = d.screen().root
        pixel = (colour[0] << 16) | (colour[1] << 8) | colour[2]
        root.change_attributes(background_pixel=pixel)
        root.clear_area(0, 0, 0, 0)
        d.sync()
    finally:
        d.close()


def shot(gate, path):
    gate.x("import", "-window", "root", str(path))
    return Image.open(path).convert("RGB")


def wait_for(fn, label, tries=150, pause=0.1):
    for _ in range(tries):
        value = fn()
        if value:
            return value
        time.sleep(pause)
    raise AssertionError(f"timed out: {label}")


def window_of(gate, pid):
    """The launcher's window: the one window titled `UKUI Menu` (only the UKUI style sets this title). Exactly one may exist; its owner pid is recorded when the window publishes it."""
    found = gate.x("xdotool", "search", "--name", f"^{TITLE}$", check=False).split()
    assert len(found) <= 1, f"more than one {TITLE} window: {found}"
    return found[0] if found else None


def buttons(card, scale):
    """Centres of the expand button in the start menu and of Restore in the full-screen menu, from the card the render actually shows (no shadow without a compositor)."""
    x0, y0, x1, y1 = card
    width, height = x1 - x0, y1 - y0
    side = min(56 * scale, width * 0.25)
    b = min(36 * scale, side, (height - 24 * scale - 4 * 4 * scale) / 5)   # five slots: expand, user, computer, settings, power
    return (x1 - side / 2, y0 + 12 * scale + b / 2)


def restore_button(free, scale):
    x0, y0, x1, y1 = free
    top = max(round(48 * ((y1 - y0) / scale) / 1080), 8) * scale
    return (x1 - 40 * scale - 18 * scale, y0 + top + 18 * scale)


def one_run(gate, private, calls, theme, shots, screen, scale, mode, edge, keep, big):
    w, h = screen
    thick = round(48 * scale)
    free = free_area(edge, thick, w, h)
    ax, ay = anchor_point(edge, scale, thick, w, h)
    tag = f"{w}x{h}-{int(scale * 100)}-{mode}-{edge}"
    (private / "config/decebal.json").write_text(json.dumps({"theme": {"name": theme["id"], "mode": mode}, "launcher": {"style": "ukui"}}))
    set_backdrop(gate, BACKDROP[mode])
    gate.env["DECEBAL_SCALE"] = str(scale)
    base = shot(gate, shots / f"base-{tag}.png")
    assert near(base.getpixel((w // 2, h // 2)), BACKDROP[mode], 2), f"{tag}: the private root is not the backdrop"
    calls.write_text("")
    gate.x("xdotool", "mousemove", "1", "1")   # a neutral pointer position: no hover state in the first capture
    proc = gate.start("declaunch", [str(private / "bin/declaunch"), "open", "--edge", edge, "--anchor", f"{ax},{ay}", "--thickness", str(thick)], env=gate.env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        win = wait_for(lambda: window_of(gate, proc.pid), f"{tag}: the UKUI Menu window of pid {proc.pid}")
        time.sleep(1.0)
        record = dict(tag=tag, screen=[w, h], scale=scale, mode=mode, edge=edge, strip=thick, free=list(free), pid=proc.pid, xid=win, window_pid=gate.x("xdotool", "getwindowpid", win, check=False) or None)
        normal = shot(gate, shots / f"{tag}-normal.png")
        box_n = bbox(base, normal)
        assert box_n, f"{tag}: the start menu painted nothing"
        fx0, fy0, fx1, fy1 = free[0], free[1], free[0] + free[2], free[1] + free[3]
        assert box_n[0] >= fx0 and box_n[1] >= fy0 and box_n[2] <= fx1 and box_n[3] <= fy1, f"{tag}: card {box_n} leaves the free area {free} (strip {thick} covered or output bounds exceeded)"
        cw, ch = box_n[2] - box_n[0], box_n[3] - box_n[1]
        assert cw <= 766 * scale + 2 and ch <= 688 * scale + 2, f"{tag}: card {cw}x{ch} larger than the 766x688 original at this scale"
        if big and edge == "bottom":
            expected_w = min(766 * scale, free[2] - 16 * scale)
            expected_h = min(688 * scale, free[3] - 16 * scale)
            assert abs(cw - expected_w) <= 2 and abs(ch - expected_h) <= 4, f"{tag}: card {cw}x{ch}, expected {expected_w}x{expected_h} after both 8px margins"
        record["card_normal"] = list(box_n)
        if big:
            # the opaque card (no compositor) is the package's window colour: sampled in the plain top margin above the search bar
            colours = theme["colors"][mode]["overlay"]
            want = tuple(int(colours[1 + 2 * i:3 + 2 * i], 16) for i in range(3))
            sample = (box_n[0] + round(160 * scale), box_n[1] + max(round(4 * scale), 3))
            got = normal.getpixel(sample)
            assert near(got, want, 3), f"{tag}: card colour {got} at {sample}, package overlay {want}"
            record["card_colour"] = {"sample": list(sample), "got": list(got), "want": list(want)}

        # keyboard: Shift+Tab from the initial application list lands on the first action (UKUI's expand button), Return expands, Return restores
        gate.x("xdotool", "key", "shift+Tab")
        time.sleep(0.5)
        focused = shot(gate, shots / f"{tag}-focus.png")
        assert ImageChops.difference(normal, focused).getbbox(), f"{tag}: Shift+Tab changed no pixel (no focus ring on the expand button)"
        gate.x("xdotool", "key", "Return")
        want_flat = tuple(int(theme["colors"][mode]["overlay"][1 + 2 * i:3 + 2 * i], 16) for i in range(3))
        def expanded_ready():
            # Configure can expose a black buffer before the first paint; geometry alone is not presentation.
            frame = shot(gate, shots / f"{tag}-expanded.png")
            b = bbox(base, frame)
            return b if b and near(b, (fx0, fy0, fx1, fy1), 2) and near(frame.getpixel((fx0 + 3, fy0 + 3)), want_flat, 3) else None
        expanded_box = wait_for(expanded_ready, f"{tag}: the painted card did not grow to the free area {free}")
        expanded = Image.open(shots / f"{tag}-expanded.png").convert("RGB")
        assert ImageChops.difference(normal, expanded).getbbox(), f"{tag}: expanded render equals the normal one"
        assert window_of(gate, proc.pid) == win and proc.poll() is None, f"{tag}: the launcher window or process changed on expand"
        corner = expanded.getpixel((fx0 + 3, fy0 + 3))
        want_flat = tuple(int(theme["colors"][mode]["overlay"][1 + 2 * i:3 + 2 * i], 16) for i in range(3))
        assert near(corner, want_flat, 3), f"{tag}: the full-screen card corner {corner} is not the window colour {want_flat}"
        record["card_expanded"] = list(expanded_box)
        gate.x("xdotool", "key", "Return")
        restored_box = wait_for(lambda: (lambda b: b if b and near(b, box_n, 2) else None)(bbox(base, shot(gate, shots / f"{tag}-restored.png"))), f"{tag}: Return did not restore the start menu card {box_n}")
        assert window_of(gate, proc.pid) == win and proc.poll() is None, f"{tag}: the launcher window or process changed on restore"
        record["card_restored"] = list(restored_box)

        if big:
            # pointer: the expand and restore buttons are real hit targets at the positions the render shows
            ex, ey = buttons(box_n, scale)
            gate.x("xdotool", "mousemove", str(round(ex)), str(round(ey)))
            time.sleep(0.3)
            gate.x("xdotool", "click", "1")
            wait_for(lambda: (lambda b: b and abs(b[2] - fx1) <= 2 and abs(b[3] - fy1) <= 2)(bbox(base, shot(gate, shots / f"{tag}-pointer-expanded.png"))), f"{tag}: a click on the expand button did not expand")
            rx, ry = restore_button((fx0, fy0, fx1, fy1), scale)
            gate.x("xdotool", "mousemove", str(round(rx)), str(round(ry)))
            time.sleep(0.3)
            gate.x("xdotool", "click", "1")
            wait_for(lambda: (lambda b: b and near(b, box_n, 2))(bbox(base, shot(gate, shots / f"{tag}-pointer-restored.png"))), f"{tag}: a click on the restore button did not restore")
            assert window_of(gate, proc.pid) == win and proc.poll() is None
            record["pointer_expand_restore"] = True

        # search + Return: the only result, the green fixture app, is launched through the fake recorder
        gate.x("xdotool", "type", "--delay", "80", "Green")
        time.sleep(0.6)
        gate.x("xdotool", "key", "Return")
        wait_for(lambda: calls.read_text().strip(), f"{tag}: the searched fixture app was not launched")
        assert calls.read_text().split() == ["green"], f"{tag}: launched {calls.read_text()!r}, expected only the green fixture app"
        code = proc.wait(timeout=15)
        assert code == 0, f"{tag}: the launcher exited with {code}"
        record["search_launch"] = "green"
        record["exit_code"] = code
        if not keep:
            for suffix in ("normal", "focus", "expanded", "restored", "pointer-expanded", "pointer-restored"):
                (shots / f"{tag}-{suffix}.png").unlink(missing_ok=True)
        return record
    except BaseException:
        gate.events.append(f"{tag}: failed")
        raise
    finally:
        if proc.poll() is None:
            gate.stop("declaunch", proc)


def click_coloured_app(gate, private, calls, theme, shots, scale=1, mode="light"):
    """A pointer launch: find the red fixture icon by its colour in the normal start menu and click its centre."""
    w, h, edge, thick = 1600, 900, "bottom", round(48 * scale)
    (private / "config/decebal.json").write_text(json.dumps({"theme": {"name": theme["id"], "mode": mode}, "launcher": {"style": "ukui"}}))
    set_backdrop(gate, BACKDROP[mode])
    gate.env["DECEBAL_SCALE"] = str(scale)
    base = shot(gate, shots / "base-click.png")
    calls.write_text("")
    gate.x("xdotool", "mousemove", "1", "1")
    ax, ay = anchor_point(edge, scale, thick, w, h)
    proc = gate.start("declaunch", [str(private / "bin/declaunch"), "open", "--edge", edge, "--anchor", f"{ax},{ay}", "--thickness", str(thick)], env=gate.env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        wait_for(lambda: window_of(gate, proc.pid), "click run: the UKUI Menu window")
        time.sleep(1.0)
        normal = shot(gate, shots / "click-normal.png")
        box = bbox(base, normal)
        px = normal.load()
        pts = [(x, y) for y in range(box[1], box[3]) for x in range(box[0], box[0] + round(312 * scale)) if near(px[x, y], COLOURS["red"], 10)]
        assert len(pts) > 40, f"the red fixture icon was not found in the application list ({len(pts)} pixels)"
        cx, cy = sum(p[0] for p in pts) // len(pts), sum(p[1] for p in pts) // len(pts)
        gate.x("xdotool", "mousemove", str(cx), str(cy))
        time.sleep(0.3)
        gate.x("xdotool", "click", "1")
        wait_for(lambda: calls.read_text().strip(), "click run: the clicked fixture app was not launched")
        assert calls.read_text().split() == ["red"], f"clicked the red icon but launched {calls.read_text()!r}"
        code = proc.wait(timeout=15)
        assert code == 0, f"click run: launcher exit {code}"
        return {"click": [cx, cy], "launched": "red", "exit_code": code, "red_pixels": len(pts)}
    finally:
        if proc.poll() is None:
            gate.stop("declaunch", proc)



def matrix_cases(quick=False, shard_index=0, shard_count=1):
    cases = [((1600, 900), 1, "light", "bottom")] if quick else [(s, k, m, e) for s in SCREENS for k in SCALES for m in ("light", "dark") for e in EDGES]
    if not 1 <= shard_count <= len(cases) or not 0 <= shard_index < shard_count:
        raise ValueError("invalid shard index/count")
    return cases[shard_index::shard_count]


def case_tag(case):
    (w, h), scale, mode, edge = case
    return f"{w}x{h}-{int(scale * 100)}-{mode}-{edge}"


def verify_shards(records):
    """Require complete, disjoint, clean evidence from the exact same inputs."""
    expected = {case_tag(c) for c in matrix_cases()}
    seen, indices = set(), set()
    count, inputs = None, None
    pointer = False
    if not records:
        raise ValueError("no shard evidence")
    for record in records:
        if record.get("quick"):
            raise ValueError("quick evidence cannot qualify the matrix")
        shard = record["shard"]
        index, total = shard["index"], shard["count"]
        selected = {case_tag(c) for c in matrix_cases(False, index, total)}
        if count is None:
            count = total
            inputs = {k: v for k, v in record["inputs"].items() if k != "fixture_desktop_entries"}
        comparable = {k: v for k, v in record["inputs"].items() if k != "fixture_desktop_entries"}
        if not comparable.get("fixture_desktop_entries_normalized"):
            raise ValueError("missing normalized fixture identity")
        if total != count or index in indices or comparable != inputs:
            raise ValueError("duplicate shard or mismatched inputs/count")
        indices.add(index)
        cleanup = record["cleanup"]
        if cleanup["run_failed"] or cleanup["error"] or cleanup["forced"] or cleanup["rescues"]:
            raise ValueError("interrupted or unclean shard")
        tags = [r["tag"] for r in record["runs"]]
        if len(tags) != len(selected) or set(tags) != selected or record["passed"] != len(tags):
            raise ValueError("missing, duplicate, or unexpected cases")
        if seen.intersection(tags):
            raise ValueError("overlapping shard coverage")
        seen.update(tags)
        launch = record.get("pointer_launch")
        if launch is not None:
            if launch.get("launched") != "red" or launch.get("exit_code") != 0 or launch.get("red_pixels", 0) <= 40:
                raise ValueError("pointer launch not proven")
            pointer = True
    if indices != set(range(count)) or seen != expected or not pointer:
        raise ValueError("incomplete matrix or missing pointer launch")
    return {"passed": True, "cases": len(seen), "shards": count, "inputs": inputs,
            "scope": "UKUI native matrix only; not full release qualification"}


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--bin", required=True, help="the candidate declaunch (copied into the private bin; installed siblings are never used)")
    ap.add_argument("--out", required=True, help="a NEW evidence directory")
    ap.add_argument("--fixture-dir", required=True, help="directory holding the current private_x11.py (e.g. the main checkout's tests/e2e)")
    ap.add_argument("--package", default=str(ROOT / "themes/openkylin"), help="the openkylin package copied into the private themes dir")
    ap.add_argument("--quick", action="store_true", help="smoke: one 1600x900 scale-1 light run on the bottom edge plus the click run")
    ap.add_argument("--captures", choices=("bottom", "all"), default="bottom", help="which runs keep their PNGs (the checks always run)")
    ap.add_argument("--shard-index", type=int, default=0)
    ap.add_argument("--shard-count", type=int, default=1, help="split coverage across separately guarded runs; aggregate all shards before claiming full acceptance")
    args = ap.parse_args()
    try:
        matrix = matrix_cases(args.quick, args.shard_index, args.shard_count)
    except ValueError as error:
        ap.error(str(error))
    out = Path(args.out).resolve()
    out.mkdir(parents=True, exist_ok=False)
    binary, package = Path(args.bin).resolve(), Path(args.package).resolve()
    assert binary.is_file() and os.access(binary, os.X_OK), f"{binary} is not an executable file"
    theme = tomllib.loads((package / "theme.toml").read_text())
    assert theme["launcher"]["style"] == "ukui" and theme["id"] == "openkylin", "the package must be the openkylin package with launcher.style = ukui"
    x11, helper = load_helper(args.fixture_dir)
    gate = Gate(args, x11)
    runs, click = [], None
    shots = out / "shots"
    shots.mkdir()
    with tempfile.TemporaryDirectory(prefix="oklaunch-") as tmp:
        private = Path(tmp)
        calls, names = build_fixture(private)
        shutil.copytree(package, private / "themes" / theme["id"])
        actions = build_bin(private, binary)
        base_env = {
            "HOME": str(private / "home"), "XDG_CONFIG_HOME": str(private / "config"), "XDG_CACHE_HOME": str(private / "cache"), "XDG_DATA_HOME": str(private / "data"),
            "XDG_DATA_DIRS": str(private / "data"), "XDG_STATE_HOME": str(private / "state"), "XDG_RUNTIME_DIR": str(private / "run"), "DECEBAL_CONFIG": str(private / "config"),
            "DECEBAL_THEMES": str(private / "themes"), "DECEBAL_SYSTEM_THEMES": str(private / "themes"), "DECEBAL_BACKEND": "x11", "DECEBAL_SYSTEM_AUTOMATIC": "0",
            "DBUS_SESSION_BUS_ADDRESS": f"unix:path={private}/no-session-bus", "DBUS_SYSTEM_BUS_ADDRESS": f"unix:path={private}/no-system-bus",
            "PATH": str(private / "bin"), "LANG": "C.UTF-8", "DECEBAL_NATIVE_TOKEN": gate.token,
        }
        if os.environ.get("E2E_PARENT_TOKEN"):
            base_env["E2E_PARENT_TOKEN"] = os.environ["E2E_PARENT_TOKEN"]
        hashes = {
            "declaunch": sha(private / "bin/declaunch"), "declaunch_source_path": str(binary), "theme.toml": sha(package / "theme.toml"),
            "start_button_logo": sha(package / "icons/kylin-startmenu.png") if (package / "icons/kylin-startmenu.png").exists() else None,
            "private_x11.py": sha(helper), "harness": sha(__file__),
            "fixture_desktop_entries": {p.name: sha(p) for p in sorted((private / "data/applications").glob("*.desktop"))},
            "fixture_desktop_entries_normalized": {p.name: hashlib.sha256(p.read_bytes().replace(os.fsencode(private), b"<PRIVATE>")).hexdigest() for p in sorted((private / "data/applications").glob("*.desktop"))},
            "fixture_icons": {p.name: sha(p) for p in sorted((private / "icons").glob("*.svg"))},
        }
        try:
            # one owned Xvfb per screen size
            for screen in dict.fromkeys(item[0] for item in matrix):
                gate.env = dict(base_env)
                with (out / f"xvfb-{screen[0]}x{screen[1]}.log").open("w") as log:
                    server, display = x11.start_private_xvfb(env=gate.env, log=log, screen=f"{screen[0]}x{screen[1]}x24", executable=gate.tools["Xvfb"])
                gate.server, gate.display, gate.number = server, display, int(display[1:])
                gate.env["DISPLAY"] = display
                gate.children.append(("Xvfb", server))
                wait_for(lambda: gate.x("xdotool", "getdisplaygeometry", check=False), "the private display answers")
                for item in matrix:
                    if item[0] != screen:
                        continue
                    _, scale, mode, edge = item
                    big = screen == (1600, 900)
                    keep = args.captures == "all" or edge == "bottom"
                    runs.append(one_run(gate, private, calls, theme, shots, screen, scale, mode, edge, keep, big))
                    print("PASS", runs[-1]["tag"], flush=True)
                if screen == (1600, 900):
                    click = click_coloured_app(gate, private, calls, theme, shots)
                    print("PASS pointer launch of the red fixture app", flush=True)
                gate.stop("Xvfb", server)
                x11.close(server)
                gate.server = None
        finally:
            ledger_error = None
            in_flight = sys.exc_info()[0] is not None
            try:
                gate.close()
            except AssertionError as error:
                ledger_error = str(error)
            (out / "native-launcher.json").write_text(json.dumps({
                "note": "UKUI start menu driven natively on an owned private X server (no compositor, no panel, no theme apply); synthetic fixture apps; not an upstream pixel comparison",
                "quick": args.quick, "shard": {"index": args.shard_index, "count": args.shard_count}, "inputs": hashes, "token": gate.token, "fake_tool_calls": actions.read_text().splitlines(),
                "cleanup": {"forced": gate.forced, "rescues": gate.rescues, "events": gate.events, "error": ledger_error, "run_failed": in_flight},
                "pointer_launch": click, "runs": runs, "passed": len(runs)}, indent=1))
            if ledger_error:
                print("FAIL cleanup:", ledger_error, file=sys.stderr)
                if not in_flight:
                    sys.exit(1)
    print(f"PASS {len(runs)} runs of the UKUI start menu (keyboard expand/restore, search launch, strip and bounds on every edge); evidence: {out}")


if __name__ == "__main__":
    main()
