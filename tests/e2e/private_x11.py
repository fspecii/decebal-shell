"""Linux-only owned private Xvfb fixture. No automatic display allocation."""
import atexit
import os
from pathlib import Path
import random
import socket
import signal
import itertools
import subprocess
import time

_owned = set()


def close(process):
    """Reap exactly the child we started; never remove X server paths."""
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=2)
    _owned.discard(process)


def _cleanup():
    for process in tuple(_owned):
        close(process)


atexit.register(_cleanup)


def _terminated(signum, frame):
    _cleanup()
    raise SystemExit(128 + signum)



def available(number):
    if not 700 <= number <= 65535:
        return False
    path = f'/tmp/.X11-unix/X{number}'
    if os.path.lexists(path) or os.path.lexists(f'/tmp/.X{number}-lock'):
        return False
    with socket.socket(socket.AF_UNIX) as probe:
        try:
            probe.bind('\0' + path)
        except OSError:
            return False
    return True


def listeners_owned(process, number):
    """Require both listening transports to belong to the exact child PID."""
    if process.poll() is not None:
        return False
    try:
        inodes = set()
        for fd in Path(f'/proc/{process.pid}/fd').iterdir():
            try:
                target = os.readlink(fd)
            except FileNotFoundError:
                continue
            if target.startswith('socket:['):
                inodes.add(target[8:-1])
        names = {f'/tmp/.X11-unix/X{number}', f'@/tmp/.X11-unix/X{number}'}
        found = set()
        for line in Path('/proc/net/unix').read_text().splitlines()[1:]:
            fields = line.split()
            if len(fields) == 8 and fields[7] in names:
                # SO_ACCEPTCON flag, SOCK_STREAM, listening state.
                # Accepted client sockets can carry the same pathname. They do not
                # replace the listening endpoints and must not invalidate ownership.
                if fields[3:6] != ['00010000', '0001', '01']:
                    continue
                if fields[6] not in inodes:
                    return False
                found.add(fields[7])
        return found == names and process.poll() is None
    except (OSError, ProcessLookupError):
        return False


def start_private_xvfb(*, env, log, screen='1280x800x24', attempts=12,
                       timeout=5, candidates=None, executable='Xvfb', extra_args=()):
    """Return (owned Popen, verified DISPLAY); caller registers that exact child."""
    for key in ('DBUS_SESSION_BUS_ADDRESS', 'DBUS_SYSTEM_BUS_ADDRESS'):
        env.setdefault(key, 'unix:path=' + str(Path(env['HOME']) / 'private-x11-no-bus'))
    sequence = iter(candidates) if candidates is not None else (random.SystemRandom().randrange(20000, 50000) for _ in range(attempts))
    # Preserve custom caller handlers; default termination must reap our child.
    for signum in (signal.SIGTERM, signal.SIGINT):
        if signal.getsignal(signum) in (signal.SIG_DFL, signal.default_int_handler):
            signal.signal(signum, _terminated)
    for number in itertools.islice(sequence, attempts):
        if not available(number):
            continue
        clean_env = dict(env)
        for key in ('DISPLAY', 'WAYLAND_DISPLAY', 'XAUTHORITY'):
            clean_env.pop(key, None)
        process = subprocess.Popen([executable, f':{number}', '-screen', '0', screen,
                                    '-noreset', '-nolisten', 'tcp', *extra_args],
                                   env=clean_env, stdout=log, stderr=log,
                                   start_new_session=True)
        _owned.add(process)
        deadline = time.monotonic() + timeout
        try:
            while process.poll() is None and time.monotonic() < deadline:
                if listeners_owned(process, number):
                    return process, f':{number}'
                time.sleep(.02)
        except BaseException:
            close(process)
            raise
        close(process)
    raise RuntimeError('No verified owned private Xvfb became ready')
