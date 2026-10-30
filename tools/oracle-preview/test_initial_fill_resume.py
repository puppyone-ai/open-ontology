"""Pure fill-resume protocol guards. Fake ACKs here are not target acceptance."""
import copy
import json
from pathlib import Path
import tempfile
import unittest

import bounded_source as b
import buffer_cursor as c
import producer as p
import rollover as r


class InitialFillResumeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        spec = {"columns": [{"name": "ID", "oid": 23, "typmod": -1, "nullable": False}], "keys": [0]}
        self.frames = [{"kind": "typed_batch", "batch": {
            "protocol": "oo.c1.typed-batch/v3", "kind": "fill", "sequence": n,
            "position": "0", "previous_position": "0",
            "parts": [{"table": "APP.T", "spec": spec, "path": "/unit-only/part",
                       "sha256": "f"*64, "bytes": 10, "rows": 2}]}} for n in range(1, 4)]
        b.atomic(self.root/"batches.jsonl", b"".join(b.canonical(f)+b"\n" for f in self.frames))
        self.manifest = {"source_id": "oracle_fill_unit", "batch_count": 3, "final_rows": 6,
                         "boundary": {"final_position": "0"}, "typed_tables": {"APP.T": spec},
                         "artifacts": {"batches.jsonl": b.sha(self.root/"batches.jsonl")}}
        b.atomic(self.root/"manifest.json", b.canonical(self.manifest))
        self.context = {"initial_cursor": c.cursor(0, None), "bundle": c.reference(self.root/"manifest.json"),
                        "typed_schema_sha256": p.digest(self.manifest["typed_tables"])}
        b.atomic(self.root/"context.json", b.canonical(self.context))
        self.index = {"transactions": [], "final_cursor": c.cursor(0, None),
                      "context": c.reference(self.root/"context.json")}
        b.atomic(self.root/"index.json", b.canonical(self.index))
        self.config = {"bundle": c.reference(self.root/"manifest.json"),
                       "buffer_index": c.reference(self.root/"index.json"),
                       "admission_config": {"protocol": "oo.source-admission.config/v1", "source_id": "oracle_fill_unit"}}
        self.bootstrap = {"source_id": "oracle_fill_unit", "source_generation": 1,
                          "lease_id": "unit-task", "lease_generation": 3, "previous": None, "resume": None}
        self.plan = r.initial_fill_plan(self.config, self.index, self.context, self.manifest)

    def tearDown(self):
        self.tmp.cleanup()

    def state(self, sequence, complete=False):
        ack_ref = None
        if sequence:
            ack = {"protocol": "oo.jobs.oracle-c1-baseline-ack/v1", "stage": "typed_batch", "durable": True,
                   "receipt": {"protocol": "oo.c1.typed-ack/v3", "source": "oracle_fill_unit",
                               "task": "unit-task", "generation": 2, "epoch": 7, "sequence": sequence,
                               "position": "0", "digest": self.plan["batches"][sequence-1]["digest"]}}
            b.atomic(self.root/"ack.json", b.canonical(ack))
            ack_ref = c.reference(self.root/"ack.json")
        return {"protocol": r.FILL_RESUME, "engine": "oracle", "source_id": "oracle_fill_unit",
                "task_id": "unit-task", "source_generation": 1, "lease_generation": 2, "source_epoch": 7,
                "config_digest": self.plan["config_digest"], "schema_digest": self.plan["typed_schema_sha256"],
                "buffer_index": self.config["buffer_index"], "buffer_context": self.index["context"],
                "bundle_sha256": self.context["bundle"]["sha256"], "initial_fill": self.plan,
                "state": "open", "resume_sequence": sequence, "completed_tables": ["APP.T"] if complete else [],
                "last_ack": ack_ref}

    def check(self, resume):
        return r.stream_state(self.config, {**self.bootstrap, "resume": resume},
                              self.index, self.context, self.manifest)

    def test_initial_plan_is_fill_sequence_not_source_cursor(self):
        self.assertEqual(self.plan["batch_count"], 3)
        self.assertEqual(self.plan["tables"], {"APP.T": {"last_sequence": 3, "rows": 6}})
        self.assertEqual(self.index["final_cursor"], c.cursor(0, None))
        self.assertEqual(self.check(None)["sequence"], 0)

    def test_durable_checkpoint_controls_skip_even_at_position_zero(self):
        for sequence in range(4):
            value = self.check(self.state(sequence))
            self.assertEqual(value["sequence"], sequence)
            self.assertEqual(value["source_epoch"], 7)
        mutated = self.state(1)
        mutated["resume_sequence"] = 2
        with self.assertRaisesRegex(ValueError, "checkpoint_or_complete"):
            self.check(mutated)

    def test_wrong_context_schema_epoch_task_and_future_ack_refused(self):
        for key, value in (("source_id", "foreign"), ("task_id", "foreign"), ("schema_digest", "wrong"),
                           ("buffer_index", {}), ("buffer_context", {}), ("bundle_sha256", "wrong"),
                           ("source_epoch", 8), ("lease_generation", 4), ("source_generation", 2)):
            resume = copy.deepcopy(self.state(1))
            resume[key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.check(resume)
        resume = self.state(1)
        ack = json.loads((self.root/"ack.json").read_text())
        ack["receipt"]["sequence"] = 4
        b.atomic(self.root/"future.json", b.canonical(ack))
        resume["last_ack"] = c.reference(self.root/"future.json")
        with self.assertRaisesRegex(ValueError, "resume_ack"):
            self.check(resume)

    def test_partial_fill_cannot_restore_complete_or_sealed_state(self):
        for sequence in (0, 1, 2):
            with self.assertRaises(ValueError):
                self.check(self.state(sequence, True))
            resume = self.state(sequence)
            resume["state"] = "awaiting_publish"
            with self.assertRaisesRegex(ValueError, "unsealed"):
                self.check(resume)
        resume = self.state(3, True)
        resume["state"] = "awaiting_publish"
        self.assertEqual(self.check(resume)["completed_tables"], ["APP.T"])

    def test_unconfirmed_table_end_is_reissued_after_all_fill_acks(self):
        state = self.check(self.state(3))
        self.assertEqual(state["sequence"], 3)
        self.assertEqual(state["completed_tables"], [])

    def test_ack_digest_and_durability_bound_to_same_fill(self):
        for field, value in (("digest", "0"*64), ("position", "1"), ("task", "other"), ("generation", 3)):
            resume = self.state(1)
            ack = json.loads((self.root/"ack.json").read_text())
            ack["receipt"][field] = value
            b.atomic(self.root/"altered.json", b.canonical(ack))
            resume["last_ack"] = c.reference(self.root/"altered.json")
            with self.assertRaisesRegex(ValueError, "resume_ack"):
                self.check(resume)
        resume = self.state(1)
        ack = json.loads((self.root/"ack.json").read_text())
        ack["durable"] = False
        b.atomic(self.root/"not-durable.json", b.canonical(ack))
        resume["last_ack"] = c.reference(self.root/"not-durable.json")
        with self.assertRaisesRegex(ValueError, "resume_ack"):
            self.check(resume)

    def test_mixed_capture_not_silently_granted_fill_resume(self):
        index = {**self.index, "transactions": [{"unit": "cdc"}]}
        self.assertIsNone(r.initial_fill_plan(self.config, index, self.context, self.manifest))
        with self.assertRaises(ValueError):
            r.stream_state(self.config, {**self.bootstrap, "resume": self.state(0)},
                           index, self.context, self.manifest)

    def test_plan_refuses_noncontiguous_or_nonfill_batches(self):
        for key, value in (("sequence", 7), ("kind", "cdc"), ("position", "1")):
            frames = copy.deepcopy(self.frames)
            frames[1]["batch"][key] = value
            b.atomic(self.root/"batches.jsonl", b"".join(b.canonical(f)+b"\n" for f in frames))
            manifest = copy.deepcopy(self.manifest)
            manifest["artifacts"]["batches.jsonl"] = b.sha(self.root/"batches.jsonl")
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, "initial_fill_batch"):
                r.initial_fill_plan(self.config, self.index, self.context, manifest)


if __name__ == "__main__":
    unittest.main()
