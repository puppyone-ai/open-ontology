import importlib.util
import json
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("dialogue", Path(__file__).parents[1] / "dialogue.py")
dialogue = importlib.util.module_from_spec(spec)
spec.loader.exec_module(dialogue)


class DialogueEvidence(unittest.TestCase):
    def test_denied_query_retains_error(self):
        body = {"class": "gateway", "exit": 2, "code": "FUNCTION_NOT_ALLOWED", "message": "refused", "query_id": "denied"}
        self.assertEqual(dialogue.model_view("query", body), body)

    def test_named_fact_requires_matching_successful_cell(self):
        body = {"columns": [{"name": "updated_count"}], "rows": [["1"]]}
        good = {"facts": {"updated_count": "1"}, "evidence": {"updated_count": {"query_id": "success", "column": "updated_count"}}}
        dialogue.verify_named_facts(json.dumps(good), {"updated_count": 1}, {"success": body})
        for value in ("0", True, 1.0):
            bad = {**good, "facts": {"updated_count": value}}
            with self.assertRaises(ValueError):
                dialogue.verify_named_facts(json.dumps(bad), {"updated_count": 1}, {"success": body})
        bad = {**good, "evidence": {"updated_count": {"query_id": "denied", "column": "updated_count"}}}
        with self.assertRaises(KeyError):
            dialogue.verify_named_facts(json.dumps(bad), {"updated_count": 1}, {"success": body})
        with self.assertRaises(ValueError):
            dialogue.verify_named_facts(json.dumps({**good, "answer": "update count is zero"}), {"updated_count": 1}, {"success": body})

    def test_rich_value_and_wrong_evidence_shape(self):
        result = {"columns": [{"name": "row_count"}], "rows": [["14"]]}
        report = {"facts": {"row_count": {"value": "14", "result_type": "int8"}}, "evidence": {"row_count": {"query_id": "real", "column": "row_count"}}}
        dialogue.verify_named_facts(json.dumps(report), {"row_count": 14}, {"real": result})
        with self.assertRaises(ValueError):
            dialogue.verify_named_facts(json.dumps({**report, "evidence": []}), {"row_count": 14}, {"real": result})
