import copy
import unittest
from unittest.mock import Mock

import library_selfbuild as lib
import report_definitions as reports
from observation_test_support import observed_fixture


def fixture():
    table = {"dataset": "app.reports", "physical": '"_oo"."reports"', "primary_key": [],
             "source_id": "fixture", "source_generation": 1, "target_generation": 1,
             "columns": [{"name": c, "physical_name": "p_" + c, "data_type": "text",
                          "profile": {"rows": 1, "distinct": 1}}
                         for c in ["tenant", "company", "name", "sql_text"]]}
    config = {"dataset": "app.reports", "columns": ["name", "sql_text"],
              "search_columns": ["name"], "terms": ["receipts_%'"],
              "scope": {"tenant": "101", "company": "1001"}}
    return observed_fixture([table]), config


class ReportDefinitionsTest(unittest.TestCase):
    def test_sql_is_evidence_not_executed_or_sent_to_model(self):
        tables, config = fixture()
        stored = [{"name": "receipts", "sql_text": "DELETE FROM app.private"}]
        db = Mock(query=Mock(return_value=stored))
        evidence = reports.discover(db, tables, config)
        db.query.assert_called_once()
        self.assertNotIn("DELETE", db.query.call_args.args[0])
        self.assertIn("strpos(", db.query.call_args.args[0])
        self.assertNotIn(" ilike ", db.query.call_args.args[0])
        facts = lib.build_facts(tables, lambda *args: {}, reports=evidence)
        lib.require_complete_facts(facts, report_config=config)
        self.assertNotIn("DELETE", str(lib.draft_batches(facts)))
        _, _, audit = lib.effective_documents(facts, [])
        self.assertTrue(any(r["kind"] == "stored_report_definitions" for r in audit["decisions"]))
        bad = copy.deepcopy(facts)
        bad["report_definitions_observation"]["definitions"][0]["name"] = "changed"
        bad["facts_sha256"] = lib.digest({k: v for k, v in bad.items() if k != "facts_sha256"})
        with self.assertRaisesRegex(ValueError, "report_definitions_binding"):
            lib.require_complete_facts(bad)

    def test_scope_overflow_and_drift_fail_closed(self):
        tables, config = fixture()
        for override in ({"scope": {}}, {"max_rows": True}, {"max_rows": 1001},
                         {"columns": ["name", "missing"]}):
            db = Mock()
            with self.assertRaises(ValueError):
                reports.discover(db, tables, dict(config, **override))
            db.query.assert_not_called()
        rows = [{"name": "receipts", "sql_text": "SELECT 1"}]
        with self.assertRaisesRegex(ValueError, "truncated"):
            reports.discover(Mock(query=Mock(return_value=rows * 201)), tables, config)
        with self.assertRaisesRegex(ValueError, "bytes"):
            reports.discover(Mock(query=Mock(return_value=rows)), tables, dict(config, max_bytes=1))
        facts = lib.build_facts(tables, lambda *args: {})
        with self.assertRaisesRegex(ValueError, "config_drift"):
            lib.require_complete_facts(facts, report_config=config)


if __name__ == "__main__":
    unittest.main()
