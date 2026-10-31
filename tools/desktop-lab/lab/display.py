"""One private Xvfb, or one headless compositor, plus PNG capture.

Xvfb listens on an explicit display from 700 through 999 with -nolisten tcp,
-noreset, and -ac. Reservation skips a number when its lock, filesystem socket
directory entry, or abstract socket is already present. Before input, every
listening socket inode of both namespaces must be a socket fd of the spawned
server. -displayfd is not used. Foreign sockets and locks are not unlinked.
The flock keeps this lane to a single Xvfb. Wayland capture is not invented
when grim is absent.
"""

from __future__ import annotations

import fcntl
import hashlib
import os
from pathlib import Path
import re
import stat
import struct
import subprocess
import time

from .policy import (
    MAX_PRIVATE_DISPLAY,
    MIN_PRIVATE_DISPLAY,
    PolicyError,
    ToolMissing,
    host_display_number,
    parse_display_number,
)
from .procman import Owned
from .slot import open_owned_lock

# A /proc/net/unix path without the leading '@' is not an abstract name, and it
# is not proof that the filesystem directory entry exists. Display :0 can keep
# the abstract socket after X0 is gone. The cause of that missing path is
# unconfirmed. This module never unlinks a socket or a lock.
_ABSTRACT_NAME = re.compile(r"(?m)(?:^|\s)@/tmp/\.X11-unix/X(\d+)\s*$")
# State 01 is also a bound datagram. A listener has SO_ACCEPTCON and SOCK_STREAM.
_SO_ACCEPTCON = 0x10000
_SOCK_STREAM = 1

_PNG = b"\x89PNG\r\n\x1a\n"


_HELD_XVFB: set[str] = set()


class XvfbSlot:
    def __init__(self, path: Path | None = None) -> None:
        self.path = path or Path(f"/tmp/decebal-desktop-lab-{os.getuid()}/xvfb.lock")
        self.fd: int | None = None
        self._key: str | None = None
        self._inode_key: str | None = None

    def acquire(self) -> None:
        lexical = str(self.path)
        if lexical in _HELD_XVFB:
            raise PolicyError("this lane already owns an Xvfb (max 1)")
        fd = open_owned_lock(self.path)
        try:
            info = os.fstat(fd)
            if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or info.st_uid != os.getuid():
                raise PolicyError("refusing to write a lock file that is not a private regular file")
            inode_key = f"{info.st_dev}:{info.st_ino}"
            if inode_key in _HELD_XVFB:
                raise PolicyError("this lane already owns an Xvfb (max 1)")
            os.ftruncate(fd, 0)
            os.write(fd, f"{os.getpid()}\n".encode())
        except Exception:
            fcntl.flock(fd, fcntl.LOCK_UN)
            os.close(fd)
            raise
        self.fd = fd
        self._key = lexical
        self._inode_key = inode_key
        _HELD_XVFB.add(lexical)
        _HELD_XVFB.add(inode_key)

    def release(self) -> None:
        if self.fd is None:
            return
        if self._key is not None:
            _HELD_XVFB.discard(self._key)
        if self._inode_key is not None:
            _HELD_XVFB.discard(self._inode_key)
        fcntl.flock(self.fd, fcntl.LOCK_UN)
        os.close(self.fd)
        self.fd = None
        self._key = None
        self._inode_key = None


def xvfb_argv(binary: str, number: int, screen: str) -> list[str]:
    if number < MIN_PRIVATE_DISPLAY or number > MAX_PRIVATE_DISPLAY:
        raise PolicyError(f"refusing to start Xvfb on :{number}")
    argv = [binary, f":{number}", "-screen", "0", screen, "-nolisten", "tcp", "-noreset", "-ac"]
    if "-displayfd" in argv:
        raise PolicyError("refusing -displayfd")
    return argv


def abstract_display_numbers(text: str) -> set[int]:
    """Display numbers whose Linux abstract socket is `@/tmp/.X11-unix/XN`."""

    return {int(match) for match in _ABSTRACT_NAME.findall(text)}


def reserve_high_display(
    host: int | None,
    *,
    locks: set[int],
    filesystem: set[int],
    abstract: set[int],
) -> int:
    """First free display in 700..999. A full window fails closed below 700."""

    for number in range(MIN_PRIVATE_DISPLAY, MAX_PRIVATE_DISPLAY + 1):
        if number == host or number in locks or number in filesystem or number in abstract:
            continue
        return number
    raise PolicyError("no free X display at or above 700")


def listening_socket_inodes(text: str, path: str) -> set[int]:
    """Inodes of listening AF_UNIX stream sockets whose path is exactly `path`.

    A listener has Flags bit SO_ACCEPTCON (0x10000), type SOCK_STREAM (1), and
    state 01. A bound datagram is state 01 with type SOCK_DGRAM and is omitted.
    """

    found: set[int] = set()
    for line in text.splitlines():
        parts = line.split()
        if len(parts) < 8 or parts[5] != "01" or parts[7] != path or not parts[6].isdigit():
            continue
        try:
            flags = int(parts[3], 16)
            sock_type = int(parts[4], 16)
        except ValueError:
            continue
        if flags & _SO_ACCEPTCON == 0 or sock_type != _SOCK_STREAM:
            continue
        found.add(int(parts[6]))
    return found


def process_socket_inodes(pid: int) -> set[int]:
    """Socket inodes linked from /proc/PID/fd as socket:[inode]."""

    found: set[int] = set()
    try:
        entries = os.scandir(f"/proc/{pid}/fd")
    except OSError:
        return found
    with entries:
        for entry in entries:
            try:
                target = os.readlink(entry.path)
            except OSError:
                continue
            if target.startswith("socket:[") and target.endswith("]"):
                inode = target[len("socket:[") : -1]
                if inode.isdigit():
                    found.add(int(inode))
    return found


def require_listener_ownership(
    *,
    filesystem_listeners: set[int],
    abstract_listeners: set[int],
    server_socket_inodes: set[int],
) -> tuple[int, int]:
    """Both listening sockets must be fds of the spawned server. Presence is not ownership.

    The inode is the one in `/proc/net/unix` and in `socket:[inode]`. It is not
    the directory entry's `st_ino`. Every listening inode for that exact path
    has to be in the server fd set, so an extra foreign listener is refused.
    Listening means SO_ACCEPTCON, SOCK_STREAM, and state 01.
    """

    if not filesystem_listeners or not filesystem_listeners <= server_socket_inodes:
        raise PolicyError("filesystem X socket inode is not held by the spawned server")
    if not abstract_listeners or not abstract_listeners <= server_socket_inodes:
        raise PolicyError("abstract X socket inode is not held by the spawned server")
    if filesystem_listeners & abstract_listeners:
        raise PolicyError("filesystem and abstract sockets are not two distinct inodes")
    return min(filesystem_listeners), min(abstract_listeners)


def transport_verdict(
    *,
    number: int,
    pid: int,
    alive: bool,
    lock_pid: int | None,
    filesystem_is_socket: bool,
    filesystem_is_symlink: bool,
    abstract_present: bool,
    filesystem_listeners: set[int],
    abstract_listeners: set[int],
    server_socket_inodes: set[int],
) -> dict:
    """Proof required before a client connects. This function does not signal or unlink."""

    if number < MIN_PRIVATE_DISPLAY:
        raise PolicyError(f"display :{number} is below {MIN_PRIVATE_DISPLAY}")
    if not alive:
        raise PolicyError("owned Xvfb is not alive")
    if lock_pid != pid:
        raise PolicyError("Xvfb lock file did not identify the owned server")
    if filesystem_is_symlink or not filesystem_is_socket:
        raise PolicyError("filesystem X socket is absent or is not a socket")
    if not abstract_present:
        raise PolicyError("abstract X socket is absent")
    filesystem_inode, abstract_inode = require_listener_ownership(
        filesystem_listeners=filesystem_listeners,
        abstract_listeners=abstract_listeners,
        server_socket_inodes=server_socket_inodes,
    )
    return {
        "number": number,
        "filesystem_socket": True,
        "abstract_socket": True,
        "lock_pid_matches": True,
        "displayfd_probe": False,
        "filesystem_inode": filesystem_inode,
        "abstract_inode": abstract_inode,
        "filesystem_listener_inodes": sorted(filesystem_listeners),
        "abstract_listener_inodes": sorted(abstract_listeners),
        "inodes_owned_by_server": True,
    }


def _argv_display(argv: list[str]) -> int | None:
    for arg in argv:
        number = parse_display_number(arg)
        if number is not None:
            return number
    if "-displayfd" in argv:
        return 0
    return None


def stop_low_owned_xvfb(owned: Owned) -> None:
    """Stop a server this object spawned on a low display, before any input.

    Only tracks already recorded on `owned` are signaled. The live server and
    other processes are left alone. Sockets and locks are not unlinked.
    """

    for track in owned.tracks:
        process = track.get("process")
        if track.get("name") != "xvfb" or process is None or process.poll() is not None:
            continue
        number = _argv_display(list(track.get("argv") or []))
        if number is None or number >= MIN_PRIVATE_DISPLAY:
            continue
        owned.stop_one(track, 1)
        raise PolicyError(f"stopped the owned Xvfb on :{number} before input")


def start_xvfb(owned: Owned, *, binary: str, screen: str, log, env: dict[str, str]) -> tuple[dict, str]:
    if not Path(binary).is_file():
        raise ToolMissing(f"Xvfb is missing at {binary}")
    stop_low_owned_xvfb(owned)
    host = host_display_number()
    last: PolicyError | None = None
    for _ in range(5):
        locks, filesystem, abstract = _display_snapshot()
        number = reserve_high_display(host, locks=locks, filesystem=filesystem, abstract=abstract)
        if number < MIN_PRIVATE_DISPLAY:
            raise PolicyError(f"refusing to start Xvfb on :{number}")
        if _display_busy_now(number) or number == host:
            continue
        track = owned.spawn(
            xvfb_argv(binary, number, screen),
            env=env,
            name="xvfb",
            stdout=log,
            stderr=subprocess.STDOUT,
        )
        try:
            proof = _wait_owned_transport(number, track, 5)
        except PolicyError as exc:
            owned.stop_one(track, 1)
            last = exc
            continue
        track["display_transport"] = proof
        return track, f":{number}"
    raise last or PolicyError("no free display at or above 700 accepted the owned Xvfb")


def _display_snapshot() -> tuple[set[int], set[int], set[int]]:
    return _lock_numbers(), _filesystem_display_numbers(), _read_abstract_numbers()


def _display_busy_now(number: int) -> bool:
    locks, filesystem, abstract = _display_snapshot()
    return number in locks or number in filesystem or number in abstract


def _read_abstract_numbers() -> set[int]:
    try:
        text = Path("/proc/net/unix").read_text(errors="replace")
    except OSError as exc:
        raise PolicyError("cannot read abstract unix sockets") from exc
    return abstract_display_numbers(text)


def _lock_numbers() -> set[int]:
    found: set[int] = set()
    try:
        entries = os.scandir("/tmp")
    except OSError as exc:
        raise PolicyError("cannot inspect X lock names") from exc
    with entries:
        for entry in entries:
            name = entry.name
            if not name.startswith(".X") or not name.endswith("-lock"):
                continue
            number = name[2:-5]
            if number.isdigit():
                found.add(int(number))
    return found


def _filesystem_display_numbers() -> set[int]:
    found: set[int] = set()
    directory = "/tmp/.X11-unix"
    try:
        info = os.lstat(directory)
    except FileNotFoundError:
        return found
    except OSError as exc:
        raise PolicyError("cannot inspect the X socket directory") from exc
    if stat.S_ISLNK(info.st_mode) or not stat.S_ISDIR(info.st_mode):
        raise PolicyError("refusing a non-directory X socket path")
    try:
        entries = os.scandir(directory)
    except OSError as exc:
        raise PolicyError("cannot inspect the X socket directory") from exc
    with entries:
        for entry in entries:
            name = entry.name
            if name.startswith("X") and name[1:].isdigit():
                found.add(int(name[1:]))
    return found


def _lock_pid(path: str) -> int | None:
    try:
        fd = os.open(path, os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW)
    except OSError:
        return None
    try:
        info = os.fstat(fd)
        if not stat.S_ISREG(info.st_mode):
            return None
        data = os.read(fd, 64)
    finally:
        os.close(fd)
    try:
        return int(data.decode().strip())
    except (UnicodeError, ValueError):
        return None


def _filesystem_socket_ok(path: str) -> tuple[bool, bool]:
    """Return (is_socket, is_symlink) from lstat. A missing path is neither."""

    try:
        info = os.lstat(path)
    except OSError:
        return False, False
    return stat.S_ISSOCK(info.st_mode), stat.S_ISLNK(info.st_mode)


def _wait_owned_transport(number: int, track: dict, timeout: float) -> dict:
    if number < MIN_PRIVATE_DISPLAY:
        raise PolicyError(f"display :{number} is below {MIN_PRIVATE_DISPLAY}")
    lock_path = f"/tmp/.X{number}-lock"
    sock_path = f"/tmp/.X11-unix/X{number}"
    abstract_name = f"@/tmp/.X11-unix/X{number}"
    pid = track["pid"]
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if track["process"].poll() is not None:
            raise PolicyError("Xvfb exited before its sockets identified the owned server")
        lock_pid = _lock_pid(lock_path)
        is_socket, is_symlink = _filesystem_socket_ok(sock_path)
        try:
            text = Path("/proc/net/unix").read_text(errors="replace")
        except OSError as exc:
            raise PolicyError("cannot read abstract unix sockets") from exc
        abstract_present = number in abstract_display_numbers(text)
        alive = track["process"].poll() is None
        if alive and lock_pid == pid and is_socket and not is_symlink and abstract_present:
            try:
                return transport_verdict(
                    number=number,
                    pid=pid,
                    alive=True,
                    lock_pid=lock_pid,
                    filesystem_is_socket=True,
                    filesystem_is_symlink=False,
                    abstract_present=True,
                    filesystem_listeners=listening_socket_inodes(text, sock_path),
                    abstract_listeners=listening_socket_inodes(text, abstract_name),
                    server_socket_inodes=process_socket_inodes(pid),
                )
            except PolicyError:
                pass
        time.sleep(0.02)
    raise PolicyError("owned Xvfb did not prove both socket namespaces and the lock pid")


def start_session_bus(owned: Owned, runtime: Path, *, binary: str, log, env: dict[str, str]) -> str:
    if not Path(binary).is_file():
        raise ToolMissing(f"dbus-daemon is missing at {binary}")
    sock = runtime / "session-bus"
    config = runtime / "session.conf"
    # No service directories and no systemd activation: a private bus cannot
    # start installed host services. Same shape as tests/file-reveal.py.
    config.write_text(
        "<busconfig><type>session</type>"
        f"<listen>unix:path={sock}</listen>"
        "<auth>EXTERNAL</auth><policy context=\"default\">"
        "<allow send_destination=\"*\"/><allow receive_sender=\"*\"/><allow own=\"*\"/>"
        "</policy></busconfig>\n"
    )
    track = owned.spawn(
        [binary, "--nofork", "--nopidfile", "--nosyslog", f"--config-file={config}"],
        env=env,
        name="session-bus",
        stdout=log,
        stderr=subprocess.STDOUT,
    )
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        if track["process"].poll() is not None:
            raise PolicyError(f"private session bus exited {track['process'].returncode}")
        if sock.exists() and stat.S_ISSOCK(sock.stat().st_mode):
            return f"unix:path={sock}"
        time.sleep(0.02)
    raise PolicyError("private session bus socket did not appear")


def start_wayland(owned: Owned, runtime: Path, *, binary: str, log, env: dict[str, str]) -> str:
    if not Path(binary).is_file():
        raise ToolMissing(f"sway is missing at {binary}")
    config = runtime / "sway.conf"
    config.write_text("output * mode 640x480\nseat * hide_cursor 1000\n")
    child = dict(env)
    child.pop("WAYLAND_DISPLAY", None)
    child.pop("DISPLAY", None)
    child["WLR_BACKENDS"] = "headless"
    child["WLR_RENDERER"] = "pixman"
    child["WLR_HEADLESS_OUTPUTS"] = "1"
    track = owned.spawn(
        [binary, "--unsupported-gpu", "-c", str(config)],
        env=child,
        name="wayland",
        stdout=log,
        stderr=subprocess.STDOUT,
    )
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        if track["process"].poll() is not None:
            raise PolicyError(f"private compositor exited {track['process'].returncode}")
        for entry in runtime.iterdir():
            name = entry.name
            if name.startswith("wayland-") and not name.endswith(".lock"):
                return name
        time.sleep(0.02)
    raise PolicyError("private Wayland socket did not appear")


def capture_png(binary: str, dest: Path, env: dict[str, str]) -> dict:
    if not Path(binary).is_file():
        raise ToolMissing(f"screenshot tool is missing at {binary}")
    proc = subprocess.run(
        [binary, "-window", "root", str(dest)],
        env=env,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=10,
        check=False,
    )
    if proc.returncode != 0 or not dest.is_file():
        raise PolicyError("screenshot command failed; no image was invented")
    return png_info(dest)


def png_info(path: Path) -> dict:
    data = path.read_bytes()
    if not data.startswith(_PNG) or data[12:16] != b"IHDR":
        raise PolicyError("screenshot is not a PNG")
    width, height = struct.unpack(">II", data[16:24])
    if width < 1 or height < 1:
        raise PolicyError("screenshot PNG has no size")
    return {
        "path": str(path),
        "sha256": hashlib.sha256(data).hexdigest(),
        "bytes": len(data),
        "width": width,
        "height": height,
    }
