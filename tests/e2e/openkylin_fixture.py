"""Private X fixture helpers shared by the openKylin native harnesses (no live display, no foreign socket is ever touched).

Rules (after the rollout X11 transport incident): a reserved display number >= 700, never `:0`, never an automatic probe; a number is used only when its
lock file, filesystem socket and abstract socket are all absent; after the start the process must be the Xvfb for exactly that number and hold the inode of
BOTH socket namespaces (`/tmp/.X11-unix/X<n>` and `@/tmp/.X11-unix/X<n>`). Nothing is ever unlinked.
"""
import os, pathlib, subprocess, time

FIRST_DISPLAY = 700


def unix_sockets():
    """name -> inode of every unix socket (abstract ones start with `@`)."""
    out = {}
    for line in pathlib.Path("/proc/net/unix").read_text().splitlines()[1:]:
        f = line.split()
        if len(f) >= 8: out[f[-1]] = f[6]
    return out


def abstract_sockets():
    return {n for n in unix_sockets() if n.startswith("@/tmp/.X11-unix/X")}


def free_display(taken):
    """A display number >= 700 with no lock, no filesystem socket and no abstract socket, not used earlier in this run."""
    ab = abstract_sockets()
    for n in range(FIRST_DISPLAY, FIRST_DISPLAY + 200):
        if n in taken: continue
        if pathlib.Path(f"/tmp/.X11-unix/X{n}").exists() or pathlib.Path(f"/tmp/.X{n}-lock").exists() or f"@/tmp/.X11-unix/X{n}" in ab: continue
        taken.add(n); return n
    raise RuntimeError("no free private display in the reserved range")


def held_inodes(pid):
    held = set()
    for fd in pathlib.Path(f"/proc/{pid}/fd").iterdir():
        try: t = os.readlink(fd)
        except OSError: continue
        if t.startswith("socket:["): held.add(t[8:-1])
    return held


def verify_owned(n, server):
    """Before any input: n >= 700, our Xvfb is alive, is the lock-file owner, the process is `Xvfb :n`, and it holds the inode of both socket namespaces."""
    assert n >= FIRST_DISPLAY, f"refusing display :{n}: outside the reserved private range"
    reason = "not ready"
    for _ in range(100):
        assert server.poll() is None, f"owned Xvfb for :{n} exited before it was ready"
        socks = unix_sockets(); fs = socks.get(f"/tmp/.X11-unix/X{n}"); ab = socks.get(f"@/tmp/.X11-unix/X{n}")
        try: owner = int(pathlib.Path(f"/tmp/.X{n}-lock").read_text().strip())
        except (OSError, ValueError): owner = None
        try: cmd = pathlib.Path(f"/proc/{server.pid}/cmdline").read_bytes().replace(b"\0", b" ").decode()
        except OSError: cmd = ""
        if owner == server.pid and fs and ab and cmd.startswith(f"Xvfb :{n} ") and {fs, ab} <= held_inodes(server.pid):
            return
        reason = f"lock owner {owner}, pid {server.pid}, fs inode {fs}, abstract inode {ab}"
        time.sleep(0.1)
    raise AssertionError(f":{n}: not proven to be our server ({reason}); stopping before any input")


def start_xvfb(n, geom, env, log=None):
    p = subprocess.Popen(["Xvfb", f":{n}", "-screen", "0", f"{geom}x24", "-nolisten", "tcp"], env=env, stdout=log or subprocess.DEVNULL, stderr=log or subprocess.DEVNULL)
    verify_owned(n, p)
    return p
