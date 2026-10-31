#!/usr/bin/env python3
"""Private headless-Sway widget output-loss/recovery gate. Native execution is root-owned and serialized.

No physical input, session bus or live desktop is used. The readiness record must be deleted before its next
publication during output changes: an inotify sequence proves invalidation without sampling a tiny Starting gap.
This does not induce a retained-output layer-shell closed/retry window; see docs/wayland-widget-native-gate.md.
"""
import argparse
import ctypes
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import select
import shutil
import signal
import struct
import subprocess
import time

HELPER_PATH = Path(__file__).with_name("wayland_compact_popup.py")
SPEC = importlib.util.spec_from_file_location("private_popup", HELPER_PATH)
HELPER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(HELPER)

IN_DELETE, IN_MOVED_TO, IN_Q_OVERFLOW = 0x200, 0x80, 0x4000
SWAY_CONFIG = "xwayland disable\noutput HEADLESS-1 resolution 960x640 position 0 0 scale 1\n"


def parse_events(data):
    events, offset = [], 0
    while offset < len(data):
        if len(data) - offset < 16:
            raise ValueError("truncated inotify event")
        _, mask, _, length = struct.unpack_from("iIII", data, offset)
        offset += 16
        if len(data) - offset < length:
            raise ValueError("truncated inotify filename")
        name = os.fsdecode(data[offset:offset + length].split(b"\0", 1)[0])
        offset += length
        events.append({"mask": mask, "name": name})
    return events


def invalidated_before_publication(events, name):
    deleted = False
    for event in events:
        if event["mask"] & IN_Q_OVERFLOW:
            raise AssertionError("readiness event queue overflowed; evidence incomplete")
        if event["name"] != name:
            continue
        if event["mask"] & IN_DELETE:
            deleted = True
        if event["mask"] & IN_MOVED_TO:
            return deleted
    return False


class RecordWatch:
    def __init__(self, directory):
        libc = ctypes.CDLL(None, use_errno=True)
        self.fd = libc.inotify_init1(os.O_NONBLOCK | os.O_CLOEXEC)
        if self.fd < 0:
            raise OSError(ctypes.get_errno(), "inotify_init1")
        if libc.inotify_add_watch(self.fd, os.fsencode(directory), IN_DELETE | IN_MOVED_TO) < 0:
            error = ctypes.get_errno()
            os.close(self.fd)
            raise OSError(error, "inotify_add_watch")

    def drain(self):
        result = []
        while select.select([self.fd], [], [], 0)[0]:
            try:
                result.extend(parse_events(os.read(self.fd, 65536)))
            except BlockingIOError:
                break
        return result

    def close(self):
        os.close(self.fd)


def wait_for(fn, label, timeout=12):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = fn()
        if value:
            return value
        time.sleep(0.05)
    raise AssertionError(f"timed out: {label}")


def write_config(path, source="builtin:tty-clock", enabled=True):
    items = [] if source is None else [{"use": source, "place": "desktop", "monitor": "HEADLESS-1",
                                      "anchor": "top-left", "x": 32, "y": 32, "enabled": True}]
    temp = path.with_suffix(".new")
    temp.write_text(json.dumps({"widgets": {"enabled": enabled, "allow_exec": False, "items": items}}))
    os.replace(temp, path)


class Run:
    def __init__(self, root, binary):
        self.root, self.children, self.forced = root, [], []
        self.token = os.urandom(16).hex()
        self.runtime = root / "run"
        self.runtime.mkdir(mode=0o700)
        bins = root / "bin"
        bins.mkdir()
        self.binary = bins / "decwidgets"
        shutil.copyfile(binary, self.binary)
        self.binary.chmod(0o500)
        self.env = {k: v for k, v in os.environ.items() if not k.startswith(("XDG_", "DBUS_", "DECEBAL_", "RDWM_"))
                    and k not in ("DISPLAY", "WAYLAND_DISPLAY", "SWAYSOCK", "XAUTHORITY")}
        self.env.update(HOME=str(root / "home"), XDG_RUNTIME_DIR=str(self.runtime),
                        XDG_CONFIG_HOME=str(root / "config"), XDG_CACHE_HOME=str(root / "cache"),
                        XDG_DATA_HOME=str(root / "data"), XDG_DATA_DIRS=str(root / "data"),
                        XDG_STATE_HOME=str(root / "state"), DECEBAL_CONFIG=str(root / "config"),
                        DECEBAL_THEMES=str(root / "themes"), DECEBAL_SYSTEM_THEMES=str(root / "themes"),
                        DECEBAL_SYSTEM_AUTOMATIC="0", DECEBAL_NATIVE_TOKEN=self.token,
                        DECEBAL_BACKEND="wayland", XDG_SESSION_TYPE="wayland",
                        DBUS_SESSION_BUS_ADDRESS=f"unix:path={self.runtime}/absent-session-bus",
                        DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={self.runtime}/absent-system-bus",
                        RDWM_SOCK=str(self.runtime / "absent-rdwm"), PULSE_SERVER=f"unix:{self.runtime}/absent-pulse")
        (root / "config").mkdir()
        (root / "home").mkdir()
        self.config = root / "config/decebal.json"
        self.server = self.host = self.wayland = self.ipc = None
        self.statuses = []

    def start(self, name, argv, env):
        with (self.root / f"{name}.log").open("w") as log:
            proc = subprocess.Popen(argv, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        self.children.append((name, proc, os.pidfd_open(proc.pid)))
        return proc

    def owned(self):
        assert HELPER.socket_owned(self.server, self.wayland), "private Wayland socket ownership changed"
        assert HELPER.socket_owned(self.server, self.ipc), "private Sway IPC socket ownership changed"
        assert self.host is None or self.host.poll() is None, "widget host exited unexpectedly"

    def ipc_call(self, *args):
        self.owned()
        done = subprocess.run([shutil.which("swaymsg"), "-r", "-s", str(self.ipc), *args], env=self.env,
                              capture_output=True, text=True, check=True, timeout=5)
        value = json.loads(done.stdout)
        assert value and all(item.get("success") for item in value), value

    def status(self):
        self.owned()
        done = subprocess.run([str(self.binary), "status", "--json"], env=self.env,
                              capture_output=True, text=True, check=True, timeout=5)
        value = json.loads(done.stdout)
        self.statuses.append({"at": time.monotonic(), "status": value})
        return value

    def expect(self, state, wanted, shown, broken=0, unplaced=0):
        def matches():
            value = self.status()
            expected = {"wanted": wanted, "shown": shown, "broken": broken, "unplaced": unplaced}
            return value if value.get("running") is True and value.get("state") == state and value.get("cards") == expected else None
        return wait_for(matches, f"{state} {wanted}/{shown}/{broken}/{unplaced}")

    def close(self):
        for name, proc, fd in reversed(self.children):
            try:
                if proc.poll() is None:
                    try:
                        signal.pidfd_send_signal(fd, signal.SIGTERM)
                    except ProcessLookupError:
                        pass
                    try:
                        proc.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        self.forced.append(name)
                        signal.pidfd_send_signal(fd, signal.SIGKILL)
                        proc.wait(timeout=5)
            finally:
                os.close(fd)
        rescued = HELPER.rescue_tagged(self.token)
        return {"forced": self.forced, "rescued": rescued}


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--bin", type=Path, required=True, help="immutable decwidgets candidate")
    parser.add_argument("--out", type=Path, required=True, help="new evidence directory")
    parser.add_argument("--cycles", type=int, choices=range(1, 6), default=2)
    args = parser.parse_args()
    for name in ("sway", "swaymsg"):
        if not shutil.which(name):
            raise SystemExit(f"missing tool: {name}")
    root = args.out.resolve()
    root.mkdir(mode=0o700, parents=True, exist_ok=False)
    run = Run(root, args.bin.resolve())
    result = {"passed": False, "error": None, "transitions": [],
              "scope": "real private Sway output loss/recovery; record invalidation sequence; not a retained-output server-close retry test",
              "inputs_sha256": {"decwidgets": hashlib.sha256(run.binary.read_bytes()).hexdigest(),
                                "harness": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                                "private_popup_helper": hashlib.sha256(HELPER_PATH.read_bytes()).hexdigest()}}
    def interrupted(signum, frame):
        raise KeyboardInterrupt(f"interrupted by signal {signum}")
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    try:
        write_config(run.config)
        (root / "sway.conf").write_text(SWAY_CONFIG)
        env = dict(run.env, WLR_BACKENDS="headless", WLR_HEADLESS_OUTPUTS="1", WLR_LIBINPUT_NO_DEVICES="1", WLR_RENDERER="pixman")
        run.server = run.start("sway", [shutil.which("sway"), "-c", str(root / "sway.conf")], env)
        run.wayland = wait_for(lambda: next((p for p in run.runtime.glob("wayland-*")
                                            if not p.name.endswith(".lock") and HELPER.socket_owned(run.server, p)), None), "owned Wayland socket")
        run.ipc = wait_for(lambda: next((p for p in run.runtime.glob("sway-ipc.*.sock")
                                       if HELPER.socket_owned(run.server, p)), None), "owned Sway IPC socket")
        run.env["WAYLAND_DISPLAY"] = run.wayland.name
        run.host = run.start("decwidgets", [str(run.binary), "daemon"], dict(run.env, WAYLAND_DEBUG="client"))
        run.expect("ready", 1, 1)
        record = wait_for(lambda: next(iter(run.runtime.glob("decwidgets-wl-*.state")), None), "host readiness record")
        for cycle in range(args.cycles):
            for action, expected in (("disable", ("degraded", 1, 0, 0, 1)), ("enable", ("ready", 1, 1, 0, 0))):
                watch = RecordWatch(run.runtime)
                try:
                    run.ipc_call("output", "HEADLESS-1", action)
                    status = run.expect(*expected)
                    events = watch.drain()
                    assert invalidated_before_publication(events, record.name), f"stale readiness was not removed before {action} publication: {events}"
                    result["transitions"].append({"cycle": cycle, "action": action, "status": status,
                                                  "readiness_events": events, "same_host_pid": run.host.pid,
                                                  "threads": len(list(Path(f"/proc/{run.host.pid}/task").iterdir()))})
                finally:
                    watch.close()
        write_config(run.config, "builtin:fixture-missing-definition")
        result["invalid_definition"] = run.expect("degraded", 1, 0, 1, 0)
        write_config(run.config, None)
        result["zero_enabled"] = run.expect("ready", 0, 0)
        write_config(run.config)
        result["restored_configuration"] = run.expect("ready", 1, 1)
        result["passed"] = True
    except BaseException as error:
        result["error"] = repr(error)
        raise
    finally:
        result["statuses"] = run.statuses
        try:
            result["cleanup"] = run.close()
        except BaseException as error:
            result["cleanup"] = {"forced": run.forced, "rescued": [], "error": repr(error)}
            result["passed"] = False
        result["passed"] = result["passed"] and not result["cleanup"]["forced"] and not result["cleanup"]["rescued"]
        (root / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    if not result["passed"]:
        raise SystemExit("irregular cleanup; see result.json")
    print(f"PASS widget output loss/recovery, invalidation, invalid/zero/restored configuration: {root}")


if __name__ == "__main__":
    main()
