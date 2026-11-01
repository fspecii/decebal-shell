#!/usr/bin/env python3
"""DRAFT (never executed): native Wayland gate for decnotify (docs/wayland-notifications.md).

Everything is private and owned: a fresh evidence directory (refused when it exists, never deleted), a private HOME and
XDG tree, a private session bus with a fixed configuration (no service activation), and a headless Sway with two outputs
(HEADLESS-1 at scale 1.5, HEADLESS-2 at scale 1). decnotify runs with DISPLAY unset and WAYLAND_DEBUG=client; the
checks read the wire trace, the daemon's opt-in debug JSON (DECNOTIFY_DEBUG_BANNERS / _HISTORY), D-Bus signals recorded
by dbus-monitor on the private bus, and keystrokes/clicks seen by a native foot window. Sockets are checked with kernel
peer credentials before any control command is sent to them; cleanup uses pidfds.

usage: wayland_notifications.py --bin <immutable decnotify> --out <NEW short dir> [--phases a,b,...] [--keep]
phases: contract bus queue actions history persistence dnd themes scales burst churn pinned
Needs: sway swaymsg dbus-daemon dbus-monitor busctl foot wtype. The evidence directory must be short (<= 60 bytes):
the private sockets live under it and Unix socket paths are limited to 108 bytes.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import time

HERE = Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("popup_fixture", HERE / "wayland_compact_popup.py")
POPUP = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(POPUP)
wait_for, requests, socket_owned, rescue_tagged, FORCED = POPUP.wait_for, POPUP.requests, POPUP.socket_owned, POPUP.rescue_tagged, POPUP.FORCED

ALL_PHASES = ["contract", "bus", "queue", "actions", "history", "persistence", "dnd", "themes", "scales", "burst", "churn", "pinned"]
MAX_BUFFERS, MAX_CALLBACKS = 3, 2  # notify::present limits
POOL_IDLE_BYTES = 0  # every buffer owns its pool, so nothing is left once the buffers are gone
NAMESPACE = "decebal-notifications"
CATCHER_NAMESPACE = "decebal-notifications-outside"
LAYER_TOP, LAYER_OVERLAY = 2, 3
ANCHOR_TOP_RIGHT = 1 | 8
BUS = "org.freedesktop.Notifications"
OBJ = "/org/freedesktop/Notifications"

SWAY_CONFIG = """
xwayland disable
output HEADLESS-1 resolution 1920x1080 position 0 0 scale 1.5
output HEADLESS-2 resolution 1920x1080 position 1280 0 scale 1
for_window [app_id="decnotify-sink"] move to output HEADLESS-1
"""

BUS_CONFIG = """<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig><type>session</type><listen>unix:path={path}</listen><auth>EXTERNAL</auth>
<policy context="default"><allow send_destination="*" eavesdrop="true"/><allow eavesdrop="true"/><allow own="*"/></policy>
</busconfig>
"""

# A native window that records the raw bytes it receives (keys and mouse reports) to a file.
SINK = r"""
import os, sys, termios, tty
out, ready = sys.argv[1], sys.argv[2]
fd = sys.stdin.fileno()
old = termios.tcgetattr(fd)
tty.setraw(fd)
sys.stdout.write("\x1b[?1000h\x1b[?1006h"); sys.stdout.flush()
open(ready, "w").write("1")
with open(out, "ab", buffering=0) as log:
    while True:
        data = os.read(fd, 64)
        if not data:
            break
        log.write(data)
"""

EVT = re.compile(r"<- (?P<obj>[a-z0-9_]+)@(?P<id>\d+)\.(?P<ev>[a-z_]+), \((?P<args>.*)\)\s*$")
SIGNAL = re.compile(r"^signal .*member=(?P<member>\w+)")
ARG = re.compile(r"^\s+(?P<type>uint32|string)\s+(?P<value>.*)$")


# ------------------------------------------------------------------------------ trace parsing (unit tested)

def events(log):
    out = []
    for line in log.splitlines():
        m = EVT.search(line)
        if m:
            out.append((m["obj"], int(m["id"]), m["ev"], [a.strip() for a in m["args"].split(",")] if m["args"] else []))
    return out


def unquote(value):
    value = value.strip()
    if value.startswith("Some(") and value.endswith(")"):
        value = value[5:-1]
    return value.strip('"')


def layer_surfaces(reqs):
    """Every get_layer_surface: its layer object, wl_surface, output ('nil' or wl_output@N), layer and namespace."""
    found = []
    for obj, _, req, args in reqs:
        if obj == "zwlr_layer_shell_v1" and req == "get_layer_surface" and len(args) >= 5:
            layer_id = int(args[0].split("@")[1])
            found.append({"layer": layer_id, "surface": int(args[1].split("@")[1]), "output": args[2], "level": int(args[3]), "namespace": unquote(args[4])})
    return found


def layer_requests(reqs, layer_id):
    """Requests sent to one zwlr_layer_surface_v1, as {name: [args, ...]} in order."""
    out = {}
    for obj, ident, req, args in reqs:
        if obj == "zwlr_layer_surface_v1" and ident == layer_id:
            out.setdefault(req, []).append(args)
    return out


def region_adds(reqs):
    adds = {}
    for obj, ident, req, _ in reqs:
        if obj == "wl_region" and req == "add":
            adds[ident] = adds.get(ident, 0) + 1
    return adds


def surface_order(reqs, surface_id):
    """('set_input_region', region|'nil') / ('attach', ...) / ('commit',) for one wl_surface, in wire order."""
    steps = []
    for obj, ident, req, args in reqs:
        if obj == "wl_surface" and ident == surface_id and req in ("set_input_region", "attach", "commit", "set_buffer_scale", "destroy"):
            steps.append((req, *args))
    return steps


def check_banner_contract(trace, failures):
    """Top layer, top+right anchor, exclusive zone 0, no keyboard, an explicit empty input region before any buffer."""
    reqs = requests(trace)
    adds = region_adds(reqs)
    banners = [s for s in layer_surfaces(reqs) if s["namespace"] == NAMESPACE and s["level"] == LAYER_TOP]
    if not banners:
        failures.append("contract: no Layer::Top surface in namespace " + NAMESPACE)
    evidence = []
    for s in banners:
        state = layer_requests(reqs, s["layer"])
        anchor = [int(a[0]) for a in state.get("set_anchor", [])]
        zone = [int(a[0]) for a in state.get("set_exclusive_zone", [])]
        keyboard = [int(a[0]) for a in state.get("set_keyboard_interactivity", [])]
        if anchor[:1] != [ANCHOR_TOP_RIGHT]:
            failures.append(f"contract: banner anchor {anchor}, expected top|right ({ANCHOR_TOP_RIGHT})")
        if zone[:1] != [0]:
            failures.append(f"contract: banner exclusive zone {zone}, expected 0")
        if keyboard[:1] != [0] or any(k != 0 for k in keyboard):
            failures.append(f"contract: banner keyboard interactivity {keyboard}, expected none (0)")
        steps = surface_order(reqs, s["surface"])
        regions = [st for st in steps if st[0] == "set_input_region"]
        first_attach = next((i for i, st in enumerate(steps) if st[0] == "attach" and st[1] != "nil"), None)
        if not regions or regions[0][1] == "nil":
            failures.append(f"contract: surface {s['surface']} never gets an explicit input region")
        else:
            region_id = int(regions[0][1].split("@")[1])
            if adds.get(region_id, 0) != 0:
                failures.append(f"contract: first input region of surface {s['surface']} is not empty")
            if first_attach is not None and steps.index(regions[0]) > first_attach:
                failures.append(f"contract: surface {s['surface']} attached a buffer before its explicit empty region")
        evidence.append({"surface": s["surface"], "anchor": anchor, "zone": zone, "keyboard": keyboard, "margins": state.get("set_margin", [])})
    return evidence


def check_no_banner_keyboard(trace, failures):
    reqs = requests(trace)
    banner_surfaces = {s["surface"] for s in layer_surfaces(reqs) if s["namespace"] == NAMESPACE and s["level"] == LAYER_TOP}
    entered = [a for obj, _, ev, a in events(trace) if obj == "wl_keyboard" and ev == "enter" and len(a) > 1 and int(a[1].split("@")[1]) in banner_surfaces]
    if entered:
        failures.append(f"actions: the keyboard entered a banner surface: {entered}")
    return {"banner_surfaces": sorted(banner_surfaces), "keyboard_enters": len(entered)}


def check_history_contract(trace, outputs, failures):
    """Overlay panel with an exclusive keyboard and one catcher below it per output."""
    reqs = requests(trace)
    layers = layer_surfaces(reqs)
    panels = [s for s in layers if s["namespace"] == NAMESPACE and s["level"] == LAYER_OVERLAY]
    catchers = [s for s in layers if s["namespace"] == CATCHER_NAMESPACE]
    if not panels:
        failures.append("history: no Layer::Overlay panel")
    for s in panels:
        keyboard = [int(a[0]) for a in layer_requests(reqs, s["layer"]).get("set_keyboard_interactivity", [])]
        if keyboard[:1] != [1]:
            failures.append(f"history: panel keyboard interactivity {keyboard}, expected exclusive (1)")
    if len({c["output"] for c in catchers}) < outputs:
        failures.append(f"history: catchers on {len({c['output'] for c in catchers})} outputs, expected {outputs}")
    for c in catchers:
        if c["level"] >= LAYER_OVERLAY:
            failures.append("history: a catcher is not below the panel's layer")
    return {"panels": len(panels), "catchers": len(catchers)}


def check_scale_protocol(trace, scale, failures):
    """Integer scales use a buffer scale; a fractional one a viewport destination with buffer scale 1."""
    reqs = requests(trace)
    buffer_scales = {int(a[0]) for obj, _, req, a in reqs if obj == "wl_surface" and req == "set_buffer_scale" and a}
    destinations = [tuple(int(v) for v in a) for obj, _, req, a in reqs if obj == "wp_viewport" and req == "set_destination" and len(a) == 2]
    if float(scale).is_integer():
        if int(scale) not in buffer_scales:
            failures.append(f"scales: no set_buffer_scale({int(scale)}) for an integer scale ({sorted(buffer_scales)})")
    else:
        positive = [d for d in destinations if d[0] > 0]
        if not positive:
            failures.append("scales: a fractional scale sent no wp_viewport destination")
        if buffer_scales - {1, 0}:
            # surfaces from an earlier scale may remain in the trace; only the latest matters, so report not fail
            pass
    return {"buffer_scales": sorted(buffer_scales), "destinations": destinations[-4:]}


def parse_signals(text):
    """dbus-monitor output to [(member, [args])]; only uint32 and string arguments are kept."""
    out = []
    for line in text.splitlines():
        m = SIGNAL.match(line)
        if m:
            out.append((m["member"], []))
            continue
        a = ARG.match(line)
        if a and out:
            out[-1][1].append(int(a["value"]) if a["type"] == "uint32" else a["value"].strip().strip('"'))
    return out


def trace_count(trace, obj, req):
    return sum(1 for o, _, r, _ in requests(trace) if o == obj and r == req)


def wire(trace):
    """Requests and events of a WAYLAND_DEBUG=client trace in wire order: ('->' | '<-', object, id, name, args)."""
    out = []
    for line in trace.splitlines():
        m = POPUP.REQ.search(line)
        if m:
            out.append(("->", m["obj"], int(m["id"]), m["req"], [a.strip() for a in m["args"].split(",")] if m["args"] else []))
            continue
        e = EVT.search(line)
        if e:
            out.append(("<-", e["obj"], int(e["id"]), e["ev"], [a.strip() for a in e["args"].split(",")] if e["args"] else []))
    return out


def buffer_peaks(trace):
    """({surface: most shm buffers alive at once for it, counting buffers the compositor still held}, buffers created)."""
    owner, live, peak, created = {}, {}, {}, 0
    for way, obj, ident, name, args in wire(trace):
        if way != "->":
            continue
        if obj == "wl_shm_pool" and name == "create_buffer" and args:
            created += 1
            b = int(args[0].split("@")[1])
            old = owner.pop(b, None)  # a recycled protocol id is a new buffer
            if old is not None:
                live[old].discard(b)
        elif obj == "wl_surface" and name == "attach" and args and args[0].startswith("wl_buffer@"):
            b = int(args[0].split("@")[1])
            if b not in owner:
                owner[b] = ident
                live.setdefault(ident, set()).add(b)
                peak[ident] = max(peak.get(ident, 0), len(live[ident]))
        elif obj == "wl_buffer" and name == "destroy" and ident in owner:
            live[owner.pop(ident)].discard(ident)
    return peak, created


def callback_peaks(trace):
    """{surface: most frame callbacks outstanding at once} and the number requested."""
    owner, out, peak, requested = {}, {}, {}, 0
    for way, obj, ident, name, args in wire(trace):
        if way == "->" and obj == "wl_surface" and name == "frame" and args:
            requested += 1
            owner[int(args[0].split("@")[1])] = ident
            out[ident] = out.get(ident, 0) + 1
            peak[ident] = max(peak.get(ident, 0), out[ident])
        elif way == "<-" and obj == "wl_callback" and name == "done" and ident in owner:
            surface = owner.pop(ident)
            out[surface] -= 1
    return peak, requested


def shm_mapping_bytes(maps):
    """Independent kernel mapping size, without retaining or reporting mapping paths."""
    total = 0
    for line in maps.splitlines():
        fields = line.split(maxsplit=5)
        if len(fields) == 6 and fields[5].startswith('/memfd:smithay-client-toolkit'):
            start, end = (int(v, 16) for v in fields[0].split('-'))
            total += end - start
    return total


def check_presentation_bounds(trace, present, failures, label):
    """Per surface at most 3 buffers and 2 frame callbacks alive, on the wire and in the daemon's own counters."""
    buffers, created = buffer_peaks(trace)
    callbacks, requested = callback_peaks(trace)
    for surface, peak in buffers.items():
        if peak > MAX_BUFFERS:
            failures.append(f"{label}: surface {surface} had {peak} shm buffers alive (bound {MAX_BUFFERS})")
    for surface, peak in callbacks.items():
        if peak > MAX_CALLBACKS:
            failures.append(f"{label}: surface {surface} had {peak} frame callbacks outstanding (bound {MAX_CALLBACKS})")
    if present:
        totals, limits = present["totals"], present["limits"]
        if totals["max_buffers"] > limits["buffers"] or totals["max_callbacks"] > limits["callbacks"]:
            failures.append(f"{label}: the daemon reports {totals['max_buffers']} buffers / {totals['max_callbacks']} callbacks, past its own limits")
        for s in present["surfaces"]:
            if s["buffers"] > limits["buffers"] or s.get("alloc_bytes", s["bytes"]) > limits["surface_bytes"]:
                failures.append(f"{label}: surface {s['id']} holds {s['buffers']} buffers / {s.get('alloc_bytes', s['bytes'])} shm bytes")
        if present["total_bytes"] > limits["total_bytes"]:
            failures.append(f"{label}: {present['total_bytes']} shm bytes in use, past the {limits['total_bytes']} budget")
        if present.get("total_buffers", 0) > limits.get("total_buffers", 64):
            failures.append(f"{label}: total retained pool count exceeds its budget")
        # every buffer owns an exact-size pool: the pool lengths the library reports must stay within the modelled bytes
        if present.get("pool_bytes", 0) > present["total_bytes"]:
            failures.append(f"{label}: pools total {present['pool_bytes']} bytes, past the {present['total_bytes']} the budget counted")
    return {"buffer_peaks": buffers, "callback_peaks": callbacks, "buffers_created": created, "callbacks_requested": requested}


def closed_reasons(signals):
    return {args[0]: args[1] for member, args in signals if member == "NotificationClosed" and len(args) == 2}


def actions_invoked(signals):
    return [(args[0], args[1]) for member, args in signals if member == "ActionInvoked" and len(args) == 2]


def button_center(banner, key, scale, origin=(0, 0)):
    """Global logical coordinates of a banner action button's centre. `banner` is one `active` entry of the debug JSON
    (device pixels inside the usable area); `origin` is that area's logical offset on the compositor layout."""
    button = next(b for b in banner["buttons"] if b["key"] == key)
    bx, by, bw, bh = banner["rect"]
    x, y, w, h = button["rect"]
    return origin[0] + (bx + x + w / 2) / scale, origin[1] + (by + y + h / 2) / scale


def close_center(banner, scale, origin=(0, 0)):
    bx, by, _, _ = banner["rect"]
    x, y, w, h = banner["close_rect"]
    return origin[0] + (bx + x + w / 2) / scale, origin[1] + (by + y + h / 2) / scale


# ------------------------------------------------------------------------------ process plumbing

class Rig:
    """Owns every process of one run; nothing is signalled that is not a pidfd of ours."""

    def __init__(self, root, binary, token):
        self.root, self.token = root, token
        self.run = root / "run"
        self.bin = root / "bin"
        self.children = []
        self.failures = []
        self.evidence = {}
        self.binary = binary
        self.base = {k: v for k, v in os.environ.items() if not k.startswith(("XDG_", "DECEBAL_", "DECNOTIFY_", "RDWM_", "DBUS_")) and k not in ("WAYLAND_DISPLAY", "DISPLAY", "SWAYSOCK", "XAUTHORITY")}
        self.base.update(XDG_RUNTIME_DIR=str(self.run), HOME=str(root / "home"), XDG_CONFIG_HOME=str(root / "cfg"), XDG_CACHE_HOME=str(root / "cache"),
                         XDG_DATA_HOME=str(root / "data"), XDG_STATE_HOME=str(root / "state"), XDG_DATA_DIRS=str(root / "data"),
                         DECEBAL_CONFIG=str(root / "cfg" / "decebal"), DECEBAL_THEMES=str(root / "themes"), DECEBAL_SYSTEM_THEMES=str(root / "themes"),
                         DECEBAL_SYSTEM_AUTOMATIC="0", DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={self.run}/absent-system-bus", DBUS_SESSION_BUS_ADDRESS=f"unix:path={self.run}/absent-session-bus", RDWM_SOCK=str(self.run / "absent-rdwm"), PULSE_SERVER=f"unix:{self.run}/absent-pulse",
                         DECEBAL_NATIVE_TOKEN=token, PATH=str(self.bin) + os.pathsep + os.defpath)
        for d in ("home", "cfg/decebal", "cache", "data", "state/decebal", "themes"):
            (root / d).mkdir(parents=True, exist_ok=True)
        self.bus_address = f"unix:path={self.run}/bus"
        self.sway = self.wayland = self.ipc = self.bus = self.daemon = None
        self.sink = self.sink_log = self.monitor = self.monitor_log = None

    # -- generic child handling
    def spawn(self, label, argv, env, stdout=None):
        err = (self.root / f"{label}.log").open("w")
        try:
            proc = subprocess.Popen(argv, env=env, stdout=stdout or subprocess.DEVNULL, stderr=err, start_new_session=True)
        finally:
            err.close()
        self.children.append((label, proc, os.pidfd_open(proc.pid)))
        return proc

    def stop(self, proc):
        for _, child, fd in self.children:
            if child is not proc or child.poll() is not None:
                continue
            signal.pidfd_send_signal(fd, signal.SIGTERM)
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                FORCED.append(child.pid)
                signal.pidfd_send_signal(fd, signal.SIGKILL)
                child.wait(timeout=5)

    def close(self):
        # Signal every owned child first: cleanup has one bounded grace period,
        # rather than up to five seconds for each sequential child.
        for label, proc, fd in reversed(self.children):
            try:
                if proc.poll() is None:
                    signal.pidfd_send_signal(fd, signal.SIGTERM)
            except ProcessLookupError:
                pass
            except Exception as error:
                self.fail(f"cleanup signal {label}: {error!r}")
        deadline = time.monotonic() + 6
        for label, proc, fd in reversed(self.children):
            try:
                if proc.poll() is None:
                    try:
                        proc.wait(timeout=max(.05, deadline - time.monotonic()))
                    except subprocess.TimeoutExpired:
                        FORCED.append(proc.pid)
                        signal.pidfd_send_signal(fd, signal.SIGKILL)
                        proc.wait(timeout=1)
            except Exception as error:
                self.fail(f"cleanup wait {label}: {error!r}")
            finally:
                try:
                    os.close(fd)
                except OSError as error:
                    self.fail(f"cleanup descriptor {label}: {error!r}")

    # -- the private bus and compositor
    def start_bus(self):
        conf = self.root / "bus.conf"
        conf.write_text(BUS_CONFIG.format(path=self.run / "bus"))
        self.bus = self.spawn("dbus-daemon", [shutil.which("dbus-daemon"), f"--config-file={conf}", "--nofork", "--nopidfile"], dict(self.base))
        wait_for(lambda: socket_owned(self.bus, self.run / "bus"), 5, "owned private session bus")

    def start_sway(self):
        (self.root / "sway.conf").write_text(SWAY_CONFIG)
        env = dict(self.base, WLR_BACKENDS="headless", WLR_HEADLESS_OUTPUTS="2", WLR_LIBINPUT_NO_DEVICES="1", WLR_RENDERER="pixman")
        self.sway = self.spawn("sway", [shutil.which("sway"), "-c", str(self.root / "sway.conf")], env)
        sock = wait_for(lambda: next((p for p in self.run.glob("wayland-*") if not p.name.endswith(".lock")), None), 10, "sway socket")
        wait_for(lambda: socket_owned(self.sway, sock), 5, "owned compositor socket")
        self.wayland = sock
        self.ipc = wait_for(lambda: next((p for p in self.run.glob("sway-ipc.*.sock") if socket_owned(self.sway, p)), None), 5, "owned sway IPC")
        self.sway_cmd("focus", "output", "HEADLESS-1")

    def sway_cmd(self, *args, query=False):
        assert socket_owned(self.sway, self.ipc), "private sway IPC ownership changed"
        done = subprocess.run([shutil.which("swaymsg"), "-r", "-s", str(self.ipc), *args], env=self.base, capture_output=True, text=True, timeout=5, check=True)
        value = json.loads(done.stdout)
        if not query:
            assert isinstance(value, list) and value and all(v.get("success") for v in value), value
        return value

    def client_env(self, **extra):
        env = dict(self.base, WAYLAND_DISPLAY=self.wayland.name, XDG_SESSION_TYPE="wayland", DBUS_SESSION_BUS_ADDRESS=self.bus_address)
        env.update(extra)
        assert "DISPLAY" not in env, "the Wayland gate must run with DISPLAY unset"
        return env

    # -- decnotify and the recorders
    def start_daemon(self, label="decnotify", **extra):
        env = self.client_env(WAYLAND_DEBUG="client", DECNOTIFY_DEBUG_BANNERS=str(self.root / f"{label}-banners.json"),
                              DECNOTIFY_DEBUG_HISTORY=str(self.root / f"{label}-history.json"),
                              DECNOTIFY_DEBUG_WAYLAND=str(self.root / f"{label}-present.json"), **extra)
        proc = self.spawn(label, [str(self.bin / "decnotify")], env)
        self.daemon, self.daemon_label = proc, label
        wait_for(lambda: self.owner_alive(), 10, "decnotify owns " + BUS)
        return proc

    def owner_alive(self):
        done = subprocess.run([shutil.which("busctl"), f"--address={self.bus_address}", "call", BUS, OBJ, BUS, "GetServerInformation"], env=self.base, capture_output=True, text=True, timeout=5)
        return done.returncode == 0 and "decnotify" in done.stdout

    def trace(self, label=None):
        return (self.root / f"{label or self.daemon_label}.log").read_text()

    def banners(self, label=None):
        path = self.root / f"{label or self.daemon_label}-banners.json"
        return json.loads(path.read_text()) if path.exists() and path.stat().st_size else {"active": [], "queued": []}

    def present(self, label=None):
        """The daemon's presentation counters (buffers, frame callbacks, commits), or None before the first commit."""
        path = self.root / f"{label or self.daemon_label}-present.json"
        return json.loads(path.read_text()) if path.exists() and path.stat().st_size else None

    def shm_sample(self):
        # Popen keeps an exited child unreaped until poll(), preventing PID reuse
        # between the initial liveness check and these read-only /proc reads.
        assert self.daemon.poll() is None, "cannot sample an exited notification daemon"
        proc = Path(f"/proc/{self.daemon.pid}")
        sample = {"mapped_shm_bytes": shm_mapping_bytes((proc / "maps").read_text()),
                  "fd_count": len(list((proc / "fd").iterdir())), "rss_kb": self.rss_kb()}
        assert self.daemon.poll() is None, "notification daemon exited while sampled"
        return sample

    def rss_kb(self):
        for line in Path(f"/proc/{self.daemon.pid}/status").read_text().splitlines():
            if line.startswith("VmRSS:"):
                return int(line.split()[1])
        return 0

    def history(self, label=None):
        path = self.root / f"{label or self.daemon_label}-history.json"
        return json.loads(path.read_text()) if path.exists() and path.stat().st_size else None

    def start_monitor(self):
        self.monitor_log = self.root / "signals.txt"
        out = self.monitor_log.open("w")
        try:
            argv = [shutil.which("dbus-monitor"), f"--address", self.bus_address, f"type='signal',interface='{BUS}'"]
            self.monitor = self.spawn("dbus-monitor", argv, self.base, stdout=out)
        finally:
            out.close()
        time.sleep(0.3)

    def signals(self):
        return parse_signals(self.monitor_log.read_text())

    def start_sink(self):
        self.sink_log, ready = self.root / "sink.bin", self.root / "sink.ready"
        self.sink = self.spawn("sink", [shutil.which("foot"), "--config=/dev/null", "--app-id=decnotify-sink", sys.executable, "-I", "-u", "-c", SINK, str(self.sink_log), str(ready)], self.client_env())
        wait_for(lambda: ready.exists(), 8, "native sink mapped")
        node = wait_for(self.sink_node, 5, "sink in the tree")
        self.sway_cmd(f'[con_id={node["id"]}] focus')

    def sink_node(self):
        def walk(n):
            if n.get("app_id") == "decnotify-sink":
                return n
            for c in n.get("nodes", []) + n.get("floating_nodes", []):
                hit = walk(c)
                if hit:
                    return hit
        return walk(self.sway_cmd("-t", "get_tree", query=True))

    def sink_bytes(self):
        return self.sink_log.read_bytes() if self.sink_log.exists() else b""

    def type(self, *args):
        assert socket_owned(self.sway, self.wayland), "private compositor ownership changed"
        subprocess.run([shutil.which("wtype"), *args], env=self.client_env(), capture_output=True, timeout=5, check=True)

    def click(self, x, y):
        self.sway_cmd("seat", "seat0", "cursor", "set", str(round(x)), str(round(y)))
        self.sway_cmd("seat", "seat0", "cursor", "press", "button1")
        self.sway_cmd("seat", "seat0", "cursor", "release", "button1")

    def notify(self, app="fixture", summary="Hello", body="Body", actions=(), replaces=0, timeout=60000, hints=()):
        acts = [str(len(actions) * 2)] + [v for pair in actions for v in pair]
        hint = [str(len(hints))] + [str(v) for h in hints for v in h]
        argv = [shutil.which("busctl"), f"--address={self.bus_address}", "call", BUS, OBJ, BUS, "Notify", "susssasa{sv}i", app, str(replaces), "", summary, body, *acts, *hint, str(timeout)]
        done = subprocess.run(argv, env=self.base, capture_output=True, text=True, timeout=5, check=True)
        return int(done.stdout.split()[1])

    def call(self, member, sig="", *args):
        argv = [shutil.which("busctl"), f"--address={self.bus_address}", "call", BUS, OBJ, BUS, member, sig, *map(str, args)]
        return subprocess.run(argv, env=self.base, capture_output=True, text=True, timeout=5, check=True).stdout

    def ext(self, member, sig="", *args):
        argv = [shutil.which("busctl"), f"--address={self.bus_address}", "call", BUS, OBJ, "org.decebal.Notifications", member, sig, *map(str, args)]
        return subprocess.run(argv, env=self.base, capture_output=True, text=True, timeout=5)

    def cli(self, *args):
        return subprocess.run([str(self.bin / "decnotify"), *args], env=self.client_env(), capture_output=True, text=True, timeout=10)

    def fail(self, message):
        self.failures.append(message)

    def check(self, condition, message):
        if not condition:
            self.fail(message)
        return bool(condition)

    def settle(self, predicate, what, seconds=6):
        try:
            return wait_for(predicate, seconds, what)
        except RuntimeError as error:
            self.fail(str(error))
            return None


# ------------------------------------------------------------------------------ phases

def phase_contract(rig):
    """DISPLAY unset, session chosen by backend::session, banner surface contract, no tiled window."""
    rig.start_sink()
    rig.notify(summary="contract", actions=[("reply", "Reply")])
    rig.settle(lambda: rig.banners()["active"], "a banner is shown")
    time.sleep(0.5)
    rig.evidence["banner_contract"] = check_banner_contract(rig.trace(), rig.failures)
    tree = json.dumps(rig.sway_cmd("-t", "get_tree", query=True))
    rig.check("decebal-notifications" not in tree and '"app_id": "decnotify"' not in tree, "contract: a layer-shell banner appears as a tiled/floating window")
    rig.check(rig.daemon.poll() is None, "contract: decnotify exited with DISPLAY unset")


def phase_bus(rig):
    """A second daemon exits quietly and takes nothing; the owner keeps working."""
    second = rig.spawn("decnotify-second", [str(rig.bin / "decnotify")], rig.client_env(WAYLAND_DEBUG="client"))
    code = None
    try:
        code = second.wait(timeout=10)
    except subprocess.TimeoutExpired:
        rig.fail("bus: the second decnotify did not exit")
        rig.stop(second)
    rig.check(code == 0, f"bus: the second decnotify exited {code}, expected 0")
    rig.check("already owned" in (rig.root / "decnotify-second.log").read_text().replace("\n", " ") or code == 0, "bus: no message about the name being taken")
    rig.check("get_layer_surface" not in (rig.root / "decnotify-second.log").read_text(), "bus: the second daemon created a surface")
    rig.check(rig.owner_alive() and rig.daemon.poll() is None, "bus: the first daemon lost the name")


def phase_queue(rig):
    """Five visible, the rest queued with lifetimes unstarted; replacement; expiry reason 1; CloseNotification reason 3; promotion."""
    rig.start_monitor()
    ids = [rig.notify(summary=f"n{i}", timeout=60000) for i in range(7)]
    state = rig.settle(lambda: (lambda b: b if len(b["active"]) == 5 and len(b["queued"]) == 2 else None)(rig.banners()), "five visible and two queued")
    if state:
        rig.check(all(not q["started"] for q in state["queued"]), "queue: a queued entry's lifetime started before it was displayed")
        before = [b["id"] for b in state["active"]]
        again = rig.notify(summary="replaced", replaces=ids[2], timeout=60000)
        rig.check(again == ids[2], f"queue: replacement returned id {again}, expected {ids[2]}")
        after = rig.banners()
        rig.check([b["id"] for b in after["active"]] == before, "queue: a replacement reordered or dropped banners")
        deadlines = {b["id"]: b["deadline"] for b in state["active"] if b["id"] != ids[2]}
        rig.check(all(deadlines.get(b["id"], b["deadline"]) == b["deadline"] for b in after["active"]), "queue: a replacement changed another banner's deadline")
        rig.call("CloseNotification", "u", ids[0])
        promoted = rig.settle(lambda: (lambda b: b if len(b["queued"]) == 1 else None)(rig.banners()), "closing one promotes one queued entry")
        rig.check(promoted is not None and len(promoted["active"]) == 5, "queue: the queue was not promoted into the freed slot")
        rig.settle(lambda: closed_reasons(rig.signals()).get(ids[0]) == 3, "NotificationClosed(id, 3) after CloseNotification")
    short = rig.notify(summary="short", timeout=1200)
    rig.settle(lambda: closed_reasons(rig.signals()).get(short) == 1, "expiry reports reason 1", 8)
    resident = rig.notify(summary="resident", actions=[("keep", "Keep")], hints=[("resident", "b", "true")], timeout=60000)
    banner = rig.settle(lambda: next((b for b in rig.banners()["active"] if b["id"] == resident), None), "resident banner shown")
    rig.evidence["queue_ids"] = ids
    rig.evidence["resident_id"] = resident
    if banner:
        rig.evidence["resident_banner"] = banner


def phase_actions(rig):
    """Pointer actions without moving the keyboard: reply, default, close button, resident stays, right/middle."""
    for _ in range(2):  # start from an empty screen: dismiss everything with the middle button
        active = rig.banners()["active"]
        if not active:
            break
        x, y = close_center(active[0], 1.5)
        rig.sway_cmd("seat", "seat0", "cursor", "set", str(round(x - 40)), str(round(y + 30)))
        rig.sway_cmd("seat", "seat0", "cursor", "press", "button2")
        rig.sway_cmd("seat", "seat0", "cursor", "release", "button2")
        time.sleep(0.4)
    first = rig.notify(summary="act", actions=[("reply", "Reply"), ("default", "Open")], timeout=60000)
    banner = rig.settle(lambda: next((b for b in rig.banners()["active"] if b["id"] == first), None), "action banner shown")
    if not banner:
        return
    node = rig.sink_node()
    before = rig.sink_bytes()
    rig.type("a")
    rig.settle(lambda: rig.sink_bytes() != before, "typing reaches the native sink while a banner is visible")
    rig.check(node and node.get("focused"), "actions: the sink lost focus when a banner appeared")
    x, y = button_center(banner, "reply", 1.5)
    rig.click(x, y)
    rig.settle(lambda: (first, "reply") in actions_invoked(rig.signals()), "ActionInvoked(id, reply)")
    rig.settle(lambda: closed_reasons(rig.signals()).get(first) == 2, "the banner is dismissed (reason 2) after a non-resident action")
    mark = rig.sink_bytes()
    rig.type("b")
    rig.settle(lambda: rig.sink_bytes() != mark, "keyboard still goes to the sink after the click")
    rig.check(rig.sink_node().get("focused"), "actions: the sink is not focused after a banner click")
    rig.evidence["actions"] = check_no_banner_keyboard(rig.trace(), rig.failures)
    # resident: the action is reported, the banner stays
    resident = rig.notify(summary="resident", actions=[("keep", "Keep")], hints=[("resident", "b", "true")], timeout=60000)
    banner = rig.settle(lambda: next((b for b in rig.banners()["active"] if b["id"] == resident), None), "resident banner")
    if banner:
        rig.click(*button_center(banner, "keep", 1.5))
        rig.settle(lambda: (resident, "keep") in actions_invoked(rig.signals()), "ActionInvoked for a resident banner")
        time.sleep(0.5)
        rig.check(any(b["id"] == resident for b in rig.banners()["active"]), "actions: a resident banner was dismissed by its action")
        rig.check(closed_reasons(rig.signals()).get(resident) is None, "actions: NotificationClosed was sent for a resident banner")


def phase_history(rig):
    """Opening dismisses banners and queue; keyboard Tab/Shift+Tab/Enter/Delete/Escape; outside clicks on both outputs; no action replay."""
    rig.sway_cmd("focus", "output", "HEADLESS-1")
    ids = [rig.notify(summary=f"h{i}", actions=[("reply", "Reply")], timeout=60000) for i in range(7)]
    rig.settle(lambda: len(rig.banners()["active"]) == 5, "banners before history")
    node = rig.sink_node()
    invoked_before = len(actions_invoked(rig.signals()))
    # HEADLESS-1 is 1280x720 logical at (0,0) and holds the panel top-right; HEADLESS-2 starts at x=1280
    for case, closer in (("escape", "key"), ("outside-same-output", (100, 600)), ("outside-other-output", (2000, 500))):
        destroyed = trace_count(rig.trace(), "zwlr_layer_surface_v1", "destroy")
        rig.cli("history", "show")
        panel = rig.settle(lambda: rig.history(), "history panel open")
        rig.settle(lambda: not rig.banners()["active"] and not rig.banners()["queued"], f"{case}: opening history dismissed the banners and the queue")
        reasons = closed_reasons(rig.signals())
        rig.check(all(reasons.get(i) in (2, 1, 3) for i in ids), f"{case}: dismissed banners have no close reason")
        rig.settle(lambda: re.search(r"<- wl_keyboard@\d+\.enter", rig.trace()), f"{case}: the panel received the keyboard")
        rig.evidence[f"history_{case}"] = check_history_contract(rig.trace(), 2, rig.failures)
        if closer == "key":
            for keys in (["-k", "Tab"], ["-M", "shift", "-k", "Tab", "-m", "shift"], ["-k", "Tab"], ["-k", "Return"]):
                rig.type(*keys)
            rig.type("-k", "Delete")
            rig.type("-k", "Escape")
        else:
            rig.click(*closer)
        rig.settle(lambda: trace_count(rig.trace(), "zwlr_layer_surface_v1", "destroy") >= destroyed + 3, f"{case}: the panel and both catchers were destroyed")
        mark = rig.sink_bytes()
        rig.type(f"{case[0]}")
        rig.settle(lambda: rig.sink_bytes() != mark, f"{case}: the keyboard returned to the sink without refocusing it")
        rig.check(rig.sink_node().get("focused"), f"{case}: the sink is not focused after the panel closed")
        if case != "escape":
            ids = [rig.notify(summary=f"h{i}b", timeout=60000) for i in range(2)]
            rig.settle(lambda: rig.banners()["active"], f"{case}: new banners after the panel closed")
    rig.check(len(actions_invoked(rig.signals())) == invoked_before, "history: closing or removing history rows replayed an action")
    rig.evidence["sink_node_same"] = bool(node and rig.sink_node() and node["id"] == rig.sink_node()["id"])


def phase_persistence(rig):
    """An unwritable state directory shows the error card; Retry (clicked) repairs it."""
    state = rig.root / "state" / "decebal"
    state.chmod(0o500)
    try:
        rig.notify(summary="unsaved", timeout=60000)
        status = rig.cli("status", "--json")
        rig.check(status.returncode == 1 and '"pending":true' in status.stdout.replace(" ", ""), f"persistence: expected pending unsaved history, got {status.stdout!r}")
        rig.cli("history", "show")
        panel = rig.settle(lambda: (lambda h: h if h and h.get("error") else None)(rig.history()), "the history panel shows the storage error")
    finally:
        state.chmod(0o700)
    if panel and panel.get("retry_rect"):
        scale, origin = 1.5, (0, 0)
        x, y, w, h = panel["retry_rect"]
        px, py = panel["origin"]
        rig.click(origin[0] + (px + x + w / 2) / scale, origin[1] + (py + y + h / 2) / scale)
        rig.settle(lambda: rig.cli("status", "--json").returncode == 0, "Retry saved the history")
        rig.check(not rig.cli("status", "--json").stdout.count('"pending":true'), "persistence: changes are still pending after Retry")
    rig.type("-k", "Escape")


def phase_dnd(rig):
    """DND hides normal banners, lets critical through, and sounds follow the shared rules."""
    play = rig.root / "sounds.log"
    (rig.bin / "canberra-gtk-play").write_text(f"#!/bin/sh\necho \"$@\" >> '{play}'\n")
    (rig.bin / "canberra-gtk-play").chmod(0o500)
    (rig.root / "cfg" / "decebal" / "notifications.toml").write_text("sounds = true\ndnd = false\n")
    rig.notify(summary="sound on")
    rig.settle(lambda: play.exists(), "a sound plays when sounds are on", 8)
    count = len(play.read_text().splitlines()) if play.exists() else 0
    rig.cli("dnd", "on")
    quiet = rig.notify(summary="quiet", timeout=60000)
    time.sleep(0.6)
    rig.check(all(b["id"] != quiet for b in rig.banners()["active"]), "dnd: a normal banner appeared")
    rig.check((len(play.read_text().splitlines()) if play.exists() else 0) == count, "dnd: a sound played")
    critical = rig.notify(summary="critical", hints=[("urgency", "y", "2")], timeout=0)
    rig.settle(lambda: any(b["id"] == critical for b in rig.banners()["active"]), "a critical banner shows through dnd")
    rig.cli("dnd", "off")


def phase_themes(rig):
    """A live theme change re-lays the banners out; ids, deadlines and hover survive."""
    cfg = rig.root / "cfg" / "decebal" / "decebal.json"
    cfg.write_text(json.dumps({"theme": "macos"}))
    ids = [rig.notify(summary=f"t{i}", timeout=60000) for i in range(3)]
    state = rig.settle(lambda: (lambda b: b if len(b["active"]) >= 3 else None)(rig.banners()), "banners before the theme change")
    if not state:
        return
    deadlines = {b["id"]: b["deadline"] for b in state["active"]}
    generation = state["generation"]
    cfg.write_text(json.dumps({"theme": "aster"}))
    changed = rig.settle(lambda: (lambda b: b if b["generation"] != generation else None)(rig.banners()), "the theme generation advanced", 10)
    if changed:
        rig.check({b["id"]: b["deadline"] for b in changed["active"] if b["id"] in deadlines} == {k: v for k, v in deadlines.items() if k in {b["id"] for b in changed["active"]}}, "themes: a theme change reset deadlines")
        rig.check({b["id"] for b in changed["active"]} >= set(ids), "themes: a theme change dropped a banner")


def phase_scales(rig):
    """Integer and fractional scales, an output hotplug and hit alignment at every scale."""
    results = {}
    for scale in (2, 1, 1.5):
        rig.sway_cmd("output", "HEADLESS-1", "scale", str(scale))
        time.sleep(0.8)
        pid = rig.notify(summary=f"scale {scale}", actions=[("reply", "Reply")], timeout=60000)
        banner = rig.settle(lambda: next((b for b in rig.banners()["active"] if b["id"] == pid), None), f"banner at scale {scale}")
        state = rig.banners()
        results[str(scale)] = {"debug_scale": state.get("scale"), "protocol": check_scale_protocol(rig.trace(), scale, rig.failures)}
        rig.check(abs(state.get("scale", 0) - scale) < 0.01, f"scales: debug scale {state.get('scale')} != {scale}")
        if banner:
            rect = banner["rect"]
            # the output is 1920 device pixels wide at every scale (1920 / scale logical)
            rig.check(rect[0] + rect[2] <= 1920, f"scales: the banner overflows the output at {scale}")
            rig.click(*button_center(banner, "reply", scale))
            rig.settle(lambda: (pid, "reply") in actions_invoked(rig.signals()), f"hit alignment: the button fires at scale {scale}")
    rig.sway_cmd("create_output")
    time.sleep(0.8)
    rig.check(rig.daemon.poll() is None, "scales: decnotify died when an output was added")
    rig.evidence["scales"] = results
    rig.sway_cmd("output", "HEADLESS-1", "scale", "1.5")


def phase_pinned(rig):
    """DECNOTIFY_OUTPUT pins every banner to that output, whatever has the focus."""
    rig.stop(rig.daemon)
    rig.sway_cmd("focus", "output", "HEADLESS-1")
    rig.start_daemon("decnotify-pinned", DECNOTIFY_OUTPUT="HEADLESS-2")
    rig.notify(summary="pinned")
    rig.settle(lambda: rig.banners("decnotify-pinned")["active"], "pinned banner")
    trace = rig.trace("decnotify-pinned")
    reqs = requests(trace)
    named = re.findall(r"<- wl_output@(\d+)\.name, \(Some\(\"([^\"]+)\"\)\)", trace)
    by_id = {int(i): n for i, n in named}
    used = {by_id.get(int(s["output"].split("@")[1])) for s in layer_surfaces(reqs) if s["namespace"] == NAMESPACE and s["output"].startswith("wl_output@")}
    rig.check(used == {"HEADLESS-2"}, f"pinned: banners requested on {used}, expected only HEADLESS-2")
    rig.evidence["pinned_outputs"] = sorted(x for x in used if x)


def phase_burst(rig):
    """A burst of input on a banner and on the history panel: bounded buffers and callbacks, coalesced commits."""
    rig.sway_cmd("focus", "output", "HEADLESS-1")
    pid = rig.notify(summary="burst", actions=[("reply", "Reply"), ("later", "Later")], timeout=60000)
    banner = rig.settle(lambda: next((b for b in rig.banners()["active"] if b["id"] == pid), None), "burst banner shown")
    if not banner:
        return
    rig.settle(lambda: (rig.present() or {}).get("totals", {}).get("commits", 0) >= 1, "the banner's first frame was committed")
    points = [button_center(banner, "reply", 1.5), button_center(banner, "later", 1.5), close_center(banner, 1.5)]
    before = rig.present()["totals"]
    motions = 600
    # one IPC call: 600 pointer motions with nothing between them, alternating over the buttons (a hover repaint each)
    rig.sway_cmd("; ".join(f"seat seat0 cursor set {round(points[n % 3][0])} {round(points[n % 3][1])}" for n in range(motions)))
    time.sleep(2.0)
    after = rig.present()
    commits = after["totals"]["commits"] - before["commits"]
    rig.check(commits < motions / 4, f"burst: {commits} commits for {motions} motions (frames were not coalesced)")
    rig.check(after["totals"]["coalesced"] - before["coalesced"] > 0 or commits < motions / 4, "burst: no frame was ever replaced before it was shown")
    rig.check(all(not s["frame_pending"] for s in after["surfaces"]), "burst: a frame is still waiting after the burst ended")
    rig.evidence["burst_banner"] = {"motions": motions, "commits": commits, "bounds": check_presentation_bounds(rig.trace(), after, rig.failures, "burst-banner")}
    # the history panel: 150 Tab presses in one wtype call, each moving the focus ring
    rig.cli("history", "show")
    rig.settle(lambda: re.search(r"<- wl_keyboard@\d+\.enter", rig.trace()), "the panel has the keyboard")
    before = rig.present()["totals"]
    rig.type(*(["-k", "Tab"] * 150))
    time.sleep(2.0)
    after = rig.present()
    commits = after["totals"]["commits"] - before["commits"]
    rig.check(commits < 150 / 2, f"burst: {commits} panel commits for 150 key presses")
    rig.evidence["burst_panel"] = {"keys": 150, "commits": commits, "bounds": check_presentation_bounds(rig.trace(), after, rig.failures, "burst-panel")}
    rig.type("-k", "Escape")
    time.sleep(0.5)


def phase_churn(rig):
    """Scale and size churn with banners and the panel up never passes the bounds; everything is given back when idle."""
    rig.sway_cmd("focus", "output", "HEADLESS-1")
    ids = [rig.notify(summary=f"churn {i}", actions=[("reply", "Reply")], timeout=60000) for i in range(3)]
    rig.settle(lambda: len(rig.banners()["active"]) >= 3, "banners before the churn")
    rig.evidence["churn_kernel_before"] = rig.shm_sample()
    rss_before = rig.rss_kb()
    for cycle in range(3):
        # one IPC call per cycle: the scale changes follow each other with no pause
        rig.sway_cmd("; ".join(f"output HEADLESS-1 scale {s}" for s in (2, 1, 1.5, 1.25, 2, 1.5) * 3))
        time.sleep(1.0)
        if cycle == 1:
            rig.cli("history", "show")  # the panel is up through the next scale changes
        rig.notify(summary=f"during {cycle}", actions=[("reply", "Reply")], replaces=ids[0], timeout=60000)
    time.sleep(1.5)
    present = rig.present()
    rig.evidence["churn"] = check_presentation_bounds(rig.trace(), present, rig.failures, "churn")
    rig.evidence["churn_kernel_peak_sample"] = rig.shm_sample()
    rig.check(rig.evidence["churn_kernel_peak_sample"]["mapped_shm_bytes"] > 0, "churn: no notification shm mappings identified while banners are active")
    rig.check(rig.evidence["churn_kernel_peak_sample"]["mapped_shm_bytes"] <= 96 << 20,
              "churn: kernel shm mappings exceed 96 MiB (sample, not continuous peak)")
    rig.check(rig.daemon.poll() is None, "churn: the daemon died")
    # idle again: close the panel and every banner; buffers of gone surfaces are released and the pool shrinks
    rig.type("-k", "Escape")  # closes the panel when it is open (the sink only records the key otherwise)
    time.sleep(0.5)
    for i in ids:
        rig.call("CloseNotification", "u", i)
    idle = rig.settle(lambda: (lambda p: p if p and not p["surfaces"] and p["graveyard"] == 0 and p["pool_bytes"] <= POOL_IDLE_BYTES else None)(rig.present()),
                      "no surfaces, no held buffers and no shm pools after the churn", 10)
    rig.evidence["churn_idle"] = idle
    rig.evidence["churn_kernel_idle"] = rig.shm_sample()
    rig.check(rig.evidence["churn_kernel_idle"]["mapped_shm_bytes"] == 0, "churn: notification shm mappings remain at idle")
    growth = rig.rss_kb() - rss_before
    rig.check(growth < 16 * 1024, f"churn: resident memory grew {growth} kB across the churn")


PHASES = {name: globals()[f"phase_{name}"] for name in ALL_PHASES}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", type=Path, required=True, help="immutable decnotify binary")
    ap.add_argument("--out", type=Path, required=True, help="NEW evidence directory (refused when it exists; never deleted)")
    ap.add_argument("--phases", default=",".join(ALL_PHASES))
    args = ap.parse_args()
    if len(str(args.out.resolve())) > 60:
        raise SystemExit("evidence directory path is longer than 60 bytes (private socket paths would exceed 108)")
    phases = args.phases.split(",")
    unknown = [p for p in phases if p not in PHASES]
    if unknown:
        raise SystemExit(f"unknown phases: {unknown}")
    missing = [t for t in ("sway", "swaymsg", "dbus-daemon", "dbus-monitor", "busctl", "foot", "wtype") if not shutil.which(t)]
    if missing:
        raise SystemExit(f"missing fixture tools: {missing}")
    root = args.out.resolve()
    root.mkdir(mode=0o700, parents=True, exist_ok=False)
    (root / "run").mkdir(mode=0o700)
    (root / "bin").mkdir()
    shutil.copyfile(args.bin.resolve(), root / "bin" / "decnotify")
    (root / "bin" / "decnotify").chmod(0o500)
    token = os.urandom(16).hex()
    rig = Rig(root, args.bin, token)
    completed = False
    try:
        rig.start_bus()
        rig.start_sway()
        rig.start_daemon()
        for name in phases:
            try:
                PHASES[name](rig)
            except Exception as error:  # a phase failure is evidence, not a reason to skip cleanup
                rig.fail(f"{name}: {error!r}")
            (root / "result.partial.json").write_text(json.dumps({"failures": rig.failures, "evidence": rig.evidence}, indent=2, default=str))
        completed = True
    finally:
        rig.close()
        rescued = rescue_tagged(token)
        if FORCED or rescued:
            rig.fail(f"irregular cleanup: forced={FORCED}, rescued={rescued}")
        (root / "result.json").write_text(json.dumps({
            "completed": completed, "phases": phases, "failures": rig.failures, "forced": FORCED, "rescued": rescued, "evidence": rig.evidence,
            "inputs_sha256": {"decnotify": hashlib.sha256((root / "bin" / "decnotify").read_bytes()).hexdigest(), "harness": hashlib.sha256(Path(__file__).read_bytes()).hexdigest()},
        }, indent=2, default=str))
    if rig.failures:
        raise SystemExit("FAIL\n" + "\n".join(rig.failures) + f"\nlogs: {root}")
    print("ok")


def interrupted(signum, frame):
    # BaseException bypasses individual phase error handling, reaching main's
    # finally cleanup immediately when the resource guard requests shutdown.
    raise KeyboardInterrupt(f"fixture interrupted by signal {signum}")


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, interrupted)
    main()
