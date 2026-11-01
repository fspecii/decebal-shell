#!/usr/bin/env python3
"""Read-only Linux shell process checks. Never signal, restart, or edit config."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess


def stat_identity(text):
    # comm may contain spaces and parentheses. Fields after the final ') ' start at field 3.
    fields = text.rsplit(') ', 1)[1].split()
    return fields[0], fields[19]


def inspect_component(entry, proc=Path('/proc'), uid=None):
    result = {'name': entry['name'], 'pid': entry['pid']}
    uid = os.getuid() if uid is None else uid
    try:
        pid = int(entry['pid'])
        if pid <= 0:
            raise ValueError('pid must be positive')
        directory = proc / str(pid)
        if directory.stat().st_uid != uid:
            return dict(result, state='foreign-owner')
        state, start = stat_identity((directory / 'stat').read_text())
        if state in ('Z', 'X'):
            return dict(result, state='exited')
        if 'starttime' in entry and str(entry['starttime']) != start:
            return dict(result, state='pid-reused')
        exe = directory / 'exe'
        if os.readlink(exe) != entry['target']:
            return dict(result, state='executable-changed')
        digest = hashlib.sha256(exe.read_bytes()).hexdigest()
        if entry.get('sha256') and digest != entry['sha256']:
            return dict(result, state='binary-changed')
        # Recheck start time after reading the executable, rather than combine two different processes.
        if stat_identity((directory / 'stat').read_text())[1] != start:
            return dict(result, state='identity-raced')
        return dict(result, state='running' if 'starttime' in entry else 'running-unpinned',
                    starttime=start, sha256=digest)
    except FileNotFoundError:
        return dict(result, state='absent')
    except (OSError, ValueError, IndexError, KeyError) as exc:
        return dict(result, state='unknown', error=type(exc).__name__)


def dock_wanted(binary, expected_sha):
    # Only the explicit read-only query on an explicitly pinned binary is permitted.
    if hashlib.sha256(binary.read_bytes()).hexdigest() != expected_sha:
        raise ValueError('dock probe binary hash mismatch')
    run = subprocess.run([str(binary), '--wanted'], stdin=subprocess.DEVNULL,
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5)
    return {0: 'enabled', 1: 'disabled'}.get(run.returncode, 'unknown')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipt', type=Path, required=True,
                        help='JSON with components: name, pid, target, optional starttime and sha256')
    parser.add_argument('--dock-probe', type=Path, help='Optional trusted decdock binary; uses current session environment')
    parser.add_argument('--dock-sha256', help='Required pinned digest for --dock-probe')
    args = parser.parse_args()
    if bool(args.dock_probe) != bool(args.dock_sha256):
        parser.error('--dock-probe and --dock-sha256 must be supplied together')
    try:
        receipt = json.loads(args.receipt.read_text())
        rows = [inspect_component(e) for e in receipt['components']]
        wanted = dock_wanted(args.dock_probe, args.dock_sha256) if args.dock_probe else 'not-probed'
        for row in rows:
            if row['name'] == 'decdock' and row['state'] in ('absent', 'exited') and wanted == 'disabled':
                row['state'] = 'disabled-by-configuration'
        complete = bool(rows) and all(r['state'] in ('running', 'disabled-by-configuration') for r in rows)
        print(json.dumps({'components': rows, 'dock_configuration': wanted,
                          'identities_verified': complete, 'mutation_performed': False}, indent=2))
        return 0 if complete else 1
    except (OSError, ValueError, KeyError, TypeError, subprocess.TimeoutExpired) as exc:
        print(json.dumps({'error': str(exc), 'mutation_performed': False}))
        return 2


if __name__ == '__main__':
    raise SystemExit(main())
