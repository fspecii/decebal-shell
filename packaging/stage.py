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
    if any(p in ('.', '..') for p in prefix.split('/')) or '//' in prefix:
        raise StageError('PREFIX must be absolute without ., .. or empty path components')
    return prefix.rstrip('/') or '/'


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


def _write(target, data, mode):
    parent = os.path.dirname(target)
    os.makedirs(parent, mode=0o755, exist_ok=True)
    tmp = os.path.join(parent, f'.{os.path.basename(target)}.stage-{secrets.token_hex(4)}')
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    try:
        with os.fdopen(fd, 'wb') as f:
            f.write(data)
            os.fchmod(f.fileno(), mode)
        os.replace(tmp, target)       # replaces a symlink at target itself, never writes through it
    except BaseException:
        try:
            os.unlink(tmp)
        except FileNotFoundError:
            pass
        raise


def _read(src):
    fd = os.open(src, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK)
    try:
        if not stat.S_ISREG(os.fstat(fd).st_mode):
            raise StageError(f'{src} is not a regular file')
        with os.fdopen(fd, 'rb', closefd=False) as f:
            return f.read(MAX_FILE + 1)
    finally:
        os.close(fd)


def _unwritable(target):
    """The nearest existing ancestor of target when it is not a writable directory, else None."""
    d = os.path.dirname(target)
    while not os.path.lexists(d):
        d = os.path.dirname(d)
    return None if os.path.isdir(d) and not os.path.islink(d) and os.access(d, os.W_OK | os.X_OK) else d


def _allowed(path, prefix):
    return (path.startswith(prefix.rstrip('/') + '/') or path.startswith('/etc/xdg/autostart/')) \
        and not any(p in ('', '.', '..') for p in path.split('/')[1:])


def _load_inventory(destdir, prefix):
    try:
        data = json.loads(Path(destdir + f'{prefix}/{INVENTORY_REL}').read_text())
    except FileNotFoundError:
        return None
    if data.get('schema') != SCHEMA_INVENTORY or not isinstance(data.get('files'), dict):
        raise StageError('existing install inventory is not readable; refusing to guess what to remove')
    bad = [p for p in data['files'] if not _allowed(p, data.get('prefix', prefix))]
    if bad:
        raise StageError(f'install inventory lists paths outside its prefix: {bad[:3]}')
    return data


def _remove_if_unchanged(destdir, files, report):
    for path, digest in sorted(files.items()):
        target = destdir + path
        try:
            current, _ = sha256_file(target)
        except FileNotFoundError:
            continue
        except StageError:
            report['kept_changed'].append(path)
            continue
        if current != digest:
            report['kept_changed'].append(path)
            continue
        os.unlink(target)
        report['removed'].append(path)


def install(prefix, destdir, manifest, source=ROOT, write_inventory=True):
    prefix, destdir = check_prefix(prefix), check_destdir(destdir)
    build_data = load_build(manifest)
    items = inventory(prefix, build_data, source)
    old = _load_inventory(destdir, prefix)
    blocked = sorted({_unwritable(destdir + dst) for dst, *_ in items} - {None})
    if blocked:
        raise StageError(f'not writable, nothing installed: {blocked} (PREFIX={prefix}; autostart entries always go to '
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
        _write(destdir + dst, data, mode)
        written[dst] = digest
    report = {'installed': len(written), 'removed': [], 'kept_changed': []}
    if not write_inventory:      # distribution packages: the package manager owns the file list
        return report
    if old:
        _remove_if_unchanged(destdir, {p: d for p, d in old['files'].items() if p not in written}, report)
    inv = {'schema': SCHEMA_INVENTORY, 'prefix': prefix, 'build_manifest': str(manifest),
           'source_commit': build_data.get('source_commit'), 'source_dirty': build_data.get('source_dirty'),
           'binaries': {n: b['sha256'] for n, b in build_data['binaries'].items()}, 'files': written}
    _write(destdir + f'{prefix}/{INVENTORY_REL}', (json.dumps(inv, indent=2, sort_keys=True) + '\n').encode(), 0o644)
    return report


def uninstall(prefix, destdir):
    prefix, destdir = check_prefix(prefix), check_destdir(destdir)
    report = {'removed': [], 'kept_changed': []}
    old = _load_inventory(destdir, prefix)
    if old is None:
        raise StageError(f'no {prefix}/{INVENTORY_REL} under DESTDIR; nothing is removed without an install inventory')
    _remove_if_unchanged(destdir, old['files'], report)
    os.unlink(destdir + f'{prefix}/{INVENTORY_REL}')
    # Prune only directories this package owns and that are now empty.
    for top in (f'{prefix}/share/decebal', f'{prefix}/share/doc/decebal', f'{prefix}/share/licenses/decebal'):
        base = destdir + top
        for dirpath, _, _ in sorted(os.walk(base, topdown=False), key=lambda w: -len(w[0])):
            try:
                os.rmdir(dirpath)
            except OSError:
                pass
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
