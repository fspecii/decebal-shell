#!/usr/bin/env python3
"""Protocol-level smoke check of the Wayland compact popup path (docs/wayland-compact-popups.md).

Starts a private headless Sway with two outputs (HEADLESS-1 at scale 1.5, HEADLESS-2 at scale 1), runs `dechud` and
`decdash --output HEADLESS-2` against it with WAYLAND_DEBUG=client and checks the requests they send:

* the card is a subsurface of an overlay layer surface; card buffers are card-sized, never output-sized;
* the HUD on the 1.5 output presents a 360 x 222 buffer through a 240 x 148 viewport (sharp, not double-scaled);
* every card commit is paced by wl_surface.frame; at most 3 card buffers are created per size;
* decdash's host asks for exclusive keyboard focus on the named output and puts one catcher
  (namespace decebal-popup-outside) on the other output.

With --input, a private foot key recorder and wtype exercise Escape, same-output and other-output outside clicks,
and keyboard hand-back. Sway IPC pointer commands are used only after kernel peer ownership checks.
Usage: wayland_compact_popup.py --bin-dir <immutable-bin-dir> [--out <new-dir>] [--keep]
"""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import signal
import socket
import stat
import struct
import subprocess
import tempfile
import sys
import hashlib
import time

SWAY_CONFIG = """
xwayland disable
output HEADLESS-1 resolution 1920x1080 position 0 0 scale 1.5
output HEADLESS-2 resolution 1920x1080 position 1280 0 scale 1
for_window [app_id="decebal-popup-key-sink"] move to output HEADLESS-1
"""

REQ = re.compile(r"-> (?P<obj>[a-z0-9_]+)@(?P<id>\d+)\.(?P<req>[a-z_]+)\((?P<args>.*)\)\s*$")
FORCED = []


def wait_for(predicate, seconds, what):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.05)
    raise RuntimeError(f"timeout: {what}")


def requests(log):
    out = []
    for line in log.splitlines():
        m = REQ.search(line)
        if m:
            out.append((m["obj"], int(m["id"]), m["req"], [a.strip() for a in m["args"].split(",")] if m["args"] else []))
    return out


def ints(args):
    return [int(a) for a in args if re.fullmatch(r"-?\d+", a)]


def socket_owned(proc, path):
    """Require kernel peer credentials to identify the private compositor child."""
    if proc.poll() is not None:
        return False
    try:
        process = Path(f"/proc/{proc.pid}")
        if process.stat().st_uid != os.getuid() or path.stat().st_uid != os.getuid() or not stat.S_ISSOCK(path.stat().st_mode):
            return False
        # Sway can be nondumpable, making /proc/PID/fd unreadable even for the same UID.
        # SO_PEERCRED gives the listening server's PID and UID directly from the kernel.
        with socket.socket(socket.AF_UNIX) as connection:
            connection.settimeout(0.5)
            connection.connect(str(path))
            pid, uid, _ = struct.unpack("3i", connection.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, struct.calcsize("3i")))
        return pid == proc.pid and uid == os.getuid() and proc.poll() is None
    except (OSError, ValueError):
        return False


def run_client(argv, env, seconds):
    proc = subprocess.Popen(argv, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True, start_new_session=True)
    fd = os.pidfd_open(proc.pid)
    try:
        try:
            _, err = proc.communicate(timeout=seconds)
        except subprocess.TimeoutExpired:
            signal.pidfd_send_signal(fd, signal.SIGTERM)
            _, err = proc.communicate(timeout=5)
        return err
    finally:
        if proc.poll() is None:
            FORCED.append(proc.pid)
            signal.pidfd_send_signal(fd, signal.SIGKILL)
            proc.wait(timeout=5)
        os.close(fd)


def tagged_processes(token):
    marker = f"DECEBAL_NATIVE_TOKEN={token}".encode()
    found = []
    for proc in Path("/proc").iterdir():
        if not proc.name.isdigit() or int(proc.name) == os.getpid():
            continue
        try:
            if proc.stat().st_uid == os.getuid() and marker in (proc / "environ").read_bytes().split(b"\0"):
                found.append(int(proc.name))
        except OSError:
            pass
    return found


def rescue_tagged(token):
    found = tagged_processes(token)
    for pid in found:
        try:
            fd = os.pidfd_open(pid)
            try:
                if pid in tagged_processes(token):
                    signal.pidfd_send_signal(fd, signal.SIGTERM)
            finally:
                os.close(fd)
        except ProcessLookupError:
            pass
    return found


def check_hud(reqs, failures):
    sizes = [tuple(ints(a)[1:3]) for o, _, r, a in reqs if o == "wl_shm_pool" and r == "create_buffer"]
    if any(w >= 1280 or h >= 720 for w, h in sizes):
        failures.append(f"hud: output-sized buffer created: {sizes}")
    if (360, 222) not in sizes:
        failures.append(f"hud: no 360x222 card buffer at scale 1.5: {sizes}")
    dest = [tuple(ints(a)) for o, _, r, a in reqs if o == "wp_viewport" and r == "set_destination"]
    if (240, 148) not in dest:
        failures.append(f"hud: card viewport is not 240x148 logical: {dest}")
    if not any(o == "wl_subcompositor" and r == "get_subsurface" for o, _, r, _ in reqs):
        failures.append("hud: no card subsurface")
    commits = sum(1 for o, _, r, _ in reqs if o == "wl_surface" and r == "commit")
    frames = sum(1 for o, _, r, _ in reqs if o == "wl_surface" and r == "frame")
    if frames == 0 or frames > commits:
        failures.append(f"hud: frame callbacks {frames} for {commits} commits")
    card_buffers = [s for s in sizes if s == (360, 222)]
    if len(card_buffers) > 3:
        failures.append(f"hud: {len(card_buffers)} card buffers of one size (max 3)")


def check_dash(reqs, failures):
    layers = [a for o, _, r, a in reqs if o == "zwlr_layer_shell_v1" and r == "get_layer_surface"]
    if not any("decdash" in " ".join(a) and "nil" not in a[2] for a in layers):
        failures.append(f"dash: host not requested on a named output: {layers}")
    catchers = [a for a in layers if "decebal-popup-outside" in " ".join(a)]
    if len(catchers) != 1:
        failures.append(f"dash: expected one catcher on the other output, got {len(catchers)}")
    exclusive = [a for o, _, r, a in reqs if o == "zwlr_layer_surface_v1" and r == "set_keyboard_interactivity"]
    if not any(ints(a) == [1] for a in exclusive):
        failures.append(f"dash: host never asked for exclusive keyboard focus: {exclusive}")


# rust-wayland's debug event spelling differs from requests by the comma before `(`.
WIRE = re.compile(r"\[(?P<time>[0-9.]+)\].*?(?P<direction>->|<-) (?P<obj>[a-z0-9_]+)@(?P<id>\d+)\.(?P<op>[a-z_]+)(?:, )?\((?P<args>.*)\)\s*$")
OBJECT = re.compile(r"([a-z0-9_]+)@(\d+)")


def wire_events(log):
    events = []
    for line in log.splitlines():
        match = WIRE.search(line)
        if match:
            events.append((float(match["time"]), match["direction"], match["obj"], int(match["id"]), match["op"], match["args"]))
    return events


def output_names(log):
    names = {}
    for _, direction, obj, ident, op, args in wire_events(log):
        if direction == "<-" and obj == "wl_output" and op == "name":
            match = re.fullmatch(r'(?:Some\()?"([^"]+)"\)?', args)
            if match:
                names[ident] = match[1]
    return names


def check_output_identity(log, failures):
    names = output_names(log)
    host_outputs, catcher_outputs = [], []
    for _, direction, obj, _, op, args in wire_events(log):
        if direction != "->" or obj != "zwlr_layer_shell_v1" or op != "get_layer_surface":
            continue
        outputs = [int(ident) for kind, ident in OBJECT.findall(args) if kind == "wl_output"]
        if '"decdash"' in args:
            host_outputs.extend(names.get(ident) for ident in outputs)
        if '"decebal-popup-outside"' in args:
            catcher_outputs.extend(names.get(ident) for ident in outputs)
    if host_outputs != ["HEADLESS-2"]:
        failures.append(f"dash: requested output identity {host_outputs}, wanted HEADLESS-2")
    if catcher_outputs != ["HEADLESS-1"]:
        failures.append(f"dash: catcher output identity {catcher_outputs}, wanted HEADLESS-1")
    return {"host_outputs": host_outputs, "catcher_outputs": catcher_outputs}


def check_card_frames(log, failures, label):
    """Tie every card-buffer commit to that card's frame request; track real releases and live allocations."""
    cards, buffers, busy, attached, requested, outstanding = set(), {}, set(), {}, {}, {}
    max_live, commits, callbacks, partial_damage = 0, 0, 0, 0
    for stamp, direction, obj, ident, op, args in wire_events(log):
        refs = OBJECT.findall(args)
        if direction == "->" and obj == "wl_subcompositor" and op == "get_subsurface":
            surfaces = [int(i) for kind, i in refs if kind == "wl_surface"]
            if surfaces:
                cards.add(surfaces[0])
        if direction == "->" and obj == "wl_shm_pool" and op == "create_buffer":
            ids = [int(i) for kind, i in refs if kind == "wl_buffer"]
            numbers = ints([part.strip() for part in args.split(",")])
            if ids and len(numbers) >= 3:
                buffers[ids[0]] = tuple(numbers[1:3])
                counts = [sum(size == other for other in buffers.values()) for size in buffers.values() if size != (1, 1)]
                max_live = max(max_live, max(counts, default=0))
        if obj == "wl_buffer" and ((direction == "<-" and op == "release") or (direction == "->" and op == "destroy")):
            busy.discard(ident)
            if op == "destroy":
                buffers.pop(ident, None)
        if direction == "<-" and obj == "wl_callback" and op == "done":
            for surface, pending in list(outstanding.items()):
                if pending[0] == ident:
                    del outstanding[surface]
                    callbacks += 1
        if direction != "->" or obj != "wl_surface" or ident not in cards:
            continue
        if op == "frame":
            ids = [int(i) for kind, i in refs if kind == "wl_callback"]
            if ids:
                old = outstanding.get(ident)
                # The backend explicitly allows its documented 1-second lost-callback fallback.
                if old and stamp - old[1] < 990:
                    failures.append(f"{label}: card {ident} requested another frame before callback {old[0]}")
                requested[ident] = ids[0]
                outstanding[ident] = (ids[0], stamp)
        elif op == "attach":
            ids = [int(i) for kind, i in refs if kind == "wl_buffer"]
            if ids:
                attached[ident] = ids[0]
        elif op == "damage_buffer":
            numbers = ints([part.strip() for part in args.split(",")])
            if len(numbers) == 4 and (numbers[0] or numbers[1]):
                partial_damage += 1
        elif op == "commit" and ident in attached:
            buffer = attached.pop(ident)
            if ident not in requested:
                failures.append(f"{label}: card-buffer commit has no frame request on the same surface")
            requested.pop(ident, None)
            if buffer in busy:
                failures.append(f"{label}: buffer {buffer} reused before compositor release")
            busy.add(buffer)
            commits += 1
    if not cards or not commits or not callbacks:
        failures.append(f"{label}: no proven card commits/callbacks ({len(cards)}, {commits}, {callbacks})")
    if max_live > 3:
        failures.append(f"{label}: {max_live} live same-size card buffers, limit 3")
    return {"card_surfaces": sorted(cards), "card_commits": commits, "completed_callbacks": callbacks,
            "max_live_same_size_buffers": max_live, "partial_damage_requests": partial_damage}


KEY_SINK = r"""
import os, sys, tty
from pathlib import Path
tty.setraw(sys.stdin.fileno())
Path(sys.argv[2]).write_text("ready")
with open(sys.argv[1], "ab", buffering=0) as log:
    while True:
        value = os.read(sys.stdin.fileno(), 256)
        if not value:
            break
        log.write(value)
"""


def wl_uints(*values):
    return struct.pack("=" + "I" * len(values), *values)


def wl_string(value):
    data = value.encode() + b"\0"
    return wl_uints(len(data)) + data + b"\0" * (-len(data) % 4)


def wl_read_string(data, offset=0):
    if offset + 4 > len(data):
        raise ValueError("truncated Wayland string length")
    length = struct.unpack_from("=I", data, offset)[0]
    start, end = offset + 4, offset + 4 + length
    padded_end = start + (length + 3) // 4 * 4
    if length == 0 or padded_end > len(data) or data[end - 1] != 0:
        raise ValueError("invalid Wayland string")
    return data[start:end - 1].decode(), padded_end


def wl_message(ident, opcode, payload=b""):
    size = 8 + len(payload)
    if size > 65535 or size % 4 or not 0 <= opcode <= 65535:
        raise ValueError("invalid Wayland message size/opcode")
    return wl_uints(ident, size << 16 | opcode) + payload


class VirtualPointer:
    """Minimal registry/seat/virtual-pointer v1 client; no physical devices and no installed input daemon.

    Request order follows wlr-virtual-pointer-unstable-v1.xml (vendored in wayland-protocols-wlr):
    manager.create_virtual_pointer=0, pointer.motion_absolute=1, button=2, frame=4, destroy=8.
    No interfaces used here send file descriptors. The pinned socket is verified before connecting and with
    SO_PEERCRED on the actual connection; closing it removes this client's virtual device.
    """
    def __init__(self, proc, path):
        self.connection = None
        self.proc, self.path = proc, path
        self.next_id = 2
        self.globals = {}
        self.registry = self.manager = self.seat = self.pointer = None
        self.capabilities = 0
        if not socket_owned(proc, path):
            raise RuntimeError("virtual pointer refused an unowned compositor socket")
        connection = socket.socket(socket.AF_UNIX)
        connection.settimeout(5)
        try:
            connection.connect(str(path))
            pid, uid, _ = struct.unpack("3i", connection.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, struct.calcsize("3i")))
            if pid != proc.pid or uid != os.getuid() or proc.poll() is not None:
                raise RuntimeError("virtual pointer compositor peer changed")
            self.connection = connection
            self.registry = self.allocate()
            self.send(1, 1, wl_uints(self.registry))  # wl_display.get_registry
            self.sync()
            for interface in ("wl_seat", "zwlr_virtual_pointer_manager_v1"):
                if interface not in self.globals:
                    raise RuntimeError(f"private compositor lacks {interface}")
            self.manager = self.bind("zwlr_virtual_pointer_manager_v1", 1)
            self.seat = self.bind("wl_seat", 1)
            self.sync()
            self.pointer = self.allocate()
            self.send(self.manager, 0, wl_uints(self.seat, self.pointer))
            deadline = time.monotonic() + 5
            while not self.capabilities & 1:
                self.sync()
                if time.monotonic() >= deadline:
                    raise RuntimeError(f"virtual pointer did not give seat pointer capability: {self.capabilities}")
                if not self.capabilities & 1:
                    time.sleep(0.05)
        except BaseException:
            connection.close()
            self.connection = None
            raise

    def allocate(self):
        ident = self.next_id
        self.next_id += 1
        return ident

    def send(self, ident, opcode, payload=b""):
        self.connection.sendall(wl_message(ident, opcode, payload))

    def bind(self, interface, version):
        name, supported = self.globals[interface]
        ident = self.allocate()
        self.send(self.registry, 0, wl_uints(name) + wl_string(interface) + wl_uints(min(version, supported), ident))
        return ident

    def exact(self, size):
        result = bytearray()
        while len(result) < size:
            part = self.connection.recv(size - len(result))
            if not part:
                raise RuntimeError("private compositor closed virtual-pointer connection")
            result.extend(part)
        return bytes(result)

    def receive(self):
        ident, header = struct.unpack("=II", self.exact(8))
        size, opcode = header >> 16, header & 65535
        if size < 8 or size % 4:
            raise RuntimeError("invalid Wayland event framing")
        payload = self.exact(size - 8)
        if ident == 1 and opcode == 0:
            object_id, code = struct.unpack_from("=II", payload)
            message, _ = wl_read_string(payload, 8)
            raise RuntimeError(f"virtual-pointer protocol error {object_id}/{code}: {message}")
        if ident == self.registry and opcode == 0:
            name = struct.unpack_from("=I", payload)[0]
            interface, end = wl_read_string(payload, 4)
            version = struct.unpack_from("=I", payload, end)[0]
            self.globals[interface] = (name, version)
        elif ident == self.seat and opcode == 0:
            self.capabilities = struct.unpack_from("=I", payload)[0]
        return ident, opcode

    def sync(self):
        callback = self.allocate()
        self.send(1, 0, wl_uints(callback))  # wl_display.sync
        deadline = time.monotonic() + 5
        while self.receive() != (callback, 0):
            if time.monotonic() >= deadline:
                raise RuntimeError("private compositor virtual-pointer roundtrip timed out")

    def close(self):
        if self.connection is not None:
            try:
                if self.pointer is not None:
                    self.send(self.pointer, 8)
                if self.manager is not None:
                    self.send(self.manager, 1)
            finally:
                self.connection.close()
                self.connection = None


class InputDriver:
    """All controls are pinned to the owned Wayland and IPC sockets; never infer an active desktop."""
    def __init__(self, sway, wayland, ipc, env, root):
        self.sway, self.wayland, self.ipc, self.env, self.root = sway, wayland, ipc, env, root
        self.tools = {name: shutil.which(name) for name in ("swaymsg", "wtype", "foot")}
        missing = [name for name, path in self.tools.items() if not path]
        if missing:
            raise RuntimeError(f"input gate missing tools: {missing}")
        self.children = []
        self.pointer = None

    def owned(self):
        assert socket_owned(self.sway, self.wayland), "private Wayland socket ownership changed"
        assert socket_owned(self.sway, self.ipc), "private Sway IPC socket ownership changed"

    def call(self, *args, query=False):
        self.owned()
        done = subprocess.run([self.tools["swaymsg"], "-r", "-s", str(self.ipc), *args],
                              env=self.env, capture_output=True, text=True, timeout=5, check=True)
        value = json.loads(done.stdout)
        if not query:
            assert isinstance(value, list) and value and all(item.get("success") for item in value), value
        return value

    def type(self, *args):
        self.owned()
        subprocess.run([self.tools["wtype"], *args], env=self.env, capture_output=True, timeout=5, check=True)

    def start(self, label, argv):
        self.owned()
        stream = (self.root / f"input-{label}.log").open("w")
        try:
            proc = subprocess.Popen(argv, env=self.env, stdout=subprocess.DEVNULL, stderr=stream, start_new_session=True)
        finally:
            stream.close()
        self.children.append((label, proc, os.pidfd_open(proc.pid)))
        return proc

    def stop(self, proc):
        for label, child, fd in self.children:
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
        try:
            for _, proc, _ in reversed(self.children):
                self.stop(proc)
        finally:
            for _, _, fd in self.children:
                os.close(fd)
            if self.pointer is not None:
                self.pointer.close()

    def sink_node(self):
        def walk(node):
            if node.get("app_id") == "decebal-popup-key-sink":
                return node
            for child in node.get("nodes", []) + node.get("floating_nodes", []):
                found = walk(child)
                if found:
                    return found
            return None
        return walk(self.call("-t", "get_tree", query=True))

    def click(self, x, y):
        self.call("seat", "seat0", "cursor", "set", str(x), str(y))
        self.call("seat", "seat0", "cursor", "press", "button1")
        self.call("seat", "seat0", "cursor", "release", "button1")


def check_inputs(sway, wayland, ipc, env, root, binaries, failures):
    driver = InputDriver(sway, wayland, ipc, env, root)
    keys, ready = root / "key-sink.bin", root / "key-sink.ready"
    records = []
    try:
        driver.owned()
        driver.pointer = VirtualPointer(sway, wayland)
        (root / "virtual-pointer.json").write_text(json.dumps({"seat_capabilities": driver.pointer.capabilities,
            "pointer_capability": bool(driver.pointer.capabilities & 1), "transport": "owned Wayland socket",
            "protocol": "zwlr_virtual_pointer_manager_v1 version 1"}, indent=2))
        # Keep one native keyboard present throughout; short wtype sends must not remove the seat's last keyboard.
        keyboard = driver.start("keyboard-holder", [driver.tools["wtype"], "-s", "60000"])
        sink = driver.start("key-sink", [driver.tools["foot"], "--config=/dev/null", "--app-id=decebal-popup-key-sink",
                                       sys.executable, "-u", "-c", KEY_SINK, str(keys), str(ready)])
        wait_for(lambda: ready.exists() and driver.sink_node(), 8, "native key recorder mapped")
        node = driver.sink_node()
        driver.call(f'[con_id={node["id"]}] focus')
        driver.type("preflight")
        wait_for(lambda: keys.exists() and keys.read_bytes() == b"preflight", 5, "real native keyboard reaches recorder")
        for index, (kind, point) in enumerate((("escape", None), ("same-output-outside", (3100, 1000)),
                                               ("other-output-outside", (100, 100)))):
            assert keyboard.poll() is None and sink.poll() is None, "fixture input client exited"
            # Prove the focused application is on the *other* output before opening on HEADLESS-2.
            driver.call("focus", "output", "HEADLESS-1")
            driver.call(f'[con_id={node["id"]}] focus')
            outputs = driver.call("-t", "get_outputs", query=True)
            assert any(output["name"] == "HEADLESS-1" and output.get("focused") for output in outputs), outputs
            before = keys.read_bytes()
            popup = driver.start(kind, [str(binaries / "decdash"), "--output", "HEADLESS-2"])
            log_path = root / f"input-{kind}.log"
            def popup_ready():
                assert popup.poll() is None, f"{kind}: popup exited before keyboard focus"
                log = log_path.read_text()
                return re.search(r"<- wl_keyboard@\d+\.enter", log) and re.search(r"-> wl_surface@\d+\.attach", log)
            wait_for(popup_ready, 5, f"{kind}: actual wl_keyboard.enter")
            if point is not None:
                wait_for(lambda: re.search(r"-> wl_seat@\d+\.get_pointer", log_path.read_text()), 5,
                         f"{kind}: popup binds the advertised pointer")
            driver.type("z")
            time.sleep(0.15)
            assert keys.read_bytes() == before, f"{kind}: key leaked through popup grab"
            if kind == "escape":
                driver.type("-k", "Escape")
            else:
                driver.click(*point)
            code = popup.wait(timeout=5)
            assert code == 0, f"{kind}: popup exited {code}, expected normal close"
            # Do not refocus the recorder: this is the focus hand-back assertion.
            marker = f"returned{index}"
            driver.type(marker)
            wait_for(lambda: keys.read_bytes() == before + marker.encode(), 5, f"{kind}: keyboard returned to recorder")
            node_after = driver.sink_node()
            assert node_after and node_after.get("focused"), f"{kind}: recorder is not focused after close"
            log = log_path.read_text()
            identity = check_output_identity(log, failures)
            frames = check_card_frames(log, failures, f"input-{kind}")
            records.append({"case": kind, "exit_code": code, "actual_keyboard_enter": True,
                            "blocked_key_not_delivered_to_sink": True, "sink_received_after_close": marker,
                            "sink_id": node["id"], "same_sink_process": sink.poll() is None,
                            "click_global_logical": point, "pointer_capability": bool(driver.pointer.capabilities & 1),
                            "pointer_bound": bool(re.search(r"-> wl_seat@\d+\.get_pointer", log)),
                            "identity": identity, "frames": frames})
            (root / "input-results.json").write_text(json.dumps(records, indent=2))
    finally:
        driver.close()
    return records


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin-dir", type=Path, required=True)
    ap.add_argument("--out", type=Path, help="new evidence directory (otherwise a private temporary directory)")
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--input", action="store_true", help="also require private foot/wtype Escape, outside-click and keyboard hand-back gates")
    args = ap.parse_args()
    required = ("sway", "swaymsg", "foot", "wtype") if args.input else ("sway", "swaymsg")
    missing = [name for name in required if not shutil.which(name)]
    if missing:
        raise SystemExit(f"missing fixture tools: {missing}")
    if args.out:
        root = args.out.resolve()
        root.mkdir(mode=0o700, parents=True, exist_ok=False)
    else:
        root = Path(tempfile.mkdtemp(prefix="decebal-compact-"))
    runtime = root / "run"
    runtime.mkdir(mode=0o700)
    (root / "sway.conf").write_text(SWAY_CONFIG)
    binaries = root / "bin"
    binaries.mkdir()
    for name in ("dechud", "decdash"):
        shutil.copyfile(args.bin_dir.resolve() / name, binaries / name)
        (binaries / name).chmod(0o500)
    # A deterministic reading, without connecting to the user's PipeWire or PulseAudio server.
    (binaries / "wpctl").write_text("#!/bin/sh\ncase \"$1\" in\nget-volume) echo 'Volume: 0.50';;\ninspect) echo 'node.description = \"Fixture audio\"';;\n*) exit 1;;\nesac\n")
    (binaries / "wpctl").chmod(0o500)
    base = {k: v for k, v in os.environ.items() if not k.startswith(("XDG_", "DECEBAL_", "RDWM_", "DBUS_")) and k not in ("WAYLAND_DISPLAY", "DISPLAY", "SWAYSOCK", "XAUTHORITY")}
    token = os.urandom(16).hex()
    base.update(XDG_RUNTIME_DIR=str(runtime), HOME=str(root), XDG_CONFIG_HOME=str(root / "config"),
                XDG_CACHE_HOME=str(root / "cache"), XDG_DATA_HOME=str(root / "data"),
                XDG_STATE_HOME=str(root / "state"), XDG_DATA_DIRS=str(root / "data"),
                DECEBAL_CONFIG=str(root / "config"), DECEBAL_THEMES=str(root / "themes"),
                DECEBAL_SYSTEM_THEMES=str(root / "themes"), DECEBAL_SYSTEM_AUTOMATIC="0",
                DBUS_SESSION_BUS_ADDRESS=f"unix:path={runtime}/absent-session-bus",
                DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={runtime}/absent-system-bus",
                PULSE_SERVER=f"unix:{runtime}/absent-pulse", RDWM_SOCK=str(runtime / "absent-rdwm"),
                DECEBAL_NATIVE_TOKEN=token, PATH=str(binaries) + os.pathsep + os.defpath)
    sway_env = dict(base, WLR_BACKENDS="headless", WLR_HEADLESS_OUTPUTS="2", WLR_LIBINPUT_NO_DEVICES="1", WLR_RENDERER="pixman")
    with (root / "sway.log").open("w") as log:
        sway = subprocess.Popen([shutil.which("sway"), "-c", str(root / "sway.conf")], env=sway_env, stdout=subprocess.DEVNULL, stderr=log, start_new_session=True)
    sway_fd = os.pidfd_open(sway.pid)
    failures = []
    metrics = {}
    input_results = []
    completed = False
    try:
        sock = wait_for(lambda: next((p.name for p in runtime.glob("wayland-*") if not p.name.endswith(".lock")), None), 10, "sway socket")
        try:
            wait_for(lambda: socket_owned(sway, runtime / sock), 5, "owned compositor socket")
        except RuntimeError:
            diagnostics = {"pid": sway.pid, "uid": os.getuid(), "socket": str(runtime / sock)}
            try:
                diagnostics["process_uid"] = Path(f"/proc/{sway.pid}").stat().st_uid
                diagnostics["fds"] = [os.readlink(p) for p in Path(f"/proc/{sway.pid}/fd").iterdir()]
            except OSError as error:
                diagnostics["process_error"] = str(error)
            diagnostics["socket_rows"] = [line for line in Path("/proc/net/unix").read_text().splitlines() if str(runtime / sock) in line]
            (root / "socket-ownership.json").write_text(json.dumps(diagnostics, indent=2))
            raise
        ipc = wait_for(lambda: next((p for p in runtime.glob("sway-ipc.*.sock") if socket_owned(sway, p)), None), 5, "owned Sway IPC socket")
        subprocess.run([shutil.which("swaymsg"), "-s", str(ipc), "focus", "output", "HEADLESS-1"], env=base, check=True, capture_output=True, timeout=5)
        env = dict(base, WAYLAND_DISPLAY=sock, XDG_SESSION_TYPE="wayland", WAYLAND_DEBUG="client", DECEBAL_BACKEND="wayland")
        hud_log = run_client([str(binaries / "dechud"), "volume"], env, 4)
        (root / "dechud.log").write_text(hud_log)
        check_hud(requests(hud_log), failures)
        metrics["hud_frames"] = check_card_frames(hud_log, failures, "hud")
        assert socket_owned(sway, runtime / sock), "private compositor socket ownership changed"
        dash_log = run_client([str(binaries / "decdash"), "--output", "HEADLESS-2"], env, 4)
        (root / "decdash.log").write_text(dash_log)
        check_dash(requests(dash_log), failures)
        metrics["dash_output_identity"] = check_output_identity(dash_log, failures)
        metrics["dash_frames"] = check_card_frames(dash_log, failures, "dash")
        if args.input:
            input_results = check_inputs(sway, runtime / sock, ipc, env, root, binaries, failures)
        completed = True
    finally:
        try:
            if sway.poll() is None:
                signal.pidfd_send_signal(sway_fd, signal.SIGTERM)
                sway.wait(timeout=5)
        finally:
            if sway.poll() is None:
                FORCED.append(sway.pid)
                signal.pidfd_send_signal(sway_fd, signal.SIGKILL)
                sway.wait(timeout=5)
            os.close(sway_fd)
        rescued = rescue_tagged(token)
        if FORCED or rescued:
            failures.append(f"irregular cleanup: forced={FORCED}, rescued={rescued}")
        inputs = {name: hashlib.sha256((binaries / name).read_bytes()).hexdigest() for name in ("dechud", "decdash")}
        inputs["harness"] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
        if not input_results and (root / "input-results.json").exists():
            input_results = json.loads((root / "input-results.json").read_text())
        (root / "result.json").write_text(json.dumps({"completed": completed, "error": repr(sys.exc_info()[1]) if sys.exc_info()[1] else None,
            "failures": failures, "forced": FORCED,
            "rescued": rescued, "input_requested": args.input, "input_results": input_results, "metrics": metrics,
            "inputs_sha256": inputs}, indent=2))
        if completed and not args.out and not args.keep and not failures:
            shutil.rmtree(root, ignore_errors=True)
    if failures:
        raise SystemExit("FAIL\n" + "\n".join(failures) + f"\nlogs: {root}")
    print("ok")


if __name__ == "__main__":
    main()
