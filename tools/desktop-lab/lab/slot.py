"""Parent-held build slots. The lock fd is CLOEXEC and is not passed to the job.

Plain `flock file command` leaves the lock fd open across exec. A double-fork
daemon then keeps the semaphore after the job. `flock --close` and this wrapper
keep the lock out of the job command. A guardian may inherit a duplicate of the
same open file description. `LOCK_UN` on any duplicate drops the lock for all
of them, so the guardian releases by closing its fd after the contained tree
is verified dead.

Live coordinator slots are refused before any open. The open itself does not
follow symlinks: a parent path is walked with O_NOFOLLOW, the immediate parent
must be a user-owned mode-0700 directory, and the fd is fstat'd (regular file,
owner, nlink == 1) before flock and before any truncate or write.
"""

from __future__ import annotations

import errno
import fcntl
import os
from pathlib import Path
import stat

from .policy import LIVE_SLOT_PATHS, PolicyError, live_slot_path


def equivalent_argv(slot: Path, argv: list[str]) -> list[str]:
    if live_slot_path(str(slot)):
        raise PolicyError("refusing a live build slot")
    return ["flock", "--close", str(slot), "nice", "-n", "10", "--", *argv]


def acquire_build_slot(path: Path) -> int:
    if live_slot_path(str(path)):
        raise PolicyError("refusing a live build slot")
    return open_owned_lock(path)


def open_owned_lock(path: Path) -> int:
    """Open a lock file without following a symlink or locking a hardlink.

    Checks happen before flock. Callers that truncate or write must fstat again
    first. On failure the target is left unchanged.
    """

    if live_slot_path(str(path)):
        raise PolicyError("refusing a live build slot")
    if not path.is_absolute():
        raise PolicyError("lock path must be an absolute path")
    parts = path.parts
    if len(parts) < 2 or any(part in {"", ".", ".."} for part in parts[1:]):
        raise PolicyError("lock path must not contain . or ..")
    dir_fd = os.open("/", os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC | os.O_NOFOLLOW)
    os.set_inheritable(dir_fd, False)
    try:
        for name in parts[1:-1]:
            next_fd = _advance(dir_fd, name)
            os.close(dir_fd)
            dir_fd = next_fd
        _require_private_dir(dir_fd)
        return _open_exclusive_file(dir_fd, parts[-1])
    finally:
        os.close(dir_fd)


def release_build_slot(fd: int | None) -> None:
    """Unlock and close. `LOCK_UN` drops every duplicate, so a guardian must close instead."""

    if fd is None:
        return
    try:
        fcntl.flock(fd, fcntl.LOCK_UN)
    except OSError:
        pass
    os.close(fd)


def slot_metadata(fd: int, slot: Path, argv: list[str]) -> dict:
    flags = fcntl.fcntl(fd, fcntl.F_GETFD)
    return {
        "close_on_exec": bool(flags & fcntl.FD_CLOEXEC),
        "passed_to_child": False,
        "live_slot": False,
        "argv": equivalent_argv(slot, argv),
    }


def incident_record() -> dict:
    """Read-only inode metadata for the leaked slot. Does not lock, write, or signal."""

    rows = []
    for path in LIVE_SLOT_PATHS:
        rows.append(_slot_stat(path))
    return {
        "kind": "build-slot-leak",
        "summary": "plain flock let daemonized xclip keep slot1 via an inherited lock fd",
        "fixture": "decbar-rdwm unit fixture used DISPLAY=:0 with a private XDG_RUNTIME_DIR and the real HOME",
        "runtime_dir_alone_is_isolation": False,
        "retired_name": "slot1-retired-0200.lock",
        "fresh_slot_name": "slot1.lock",
        "slot2_restarted": False,
        "clipboard_read": False,
        "helpers_killed_by_this_lane": False,
        "coordinator_sigkill_is_not_a_compiler_failure": True,
        "remediation": (
            "hold the slot with flock --close or a CLOEXEC parent fd until the contained "
            "tree is verified dead; scrub host DISPLAY, HOME, and both buses"
        ),
        "full_acceptance": False,
        "slots": rows,
    }


def _advance(dir_fd: int, name: str) -> int:
    """Open the next directory. The caller closes dir_fd. A symlink raises before mkdir."""

    try:
        return _open_subdir(dir_fd, name)
    except FileNotFoundError:
        _mkdir_private(dir_fd, name)
        return _open_subdir(dir_fd, name)


def _open_subdir(dir_fd: int, name: str) -> int:
    try:
        fd = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC | os.O_NOFOLLOW, dir_fd=dir_fd)
    except FileNotFoundError:
        raise
    except OSError as exc:
        raise _path_error(exc) from exc
    os.set_inheritable(fd, False)
    return fd


def _mkdir_private(dir_fd: int, name: str) -> None:
    try:
        os.mkdir(name, 0o700, dir_fd=dir_fd)
    except FileExistsError:
        return
    os.chmod(name, 0o700, dir_fd=dir_fd, follow_symlinks=False)


def _require_private_dir(dir_fd: int) -> None:
    info = os.fstat(dir_fd)
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077:
        raise PolicyError("lock parent must be owned by this user and mode 0700")


def _open_exclusive_file(dir_fd: int, name: str) -> int:
    flags = os.O_CREAT | os.O_RDWR | os.O_CLOEXEC | os.O_NOFOLLOW
    try:
        fd = os.open(name, flags, 0o600, dir_fd=dir_fd)
    except OSError as exc:
        raise _path_error(exc) from exc
    os.set_inheritable(fd, False)
    try:
        _require_private_file(fd)
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as exc:
            raise PolicyError("build slot is busy") from exc
        _require_private_file(fd)
    except Exception:
        os.close(fd)
        raise
    return fd


def _require_private_file(fd: int) -> None:
    info = os.fstat(fd)
    if info.st_nlink != 1:
        raise PolicyError("refusing a hardlinked lock file")
    if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid():
        raise PolicyError("refusing a lock file that is not a private regular file")


def _path_error(exc: OSError) -> PolicyError:
    if exc.errno in {errno.ELOOP, errno.ENOTDIR}:
        return PolicyError("refusing a symlink in the lock path")
    return PolicyError(f"lock path could not be opened safely: {exc.strerror}")


def _slot_stat(path: str) -> dict:
    row = {"path": path, "locked_by_this_lane": False}
    try:
        info = os.stat(path, follow_symlinks=False)
    except FileNotFoundError:
        row["exists"] = False
        return row
    except OSError:
        row["exists"] = False
        row["error"] = "unreadable"
        return row
    row.update(exists=True, inode=info.st_ino, size=info.st_size, mtime_unix=info.st_mtime)
    return row
