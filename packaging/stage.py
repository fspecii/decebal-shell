#!/usr/bin/env python3
"""Build provenance and staged install for the decebal shell (used by the Makefile and the PKGBUILD).

  build     run `cargo build --release --locked --message-format=json-render-diagnostics [ARGS]` and record the exact
            executables cargo reported for this invocation (wherever CARGO_TARGET_DIR / cargo config puts them) with
            their sha256 in BUILD_MANIFEST. A stale ./target/release is never consulted.
  install   copy exactly those executables (digests re-checked) plus the desktop entries, user units, autostart entries,
            D-Bus service file, theme packages (fonts, wallpapers, widgets, licences), README and LICENSE into
            $DESTDIR$PREFIX, and write $PREFIX/share/decebal/install-inventory.json.
  uninstall remove what the inventory lists (files changed since install are kept and reported).
  inventory print the planned install file list (no writes).

Never runs systemctl, daemon-reload, update-desktop-database or anything in $HOME; never enables a unit; writes nothing
outside $DESTDIR$PREFIX and $DESTDIR/etc/xdg/autostart. Settings come from arguments or the PREFIX, DESTDIR,
BUILD_MANIFEST and DECEBAL_CARGO environment variables, so make recipes never interpolate them into shell text.
"""
from __future__ import annotations

import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import shlex
import stat
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
BINS = ('decwall decdock decbar dechaven declaunch decspot decswitch decexpose dechud declock decdash decmon decsession '
        'decutil decsettings decthemectl decnotify decwm decclip dechot decwidgets decnotes dectimer').split()
# Programs that did not exist under the old macshell name get no mac* compatibility wrapper.
NO_COMPAT = {'declock', 'decmon', 'decwall', 'decnotes', 'dectimer'}
UNITS = ('timer', 'live-wallpaper', 'widgets', 'dock', 'bar', 'notify', 'clip', 'hot', 'wallpaper-colors')
APPLICATIONS = ('dechaven', 'decsettings', 'decnotes', 'dectimer', 'decmon')
PREFIX_RE = re.compile(r'/[A-Za-z0-9_./+\-]*')
SCHEMA_BUILD = 'decebal-build-manifest/1'
SCHEMA_INVENTORY = 'decebal-install-inventory/1'
INVENTORY_REL = 'share/decebal/install-inventory.json'
MAX_FILE = 256 << 20


class StageError(RuntimeError):
    pass


def sha256_file(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK)
    try:
        st = os.fstat(fd)
        if not stat.S_ISREG(st.st_mode) or st.st_size > MAX_FILE:
            raise StageError(f'{path}: not a regular file within {MAX_FILE} bytes')
        h = hashlib.sha256()
        while b := os.read(fd, 1 << 20):
            h.update(b)
        return h.hexdigest(), st
    finally:
        os.close(fd)


def check_prefix(prefix):
    if not PREFIX_RE.fullmatch(prefix):
        raise StageError('PREFIX must be absolute and contain only letters, digits, /, _, ., + or - '
                         '(it is written into unit and D-Bus files; no whitespace or shell/systemd metacharacters)')
    if prefix.rstrip('/') == '':
        raise StageError('PREFIX=/ is not supported (use /usr or /usr/local)')
    if any(p in ('.', '..') for p in prefix.split('/')) or '//' in prefix:
        raise StageError('PREFIX must be absolute without ., .. or empty path components')
    return prefix.rstrip('/')


def check_destdir(destdir):
    if destdir and (not destdir.startswith('/') or '\0' in destdir or '\n' in destdir
                    or any(p in ('.', '..') for p in destdir.split('/'))):
        raise StageError('DESTDIR must be empty or an absolute path without . or .. components')
    return destdir.rstrip('/')


# ------------------------------------------------------------------ build

def build(cargo, manifest, cargo_args, source=ROOT):
    argv = [*cargo, 'build', '--release', '--locked', '--message-format=json-render-diagnostics', *cargo_args]
    proc = subprocess.run(argv, cwd=source, stdout=subprocess.PIPE, text=True)
    if proc.returncode != 0:
        raise StageError(f'cargo build failed with exit {proc.returncode}')
    want_manifest = str((Path(source) / 'Cargo.toml').resolve())
    found = {}
    for line in proc.stdout.splitlines():
        try:
            msg = json.loads(line)
        except ValueError:
            continue
        t = msg.get('target') or {}
        if msg.get('reason') != 'compiler-artifact' or 'bin' not in t.get('kind', []) or t.get('name') not in BINS:
            continue
        if msg.get('manifest_path') != want_manifest or not msg.get('executable'):
            continue
        if t['name'] in found:
            raise StageError(f'cargo reported {t["name"]} twice')
        found[t['name']] = msg['executable']
    missing = sorted(set(BINS) - set(found))
    if missing:
        raise StageError(f'cargo did not report these binaries: {missing}')
    bins = {}
    for name in BINS:
        digest, st = sha256_file(found[name])
        bins[name] = {'path': found[name], 'sha256': digest, 'size': st.st_size}
    data = {'schema': SCHEMA_BUILD, 'created_utc': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
            'cargo_args': argv[1:], 'source_manifest': want_manifest,
            'target_release_dirs': sorted({str(Path(b['path']).parent) for b in bins.values()}), 'binaries': bins}
    head = subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=source, capture_output=True, text=True)
    if head.returncode == 0:
        dirty = subprocess.run(['git', 'status', '--porcelain', '--untracked-files=no'], cwd=source, capture_output=True, text=True)
        data['source_commit'] = head.stdout.strip()
        data['source_dirty'] = bool(dirty.stdout.strip()) if dirty.returncode == 0 else None
    manifest = Path(manifest)
    manifest.parent.mkdir(parents=True, exist_ok=True)
    tmp = manifest.with_name(f'.{manifest.name}.{secrets.token_hex(4)}')
    tmp.write_text(json.dumps(data, indent=2, sort_keys=True) + '\n')
    os.replace(tmp, manifest)
    return data


def load_build(manifest):
    try:
        data = json.loads(Path(manifest).read_text())
    except FileNotFoundError:
        raise StageError(f'{manifest} not found: run `make` (or `packaging/stage.py build`) first')
    if data.get('schema') != SCHEMA_BUILD or sorted(data.get('binaries', {})) != sorted(BINS):
        raise StageError(f'{manifest} is not a complete {SCHEMA_BUILD} for the {len(BINS)} shell binaries')
    for name, b in data['binaries'].items():
        digest, _ = sha256_file(b['path'])
        if digest != b['sha256']:
            raise StageError(f'{name}: {b["path"]} changed since the build that recorded it (sha256 {digest} != {b["sha256"]})')
    return data


# ------------------------------------------------------------------ inventory

def rewrite_bin(text, prefix):
    return text.replace('/usr/bin/', f'{prefix}/bin/')


def inventory(prefix, build_data=None, source=ROOT):
    """[(absolute runtime path, source path, mode, transform or None)] in install order."""
    source = Path(source)
    contrib = source / 'contrib'
    items = []
    for name in BINS:
        items.append((f'{prefix}/bin/{name}', build_data['binaries'][name]['path'] if build_data else f'<build>/{name}', 0o755, None))
    # DEPRECATED compatibility names (macbar -> decbar, ...), kept for one release so existing keybinds keep working.
    for name in BINS:
        if name not in NO_COMPAT:
            items.append((f'{prefix}/bin/mac{name[3:]}', contrib / 'compat/mac-compat', 0o755, None))
    for u in UNITS:
        items.append((f'{prefix}/lib/systemd/user/decebal-{u}.service', contrib / f'decebal-{u}.service', 0o644, rewrite_bin))
        items.append((f'/etc/xdg/autostart/decebal-{u}.desktop', contrib / f'decebal-{u}.desktop', 0o644, None))
    for a in APPLICATIONS:
        items.append((f'{prefix}/share/applications/{a}.desktop', contrib / f'{a}.desktop', 0o644, None))
    items.append((f'{prefix}/share/decebal/dbus/org.freedesktop.Notifications.service',
                  contrib / 'dbus/org.freedesktop.Notifications.service', 0o644, rewrite_bin))
    themes = source / 'themes'
    for dirpath, dirs, files in os.walk(themes, followlinks=False):
        dirs.sort()
        for d in dirs:
            if os.path.islink(os.path.join(dirpath, d)):
                raise StageError(f'symlinked theme directory {os.path.join(dirpath, d)}')
        for f in sorted(files):
            p = Path(dirpath) / f
            st = os.lstat(p)
            if not stat.S_ISREG(st.st_mode):
                raise StageError(f'theme entry {p} is not a regular file')
            if '\n' in f:
                raise StageError(f'theme file name with a newline: {p!r}')
            items.append((f'{prefix}/share/decebal/themes/{p.relative_to(themes)}', p, 0o644, None))
    items.append((f'{prefix}/share/doc/decebal/README.md', source / 'README.md', 0o644, None))
    items.append((f'{prefix}/share/licenses/decebal/LICENSE', source / 'LICENSE', 0o644, None))
    seen = set()
    for dst, *_ in items:
        if dst in seen:
            raise StageError(f'duplicate install path {dst}')
        seen.add(dst)
    return items


# ------------------------------------------------------------------ destination tree (anchored descriptor walks)
#
# DESTDIR (or / when empty) is the one trusted anchor: it is re-opened by path (following symlinks, as the caller's
# own staging root) at the start of every walk, so if DESTDIR itself is moved or replaced between steps, later steps
# use whatever DESTDIR names then. Every directory *below* it is reached component by component with
# openat(O_DIRECTORY|O_NOFOLLOW), created with mkdirat when missing. Files are created, replaced (renameat over a temp
# in the same held directory), read and removed only relative to held directory descriptors, so a symlinked
# intermediate parent is refused rather than followed, including one swapped in mid-run. A symlink at the final name
# is replaced, never written through.

DIR_FLAGS = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
RENAME_NOREPLACE = 1
_libc = ctypes.CDLL(None, use_errno=True)


def _rename_noreplace(sfd, sname, dfd, dname):
    """renameat2(RENAME_NOREPLACE): atomic, and never replaces an existing entry at the destination."""
    if _libc.renameat2(sfd, os.fsencode(sname), dfd, os.fsencode(dname), RENAME_NOREPLACE) != 0:
        e = ctypes.get_errno()
        if e in (22, 38, 95):       # EINVAL / ENOSYS / EOPNOTSUPP: no RENAME_NOREPLACE here
            raise StageError(f'{dname}: this filesystem cannot rename without replacing (renameat2 NOREPLACE); nothing removed')
        raise OSError(e, os.strerror(e), dname)
MAX_INVENTORY = 16 << 20
FAULTS: dict = {}       # fixture-test hooks only; nothing in the CLI sets them


def _fault(point, **ctx):
    hook = FAULTS.get(point)
    if hook:
        hook(**ctx)


def _split(path):
    parts = path.split('/')[1:]
    if not path.startswith('/') or any(p in ('', '.', '..') for p in parts):
        raise StageError(f'unsafe install path {path!r}')
    return parts[:-1], parts[-1]


class Dest:
    def __init__(self, destdir):
        self.destdir = destdir

    def anchor(self, create):
        """Descriptor of DESTDIR (followed: it is the caller's own staging root), or None when it does not exist yet."""
        root = self.destdir or '/'
        try:
            return os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
        except FileNotFoundError:
            if not create:
                return None
            os.makedirs(root, 0o755, exist_ok=True)
            return os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)

    def walk(self, dirs, create):
        """(fd of the deepest directory reached, number of components reached). Refuses symlinks and non-directories."""
        fd = self.anchor(create)
        if fd is None:
            return None, -1
        for i, part in enumerate(dirs):
            try:
                nfd = os.open(part, DIR_FLAGS, dir_fd=fd)
            except FileNotFoundError:
                if not create:
                    return fd, i
                try:
                    os.mkdir(part, 0o755, dir_fd=fd)
                except FileExistsError:
                    pass
                try:
                    nfd = os.open(part, DIR_FLAGS, dir_fd=fd)
                except OSError as e:
                    os.close(fd)
                    raise StageError(f'{self.show(dirs[:i + 1])}: not a real directory ({e.strerror}); refusing to follow it')
            except OSError as e:
                os.close(fd)
                raise StageError(f'{self.show(dirs[:i + 1])}: symlink or non-directory parent ({e.strerror}); refusing to follow it')
            os.close(fd)
            fd = nfd
        return fd, len(dirs)

    def show(self, dirs):
        return f'{self.destdir}/' + '/'.join(dirs)

    def preflight(self, path):
        """No writes: every existing parent is a real directory, the deepest one is writable, the final name is not a
        directory. Returns a problem string or None."""
        dirs, name = _split(path)
        try:
            fd, reached = self.walk(dirs, create=False)
        except StageError as e:
            return str(e)
        if fd is None:
            parent = os.path.dirname(self.destdir.rstrip('/')) or '/'
            return None if os.access(parent, os.W_OK | os.X_OK) else f'{parent} is not writable'
        try:
            if not os.access('.', os.W_OK | os.X_OK, dir_fd=fd):
                return f'{self.show(dirs[:reached])} is not writable'
            if reached == len(dirs):
                try:
                    st = os.stat(name, dir_fd=fd, follow_symlinks=False)
                    if stat.S_ISDIR(st.st_mode):
                        return f'{self.destdir}{path} is a directory'
                except FileNotFoundError:
                    pass
            return None
        finally:
            os.close(fd)

    def write(self, path, data, mode):
        _fault('before_write', path=path)
        dirs, name = _split(path)
        fd, _ = self.walk(dirs, create=True)
        tmp = f'.{name}.stage-{secrets.token_hex(4)}'
        try:
            wfd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=fd)
            try:
                view = memoryview(data)
                while view:
                    view = view[os.write(wfd, view):]
                os.fchmod(wfd, mode)
            finally:
                os.close(wfd)
            os.replace(tmp, name, src_dir_fd=fd, dst_dir_fd=fd)
        except BaseException:
            try:
                os.unlink(tmp, dir_fd=fd)
            except OSError:
                pass
            raise
        finally:
            os.close(fd)

    def read_regular(self, path, limit):
        """Bytes of a regular file reached without following any symlink, or None when absent."""
        dirs, name = _split(path)
        fd, reached = self.walk(dirs, create=False)
        if fd is None or reached < len(dirs):
            if fd is not None:
                os.close(fd)
            return None
        try:
            try:
                ffd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK, dir_fd=fd)
            except FileNotFoundError:
                return None
            except OSError as e:
                raise StageError(f'{self.destdir}{path}: not a regular file ({e.strerror})')
            try:
                return _read_fd(ffd, limit, f'{self.destdir}{path}')
            finally:
                os.close(ffd)
        finally:
            os.close(fd)

    def remove_if(self, path, digest):
        """Remove the file at `path` only if it is the regular file with `digest`. Returns (state, kept_as) with state
        'removed', 'absent', 'kept' (changed, or not a regular file; left at its name) or 'preserved' (changed, and the
        name was re-created meanwhile, so it stays in the private folder named by kept_as).

        Protocol, all relative to the held parent directory (never following a symlink):
          1. mkdirat a fresh private folder (0700) in that parent: same filesystem, so the next step is a rename;
          2. renameat2(RENAME_NOREPLACE) whatever is at the name into it. From then on no pathname outside the private
             folder refers to that entry, so a later replacement at the original name is a different, untouched file;
          3. verify the moved entry there (regular, O_NOFOLLOW, bounded read, sha256);
          4. match: unlink it inside the private folder. Mismatch: rename it back with RENAME_NOREPLACE; if the name
             was re-created meanwhile, never clobber it, keep the moved entry in the private folder and report it.
        """
        dirs, name = _split(path)
        fd, reached = self.walk(dirs, create=False)
        if fd is None or reached < len(dirs):
            if fd is not None:
                os.close(fd)
            return 'absent', None
        try:
            try:
                st = os.stat(name, dir_fd=fd, follow_symlinks=False)
            except FileNotFoundError:
                return 'absent', None
            if not stat.S_ISREG(st.st_mode):
                return 'kept', None
            _fault('before_quarantine', path=path)
            qname = f'.decebal-quarantine-{secrets.token_hex(6)}'
            os.mkdir(qname, 0o700, dir_fd=fd)
            qfd = os.open(qname, DIR_FLAGS, dir_fd=fd)
            result = ('absent', None)
            try:
                qst = os.fstat(qfd)
                if qst.st_uid != os.geteuid() or stat.S_IMODE(qst.st_mode) & 0o077:
                    raise StageError(f'{self.show(dirs)}/{qname}: private folder is not private; nothing removed')
                try:
                    _rename_noreplace(fd, name, qfd, name)
                except FileNotFoundError:
                    result = ('absent', None)
                else:
                    _fault('after_quarantine', path=path)
                    if self._matches_at(qfd, name, digest):
                        _fault('before_unlink', path=path)
                        os.unlink(name, dir_fd=qfd)
                        result = ('removed', None)
                    else:
                        try:
                            _rename_noreplace(qfd, name, fd, name)
                            result = ('kept', None)
                        except FileExistsError:
                            result = ('preserved', f'{self.show(dirs)}/{qname}/{name}')
            finally:
                os.close(qfd)
            if result[0] != 'preserved':
                try:
                    os.rmdir(qname, dir_fd=fd)
                except OSError:
                    pass
            return result
        finally:
            os.close(fd)

    @staticmethod
    def _matches_at(dfd, name, digest):
        try:
            st = os.stat(name, dir_fd=dfd, follow_symlinks=False)
            if not stat.S_ISREG(st.st_mode):
                return False
            ffd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK, dir_fd=dfd)
        except OSError:
            return False
        try:
            return os.fstat(ffd).st_ino == st.st_ino and hashlib.sha256(_read_fd(ffd, MAX_FILE, name)).hexdigest() == digest
        except StageError:
            return False
        finally:
            os.close(ffd)

    def prune(self, top):
        """rmdir empty directories at and below the owned folder `top` (never follows symlinks)."""
        dirs, name = _split(top)
        fd, reached = self.walk(dirs, create=False)
        if fd is None or reached < len(dirs):
            if fd is not None:
                os.close(fd)
            return
        try:
            _prune_at(fd, name)
        finally:
            os.close(fd)


def _prune_at(pfd, name):
    try:
        fd = os.open(name, DIR_FLAGS, dir_fd=pfd)
    except OSError:
        return
    try:
        for entry in os.listdir(fd):
            _prune_at(fd, entry)
    finally:
        os.close(fd)
    try:
        os.rmdir(name, dir_fd=pfd)
    except OSError:
        pass


def _read_fd(fd, limit, label):
    st = os.fstat(fd)
    if not stat.S_ISREG(st.st_mode):
        raise StageError(f'{label} is not a regular file')
    if st.st_size > limit:
        raise StageError(f'{label} is {st.st_size} bytes, over the {limit}-byte limit')
    _fault('source_opened', fd=fd, label=label)
    chunks, n = [], 0
    while b := os.read(fd, 1 << 20):
        n += len(b)
        if n > limit:
            raise StageError(f'{label} grew past the {limit}-byte limit while being read')
        chunks.append(b)
    return b''.join(chunks)


def _read(src):
    fd = os.open(src, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK)
    try:
        return _read_fd(fd, MAX_FILE, str(src))
    finally:
        os.close(fd)


# What an inventory may name: only this package's own install namespace under the exact prefix it was written for.
def _owned(path, prefix):
    # Shared system directories require exact shipped names. A dec*/mac* prefix also
    # matches unrelated programs such as machinectl and macchanger.
    binaries = set(BINS) | {'mac' + name[3:] for name in BINS if name not in NO_COMPAT}
    exact = {f'{prefix}/bin/{name}' for name in binaries}
    exact |= {f'{prefix}/share/applications/{name}.desktop' for name in APPLICATIONS}
    exact |= {f'{prefix}/lib/systemd/user/decebal-{name}.service' for name in UNITS}
    exact |= {f'/etc/xdg/autostart/decebal-{name}.desktop' for name in UNITS}
    if path in exact:
        return True
    # Only package-exclusive trees may contain arbitrary asset names. Deliberately
    # retain unknown/retired shared-directory names rather than guessing ownership.
    roots = (f'{prefix}/share/decebal/', f'{prefix}/share/doc/decebal/', f'{prefix}/share/licenses/decebal/')
    return path.startswith(roots) and not any(x in ('', '.', '..') for x in path.split('/')[1:])


def _load_inventory(dest, prefix):
    raw = dest.read_regular(f'{prefix}/{INVENTORY_REL}', MAX_INVENTORY)
    if raw is None:
        return None
    try:
        data = json.loads(raw)
    except ValueError:
        raise StageError('existing install inventory is not valid JSON; refusing to guess what to remove')
    if not isinstance(data, dict) or data.get('schema') != SCHEMA_INVENTORY or not isinstance(data.get('files'), dict):
        raise StageError('existing install inventory has an unknown format; refusing to guess what to remove')
    if data.get('prefix') != prefix:
        raise StageError(f'install inventory was written for prefix {data.get("prefix")!r}, not {prefix!r}; refusing')
    bad = [p for p, d in data['files'].items() if not (isinstance(p, str) and _owned(p, prefix)
                                                       and isinstance(d, str) and re.fullmatch(r'[0-9a-f]{64}', d))]
    if bad:
        raise StageError(f'install inventory names paths outside the decebal install namespace: {bad[:3]}; refusing')
    return data


def _remove_if_unchanged(dest, files, report):
    for path, digest in sorted(files.items()):
        state, kept_as = dest.remove_if(path, digest)
        if state == 'removed':
            report['removed'].append(path)
        elif state == 'kept':
            report['kept_changed'].append(path)
        elif state == 'preserved':
            report['preserved'].append({'path': path, 'kept_as': kept_as})


def install(prefix, destdir, manifest, source=ROOT, write_inventory=True):
    prefix, destdir = check_prefix(prefix), check_destdir(destdir)
    dest = Dest(destdir)
    build_data = load_build(manifest)
    items = inventory(prefix, build_data, source)
    old = _load_inventory(dest, prefix) if write_inventory else None
    targets = [dst for dst, *_ in items] + [f'{prefix}/{INVENTORY_REL}']
    blocked = sorted({p for p in map(dest.preflight, targets) if p})
    if blocked:
        raise StageError(f'nothing installed: {blocked[:5]} (PREFIX={prefix}; autostart entries always go to '
                         '$DESTDIR/etc/xdg/autostart)')
    # Pass 1 reads, transforms and digests every file before anything is written, so a bad source fails with no writes.
    planned = []
    for dst, src, mode, transform in items:
        data = _read(src)
        if transform:
            data = transform(data.decode(), prefix).encode()
        digest = hashlib.sha256(data).hexdigest()
        name = dst.rsplit('/', 1)[1]
        if dst == f'{prefix}/bin/{name}' and name in build_data['binaries'] and digest != build_data['binaries'][name]['sha256']:
            raise StageError(f'{src} changed since the build that recorded it')
        planned.append((dst, src, mode, data if transform else None, digest))
    # Pass 2 writes; each source is re-read and must still match its pass-1 digest.
    written = {}
    for dst, src, mode, data, digest in planned:
        if data is None:
            data = _read(src)
            if hashlib.sha256(data).hexdigest() != digest:
                raise StageError(f'{src} changed while installing; files written so far: {len(written)}')
        dest.write(dst, data, mode)
        written[dst] = digest
    report = {'installed': len(written), 'removed': [], 'kept_changed': [], 'preserved': []}
    if not write_inventory:      # package builds: the package manager records the file list
        return report
    if old:
        _remove_if_unchanged(dest, {p: d for p, d in old['files'].items() if p not in written}, report)
    inv = {'schema': SCHEMA_INVENTORY, 'prefix': prefix, 'source_commit': build_data.get('source_commit'),
           'source_dirty': build_data.get('source_dirty'),
           'binaries': {n: b['sha256'] for n, b in build_data['binaries'].items()}, 'files': written}
    dest.write(f'{prefix}/{INVENTORY_REL}', (json.dumps(inv, indent=2, sort_keys=True) + '\n').encode(), 0o644)
    return report


def uninstall(prefix, destdir):
    prefix, destdir = check_prefix(prefix), check_destdir(destdir)
    dest = Dest(destdir)
    report = {'removed': [], 'kept_changed': [], 'preserved': []}
    old = _load_inventory(dest, prefix)
    if old is None:
        raise StageError(f'no {prefix}/{INVENTORY_REL} under DESTDIR; nothing is removed without an install inventory')
    blocked = sorted({p for p in map(dest.preflight, list(old['files']) + [f'{prefix}/{INVENTORY_REL}']) if p})
    if blocked:
        raise StageError(f'nothing removed: {blocked[:5]}')
    _remove_if_unchanged(dest, old['files'], report)
    raw = dest.read_regular(f'{prefix}/{INVENTORY_REL}', MAX_INVENTORY)
    if raw is not None:
        dest.remove_if(f'{prefix}/{INVENTORY_REL}', hashlib.sha256(raw).hexdigest())
    for top in (f'{prefix}/share/decebal', f'{prefix}/share/doc/decebal', f'{prefix}/share/licenses/decebal'):
        dest.prune(top)
    return report


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--source-root', default=str(ROOT), help=argparse.SUPPRESS)
    sub = ap.add_subparsers(dest='cmd', required=True)
    b = sub.add_parser('build')
    b.add_argument('--manifest', default=os.environ.get('BUILD_MANIFEST') or 'target/decebal-build-manifest.json')
    b.add_argument('cargo_args', nargs='*')
    for name in ('install', 'uninstall', 'inventory'):
        s = sub.add_parser(name)
        s.add_argument('--prefix', default=os.environ.get('PREFIX') or '/usr/local')
        s.add_argument('--destdir', default=os.environ.get('DESTDIR', ''))
        if name == 'install':
            s.add_argument('--manifest', default=os.environ.get('BUILD_MANIFEST') or 'target/decebal-build-manifest.json')
            s.add_argument('--no-inventory', action='store_true', help='package builds: no install-inventory.json')
    a = ap.parse_args(argv)
    src = Path(a.source_root)
    try:
        if a.cmd == 'build':
            raw = os.environ.get('DECEBAL_CARGO') or 'cargo'
            # A path to an executable is used verbatim (it may contain spaces); otherwise shell-style words.
            cargo = [raw] if os.path.isfile(raw) and os.access(raw, os.X_OK) else shlex.split(raw)
            manifest = a.manifest if os.path.isabs(a.manifest) else str(src / a.manifest)
            data = build(cargo, manifest, a.cargo_args, src)
            print(f'recorded {len(data["binaries"])} binaries from {", ".join(data["target_release_dirs"])} in {manifest}')
        elif a.cmd == 'install':
            manifest = a.manifest if os.path.isabs(a.manifest) else str(src / a.manifest)
            print(json.dumps(install(a.prefix, a.destdir, manifest, src, not a.no_inventory) | {'prefix': a.prefix}, sort_keys=True))
        elif a.cmd == 'uninstall':
            print(json.dumps(uninstall(a.prefix, a.destdir), sort_keys=True))
        else:
            for dst, srcp, mode, _ in inventory(check_prefix(a.prefix), None, src):
                print(f'{mode:o} {dst}')
    except (StageError, OSError, ValueError) as e:
        print(f'stage.py: {e}', file=sys.stderr)
        return 2
    return 0


if __name__ == '__main__':
    sys.exit(main())
