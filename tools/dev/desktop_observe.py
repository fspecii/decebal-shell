#!/usr/bin/env python3
"""Read-only Linux desktop I/O observation; no workload, arguments or environments are inspected."""
import argparse
import hashlib
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


def disk_counters(text, partitions=()):
    """Linux whole-device counters; field9 (in-flight) is a gauge, not monotonic."""
    result = {}
    for line in text.splitlines():
        fields = line.split()
        if len(fields) < 14 or fields[2] in partitions or '/' in fields[2]:
            continue
        try:
            major, minor = int(fields[0]), int(fields[1])
            values = list(map(int, fields[3:]))
            if min([major, minor, *values]) < 0:
                continue
        except ValueError:
            continue
        result[f'{major}:{minor}:{fields[2]}'] = dict(
            device=fields[2], read_ops=values[0], read_ms=values[3],
            write_ops=values[4], write_ms=values[7], inflight=values[8],
            io_ms=values[9], weighted_ms=values[10],
            flush_ops=values[15] if len(values) >= 17 else 0,
            flush_ms=values[16] if len(values) >= 17 else 0)
    return result


def disk_snapshot(proc=Path('/proc/diskstats'), sysblock=Path('/sys/class/block')):
    text = proc.read_text()
    names = [row.split()[2] for row in text.splitlines() if len(row.split()) >= 3]
    partitions = {name for name in names if '/' not in name and (sysblock / name / 'partition').exists()}
    return disk_counters(text, partitions)


def disk_deltas(before, after, seconds):
    """Completion latency and activity estimates, never a device-health verdict."""
    if seconds <= 0:
        raise ValueError('positive disk observation interval required')
    rows = []
    keys = ('read_ops', 'read_ms', 'write_ops', 'write_ms', 'io_ms', 'weighted_ms', 'flush_ops', 'flush_ms')
    for identity, current in after.items():
        previous = before.get(identity)
        if previous is None:
            continue
        delta = {key: current[key] - previous[key] for key in keys}
        if min(delta.values()) < 0 or not (any(delta.values()) or current['inflight']):
            continue
        average = lambda duration, count: round(delta[duration] / delta[count], 3) if delta[count] else None
        rows.append(dict(device=current['device'], read_ops=delta['read_ops'], write_ops=delta['write_ops'],
                         flush_ops=delta['flush_ops'], read_await_ms=average('read_ms', 'read_ops'),
                         write_await_ms=average('write_ms', 'write_ops'), flush_await_ms=average('flush_ms', 'flush_ops'),
                         active_time_percent=round(delta['io_ms'] / seconds / 10, 3),
                         average_queue_depth=round(delta['weighted_ms'] / seconds / 1000, 3),
                         inflight_at_end=current['inflight']))
    return sorted(rows, key=lambda row: (-row['active_time_percent'], row['device']))


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
    parser.add_argument('--disks', action='store_true', help='include whole-device I/O counters; no disk workload is generated')
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
    previous_disks = disk_snapshot() if args.disks else None
    started = previous_at = time.monotonic()
    report = {'scope': 'global I/O PSI and same-UID processes present at both sample boundaries; '
              'not per-job attribution or a disk-health test. Short-lived processes and kernel writeback '
              'may be absent. No command arguments, environments or open file paths are read.',
              'uid': os.getuid(), 'observer_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), 'samples': [], 'interrupted': False}
    if args.disks:
        report['disk_scope'] = 'Whole devices only, including device-mapper/virtual layers; never sum overlapping devices. Completion latency and active time are interval estimates, not saturation or disk-health proof. Reset/replaced counters are skipped.'
    try:
        while time.monotonic() - started < args.seconds:
            remaining = args.seconds - (time.monotonic() - started)
            if remaining <= 0:
                break
            time.sleep(min(args.interval, remaining))
            current_tasks = tasks(proc, os.getuid())
            current_psi = pressure_totals(proc / 'pressure/io')
            current_disks = disk_snapshot() if args.disks else None
            now = time.monotonic()
            elapsed = now - previous_at
            rows = deltas(previous_tasks, current_tasks, elapsed, hz)
            report['samples'].append(dict(elapsed_seconds=round(now - started, 3),
                interval_seconds=round(elapsed, 3), **pressure_delta(previous_psi, current_psi, elapsed),
                top_io=sorted(rows, key=lambda r: r['read_bytes'] + r['write_bytes'], reverse=True)[:args.top],
                top_cpu=sorted(rows, key=lambda r: r['cpu_percent_one_core'], reverse=True)[:args.top]))
            if args.disks:
                report['samples'][-1]['disk_activity'] = disk_deltas(previous_disks, current_disks, elapsed)
            previous_at, previous_tasks, previous_psi = now, current_tasks, current_psi
            previous_disks = current_disks
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
