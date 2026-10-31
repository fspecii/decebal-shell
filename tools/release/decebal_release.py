#!/usr/bin/env python3
"""Release preflight for one sealed frozen-build artifact bundle (decebal-frozen-release-manifest/1).

  verify  ARTIFACT [--expect-commit SHA] [--assets DIR]   bundle, hashes, provenance, path/symlink/size bounds
  plan    ARTIFACT --dest DIR [--dest DIR ...] [--only NAME ...] [--out FILE]
          read-only per-file dry run for exact binary folders, plus the runtime handoff plan
  publish --plan FILE --private-root DIR --journal NEWDIR --lock FILE
          journaled install inside a 0700 private root; --managed-runtime explicitly enables the two managed binary slots
  rollback --journal DIR --lock FILE [--managed-runtime]    roll back an interrupted, failed or committed publish

verify and plan never write outside --out, never execute artifact binaries, never send signals and never change config.
Diagnostics carry paths, digests, identities and environment variable *names* only; no values, no credentials.
"""
from __future__ import annotations

import argparse
import calendar
import contextlib
import ctypes
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import stat
import subprocess
import sys
import tempfile
import time

SCHEMA = 'decebal-frozen-release-manifest/1'
PLAN_SCHEMA = 'decebal-release-plan/1'
JOURNAL_SCHEMA = 'decebal-release-journal/1'
MANIFEST_KEYS = {'binaries', 'build', 'records', 'schema', 'sealed_utc', 'source'}
RECORDS = {'SHA256SUMS', 'build-messages.jsonl', 'build.log', 'result.json', 'source-after.json', 'source-before.json'}
BINARY_KEYS = {'cargo_executable', 'cargo_fresh', 'debug_assertions', 'opt_level', 'sha256', 'size'}
NAME_RE = re.compile(r'^(examples/)?[a-z][a-z0-9_-]{0,63}$')
HEX64 = re.compile(r'^[0-9a-f]{64}$')
HEX40 = re.compile(r'^[0-9a-f]{40}$')
MAX_MANIFEST = 1 << 20
MAX_RECORD = 64 << 20
MAX_BINARY = 64 << 20
MAX_BINARIES = 64
MAX_TOTAL = 1 << 30
MAX_ASSETS = 20000
MAX_ASSET = 64 << 20
MAX_PLAN = 4 << 20
MANAGED_MAX_AGE = 15 * 60
SHIPPED_BINARIES = frozenset('decbar decclip decdash decdock decexpose dechaven dechot dechud declaunch declock decmon decnotes decnotify decsession decsettings decspot decswitch decthemectl dectimer decutil decwall decwidgets decwm'.split())

HOME = Path.home()
# User-local slots the shell runs from. Private publication always refuses these and their ancestors/children.
LIVE_DIRS = (HOME / '.local/share/rdwm-decebal/bin', HOME / '.local/share/rdwm-macshell/bin')
LIVE_FORBIDDEN_ROOTS = LIVE_DIRS + (HOME / '.local/bin', HOME / '.config', Path('/usr'), Path('/etc'), Path('/opt'))

# Config files root's deployment journals fingerprint (config_before/config_after). Hashed read-only; never written.
CONFIG_WATCH = (HOME / '.config/rdwm/config.toml', HOME / '.config/omarchy/decebal.json',
                HOME / '.config/omarchy/dock-settings.json', HOME / '.config/picom/picom.conf')

# Components that render the theme, how they hold their instance and how readiness is observed. Readiness is reported, never
# exercised here: launching a second copy of a `hold` daemon exits, an `acquire_signalling` launcher signals the holder.
THEME_COMPONENTS = {
    'decbar': {'locks': ['decbar.lock', 'decbar-bottom.lock'], 'instance': 'single::hold (second copy exits)',
               'ready': 'new holder pid written to $XDG_RUNTIME_DIR/decbar.lock and decbar-bottom.lock',
               'theme_data': 'theme::live watcher (500 ms stat poll) reloads theme files without a process change'},
    'decdock': {'locks': ['decdock.lock'], 'instance': 'single::hold (second copy exits)',
                'ready': 'new holder pid written to $XDG_RUNTIME_DIR/decdock.lock',
                'theme_data': 'theme::live watcher reloads theme files without a process change'},
    'decwidgets': {'locks': ['decwidgets.lock'], 'instance': 'single::hold (second copy exits)',
                   'ready': '`decwidgets status --json` reports "running": true, and the lock names the new pid',
                   'theme_data': 'theme::live watcher reloads theme files without a process change'},
    'dechud': {'locks': ['dechud.lock', 'dechud-watch.lock'], 'instance': 'single::acquire_signalling (SIG_UPDATE to holder)',
               'ready': 'new holder pid written to $XDG_RUNTIME_DIR/dechud-watch.lock',
               'theme_data': 'theme::live watcher reloads theme files without a process change'},
}
# Never stopped, restarted or closed by this tool, whatever the plan says.
NEVER_TOUCH = {
    'decwm': 'window manager: never restarted or killed; a new binary takes effect only at the next user-initiated session start',
    'decsettings': 'open Settings windows and drafts are never force-closed; the new binary is used by the next window the user opens',
}

# Fault-injection points for the fixture tests only. Nothing in the CLI populates this.
FAULTS: dict = {}


class ReleaseError(RuntimeError):
    pass


def _fault(point, **ctx):
    hook = FAULTS.get(point)
    if hook:
        hook(**ctx)


def utc():
    return time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())


# ------------------------------------------------------------------ safe file access

def _open_nofollow(path, flags=os.O_RDONLY):
    # O_NONBLOCK: a FIFO planted where a file is expected must not hang the open; it is then refused as non-regular.
    return os.open(path, flags | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK)


def _check_components(root: Path, rel: str):
    """Every component of root/rel below root is a real directory/file, never a symlink; returns the full path."""
    if rel.startswith('/') or any(p in ('', '.', '..') for p in rel.split('/')):
        raise ReleaseError(f'unsafe relative path {rel!r}')
    cur = root
    for part in rel.split('/'):
        cur = cur / part
        st = os.lstat(cur)
        if stat.S_ISLNK(st.st_mode):
            raise ReleaseError(f'symlink in bundle path: {cur}')
    return cur


def identity(st):
    return {'dev': st.st_dev, 'ino': st.st_ino, 'size': st.st_size, 'mode': stat.S_IMODE(st.st_mode),
            'mtime_ns': st.st_mtime_ns, 'nlink': st.st_nlink, 'uid': st.st_uid}


def hash_fd(fd, limit):
    st = os.fstat(fd)
    if not stat.S_ISREG(st.st_mode):
        raise ReleaseError('not a regular file')
    if st.st_size > limit:
        raise ReleaseError(f'file exceeds bound {limit} bytes ({st.st_size})')
    h, n = hashlib.sha256(), 0
    os.lseek(fd, 0, os.SEEK_SET)
    while True:
        b = os.read(fd, 1 << 20)
        if not b:
            break
        n += len(b)
        if n > limit:
            raise ReleaseError(f'file grew past bound {limit} bytes')
        h.update(b)
    after = os.fstat(fd)
    if (after.st_size, after.st_mtime_ns, after.st_ino) != (st.st_size, st.st_mtime_ns, st.st_ino) or n != st.st_size:
        raise ReleaseError('file changed while hashing')
    return h.hexdigest(), st


def hash_path(path, limit):
    fd = _open_nofollow(path)
    try:
        return hash_fd(fd, limit)
    finally:
        os.close(fd)


def read_bounded(path, limit):
    fd = _open_nofollow(path)
    try:
        st = os.fstat(fd)
        if not stat.S_ISREG(st.st_mode) or st.st_size > limit:
            raise ReleaseError(f'{path}: not a regular file within {limit} bytes')
        data = b''
        while len(data) <= limit:
            b = os.read(fd, 1 << 20)
            if not b:
                break
            data += b
        if len(data) > limit:
            raise ReleaseError(f'{path}: exceeds {limit} bytes')
        return data, st
    finally:
        os.close(fd)


def load_json(path, limit):
    data, _ = read_bounded(path, limit)
    return json.loads(data), hashlib.sha256(data).hexdigest()


# ------------------------------------------------------------------ verify

def verify(art, expect_commit=None, assets=None):
    """Full check of the sealed bundle. Returns a report; report['ok'] is False with reasons in report['problems']."""
    rep = {'artifact_dir': str(art), 'checked_utc': utc(), 'problems': []}
    try:
        return _verify(Path(art), expect_commit, assets, rep)
    except (AttributeError, TypeError, KeyError) as e:
        rep['problems'].append(f'malformed manifest or record field: {type(e).__name__}: {e}')
        return _done(rep)


def _verify(art, expect_commit, assets, rep):
    problems = rep['problems']

    def need(cond, msg):
        if not cond:
            problems.append(msg)
        return bool(cond)

    try:
        st = os.lstat(art)
        if not need(stat.S_ISDIR(st.st_mode) and not stat.S_ISLNK(st.st_mode), 'artifact dir is not a real directory'):
            return _done(rep)
        if not need(art.is_absolute() and os.path.realpath(art) == str(art), 'artifact dir must be absolute with no symlink components'):
            return _done(rep)
        raw, _ = read_bounded(art / 'MANIFEST.json', MAX_MANIFEST)
        digest = hashlib.sha256(raw).hexdigest()
        rep['manifest_sha256'] = digest
        seal, _ = read_bounded(art / 'MANIFEST.json.sha256', 256)
        need(seal.decode('ascii', 'replace') == f'{digest}  MANIFEST.json\n', 'MANIFEST.json.sha256 does not seal MANIFEST.json')
        m = json.loads(raw)
    except (OSError, ValueError, ReleaseError) as e:
        problems.append(f'manifest unreadable: {e}')
        return _done(rep)

    if not need(isinstance(m, dict) and set(m) == MANIFEST_KEYS, f'manifest keys {sorted(m) if isinstance(m, dict) else type(m).__name__} != {sorted(MANIFEST_KEYS)}'):
        return _done(rep)
    need(m['schema'] == SCHEMA, f'schema {m["schema"]!r} unsupported')
    src, build, bins, records = m['source'], m['build'], m['binaries'], m['records']
    commit = src.get('commit') if isinstance(src, dict) else None
    if not need(isinstance(commit, str) and HEX40.match(commit), 'source.commit is not a 40-hex sha'):
        return _done(rep)
    rep['commit'] = commit
    need(src.get('dirty') is False, 'source was dirty')
    need(src.get('production_inputs_unchanged_during_build') is True, 'production inputs changed during build')
    if expect_commit is not None:
        need(commit == expect_commit, f'commit {commit} != expected {expect_commit}')
    need(isinstance(build, dict) and build.get('exit') == 0 and build.get('timed_out') is False and build.get('profile') == 'release'
         and build.get('locked') is True and build.get('offline') is True, 'build record is not a clean locked offline release build')

    # sealed records
    if need(isinstance(records, dict) and set(records) == RECORDS, f'records {sorted(records) if isinstance(records, dict) else records} != {sorted(RECORDS)}'):
        for name, want in sorted(records.items()):
            try:
                _check_components(art, name)
                got, rst = hash_path(art / name, MAX_RECORD)
                need(got == want, f'record {name}: sha256 {got} != sealed {want}')
                need(not rst.st_mode & 0o222, f'record {name} is writable')
            except (OSError, ReleaseError) as e:
                problems.append(f'record {name}: {e}')
    try:
        before, _ = load_json(art / 'source-before.json', MAX_RECORD)
        after, _ = load_json(art / 'source-after.json', MAX_RECORD)
        result, _ = load_json(art / 'result.json', MAX_RECORD)
        sums_raw, _ = read_bounded(art / 'SHA256SUMS', MAX_RECORD)
    except (OSError, ValueError, ReleaseError) as e:
        problems.append(f'records unreadable: {e}')
        return _done(rep)
    for label, s in (('before', before), ('after', after)):
        need(s.get('head') == commit and s.get('dirty') is False and s.get('tree') == src.get('tree'),
             f'source-{label}: head/tree/dirty do not match the manifest commit')
        need(s.get('production_fingerprint') == src.get('production_fingerprint'), f'source-{label}: production fingerprint differs')
    need(before.get('production_inputs') == after.get('production_inputs'), 'source-before/after production inputs differ')
    for k, v in (src.get('key_inputs') or {}).items():
        need((before.get('production_inputs') or {}).get(k) == v, f'key input {k} digest differs from source-before')

    # binaries
    if not need(isinstance(bins, dict) and 0 < len(bins) <= MAX_BINARIES, f'binary count outside 1..{MAX_BINARIES}'):
        return _done(rep)
    need(result.get('passed') is True and result.get('sha') == commit and result.get('dirty') is False, 'result.json is not a passed clean build of this commit')
    need(result.get('binaries') == {n: b.get('sha256') for n, b in bins.items()}, 'result.json binaries differ from manifest')
    sums = {}
    for ln in sums_raw.decode('utf-8', 'replace').splitlines():
        mm = re.fullmatch(r'([0-9a-f]{64})  (\S+)', ln)
        if not mm or mm.group(2) in sums:
            problems.append(f'SHA256SUMS: malformed or duplicate line {ln[:120]!r}')
            continue
        sums[mm.group(2)] = mm.group(1)
    need(sums == {n: b.get('sha256') for n, b in bins.items()}, 'SHA256SUMS differs from manifest binaries')
    total, verified = 0, {}
    for name, b in sorted(bins.items()):
        if not (NAME_RE.match(name) and isinstance(b, dict) and set(b) == BINARY_KEYS and isinstance(b['sha256'], str)
                and HEX64.match(b['sha256']) and isinstance(b['size'], int) and 0 < b['size'] <= MAX_BINARY):
            problems.append(f'binary {name!r}: bad name, keys, digest or size bound')
            continue
        need(b['debug_assertions'] is False and b['opt_level'] == '3', f'binary {name}: not an optimized release artifact')
        total += b['size']
        try:
            p = _check_components(art / 'bin', name)
            got, bst = hash_path(p, MAX_BINARY)
            ok = need(got == b['sha256'] and bst.st_size == b['size'], f'binary {name}: sha256/size mismatch')
            ok &= need(not bst.st_mode & 0o222, f'binary {name}: writable artifact')
            ok &= need(bst.st_mode & 0o100, f'binary {name}: not executable')
            if ok:
                verified[name] = {'sha256': got, 'size': bst.st_size}
        except (OSError, ReleaseError) as e:
            problems.append(f'binary {name}: {e}')
    need(total <= MAX_TOTAL, f'binaries total {total} exceeds {MAX_TOTAL}')
    try:
        for root, dirs, files in os.walk(art / 'bin', followlinks=False):
            for d in dirs:
                need(not os.path.islink(os.path.join(root, d)), f'symlinked dir in bin/: {d}')
            for f in files:
                rel = os.path.relpath(os.path.join(root, f), art / 'bin')
                need(rel in bins, f'unexpected file in bin/: {rel}')
    except OSError as e:
        problems.append(f'bin/ walk: {e}')
    rep['binaries'] = verified
    rep['binary_total_bytes'] = total
    rep['source'] = {k: src.get(k) for k in ('commit', 'tree', 'branch', 'production_fingerprint', 'tracked_fingerprint', 'pin_record_sha256')}
    rep['build'] = {k: build.get(k) for k in ('profile', 'exit', 'timed_out', 'started_utc', 'finished_utc')}
    rep['build']['env_keys'] = sorted(build.get('env_keys') or [])  # names only, as sealed
    if assets is not None:
        rep['assets'] = verify_assets(Path(assets), commit, problems)
    return _done(rep)


def verify_assets(out, commit, problems):
    """The version-matched data bundle is NOT sealed by MANIFEST.json; this proves its own SHA256 list and its commit only."""
    info = {'dir': str(out), 'sealed_by_manifest': False}
    try:
        res, _ = load_json(out / 'result.json', MAX_RECORD)
        if res.get('sha') != commit or res.get('passed') is not True:
            problems.append('assets: result.json is not a passed proof for this commit')
        raw, _ = read_bounded(out / 'ASSETS.SHA256SUMS', MAX_RECORD)
        lines = raw.decode('utf-8', 'replace').splitlines()
        if len(lines) > MAX_ASSETS:
            problems.append(f'assets: {len(lines)} entries exceed {MAX_ASSETS}')
            return info
        n = 0
        for ln in lines:
            mm = re.fullmatch(r'([0-9a-f]{64})  (share/\S.*)', ln)
            if not mm:
                problems.append(f'assets: malformed line {ln[:120]!r}')
                continue
            p = _check_components(out, mm.group(2))
            got, _ = hash_path(p, MAX_ASSET)
            if got != mm.group(1):
                problems.append(f'assets: {mm.group(2)} sha256 mismatch')
            n += 1
        info['files'] = n
        info['archive_sha256'] = res.get('archive_sha256')
    except (OSError, ValueError, ReleaseError) as e:
        problems.append(f'assets: {e}')
    return info


def _done(rep):
    rep['ok'] = not rep['problems']
    return rep


# ------------------------------------------------------------------ plan (read-only)

def _dest_dir_check(d: Path):
    if not d.is_absolute() or os.path.realpath(d) != str(d):
        raise ReleaseError(f'destination {d} must be absolute with no symlink components')
    st = os.lstat(d)
    if not stat.S_ISDIR(st.st_mode):
        raise ReleaseError(f'destination {d} is not a directory')
    if st.st_uid != os.getuid():
        raise ReleaseError(f'destination {d} is not owned by the current user')
    if st.st_mode & 0o022:
        raise ReleaseError(f'destination {d} is group/other writable')
    return st


def _is_within(p: Path, root: Path):
    return p == root or root in p.parents


def is_live(d: Path):
    return any(_is_within(d, r) or _is_within(r, d) for r in LIVE_FORBIDDEN_ROOTS)


def config_fingerprint(paths):
    """sha256 per config path (None when absent). Content is never copied into reports."""
    out = {}
    for p in map(str, paths):
        try:
            out[p] = hash_path(p, MAX_RECORD)[0]
        except FileNotFoundError:
            out[p] = None
    return out


def plan(art, dests, only=None, expect_commit=None, proc='/proc', runtime_dir=None, config_watch=None, managed_runtime=False):
    rep = verify(art, expect_commit=expect_commit)
    if not rep['ok']:
        raise ReleaseError('manifest verification failed: ' + '; '.join(rep['problems']))
    bins = rep['binaries']
    names = sorted(only) if only else sorted(n for n in bins if not n.startswith('examples/'))
    unknown = [n for n in names if n not in bins]
    if unknown:
        raise ReleaseError(f'not in the manifest: {unknown}')
    if len(set(map(str, dests))) != len(dests) or not dests:
        raise ReleaseError('give each --dest exactly once')
    files, untouched = [], {}
    for d in map(Path, dests):
        _dest_dir_check(d)
        for name in names:
            if '/' in name:
                raise ReleaseError(f'{name}: nested artifacts are not installed into flat binary folders')
            target = d / name
            entry = {'dest_dir': str(d), 'name': name, 'path': str(target), 'new_sha256': bins[name]['sha256'],
                     'new_size': bins[name]['size'], 'source': str(Path(art) / 'bin' / name)}
            try:
                st = os.lstat(target)
            except FileNotFoundError:
                entry.update(action='create', before=None)
                files.append(entry)
                continue
            if stat.S_ISLNK(st.st_mode) or not stat.S_ISREG(st.st_mode):
                raise ReleaseError(f'{target}: existing entry is not a regular file; refusing')
            if st.st_uid != os.getuid():
                raise ReleaseError(f'{target}: not owned by the current user')
            old, st = hash_path(target, MAX_BINARY)
            entry['before'] = dict(identity(st), sha256=old)
            entry['action'] = 'unchanged' if old == entry['new_sha256'] else 'replace'
            if st.st_nlink != 1:
                entry['warning'] = f'{st.st_nlink} hard links; other names keep the old content'
            files.append(entry)
        untouched[str(d)] = sorted(set(os.listdir(d)) - set(names))
    body = {
        'schema': PLAN_SCHEMA, 'created_utc': utc(), 'artifact_dir': str(art), 'manifest_sha256': rep['manifest_sha256'],
        'commit': rep['commit'], 'files': files, 'untouched_entries': untouched,
        'config_changes': [], 'runtime_actions': [], 'config_watch': config_fingerprint(CONFIG_WATCH if config_watch is None else config_watch),
        'live_destinations': sorted(str(d) for d in map(Path, dests) if is_live(Path(d))),
        'handoff': handoff(dests, names, {n: bins[n]['sha256'] for n in names}, proc=proc, runtime_dir=runtime_dir),
        'summary': {a: sum(f['action'] == a for f in files) for a in ('create', 'replace', 'unchanged')},
    }
    if managed_runtime:
        if config_watch is not None:
            raise ReleaseError('managed plans require the standard config watch')
        _managed_manifest(rep, body['files'])
        if not set(names) <= SHIPPED_BINARIES:
            raise ReleaseError('managed plans allow only shipped runtime binary names')
        dirs = {}
        for d in dests:
            fd = _managed_dest(d)
            try:
                dirs[str(d)] = _dir_ident(fd)
            finally:
                os.close(fd)
        body['managed_runtime'] = {'home': str(HOME), 'home_identity': _managed_home(),
                                   'dest_dirs': dirs, 'config_identity': _managed_configs()}
    body['plan_sha256'] = _canon_sha(body)
    return body


def _canon_sha(body):
    clean = {k: v for k, v in body.items() if k != 'plan_sha256'}
    return hashlib.sha256(json.dumps(clean, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def _proc_info(proc, pid):
    base = Path(proc) / str(pid)
    try:
        st = os.stat(base)
        exe = os.readlink(base / 'exe')
        stat_line = (base / 'stat').read_text()
        comm = (base / 'comm').read_text().strip()
    except OSError:
        return None
    fields = stat_line[stat_line.rfind(')') + 2:].split()
    deleted = exe.endswith(' (deleted)')
    return {'pid': pid, 'uid': st.st_uid, 'comm': comm, 'exe': exe[:-10] if deleted else exe, 'exe_deleted': deleted,
            'starttime_ticks': int(fields[19]) if len(fields) > 19 else None}


def handoff(dests, names, new_shas, proc='/proc', runtime_dir=None):
    """Read-only: which owned processes run the planned binaries, by lock pid + /proc exe; what a later, separately
    authorized handoff would need. Sends nothing and takes no locks (even a probing flock could race a starting instance)."""
    runtime_dir = Path(runtime_dir or os.environ.get('XDG_RUNTIME_DIR') or f'/run/user/{os.getuid()}')
    dest_paths = {str(Path(d) / n) for d in dests for n in names}
    comps = []
    for name in sorted(set(names) & set(THEME_COMPONENTS)):
        spec = THEME_COMPONENTS[name]
        holders = []
        for lock in spec['locks']:
            entry = {'lock': str(runtime_dir / lock)}
            try:
                raw, _ = read_bounded(runtime_dir / lock, 64)
                pid = int(raw.decode().strip())
            except (OSError, ValueError, ReleaseError):
                entry['state'] = 'no-readable-pid'
                holders.append(entry)
                continue
            info = _proc_info(proc, pid)
            entry['pid'] = pid
            if info is None:
                entry['state'] = 'stale-pid (no such process)'
            elif info['uid'] != os.getuid():
                entry['state'] = 'not-owned (different uid)'
            elif info['exe'] not in dest_paths:
                entry.update(state='not-owned (exe outside planned destinations)', exe=info['exe'])
            else:
                entry.update(state='owned', **{k: info[k] for k in ('comm', 'exe', 'exe_deleted', 'starttime_ticks')})
            holders.append(entry)
        comps.append({'component': name, 'instance_protocol': spec['instance'], 'ready_protocol': spec['ready'],
                      'theme_data': spec['theme_data'], 'new_sha256': new_shas[name], 'holders': holders,
                      'requires_authorization': 'root integration; this tool sends no signals and starts nothing'})
    return {'components': comps, 'never_touch': {n: why for n, why in NEVER_TOUCH.items() if n in names},
            'not_theme_components': sorted(set(names) - set(THEME_COMPONENTS) - set(NEVER_TOUCH)),
            'policy': 'no process restarts, kills, signals or name-based process matching; only exact lock pids cross-checked with /proc exe'}


# ------------------------------------------------------------------ publish / rollback (private destinations only)
#
# Every mutation goes through directory descriptors pinned up front: the private root is opened from '/' one component
# at a time with O_NOFOLLOW, each destination folder is opened from that root descriptor the same way, and files are
# addressed by single-component names relative to those descriptors. A parent renamed or replaced by a symlink after
# pinning cannot redirect an operation; publish detects it at the end, rollback refuses a folder that is not the one
# published into. Journal path/staged/backup values are names validated against fixed patterns, never trusted paths.

AT_FDCWD = -100
RENAME_NOREPLACE, RENAME_EXCHANGE = 1, 2
_libc = ctypes.CDLL(None, use_errno=True)
PLAN_KEYS = {'config_watch', 'schema', 'created_utc', 'artifact_dir', 'manifest_sha256', 'commit', 'files', 'untouched_entries',
             'config_changes', 'runtime_actions', 'live_destinations', 'handoff', 'summary', 'plan_sha256'}
FILE_KEYS = {'dest_dir', 'name', 'path', 'new_sha256', 'new_size', 'source', 'action', 'before', 'warning'}
DIR_FLAGS = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
FLAT_RE = re.compile(r'[a-z][a-z0-9_-]{0,63}')
AUX_RE = re.compile(r'\.[a-z][a-z0-9_-]{0,63}\.decebal-(release|restore|claim)-[0-9a-f]{12}')
BACKUP_RE = re.compile(r'[0-9]{3}-[a-z][a-z0-9_-]{0,63}')


def renameat2(odfd, old, ndfd, new, flags):
    if _libc.renameat2(odfd, os.fsencode(old), ndfd, os.fsencode(new), flags) != 0:
        e = ctypes.get_errno()
        raise OSError(e, os.strerror(e), str(new))


def _check_name(n, regex, what):
    if not isinstance(n, str) or not regex.fullmatch(n):
        raise ReleaseError(f'{what} {n!r} is not a single safe file name')
    return n


def _parts(path):
    p = str(path)
    parts = p.split('/')[1:]
    if not p.startswith('/') or not parts or any(x in ('', '.', '..') for x in parts):
        raise ReleaseError(f'{p!r} must be an absolute path without empty, "." or ".." components')
    return parts


def _walk(base_fd, parts, what):
    """Open base/parts[0]/parts[1]/... as directories, never following a symlink at any step."""
    fd = os.dup(base_fd)
    for part in parts:
        try:
            nfd = os.open(part, DIR_FLAGS, dir_fd=fd)
        except OSError as e:
            os.close(fd)
            raise ReleaseError(f'{what}: {part!r} is not a real directory reachable without symlinks ({e.strerror})')
        os.close(fd)
        fd = nfd
    return fd


def _open_abs_dir(path, what):
    top = os.open('/', os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    try:
        return _walk(top, _parts(path), what)
    finally:
        os.close(top)


def _dir_ident(fd):
    st = os.fstat(fd)
    return {'dev': st.st_dev, 'ino': st.st_ino}


def _same(st, ident):
    return st is not None and ident is not None and (st.st_dev, st.st_ino) == (ident['dev'], ident['ino'])


def _lstat_at(dfd, name):
    try:
        return os.stat(name, dir_fd=dfd, follow_symlinks=False)
    except FileNotFoundError:
        return None


def _hash_at(dfd, name, limit=MAX_BINARY):
    fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK, dir_fd=dfd)
    try:
        return hash_fd(fd, limit)
    finally:
        os.close(fd)


def _aux(name, kind):
    return f'.{name}.decebal-{kind}-{secrets.token_hex(6)}'


class Pins:
    """Descriptors for the private root, each destination folder (keyed by its planned path), artifact and journal."""

    def __init__(self, root_path):
        self.root_path, self.fds, self.dest, self.backups = Path(root_path), [], {}, None

    def keep(self, fd):
        self.fds.append(fd)
        return fd

    def close(self):
        for fd in self.fds:
            with contextlib.suppress(OSError):
                os.close(fd)
        self.fds = []


def _pin_root(root):
    root = Path(root)
    if is_live(root):
        raise ReleaseError(f'{root} is or contains a live user-local folder; live publishing is not authorized in this increment')
    pins = Pins(root)
    try:
        pins.root_fd = pins.keep(_open_abs_dir(root, f'private root {root}'))
        st = os.fstat(pins.root_fd)
        if st.st_uid != os.getuid() or st.st_mode & 0o077:
            raise ReleaseError(f'private root {root} must be owned by you with mode 0700')
        pins.root_ident = _dir_ident(pins.root_fd)
    except BaseException:
        pins.close()
        raise
    return pins


def _private_root_check(root):
    _pin_root(root).close()
    return Path(root)


def _pin_dest(pins, d, expect=None):
    parts, rparts = _parts(d), _parts(pins.root_path)
    if len(parts) <= len(rparts) or parts[:len(rparts)] != rparts or is_live(Path(d)):
        raise ReleaseError(f'destination {d} is not inside the private root {pins.root_path}')
    fd = pins.keep(_walk(pins.root_fd, parts[len(rparts):], f'destination {d}'))
    st = os.fstat(fd)
    if st.st_uid != os.getuid() or st.st_mode & 0o022:
        raise ReleaseError(f'destination {d} must be owned by you and not group/other writable')
    ident = _dir_ident(fd)
    if expect is not None and ident != {'dev': expect.get('dev'), 'ino': expect.get('ino')}:
        raise ReleaseError(f'destination {d} is not the folder that was published into (replaced since publish); refusing')
    pins.dest[str(d)] = (fd, ident)
    return fd


def _managed_home():
    fd = _open_abs_dir(HOME, 'managed HOME')
    try:
        st = os.fstat(fd)
        if st.st_uid != os.getuid() or st.st_mode & 0o022:
            raise ReleaseError('managed HOME must be owned and not group/other writable')
        return _dir_ident(fd)
    finally:
        os.close(fd)


def _managed_dest(d, expect=None):
    # Deliberately independent of the private-root guard: no parent/child paths are eligible.
    if str(d) not in {str(HOME / '.local/share/rdwm-decebal/bin'),
                      str(HOME / '.local/share/rdwm-macshell/bin')}:
        raise ReleaseError(f'{d}: not an exact managed runtime destination')
    _managed_home()
    _dest_dir_check(Path(d))
    fd = _open_abs_dir(d, 'managed destination')
    st = os.fstat(fd)
    if st.st_uid != os.getuid() or st.st_mode & 0o022 or (expect is not None and _dir_ident(fd) != expect):
        os.close(fd)
        raise ReleaseError(f'{d}: managed destination ownership or identity changed')
    return fd


def _managed_configs():
    out = {}
    for path in CONFIG_WATCH:
        # Validate existing ancestors even when the config itself is absent.
        parent = Path(path).parent
        while not parent.exists():
            parent = parent.parent
        fd = _open_abs_dir(parent, 'managed config parent')
        os.close(fd)
        try:
            digest, st = hash_path(path, MAX_RECORD)
            out[str(path)] = dict(identity(st), sha256=digest)
        except FileNotFoundError:
            out[str(path)] = None
    return out


def _managed_manifest(rep, files):
    names = set(rep['binaries'])
    if names == SHIPPED_BINARIES | {'examples/launchapp'}:
        return
    # The preview producer and consumer are one explicitly supported follow-up unit. Every file still comes from
    # this verified, clean, sealed release; no base binary is relabelled as belonging to the follow-up commit.
    preview_pair = {'decsettings', 'decthemectl'}
    if names != preview_pair:
        raise ReleaseError('managed publication requires the sealed standard shipped binary set or the exact preview follow-up pair')
    per_dest = {}
    for entry in files:
        per_dest.setdefault(entry['dest_dir'], set()).add(entry['name'])
    if not per_dest or any(selected != preview_pair for selected in per_dest.values()):
        raise ReleaseError('preview follow-up requires both decsettings and decthemectl in every selected destination')


def _managed_plan(p):
    m = p.get('managed_runtime')
    if not isinstance(m, dict) or set(m) != {'home', 'home_identity', 'dest_dirs', 'config_identity'}:
        raise ReleaseError('managed publication requires a managed-runtime plan')
    if m['home'] != str(HOME) or m['home_identity'] != _managed_home():
        raise ReleaseError('managed HOME identity changed')
    try:
        age = time.time() - calendar.timegm(time.strptime(p['created_utc'], '%Y-%m-%dT%H:%M:%SZ'))
    except (ValueError, TypeError):
        raise ReleaseError('invalid managed plan timestamp')
    if not 0 <= age <= MANAGED_MAX_AGE:
        raise ReleaseError('managed plan is stale or future dated; make a fresh plan')
    if set(p['config_watch']) != set(map(str, CONFIG_WATCH)) or m['config_identity'] != _managed_configs():
        raise ReleaseError('managed config identity changed since planning')
    if set(m['dest_dirs']) != {f['dest_dir'] for f in p['files']}:
        raise ReleaseError('managed plan destination set disagrees with files')
    return m


def _private_parent(pins, path):
    parts, root = _parts(path), _parts(pins.root_path)
    if len(parts) <= len(root) or parts[:len(root)] != root:
        raise ReleaseError('managed journal and lock must be strictly below the private root')
    return pins.keep(_walk(pins.root_fd, parts[len(root):-1], 'managed private parent'))


def _moved(pins):
    """Planned paths that no longer lead (symlink-free, from '/') to the pinned folders."""
    out = []
    paths = [(str(pins.root_path), pins.root_ident)] + [(p, i) for p, (_, i) in pins.dest.items()]
    for path, ident in paths + getattr(pins, 'extra_paths', []):
        try:
            fd = _open_abs_dir(path, path)
        except ReleaseError:
            out.append(path)
            continue
        try:
            if _dir_ident(fd) != ident:
                out.append(path)
        finally:
            os.close(fd)
    return out


def _copy_verified(sdfd, sname, ddfd, dname, want_sha, mode, on_create=None, fault_point=None, mtime_ns=None, **ctx):
    """Exclusive-create ddfd/dname, report its identity via on_create before writing, stream sdfd/sname into it, fsync,
    then re-read it from disk and require want_sha. Returns the written file's stat."""
    sfd = os.open(sname, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK, dir_fd=sdfd)
    try:
        if not stat.S_ISREG(os.fstat(sfd).st_mode):
            raise ReleaseError(f'{sname}: copy source is not a regular file')
        dfd = os.open(dname, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=ddfd)
        try:
            written = os.fstat(dfd)
            if on_create:
                on_create(written)
            h = hashlib.sha256()
            while True:
                b = os.read(sfd, 1 << 20)
                if not b:
                    break
                h.update(b)
                view = memoryview(b)
                while view:
                    view = view[os.write(dfd, view):]
            if fault_point:
                _fault(fault_point, fd=dfd, name=dname, **ctx)
            os.fchmod(dfd, mode)
            if mtime_ns is not None:
                os.utime(dfd, ns=(mtime_ns, mtime_ns))
            os.fsync(dfd)
        finally:
            os.close(dfd)
    finally:
        os.close(sfd)
    if h.hexdigest() != want_sha:
        raise ReleaseError(f'{sname}: source digest {h.hexdigest()} != {want_sha}')
    got, st = _hash_at(ddfd, dname)
    if not _same(st, {'dev': written.st_dev, 'ino': written.st_ino}):
        raise ReleaseError(f'{dname}: copy destination was replaced externally')
    if got != want_sha:
        raise ReleaseError(f'{dname}: written copy digest {got} != {want_sha} (partial or altered write)')
    return st


def _claim(dfd, name, ident, sha, base):
    """Remove dfd/name only if it is exactly `ident` (and `sha`): first atomically rename it to a fresh claim name, then
    check what was claimed. A foreign entry gets its name back (or, if that was re-taken meanwhile, stays under the
    claim name). Returns ('removed'|'absent'|'conflict', preserved name or None)."""
    claim = _aux(base, 'claim')
    try:
        renameat2(dfd, name, dfd, claim, RENAME_NOREPLACE)
    except FileNotFoundError:
        return 'absent', None
    st = _lstat_at(dfd, claim)
    ok = st is not None and stat.S_ISREG(st.st_mode) and _same(st, ident)
    if ok and sha is not None:
        ok = _hash_at(dfd, claim)[0] == sha
    if ok:
        os.unlink(claim, dir_fd=dfd)
        return 'removed', None
    try:
        renameat2(dfd, claim, dfd, name, RENAME_NOREPLACE)
        return 'conflict', name
    except FileExistsError:
        return 'conflict', claim


class Journal:
    """journal.json is replaced whole (write tmp, fsync, rename, fsync dir) inside the pinned journal folder after every
    step. During rollback a failed journal write (e.g. ENOSPC) does not stop the restore: on-disk identities decide each
    file, and the last error is kept."""

    def __init__(self, fd, path, data):
        self.fd, self.dir, self.data, self.lenient, self.unsaved = fd, Path(path), data, False, None

    def save(self):
        tmp = f'.journal.{secrets.token_hex(4)}.tmp'
        try:
            _fault('journal_save', journal=self)
            fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=self.fd)
            with os.fdopen(fd, 'w') as f:
                f.write(json.dumps(self.data, indent=2, sort_keys=True) + '\n')
                f.flush()
                os.fsync(f.fileno())
            os.rename(tmp, 'journal.json', src_dir_fd=self.fd, dst_dir_fd=self.fd)
            os.fsync(self.fd)
            self.unsaved = None
        except OSError as e:
            with contextlib.suppress(OSError):
                os.unlink(tmp, dir_fd=self.fd)
            if not self.lenient:
                raise
            self.unsaved = str(e)

    def file(self, i, **kw):
        self.data['files'][i].update(kw)
        self.save()


def _lock(path, parent_fd=None):
    fd = os.open(Path(path).name if parent_fd is not None else path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=parent_fd)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        os.close(fd)
        raise ReleaseError(f'{path} is held by another publish')
    return fd


def _busy(dest_dirs, proc='/proc'):
    """Owned processes whose executable lives in a destination folder (refuse rather than swap under them)."""
    out = []
    for e in os.scandir(proc):
        if e.name.isdigit():
            info = _proc_info(proc, int(e.name))
            if info and info['uid'] == os.getuid() and any(os.path.dirname(info['exe']) == d for d in dest_dirs):
                out.append({'pid': info['pid'], 'exe': info['exe']})
    return out


def load_plan(path):
    p, _ = load_json(path, MAX_PLAN)
    if not isinstance(p, dict) or set(p) not in (PLAN_KEYS, PLAN_KEYS | {'managed_runtime'}) or p['schema'] != PLAN_SCHEMA:
        raise ReleaseError('plan has unknown or missing keys / schema')
    if p['plan_sha256'] != _canon_sha(p):
        raise ReleaseError('plan_sha256 does not match plan body (edited plan)')
    if p['config_changes'] != [] or p['runtime_actions'] != []:
        raise ReleaseError('plan requests config or runtime changes; this tool performs neither')
    if not isinstance(p['files'], list) or len({f.get('path') for f in p['files'] if isinstance(f, dict)}) != len(p['files']):
        raise ReleaseError('plan files must be a list of distinct paths')
    for f in p['files']:
        if not isinstance(f, dict) or not set(f) <= FILE_KEYS or f.get('action') not in ('create', 'replace', 'unchanged'):
            raise ReleaseError(f'unknown plan entry {str(f)[:200]}')
        _check_name(f.get('name'), FLAT_RE, 'plan file name')
        if f.get('path') != f'{f.get("dest_dir")}/{f["name"]}':
            raise ReleaseError(f'plan path {f.get("path")!r} is not dest_dir/name')
        if (f['action'] == 'create') != (f.get('before') is None):
            raise ReleaseError(f'{f["path"]}: action {f["action"]} inconsistent with its recorded before-identity')
    return p


def publish(plan_path, private_root, journal_dir, lock_path, proc='/proc', managed_runtime=False):
    p = load_plan(plan_path)
    if 'managed_runtime' in p and not managed_runtime:
        raise ReleaseError('managed-runtime plan requires explicit --managed-runtime')
    managed = _managed_plan(p) if managed_runtime else None
    jd = Path(journal_dir)
    jparts = _parts(jd)
    pins = _pin_root(private_root)
    try:
        rep = verify(p['artifact_dir'])
        if not rep['ok'] or rep['manifest_sha256'] != p['manifest_sha256']:
            raise ReleaseError('artifact no longer verifies or its manifest changed since planning: ' + '; '.join(rep['problems']))
        if managed_runtime:
            _managed_manifest(rep, p['files'])
            if p['commit'] != rep['commit']:
                raise ReleaseError('managed plan commit differs from manifest')
            if not _is_within(jd, pins.root_path) or not _is_within(Path(lock_path), pins.root_path):
                raise ReleaseError('managed journal and lock must be inside the private root')
            # Validate the lock parent through no-follow descriptors before opening the lock.
            lock_parent = _private_parent(pins, lock_path)
            home_fd = pins.keep(_open_abs_dir(HOME, 'managed HOME'))
            if _dir_ident(home_fd) != managed['home_identity']:
                raise ReleaseError('managed HOME changed during preflight')
            pins.extra_paths = [(str(HOME), managed['home_identity'])]
        dest_dirs = sorted({f['dest_dir'] for f in p['files']})
        for d in dest_dirs:
            if managed_runtime:
                fd = pins.keep(_managed_dest(d, managed['dest_dirs'][d]))
                pins.dest[d] = (fd, _dir_ident(fd))
            else:
                _pin_dest(pins, d)
        for f in p['files']:
            b = rep['binaries'].get(f['name'])
            if b is None or b['sha256'] != f['new_sha256'] or b['size'] != f['new_size'] or f['source'] != str(Path(p['artifact_dir']) / 'bin' / f['name']):
                raise ReleaseError(f'{f["name"]}: plan digest/source disagrees with the verified manifest')
        if is_live(jd) or jd == pins.root_path or any(_is_within(jd, Path(d)) or _is_within(Path(d), jd) for d in dest_dirs):
            raise ReleaseError('journal must be a new folder outside every destination and live folder')
        busy = [] if managed_runtime else _busy(dest_dirs, proc)
        if busy:
            raise ReleaseError(f'destination executables are in use: {busy}')
        art_fd = pins.keep(_open_abs_dir(Path(p['artifact_dir']) / 'bin', 'artifact bin/'))
        jparent = _private_parent(pins, jd) if managed_runtime else pins.keep(_open_abs_dir('/' + '/'.join(jparts[:-1]), 'journal parent') if len(jparts) > 1 else os.open('/', DIR_FLAGS))
        lock = _lock(lock_path, lock_parent if managed_runtime else None)
        try:
            if managed_runtime:
                _managed_plan(p)
            os.mkdir(jparts[-1], 0o700, dir_fd=jparent)
            jfd = pins.keep(os.open(jparts[-1], DIR_FLAGS, dir_fd=jparent))
            os.mkdir('backups', 0o700, dir_fd=jfd)
            pins.backups = pins.keep(os.open('backups', DIR_FLAGS, dir_fd=jfd))
            j = Journal(jfd, jd, {
                'schema': JOURNAL_SCHEMA, 'plan_sha256': p['plan_sha256'], 'manifest_sha256': p['manifest_sha256'],
                'commit': p['commit'], 'artifact_dir': p['artifact_dir'], 'private_root': str(pins.root_path),
                'dest_dirs': {d: ident for d, (_, ident) in pins.dest.items()},
                'started_utc': utc(), 'state': 'preflight', 'restarts_sent': False, 'config_changes': [],
                'config_before': p['config_watch'],
                'files': [{'path': f['path'], 'dest_dir': f['dest_dir'], 'name': f['name'], 'action': f['action'],
                           'before': f['before'], 'after': f['new_sha256'], 'backup': None, 'staged': None,
                           'staged_identity': None, 'committed_identity': None, 'state': 'pending'} for f in p['files']]})
            if managed_runtime:
                j.data['managed_runtime'] = {'home': str(HOME), 'home_identity': managed['home_identity']}
            j.save()
            # Stale-destination and config preflight for every file before the first change.
            drift = sorted(k for k, v in config_fingerprint(p['config_watch']).items() if v != p['config_watch'][k])
            if managed_runtime and _managed_configs() != managed['config_identity']:
                drift = drift or ['managed config identity']
            for f in p['files']:
                stale = f'config changed since planning: {drift}' if drift else _stale(f, pins.dest[f['dest_dir']][0])
                if stale:
                    j.data.update(state='aborted_before_changes', error=stale, finished_utc=utc())
                    j.save()
                    return j.data
            j.data['state'] = 'publishing'
            j.save()
            try:
                for i, f in enumerate(p['files']):
                    _publish_one(j, pins, i, f, art_fd)
                for f in p['files']:
                    got, _ = _hash_at(pins.dest[f['dest_dir']][0], f['name'])
                    if got != f['new_sha256']:
                        raise ReleaseError(f'{f["path"]}: post-publish digest {got} != planned')
                moved = _moved(pins)
                if moved:
                    raise ReleaseError(f'destination folder replaced during publish: {moved}; rolling back the pinned folders')
                after = config_fingerprint(p['config_watch'])
                if managed_runtime and _managed_configs() != managed['config_identity']:
                    raise ReleaseError('managed config changed during publish; preserving config and rolling back binaries')
                j.data.update(state='committed', finished_utc=utc(), config_after=after, configs_match_plan=after == p['config_watch'])
                j.save()
            except (ReleaseError, OSError) as e:
                j.lenient = True
                j.data.update(state='rolling_back', error=str(e))
                j.save()
                _rollback(j, pins)
            return j.data
        finally:
            os.close(lock)
    finally:
        pins.close()


def _stale(f, dfd):
    st = _lstat_at(dfd, f['name'])
    if st is None:
        return None if f['before'] is None else f'{f["path"]} disappeared since planning'
    if f['before'] is None:
        return f'{f["path"]} appeared since planning'
    if not stat.S_ISREG(st.st_mode) or not _same(st, f['before']) or st.st_size != f['before']['size'] \
            or st.st_mtime_ns != f['before']['mtime_ns']:
        return f'{f["path"]} identity changed since planning'
    return None if _hash_at(dfd, f['name'])[0] == f['before']['sha256'] else f'{f["path"]} content changed since planning'


def _publish_one(j, pins, i, f, art_fd):
    dfd, name = pins.dest[f['dest_dir']][0], f['name']
    if f['action'] == 'unchanged':
        stale = _stale(f, dfd)
        if stale:
            raise ReleaseError(stale)
        j.file(i, state='unchanged')
        return
    if f['action'] == 'replace':
        if not _same(_lstat_at(dfd, name), f['before']):
            raise ReleaseError(f'{f["path"]} replaced externally before backup')
        backup = f'{i:03d}-{name}'
        _copy_verified(dfd, name, pins.backups, backup, f['before']['sha256'], 0o400)
        j.file(i, backup=backup, state='backed_up')
    staged = _aux(name, 'release')
    j.file(i, staged=staged, state='staging')
    mode = f['before']['mode'] if f['before'] else 0o755
    _copy_verified(art_fd, name, dfd, staged, f['new_sha256'], mode, fault_point='stage_write', index=i,
                   on_create=lambda st: j.file(i, staged_identity={'dev': st.st_dev, 'ino': st.st_ino}))
    os.fsync(dfd)
    j.file(i, state='staged')
    _fault('before_commit', index=i, path=f['path'])
    if f['action'] == 'create':
        try:
            renameat2(dfd, staged, dfd, name, RENAME_NOREPLACE)
        except FileExistsError:
            raise ReleaseError(f'{f["path"]} appeared externally before commit')
    else:
        renameat2(dfd, staged, dfd, name, RENAME_EXCHANGE)
        _fault('after_swap', index=i, path=f['path'])
        displaced = _lstat_at(dfd, staged)
        ok = displaced is not None and stat.S_ISREG(displaced.st_mode) and _same(displaced, f['before'])
        if ok:
            ok = _hash_at(dfd, staged)[0] == f['before']['sha256']
        if not ok:
            renameat2(dfd, staged, dfd, name, RENAME_EXCHANGE)   # put the external file back where it was
            raise ReleaseError(f'{f["path"]} was replaced externally before commit (stale destination); left in place')
        res, where = _claim(dfd, staged, f['before'], f['before']['sha256'], name)
        if res != 'removed':
            raise ReleaseError(f'{f["path"]}: displaced original changed before removal; preserved as {where}')
    os.fsync(dfd)
    j.file(i, committed_identity=dict(j.data['files'][i]['staged_identity']), state='committed')
    _fault('after_commit', index=i, path=f['path'])


def _rollback(j, pins):
    j.lenient = True
    for i in reversed(range(len(j.data['files']))):
        f = j.data['files'][i]
        try:
            _rollback_one(j, pins, i, f)
        except (ReleaseError, OSError) as e:
            j.file(i, state='rollback_failed', error=str(e))
    bad = [f['path'] for f in j.data['files'] if f['state'] in ('rollback_failed', 'rollback_conflict')]
    j.data.update(state='rollback_incomplete' if bad else 'rolled_back', unresolved=bad, finished_utc=utc())
    j.lenient = False
    j.save()


def _drop_staged(dfd, f):
    """Remove our staged copy only when it is provably ours; anything else at that name is preserved and reported."""
    staged = f.get('staged')
    if not staged or _lstat_at(dfd, staged) is None:
        return {}
    if f.get('staged_identity') is None:
        return {'preserved': staged, 'note': 'staged entry of unrecorded identity left in place'}
    res, where = _claim(dfd, staged, f['staged_identity'], None, f['name'])
    return {} if res in ('removed', 'absent') else {'preserved': where, 'note': 'entry at the staged name was not ours; preserved'}


def _rollback_one(j, pins, i, f):
    dfd, name = pins.dest[f['dest_dir']][0], f['name']
    if f['state'] in ('pending', 'backed_up', 'unchanged', 'rolled_back', 'rollback_conflict'):
        if f['state'] not in ('rolled_back', 'rollback_conflict'):
            j.file(i, state='rolled_back' if f['state'] != 'unchanged' else 'unchanged')
        return
    if f['state'] in ('staging', 'staged'):
        # The journal step before the swap ran; whether the swap itself did is decided from on-disk identities.
        if _same(_lstat_at(dfd, name), f['staged_identity']):
            f['committed_identity'] = f['staged_identity']
            f['state'] = 'committed'
        else:
            j.file(i, state='rolled_back', **_drop_staged(dfd, f))
            return
    if f['state'] != 'committed':
        raise ReleaseError(f'unexpected journal state {f["state"]}')
    cur = _lstat_at(dfd, name)
    if cur is None:
        if f['action'] == 'create':          # already back to the planned "absent" state (earlier unjournaled rollback)
            j.file(i, state='rolled_back')
        else:
            j.file(i, state='rollback_conflict', error=f'{f["path"]} vanished after commit; not recreated')
        return
    cur_sha = _hash_at(dfd, name)[0] if stat.S_ISREG(cur.st_mode) else None
    if f['action'] == 'replace' and cur_sha == f['before']['sha256']:
        j.file(i, state='rolled_back', note='already holds the pre-publish content')   # earlier unjournaled restore
        return
    if not _same(cur, f['committed_identity']) or cur_sha != f['after']:
        j.file(i, state='rollback_conflict', error=f'{f["path"]} was replaced externally after commit; left in place')
        return
    _fault('rollback_prechecked', index=i, name=name)
    if f['action'] == 'create':
        res, where = _claim(dfd, name, f['committed_identity'], f['after'], name)
        if res == 'conflict':
            j.file(i, state='rollback_conflict', preserved=where,
                   error=f'{f["path"]} was replaced externally during rollback; preserved as {where}')
            return
        os.fsync(dfd)
        j.file(i, state='rolled_back')
        return
    before = f['before']
    staged, src = f.get('staged'), None
    if staged:
        sst = _lstat_at(dfd, staged)
        if sst is not None and stat.S_ISREG(sst.st_mode) and _same(sst, before) and _hash_at(dfd, staged)[0] == before['sha256']:
            src = staged     # a crash left the displaced original at the staged name: swap it straight back
    if src is None:
        if not f['backup']:
            raise ReleaseError(f'{f["path"]}: no backup recorded')
        restore = _aux(name, 'restore')
        j.file(i, restore=restore)
        _fault('rollback_restore', index=i, backup=str(j.dir / 'backups' / f['backup']))
        restore_ident = None
        def created(st):
            nonlocal restore_ident
            restore_ident = {'dev': st.st_dev, 'ino': st.st_ino}
            j.file(i, restore_identity=restore_ident)
        try:
            rst = _copy_verified(pins.backups, f['backup'], dfd, restore, before['sha256'], before['mode'],
                                 on_create=created, mtime_ns=before['mtime_ns'])
        except BaseException:
            if restore_ident is not None:
                with contextlib.suppress(OSError):
                    _claim(dfd, restore, restore_ident, None, name)
            raise
        src, src_ident = restore, {'dev': rst.st_dev, 'ino': rst.st_ino}
    else:
        src_ident = before
    renameat2(dfd, src, dfd, name, RENAME_EXCHANGE)
    out = _lstat_at(dfd, src)
    ours = out is not None and stat.S_ISREG(out.st_mode) and _same(out, f['committed_identity']) \
        and _hash_at(dfd, src)[0] == f['after']
    if not ours:
        renameat2(dfd, src, dfd, name, RENAME_EXCHANGE)   # the racing entry goes back to its name untouched
        extra = {}
        if src != staged:
            res, where = _claim(dfd, src, src_ident, before['sha256'], name)
            extra = {} if res == 'removed' else {'preserved': where}
        j.file(i, state='rollback_conflict', error=f'{f["path"]} was replaced externally during rollback; left in place', **extra)
        return
    res, where = _claim(dfd, src, f['committed_identity'], f['after'], name)
    os.fsync(dfd)
    j.file(i, state='rolled_back', restored_from=src if src == staged else f['backup'],
           **({} if res == 'removed' else {'preserved': where}))


def _read_journal(jfd):
    fd = os.open('journal.json', os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK, dir_fd=jfd)
    try:
        st = os.fstat(fd)
        if not stat.S_ISREG(st.st_mode) or st.st_size > MAX_PLAN:
            raise ReleaseError('journal.json is not a bounded regular file')
        return json.loads(os.read(fd, MAX_PLAN + 1))
    finally:
        os.close(fd)


def rollback(journal_dir, lock_path, managed_runtime=False):
    jd = Path(journal_dir)
    if is_live(jd):
        raise ReleaseError(f'journal {jd} is inside a live folder')
    jfd = _open_abs_dir(jd, f'journal {jd}')
    pins = None
    try:
        st = os.fstat(jfd)
        if st.st_uid != os.getuid() or st.st_mode & 0o022:
            raise ReleaseError(f'journal {jd} must be owned by you and not group/other writable')
        data = _read_journal(jfd)
        if not isinstance(data, dict) or data.get('schema') != JOURNAL_SCHEMA:
            raise ReleaseError('not a decebal-release journal')
        managed = data.get('managed_runtime')
        if managed is not None:
            if not managed_runtime:
                raise ReleaseError('managed rollback requires explicit --managed-runtime')
            if managed != {'home': str(HOME), 'home_identity': _managed_home()}:
                raise ReleaseError('managed journal HOME identity invalid')
        elif managed_runtime:
            raise ReleaseError('private journal cannot authorize managed rollback')
        pins = _pin_root(data.get('private_root'))
        if managed is not None and (not _is_within(jd, pins.root_path) or not _is_within(Path(lock_path), pins.root_path)):
            raise ReleaseError('managed journal and lock must be inside the private root')
        dests = data.get('dest_dirs')
        if not isinstance(dests, dict) or not dests:
            raise ReleaseError('journal records no pinned destination folders')
        for d, ident in dests.items():
            if managed is not None:
                fd = pins.keep(_managed_dest(d, ident if isinstance(ident, dict) else {}))
                pins.dest[d] = (fd, _dir_ident(fd))
            else:
                _pin_dest(pins, d, expect=ident if isinstance(ident, dict) else {})
        if managed is not None:
            _private_parent(pins, jd)
        for f in data['files']:
            _check_name(f.get('name'), FLAT_RE, 'journal file name')
            if managed is not None and f['name'] not in SHIPPED_BINARIES:
                raise ReleaseError('journal file is not a shipped managed binary')
            if f.get('dest_dir') not in dests or f.get('path') != f'{f["dest_dir"]}/{f["name"]}':
                raise ReleaseError(f'journal path {f.get("path")!r} is not one of its pinned folders + name')
            for key, regex in (('staged', AUX_RE), ('restore', AUX_RE), ('backup', BACKUP_RE)):
                if f.get(key) is not None:
                    _check_name(f[key], regex, f'journal {key} entry')
        pins.backups = pins.keep(os.open('backups', DIR_FLAGS, dir_fd=jfd))
        lock_parent = _private_parent(pins, lock_path) if managed is not None else None
        lock = _lock(lock_path, lock_parent)
        try:
            j = Journal(jfd, jd, data)
            if data['state'] in ('rolled_back', 'aborted_before_changes'):
                return data
            j.data['state'] = 'rolling_back'
            j.save()
            _rollback(j, pins)
            return j.data
        finally:
            os.close(lock)
    finally:
        if pins:
            pins.close()
        os.close(jfd)


# ------------------------------------------------------------------ exact child helper

def run_exact(path, args, expected_sha, env, timeout, cwd=None, max_output=64 << 10):
    """Run exactly the inode whose digest was verified (exec through /proc/self/fd), with an explicit environment only,
    in a new session, bounded by `timeout`. On timeout only that new process group is killed. Reports env *keys* only."""
    if not isinstance(env, dict):
        raise ReleaseError('run_exact needs an explicit environment dict; the caller environment is never inherited')
    fd = _open_nofollow(path)
    try:
        got, st = hash_fd(fd, MAX_BINARY)
        if got != expected_sha:
            raise ReleaseError(f'{path}: sha256 {got} != expected {expected_sha}; not executed')
        if st.st_mode & 0o022:
            raise ReleaseError(f'{path}: group/other writable; not executed')
        with tempfile.TemporaryFile() as out, tempfile.TemporaryFile() as err:
            t0 = time.monotonic()
            child = subprocess.Popen([str(path), *args], executable=f'/proc/self/fd/{fd}', pass_fds=(fd,), env=env, cwd=cwd,
                                     stdin=subprocess.DEVNULL, stdout=out, stderr=err, start_new_session=True)
            timed_out = False
            try:
                rc = child.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                timed_out = True
                # child is unreaped, so its pid (== its new pgid) cannot have been reused.
                with contextlib.suppress(ProcessLookupError):
                    os.killpg(child.pid, 9)
                rc = child.wait()
            res = {'argv0': str(path), 'sha256': got, 'returncode': rc, 'timed_out': timed_out, 'pid': child.pid,
                   'wall_s': round(time.monotonic() - t0, 3), 'env_keys': sorted(env)}
            for name, fh in (('stdout', out), ('stderr', err)):
                fh.seek(0)
                res[name] = fh.read(max_output).decode('utf-8', 'replace')
            return res
    finally:
        os.close(fd)


# ------------------------------------------------------------------ CLI

def _emit(obj, out=None):
    text = json.dumps(obj, indent=2, sort_keys=True) + '\n'
    if out:
        fd = os.open(out, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
        with os.fdopen(fd, 'w') as f:
            f.write(text)
    else:
        sys.stdout.write(text)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest='cmd', required=True)
    v = sub.add_parser('verify')
    v.add_argument('artifact')
    v.add_argument('--expect-commit')
    v.add_argument('--assets')
    p = sub.add_parser('plan')
    p.add_argument('artifact')
    p.add_argument('--dest', action='append', required=True)
    p.add_argument('--only', action='append')
    p.add_argument('--expect-commit')
    p.add_argument('--out')
    p.add_argument('--managed-runtime', action='store_true')
    u = sub.add_parser('publish')
    u.add_argument('--plan', required=True)
    u.add_argument('--private-root', required=True)
    u.add_argument('--journal', required=True)
    u.add_argument('--lock', required=True)
    u.add_argument('--managed-runtime', action='store_true')
    r = sub.add_parser('rollback')
    r.add_argument('--journal', required=True)
    r.add_argument('--lock', required=True)
    r.add_argument('--managed-runtime', action='store_true')
    a = ap.parse_args(argv)
    try:
        if a.cmd == 'verify':
            rep = verify(a.artifact, a.expect_commit, a.assets)
            _emit(rep)
            return 0 if rep['ok'] else 1
        if a.cmd == 'plan':
            _emit(plan(a.artifact, a.dest, a.only, a.expect_commit, managed_runtime=a.managed_runtime), a.out)
            return 0
        if a.cmd == 'publish':
            res = publish(a.plan, a.private_root, a.journal, a.lock, managed_runtime=a.managed_runtime)
        else:
            res = rollback(a.journal, a.lock, managed_runtime=a.managed_runtime)
        _emit({k: res.get(k) for k in ('state', 'error', 'unresolved', 'commit', 'plan_sha256')} | {'journal': a.journal})
        return 0 if res['state'] in ('committed', 'rolled_back') else 1
    except (ReleaseError, OSError, ValueError) as e:
        print(f'REFUSE: {e}', file=sys.stderr)
        return 2
    return 2


if __name__ == '__main__':
    sys.exit(main())
