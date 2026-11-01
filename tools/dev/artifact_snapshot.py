#!/usr/bin/env python3
"""Record/verify explicit artifact bytes. Not a signature or source-build attestation."""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import stat
import sys

MAX_FILES = 256
MAX_FILE_BYTES = 1024 ** 3
MAX_MANIFEST_BYTES = 1024 ** 2


def relative(name):
    path = PurePosixPath(name)
    if not name or path.is_absolute() or any(p in ('', '.', '..') for p in name.split('/')):
        raise ValueError('artifact names must be normalized relative paths')
    return path


def open_relative(root, name):
    """Walk with directory fds: no symlink component can redirect a selected file."""
    parts = relative(name).parts
    directory = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        for part in parts[:-1]:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=directory)
            os.close(directory)
            directory = child
        # NONBLOCK ensures an unexpected FIFO cannot hang before the regular-file check.
        return os.open(parts[-1], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=directory)
    finally:
        os.close(directory)


def identity(s):
    return s.st_dev, s.st_ino, s.st_size, s.st_mtime_ns, s.st_ctime_ns


def digest(root, name):
    fd = open_relative(root, name)
    try:
        before = os.fstat(fd)
        if not stat.S_ISREG(before.st_mode) or before.st_size > MAX_FILE_BYTES:
            raise ValueError('artifacts must be regular files at most 1 GiB each')
        h = hashlib.sha256()
        size = 0
        while block := os.read(fd, 1024 * 1024):
            size += len(block)
            if size > MAX_FILE_BYTES:
                raise ValueError('artifact grew beyond the size budget')
            h.update(block)
        if identity(before) != identity(os.fstat(fd)) or size != before.st_size:
            raise ValueError('artifact changed while hashing')
        # A replacement at the same path must not be mistaken for the open inode.
        current = open_relative(root, name)
        try:
            if identity(before) != identity(os.fstat(current)):
                raise ValueError('artifact path changed while hashing')
        finally:
            os.close(current)
        return {'path': name, 'bytes': size, 'sha256': h.hexdigest()}
    finally:
        os.close(fd)


def snapshot(root, names):
    if not 1 <= len(names) <= MAX_FILES or len(set(names)) != len(names):
        raise ValueError('select 1..256 distinct artifact paths')
    return {'format': 1, 'scope': 'byte identity only; no source provenance or signature',
            'files': [digest(root, name) for name in sorted(names)]}


def verify(root, manifest):
    if not isinstance(manifest, dict) or manifest.get('format') != 1:
        raise ValueError('unsupported artifact snapshot format')
    entries = manifest.get('files')
    if not isinstance(entries, list) or not 1 <= len(entries) <= MAX_FILES:
        raise ValueError('invalid artifact list')
    seen = set()
    for entry in entries:
        if not isinstance(entry, dict) or not isinstance(entry.get('path'), str):
            raise ValueError('invalid artifact entry')
        name = entry['path']
        if name in seen:
            raise ValueError('duplicate artifact path')
        seen.add(name)
        actual = digest(root, name)
        if actual != entry:
            raise ValueError('artifact bytes differ: ' + name)
    return len(entries)


def load_manifest(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, 'rb') as stream:
        info = os.fstat(stream.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_size > MAX_MANIFEST_BYTES:
            raise ValueError('manifest must be a regular file at most 1 MiB')
        data = stream.read(MAX_MANIFEST_BYTES + 1)
    if len(data) > MAX_MANIFEST_BYTES:
        raise ValueError('manifest grew beyond the size budget')
    return json.loads(data)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='action', required=True)
    create = sub.add_parser('snapshot')
    create.add_argument('--root', type=Path, required=True)
    create.add_argument('--output', type=Path, required=True)
    create.add_argument('files', nargs='+')
    check = sub.add_parser('verify')
    check.add_argument('--root', type=Path, required=True)
    check.add_argument('--manifest', type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.action == 'snapshot':
            manifest = snapshot(args.root, args.files)
            data = (json.dumps(manifest, indent=2) + '\n').encode()
            fd = os.open(args.output, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
            with os.fdopen(fd, 'wb') as stream:
                stream.write(data)
            print(json.dumps({'snapshot': str(args.output), 'files': len(manifest['files'])}))
        else:
            count = verify(args.root, load_manifest(args.manifest))
            print(json.dumps({'verified': True, 'files': count, 'scope': 'byte identity only'}))
    except (OSError, ValueError, TypeError) as error:
        print('artifact-snapshot: ' + str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
