"""Private Xvfb transport rules shared by the GNOME native harnesses (imported by the sibling scripts; not a test itself).

Rules (root, 2026-10-31): the live display :0 may lack its filesystem socket while keeping its abstract one, so a fixture must never be able to
land on, or be confused with, a live server.
* Displays are explicitly reserved HIGH numbers (>= 700); there is no `-displayfd` auto-probe (it starts at :0).
* A number is taken only when neither socket namespace (filesystem `/tmp/.X11-unix/Xn`, abstract `@/tmp/.X11-unix/Xn`) nor the lock file mentions it.
* Before any client connects or any input is sent, `verify_owned` proves the server is the one this script started: still running, lock file holds its
  pid, and both the filesystem and the abstract socket exist and belong to that pid's file descriptors.
* Nothing foreign is ever unlinked or signalled; only the `Popen` this script created is stopped (`stop_owned`).
"""
from pathlib import Path
import os
import re
import signal
import time

FIRST, LAST = 700, 799
SOCKET_DIR = Path("/tmp/.X11-unix")


def _unix_sockets():
    """{path: inode} for every unix socket of the kernel table (abstract paths start with '@')."""
    out = {}
    for line in Path("/proc/net/unix").read_text().splitlines()[1:]:
        parts = line.split()
        if len(parts) >= 8:
            out[parts[7]] = int(parts[6])
    return out


def _taken(n):
    return (SOCKET_DIR / f"X{n}").exists() or Path(f"/tmp/.X{n}-lock").exists() or any(p.endswith(f"/tmp/.X11-unix/X{n}") for p in _unix_sockets())


def reserve_display(skip=()):
    """The first free high display number, as ':N'. `skip` lists numbers already handed out in this run."""
    for n in range(FIRST, LAST + 1):
        if n not in skip and not _taken(n):
            return f":{n}"
    raise RuntimeError(f"no free display in :{FIRST}..:{LAST}")


def _number(display):
    m = re.fullmatch(r":(\d+)", display)
    if not m or int(m.group(1)) < FIRST:
        raise RuntimeError(f"refusing display {display!r}: private fixtures use :{FIRST} and above, never :0")
    return int(m.group(1))


def verify_owned(server, display):
    """Raise unless `server` (the Popen this script started) really owns `display` in both socket namespaces. Call before any client or input.
    Returns the proof: {display, pid, lock_pid, fs_inode, abstract_inode} (the inodes are the kernel's, held by the server's own descriptors)."""
    n = _number(display)
    deadline = time.time() + 20
    problems = []
    while time.time() < deadline:
        problems = []
        if server.poll() is not None:
            raise RuntimeError(f"Xvfb {display} exited with {server.returncode} (a foreign server or lock may hold the number; nothing was removed)")
        try:
            lock_pid = int(Path(f"/tmp/.X{n}-lock").read_text().strip())
        except (OSError, ValueError):
            lock_pid = None
        if lock_pid != server.pid:
            problems.append(f"lock file holds pid {lock_pid}, not {server.pid}")
        owned = set()
        try:
            for fd in Path(f"/proc/{server.pid}/fd").iterdir():
                m = re.fullmatch(r"socket:\[(\d+)\]", os.readlink(fd))
                if m:
                    owned.add(int(m.group(1)))
        except OSError as e:
            problems.append(f"cannot read the server's descriptors: {e}")
        socks = _unix_sockets()
        fs, abstract = f"/tmp/.X11-unix/X{n}", f"@/tmp/.X11-unix/X{n}"
        if not (SOCKET_DIR / f"X{n}").is_socket():
            problems.append("filesystem socket missing")
        if socks.get(fs) not in owned:
            problems.append("filesystem socket is not held by the server")
        if socks.get(abstract) not in owned:
            problems.append("abstract socket missing or not held by the server")
        if not problems:
            return {"display": display, "pid": server.pid, "lock_pid": lock_pid, "fs_inode": socks[fs], "abstract_inode": socks[abstract]}
        time.sleep(0.2)
    raise RuntimeError(f"Xvfb {display} is not a verified private fixture: {'; '.join(problems)}")


def stop_owned(server):
    """Stop only the server this script started; never touches any other process, socket or lock."""
    if server.poll() is None:
        server.send_signal(signal.SIGTERM)
        try:
            server.wait(timeout=10)
        except Exception:
            server.kill()
            server.wait()
