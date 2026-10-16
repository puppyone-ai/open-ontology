#!/usr/bin/env python3
import json
import hashlib
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools/agent-product-verifier"))
import dialogue

import normal_whole


class NormalWholeTests(unittest.TestCase):
    def setUp(self):
        self.rows = [
            {"key": "1", "amount": "22.50", "label": "updated\t ", "occurred": "2026-10-14T08:00:00", "payload": "00ff"},
            {"key": "25", "amount": "-25.50", "label": None, "occurred": "2026-10-16T18:19:20", "payload": "2505"},
            {"key": "26", "amount": "-26.75", "label": None, "occurred": "2026-10-16T20:21:22", "payload": "2626"},
            {"key": "27", "amount": "27.27", "label": "luna-max-cycle-g-recovery", "occurred": "2026-10-16T19:20:21", "payload": "2727"},
        ]

    def test_two_new_random_question_groups_have_typed_independent_truth(self):
        ((bank_a, gold_a), (bank_b, gold_b)) = normal_whole.make_dialogue_materials(self.rows)
        self.assertEqual(len(bank_a), 4)
        self.assertEqual(len(bank_b), 4)
        self.assertTrue(set(gold_a).isdisjoint(gold_b))
        for bank, truth in ((bank_a, gold_a), (bank_b, gold_b)):
            dialogue.validate_bank(bank, truth)
            selected = dialogue.draw(bank, truth, 3841941, 2, [])
            self.assertEqual(len(selected), 2)
            self.assertEqual({item["query_class"] for item in selected}, {"row_fact", "aggregate"})
            self.assertTrue(all(set(item) == set(dialogue.CANDIDATE_KEYS) for item in bank))
            self.assertFalse(any("sql" in item for item in bank))

    def test_recovery_requires_same_task_through_ack_and_final_lease(self):
        task = "8bc2bd04-4a7b-49b3-b294-6fd6881ab912"
        ack = {"task": task, "task_state": "succeeded", "lease_generation": 4,
               "load": {"loader_invoked": True, "mirror_committed": True,
                        "jobs_lease_receipt": {"generation": 4}}}
        wait = {"task": {"id": task, "attempt": 2}}
        killed = {"task_row": json.dumps({"id": task, "attempt": 3}),
                  "ungranted_full_import_locks": "0", "active_native_runtime_queries": "0"}
        restart = {"same_container_id": True, "configured_seconds": 86400, "process_sha256": "a" * 64}
        self.assertEqual(normal_whole.verify_recovery(ack, wait, killed, restart, "a" * 64), task)
        killed["task_row"] = json.dumps({"id": "different-task", "attempt": 3})
        with self.assertRaises(normal_whole.Refused):
            normal_whole.verify_recovery(ack, wait, killed, restart, "a" * 64)

    def test_prior_dialogue_attempt_refuses_any_new_random_draw(self):
        with tempfile.TemporaryDirectory() as d:
            run = Path(d)
            (run / "dialogue-a-gold.json").write_text("{}\n")
            normal_whole.assert_fresh_dialogue_slot(run, "a")
            attempt = run / "attempts" / "dialogue-a-1234-deadbeef"
            attempt.mkdir(parents=True)
            (attempt / "selection.json").write_text(json.dumps({"seed": 10}))
            normal_whole.record_dialogue_attempt_failure(
                attempt, run, "a", {"source_id": "oracle_fixture"}, "b" * 64,
                kind="timeout", exit_code=None, timed_out=True,
                stdout=b"partial stdout", stderr=b"partial stderr", reason="dialogue_timeout")
            with self.assertRaisesRegex(normal_whole.Refused, "prior_dialogue_attempt_requires_review"):
                normal_whole.assert_fresh_dialogue_slot(run, "a")
            failure = json.loads((attempt / "failed-attempt.json").read_text())
            self.assertTrue(failure["timed_out"])
            self.assertEqual(failure["source_id"], "oracle_fixture")
            self.assertEqual(failure["selection_sha256"], normal_whole.sha(attempt / "selection.json"))
            self.assertEqual((attempt / "runner.stderr").read_text(), "partial stderr")
            normal_whole.assert_fresh_dialogue_slot(run, "b")

    def test_active_release_requires_same_immutable_candidate_and_test_scope(self):
        release = {"contract": "oo.release-control-api/v1", "operation": "getActiveRelease",
                   "project_id": "oracle", "candidate_id": "oracle-c5-g15-test",
                   "version": "oracle-c5-g15-test", "manifest_sha256": "a" * 64,
                   "pack_digest": "b" * 64, "certificate_class": "test_scope_not_production",
                   "production_ready": False, "customer_gate_verified": False}
        pack = {"contract": release["contract"], "operation": "getActiveRolePack",
                "project_id": "oracle", "candidate_id": release["candidate_id"],
                "version": release["version"], "manifest_sha256": release["manifest_sha256"],
                "pack_digest": release["pack_digest"]}
        ready = {"release_version": release["version"], "pack_digest": release["pack_digest"]}
        self.assertEqual(normal_whole.verify_active_release(release, pack, ready), release["candidate_id"])
        ready["release_version"] = "stale-candidate"
        with self.assertRaisesRegex(normal_whole.Refused, "active_release"):
            normal_whole.verify_active_release(release, pack, ready)

    def test_dialogue_verification_binds_exact_truth_seed_and_transcript_qids(self):
        with tempfile.TemporaryDirectory() as d:
            folder = Path(d) / "dialogue"
            folder.mkdir()
            gold = Path(d) / "gold.json"
            gold.write_text('{"fixture":true}\n')
            truth_sha = hashlib.sha256(gold.read_bytes()).hexdigest()
            ids = ["fact.one", "aggregate.two"]
            qids = ["qid-1", "qid-2"]
            (folder / "selection.json").write_text(json.dumps({
                "mode": "random", "counts_as_random": True, "seed": 42,
                "semantic_ids": ids, "gold_sha256": truth_sha,
                "identity": {"source_id": "oracle_fixture", "source_generation": 15,
                             "pack_sha256": "b" * 64, "role": "oo_r_oracle_orders"}}))
            (folder / "verdict.json").write_text(json.dumps({
                "outcome": "verified", "model": "grok-4.7", "reasoning_effort": "low",
                "turns": 2, "selection": "random", "counts_as_random": True,
                "seed": 42, "semantic_ids": ids, "gold_sha256": truth_sha,
                "truth_sha256": truth_sha, "tools": ["query", "freshness", "describe", "knowledge_cat"],
                "query_ids": qids, "audit_sha256": "c" * 64}))
            (folder / "transcript.json").write_text(json.dumps([
                {"turn": 1, "query_id": qids[0]}, {"turn": 2, "query_id": qids[1]}]))
            self.assertEqual(normal_whole.verify_dialogue_folder(
                folder, {"source_id": "oracle_fixture"}, "b" * 64, gold)["query_ids"], qids)
            selection = json.loads((folder / "selection.json").read_text())
            selection["seed"] = 43
            (folder / "selection.json").write_text(json.dumps(selection))
            with self.assertRaisesRegex(normal_whole.Refused, "dialogue_truth"):
                normal_whole.verify_dialogue_folder(folder, {"source_id": "oracle_fixture"}, "b" * 64, gold)


if __name__ == "__main__":
    unittest.main()
