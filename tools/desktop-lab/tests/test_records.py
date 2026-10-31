"""Read-only catalog contracts. These tests do not execute the fixtures."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from lab.fingerprint import sha256_file
from lab.records import classify_panel, classify_settings, preview_contract, read_control

_REPO = Path(__file__).resolve().parents[3]
_PANEL = Path("/var/tmp/panel-wm-acceptance-final/summary.json")
_SETTINGS = Path("/var/tmp/settings-current-stress/summary.json")
_INPUTS = Path("/var/tmp/settings-current-stress/inputs.json")
_PREVIEW = _REPO / "tests/e2e/settings-preview-stress.py"
_REVIEWED = "3286ae88b51bdb8d4ecafcd7680a5e7ed1293432daf1993d2c03ee75a7965788"


class RecordTests(unittest.TestCase):
    def test_panel_contract_is_candidate_evidence(self) -> None:
        document = {
            "workarea_cases": 21,
            "short_all_applet_scale2_cases": 4,
            "popup_directions": {f"pop-{index}": True for index in range(8)},
            "source_binding": "author smoke; no independent frozen fingerprint",
            "passed": True,
            "limits": ["dev candidate"],
        }
        report = classify_panel(document)
        self.assertTrue(report["counts_match_contract"])
        self.assertEqual(report["workarea_total"], 25)
        self.assertEqual(report["popup_true"], 8)
        self.assertFalse(report["source_frozen"])
        self.assertFalse(report["full_acceptance"])
        self.assertEqual(report["acceptance"], "candidate_runtime")
        if _PANEL.is_file():
            live = classify_panel(json.loads(_PANEL.read_text()))
            self.assertTrue(live["counts_match_contract"])
            self.assertEqual(live["workarea_total"], 25)
            self.assertEqual(live["popup_count"], 8)
            self.assertFalse(live["full_acceptance"])
            self.assertFalse(live["source_frozen"])

    def test_settings_passed_is_not_acceptance(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            body = b"settings-bytes"
            (root / "decsettings").write_bytes(body)
            digest = hashlib.sha256(body).hexdigest()
            document = {
                "passed": True,
                "historical_crash_cause": "unconfirmed",
                "unexpected_exit_observed": False,
                "limits": ["bounded observation"],
                "binaries_unchanged_and_pinned": {"decsettings": {"expected": digest}},
                "hardware_acceptance": True,
            }
            report = classify_settings(document, root)
            self.assertTrue(report["json_passed"])
            self.assertTrue(report["json_passed_is_not_full_acceptance"])
            self.assertFalse(report["full_acceptance"])
            self.assertFalse(report["hardware_acceptance"])
            self.assertTrue(report["recorded_hardware_claim_ignored"])
            self.assertEqual(report["historical_crash_cause"], "unconfirmed")
            self.assertFalse(report["unexpected_exit_observed"])
            self.assertTrue(report["rehash"]["decsettings"]["matches_claimed"])
            self.assertFalse(report["source_frozen"])

    def test_preview_contract_is_not_executed(self) -> None:
        report = preview_contract(_PREVIEW, _INPUTS if _INPUTS.is_file() else None)
        self.assertFalse(report["executed"])
        self.assertFalse(report["full_acceptance"])
        self.assertTrue(report["contract_ok"])
        self.assertEqual(report["tabs"], ["theme", "palettes", "customise", "apps"])
        self.assertEqual(report["root_reported_switches"]["count"], 40)
        self.assertFalse(report["root_reported_switches"]["verified_by_this_lane"])
        digest = sha256_file(_PREVIEW)
        self.assertEqual(report["sha256"], digest)
        if _INPUTS.is_file():
            self.assertEqual(report["reviewed_hash"], _REVIEWED)
            self.assertEqual(report["reviewed_hash_matches"], digest == _REVIEWED)

    def test_control_read_drops_thread_ids(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            path = Path(raw) / "control.json"
            path.write_text(
                json.dumps(
                    {
                        "deadline_utc": "2026-10-31T11:36:32+00:00",
                        "max_vms": 2,
                        "policy": {"no_coauthor_trailers": True},
                        "threads": [{"id": "mcp:launch:deadbeef"}],
                    }
                )
            )
            report = read_control(path)
        blob = json.dumps(report)
        self.assertNotIn("mcp:launch", blob)
        self.assertNotIn("deadbeef", blob)
        self.assertTrue(report["threads_ignored"])
        self.assertTrue(report["deadline_matches_sprint"])
        self.assertTrue(report["no_coauthor_trailers"])
        self.assertEqual(report["vcpu"], 2)
        self.assertEqual(report["memory_mib"], 1024)
        self.assertFalse(report["full_acceptance"])


if __name__ == "__main__":
    unittest.main()
