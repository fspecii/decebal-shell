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


def source_target(command, env, cache, cwd=None):
    """Scope direct Cargo jobs even when a caller supplies a shared target base."""
    cwd = Path.cwd() if cwd is None else Path(cwd)
    source = cwd.resolve()
    command = list(command)
    if Path(command[0]).name != 'cargo':
        key = hashlib.sha256(os.fsencode(source)).hexdigest()[:16]
        env.setdefault('CARGO_TARGET_DIR', str(cache / 'decebal-targets' / key))
        return command
    args_end = command.index('--') if '--' in command else len(command)
    base = Path(env.get('CARGO_TARGET_DIR', str(cache / 'decebal-targets')))
    targets = []
    i = 1
    while i < args_end:
        arg = command[i]
        if arg in ('--manifest-path', '--target-dir'):
            if i + 1 >= args_end:
                raise Refused(f'{arg} requires a value')
            value = command[i + 1]
            if arg == '--manifest-path':
                source = (cwd / value).resolve().parent
            else:
                base = Path(value)
                targets.append((i + 1, False))
            i += 2
            continue
        if arg.startswith('--manifest-path='):
            source = (cwd / arg.split('=', 1)[1]).resolve().parent
        elif arg.startswith('--target-dir='):
            base = Path(arg.split('=', 1)[1])
            targets.append((i, True))
        i += 1
    # A manifest in another checkout must never inherit this checkout's root
    # product fingerprints. Dependency-only seeds may be placed in this leaf.
    key = hashlib.sha256(os.fsencode(source)).hexdigest()[:16]
    target = str((cwd / base).resolve() / 'desktop-sources' / key)
    env['CARGO_TARGET_DIR'] = target
    for index, joined in targets:
        command[index] = '--target-dir=' + target if joined else target
    return command


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


def io_full_total():
    """Microseconds where all non-idle tasks were stalled on I/O (Linux PSI)."""
    for line in Path('/proc/pressure/io').read_text().splitlines():
        fields = line.split()
        if fields and fields[0] == 'full':
            for field in fields[1:]:
                if field.startswith('total='):
                    try:
                        value = int(field[6:])
                    except ValueError:
                        break
                    if value >= 0:
                        return value
    raise Refused('Linux I/O pressure total is unavailable; no unguarded job started')


def pause_jobs(slots):
    fd = os.open(slots / 'paused', os.O_WRONLY | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    os.close(fd)


class Pressure:
    def __init__(self, limit):
        self.limit = limit
        self.total = io_full_total()
        self.at = time.monotonic()

    def check(self):
        now = time.monotonic()
        elapsed = now - self.at
        if elapsed < 1:
            return None
        total = io_full_total()
        percent = max(0, total - self.total) / (elapsed * 1_000_000) * 100
        self.at, self.total = now, total
        if percent > self.limit:
            return f'I/O full stalls {percent:.1f}% exceeded {self.limit:g}% budget'
        return None


def run(command, slots, timeout, max_io_stall=10, quiet_seconds=0):
    ionice = shutil.which('ionice')
    if not ionice:
        raise Refused('ionice is required; refusing to run without idle I/O priority')
    env = dict(os.environ, CARGO_BUILD_JOBS='1', RUST_TEST_THREADS='1')
    env.setdefault('CARGO_PROFILE_DEV_DEBUG', '0')
    env.setdefault('CARGO_PROFILE_TEST_DEBUG', '0')
    cache = Path(env.get('XDG_CACHE_HOME', str(Path.home() / '.cache')))
    command = source_target(command, env, cache)
    pressure = Pressure(max_io_stall)
    cancel = []
    old = {sig: signal.getsignal(sig) for sig in (signal.SIGINT, signal.SIGTERM)}
    for sig in old:
        signal.signal(sig, lambda signum, frame: cancel.append(signum))
    child = None
    try:
        # Measure before creating any native fixture or detached compositor.
        # Refuse rather than continuously retrying on a busy desktop.
        admission_deadline = time.monotonic() + quiet_seconds
        while time.monotonic() < admission_deadline:
            if cancel:
                raise Refused(f'interrupted before command start by signal {cancel[0]}')
            if (slots / 'paused').exists():
                raise Refused('developer jobs paused before command start')
            pressure_reason = pressure.check()
            if pressure_reason:
                pause_jobs(slots)
                raise Refused(f'{pressure_reason}; admission refused, no command started')
            time.sleep(min(.05, max(0, admission_deadline - time.monotonic())))
        if quiet_seconds:
            pressure_reason = pressure.check()
            if pressure_reason:
                pause_jobs(slots)
                raise Refused(f'{pressure_reason}; admission refused, no command started')
            pressure = Pressure(max_io_stall)
        if cancel or (slots / 'paused').exists():
            raise Refused('developer jobs paused or interrupted before command start')
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
            pressure_reason = pressure.check()
            if pressure_reason:
                pause_jobs(slots)
                reason = pressure_reason
                break
            if time.monotonic() >= deadline:
                reason = 'execution deadline expired'
                break
            time.sleep(0.05)
        signal_group(child, signal.SIGTERM)
        if reason:
            # Native fixtures may need to stop a detached private compositor,
            # flush their evidence and reap owned helpers. Keep both slots
            # held during bounded cleanup instead of killing them after 2s.
            grace = time.monotonic() + 10
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
    parser.add_argument('--max-io-stall', type=float, default=10, help='pause jobs above this full I/O stall percentage over a measured interval of at least one second (default: 10)')
    parser.add_argument('--quiet-seconds', type=float, default=0, help='measure I/O before starting; refuse and pause if over budget (0 disables, otherwise 1..30 seconds)')
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument('--pause', action='store_true')
    modes.add_argument('--resume', action='store_true')
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    held = []
    try:
        slots = directory(args.slots_dir)
        if args.pause:
            pause_jobs(slots)
            return 0
        if args.resume:
            (slots / 'paused').unlink(missing_ok=True)
            return 0
        command = args.command[1:] if args.command[:1] == ['--'] else args.command
        if not command or not math.isfinite(args.timeout) or not math.isfinite(args.queue_timeout) or args.timeout <= 0 or args.queue_timeout < 0:
            raise Refused('provide a command and positive execution deadline')
        if not math.isfinite(args.max_io_stall) or not 0 < args.max_io_stall <= 100:
            raise Refused('I/O stall budget must be finite and in (0, 100]')
        if not math.isfinite(args.quiet_seconds) or (args.quiet_seconds != 0 and not 1 <= args.quiet_seconds <= 30):
            raise Refused('quiet observation must be 0 or between 1 and 30 seconds')
        cargo_args(command)
        held = acquire(slots, time.monotonic() + args.queue_timeout)
        return run(command, slots, args.timeout, args.max_io_stall, args.quiet_seconds)
    except (Refused, OSError) as error:
        print(f'desktop-job: {error}', file=sys.stderr)
        return 75
    finally:
        for fd in held:
            os.close(fd)


if __name__ == '__main__':
    raise SystemExit(main())
