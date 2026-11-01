#!/usr/bin/env python3
"""Evidence validation tests only; never launches the native harness."""
import copy
import importlib.util
from pathlib import Path
import unittest
spec = importlib.util.spec_from_file_location('ukui', Path(__file__).parent / 'e2e/openkylin-launcher-native.py')
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


def records():
    return [dict(quick=False, shard=dict(index=i, count=12), inputs={'declaunch': 'pinned', 'harness': 'same', 'fixture_desktop_entries_normalized': {'red.desktop': 'same'}},
                 cleanup=dict(run_failed=False, error=None, forced=[], rescues=[]),
                 runs=[dict(tag=m.case_tag(c)) for c in m.matrix_cases(False, i, 12)], passed=6,
                 pointer_launch={'launched': 'red', 'exit_code': 0, 'red_pixels': 100} if i == 0 else None) for i in range(12)]


class Shards(unittest.TestCase):
    def test_matrix_is_complete_and_disjoint(self):
        rows=records()
        self.assertEqual(m.verify_shards(rows)['cases'], 72)
        self.assertEqual(len({r['tag'] for row in rows for r in row['runs']}), 72)

    def test_missing_shard_cannot_pass(self):
        with self.assertRaises(ValueError): m.verify_shards(records()[:-1])

    def test_duplicate_shard_cannot_pass(self):
        with self.assertRaises(ValueError): m.verify_shards(records() + [records()[0]])

    def test_different_binary_or_harness_cannot_mix(self):
        for key in ('declaunch', 'harness'):
            rows=records();rows[1]['inputs'][key]='other'
            with self.assertRaises(ValueError): m.verify_shards(rows)

    def test_interrupted_or_unclean_shard_cannot_pass(self):
        for key, value in [('run_failed', True), ('error', 'cleanup failed'), ('forced', [123]), ('rescues', [123])]:
            rows=records();rows[0]['cleanup'][key]=value
            with self.assertRaises(ValueError): m.verify_shards(rows)

    def test_duplicate_or_missing_case_cannot_pass(self):
        rows=records();rows[0]['runs'][0]=copy.deepcopy(rows[0]['runs'][1])
        with self.assertRaises(ValueError): m.verify_shards(rows)

    def test_missing_pointer_or_quick_evidence_cannot_pass(self):
        rows=records();rows[0]['pointer_launch']=None
        with self.assertRaises(ValueError): m.verify_shards(rows)
        rows=records();rows[0]['quick']=True
        with self.assertRaises(ValueError): m.verify_shards(rows)

    def test_private_paths_do_not_change_normalized_identity(self):
        rows=records()
        for i, row in enumerate(rows): row["inputs"]["fixture_desktop_entries"]={"red.desktop":str(i)}
        self.assertTrue(m.verify_shards(rows)["passed"])
        rows[0]["inputs"]["fixture_desktop_entries_normalized"]={"red.desktop":"different"}
        with self.assertRaises(ValueError): m.verify_shards(rows)

    def test_false_pointer_claim_rejected(self):
        rows=records();rows[0]["pointer_launch"]={"launched":"green", "exit_code":0,"red_pixels":100}
        with self.assertRaises(ValueError): m.verify_shards(rows)

    def test_invalid_selection_rejected(self):
        for index, count in [(-1, 1), (1, 1), (0, 0), (0, 73)]:
            with self.assertRaises(ValueError): m.matrix_cases(False, index, count)


if __name__ == '__main__': unittest.main()
