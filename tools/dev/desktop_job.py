#!/usr/bin/env python3
"""Serialize non-interactive developer jobs on a desktop in active use (Linux)."""
import argparse
import fcntl
import hashlib
import math
import os
from pathlib import Path
import pwd
import shutil
import signal
import stat
import subprocess
import sys
import time


class Refused(Exception):
    pass


def default_slots():
    # Fixtures deliberately replace HOME/XDG_CACHE_HOME. That must not create an independent workload budget.
    account_home = Path(pwd.getpwuid(os.getuid()).pw_dir)
    return Path(os.environ.get('DECEBAL_BUILD_SLOTS', str(account_home / '.cache/decebal-build-slots')))


def directory(path):
    path.mkdir(mode=0o700, parents=True, exist_ok=True)
    st = path.lstat()
    if path.is_symlink() or not path.is_dir() or st.st_uid != os.getuid() or st.st_mode & 0o022:
        raise Refused('slot directory must be owned and not writable by other users')
    return path


def cargo_args(command):
    if Path(command[0]).name != 'cargo':
        return
    args = command[1:]
    if '--' in args:
        args = args[:args.index('--')]
    for i, arg in enumerate(args):
        value = None
        if arg in ('-j', '--jobs'):
            value = args[i + 1] if i + 1 < len(args) else ''
        elif arg.startswith('--jobs='):
            value = arg.split('=', 1)[1]
        elif arg.startswith('-j'):
            value = arg[2:]
        if value is not None and value != '1':
            raise Refused('desktop jobs permit only cargo -j1; omit --jobs or use 1')


def acquire(slots, deadline):
    """Take both old slots without holding one while waiting for the other."""
    while True:
        if (slots / 'paused').exists():
            raise Refused('developer jobs are paused; no command was started')
        held = []
        try:
            for name in ('slot1.lock', 'slot2.lock'):
                fd = os.open(slots / name, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
                held.append(fd)
                st = os.fstat(fd)
                if not stat.S_ISREG(st.st_mode) or st.st_uid != os.getuid() or st.st_mode & 0o022:
                    raise Refused('slot file must be owned and not writable by other users')
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            if (slots / 'paused').exists():
                raise Refused('developer jobs are paused; no command was started')
            return held
        except BlockingIOError:
            pass
        except BaseException:
            for fd in held:
                os.close(fd)
            raise
        for fd in held:
            os.close(fd)
        if time.monotonic() >= deadline:
            raise Refused('another developer job owns a slot; queue deadline expired without starting')
        time.sleep(0.05)


def signal_group(child, sig):
    # The direct child is deliberately not reaped until group cleanup: its PID cannot be reused.
    try:
        os.killpg(child.pid, sig)
    except ProcessLookupError:
        pass


def exited(child):
    return os.waitid(os.P_PID, child.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT) is not None


def run(command, slots, timeout):
    ionice = shutil.which('ionice')
    if not ionice:
        raise Refused('ionice is required; refusing to run without idle I/O priority')
    env = dict(os.environ, CARGO_BUILD_JOBS='1', RUST_TEST_THREADS='1')
    env.setdefault('CARGO_PROFILE_DEV_DEBUG', '0')
    env.setdefault('CARGO_PROFILE_TEST_DEBUG', '0')
    cache = Path(env.get('XDG_CACHE_HOME', str(Path.home() / '.cache')))
    key = hashlib.sha256(os.fsencode(Path.cwd())).hexdigest()[:12]
    env.setdefault('CARGO_TARGET_DIR', str(cache / 'decebal-targets' / key))
    cancel = []
    old = {sig: signal.getsignal(sig) for sig in (signal.SIGINT, signal.SIGTERM)}
    for sig in old:
        signal.signal(sig, lambda signum, frame: cancel.append(signum))
    child = None
    try:
        child = subprocess.Popen([ionice, '-c', '3', '--', *command], env=env,
                                 stdin=subprocess.DEVNULL, close_fds=True, start_new_session=True,
                                 preexec_fn=lambda: os.setpriority(os.PRIO_PROCESS, 0, 19))
        deadline = time.monotonic() + timeout
        reason = None
        while not exited(child):
            if cancel:
                reason = f'interrupted by signal {cancel[0]}'
                break
            if (slots / 'paused').exists():
                reason = 'developer jobs paused'
                break
            if time.monotonic() >= deadline:
                reason = 'execution deadline expired'
                break
            time.sleep(0.05)
        signal_group(child, signal.SIGTERM)
        if reason:
            grace = time.monotonic() + 2
            while not exited(child) and time.monotonic() < grace:
                time.sleep(0.05)
        # Clear remaining non-detached children before releasing either slot.
        signal_group(child, signal.SIGKILL)
        code = child.wait()
        if reason:
            print(f'desktop-job: {reason}; command group ended', file=sys.stderr)
            return 124 if reason == 'execution deadline expired' else 130
        return code if code >= 0 else 128 - code
    finally:
        if child is not None and child.returncode is None:
            signal_group(child, signal.SIGKILL)
            child.wait()
        for sig, handler in old.items():
            signal.signal(sig, handler)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--slots-dir', type=Path, default=default_slots())
    parser.add_argument('--queue-timeout', type=float, default=60)
    parser.add_argument('--timeout', type=float, default=1800)
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument('--pause', action='store_true')
    modes.add_argument('--resume', action='store_true')
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    held = []
    try:
        slots = directory(args.slots_dir)
        if args.pause:
            fd = os.open(slots / 'paused', os.O_WRONLY | os.O_CREAT | os.O_NOFOLLOW, 0o600)
            os.close(fd)
            return 0
        if args.resume:
            (slots / 'paused').unlink(missing_ok=True)
            return 0
        command = args.command[1:] if args.command[:1] == ['--'] else args.command
        if not command or not math.isfinite(args.timeout) or not math.isfinite(args.queue_timeout) or args.timeout <= 0 or args.queue_timeout < 0:
            raise Refused('provide a command and positive execution deadline')
        cargo_args(command)
        held = acquire(slots, time.monotonic() + args.queue_timeout)
        return run(command, slots, args.timeout)
    except (Refused, OSError) as error:
        print(f'desktop-job: {error}', file=sys.stderr)
        return 75
    finally:
        for fd in held:
            os.close(fd)


if __name__ == '__main__':
    raise SystemExit(main())
