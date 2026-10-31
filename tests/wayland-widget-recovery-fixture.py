#!/usr/bin/env python3
"""Readiness watcher checks only: private local files, no compositor or desktop processes."""
import importlib.util
import os
from pathlib import Path
import struct
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("widget_recovery", Path(__file__).parent / "e2e/wayland_widgets_recovery.py")
FIXTURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FIXTURE)


def event(mask, name):
    name = name.encode() + b"\0"
    name += b"\0" * (-len(name) % 4)
    return struct.pack("iIII", 1, mask, 0, len(name)) + name


class WatchTests(unittest.TestCase):
    def test_republishing_without_invalidation_is_rejected(self):
        events = FIXTURE.parse_events(event(FIXTURE.IN_MOVED_TO, "host.state"))
        self.assertFalse(FIXTURE.invalidated_before_publication(events, "host.state"))

    def test_only_target_delete_before_publication_counts(self):
        events = FIXTURE.parse_events(event(FIXTURE.IN_DELETE, "other.state") + event(FIXTURE.IN_MOVED_TO, "host.state"))
        self.assertFalse(FIXTURE.invalidated_before_publication(events, "host.state"))
        events = FIXTURE.parse_events(event(FIXTURE.IN_DELETE, "host.state") + event(FIXTURE.IN_MOVED_TO, "host.state"))
        self.assertTrue(FIXTURE.invalidated_before_publication(events, "host.state"))

    def test_overflow_and_truncated_evidence_fail(self):
        with self.assertRaises(AssertionError):
            FIXTURE.invalidated_before_publication([{"mask": FIXTURE.IN_Q_OVERFLOW, "name": ""}], "host.state")
        with self.assertRaises(ValueError):
            FIXTURE.parse_events(b"short")
        with self.assertRaises(ValueError):
            FIXTURE.parse_events(event(FIXTURE.IN_DELETE, "host.state")[:-1])

    def test_real_filesystem_delete_then_atomic_publication_is_observed(self):
        with tempfile.TemporaryDirectory(prefix="widget-readiness-watch-") as directory:
            root = Path(directory)
            record = root / "host.state"
            record.write_text("ready")
            watch = FIXTURE.RecordWatch(root)
            try:
                record.unlink()
                replacement = root / "temporary"
                replacement.write_text("degraded")
                os.replace(replacement, record)
                self.assertTrue(FIXTURE.invalidated_before_publication(watch.drain(), record.name))
            finally:
                watch.close()

    def test_real_atomic_overwrite_does_not_masquerade_as_invalidation(self):
        with tempfile.TemporaryDirectory(prefix="widget-readiness-watch-") as directory:
            root = Path(directory)
            record = root / "host.state"
            record.write_text("old ready")
            watch = FIXTURE.RecordWatch(root)
            try:
                replacement = root / "temporary"
                replacement.write_text("new ready")
                os.replace(replacement, record)
                self.assertFalse(FIXTURE.invalidated_before_publication(watch.drain(), record.name))
            finally:
                watch.close()


if __name__ == "__main__":
    unittest.main()
