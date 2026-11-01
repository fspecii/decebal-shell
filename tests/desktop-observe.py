#!/usr/bin/env python3
"""Synthetic counter/identity tests; no real process inspection or priority changes."""
import importlib.util
import json
import os
from pathlib import Path
import stat
import tempfile
import unittest
from unittest import mock

spec = importlib.util.spec_from_file_location('desktop_observe', Path(__file__).resolve().parents[1] / 'tools/dev/desktop_observe.py')
observe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(observe)


def task(start='10', read=100, write=200, ticks=50):
    return dict(start=start, read=read, write=write, ticks=ticks, state='S', comm='fixture')


class ObserveTests(unittest.TestCase):
    def test_pid_reuse_and_counter_reset_are_not_charged_to_a_new_process(self):
        before = {1: task(), 2: task(), 3: task()}
        after = {1: task(start='11', read=9000), 2: task(read=1), 3: task(read=1124, write=2248, ticks=100), 4: task()}
        self.assertEqual(observe.deltas(before, after, 2, 100), [dict(pid=3, comm='fixture', state='S', read_bytes=1024, write_bytes=2048, cpu_percent_one_core=25.0)])

    def test_stat_names_with_spaces_and_parentheses_do_not_shift_identity(self):
        fields = ['0'] * 30
        fields[0], fields[11], fields[12], fields[19] = 'S', '40', '2', '987'
        self.assertEqual(observe.task_stat('3 (worker ) with spaces) ' + ' '.join(fields)), ('987', 42, 'S'))

    def test_pressure_uses_actual_interval_and_rejects_missing_or_reset_counters(self):
        self.assertEqual(observe.pressure_delta(dict(some=0, full=0), dict(some=500000, full=250000), 2), dict(some_stall_percent=25.0, full_stall_percent=12.5))
        with self.assertRaises(ValueError):
            observe.pressure_delta(dict(some=2, full=2), dict(some=1, full=1), 1)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'io'
            for text in ('some avg10=0 total=1\n', 'some total=1\nfull total=-1\n', 'some total=1\nfull total=oops\n'):
                path.write_text(text)
                with self.assertRaises(ValueError):
                    observe.pressure_totals(path)
            path.write_text('some avg10=0 total=12\nfull avg10=0 total=3\n')
            self.assertEqual(observe.pressure_totals(path), dict(some=12, full=3))

    def test_process_snapshot_reads_only_allowlisted_metadata_and_ignores_disappearing_tasks(self):
        with tempfile.TemporaryDirectory() as directory:
            proc = Path(directory)
            path = proc / '123'
            path.mkdir()
            fields = ['0'] * 30
            fields[0], fields[11], fields[12], fields[19] = 'S', '20', '3', '77'
            (path / 'stat').write_text('123 (fixture) ' + ' '.join(fields))
            (path / 'comm').write_text('fixture\n')
            (path / 'io').write_text('read_bytes: 1\nwrite_bytes: 2\n')
            (path / 'cmdline').write_text('PRIVATE ARGUMENT MUST NOT APPEAR')
            (path / 'environ').write_text('PRIVATE ENVIRONMENT MUST NOT APPEAR')
            (proc / '124').mkdir()
            original_read = Path.read_text

            def audited_read(candidate, *args, **kwargs):
                if candidate.parent == path:
                    self.assertIn(candidate.name, ('stat', 'comm', 'io'), 'private process metadata must not be read')
                return original_read(candidate, *args, **kwargs)

            with mock.patch.object(Path, 'read_text', audited_read):
                result = observe.tasks(proc, os.getuid())
            self.assertEqual(set(result), {123})
            self.assertEqual(result[123]['ticks'], 23)
            self.assertNotIn('PRIVATE', json.dumps(result))

    def test_disk_field_mapping_partitions_and_optional_flush_counters(self):
        text = '259 0 nvme0n1 10 0 80 20 5 0 40 30 2 100 200 0 0 0 0 3 12\n259 1 nvme0n1p1 10 0 80 20 5 0 40 30 2 100 200\n8 0 old 1 0 2 3 4 0 5 6 0 7 8\ninvalid\n8 1 bad -1 0 2 3 4 0 5 6 0 7 8'
        rows = observe.disk_counters(text, {'nvme0n1p1'})
        self.assertEqual(set(rows), {'259:0:nvme0n1', '8:0:old'})
        self.assertEqual(rows['259:0:nvme0n1']['read_ms'], 20)
        self.assertEqual(rows['259:0:nvme0n1']['write_ms'], 30)
        self.assertEqual(rows['259:0:nvme0n1']['flush_ops'], 3)
        self.assertEqual(rows['259:0:nvme0n1']['flush_ms'], 12)
        self.assertEqual(rows['8:0:old']['flush_ops'], 0)

    def test_disk_interval_rates_ignore_the_decreasing_inflight_gauge(self):
        a = observe.disk_counters('259 0 nvme0n1 10 0 80 20 5 0 40 30 10 100 200 0 0 0 0 3 12')
        b = observe.disk_counters('259 0 nvme0n1 14 0 80 28 7 0 40 38 0 600 1200 0 0 0 0 4 22')
        self.assertEqual(observe.disk_deltas(a, b, 2), [dict(device='nvme0n1', read_ops=4, write_ops=2, flush_ops=1,
            read_await_ms=2.0, write_await_ms=4.0, flush_await_ms=10.0, active_time_percent=25.0,
            average_queue_depth=.5, inflight_at_end=0)])
        self.assertEqual(observe.disk_deltas(b, a, 2), [], 'reset counters cannot be charged as activity')
        self.assertEqual(observe.disk_deltas({}, b, 2), [], 'new devices have no interval baseline')
        with self.assertRaises(ValueError):
            observe.disk_deltas(a, b, 0)

    def test_disk_snapshot_excludes_partition_devices_without_reading_their_contents(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            stats = root / 'diskstats'
            stats.write_text('259 0 disk 1 0 2 3 4 0 5 6 0 7 8\n259 1 part 1 0 2 3 4 0 5 6 0 7 8')
            (root / 'part').mkdir()
            (root / 'part/partition').write_text('1')
            self.assertEqual(set(observe.disk_snapshot(stats, root)), {'259:0:disk'})

    def test_reports_are_private_and_never_overwrite_or_follow_a_symlink(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'report.json'
            observe.write_report(path, {'ok': True})
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
            with self.assertRaises(FileExistsError):
                observe.write_report(path, {'ok': False})
            alias = Path(directory) / 'alias'
            alias.symlink_to(path)
            with self.assertRaises(FileExistsError):
                observe.write_report(alias, {'ok': False})
            self.assertEqual(json.loads(path.read_text()), {'ok': True})


if __name__ == '__main__':
    unittest.main()
