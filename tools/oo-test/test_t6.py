import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import types
import unittest
from unittest.mock import Mock, patch

import t6
from t6_fixture import GoldFixture
from t6_reviewed import ReviewedGoldFixture, completeness


class GoldDialogueTests(unittest.TestCase):
    def test_agent_egress_timeout_keeps_existing_call_budget(self):
        egress = Mock()
        with tempfile.TemporaryDirectory() as root, patch.object(
                t6, "load_module", return_value=egress):
            model = t6.Model(Path(root))
            egress.Egress.assert_called_once_with(
                t6.MODEL, Path(root) / "usage.jsonl", rpm=120, timeout=300)
            self.assertEqual(model.calls, 0)
            self.assertEqual(t6.MAX_CALLS, 160)
            model.close()
            egress.Egress.return_value.close.assert_called_once_with()

    def test_numeric_comparison_is_lossless_and_ordered(self):
        value = "12345678901234567890123456789012345678.1200"
        self.assertEqual(t6.cell(value), value[:-2])
        self.assertTrue(t6.classify({"rows": [["1.00"]]}, [["1"]], queried=True)["equal"])
        self.assertFalse(t6.classify({"rows": [["2"], ["1"]]}, [["1"], ["2"]],
                                     queried=True)["equal"])
        self.assertFalse(t6.classify({"rows": [[1.5]]}, [["1.5"]], queried=True)["equal"])

    def test_null_is_not_zero_or_text(self):
        for value in (0, "0", "NULL", ""):
            report = t6.classify({"rows": [[value]]}, [[None]], queried=True)
            self.assertFalse(report["equal"])
        report = t6.classify({"rows": [[None]]}, [[None]], queried=True)
        self.assertTrue(report["equal"])
        self.assertTrue(report["null"])

    def test_truncation_refusal_and_unverified_never_pass(self):
        for flags, status in [({"truncated": True}, "truncated"),
                              ({"refused": True}, "refused"), ({}, "unverified")]:
            report = t6.classify({"rows": [["1"]]}, [["1"]], **flags)
            self.assertEqual(report["status"], status)
            self.assertFalse(report["equal"])

    def test_no_shell_or_model_selected_connection(self):
        self.assertEqual(t6.cli_arguments({"command": "query", "argument": "SELECT 1"}),
                         ["query", "SELECT 1"])
        for value in [
            {"command": "query", "argument": "--token-file=/tmp/a"},
            {"command": "whoami", "argument": "--gateway=evil"},
            {"command": "tables", "gateway": "evil"},
            {"command": "sh", "argument": "cat secret"},
            {"command": "query", "argument": "a\x00b"},
            {"command": "login", "argument": "not-a-code"},
        ]:
            with self.assertRaises(ValueError):
                t6.cli_arguments(value)

    def test_binding_requires_real_success_and_redacts_material(self):
        code = "OO-ABCD-EFGH"
        def response(command, argument=""):
            return {"choices": [{"message": {"role": "assistant", "tool_calls": [{
                "id": command, "function": {"name": "oo", "arguments":
                    json.dumps({"command": command, "argument": argument})}}]}}]}
        responses = iter([response("login", code), response("whoami")])
        model = types.SimpleNamespace(call=lambda _: next(responses))
        with tempfile.TemporaryDirectory() as root:
            prompt = t6.bind_agent(model, lambda _: {"exit": 0, "body": {}},
                                   "Bind " + code, Path(root))
            self.assertNotIn(code, prompt)
            self.assertTrue(json.loads((Path(root) / "binding.json").read_text())["verified"])
        model.call = lambda _: response("login", code)
        with tempfile.TemporaryDirectory() as root:
            with self.assertRaises(ValueError):
                t6.bind_agent(model, lambda _: {"exit": 4, "body": {}}, code, Path(root))
            self.assertFalse(json.loads((Path(root) / "binding.json").read_text())["verified"])

    def test_budget_rejects_before_egress(self):
        model = t6.Model.__new__(t6.Model)
        model.deadline = time.monotonic() + 100
        model.calls, model.reserved = t6.MAX_CALLS, 0
        with self.assertRaisesRegex(ValueError, "MODEL_CALL_LIMIT"):
            model.call([])
        model.calls = 0
        with self.assertRaisesRegex(ValueError, "MODEL_REQUEST_BUDGET"):
            model.call([{"content": "x" * t6.MAX_REQUEST_BYTES}])
        self.assertEqual(model.calls, 0)

    def test_model_receives_question_not_gold(self):
        seen = []
        responses = iter([
            {"choices": [{"message": {"role": "assistant", "tool_calls": [{
                "id": "test", "type": "function", "function": {"name": "oo", "arguments":
                    json.dumps({"command": "query", "argument": "SELECT 1"})}}]}}]},
            {"choices": [{"message": {"role": "assistant", "content": '{"rows":[["1"]]}'}}]},
        ])
        def call(messages):
            seen.append(json.dumps(messages))
            return next(responses)
        model = types.SimpleNamespace(call=call, module=types.SimpleNamespace(Refusal=RuntimeError))
        with tempfile.TemporaryDirectory() as root:
            report = t6.dialogue(model, lambda args: {"exit": 0, "body": {"rows": [["1"]]}},
                                 "share prompt", {"id": "Q01", "question": "Count?"},
                                 [["gold-never-sent"]], Path(root))
        self.assertEqual(report["status"], "mismatch")
        self.assertNotIn("gold-never-sent", "".join(seen))

    def test_entrypoint_requires_opt_in_before_resources(self):
        entry = Path(__file__).with_name("oo-test")
        for arguments, expected in [(["t6", "--help"], 0),
                                    (["t6", "--run-id", "../escape", "--execute"], 2),
                                    (["t6", "--run-id", "test-no-opt-in"], 2)]:
            result = subprocess.run([str(entry), *arguments], capture_output=True)
            self.assertEqual(result.returncode, expected)

    def test_prepare_without_cut_reports_publish_error_code(self):
        with tempfile.TemporaryDirectory() as root:
            fixture = GoldFixture.__new__(GoldFixture)
            fixture.out = Path(root)
            fixture.stack = fixture.target = Path(root) / "unused.json"
            fixture.db = types.SimpleNamespace(directory=Path(root))
            for stderr, code in [(b'{"error":"publication_seal_mismatch","status":"failed"}\n',
                                  "PREPARE_PUBLICATION_SEAL_MISMATCH"),
                                 (b"", "PREPARE_NO_CUT"),
                                 (b'{"error":"Not A Code: secret"}', "PREPARE_NO_CUT")]:
                result = subprocess.CompletedProcess([], 1, b"", stderr)
                with patch("t6_fixture.subprocess.run", return_value=result):
                    with self.assertRaises(ValueError) as caught:
                        fixture.prepare("baseline")
                self.assertEqual(str(caught.exception), code)
                self.assertEqual(t6.error_code(caught.exception), code)

    def test_catalog_uses_native_aliases_without_changing_source_names(self):
        with tempfile.TemporaryDirectory() as root:
            fixture = GoldFixture.__new__(GoldFixture)
            fixture.out = Path(root)
            fixture.schema, fixture.names = "FIXTURE", ["ORDERS"]
            fixture.columns = {"ORDERS": [{"name": "ID", "data_type": "text",
                                           "semantic_type": "identifier"}]}
            fixture.pack_request = {"effective_oos": {"ontology": {}}, "visibility": {}}
            fixture.args = types.SimpleNamespace(builder="unused")
            fixture.fixture = types.SimpleNamespace(encode=lambda v: json.dumps(v).encode(),
                                                     sha=lambda v: "test-hash")
            fixture.base_catalog = {"envelope": {"review": {}, "producer": {}}}
            fixture.command = lambda *args: types.SimpleNamespace(
                stdout=b'{"artifact":{"pack_digest":"test-pack"}}')
            cut = {"receipt": {"shards": [{"receipt": {"tables": [{
                "physical_name": "physical", "schema": "FIXTURE", "table": "ORDERS"}]}}]},
                "data_vector": [{"physical_name": "physical", "dataset": "q_fixture.q_orders",
                                 "expected_columns": [{"name": "q_id", "data_type": "text"}]}]}
            fixture.compile_catalog(cut)
            table = fixture.pack_request["effective_oos"]["datasets"][0]
            self.assertEqual(table["dataset"], "q_fixture.q_orders")
            self.assertEqual(table["columns"][0]["name"], "q_id")
            self.assertEqual(table["columns"][0]["label"], "ID")
            self.assertEqual(fixture.columns["ORDERS"][0]["name"], "ID")
            cut["data_vector"][0]["expected_columns"][0]["data_type"] = "numeric"
            with self.assertRaisesRegex(ValueError, "PACK_TARGET_TYPES"):
                fixture.compile_catalog(cut)

    def test_reviewed_path_uses_publisher_document_and_never_opens_fences(self):
        with tempfile.TemporaryDirectory() as root:
            fixture = ReviewedGoldFixture.__new__(ReviewedGoldFixture)
            fixture.out = fixture.runtime = Path(root)
            sql = []
            fixture.db = types.SimpleNamespace(sql=sql.append)
            fixture.prepare = lambda run: None
            fixture.builder_call_limit = 21
            fixture.plan_builder = lambda run: None
            fixture.fixture = types.SimpleNamespace(
                encode=lambda v: json.dumps(v).encode(), replace=lambda p, v: p.write_bytes(v))
            document = {"visibility": {
                "project": "owned", "principal": "owner", "role": "oo_r_owner",
                "policy": {"version": "draft-v1"}, "knowledge": {"version": "draft-v1"}},
                "envelope": {"producer": {"artifact": {"pack_digest": "real"}}}}
            receipt = {"outcome": "activated", "state": "ready", "gateway_document": document,
                       "receipt_sha256": "receipt", "pack_digest": "real", "fences": []}
            fixture.product = types.SimpleNamespace(Retained=types.SimpleNamespace(
                builder=lambda self, run: ({"stage": "review_required", "model_calls": 2}, Path(root)),
                review=lambda self, output: b"reviewed",
                publisher=lambda *args: (0, receipt)))
            (Path(root) / "facts.json").write_text("{}")
            (Path(root) / "draft.json").write_text("{}")
            fixture.product.Retained.review = lambda self, output: b"{}"
            with patch("t6_reviewed.completeness", return_value={"status": "passed"}):
                self.assertEqual(fixture.publish("baseline"), receipt)
            self.assertEqual(fixture.identity["role"], "oo_r_owner")
            self.assertFalse(any("fence" in s for s in sql))
            self.assertEqual(json.loads(
                (Path(root) / "reviewed/library/library.json").read_text()), document)

    def test_builder_limit_is_preflight_batches_plus_repairs(self):
        with tempfile.TemporaryDirectory() as root:
            fixture = ReviewedGoldFixture.__new__(ReviewedGoldFixture)
            fixture.out = Path(root)
            path = fixture.out / "preflight.json"
            path.write_text("{}")
            for batches in (7, 13):
                fixture.set_builder_budget({"batches": batches})
                report = json.loads(path.read_text())
                self.assertEqual(fixture.builder_call_limit, batches * 3)
                self.assertEqual(report["builder_repair_calls"], batches * 2)
                self.assertEqual(report["max_total_model_calls"], t6.MAX_CALLS + batches * 3)
            for batches in (0, -1, 1366, True, 1.5, None, "7"):
                with self.assertRaisesRegex(ValueError, "BUILDER_PREFLIGHT_BUDGET"):
                    fixture.set_builder_budget({"batches": batches})

    def test_t6_verdict_requires_all_twenty_distinct_correct_answers(self):
        report = {"publisher": "reviewed", "status": "passed", "questions": [
            {"id": f"Q{i:02}", "equal": True, "status": "correct"} for i in range(1, 21)]}
        t5 = {"status": "passed"}
        self.assertEqual(t6.suite_verdicts(report, t5)[1][2], "pass")
        for field, value in (("equal", False), ("truncated", True), ("refused", True),
                             ("error", "UPSTREAM_TIMEOUT"), ("id", "Q02")):
            bad = dict(report, questions=[dict(report["questions"][0], **{field: value}),
                                         *report["questions"][1:]])
            self.assertEqual(t6.suite_verdicts(bad, t5)[1][2], "fail")
        self.assertEqual(t6.suite_verdicts(dict(report, questions=[]), t5)[1][2], "fail")
        self.assertEqual(t6.suite_verdicts(dict(report, publisher="fixture"), t5)[0][2], "skip")
        self.assertEqual(t6.suite_verdicts(report, {})[0][2], "fail")

    def test_t5_rechecks_inventory_not_only_empty_unresolved(self):
        sys.path.insert(0, str(t6.ROOT / "tools/pg-builder/scripts"))
        import selfbuild_budget
        facts = {"tables": [{"dataset": "app.items", "columns": [{"name": "id"}]}]}
        accepted = {}
        for kind, column in (("table", None), ("column", "id")):
            key = selfbuild_budget.stable_id(kind, "app.items", column)
            accepted[key] = {"id": key, "kind": kind, "label": "item",
                             "description": "Meaning unknown"}
        draft = {"accepted": accepted, "unresolved": [], "review": {}}
        reviewed = {"status": "reviewed", "review_decision": {"mode": "self"},
                    "compiled": {"effective": {"datasets": facts["tables"]}}}
        self.assertEqual(completeness(facts, draft, reviewed)["status"], "passed")
        self.assertEqual(completeness(facts, draft, {})["status"], "failed")
        accepted.pop(key)
        self.assertEqual(completeness(facts, draft, reviewed)["status"], "failed")

    def test_versioned_sources_use_the_requested_pin_and_service(self):
        for engine, version, service, schema in (
            ("mysql", "5.7.44", "fixture", "fixture"),
            ("postgres", "13.23", "fixture", "public"),
            ("oracle", "21.3", "XEPDB1", "FIXTURE"),
            ("oracle", "11.2", "XE", "FIXTURE"),
        ):
            with self.subTest(engine=engine, version=version), tempfile.TemporaryDirectory() as root:
                fixture = GoldFixture.__new__(GoldFixture)
                fixture.out = fixture.root = Path(root)
                fixture.db = types.SimpleNamespace(directory=Path(root))
                fixture.engine, fixture.version, fixture.schema = engine, version, schema
                fixture.scale, fixture.names = 2000000, ["items"]
                fixture.worker_image = "owned-worker"
                source = Mock(id="run", name="source", reader_secret="fixture-only")
                source.name = "source"
                source.receipt = {"version_actual": version}
                source.gold.return_value = {}
                source.command.return_value = (0, "127.0.0.1:12345")
                def tls(run, store):
                    (store / "source-ca").write_text("fixture-ca")
                with patch("t6_fixture.LaneRun", return_value=source) as factory, patch(
                    "t6_fixture.configure_mysql", side_effect=tls), patch(
                    "t6_fixture.configure_pg", side_effect=tls):
                    config = fixture.external_source()
                self.assertEqual(factory.call_args.args[1:3], (engine, version))
                self.assertEqual(config["database"], service)
                self.assertEqual(config["scope"]["include"], [schema + ".items"])
                self.assertEqual(config["user"], "fixture_reader")
                if version == "11.2":
                    self.assertEqual(config["port"], 1521)
                    self.assertEqual(fixture.worker_args[0], "--oracle-worker")
                    commands = [c.args[0] for c in source.command.call_args_list]
                    create = next(c for c in commands if c[:2] == ["docker", "create"])
                    self.assertIn("oo.lane=g", create)
                    self.assertIn("linux/amd64", create)


if __name__ == "__main__":
    unittest.main()
