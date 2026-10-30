#!/usr/bin/env python3
"""Retire exact, source-managed installed artwork; dry-run unless --apply is given.

Never changes desktop configuration. Pass every referenced image with --keep-path.
Only raw manifests known to Git are eligible; unknown files and edits stay untouched.
"""
from __future__ import annotations

import argparse
import contextlib
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import secrets
import stat
import subprocess

LIMIT = 64 * 1024 * 1024


class RetirementError(RuntimeError):
    def __init__(self, message, report=None):
        super().__init__(message)
        self.report = report or {}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def relative(value):
    if not isinstance(value, str):
        raise RetirementError("Relative paths must be strings")
    p = PurePosixPath(value)
    if p.is_absolute() or not p.parts or any(v in ('', '.', '..') for v in value.split('/')):
        raise RetirementError(f'Unsafe relative path: {value}')
    return p


@contextlib.contextmanager
def directory(path):
    """Open every directory component without following links, including ancestors."""
    path = Path(os.path.abspath(path))
    fd = os.open('/', os.O_RDONLY | os.O_DIRECTORY)
    try:
        for part in path.parts[1:]:
            next_fd = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
            os.close(fd)
            fd = next_fd
        yield fd
    finally:
        os.close(fd)


@contextlib.contextmanager
def parent(root, rel):
    rel = relative(str(rel))
    with directory(root) as root_fd:
        fd = os.dup(root_fd)
        try:
            for part in rel.parts[:-1]:
                next_fd = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
                os.close(fd)
                fd = next_fd
            yield fd, rel.name
        finally:
            os.close(fd)


def read(root, rel):
    with parent(root, rel) as (fd, name):
        file_fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=fd)
        try:
            info = os.fstat(file_fd)
            if not stat.S_ISREG(info.st_mode) or info.st_size > LIMIT:
                raise RetirementError(f'Not a bounded regular file: {rel}')
            with os.fdopen(os.dup(file_fd), 'rb') as stream:
                data = stream.read(LIMIT + 1)
            if len(data) > LIMIT:
                raise RetirementError(f'File grew beyond limit: {rel}')
            return data, stat.S_IMODE(info.st_mode)
        finally:
            os.close(file_fd)


def replace(root, rel, data, mode):
    with parent(root, rel) as (fd, name):
        temporary = '.retire-' + secrets.token_hex(12)
        file_fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=fd)
        try:
            with os.fdopen(file_fd, 'wb') as stream:
                stream.write(data)
                stream.flush()
                os.fchmod(stream.fileno(), mode)
                os.fsync(stream.fileno())
            os.rename(temporary, name, src_dir_fd=fd, dst_dir_fd=fd)
        finally:
            try:
                os.unlink(temporary, dir_fd=fd)
            except FileNotFoundError:
                pass


def remove(root, rel):
    with parent(root, rel) as (fd, name):
        os.unlink(name, dir_fd=fd)


def known_manifests(repo, ident, baseline):
    result = set()
    for ref in dict.fromkeys(('HEAD', 'eabb41b', 'd8f9f61', baseline)):
        if not isinstance(ref, str) or not re.fullmatch(r'[a-zA-Z0-9._/-]+', ref):
            raise RetirementError('Invalid audit baseline revision')
        out = subprocess.run(['git', '-C', str(repo), 'show', f'{ref}:themes/{ident}/theme.toml'], capture_output=True, timeout=10, check=False)
        if out.returncode == 0:
            result.add(out.stdout)
    return result


def validate_roots(repo, themes, backup):
    repo, themes = Path(os.path.abspath(repo)), Path(os.path.abspath(themes))
    with directory(repo), directory(themes):
        pass
    forbidden = ('/usr', '/etc', '/var', '/opt', '/bin', '/sbin', '/lib', '/lib64', '/boot', '/dev', '/proc', '/sys')
    if themes == Path('/') or themes == Path.home() or any(themes == Path(p) or Path(p) in themes.parents for p in forbidden):
        raise RetirementError('Refusing a system directory or home root')
    if themes == repo or repo in themes.parents:
        raise RetirementError('Installed themes must be outside the source checkout')
    if backup is not None:
        backup = Path(os.path.abspath(backup))
        # The destination itself must not exist: this also forbids overwriting an old backup.
        if backup.exists() or backup.is_symlink():
            raise RetirementError('Backup directory must be a new path')
        with directory(backup.parent):
            pass
        if backup == themes or themes in backup.parents or backup in themes.parents:
            raise RetirementError('Backup and installed themes must be separate trees')
        if any(backup == Path(p) or Path(p) in backup.parents for p in forbidden):
            raise RetirementError('Refusing a system backup directory')
    return repo, themes, backup


def private_write(path, data):
    with path.open('xb') as stream:
        os.fchmod(stream.fileno(), 0o600)
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())


def record_result(backup, report):
    try:
        private_write(backup / 'result.json', (json.dumps(report, indent=2) + '\n').encode())
    except OSError as error:
        # Preserve the original transaction/rollback failure rather than masking it.
        report['result_record_error'] = str(error)


def retire(repo, themes, backup=None, keep_paths=(), apply=False):
    repo, themes, backup = validate_roots(repo, themes, backup)
    if apply and backup is None:
        raise RetirementError('--apply requires --backup-dir')
    audit = json.loads(read(repo, 'docs/generated-wallpaper-removal.json')[0])
    if audit.get('format') != 1:
        raise RetirementError('Unsupported retirement audit format')
    protected = {Path(os.path.abspath(p)) for p in keep_paths}
    # Resolve protected aliases for comparison only; mutation never follows those aliases.
    protected.update(p.resolve() for p in tuple(protected))
    report = {'apply': apply, 'themes_dir': str(themes), 'backup_dir': str(backup) if backup else None,
              'planned': [], 'skipped': [], 'changed': [], 'rollback_errors': []}
    operations = []
    seen = set()
    identities = set()
    for record in audit['themes']:
        ident = record['theme_id']
        if not isinstance(ident, str) or not re.fullmatch(r'[a-z0-9][a-z0-9._-]{0,63}', ident):
            raise RetirementError('Unsafe theme identifier', report)
        if ident in identities:
            raise RetirementError('Duplicate theme identifier', report)
        identities.add(ident)
        manifest = f'{ident}/theme.toml'
        try:
            current, mode = read(themes, manifest)
        except FileNotFoundError:
            report['skipped'].append({'path': manifest, 'reason': 'not installed'})
            continue
        except (OSError, RetirementError) as error:
            raise RetirementError(f'Unsafe installed package {ident}: {error}', report) from error
        desired = read(repo, f'themes/{manifest}')[0]
        if len(current) > 1024 * 1024 or current not in known_manifests(repo, ident, record.get('baseline_commit', audit['baseline_commit'])) | {desired}:
            report['skipped'].append({'path': manifest, 'reason': 'modified or unknown manifest; package preserved'})
            continue
        if current != desired:
            operations.append((manifest, current, mode, desired))
        for asset in record.get('removed', []):
            path = relative(asset['path'])
            if path.parts[0] == 'docs':
                # Hidden-alias audits also record source-only documentation shots.
                # They were never installed inside a theme package.
                continue
            if path.parts[:2] != ('themes', ident) or len(path.parts) < 3:
                raise RetirementError(f'Audit asset escapes package: {path}', report)
            rel = str(PurePosixPath(*path.parts[1:]))
            if rel in seen or rel == manifest:
                raise RetirementError(f'Duplicate or invalid asset: {rel}', report)
            seen.add(rel)
            try:
                data, asset_mode = read(themes, rel)
            except FileNotFoundError:
                continue
            except (OSError, RetirementError) as error:
                raise RetirementError(f'Unsafe installed asset {rel}: {error}', report) from error
            if themes / rel in protected:
                report['skipped'].append({'path': rel, 'reason': 'referenced/protected file'})
                continue
            if not isinstance(asset.get('sha256'), str) or not re.fullmatch(r'[0-9a-f]{64}', asset['sha256']) or not isinstance(asset.get('bytes'), int) or not 0 <= asset['bytes'] <= LIMIT:
                raise RetirementError(f'Invalid audit digest/size: {rel}', report)
            if digest(data) != asset['sha256'] or len(data) != asset['bytes']:
                report['skipped'].append({'path': rel, 'reason': 'modified asset preserved'})
                continue
            operations.append((rel, data, asset_mode, None))
    for rel, before, mode, after in operations:
        report['planned'].append({'path': rel, 'original_mode': oct(mode), 'operation': 'remove' if after is None else 'replace manifest', 'before_sha256': digest(before), 'after_sha256': digest(after) if after is not None else None})
    if not apply or not operations:
        return report
    try:
        backup.mkdir(mode=0o700)
        # Prepare and sync every original before touching any installed file.
        for rel, before, mode, _ in operations:
            dest = backup / rel
            dest.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
            private_write(dest, before)
        private_write(backup / 'transaction.json', (json.dumps(report, indent=2) + '\n').encode())
        dirs = [backup, *(p for p in backup.rglob('*') if p.is_dir())]
        for path in [*reversed(dirs), backup.parent]:
            with directory(path) as fd:
                os.fsync(fd)
    except OSError as error:
        report['error'] = str(error)
        report['phase'] = 'backup preparation; installed files untouched'
        raise RetirementError(f'Backup preparation failed: {error}', report) from error
    attempted = []
    try:
        for operation in operations:
            rel, before, mode, after = operation
            if read(themes, rel)[0] != before:
                raise RetirementError(f'File changed after planning: {rel}')
            attempted.append(operation)
            if after is None:
                remove(themes, rel)
            else:
                replace(themes, rel, after, mode)
            report['changed'].append(rel)
    except Exception as error:
        for rel, before, mode, after in reversed(attempted):
            try:
                try:
                    current = read(themes, rel)[0]
                except FileNotFoundError:
                    current = None
                if current == before:
                    continue
                if current != after:
                    raise RetirementError('concurrent change preserved; restore manually from backup')
                replace(themes, rel, before, mode)
            except Exception as rollback_error:
                report['rollback_errors'].append({'path': rel, 'error': str(rollback_error)})
        report['error'] = str(error)
        report['rolled_back'] = not report['rollback_errors']
        record_result(backup, report)
        raise RetirementError(f'Retirement failed: {error}; rollback failures: {len(report["rollback_errors"])}; originals: {backup}', report) from error
    record_result(backup, report)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--themes-dir', type=Path, required=True)
    parser.add_argument('--backup-dir', type=Path)
    parser.add_argument('--keep-path', type=Path, action='append', default=[])
    action = parser.add_mutually_exclusive_group()
    action.add_argument('--apply', action='store_true')
    action.add_argument('--dry-run', action='store_true', help='default; report only')
    args = parser.parse_args()
    try:
        print(json.dumps(retire(args.repo, args.themes_dir, args.backup_dir, args.keep_path, args.apply), indent=2))
        return 0
    except (RetirementError, OSError, ValueError, subprocess.SubprocessError) as error:
        print(json.dumps({'error': str(error), 'report': getattr(error, 'report', {})}, indent=2))
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
