#!/usr/bin/env python3
"""Bounded Settings preview pressure against immutable 1ec binaries.

One owned Xvfb per case. Private HOME, XDG, runtime, and bus sockets.
No production edits, no release build, no network photo download, no
host mount or quota change. Cache write denial is mode 0555 on a private
directory, not ENOSPC. This is not a desktop-environment benchmark.

Evidence must be a new directory. PREVIEW_PRESSURE_EVIDENCE defaults to
/var/tmp/decebal-preview-pressure-20261031, which the first run already
filled; an existing path is refused and left unchanged.
PREVIEW_PRESSURE_ARTIFACT may point at another absolute directory whose
decsettings and decthemectl still match the pinned SHA256SUMS.
"""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import struct
import subprocess
import sys
import time
import traceback
import zlib

sys.path.insert(0, str(Path(__file__).resolve().parent))
from private_x11 import close as close_xvfb
from private_x11 import start_private_xvfb

ROOT = Path(__file__).resolve().parents[2]
SOURCE_COMMIT = "1ec25dfe0f4d2c45f748c581665c739e38cb3fb1"
PINNED_HASHES = {
    "decsettings": "23a9f3fb4ad2e1a150acef9b0a96731cd7a8ea10359dde6319bfade7772ff98c",
    "decthemectl": "566d00be02a3ed6059ac28d2bc288b03bfc66018e4f51298ef2e0eb1731891b3",
}
DEFAULT_ARTIFACT = Path(
    "/var/tmp/decebal-release-engineering-20261031/artifacts"
) / SOURCE_COMMIT


def artifact_root() -> Path:
    raw = os.environ.get("PREVIEW_PRESSURE_ARTIFACT", "").strip()
    return Path(raw) if raw else DEFAULT_ARTIFACT


ARTIFACT = artifact_root()
BIN = ARTIFACT / "bin"
THEMES = ARTIFACT.with_name(ARTIFACT.name + ".assets") / "share" / "decebal" / "themes"
SUMS = ARTIFACT / "SHA256SUMS"
RECORDER = ROOT / "tools" / "desktop-performance.py"
EVIDENCE = Path(os.environ.get(
    "PREVIEW_PRESSURE_EVIDENCE",
    "/var/tmp/decebal-preview-pressure-20261031",
))
FIXTURE_THEMES = (
    "nord", "dracula", "solarized", "graphite", "high-contrast",
    "macos", "windows11", "caelestia", "mountain-neon-dot", "glyph-rice",
)
FONT_THEMES = ("dux-lavender", "gnome-adwaita", "gnome-catppuccin-dock", "sweeteh")
SCREEN = "1280x800x24"
WINDOW = (1100, 740)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def load_sums(path: Path) -> dict[str, str]:
    sums = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        digest, name = line.split(None, 1)
        sums[name.strip()] = digest
    return sums


def tiny_png(width: int = 8, height: int = 4) -> bytes:
    """A local RGB PNG. No photo download."""
    row = b"\x00" + bytes((40, 80, 120)) * width
    raw = row * height
    def chunk(tag: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")


def png_geometry(path: Path) -> dict | None:
    try:
        blob = path.read_bytes()
    except OSError as exc:
        return {"error": exc.strerror or str(exc)}
    if len(blob) < 24 or blob[:8] != b"\x89PNG\r\n\x1a\n" or blob[12:16] != b"IHDR":
        return {"error": "not_png", "size": len(blob)}
    width, height = struct.unpack(">II", blob[16:24])
    return {
        "width": width,
        "height": height,
        "bytes": len(blob),
        "aspect": round(width / height, 4) if height else None,
        # The renderer canvas is 1920x1080 plus a 120 px footer (2040 is not used).
        "matches_full_canvas_plus_footer": width == 1920 and height == 1200,
        "matches_canvas_without_footer": width == 1920 and height == 1080,
    }


def read_layout(path: Path) -> dict | None:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError, UnicodeError):
        return None


def proc_children(pid: int) -> list[int]:
    found = []
    proc = Path("/proc")
    for entry in proc.iterdir():
        if not entry.name.isdigit():
            continue
        try:
            stat = (entry / "stat").read_text(encoding="utf-8")
        except OSError:
            continue
        cut = stat.rfind(")")
        if cut < 0:
            continue
        fields = stat[cut + 2:].split()
        if len(fields) > 1 and fields[1] == str(pid):
            found.append(int(entry.name))
    return found


def signal_pidfd(pid: int, sig: int, parent: int | None = None) -> str:
    try:
        handle = os.pidfd_open(pid, 0)
    except OSError as exc:
        return exc.strerror or "pidfd_open_failed"
    try:
        if parent is not None:
            # Pin the PID before checking parentage, then signal only that descriptor.
            try:
                stat = Path(f"/proc/{pid}/stat").read_text()
                fields = stat[stat.rfind(")") + 2:].split()
                if len(fields) < 2 or fields[1] != str(parent):
                    return "not_our_child"
            except OSError:
                return "gone"
        signal.pidfd_send_signal(handle, sig)
    except ProcessLookupError:
        return "gone"
    except OSError as exc:
        return exc.strerror or "pidfd_signal_failed"
    finally:
        os.close(handle)
    return "sent"


def stop_process(process: subprocess.Popen | None) -> dict:
    """Signal this exact child, then children that still name it as parent."""
    if process is None:
        return {"stopped": False}
    pid = process.pid
    if process.poll() is not None:
        return {"pid": pid, "already_exited": process.returncode}
    kids = []
    for child in proc_children(pid):
        kids.append({"pid": child, "term": signal_pidfd(child, signal.SIGTERM, parent=pid)})
    parent = signal_pidfd(pid, signal.SIGTERM)
    try:
        code = process.wait(timeout=3)
        killed = False
    except subprocess.TimeoutExpired:
        signal_pidfd(pid, signal.SIGKILL)
        for child in proc_children(pid):
            signal_pidfd(child, signal.SIGKILL, parent=pid)
        code = process.wait(timeout=3)
        killed = True
    return {"pid": pid, "term": parent, "children": kids, "exit": code, "sigkill": killed}


class Fault(Exception):
    def __init__(self, kind: str, detail: str):
        super().__init__(detail)
        self.kind = kind
        self.detail = detail


def run_text(args: list[str], env: dict, timeout: float) -> str:
    completed = subprocess.run(
        args, env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        timeout=timeout, check=False,
    )
    if completed.returncode != 0:
        raise Fault("orchestration", f"{args[0]} exited {completed.returncode}: {' '.join(args[:6])}")
    return completed.stdout.strip()


def prepare_env(root: Path, fontconfig: Path) -> dict:
    env = {}
    for key in ("PATH", "LANG", "LC_ALL", "USER", "LOGNAME", "TERM"):
        if key in os.environ:
            env[key] = os.environ[key]
    for name in ("home", "config", "cache", "data", "state", "run", "empty", "tmp", "themes"):
        (root / name).mkdir(mode=0o700)
    fake = root / "fake"
    fake.mkdir(mode=0o700)
    for tool in ("curl", "wget", "aria2c", "nmcli", "pactl", "wpctl", "rdwmctl"):
        stub = fake / tool
        stub.write_text("#!/bin/sh\nexit 1\n", encoding="utf-8")
        stub.chmod(0o700)
    renderer = BIN / "decthemectl"
    (fake / "decthemectl").symlink_to(renderer)
    font_dirs = []
    for name in FONT_THEMES:
        directory = THEMES / name / "fonts"
        if directory.is_dir():
            font_dirs.append(directory)
    if not font_dirs:
        raise Fault("orchestration", "artifact font bundle is missing")
    fontconfig.parent.mkdir(parents=True, exist_ok=True)
    lines = [
        '<?xml version="1.0"?>',
        "<fontconfig>",
    ]
    for directory in font_dirs:
        lines.append(f"  <dir>{directory}</dir>")
    lines.append(f"  <cachedir>{root / 'fontconfig-cache'}</cachedir>")
    lines.append("</fontconfig>\n")
    fontconfig.write_text("\n".join(lines), encoding="utf-8")
    (root / "fontconfig-cache").mkdir(mode=0o700, exist_ok=True)
    for name in FIXTURE_THEMES:
        source = THEMES / name
        if not source.is_dir():
            raise Fault("orchestration", f"fixture theme missing: {name}")
        shutil.copytree(source, root / "themes" / name, symlinks=False)
    wallpaper = root / "home" / "fixture-wallpaper.png"
    wallpaper.write_bytes(tiny_png(32, 18))
    config = root / "config.toml"
    config.write_text(
        "[general]\nborder_px = 2\ngap_inner = 6\n\n"
        f"[wallpaper]\nimage = \"{wallpaper}\"\nmode = \"fill\"\n",
        encoding="utf-8",
    )
    env.update(
        HOME=str(root / "home"),
        TMPDIR=str(root / "tmp"),
        XDG_CONFIG_HOME=str(root / "config"),
        XDG_CACHE_HOME=str(root / "cache"),
        XDG_DATA_HOME=str(root / "data"),
        XDG_STATE_HOME=str(root / "state"),
        XDG_RUNTIME_DIR=str(root / "run"),
        XDG_CONFIG_DIRS=str(root / "empty"),
        XDG_DATA_DIRS=str(root / "empty"),
        XDG_SESSION_TYPE="x11",
        DECEBAL_CONFIG=str(root / "config"),
        DECEBAL_THEMES=str(root / "themes"),
        DECEBAL_SYSTEM_THEMES=str(root / "empty"),
        DECEBAL_BACKEND="x11",
        DECEBAL_SYSTEM_AUTOMATIC="0",
        DECEBAL_THEME_PREVIEW_BIN=str(renderer),
        DBUS_SESSION_BUS_ADDRESS=f"unix:path={root / 'run' / 'no-session-bus'}",
        DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={root / 'run' / 'no-system-bus'}",
        RDWM_SOCK=str(root / "run" / "no-wm"),
        RDWMCTL=str(fake / "rdwmctl"),
        DECSETTINGS_DEBUG_LAYOUT=str(root / "layout.json"),
        DECSETTINGS_CONFIG=str(config),
        FONTCONFIG_FILE=str(fontconfig),
        PATH=f"{fake}:/usr/bin:/bin",
    )
    for key in ("DISPLAY", "WAYLAND_DISPLAY", "XAUTHORITY", "DBUS_SESSION_BUS_ADDRESS"):
        # DBUS is set to the private path above. Drop inherited display files only.
        if key.startswith("DBUS"):
            continue
        env.pop(key, None)
    return env


def window_name(env: dict, xid: str) -> str | None:
    try:
        return run_text(["xdotool", "getwindowname", xid], env, 3)
    except (Fault, subprocess.TimeoutExpired):
        return None


def pane_rect(layout: dict, pane_id: str) -> list:
    panes = layout.get("panes") or []
    pane = next((item for item in panes if item.get("id") == pane_id), None)
    if pane is None:
        raise Fault("product", f"pane {pane_id} missing from layout")
    rect = pane.get("rect") or []
    if len(rect) != 4:
        raise Fault("product", f"pane {pane_id} has no rect")
    return rect


def view_top(layout: dict) -> float:
    search = layout.get("search")
    if isinstance(search, (list, tuple)) and len(search) >= 4:
        return float(search[1]) + float(search[3]) + 1.0
    return 48.0


def pane_in_view(layout: dict, pane_id: str) -> bool:
    x, y, _width, height = pane_rect(layout, pane_id)
    return x >= 0 and y >= view_top(layout) and y + height <= WINDOW[1] - 2


def sidebar_click_x(layout: dict) -> str:
    panes = layout.get("panes") or []
    rights = []
    for item in panes:
        rect = item.get("rect") or []
        if len(rect) == 4 and rect[2] > 0:
            rights.append(float(rect[0]) + float(rect[2]))
    if not rights:
        return "80"
    return str(max(24, int(min(rights) * 0.45)))


def reveal_pane(env: dict, xid: str, layout_path: Path, layout: dict, pane_id: str) -> dict:
    """Scroll the sidebar until the pane rect sits inside the window.

    One wheel notch on this binary moves the sidebar by about 60 px.
    A burst that does not repaint is an input-delivery fault. A repaint
    that leaves the row offscreen is a product fault.
    """
    deadline = time.monotonic() + 5
    misses = 0
    current = layout
    while time.monotonic() < deadline:
        current = read_layout(layout_path) or current
        if pane_in_view(current, pane_id):
            return current
        _x, y, _width, height = pane_rect(current, pane_id)
        top = view_top(current)
        button = "4" if y < top else "5"
        gap = (top - y) if button == "4" else (y + height - (WINDOW[1] - 4))
        repeats = max(1, min(6, int(max(gap, 1) / 50) + 1))
        frames_before = current.get("frames")
        run_text([
            "xdotool", "mousemove", "--window", xid, sidebar_click_x(current), "360",
            "click", "--repeat", str(repeats), "--delay", "12", button,
        ], env, 4)
        until = time.monotonic() + 0.5
        while time.monotonic() < until:
            nxt = read_layout(layout_path)
            if nxt is None:
                time.sleep(0.02)
                continue
            new_y = pane_rect(nxt, pane_id)[1]
            if nxt.get("frames") != frames_before or abs(new_y - y) >= 1:
                current = nxt
                break
            time.sleep(0.02)
        if pane_in_view(current, pane_id):
            return current
        new_y = pane_rect(current, pane_id)[1]
        if abs(new_y - y) < 1:
            misses += 1
            if misses >= 2:
                if current.get("frames") == frames_before:
                    raise Fault(
                        "orchestration",
                        f"sidebar wheel did not repaint; pane {pane_id} stayed at y={new_y}",
                    )
                raise Fault("product", f"pane {pane_id} stayed offscreen at y={new_y} after sidebar wheel")
    y = pane_rect(current, pane_id)[1]
    raise Fault("product", f"pane {pane_id} stayed offscreen at y={y}")


def click_pane(env: dict, xid: str, layout_path: Path, layout: dict, pane_id: str) -> dict:
    current = reveal_pane(env, xid, layout_path, layout, pane_id)
    x, y, width, height = pane_rect(current, pane_id)
    run_text([
        "xdotool", "mousemove", "--window", xid,
        str(int(x + width / 2)), str(int(y + height / 2)), "click", "1",
    ], env, 4)
    return current


def click_theme_tab(env: dict, xid: str, layout: dict) -> str | None:
    controls = layout.get("ctls") or []
    tab = next((item for item in controls if item.get("id") == "ap:tab"), None)
    if tab is None:
        return None
    parts = tab.get("parts") or []
    if not parts:
        return tab.get("value")
    tx, ty, tw, th = parts[0]
    run_text([
        "xdotool", "mousemove", "--window", xid,
        str(int(tx + tw / 2)), str(int(ty + th / 2)), "click", "1",
    ], env, 4)
    return "theme"


def snapshot(layout: dict | None, cache: Path, app: subprocess.Popen, started: float) -> dict:
    pngs = sorted(cache.glob("*.png")) if cache.is_dir() else []
    geometries = []
    for path in pngs[:8]:
        geometries.append({"name": path.name, **(png_geometry(path) or {})})
    body = {
        "seconds": round(time.monotonic() - started, 3),
        "alive": app.poll() is None,
        "exit": app.poll(),
        "pngs": len(pngs),
        "png_geometry": geometries,
    }
    if layout:
        body.update(
            pane=layout.get("pane"),
            frames=layout.get("frames"),
            loaded_thumbs=layout.get("loaded_thumbs"),
            theme_previews_painted=layout.get("theme_previews_painted"),
            card_controls=sum(
                str(item.get("id", "")).startswith("l:tcard:")
                for item in (layout.get("ctls") or [])
            ),
        )
    return body


def record_resources(pids: list[int], output: Path) -> dict:
    alive = [pid for pid in pids if pid and Path(f"/proc/{pid}").exists()]
    if not alive:
        return {"status": "no_live_pid", "pids": pids}
    command = [sys.executable, str(RECORDER)]
    for pid in alive:
        command.extend(["--pid", str(pid)])
    command.extend([
        "--duration", "1", "--period-ms", "500", "--force", "--output", str(output),
    ])
    try:
        completed = subprocess.run(
            command, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            timeout=15, check=False,
        )
    except subprocess.TimeoutExpired as exc:
        completed = None
        summary = {"status": "recorder_timeout", "detail": str(exc)[:500]}
    else:
        summary = {
            "exit": completed.returncode,
            "stderr": (completed.stderr or "").strip()[:500],
            "stdout": (completed.stdout or "").strip()[:500],
        }
    if output.is_file():
        try:
            doc = json.loads(output.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            summary["parse_error"] = str(exc)
            return summary
        summary["processes"] = []
        for proc in doc.get("processes") or []:
            cpu = ((proc.get("summary") or {}).get("cpu") or {})
            rss = ((proc.get("summary") or {}).get("rss_bytes") or {})
            pss = ((proc.get("summary") or {}).get("pss_bytes") or {})
            summary["processes"].append({
                "pid": proc.get("pid"),
                "comm": proc.get("comm"),
                "outcome": proc.get("outcome"),
                "cpu_percent_one_core": cpu.get("cpu_percent_one_core"),
                "cpu_status": cpu.get("status"),
                "rss_bytes_last": rss.get("last"),
                "rss_status": rss.get("status"),
                "pss_bytes_last": pss.get("last"),
                "pss_status": pss.get("status"),
            })
    return summary


def log_tail(path: Path, limit: int = 60) -> str:
    if not path.is_file():
        return ""
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    return "\n".join(lines[-limit:])


def write_repro(case_dir: Path, payload: dict) -> None:
    (case_dir / "min-repro.json").write_text(
        json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8",
    )


class Case:
    def __init__(self, name: str, evidence: Path):
        self.name = name
        self.evidence = evidence / name
        self.evidence.mkdir(parents=True, exist_ok=False)
        self.root = self.evidence / "root"
        self.root.mkdir(mode=0o700)
        self.env = prepare_env(self.root, self.evidence / "fonts.conf")
        self.layout_path = self.root / "layout.json"
        self.cache = self.root / "cache" / "decebal" / "theme-previews"
        self.xvfb = None
        self.app = None
        self.display = None
        self.xid = None
        self.started = time.monotonic()
        self.notes: list[str] = []

    def start_display(self) -> None:
        log = (self.evidence / "xvfb.log").open("w", encoding="utf-8")
        self._xvfb_log = log
        try:
            self.xvfb, self.display = start_private_xvfb(
                env=self.env, log=log, screen=SCREEN, timeout=5,
            )
        except Exception as exc:
            raise Fault("orchestration", f"private Xvfb did not become ready: {exc}") from exc
        self.env["DISPLAY"] = self.display
        self.env.pop("XAUTHORITY", None)
        self.env.pop("WAYLAND_DISPLAY", None)

    def start_settings(self, pane: str | None = None) -> None:
        binary = BIN / "decsettings"
        argv = [str(binary)]
        if pane:
            argv.append(pane)
        argv.extend(["--config", str(self.root / "config.toml")])
        log = (self.evidence / "settings.log").open("a", encoding="utf-8")
        self._settings_log = log
        self.app = subprocess.Popen(
            argv, env=self.env, stdout=log, stderr=log, start_new_session=True,
        )
        self.settings_argv = argv
        self.started = time.monotonic()

    def wait_window(self, timeout: float = 8) -> str:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.app.poll() is not None:
                raise Fault("product", f"Settings exited before mapping: {self.app.returncode}")
            if self.xvfb.poll() is not None:
                raise Fault("orchestration", "private Xvfb exited")
            try:
                text = run_text(["xdotool", "search", "--class", "DecSettings"], self.env, 3)
            except (Fault, subprocess.TimeoutExpired):
                time.sleep(0.05)
                continue
            lines = [line for line in text.splitlines() if line]
            if lines:
                self.xid = lines[0]
                return self.xid
            time.sleep(0.05)
        raise Fault("product", "Settings window was not mapped")

    def wait_layout(self, timeout: float = 6) -> dict:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            layout = read_layout(self.layout_path)
            if layout and layout.get("panes"):
                return layout
            if self.app.poll() is not None:
                raise Fault("product", f"Settings exited while waiting for layout: {self.app.returncode}")
            time.sleep(0.05)
        raise Fault("product", "layout dump did not appear")

    def capture(self, name: str) -> dict | None:
        if not self.xid:
            return None
        target = self.evidence / f"{name}.png"
        try:
            run_text(["import", "-window", self.xid, str(target)], self.env, 8)
        except (Fault, subprocess.TimeoutExpired) as exc:
            self.notes.append(f"capture {name} failed: {exc}")
            return None
        geometry = png_geometry(target)
        if geometry:
            geometry["file"] = str(target)
            geometry["requested_window"] = list(WINDOW)
        return geometry

    def sample_cpu(self) -> dict:
        pids = []
        if self.app is not None and self.app.poll() is None:
            pids.append(self.app.pid)
            pids.extend(proc_children(self.app.pid)[:4])
        return record_resources(pids, self.evidence / "recorder.json")

    def close(self) -> dict:
        app_info = stop_process(self.app)
        self.app = None
        if self.xvfb is not None:
            close_xvfb(self.xvfb)
            self.xvfb = None
        for handle in (getattr(self, "_settings_log", None), getattr(self, "_xvfb_log", None)):
            if handle is not None:
                handle.close()
        return app_info

    def repro(self, kind: str, detail: str) -> None:
        layout = read_layout(self.layout_path)
        write_repro(self.evidence, {
            "case": self.name,
            "classification": kind,
            "detail": detail,
            "display": self.display,
            "argv": getattr(self, "settings_argv", []),
            "binary": str(BIN / "decsettings"),
            "renderer": str(BIN / "decthemectl"),
            "fontconfig": str(self.evidence / "fonts.conf"),
            "themes": list(FIXTURE_THEMES),
            "font_dirs": [
                str(THEMES / name / "fonts") for name in FONT_THEMES
            ],
            "xid": self.xid,
            "settings_log_tail": log_tail(self.evidence / "settings.log"),
            "xvfb_log_tail": log_tail(self.evidence / "xvfb.log"),
            "last_layout_pane": None if layout is None else layout.get("pane"),
            "last_frames": None if layout is None else layout.get("frames"),
            "last_painted": None if layout is None else layout.get("theme_previews_painted"),
            "enospc_exercised": False,
            "desktop_environment_benchmark": False,
        })


def assert_hashes() -> dict:
    if not ARTIFACT.is_absolute() or not ARTIFACT.is_dir():
        raise Fault("orchestration", f"artifact root is not an absolute directory: {ARTIFACT}")
    if not SUMS.is_file():
        raise Fault("orchestration", f"missing {SUMS}")
    sums = load_sums(SUMS)
    checked = {}
    for name, pinned in PINNED_HASHES.items():
        path = BIN / name
        if not path.is_file():
            raise Fault("orchestration", f"missing binary {path}")
        listed = sums.get(name)
        if listed != pinned:
            raise Fault("orchestration", f"{name} SHA256SUMS {listed} != pinned {pinned}")
        digest = sha256_file(path)
        if digest != pinned:
            raise Fault("orchestration", f"{name} hash {digest} != pinned {pinned}")
        checked[name] = digest
    return checked


def case_rapid(evidence: Path) -> dict:
    case = Case("rapid_nav", evidence)
    samples = []
    switches = []
    try:
        case.start_display()
        case.start_settings("general")
        xid = case.wait_window()
        run_text(["xdotool", "windowsize", xid, str(WINDOW[0]), str(WINDOW[1])], case.env, 4)
        layout = case.wait_layout()
        order = ("appearance", "wallpaper")
        deadline = time.monotonic() + 16
        index = 0
        while time.monotonic() < deadline and index < 6:
            target = order[index % len(order)]
            began = time.monotonic()
            if window_name(case.env, xid) is None or case.app.poll() is not None:
                raise Fault("product", "Settings vanished during rapid navigation")
            layout = click_pane(case.env, xid, case.layout_path, layout, target)
            seen = None
            while time.monotonic() - began < 2.5:
                layout = read_layout(case.layout_path) or layout
                if layout.get("pane") == target:
                    seen = layout
                    break
                time.sleep(0.05)
            latency = round(time.monotonic() - began, 3)
            if target == "appearance" and seen:
                click_theme_tab(case.env, xid, seen)
            sample = snapshot(seen or layout, case.cache, case.app, case.started)
            sample["switch_target"] = target
            sample["switch_latency_s"] = latency
            sample["switch_reached"] = bool(seen)
            samples.append(sample)
            switches.append({"target": target, "latency_s": latency, "reached": bool(seen)})
            if not seen:
                case.notes.append(f"pane {target} not reached in 2.5s")
            index += 1
        resources = case.sample_cpu()
        capture = case.capture("rapid")
        painted = [item.get("theme_previews_painted") or 0 for item in samples]
        pngs = [item.get("pngs") or 0 for item in samples]
        domino = False
        if any(value > 0 for value in painted):
            peak = max(painted)
            if painted[-1] == 0 and peak > 0 and samples[-1].get("pane") == "appearance":
                domino = True
        if any(value > 0 for value in pngs) and pngs[-1] == 0 and samples[-1].get("pane") == "appearance":
            domino = True
        if domino:
            raise Fault("product", "thumbnail count fell to zero while Appearance stayed open")
        vanished = case.app.poll() is not None or window_name(case.env, xid) is None
        if vanished:
            raise Fault("product", "Settings vanished after rapid navigation")
        return {
            "status": "observed",
            "display": case.display,
            "switches": switches,
            "samples": samples,
            "capture": capture,
            "resources": resources,
            "notes": case.notes,
            "settings_alive": True,
        }
    except Fault as exc:
        case.repro(exc.kind, exc.detail)
        return {"status": exc.kind, "detail": exc.detail, "samples": samples, "switches": switches, "notes": case.notes}
    finally:
        case.close()


def png_stats(cache: Path) -> list[dict]:
    rows = []
    if not cache.is_dir():
        return rows
    for path in sorted(cache.glob("*.png")):
        st = path.stat()
        rows.append({
            "name": path.name,
            "inode": st.st_ino,
            "mtime_ns": st.st_mtime_ns,
            "size": st.st_size,
            **(png_geometry(path) or {}),
        })
    return rows


def case_warm(evidence: Path) -> dict:
    case = Case("warm_cache", evidence)
    phases = []
    resources = None
    try:
        case.start_display()
        for phase, budget in (("cold", 12), ("warm", 8)):
            if phase == "warm":
                stop_process(case.app)
                case.layout_path.unlink(missing_ok=True)
            case.start_settings("appearance")
            xid = case.wait_window()
            run_text(["xdotool", "windowsize", xid, str(WINDOW[0]), str(WINDOW[1])], case.env, 4)
            layout = case.wait_layout()
            click_theme_tab(case.env, xid, layout)
            began = time.monotonic()
            first = None
            last = layout
            while time.monotonic() - began < budget:
                if case.app.poll() is not None or window_name(case.env, xid) is None:
                    raise Fault("product", f"Settings vanished during {phase} preview")
                last = read_layout(case.layout_path) or last
                stats = png_stats(case.cache)
                if stats and first is None:
                    first = round(time.monotonic() - began, 3)
                    if phase == "cold":
                        resources = case.sample_cpu()
                    break
                time.sleep(0.1)
            phases.append({
                "phase": phase,
                "first_png_s": first,
                "sample": snapshot(last, case.cache, case.app, began),
                "pngs": png_stats(case.cache),
                "capture": case.capture(phase),
            })
        if resources is None:
            resources = case.sample_cpu()
        cold = {item["name"]: item for item in phases[0]["pngs"]}
        warm = {item["name"]: item for item in phases[1]["pngs"]}
        reused = []
        rewritten = []
        for name, before in cold.items():
            after = warm.get(name)
            if after is None:
                continue
            same = (
                after["inode"] == before["inode"]
                and after["mtime_ns"] == before["mtime_ns"]
                and after["size"] == before["size"]
            )
            (reused if same else rewritten).append(name)
        return {
            "status": "observed",
            "display": case.display,
            "phases": phases,
            "reused_unchanged": reused,
            "rewritten": rewritten,
            "resources": resources,
            "notes": case.notes,
        }
    except Fault as exc:
        case.repro(exc.kind, exc.detail)
        return {"status": exc.kind, "detail": exc.detail, "phases": phases, "notes": case.notes}
    finally:
        case.close()


def case_eacces(evidence: Path) -> dict:
    case = Case("cache_eacces", evidence)
    try:
        cache_root = case.root / "cache" / "decebal"
        cache_root.mkdir(parents=True, mode=0o755)
        case.cache.mkdir(mode=0o755)
        os.chmod(case.cache, 0o555)
        os.chmod(cache_root, 0o555)
        case.start_display()
        case.start_settings("appearance")
        xid = case.wait_window()
        deadline = time.monotonic() + 8
        samples = []
        while time.monotonic() < deadline:
            if case.app.poll() is not None or window_name(case.env, xid) is None:
                raise Fault("product", "Settings vanished when the private preview cache was not writable")
            samples.append(snapshot(read_layout(case.layout_path), case.cache, case.app, case.started))
            time.sleep(0.4)
        created = list(case.cache.glob("*.png")) if case.cache.is_dir() else []
        return {
            "status": "observed",
            "fault": "private_directory_mode_0555",
            "enospc_exercised": False,
            "settings_alive": case.app.poll() is None,
            "pngs_written": len(created),
            "samples": samples,
            "resources": case.sample_cpu(),
            "capture": case.capture("eacces"),
            "notes": case.notes,
        }
    except Fault as exc:
        case.repro(exc.kind, exc.detail)
        return {"status": exc.kind, "detail": exc.detail, "enospc_exercised": False, "notes": case.notes}
    finally:
        cache_root = case.root / "cache" / "decebal"
        for path in (case.cache, cache_root):
            if path.exists():
                os.chmod(path, 0o700)
        case.close()


def case_pressure(evidence: Path) -> dict:
    case = Case("cache_pressure", evidence)
    try:
        case.cache.mkdir(parents=True, mode=0o700)
        for index in range(40):
            (case.cache / f"pressure-{index:02d}.png").write_bytes(tiny_png(4, 2))
        before = len(list(case.cache.glob("*.png")))
        case.start_display()
        case.start_settings("appearance")
        xid = case.wait_window()
        deadline = time.monotonic() + 8
        samples = []
        while time.monotonic() < deadline:
            if case.app.poll() is not None or window_name(case.env, xid) is None:
                raise Fault("product", "Settings vanished under bounded private cache pressure")
            samples.append(snapshot(read_layout(case.layout_path), case.cache, case.app, case.started))
            time.sleep(0.4)
        after = len(list(case.cache.glob("*.png")))
        return {
            "status": "observed",
            "seeded_pngs": before,
            "pngs_after": after,
            "growth": after - before,
            "enospc_exercised": False,
            "settings_alive": True,
            "samples": samples,
            "resources": case.sample_cpu(),
            "notes": case.notes,
        }
    except Fault as exc:
        case.repro(exc.kind, exc.detail)
        return {"status": exc.kind, "detail": exc.detail, "enospc_exercised": False, "notes": case.notes}
    finally:
        case.close()


def case_close(evidence: Path) -> dict:
    case = Case("close_queued", evidence)
    try:
        case.start_display()
        case.start_settings("appearance")
        xid = case.wait_window()
        layout = case.wait_layout()
        click_theme_tab(case.env, xid, layout)
        began = time.monotonic()
        queued = False
        while time.monotonic() - began < 4:
            layout = read_layout(case.layout_path) or layout
            painted = layout.get("theme_previews_painted") or 0
            cards = sum(str(item.get("id", "")).startswith("l:tcard:") for item in (layout.get("ctls") or []))
            if painted or cards or any(case.cache.glob("*.png")):
                queued = True
                break
            time.sleep(0.05)
        if case.app.poll() is not None:
            raise Fault("product", "Settings exited before the queued-close signal")
        info = stop_process(case.app)
        case.app = None
        leftover = []
        if info.get("pid"):
            leftover = proc_children(info["pid"])
        window_gone = window_name(case.env, xid) is None
        return {
            "status": "observed",
            "preview_work_seen": queued,
            "stop": info,
            "leftover_children": leftover,
            "window_gone": window_gone,
            "notes": case.notes,
        }
    except Fault as exc:
        case.repro(exc.kind, exc.detail)
        return {"status": exc.kind, "detail": exc.detail, "notes": case.notes}
    finally:
        case.close()


def main() -> int:
    if EVIDENCE.exists() or EVIDENCE.is_symlink():
        print(
            f"preview-pressure: evidence path already exists, use a new directory: {EVIDENCE}",
            file=sys.stderr,
        )
        return 2
    try:
        EVIDENCE.mkdir(parents=True, exist_ok=False)
    except OSError as exc:
        print(f"preview-pressure: use a new evidence directory: {exc}", file=sys.stderr)
        return 2
    try:
        hashes = assert_hashes()
    except Fault as exc:
        write_repro(EVIDENCE, {
            "case": "hash",
            "classification": exc.kind,
            "detail": exc.detail,
            "enospc_exercised": False,
            "desktop_environment_benchmark": False,
        })
        print(f"preview-pressure: {exc.kind}: {exc.detail}", file=sys.stderr)
        return 2
    theme_stamp = {}
    for name in FIXTURE_THEMES:
        path = THEMES / name / "theme.toml"
        if path.is_file():
            theme_stamp[name] = sha256_file(path)
    font_stamp = {}
    for name in FONT_THEMES:
        directory = THEMES / name / "fonts"
        if not directory.is_dir():
            continue
        font_stamp[name] = sorted(
            item.name + ":" + sha256_file(item)
            for item in directory.iterdir() if item.is_file()
        )
    report = {
        "schema": "decebal.preview-pressure.v1",
        "desktop_environment_benchmark": False,
        "enospc_exercised": False,
        "artifact": str(ARTIFACT),
        "source_commit": SOURCE_COMMIT,
        "close_case": "harness pidfd SIGTERM, not a GUI close",
        "cache_pressure_case": "40 tiny local PNG seeds, not memory or disk pressure",
        "hashes": hashes,
        "fixture_themes": list(FIXTURE_THEMES),
        "fixture_theme_sha256": theme_stamp,
        "font_bundle_sha256": font_stamp,
        "font_bundle": "artifact theme font directories only, via FONTCONFIG_FILE",
        "cases": {},
    }
    runners = (
        ("rapid_nav", case_rapid),
        ("warm_cache", case_warm),
        ("cache_eacces", case_eacces),
        ("cache_pressure", case_pressure),
        ("close_queued", case_close),
    )
    exit_code = 0
    for name, runner in runners:
        print(f"preview-pressure: {name}", flush=True)
        try:
            result = runner(EVIDENCE)
        except FileExistsError as exc:
            result = {
                "status": "orchestration",
                "detail": f"refusing existing evidence path: {exc.filename}",
            }
        except Exception as exc:
            detail = "".join(traceback.format_exception_only(type(exc), exc)).strip()
            write_repro(EVIDENCE / name, {
                "case": name,
                "classification": "orchestration",
                "detail": detail,
                "traceback": traceback.format_exc()[-2000:],
                "enospc_exercised": False,
                "desktop_environment_benchmark": False,
            })
            result = {"status": "orchestration", "detail": detail}
        report["cases"][name] = result
        if result.get("status") == "product" and exit_code == 0:
            exit_code = 1
        if result.get("status") == "orchestration":
            exit_code = 2
    report["exit"] = exit_code
    (EVIDENCE / "summary.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"preview-pressure: evidence {EVIDENCE / 'summary.json'} exit {exit_code}")
    return exit_code


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Fault as exc:
        print(f"preview-pressure: {exc.kind}: {exc.detail}", file=sys.stderr)
        sys.exit(2 if exc.kind == "orchestration" else 1)
