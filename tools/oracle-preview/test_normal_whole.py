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

    def test_all_seven_tool_error_contracts_are_narrow_and_nonzero_only(self):
        names = ["tables", "describe", "knowledge_cat", "knowledge_search", "validate", "query", "freshness"]
        self.assertEqual(set(dialogue.TOOLS), set(names))
        expected_usage = {
            "tables": [],
            "describe": ["table is missing", "table is not public"],
            "knowledge_cat": ["path is missing", "path is not public"],
            "knowledge_search": ["search text is missing", "search text is not public"],
            "validate": ["sql is missing"],
            "query": ["sql is missing"],
            "freshness": [],
        }
        sql_codes = ["SQL_PARSE_ERROR", "STATEMENT_NOT_ALLOWED", "FUNCTION_NOT_ALLOWED",
                     "RELATION_NOT_FOUND", "COLUMN_NOT_ALLOWED", "UNQUALIFIED_RELATION", "COST_TOO_HIGH"]
        expected_gateway = {
            "tables": {},
            "describe": {"RELATION_NOT_FOUND": "table is not in this pack"},
            "knowledge_cat": {"SCHEMA_INVALID": "knowledge path is not in this pack"},
            "knowledge_search": {},
            "validate": {code: ("relation is not visible" if code == "RELATION_NOT_FOUND" else
                                "relation must be schema-qualified" if code == "UNQUALIFIED_RELATION" else None)
                         for code in sql_codes},
            "query": {code: ("relation is not visible" if code == "RELATION_NOT_FOUND" else
                             "relation must be schema-qualified" if code == "UNQUALIFIED_RELATION" else None)
                      for code in sql_codes},
            "freshness": {},
        }
        self.assertEqual(normal_whole.ORACLE_TOOL_ERROR_POLICY["usage"], expected_usage)
        self.assertEqual(normal_whole.ORACLE_TOOL_ERROR_POLICY["gateway"], expected_gateway)

        def usage(message):
            return {"class": "usage", "exit": 2, "message": message,
                    "retry": {"attempt_count": "unknown", "max_extra_attempts": 3, "max_wait_ms": 2000,
                              "nested_cli_retries": 0, "outcome": "not_applicable"},
                    "retryable": False}

        def gateway(code, message):
            return {"class": "gateway", "code": code, "exit": 2, "message": message,
                    "retry": {"attempt_count": "unknown", "max_extra_attempts": 3, "max_wait_ms": 2000,
                              "nested_cli_retries": 0, "outcome": "not_replayed"},
                    "retryable": False}

        # These four response bodies are copied from retained failed-run transcripts.
        captured = [
            ("query", {"class": "gateway", "code": "RELATION_NOT_FOUND", "exit": 2,
                       "message": "relation is not visible", "query_id": "q_pvw4yU1njd95YYbwHAdtYmwz1s",
                       "retry": {"attempt_count": "unknown", "max_extra_attempts": 3, "max_wait_ms": 2000,
                                 "nested_cli_retries": 0, "outcome": "not_replayed"}, "retryable": False},
             {"query", "validate"}),
            ("query", {"class": "gateway", "code": "UNQUALIFIED_RELATION", "exit": 2,
                       "message": "relation must be schema-qualified", "query_id": "q_T4XEZRGTLDz72nSMgBEmHJC5L1",
                       "retry": {"attempt_count": "unknown", "max_extra_attempts": 3, "max_wait_ms": 2000,
                                 "nested_cli_retries": 0, "outcome": "not_replayed"}, "retryable": False},
             {"query", "validate"}),
            ("knowledge_cat", {"class": "gateway", "code": "SCHEMA_INVALID", "exit": 2,
                                "message": "knowledge path is not in this pack",
                                "retry": {"attempt_count": "unknown", "max_extra_attempts": 3, "max_wait_ms": 2000,
                                          "nested_cli_retries": 0, "outcome": "not_replayed"},
                                "retryable": False}, {"knowledge_cat"}),
            ("describe", {"class": "gateway", "code": "RELATION_NOT_FOUND", "exit": 2,
                           "message": "table is not in this pack",
                           "retry": {"attempt_count": "unknown", "max_extra_attempts": 3, "max_wait_ms": 2000,
                                     "nested_cli_retries": 0, "outcome": "not_replayed"},
                           "retryable": False}, {"describe"}),
        ]
        cases = list(captured)
        usage_tools = {}
        for name, messages in expected_usage.items():
            for message in messages:
                usage_tools.setdefault(message, set()).add(name)
        cases.extend(("usage", usage(message), tools) for message, tools in usage_tools.items())
        for name in ("query", "validate"):
            for code in sql_codes:
                if code not in {"RELATION_NOT_FOUND", "UNQUALIFIED_RELATION"}:
                    cases.append((name, gateway(code, "SQL guard refusal"), {"query", "validate"}))

        for origin, body, allowed_tools in cases:
            with self.subTest(origin=origin, code=body.get("code"), message=body["message"]):
                for name in names:
                    self.assertEqual(normal_whole.oracle_tool_error_returned(dialogue.tool_error_returned, name, body),
                                     name in allowed_tools, name)
                self.assertFalse(normal_whole.oracle_tool_error_returned(
                    dialogue.tool_error_returned, origin,
                    {**body, "exit": 0}))
                self.assertFalse(normal_whole.oracle_tool_error_returned(
                    dialogue.tool_error_returned, origin,
                    {**body, "retryable": True}))

        fatal = [
            {"class": "gateway", "code": "SCHEMA_INVALID", "exit": 2,
             "message": "catalog identity invalid", "retryable": False},
            {"class": "gateway", "code": "SCHEMA_INVALID", "exit": 2,
             "message": "knowledge pack is not loaded", "retryable": False},
            {"class": "gateway", "code": "AUTH_DENIED", "exit": 2, "message": "permission denied", "retryable": False},
            {"class": "gateway", "code": "INTERNAL_ERROR", "exit": 2, "message": "internal failure", "retryable": False},
            {"class": "gateway", "code": "AUDIT_UNAVAILABLE", "exit": 2, "message": "audit unavailable", "retryable": False},
            {"class": "gateway", "code": "UNRECOGNIZED_CODE", "exit": 2, "message": "unknown", "retryable": False},
            {"class": "auth", "exit": 4, "message": "unauthenticated", "retryable": False},
            {"class": "transport", "exit": 3, "message": "connection failed", "retryable": False},
            {"class": "protocol", "exit": 3, "message": "malformed response", "retryable": False},
            {"class": "local", "exit": 1, "message": "invalid local configuration", "retryable": False},
            {"class": "sandbox", "exit": 1, "message": "unsupported execution", "retryable": False},
            {"class": "unsupported", "exit": 2, "message": "not implemented", "retryable": False},
        ]
        for body in fatal:
            with self.subTest(fatal=body.get("code", body["class"])):
                for name in names:
                    self.assertFalse(normal_whole.oracle_tool_error_returned(dialogue.tool_error_returned, name, body))

        # Only the two reviewed RELATION_NOT_FOUND/UNQUALIFIED messages are actionable.
        for name, body in (
            ("query", gateway("RELATION_NOT_FOUND", "relation exists but is hidden")),
            ("validate", gateway("UNQUALIFIED_RELATION", "relation is not visible")),
            ("describe", gateway("RELATION_NOT_FOUND", "relation is not visible")),
            ("knowledge_cat", gateway("SCHEMA_INVALID", "catalog identity invalid")),
        ):
            self.assertFalse(normal_whole.oracle_tool_error_returned(dialogue.tool_error_returned, name, body))

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

    def test_b_frozen_retry_accepts_failed_final_without_successful_query_and_rejects_a_query(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            truth = {f"truth_{i}": {"expected_named_facts": {"value": str(i)}} for i in range(4)}
            truth_path = root / "gold.json"
            truth_bytes = (json.dumps(truth, indent=2) + "\n").encode()
            truth_path.write_bytes(truth_bytes)
            truth_path.chmod(0o600)
            bank = [{"semantic_id": f"fact_{i}", "query_class": "rows" if i < 2 else "aggregate",
                     "prompt": f"Inspect independent fact {i}", "truth_key": f"truth_{i}"}
                    for i in range(4)]
            identity = {"source_id": "oracle_fixture", "source_generation": 15,
                        "pack_sha256": "b" * 64, "role": "reader"}
            config = {"question_bank": bank, "independent_truth_file": str(truth_path),
                      "sample_size": 2, "identity": identity}
            manifest, _ = dialogue.freeze_selection(config, truth, truth_bytes)
            attempt = root / "dialogue-b-1792127303512105000-7f65f626"
            attempt.mkdir(mode=0o700)
            selection_path = attempt / "selection.json"
            selection_bytes = (json.dumps(manifest, indent=2) + "\n").encode()
            selection_path.write_bytes(selection_bytes)
            selection_path.chmod(0o600)
            transcript = [
                {"turn": 0, "step": 2, "model": {"choices": [{"message": {"tool_calls": [{"id": "call-query"}]}}]}},
                {"turn": 0, "tool": "query", "exit": 2, "response": {"code": "FUNCTION_NOT_ALLOWED"}},
                {"turn": 0, "step": 4, "model": {"choices": [{"message": {"tool_calls": None,
                                                                                 "content": "unverified final retained"}}]}},
            ]
            transcript_path = attempt / "transcript.json"
            transcript_path.write_text(json.dumps(transcript, indent=2) + "\n")
            transcript_path.chmod(0o600)
            failure_path = attempt / "failed-attempt.json"
            failure_path.write_text(json.dumps({
                "protocol": "oo.oracle.dialogue-failed-attempt/v1", "failure_kind": "exit_nonzero",
                "exit_code": 2, "timed_out": False, "source_id": identity["source_id"],
                "source_generation": identity["source_generation"], "pack_sha256": identity["pack_sha256"],
                "truth_sha256": hashlib.sha256(truth_bytes).hexdigest(),
                "selection_sha256": hashlib.sha256(selection_bytes).hexdigest(),
                "transcript_sha256": hashlib.sha256(transcript_path.read_bytes()).hexdigest(),
                "verdict_sha256": None}) + "\n")
            failure_path.chmod(0o600)

            frozen = normal_whole.frozen_dialogue_config(
                config, selection_path, dialogue, {"source_id": identity["source_id"]}, identity["pack_sha256"], "b")
            self.assertEqual(frozen["frozen_manifest"], str(selection_path.resolve()))
            self.assertEqual(selection_path.read_bytes(), selection_bytes)
            self.assertIn("unverified final retained", transcript_path.read_text())

            transcript.append({"turn": 0, "tool": "query", "exit": 0, "response": {"query_id": "q-success"}})
            transcript_path.write_text(json.dumps(transcript, indent=2) + "\n")
            failure = json.loads(failure_path.read_text())
            failure["transcript_sha256"] = hashlib.sha256(transcript_path.read_bytes()).hexdigest()
            failure_path.write_text(json.dumps(failure) + "\n")
            with self.assertRaisesRegex(normal_whole.Refused, "dialogue_frozen_successful_query"):
                normal_whole.frozen_dialogue_config(
                    config, selection_path, dialogue, {"source_id": identity["source_id"]},
                    identity["pack_sha256"], "b")

    def test_fixed_base_import_receives_only_generic_tool_use_guidance(self):
        plan = normal_whole.load_plan()
        module = normal_whole.load_fixed_dialogue(
            plan["runtime"]["dialogue"], plan["runtime"]["dialogue_sha256"], plan)
        self.assertTrue(module.SYSTEM.startswith(dialogue.SYSTEM))
        self.assertIn("knowledge_search", module.SYSTEM)
        self.assertIn("knowledge_cat only with a path returned by that search", module.SYSTEM)
        self.assertIn("error query_id as non-evidence", module.SYSTEM)
        self.assertIn("revise the query or search the Pack", module.SYSTEM)
        self.assertNotIn("SELECT", module.SYSTEM)
        self.assertNotIn("order_25_amount", module.SYSTEM)

    def test_original_b_selection_is_retryable_only_without_any_successful_query(self):
        plan = normal_whole.load_plan()
        failure = next(item for item in plan["dialogue_history"]["prior_failed_attempts"]
                       if item.get("suffix") == "b")
        b_attempt = Path(failure["path"]).parent
        selection_path = b_attempt / "selection.json"
        transcript_path = b_attempt / "transcript.json"
        config = normal_whole.read_json(b_attempt.parents[1] / "dialogue-b-config.json")
        module = normal_whole.load_fixed_dialogue(
            plan["runtime"]["dialogue"], plan["runtime"]["dialogue_sha256"], plan)
        before = hashlib.sha256(transcript_path.read_bytes()).hexdigest()
        retried = normal_whole.frozen_dialogue_config(
            config, selection_path, module, plan["source"], plan["service"]["role_pack_sha256"], "b")
        expected_sha = next(item["sha256"] for item in plan["immutable_files"]
                            if item["name"] == "dialogue_b_failed_selection")
        self.assertEqual(hashlib.sha256(selection_path.read_bytes()).hexdigest(), expected_sha)
        self.assertEqual(retried["frozen_manifest"], str(selection_path.resolve()))
        self.assertEqual(before, hashlib.sha256(transcript_path.read_bytes()).hexdigest())

    def test_historical_a_replay_is_observed_as_a_real_fixed_base_child(self):
        plan = normal_whole.load_plan()
        with tempfile.TemporaryDirectory() as d:
            run = Path(d)
            run.chmod(0o700)
            # The original A transcript and gold are kept in the same immutable failed run.
            a_source = Path(plan["immutable_files"][[x["name"] for x in plan["immutable_files"]].index(
                "dialogue_a_verified_selection")]["path"]).parents[2]
            import shutil
            shutil.copyfile(a_source / "dialogue-a-gold.json", run / "dialogue-a-gold.json")
            (run / "dialogue-a-gold.json").chmod(0o600)
            journal = {"steps": [{"name": plan["dialogue_history"]["step_name"],
                                  "state": "running", "observed_programs": []}]}
            (run / "journal.json").write_text(json.dumps(journal) + "\n")
            (run / "journal.json").chmod(0o600)
            command = [plan["runtime"]["python"], str(Path(normal_whole.__file__).resolve()),
                       "reuse-dialogue", str(run), "a"]
            child = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                     text=True, start_new_session=True)
            observed = False
            deadline = time.monotonic() + 30
            while child.poll() is None and time.monotonic() < deadline:
                found = onboard.observe_children(child.pid)
                if plan["runtime"]["dialogue"] in found:
                    journal["steps"][0]["observed_programs"] = [plan["runtime"]["dialogue"]]
                    temp = run / ".journal.tmp"
                    temp.write_text(json.dumps(journal) + "\n")
                    temp.chmod(0o600)
                    os.replace(temp, run / "journal.json")
                    observed = True
                time.sleep(0.05)
            stdout, stderr = child.communicate(timeout=30)
            self.assertEqual(child.returncode, 0, stderr)
            self.assertTrue(observed)
            result = json.loads(stdout)
            self.assertEqual(result["evidence_mode"], "historical_reuse")
            self.assertEqual(result["fresh_model_calls"], 0)
            self.assertEqual(normal_whole.verify_dialogue(
                run, "a", plan["source"], plan["service"]["role_pack_sha256"])["evidence_mode"],
                "historical_reuse")

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
