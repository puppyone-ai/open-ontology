#!/usr/bin/env python3
import json
import hashlib
import os
from pathlib import Path
import sys
import subprocess
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools/agent-product-verifier"))
import dialogue

sys.path.insert(0, str(ROOT / "tools/source-onboarding"))
import onboard
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
            attempt.parent.mkdir(mode=0o700)
            attempt.mkdir(mode=0o700)
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

    def test_dialogue_attempt_allocates_parent_but_leaves_output_for_child(self):
        with tempfile.TemporaryDirectory() as d:
            run = Path(d)
            relative, output = normal_whole.allocate_dialogue_attempt(run, "a")
            self.assertEqual(relative.parent, Path("attempts"))
            self.assertTrue(output.parent.is_dir())
            self.assertFalse(output.exists())
            output.mkdir(mode=0o700)
            self.assertEqual(normal_whole.ensure_attempt_directory(output), output)

    def test_sql_guard_correction_errors_are_returned_without_weakening_other_errors(self):
        errors = [
            {"class": "gateway", "code": "RELATION_NOT_FOUND", "message": "relation is not visible", "exit": 2},
            {"class": "gateway", "code": "UNQUALIFIED_RELATION", "message": "relation must be schema-qualified", "exit": 2},
            {"class": "gateway", "code": "SQL_PARSE_ERROR", "message": "parse failed", "exit": 2},
            {"class": "gateway", "code": "COST_TOO_HIGH", "message": "query cost exceeded", "exit": 2},
        ]
        for body in errors:
            with self.subTest(code=body["code"]):
                self.assertTrue(normal_whole.oracle_tool_error_returned(dialogue.tool_error_returned, "query", body))
                self.assertTrue(normal_whole.oracle_tool_error_returned(dialogue.tool_error_returned, "validate", body))
        self.assertFalse(normal_whole.oracle_tool_error_returned(dialogue.tool_error_returned, "describe", errors[0]))
        self.assertFalse(normal_whole.oracle_tool_error_returned(
            dialogue.tool_error_returned, "query", {**errors[0], "message": "relation exists but is hidden"}))
        self.assertFalse(normal_whole.oracle_tool_error_returned(
            dialogue.tool_error_returned, "query", {**errors[1], "message": "relation is not visible"}))
        for denied in (
            {**errors[0], "class": "usage"},
            {"class": "gateway", "code": "SCHEMA_INVALID", "message": "catalog identity invalid", "exit": 2},
            {"class": "gateway", "code": "AUTH_DENIED", "message": "permission denied", "exit": 2},
            {"class": "gateway", "code": "INTERNAL_ERROR", "message": "internal failure", "exit": 2},
            {"class": "gateway", "code": "UNRECOGNIZED_CODE", "message": "unknown", "exit": 2},
        ):
            with self.subTest(denied=denied["code"]):
                self.assertFalse(normal_whole.oracle_tool_error_returned(dialogue.tool_error_returned, "query", denied))
        allowed = {"class": "gateway", "code": "COLUMN_NOT_ALLOWED", "message": "column denied", "exit": 2}
        self.assertTrue(normal_whole.oracle_tool_error_returned(dialogue.tool_error_returned, "query", allowed))

    def test_frozen_random_selection_reuses_exact_current_identity_and_rejects_drift(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            truth = {f"truth_{i}": {"expected_named_facts": {"value": str(i)}} for i in range(4)}
            truth_path = root / "gold.json"
            truth_bytes = (json.dumps(truth, indent=2) + "\n").encode()
            truth_path.write_bytes(truth_bytes)
            truth_path.chmod(0o600)
            bank = [{"semantic_id": f"fact_{i}", "query_class": "rows" if i < 2 else "aggregate",
                     "prompt": f"Find independently grounded fact {i}", "truth_key": f"truth_{i}"}
                    for i in range(4)]
            identity = {"source_id": "oracle_fixture", "source_generation": 15,
                        "pack_sha256": "b" * 64, "role": "reader"}
            config = {"question_bank": bank, "independent_truth_file": str(truth_path),
                      "sample_size": 2, "identity": identity}
            manifest, _ = dialogue.freeze_selection(config, truth, truth_bytes)
            attempt = root / "attempt"
            attempt.mkdir(mode=0o700)
            selection_path = attempt / "selection.json"
            selection_bytes = (json.dumps(manifest, indent=2) + "\n").encode()
            selection_path.write_bytes(selection_bytes)
            transcript = [{"turn": 0, "step": 0, "model": {"choices": [{"message": {"tool_calls": [{"id": "call-1"}]}}]},
                           "tool": "query", "exit": 2}]
            transcript_path = attempt / "transcript.json"
            transcript_path.write_text(json.dumps(transcript) + "\n")
            failure_path = attempt / "failed-attempt.json"
            failure_path.write_text(json.dumps({
                "protocol": "oo.oracle.dialogue-failed-attempt/v1", "source_id": identity["source_id"],
                "source_generation": identity["source_generation"], "pack_sha256": identity["pack_sha256"],
                "truth_sha256": hashlib.sha256(truth_bytes).hexdigest(),
                "selection_sha256": hashlib.sha256(selection_bytes).hexdigest(),
                "transcript_sha256": hashlib.sha256(transcript_path.read_bytes()).hexdigest(),
                "verdict_sha256": None}) + "\n")
            frozen = normal_whole.frozen_dialogue_config(
                config, selection_path, dialogue, {"source_id": identity["source_id"]}, identity["pack_sha256"])
            self.assertEqual(frozen["seed"], manifest["seed"])
            self.assertEqual(frozen["frozen_manifest"], str(selection_path.resolve()))
            self.assertEqual(hashlib.sha256(selection_bytes).hexdigest(), hashlib.sha256(selection_path.read_bytes()).hexdigest())
            wrong_identity = json.loads(json.dumps(config))
            wrong_identity["identity"]["pack_sha256"] = "c" * 64
            with self.assertRaisesRegex(ValueError, "resume identity mismatch"):
                normal_whole.frozen_dialogue_config(
                    wrong_identity, selection_path, dialogue,
                    {"source_id": identity["source_id"]}, identity["pack_sha256"])
            changed_truth = dict(truth)
            changed_truth["truth_0"] = {"expected_named_facts": {"value": "changed"}}
            changed_path = root / "changed-gold.json"
            changed_path.write_text(json.dumps(changed_truth) + "\n")
            changed_path.chmod(0o600)
            wrong_gold = {**config, "independent_truth_file": str(changed_path)}
            with self.assertRaisesRegex(ValueError, "resume truth digest mismatch"):
                normal_whole.frozen_dialogue_config(
                    wrong_gold, selection_path, dialogue,
                    {"source_id": identity["source_id"]}, identity["pack_sha256"])

    def test_verifier_invocation_is_observed_by_normal_onboarding_before_any_model_call(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            truth_fifo = root / "truth.fifo"
            os.mkfifo(truth_fifo)
            config_path = root / "dialogue-config.json"
            config_path.write_text(json.dumps({
                "questions": [{"prompt": "offline adapter probe", "truth_key": "fixture"}],
                "independent_truth_file": str(truth_fifo),
                "model_origin": "http://not-approved",
            }) + "\n")
            config_path.chmod(0o600)
            output_path = root / "dialogue-output"
            plan = normal_whole.load_plan()
            dialogue_path = plan["runtime"]["dialogue"]
            command = [plan["runtime"]["python"], str(Path(normal_whole.__file__).resolve()),
                       "invoke-verifier", dialogue_path, plan["runtime"]["dialogue_sha256"],
                       str(config_path), str(output_path)]
            proc = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                    text=True, start_new_session=True)
            try:
                observed = []
                deadline = time.monotonic() + 5
                while time.monotonic() < deadline and proc.poll() is None:
                    observed = onboard.observe_children(proc.pid)
                    if dialogue_path in observed:
                        break
                    time.sleep(0.05)
                self.assertIn(dialogue_path, observed)
                with truth_fifo.open("wb") as writer:
                    writer.write(b'{"fixture":{}}\n')
                stdout, stderr = proc.communicate(timeout=10)
                self.assertEqual(proc.returncode, 2)
                self.assertIn('"event": "main_enter"', stderr)
                self.assertIn(dialogue_path, stderr)
                self.assertFalse(output_path.exists())
            finally:
                if proc.poll() is None:
                    proc.kill()
                    proc.communicate(timeout=5)

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
