import argparse
import copy
import json
import os
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
from urllib.parse import urlsplit

import run as ab
import live


class LiveTests(unittest.TestCase):
    def result(self):
        return {"columns": [{"name": "value", "type": "int64"}],
                "rows": [["60"]], "returned_rows": 1, "truncated": False, "warnings": []}

    def test_typed_conversion_preserves_precision_nulls_and_duplicates(self):
        result = self.result()
        result.update(columns=[{"name": "value", "type": "numeric"}],
                      rows=[["12345678901234567890.0010"], [None],
                            ["12345678901234567890.0010"]], returned_rows=3)
        typed = live.typed_result(result)
        self.assertEqual(typed["rows"][0], typed["rows"][2])
        self.assertEqual(typed["rows"][0][0]["coefficient"], "123456789012345678900010")
        self.assertEqual(typed["rows"][0][0]["scale"], 4)
        self.assertEqual(typed["rows"][1], [{"presence": "null"}])
        result["truncated"] = True
        self.assertEqual(live.typed_result(result)["completion"], "partial")
        result["columns"][0]["type"] = "float64"
        with self.assertRaisesRegex(ab.Invalid, "gateway_type_unsupported"):
            live.typed_result(result)

    def test_live_warnings_and_aliases_do_not_change_row_correctness(self):
        result = self.result()
        result["warnings"] = [{"code": "STALE_DATA", "message": "x" * 500}]
        question = ab.load(ab.ROOT / "benchmarks/ab/fixtures/demo.json")["questions"][0]
        result["columns"][0]["name"] = "count"
        result["freshness"] = {"source_time": "unmeasured"}
        scored = live.score(question, {"kind": "gateway", "gateway_result": result})
        self.assertTrue(scored["pass"])
        self.assertEqual(scored["warnings"], result["warnings"])
        self.assertEqual(scored["freshness"], result["freshness"])
        self.assertEqual(len(result["warnings"][0]["message"]), 500)

    def test_accuracy_denial_is_a_failed_answer_not_a_missing_trial(self):
        question = {"track": "accuracy", "expected_kind": "rows"}
        result = live.score(question, {"kind": "gateway_denied",
                                      "gateway_result": {"code": "COLUMN_NOT_ALLOWED"}})
        self.assertFalse(result["pass"])
        self.assertFalse(result["no_output"])
        self.assertEqual(result["reason"], "gateway_denial")
        with tempfile.TemporaryDirectory(dir=ab.ROOT / "evidence") as directory:
            result = ab.execute_recording(
                question, {"kind": "denied", "code": "POLICY_DENIED"},
                Path(directory) / "trial", None)
        self.assertEqual(result, {"pass": False, "no_output": False, "reason": "model_refusal"})

    def test_accuracy_gold_cannot_be_changed_to_a_denial(self):
        dataset = copy.deepcopy(ab.load(ab.ROOT / "benchmarks/ab/fixtures/demo.json"))
        dataset["questions"][0]["expected_kind"] = "denied"
        with self.assertRaisesRegex(ab.Invalid, "accuracy_denial"):
            ab.validate_dataset(dataset)

    def client(self, values):
        values = iter(values)
        def call(_):
            value = next(values)
            if isinstance(value, Exception):
                raise value
            return iter([value])
        return SimpleNamespace(model={"model": ab.MODEL}, call=call,
                               url=urlsplit("https://synthetic.invalid/v1"))

    def completion(self, content):
        return {"choices": [{"message": {"content": content}}],
                "usage": {"prompt_tokens": 10, "completion_tokens": 5}}

    def test_retry_is_bounded_and_auth_is_not_retried(self):
        response = self.completion('{"sql":"SELECT 1"}')
        client = self.client([live.egress.Refusal("UPSTREAM_TIMEOUT"), response])
        with patch.object(live.time, "sleep"):
            got = live.model_response(client, {}, 42, 2)
        self.assertEqual(got["attempts"], 2)
        self.assertEqual(got["model_response"], response)
        client = self.client([live.egress.Refusal("UPSTREAM_AUTH")])
        self.assertEqual(live.model_response(client, {}, 42, 2)["attempts"], 1)

    def test_invalid_or_empty_model_output_counts_as_no_output(self):
        for content in ("", "```sql\nSELECT 1\n```", '{"sql":""}', '{"sql":1}', "null"):
            result = live.model_response(self.client([self.completion(content)]), {}, 42, 0)
            self.assertEqual(result["kind"], "no_output")
        denied = live.model_response(self.client([self.completion('{"refusal":true}')]), {}, 42, 0)
        self.assertEqual(denied["kind"], "denied")

    def test_gateway_uses_private_sql_file_and_rejects_publication_drift(self):
        binding = {"publication_id": "pub1_" + "a" * 64,
                   "data_vector_sha256": "b" * 64, "knowledge_version": "v1",
                   "policy_version": "p1"}
        result = dict(self.result(), knowledge_version="v1", policy_version="p1",
                      publication_binding={k: binding[k] for k in
                                           ("publication_id", "data_vector_sha256")})
        def execute(args, **kwargs):
            path = Path(args[-1])
            self.assertEqual(path.read_text(), "SELECT 60")
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            self.assertNotIn("SELECT 60", args)
            return SimpleNamespace(returncode=0, stdout=json.dumps(result))
        with patch.object(live.subprocess, "run", side_effect=execute), patch.object(
                live.ab, "load", return_value={"profile": "default"}):
            self.assertEqual(live.gateway(Path("oo"), "default", binding, "SELECT 60"), result)
        result["knowledge_version"] = "changed"
        with patch.object(live.subprocess, "run", return_value=SimpleNamespace(
                returncode=0, stdout=json.dumps(result))), patch.object(
                live.ab, "load", return_value={"profile": "default"}):
            with self.assertRaisesRegex(ab.Invalid, "publication_drift"):
                live.gateway(Path("oo"), "default", binding, "SELECT 60")

        for code in ("COLUMN_NOT_ALLOWED", "RELATION_NOT_FOUND", "ENUMERATION_REFUSED"):
            denied = dict(code=code, query_id="query-fixture", **{"class": "gateway"})
            with patch.object(live.subprocess, "run", return_value=SimpleNamespace(
                    returncode=1, stdout=json.dumps(denied))), patch.object(
                    live.ab, "load", return_value={"profile": "default"}):
                self.assertTrue(live.gateway(Path("oo"), "default", binding, "SELECT 60")["denied"])

    def test_captured_run_replays_byte_identically(self):
        with tempfile.TemporaryDirectory(dir=ab.ROOT / "evidence") as tmp:
            root = Path(tmp)
            fixture = ab.ROOT / "benchmarks/ab/fixtures/demo.json"
            dataset = ab.load(fixture)
            dataset["publication"] = {
                "publication_id": "pub1_" + "a" * 64, "data_vector_sha256": "b" * 64,
                "knowledge_version": "v1", "policy_version": "p1"}
            for ref in dataset["inputs"].values():
                ref["path"] = os.path.relpath((fixture.parent / ref["path"]).resolve(), root)
            ab.write(root / "input.json", dataset)
            for name, value in (("endpoint", "http://127.0.0.1:9/v1"), ("key", "synthetic-key")):
                path = root / name
                path.touch(mode=0o600)
                path.write_text(value)
            args = argparse.Namespace(
                model=ab.MODEL, model_endpoint_file=root / "endpoint",
                model_key_file=root / "key", oo=Path("oo"), profile="test",
                concurrency=2, timeout=1, retries=0, output=root / "capture")
            result = dict(self.result(), knowledge_version="v1", policy_version="p1",
                          publication_binding={k: dataset["publication"][k] for k in
                                               ("publication_id", "data_vector_sha256")})
            response = {"kind": "gateway", "sql": "SELECT 60",
                        "sql_sha256": ab.digest(b"SELECT 60"),
                        "model_response": self.completion('{"sql":"SELECT 60"}'), "attempts": 1}
            for rejected in (None, {"denied": True}, {"failed": True, "code": "TIMEOUT"},
                             dict(result, rows=[["59"]])):
                with patch.object(live, "gateway", side_effect=[result, rejected]), patch.object(
                        live, "model_response") as model:
                    with self.assertRaisesRegex(ab.Invalid, "accuracy_(not_answerable|gold_drift)"):
                        live.capture(args, root / "input.json")
                    model.assert_not_called()
                    self.assertFalse(args.output.exists())
            def gateway(_cli, _profile, _binding, sql):
                value = copy.deepcopy(result)
                if "SUM(amount)" in sql:
                    value["columns"][0]["type"] = "numeric"
                    value["rows"] = [["3517.5000"]]
                elif "demo.calendar" in sql:
                    value["rows"] = [["31"]]
                return value
            with patch.object(live, "gateway", side_effect=gateway), patch.object(
                    live, "model_response", side_effect=lambda *_: copy.deepcopy(response)):
                first = live.capture(args, root / "input.json")
            second = ab.run(root / "capture/dataset.json", root / "replay")
            self.assertEqual(first, second)
            self.assertEqual((root / "capture/scored/report.json").read_bytes(),
                             (root / "replay/report.json").read_bytes())
            recording = ab.load(root / "capture/responses.json")
            self.assertEqual(len(recording["responses"]), 10)
            self.assertNotIn("synthetic-key", (root / "capture/responses.json").read_text())
            self.assertEqual((root / "capture/responses.json").stat().st_mode & 0o777, 0o600)
            failed = recording["responses"]["c3:q01"]
            failed["kind"] = "query_failed"
            failed["gateway_result"] = {"failed": True, "code": "COST_TOO_HIGH"}
            path = root / "capture/responses.json"
            path.write_text(ab.canonical(recording))
            manifest = ab.load(root / "capture/dataset.json")
            manifest["inputs"]["responses"]["sha256"] = ab.digest(path.read_bytes())
            (root / "capture/dataset.json").write_text(ab.canonical(manifest))
            ab.run(root / "capture/dataset.json", root / "failure-first")
            ab.run(root / "capture/dataset.json", root / "failure-replay")
            self.assertEqual((root / "failure-first/report.json").read_bytes(),
                             (root / "failure-replay/report.json").read_bytes())
            scored = ab.load(root / "failure-first/report.json")["questions"][0]["c3"]
            self.assertEqual(scored["code"], "COST_TOO_HIGH")
            self.assertFalse(scored["pass"])
            # Captured receipts must still be checked on replay.
            response = recording["responses"]["c0:q01"]
            response["gateway_result"]["publication_binding"]["publication_id"] = "pub1_" + "c" * 64
            path = root / "capture/responses.json"
            path.write_text(ab.canonical(recording))
            manifest = ab.load(root / "capture/dataset.json")
            manifest["inputs"]["responses"]["sha256"] = ab.digest(path.read_bytes())
            (root / "capture/dataset.json").write_text(ab.canonical(manifest))
            with self.assertRaisesRegex(ab.Invalid, "publication_binding"):
                ab.run(root / "capture/dataset.json", root / "bad-replay")

    def test_gateway_failure_retains_closed_code_not_remote_text(self):
        binding = {"publication_id": "pub1_" + "a" * 64,
                   "data_vector_sha256": "b" * 64}
        for code in sorted(live.QUERY_FAILURE_CODES):
            error = {"class": "gateway", "code": code, "query_id": "q_ABC123",
                     "message": "private upstream diagnostic", "hint": "private hint"}
            with patch.object(live.subprocess, "run", return_value=SimpleNamespace(
                    returncode=1, stdout=json.dumps(error))), patch.object(
                    live.ab, "load", return_value={"profile": "default"}):
                result = live.gateway(Path("oo"), "default", binding, "SELECT 1")
            self.assertEqual(result, {"failed": True, "code": code, "query_id": "q_ABC123"})
            scored = live.score({"track": "permission"}, {
                "kind": "query_failed", "gateway_result": result})
            self.assertFalse(scored["pass"])  # Operational errors are not policy successes.
            self.assertEqual(scored["code"], code)
        for error in ([], {"class": "gateway", "code": "UNTRUSTED_REMOTE_TEXT"}):
            with patch.object(live.subprocess, "run", return_value=SimpleNamespace(
                    returncode=1, stdout=json.dumps(error))), patch.object(
                    live.ab, "load", return_value={"profile": "default"}):
                self.assertIsNone(live.gateway(Path("oo"), "default", binding, "SELECT 1"))
