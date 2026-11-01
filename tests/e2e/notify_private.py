"""Private, owned X11 + D-Bus + process fixture shared by the decnotify native regressions.

Used by notify-history.py and notify-theme.py. Linux only. Nothing here builds, installs or touches a live session:

* The environment is built from scratch (nothing is inherited): private HOME and XDG tree, a dead system bus, no
  session, backend, system-theme or apply paths, and no DISPLAY/WAYLAND_DISPLAY until the owned Xvfb exists.
* The Xvfb comes from private_x11.start_private_xvfb on a display >= 700 that was verified unoccupied. It is never
  adopted from, or removed from, an existing display or socket. Ownership of the listening sockets is re-proven
  immediately before every xdotool/xprop/import call (Session.run).
* The private session bus is a fixed configuration without service activation. Its socket is checked with kernel
  peer credentials (SO_PEERCRED) before any client connects.
* Every child is recorded in a pidfd-owned ledger. Signals go through the pidfd (never a bare PID) and, while the
  group leader is still unreaped, to its own process group. Cleanup signals all children together, waits a bounded
  time, escalates, reaps everything, keeps going after an individual error and records anything that needed force
  as a failure.
* SIGTERM/SIGINT/SIGHUP raise Interrupted immediately so `finally`/__exit__ cleanup runs; further signals during
  cleanup are deferred until cleanup is done. Failed or interrupted runs keep the private tree and fresh evidence.

Diagnostics never include the environment; evidence is only logs, screenshots and debug JSON of synthetic content.
"""
import contextlib
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import stat
import struct
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, str(Path(__file__).resolve().parent))
import private_x11

MIN_DISPLAY = 700
MAX_DISPLAY = 65535
SOCKET_PATH_LIMIT = 100  # sun_path is 108 bytes
X_TOOLS = ('xdotool', 'xprop', 'import')
RETRYABLE = (OSError, ValueError, KeyError, subprocess.CalledProcessError, subprocess.TimeoutExpired)

BUS_CONFIG = """<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig><type>session</type><listen>unix:path={path}</listen><auth>EXTERNAL</auth>
<policy context="default"><allow send_destination="*" eavesdrop="true"/><allow eavesdrop="true"/><allow own="*"/></policy>
</busconfig>
"""

# (variable, directory below the private root); the directories are created 0700.
PRIVATE_DIRS = (('HOME', 'home'), ('XDG_CONFIG_HOME', 'cfg'), ('XDG_CONFIG_DIRS', 'noconfig'),
                ('XDG_RUNTIME_DIR', 'run'), ('XDG_STATE_HOME', 'state'), ('XDG_DATA_HOME', 'data'),
                ('XDG_DATA_DIRS', 'nodata'), ('XDG_CACHE_HOME', 'cache'), ('TMPDIR', 'tmp'),
                ('DECEBAL_THEMES', 'themes'), ('DECEBAL_SYSTEM_THEMES', 'system-themes'),
                ('DECEBAL_SYSTEM_STATE', 'system-state'))


class Skip(Exception):
    """A prerequisite is missing; nothing was started."""


class Usage(Exception):
    """The caller supplied an invalid or missing explicit setting."""


class Interrupted(BaseException):
    def __init__(self, signum):
        super().__init__(signum)
        self.signum = signum


class OwnershipLost(RuntimeError):
    """The private X server is no longer provably ours. Deliberately not retryable by wait()."""


class CleanupFailed(AssertionError):
    pass


class SignalGuard:
    """Turn termination signals into Interrupted, except while a critical region defers them."""
    SIGNALS = (signal.SIGTERM, signal.SIGINT, signal.SIGHUP)

    def __init__(self):
        self.depth = 0
        self.pending = None
        self.received = None

    def checkpoint(self):
        # C extension callbacks (notably dbus-python) may catch signal-handler
        # exceptions. Retain cancellation until normal Python control resumes.
        if self.received and not self.depth:
            raise Interrupted(self.received)

    def handler(self, signum, frame):
        self.received = self.received or signum
        if self.depth:
            self.pending = self.pending or signum
            return
        raise Interrupted(signum)

    def install(self):
        for signum in self.SIGNALS:
            signal.signal(signum, self.handler)

    @contextlib.contextmanager
    def deferred(self):
        self.depth += 1
        try:
            yield
        finally:
            self.depth -= 1
            if not self.depth and self.pending:
                signum, self.pending = self.pending, None
                raise Interrupted(signum)


GUARD = SignalGuard()


def run_main(main):
    """Run main() so that termination signals clean up first and exit with 128+signal."""
    GUARD.install()
    try:
        code = main()
        GUARD.checkpoint()
    except Interrupted as stop:
        print(f'Interrupted by signal {stop.signum}; cleanup finished, private evidence retained', file=sys.stderr, flush=True)
        code = 128 + stop.signum
    raise SystemExit(code or 0)


# ------------------------------------------------------------------------------ explicit inputs

def explicit_directory(name):
    """A required absolute directory from the environment. There is no default, so no live tree is used silently."""
    value = os.environ.get(name, '')
    if not value or not os.path.isabs(value):
        raise Usage(f'{name} must be set to an absolute path (explicit immutable binaries are required; nothing is built or defaulted)')
    return Path(value)


def parse_display(value):
    """None for 'allocate'; otherwise the display number of an explicit ':N' with N in the private namespace."""
    if value in (None, ''):
        return None
    if not value.startswith(':') or not value[1:].isdigit() or not MIN_DISPLAY <= int(value[1:]) <= MAX_DISPLAY:
        raise Usage(f'E2E_DISPLAY must look like :{MIN_DISPLAY}..:{MAX_DISPLAY} (the private display namespace) or be unset')
    return int(value[1:])


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def private_environment(root, bin_dirs=(), decebal_config='cfg/decebal-shell', extra=None):
    """A complete environment for private children; nothing comes from os.environ."""
    root = Path(root)
    env = {'PATH': os.pathsep.join([*(str(path) for path in bin_dirs), '/usr/bin', '/bin']), 'LANG': 'C.UTF-8'}
    for name, child in PRIVATE_DIRS:
        path = root / child
        path.mkdir(mode=0o700, parents=True, exist_ok=True)
        env[name] = str(path)
    os.chmod(env['XDG_RUNTIME_DIR'], 0o700)
    config = root / decebal_config
    config.mkdir(mode=0o700, parents=True, exist_ok=True)
    env['DECEBAL_CONFIG'] = str(config)
    env['DECEBAL_SYSTEM_AUTOMATIC'] = '0'
    # Dead buses: nothing may reach a session or system bus other than the private one this fixture starts.
    env['DBUS_SESSION_BUS_ADDRESS'] = 'unix:path=' + str(Path(env['XDG_RUNTIME_DIR']) / 'absent-session-bus')
    env['DBUS_SYSTEM_BUS_ADDRESS'] = 'unix:path=' + str(Path(env['XDG_RUNTIME_DIR']) / 'absent-system-bus')
    env.update(extra or {})
    return env


def socket_owned(proc, path):
    """Kernel peer credentials of the listening socket must name this exact child and our uid."""
    if proc.poll() is not None:
        return False
    try:
        info = os.lstat(path)
        if not stat.S_ISSOCK(info.st_mode) or info.st_uid != os.getuid():
            return False
        with socket.socket(socket.AF_UNIX) as connection:
            connection.settimeout(0.5)
            connection.connect(str(path))
            pid, uid, _ = struct.unpack('3i', connection.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, struct.calcsize('3i')))
        return pid == proc.pid and uid == os.getuid() and proc.poll() is None
    except (OSError, ValueError):
        return False


def wait(check, reason, seconds=10, catch=RETRYABLE, interval=.04):
    end = time.monotonic() + seconds
    last = None
    while time.monotonic() < end:
        GUARD.checkpoint()
        try:
            result = check()
            GUARD.checkpoint()
            if result:
                return result
        except catch as error:
            last = error
        time.sleep(interval)
    raise AssertionError(reason + (': ' + str(last) if last else ''))


def group_members(pgid, proc_root='/proc'):
    """Live (non-zombie) pids other than the leader that still belong to process group `pgid`."""
    found = []
    try:
        names = os.listdir(proc_root)
    except OSError:
        return found
    for name in names:
        if not name.isdigit() or int(name) == pgid:
            continue
        try:
            data = Path(proc_root, name, 'stat').read_text()
            fields = data[data.rindex(')') + 2:].split()
            if fields[0] != 'Z' and int(fields[2]) == pgid:
                found.append(int(name))
        except (OSError, ValueError, IndexError):
            continue
    return found


# ------------------------------------------------------------------------------ pidfd-owned child ledger

class Child:
    def __init__(self, label, proc, fd):
        self.label, self.proc, self.fd = label, proc, fd
        self.closed = False
        self.forced = False


class Ledger:
    def __init__(self):
        self.children = []
        self.failures = []
        self.forced = []

    def start(self, argv, label, *, env, stdout):
        with GUARD.deferred():
            proc = subprocess.Popen([str(part) for part in argv], env=env, stdin=subprocess.DEVNULL, stdout=stdout,
                                    stderr=subprocess.STDOUT, start_new_session=True)
            return self.adopt(proc, label)

    def adopt(self, proc, label):
        """Own a child we (or an owning helper) started. The pidfd is opened before checking it is still unreaped."""
        with GUARD.deferred():
            fd = os.pidfd_open(proc.pid)
            if proc.returncode is not None:
                os.close(fd)
                raise RuntimeError(f'{label} exited before it could be owned')
            child = Child(label, proc, fd)
            self.children.append(child)
            return child

    def alive(self, child):
        return not child.closed and not self._exited(child)

    def _exited(self, child):
        if child.proc.returncode is not None:
            return True
        try:
            return os.waitid(os.P_PIDFD, child.fd, os.WEXITED | os.WNOHANG | os.WNOWAIT) is not None
        except ChildProcessError:
            return True

    def _signal(self, child, signum):
        name = signal.Signals(signum).name
        try:
            signal.pidfd_send_signal(child.fd, signum)
        except ProcessLookupError:
            pass
        except OSError as error:
            self.failures.append(f'{child.label}: {name} via pidfd failed: {error.strerror}')
        # While the leader is unreaped its pid/pgid cannot be reused, so the group signal cannot hit a stranger.
        pid = child.proc.pid
        if child.proc.returncode is None and pid != os.getpgrp():
            try:
                if os.getpgid(pid) == pid:
                    os.killpg(pid, signum)
            except ProcessLookupError:
                pass
            except OSError as error:
                self.failures.append(f'{child.label}: {name} to process group failed: {error.strerror}')

    def stop(self, children, grace=8.0, kill_wait=3.0):
        """SIGTERM every child together, wait `grace` in total, SIGKILL the rest, reap all. Forced kills are failures."""
        live = [child for child in children if not child.closed]
        for child in live:
            self._signal(child, signal.SIGTERM)
        deadline = time.monotonic() + grace
        while live and time.monotonic() < deadline and not all(self._exited(child) for child in live):
            time.sleep(.02)
        for child in live:
            if not self._exited(child):
                child.forced = True
                self.forced.append(child.label)
                self.failures.append(f'{child.label}: ignored SIGTERM for {grace:g}s and was killed')
                self._signal(child, signal.SIGKILL)
        self._reap(live, kill_wait)

    def stop_all(self, grace=8.0, kill_wait=3.0):
        self.stop(list(reversed(self.children)), grace, kill_wait)

    def _reap(self, children, kill_wait):
        deadline = time.monotonic() + kill_wait
        for child in children:
            try:
                while not self._exited(child) and time.monotonic() < deadline:
                    time.sleep(.02)
                if not self._exited(child):
                    self.failures.append(f'{child.label}: did not exit after SIGKILL')
                else:
                    self._sweep_group(child, deadline)
                try:
                    child.proc.wait(timeout=max(.1, deadline - time.monotonic()))
                except subprocess.TimeoutExpired:
                    self.failures.append(f'{child.label}: could not be reaped')
            except Exception as error:
                self.failures.append(f'{child.label}: cleanup error {type(error).__name__}')
            finally:
                with contextlib.suppress(OSError):
                    os.close(child.fd)
                child.closed = True

    def _sweep_group(self, child, deadline):
        pid = child.proc.pid
        if child.proc.returncode is not None or not group_members(pid):
            return
        self.failures.append(f'{child.label}: process-group members outlived the leader and were killed')
        with contextlib.suppress(OSError):
            os.killpg(pid, signal.SIGKILL)
        while group_members(pid) and time.monotonic() < deadline:
            time.sleep(.02)
        if group_members(pid):
            self.failures.append(f'{child.label}: process-group members survived SIGKILL')

    def summary(self):
        return [{'label': child.label, 'returncode': child.proc.returncode, 'forced': child.forced} for child in self.children]


# ------------------------------------------------------------------------------ session

class Session:
    """Everything one native run owns: prerequisites, private tree, evidence, bus, X server and children."""

    def __init__(self, prefix, *, tools, binaries, evidence_env, decebal_config, path_dirs=(), extra_env=None,
                 require_non_root=False, keep_env='KEEP_TMP'):
        missing = [name for name in tools if not shutil.which(name)]
        if missing:
            raise Skip('missing ' + ', '.join(missing))
        self.tools = {name: shutil.which(name) for name in tools}
        self.binaries, self.hashes = {}, {}
        for name, path in binaries.items():
            path = Path(path)
            if path.is_symlink() or not path.is_file() or not os.access(path, os.X_OK):
                raise Skip('missing ' + str(path))
            self.binaries[name] = path
            self.hashes[name] = sha256(path)
        if require_non_root and os.getuid() == 0:
            raise Skip('actual permission-denied regression requires non-root uid')
        self.requested = parse_display(os.environ.get('E2E_DISPLAY'))
        if self.requested is not None and not private_x11.available(self.requested):
            raise Skip(f'occupied display :{self.requested} (never adopted or removed; unset E2E_DISPLAY to allocate)')
        self.explicit_evidence = None
        if os.environ.get(evidence_env):
            self.explicit_evidence = Path(os.environ[evidence_env]).absolute()
            if os.path.lexists(self.explicit_evidence):
                raise Usage(f'{evidence_env} must name a new directory; {self.explicit_evidence} exists')
        self.prefix, self.path_dirs, self.decebal_config = prefix, tuple(path_dirs), decebal_config
        self.extra_env, self.keep = dict(extra_env or {}), bool(os.environ.get(keep_env))
        self.ledger = Ledger()
        self.hooks, self.streams, self.extra_evidence = [], [], []
        self.root = self.evidence = self.xvfb = self.display = self.number = None
        self.env = {}

    # -- lifecycle
    def __enter__(self):
        try:
            with GUARD.deferred():
                self.root = Path(tempfile.mkdtemp(prefix=self.prefix + '.'))
            self.env = private_environment(self.root, self.path_dirs, self.decebal_config, self.extra_env)
            evidence = self.explicit_evidence or self.root / 'evidence'
            evidence.mkdir(mode=0o700, parents=True)
            self.evidence = evidence
        except BaseException as error:
            self._teardown(error)
            raise
        return self

    def __exit__(self, kind, error, trace):
        self._teardown(error)
        if self.ledger.failures and error is None:
            raise CleanupFailed('; '.join(self.ledger.failures))
        return False

    def before_cleanup(self, hook):
        """Best-effort callbacks (permission repair, closing clients) run first at teardown, newest first."""
        self.hooks.append(hook)

    def _teardown(self, error):
        if self.root is None:
            return
        with GUARD.deferred():
            for hook in reversed(self.hooks):
                try:
                    hook()
                except Exception as problem:
                    self.ledger.failures.append(f'cleanup hook failed: {type(problem).__name__}')
            self.ledger.stop_all()
            if self.xvfb is not None:
                with contextlib.suppress(Exception):
                    private_x11.close(self.xvfb.proc)
            for name, path in self.binaries.items():
                try:
                    if sha256(path) != self.hashes[name]:
                        self.ledger.failures.append(f'{name} binary changed during the run')
                except OSError:
                    self.ledger.failures.append(f'{name} binary became unreadable')
            for stream in self.streams:
                with contextlib.suppress(OSError):
                    stream.close()
            repair_permissions(self.root)
            self._write_evidence(error)
            clean = error is None and not self.ledger.failures and not GUARD.received
            if clean and not self.keep:
                try:
                    if self.root.is_symlink() or not self.root.name.startswith(self.prefix + '.'):
                        raise OSError('refusing to remove an unexpected path')
                    shutil.rmtree(self.root)
                except OSError as problem:
                    self.ledger.failures.append(f'could not remove private tree: {type(problem).__name__}')
                    clean = False
            if not clean:
                print('Private run resources and evidence retained at ' + str(self.root), flush=True)
                for failure in self.ledger.failures:
                    print('CLEANUP FAILURE: ' + failure, file=sys.stderr, flush=True)

    def _write_evidence(self, error):
        if self.evidence is None:
            return
        try:
            inside = self.evidence == self.root or self.root in self.evidence.parents
            if not inside:
                for path in sorted(self.root.glob('*.log')) + [Path(p) for p in self.extra_evidence]:
                    if path.is_file() and not path.is_symlink():
                        shutil.copy2(path, self.evidence / path.name)
            outcome = {'display': self.display, 'binaries_sha256': self.hashes, 'children': self.ledger.summary(),
                       'forced': self.ledger.forced, 'cleanup_failures': self.ledger.failures,
                       'interrupted_signal': (error.signum if isinstance(error, Interrupted) else GUARD.received),
                       'error': type(error).__name__ if error is not None else None}
            (self.evidence / 'cleanup.json').write_text(json.dumps(outcome, indent=2) + '\n')
        except OSError as problem:
            self.ledger.failures.append(f'could not write evidence: {type(problem).__name__}')

    # -- resources
    def open_log(self, label):
        stream = (self.root / (label + '.log')).open('w')
        self.streams.append(stream)
        return stream

    def start(self, argv, label):
        GUARD.checkpoint()
        return self.ledger.start(argv, label, env=self.env, stdout=self.open_log(label))

    def stop(self, child, grace=4.0):
        self.ledger.stop([child], grace, 2.0)

    def alive(self, child):
        return self.ledger.alive(child)

    def start_bus(self, timeout=8):
        path = self.root / 'bus'
        if len(os.fsencode(path)) > SOCKET_PATH_LIMIT:
            raise Usage('private tree path is too long for a Unix socket; use a shorter TMPDIR')
        config = self.root / 'bus.conf'
        config.write_text(BUS_CONFIG.format(path=path))
        child = self.start([self.tools['dbus-daemon'], '--nofork', '--config-file', config], 'bus')
        wait(lambda: socket_owned(child.proc, path), 'private bus socket not owned by its daemon', timeout)
        self.env['DBUS_SESSION_BUS_ADDRESS'] = 'unix:path=' + str(path)
        return self.env['DBUS_SESSION_BUS_ADDRESS']

    def start_xvfb(self, screen='1280x900x24'):
        log = self.open_log('xvfb')
        env = dict(self.env)
        for key in ('DISPLAY', 'WAYLAND_DISPLAY', 'XAUTHORITY'):
            env.pop(key, None)
        explicit = self.requested is not None
        proc, display = private_x11.start_private_xvfb(
            env=env, log=log, screen=screen, executable=self.tools['Xvfb'],
            candidates=[self.requested] if explicit else None, attempts=1 if explicit else 12)
        try:
            self.xvfb = self.ledger.adopt(proc, 'xvfb')
        except BaseException:
            private_x11.close(proc)
            raise
        self.display, self.number = display, int(display[1:])
        self.env['DISPLAY'] = display
        self.require_display()
        return display

    def x_owned(self):
        return self.xvfb is not None and private_x11.listeners_owned(self.xvfb.proc, self.number)

    def require_display(self):
        if not self.x_owned():
            raise OwnershipLost('private X display is not provably owned by this run')

    def run(self, *args, timeout=10):
        """Run a helper tool to completion with the private environment. X tools first re-prove display ownership."""
        GUARD.checkpoint()
        argv = [str(part) for part in args]
        if argv[0] in X_TOOLS:
            self.require_display()
        if argv[0] in self.tools:
            argv[0] = self.tools[argv[0]]
        return subprocess.run(argv, env=self.env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                              stderr=subprocess.DEVNULL, text=True, check=True, timeout=timeout).stdout.strip()


def repair_permissions(root):
    """Make every directory below `root` removable/readable again without following links."""
    stack = [Path(root)]
    while stack:
        path = stack.pop()
        try:
            if path.is_symlink():
                continue
            os.chmod(path, 0o700)
            with os.scandir(path) as entries:
                stack.extend(Path(entry.path) for entry in entries if entry.is_dir(follow_symlinks=False))
        except OSError:
            continue
