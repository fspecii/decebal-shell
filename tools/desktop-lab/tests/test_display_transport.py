"""High-display reservation. These tests do not create or remove live X sockets."""

from __future__ import annotations

import json
import os
from pathlib import Path
import secrets
import signal
import socket
import stat
import subprocess
import sys
import tempfile
import textwrap
import time
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from lab.display import (
    _argv_display,
    _filesystem_socket_ok,
    _lock_pid,
    abstract_display_numbers,
    listening_socket_inodes,
    process_socket_inodes,
    require_listener_ownership,
    reserve_high_display,
    stop_low_owned_xvfb,
    transport_verdict,
    xvfb_argv,
)
from lab.policy import MAX_PRIVATE_DISPLAY, MIN_PRIVATE_DISPLAY, PolicyError

_PROC_SAMPLE = """\
Num       RefCount Protocol Flags    Type St Inode Path
00000000: 00000002 00000000 00010000 0001 01 111 @/tmp/.X11-unix/X0
00000000: 00000002 00000000 00000000 0001 01 222 /tmp/.X11-unix/X0
00000000: 00000002 00000000 00010000 0001 01 333 @/tmp/.X11-unix/X974
00000000: 00000002 00000000 00010000 0001 01 334 @/tmp/.X11-unix/Xnot
"""


class _Proc:
    def __init__(self, running: bool = True) -> None:
        self.returncode = None if running else 0

    def poll(self):
        return self.returncode


class _Owned:
    def __init__(self) -> None:
        self.tracks: list[dict] = []
        self.stopped: list[dict] = []

    def stop_one(self, track: dict, timeout: float = 1) -> None:
        self.stopped.append(track)
        track["process"].returncode = -15


class DisplayTransportTests(unittest.TestCase):
    def test_abstract_parser_ignores_a_filesystem_path(self) -> None:
        self.assertEqual(abstract_display_numbers(_PROC_SAMPLE), {0, 974})

    def test_reserve_skips_abstract_lock_and_filesystem_hits(self) -> None:
        number = reserve_high_display(
            703,
            locks={700},
            filesystem={701},
            abstract={0, 702},
        )
        self.assertEqual(number, 704)
        self.assertGreaterEqual(number, MIN_PRIVATE_DISPLAY)

    def test_full_high_window_fails_closed(self) -> None:
        busy = set(range(MIN_PRIVATE_DISPLAY, MAX_PRIVATE_DISPLAY + 1))
        with self.assertRaises(PolicyError) as caught:
            reserve_high_display(None, locks=busy, filesystem=set(), abstract=set())
        self.assertIn("700", str(caught.exception))

    def test_argv_uses_an_explicit_high_display(self) -> None:
        argv = xvfb_argv("/usr/bin/Xvfb", 720, "640x480x24")
        self.assertEqual(argv[1], ":720")
        self.assertNotIn("-displayfd", argv)
        with self.assertRaises(PolicyError):
            xvfb_argv("/usr/bin/Xvfb", 0, "640x480x24")
        with self.assertRaises(PolicyError):
            xvfb_argv("/usr/bin/Xvfb", 20, "640x480x24")

    def test_listener_parser_keeps_state_01_only(self) -> None:
        text = (
            "Num       RefCount Protocol Flags    Type St Inode Path\n"
            "00000000: 00000002 00000000 00010000 0001 01 10 /tmp/sock\n"
            "00000000: 00000002 00000000 00000000 0001 03 11 /tmp/sock\n"
            "00000000: 00000002 00000000 00000000 0002 01 13 /tmp/sock\n"
            "00000000: 00000002 00000000 00000000 0001 01 14 /tmp/sock\n"
            "00000000: 00000002 00000000 00010000 0001 01 12 @abs\n"
        )
        self.assertEqual(listening_socket_inodes(text, "/tmp/sock"), {10})
        self.assertEqual(listening_socket_inodes(text, "@abs"), {12})

    def test_bound_datagram_on_the_host_is_not_a_listener(self) -> None:
        with tempfile.TemporaryDirectory(prefix="desktop-lab-dgram-") as raw:
            root = Path(raw)
            stream_path = root / "stream.sock"
            dgram_path = root / "dgram.sock"
            abstract = f"desktop-lab-listen-{os.getpid()}-{secrets.token_hex(8)}"
            dgram_abstract = f"desktop-lab-dgram-{os.getpid()}-{secrets.token_hex(8)}"
            stream = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            dgram = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
            abstract_stream = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            abstract_dgram = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
            try:
                stream.bind(os.fspath(stream_path))
                stream.listen(1)
                dgram.bind(os.fspath(dgram_path))
                abstract_stream.bind("\0" + abstract)
                abstract_stream.listen(1)
                abstract_dgram.bind("\0" + dgram_abstract)
                text = Path("/proc/net/unix").read_text(errors="replace")
                own = process_socket_inodes(os.getpid())
                stream_line = _unix_line(text, os.fspath(stream_path))
                dgram_line = _unix_line(text, os.fspath(dgram_path))
                abstract_dgram_line = _unix_line(text, "@" + dgram_abstract)
                self.assertIsNotNone(stream_line)
                self.assertIsNotNone(dgram_line)
                self.assertIsNotNone(abstract_dgram_line)
                self.assertEqual(dgram_line[5], "01")
                self.assertEqual(int(dgram_line[4], 16), socket.SOCK_DGRAM)
                self.assertEqual(abstract_dgram_line[5], "01")
                self.assertEqual(int(abstract_dgram_line[4], 16), socket.SOCK_DGRAM)
                self.assertEqual(int(stream_line[3], 16) & 0x10000, 0x10000)
                self.assertEqual(int(stream_line[4], 16), socket.SOCK_STREAM)
                self.assertEqual(stream_line[5], "01")
                stream_inodes = listening_socket_inodes(text, os.fspath(stream_path))
                abstract_inodes = listening_socket_inodes(text, "@" + abstract)
                self.assertEqual(listening_socket_inodes(text, os.fspath(dgram_path)), set())
                self.assertEqual(listening_socket_inodes(text, "@" + dgram_abstract), set())
                self.assertIn(int(stream_line[6]), stream_inodes)
                self.assertIn(int(dgram_line[6]), own)
                self.assertNotIn(int(dgram_line[6]), stream_inodes)
                self.assertTrue(stream_inodes <= own)
                self.assertTrue(abstract_inodes <= own)
                self.assertTrue(stream_inodes.isdisjoint(abstract_inodes))
                self.assertTrue(stream_path.exists())
                self.assertTrue(dgram_path.exists())
            finally:
                for held in (stream, dgram, abstract_stream, abstract_dgram):
                    held.close()
                for path in (stream_path, dgram_path):
                    try:
                        path.unlink()
                    except OSError:
                        pass

    def test_transport_proof_requires_both_namespaces(self) -> None:
        base = dict(
            number=720,
            pid=5,
            alive=True,
            lock_pid=5,
            filesystem_is_socket=True,
            filesystem_is_symlink=False,
            abstract_present=True,
            filesystem_listeners={11},
            abstract_listeners={22},
            server_socket_inodes={11, 22},
        )
        proof = transport_verdict(**base)
        self.assertEqual(proof["number"], 720)
        self.assertTrue(proof["filesystem_socket"])
        self.assertTrue(proof["abstract_socket"])
        self.assertTrue(proof["lock_pid_matches"])
        self.assertFalse(proof["displayfd_probe"])
        self.assertEqual(proof["filesystem_inode"], 11)
        self.assertEqual(proof["abstract_inode"], 22)
        self.assertEqual(proof["filesystem_listener_inodes"], [11])
        self.assertEqual(proof["abstract_listener_inodes"], [22])
        self.assertTrue(proof["inodes_owned_by_server"])
        rejected = []
        for update in (
            {"number": 0},
            {"number": 20},
            {"number": 699},
            {"alive": False},
            {"lock_pid": 9},
            {"filesystem_is_socket": False},
            {"filesystem_is_symlink": True},
            {"abstract_present": False},
            {"filesystem_listeners": set()},
            {"abstract_listeners": set()},
            {"server_socket_inodes": {11}},
            {"filesystem_listeners": {11, 99}},
            {"filesystem_listeners": {11}, "abstract_listeners": {11}, "server_socket_inodes": {11}},
        ):
            with self.assertRaises(PolicyError):
                transport_verdict(**(base | update))
            rejected.append(update)
        self.assertEqual(len(rejected), 13)
        with self.assertRaises(PolicyError):
            require_listener_ownership(
                filesystem_listeners={10, 99},
                abstract_listeners={12},
                server_socket_inodes={10, 12},
            )

    def test_low_display_stops_only_the_owned_track(self) -> None:
        owned = _Owned()
        low = {"name": "xvfb", "argv": ["/usr/bin/Xvfb", ":0", "-ac"], "process": _Proc()}
        probed = {"name": "xvfb", "argv": ["/usr/bin/Xvfb", "-displayfd", "4", "-ac"], "process": _Proc()}
        high = {"name": "xvfb", "argv": ["/usr/bin/Xvfb", ":720", "-ac"], "process": _Proc()}
        other = {"name": "session-bus", "argv": ["/usr/bin/dbus-daemon"], "process": _Proc()}
        owned.tracks = [low, probed, high, other]
        with self.assertRaises(PolicyError) as caught:
            stop_low_owned_xvfb(owned)
        self.assertEqual(owned.stopped, [low])
        self.assertIn("before input", str(caught.exception))
        self.assertIsNone(high["process"].returncode)
        self.assertIsNone(other["process"].returncode)
        self.assertEqual(_argv_display(probed["argv"]), 0)
        self.assertEqual(_argv_display(high["argv"]), 720)

    def test_a_dead_low_display_is_not_signaled(self) -> None:
        owned = _Owned()
        dead = {"name": "xvfb", "argv": ["/usr/bin/Xvfb", ":0"], "process": _Proc(running=False)}
        owned.tracks = [dead]
        stop_low_owned_xvfb(owned)
        self.assertEqual(owned.stopped, [])

    def test_socket_and_lock_checks_do_not_follow_links(self) -> None:
        with tempfile.TemporaryDirectory(prefix="desktop-lab-xproof-") as raw:
            root = Path(raw)
            missing = root / "missing"
            self.assertEqual(_filesystem_socket_ok(str(missing)), (False, False))
            plain = root / "plain"
            plain.write_text("nope")
            self.assertEqual(_filesystem_socket_ok(str(plain)), (False, False))
            link = root / "link"
            link.symlink_to(plain)
            self.assertEqual(_filesystem_socket_ok(str(link))[1], True)
            target = root / "target"
            target.write_text("999\n")
            lock_link = root / "lock-link"
            lock_link.symlink_to(target)
            self.assertIsNone(_lock_pid(str(lock_link)))
            own = root / "own"
            own.write_text("  42\n")
            self.assertEqual(_lock_pid(str(own)), 42)
            sock_path = root / "sock"
            held = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            try:
                held.bind(str(sock_path))
                info = os.lstat(sock_path)
                self.assertTrue(stat.S_ISSOCK(info.st_mode))
                self.assertEqual(_filesystem_socket_ok(str(sock_path)), (True, False))
            finally:
                held.close()

    def test_foreign_socket_and_owned_looking_lock_refuse_before_input(self) -> None:
        helper_script = textwrap.dedent(
            """\
            import json, os, socket, sys, time
            fs_path, abstract, result = sys.argv[1:]
            filesystem = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            filesystem.bind(fs_path)
            filesystem.listen(1)
            abstract_sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            abstract_sock.bind("\\0" + abstract)
            abstract_sock.listen(1)
            with open(result, "w", encoding="utf-8") as handle:
                json.dump({"pid": os.getpid()}, handle)
                handle.flush()
                os.fsync(handle.fileno())
            while True:
                time.sleep(30)
            """
        )
        with tempfile.TemporaryDirectory(prefix="desktop-lab-foreign-sock-") as raw:
            root = Path(raw)
            script = root / "holder.py"
            script.write_text(helper_script)
            fs_path = root / "foreign.sock"
            abstract = f"desktop-lab-proof-{os.getpid()}-{secrets.token_hex(8)}"
            result = root / "holder.json"
            helper = subprocess.Popen(
                [sys.executable, str(script), str(fs_path), abstract, str(result)],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                start_new_session=True,
            )
            helper_start = _start_of(helper.pid)
            sleeper = subprocess.Popen(
                ["/bin/sleep", "60"],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                start_new_session=True,
            )
            sleeper_start = _start_of(sleeper.pid)
            lock = root / "owned-looking.lock"
            lock.write_text(f"{sleeper.pid}\n")
            inputs: list[dict] = []
            try:
                self.assertTrue(_wait_file(result, 5), "helper did not publish its sockets")
                self.assertNotEqual(helper.pid, sleeper.pid)
                before = os.lstat(fs_path)
                self.assertTrue(stat.S_ISSOCK(before.st_mode))
                self.assertFalse(stat.S_ISLNK(before.st_mode))
                self.assertEqual(_lock_pid(str(lock)), sleeper.pid)
                text = Path("/proc/net/unix").read_text(errors="replace")
                filesystem_listeners = listening_socket_inodes(text, str(fs_path))
                abstract_listeners = listening_socket_inodes(text, "@" + abstract)
                self.assertTrue(filesystem_listeners)
                self.assertTrue(abstract_listeners)
                self.assertTrue(filesystem_listeners.isdisjoint(abstract_listeners))
                helper_fds = process_socket_inodes(helper.pid)
                self.assertTrue(filesystem_listeners <= helper_fds)
                self.assertTrue(abstract_listeners <= helper_fds)
                sleeper_fds = process_socket_inodes(sleeper.pid)
                self.assertTrue(filesystem_listeners.isdisjoint(sleeper_fds))
                proof = None
                try:
                    proof = transport_verdict(
                        number=720,
                        pid=sleeper.pid,
                        alive=True,
                        lock_pid=_lock_pid(str(lock)),
                        filesystem_is_socket=True,
                        filesystem_is_symlink=False,
                        abstract_present=True,
                        filesystem_listeners=filesystem_listeners,
                        abstract_listeners=abstract_listeners,
                        server_socket_inodes=sleeper_fds,
                    )
                except PolicyError:
                    proof = None
                else:
                    inputs.append(proof)
                self.assertIsNone(proof)
                self.assertEqual(inputs, [])
                after = os.lstat(fs_path)
                self.assertEqual(after.st_ino, before.st_ino)
                self.assertEqual(lock.read_text(), f"{sleeper.pid}\n")
                self.assertIsNone(helper.poll())
                self.assertTrue(fs_path.exists())
                still = listening_socket_inodes(Path("/proc/net/unix").read_text(errors="replace"), "@" + abstract)
                self.assertEqual(still, abstract_listeners)
                owned = transport_verdict(
                    number=720,
                    pid=helper.pid,
                    alive=True,
                    lock_pid=helper.pid,
                    filesystem_is_socket=True,
                    filesystem_is_symlink=False,
                    abstract_present=True,
                    filesystem_listeners=filesystem_listeners,
                    abstract_listeners=abstract_listeners,
                    server_socket_inodes=process_socket_inodes(helper.pid),
                )
                inputs.append(owned)
                self.assertEqual(len(inputs), 1)
                self.assertTrue(owned["inodes_owned_by_server"])
                self.assertNotEqual(owned["filesystem_inode"], owned["abstract_inode"])
                self.assertIn(owned["filesystem_inode"], filesystem_listeners)
                self.assertIn(owned["abstract_inode"], abstract_listeners)
                self.assertTrue(fs_path.exists())
                self.assertIsNone(helper.poll())
                self.assertEqual(lock.read_text(), f"{sleeper.pid}\n")
            finally:
                _signal_match(helper.pid, helper_start, signal.SIGKILL)
                _signal_match(sleeper.pid, sleeper_start, signal.SIGKILL)
                helper.wait(timeout=3)
                sleeper.wait(timeout=3)


def _unix_line(text: str, path: str) -> list[str] | None:
    for line in text.splitlines():
        parts = line.split()
        if len(parts) >= 8 and parts[7] == path:
            return parts
    return None


def _start_of(pid: int) -> int | None:
    try:
        stat_text = Path(f"/proc/{pid}/stat").read_text()
        rest = stat_text[stat_text.rfind(")") + 2 :].split()
        return int(rest[19])
    except (OSError, IndexError, ValueError):
        return None


def _signal_match(pid: int, start: int | None, sig: int) -> None:
    if not isinstance(start, int) or _start_of(pid) != start:
        return
    fd = None
    try:
        fd = os.pidfd_open(pid)
        if _start_of(pid) != start:
            return
        signal.pidfd_send_signal(fd, sig)
    except OSError:
        return
    finally:
        if fd is not None:
            os.close(fd)


def _wait_file(path: Path, seconds: float) -> bool:
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if path.is_file():
            return True
        time.sleep(0.02)
    return False


if __name__ == "__main__":
    unittest.main()
