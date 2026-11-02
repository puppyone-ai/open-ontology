"""Unit-only source boundary/refusal guards; not real UNDO/log-retention evidence."""
import copy
import json
from pathlib import Path
import tempfile
import unittest

import bounded_source as b
import buffer_cursor as c
import initial_snapshot as s
from test_bounded_source import FakeDriver, TABLE


class Driver(FakeDriver):
    def __init__(self, digest):
        super().__init__()
        self.digest, self.fail, self.chunk_scns = digest, None, []

    def call(self, action, **kw):
        if action == "log-catalog":
            inventory = super().call("inventory")
            return {"protocol": "oo.oracle.log-catalog/v1", "current_scn": self.scn,
                    "identity": inventory["identity"], "source_id": inventory["source_id"],
                    "reader_mutations": 0, "catalog_only": True,
                    "ranges": [{"thread": 1, "first_scn": "100", "next_scn": "1000", "kind": "online"}]}
        if action == "chunk":
            self.chunk_scns.append(kw["scn"])
            if self.fail:
                raise ValueError(self.fail)
        return super().call(action, **kw)


class InitialSnapshotTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        native = self.root/"UNIT-native"
        native.write_bytes(b"unit fixture, not a driver binary")
        self.driver = Driver(b.sha(native))
        inventory = self.driver.call("inventory")
        config = {"source_id": "fixture", "identity": inventory["identity"], "owner": "OWN"}
        b.atomic(self.root/"source.json", b.canonical(config))
        for name in ("offset", "history", "ready", "barrier"):
            (self.root/name).write_text("UNIT ONLY " + name)
        admission = {"source_id": "fixture", "table": "OWN.ORDERS", "log_start_scn": "100",
                     "vendor_offset": c.reference(self.root/"offset"),
                     "connector_history": c.reference(self.root/"history"),
                     "subscription_ready": c.reference(self.root/"ready")}
        b.atomic(self.root/"admission.json", b.canonical(admission))
        boundary = {"protocol": "oo.oracle.snapshot-boundary/v1", "source_id": "fixture",
                    "identity": inventory["identity"], "table": TABLE, "snapshot_scn": "123",
                    "barrier_evidence": c.reference(self.root/"barrier"),
                    "log_admission": c.reference(self.root/"admission.json")}
        b.atomic(self.root/"boundary.json", b.canonical(boundary))
        cfg = {"protocol": s.PROTOCOL, "source_config": c.reference(self.root/"source.json"),
               "boundary": c.reference(self.root/"boundary.json"), "reader": c.reference(native),
               "budgets": {"chunk_rows": 7, "chunk_bytes": 2048, "rows": 1000,
                           "bytes": 1048576, "seconds": 1200}}
        b.atomic(self.root/"config.json", b.canonical(cfg))
        self.out = self.root/"capture"

    def tearDown(self):
        self.tmp.cleanup()

    def capture(self, **kwargs):
        return s.capture(self.root/"config.json", self.out,
                         driver_factory=lambda *_: self.driver, **kwargs)

    def test_same_scn_and_key_progress_survive_source_current_scn_change(self):
        first = self.capture(stop_after=2)
        self.assertEqual(first["status"], "interrupted")
        before = {p.name: b.sha(p) for p in (self.out/"snapshot").glob("part-*.json")}
        self.driver.scn = "999"
        final = self.capture(resume=True)
        self.assertEqual(final["snapshot_scn"], "123")
        self.assertEqual(final["snapshot_result"]["rows"], 31)
        self.assertEqual(set(self.driver.chunk_scns), {"123"})
        self.assertEqual(before, {name: b.sha(self.out/"snapshot"/name) for name in before})

    def test_undo_error_preserves_partial_state_and_never_falls_back(self):
        self.capture(stop_after=2)
        self.driver.scn, self.driver.fail = "999", "ORA-01555"
        with self.assertRaisesRegex(ValueError, "ORA-01555"):
            self.capture(resume=True)
        state = json.loads((self.out/"state.json").read_text())
        self.assertTrue(state["rebaseline_required"])
        self.assertEqual(state["reason_kind"], "undo_expired")
        self.assertFalse(state["fresh_snapshot_fallback"])
        self.assertEqual(state["snapshot_scn"], "123")
        self.assertFalse((self.out/"snapshot/manifest.json").exists())
        self.driver.fail = None
        with self.assertRaisesRegex(ValueError, "explicit_rebaseline_required"):
            self.capture(resume=True)

    def test_definition_and_undo_failures_are_not_conflated(self):
        self.assertEqual(s.failure_class(ValueError("ORA-01466"))["reason_kind"], "definition_boundary")
        self.assertEqual(s.failure_class(ValueError("ORA-01555"))["reason_kind"], "undo_expired")

    def test_official_missing_log_and_identity_drift_require_explicit_rebaseline(self):
        result = s.failure_class(ValueError("ORA-01291: missing logfile"))
        self.assertEqual(result["reason_kind"], "log_gap")
        self.assertTrue(result["rebaseline_required"])
        self.assertFalse(result["fresh_snapshot_fallback"])
        for error in ("source_identity_or_schema_drift", "source_log_identity"):
            self.assertTrue(s.failure_class(ValueError(error))["rebaseline_required"])

    def test_resume_requires_exact_config_and_explicit_action(self):
        self.capture(stop_after=1)
        with self.assertRaisesRegex(ValueError, "explicit_resume"):
            self.capture()
        cfg = json.loads((self.root/"config.json").read_text())
        cfg["budgets"]["rows"] += 100
        b.atomic(self.root/"config.json", b.canonical(cfg))
        with self.assertRaisesRegex(ValueError, "config_changed"):
            self.capture(resume=True)

    def test_real_scn_strings_and_contiguous_catalog_required(self):
        catalog = self.driver.call("log-catalog")
        self.assertTrue(s.log_coverage(catalog, "100", "123")["covered"])
        for bad in ("0", "0001", "1.0", "-1", 123):
            with self.assertRaises(ValueError):
                s.log_coverage(catalog, bad, "123")
        gap = copy.deepcopy(catalog)
        gap["ranges"] = [{"thread": 1, "first_scn": "100", "next_scn": "110"},
                         {"thread": 1, "first_scn": "111", "next_scn": "200"}]
        with self.assertRaisesRegex(ValueError, "source_log_gap"):
            s.log_coverage(gap, "105", "123")
        gap["ranges"][1]["first_scn"] = "110"
        self.assertTrue(s.log_coverage(gap, "105", "123")["covered"])
        gap["ranges"][1]["thread"] = 2
        with self.assertRaisesRegex(ValueError, "multi_thread"):
            s.log_coverage(gap, "105", "123")

    def test_explicit_scn_cannot_change_mid_capture_or_point_to_future(self):
        with self.assertRaisesRegex(ValueError, "scn_invalid"):
            b.capture(self.driver, self.root/"future", snapshot_scn="124")
        b.capture(self.driver, self.root/"bounded", snapshot_scn="120", chunk_rows=7, stop_after=1)
        with self.assertRaisesRegex(ValueError, "scn_invalid"):
            b.capture(self.driver, self.root/"bounded", snapshot_scn="121", chunk_rows=7)


if __name__ == "__main__":
    unittest.main()
