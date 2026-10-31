#!/usr/bin/env python3
"""Read-only Linux desktop I/O observation; no workload, arguments or environments are inspected."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import time


def pressure_totals(path):
    totals = {}
    for line in path.read_text().splitlines():
        fields = line.split()
        if fields and fields[0] in ('some', 'full'):
            for field in fields[1:]:
                if field.startswith('total='):
                    value = int(field[6:])
                    if value < 0:
                        raise ValueError('negative PSI counter')
                    totals[fields[0]] = value
    if set(totals) != {'some', 'full'}:
        raise ValueError('both Linux I/O PSI counters are required')
    return totals


def task_stat(text):
    # A task name may itself contain spaces and parentheses.
    fields = text.rsplit(') ', 1)[1].split()
    return fields[19], int(fields[11]) + int(fields[12]), fields[0]


def tasks(proc, uid):
    found = {}
    for path in proc.iterdir():
        if not path.name.isdigit():
            continue
        try:
            if path.stat().st_uid != uid:
                continue
            start, ticks, state = task_stat((path / 'stat').read_text())
            counters = dict((key, int(value)) for key, value in
                            (line.split(':', 1) for line in (path / 'io').read_text().splitlines()))
            comm = (path / 'comm').read_text().strip()[:80]
            if task_stat((path / 'stat').read_text())[0] != start or path.stat().st_uid != uid:
                continue
            found[int(path.name)] = dict(start=start, ticks=ticks, state=state, comm=comm,
                                         read=counters['read_bytes'], write=counters['write_bytes'])
        except (OSError, ValueError, IndexError, KeyError):
            # A process may disappear or be inaccessible between any two reads.
            continue
    return found


def deltas(before, after, seconds, hz):
    if seconds <= 0 or hz <= 0:
        raise ValueError('positive elapsed time and clock frequency required')
    rows = []
    for pid, current in after.items():
        previous = before.get(pid)
        if not previous or current['start'] != previous['start']:
            continue
        change = {key: current[key] - previous[key] for key in ('read', 'write', 'ticks')}
        if min(change.values()) < 0:
            continue
        rows.append(dict(pid=pid, comm=current['comm'], state=current['state'],
                         read_bytes=change['read'], write_bytes=change['write'],
                         cpu_percent_one_core=round(change['ticks'] / hz / seconds * 100, 3)))
    return rows


def pressure_delta(before, after, seconds):
    if seconds <= 0 or any(after[k] < before[k] for k in ('some', 'full')):
        raise ValueError('invalid PSI interval or decreasing counter')
    return {k + '_stall_percent': round((after[k] - before[k]) / seconds / 1e6 * 100, 4)
            for k in ('some', 'full')}


def write_report(path, report):
    # Never overwrite a prior diagnostic or follow a final-component symlink.
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'w') as stream:
        json.dump(report, stream, indent=2)
        stream.write('\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--seconds', type=int, default=10)
    parser.add_argument('--interval', type=float, default=1)
    parser.add_argument('--top', type=int, default=6)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    if not (1 <= args.interval <= args.seconds <= 60 and 1 <= args.top <= 20):
        parser.error('require 1 <= interval <= seconds <= 60, and top between 1 and 20')
    if os.path.lexists(args.out):
        parser.error('output must be a fresh file')
    ionice = shutil.which('ionice')
    if not ionice:
        parser.error('ionice is required for idle-priority observation')
    os.nice(19)
    subprocess.run([ionice, '-c', '3', '-p', str(os.getpid())], check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    proc = Path('/proc')
    hz = os.sysconf('SC_CLK_TCK')
    previous_tasks = tasks(proc, os.getuid())
    previous_psi = pressure_totals(proc / 'pressure/io')
    started = previous_at = time.monotonic()
    report = {'scope': 'global I/O PSI and same-UID processes present at both sample boundaries; '
              'not per-job attribution or a disk-health test. Short-lived processes and kernel writeback '
              'may be absent. No command arguments, environments or open file paths are read.',
              'uid': os.getuid(), 'samples': [], 'interrupted': False}
    try:
        while time.monotonic() - started < args.seconds:
            remaining = args.seconds - (time.monotonic() - started)
            if remaining <= 0:
                break
            time.sleep(min(args.interval, remaining))
            current_tasks = tasks(proc, os.getuid())
            current_psi = pressure_totals(proc / 'pressure/io')
            now = time.monotonic()
            elapsed = now - previous_at
            rows = deltas(previous_tasks, current_tasks, elapsed, hz)
            report['samples'].append(dict(elapsed_seconds=round(now - started, 3),
                interval_seconds=round(elapsed, 3), **pressure_delta(previous_psi, current_psi, elapsed),
                top_io=sorted(rows, key=lambda r: r['read_bytes'] + r['write_bytes'], reverse=True)[:args.top],
                top_cpu=sorted(rows, key=lambda r: r['cpu_percent_one_core'], reverse=True)[:args.top]))
            previous_at, previous_tasks, previous_psi = now, current_tasks, current_psi
    except KeyboardInterrupt:
        report['interrupted'] = True
    report['seconds'] = round(time.monotonic() - started, 3)
    report['max_full_stall_percent'] = max((s['full_stall_percent'] for s in report['samples']), default=None)
    write_report(args.out, report)
    print(json.dumps({'out': str(args.out), 'samples': len(report['samples']),
                      'max_full_stall_percent': report['max_full_stall_percent'],
                      'interrupted': report['interrupted']}))
    return 130 if report['interrupted'] else 0


if __name__ == '__main__':
    raise SystemExit(main())
