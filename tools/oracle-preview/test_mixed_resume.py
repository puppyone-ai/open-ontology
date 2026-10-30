"""Offline tests against the already captured joined input; never opens a source."""
import copy
import json
import os
from pathlib import Path
import tempfile
import unittest

import buffer_cursor as c
import producer as p
import rollover as r


@unittest.skipUnless(os.environ.get("OO_ORACLE_MIXED_CONFIG"), "requires pinned mixed packet")
class MixedResumeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.config = json.loads(Path(os.environ["OO_ORACLE_MIXED_CONFIG"]).read_text())
        self.index = r.document(self.config["buffer_index"])
        self.context = r.document(self.index["context"])
        self.manifest = r.document(self.config["bundle"])
        self.plan = r.initial_fill_plan(self.config, self.index, self.context, self.manifest)
        self.bootstrap = {"source_id": self.manifest["source_id"], "source_generation": 1,
                          "lease_id": "mixed-unit-same-task", "lease_generation": 3, "previous": None}

    def tearDown(self):
        self.tmp.cleanup()

    def state(self, bootstrap=None):
        return r.fill_stream_state(self.config, bootstrap or self.bootstrap,
                                   self.index, self.context, self.manifest, self.plan)

    def resumed(self, seq, completed=False, sealed=False):
        receipt = {"protocol": "oo.c1.typed-ack/v3", "source": self.manifest["source_id"],
                   "task": self.bootstrap["lease_id"], "generation": 2, "epoch": 1,
                   "sequence": seq, "position": self.plan["batches"][seq-1]["position"],
                   "digest": self.plan["batches"][seq-1]["digest"]}
        if seq == 41:
            position = r.document(self.index["transactions"][0])["source_position"]
            sanitized = copy.deepcopy(position)
            for key in ("captured_event_ids", "end_evidence"):
                sanitized[key].pop("path")
            receipt.update(checkpoint_kind="buffer_consumption", context_sha256=position["context_sha256"],
                bundle_sha256=position["bundle_sha256"], buffer_cursor=position["cursor_after"],
                source_evidence_sha256=p.digest(position), source_offset_write=False,
                source_position={"engine": "oracle", "source_offset_advanced": False,
                    "apply_ordinal": position["target_apply_ordinal"], "buffer_position": sanitized,
                    "source_evidence_sha256": p.digest(position)})
        ack = {"protocol": "oo.jobs.oracle-c1-baseline-ack/v1", "stage": "typed_batch",
               "durable": True, "receipt": receipt}
        path = self.root/f"ack-{seq}.json"
        path.write_text(json.dumps(ack))
        resume = {"protocol": r.FILL_RESUME, "engine": "oracle", "source_id": self.manifest["source_id"],
                  "task_id": self.bootstrap["lease_id"], "source_generation": 1, "lease_generation": 2,
                  "source_epoch": 1, "config_digest": self.plan["config_digest"],
                  "schema_digest": self.plan["typed_schema_sha256"], "buffer_index": self.config["buffer_index"],
                  "buffer_context": self.index["context"], "bundle_sha256": self.context["bundle"]["sha256"],
                  "initial_fill": self.plan, "resume_sequence": seq, "last_ack": c.reference(path),
                  "completed_tables": sorted(self.plan["tables"]) if completed else [],
                  "state": "awaiting_publish" if sealed else "open"}
        return {**self.bootstrap, "resume": resume}

    def test_explicit_opt_in_preserves_legacy_and_exact_fill_coverage(self):
        self.assertIsNone(r.initial_fill_plan({**self.config, "initial_mixed_resume": False},
            self.index, self.context, self.manifest))
        self.assertEqual(self.plan["fill_batch_count"], 40)
        self.assertEqual(self.plan["batch_count"], 41)
        self.assertEqual(sum(t["rows"] for t in self.plan["tables"].values()), 10000)
        self.assertEqual(self.manifest["final_rows"], 10001)
        self.assertEqual(self.plan["final_position"], "1")
        self.assertEqual(self.state()["sequence"], 0)

    def test_only_actual_ack_sequence_is_skipped_not_commit_or_cursor(self):
        for seq in (3, 8, 39, 40, 41):
            with self.subTest(seq=seq):
                state = self.state(self.resumed(seq, completed=seq>=40))
                self.assertEqual(state["sequence"], seq)
        self.assertEqual(self.plan["batches"][39]["position"], "0")
        self.assertEqual(self.plan["batches"][40]["position"], "1")

    def test_future_cross_task_context_epoch_schema_and_ack_mismatch_refuse(self):
        good = self.resumed(3)
        for key, value in (("task_id", "other"), ("lease_generation", 4), ("source_epoch", 2),
                           ("buffer_context", {}), ("schema_digest", "wrong"),
                           ("resume_sequence", 4), ("source_generation", 2)):
            bad = copy.deepcopy(good)
            bad["resume"][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.state(bad)

    def test_table_complete_cannot_be_inferred_from_partial_fill(self):
        with self.assertRaisesRegex(ValueError, "checkpoint_or_complete"):
            self.state(self.resumed(39, completed=True))
        with self.assertRaisesRegex(ValueError, "incomplete_snapshot"):
            self.state(self.resumed(41, completed=False))

    def test_seal_requires_final_cdc_ack_not_just_all_fill(self):
        with self.assertRaisesRegex(ValueError, "unsealed"):
            self.state(self.resumed(40, completed=True, sealed=True))
        self.assertEqual(self.state(self.resumed(41, completed=True, sealed=True))["sequence"], 41)

    def test_cdc_ack_cannot_claim_source_offset_advance(self):
        bootstrap = self.resumed(41, completed=True)
        ack = r.document(bootstrap["resume"]["last_ack"])
        ack["receipt"]["source_offset_write"] = True
        path = self.root/"wrong-offset-ack.json"
        path.write_text(json.dumps(ack))
        bootstrap["resume"]["last_ack"] = c.reference(path)
        with self.assertRaisesRegex(ValueError, "buffer_ack"):
            self.state(bootstrap)


if __name__ == "__main__":
    unittest.main()
