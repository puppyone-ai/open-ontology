import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest

import bounded_source as b
sys.path.insert(0, str(Path(__file__).resolve().parents[2]/"testing/dl01-targets/oracle/scripts"))
import semantic_metadata as m

class SemanticMetadataTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.identity = {"engine": "oracle", "dbid": "42", "resetlogs_change": "7",
                         "pdb_guid": "A"*32, "pdb_name": "FREEPDB1", "db_unique_name": "FREE"}
        operator = {"source_id": "oracle_unit", "source_identity": self.identity,
                    "service": "FREEPDB1", "authorized_tables": ["OO_CONV_OWNER.C1_EVENTS"]}
        inventory = {"source_id": "oracle_unit", "scope": {"owner": "OO_CONV_OWNER", "all_visible_tables": True},
            "tables": [{"owner": "OO_CONV_OWNER",
            "table": "C1_EVENTS", "keys": ["ID"], "columns": [{"name": "ID", "type": "NUMBER",
            "precision": 9, "scale": 0, "nullable": "N", "bytes": 22}]}]}
        for name, doc in (("operator.json", operator), ("inventory.json", inventory)):
            b.atomic(self.root/name, b.canonical(doc))
        b.atomic(self.root/"registration.json", b.canonical({
            "source_id": "oracle_unit", "operator_sha256": b.sha(self.root/"operator.json"),
            "inventory_sha256": b.sha(self.root/"inventory.json")}))
        scope = {"owner": "OO_CONV_OWNER", "table": "C1_EVENTS"}
        self.records = [
            {"kind": "database", **{k: v for k, v in self.identity.items() if k != "engine"}},
            {"kind": "column", **scope, "name": "ID", "data_type": "NUMBER", "precision": 9,
             "scale": 0, "nullable": "N", "data_length": 22, "ordinal": 1,
             "default_length": None, "virtual_column": "NO", "identity_column": "NO"},
            {"kind": "column_comment", **scope, "name": "ID", "comment": None},
            {"kind": "table_comment", **scope, "comment": None},
            {"kind": "constraint", **scope, "name": "PK_T", "constraint_type": "P",
             "status": "ENABLED", "validated": "VALIDATED"},
            {"kind": "constraint_column", **scope, "constraint": "PK_T", "name": "ID", "position": 1}
        ]

    def tearDown(self):
        self.temp.cleanup()

    def build(self, records=None):
        return m.assemble(records or self.records, self.root, {"unit_only": True})

    def test_real_dictionary_shape_preserves_case_null_comments_and_scope(self):
        value = self.build()
        self.assertEqual(value["source_id"], "oracle_unit")
        self.assertEqual(value["tables"][0]["primary_key"], ["ID"])
        self.assertIsNone(value["tables"][0]["table_comment"])
        self.assertEqual(value["tables"][0]["foreign_key_count"], 0)
        self.assertFalse(value["business_semantics_approved"])
        self.assertIsNone(value["target_generation"])
        self.assertIsNone(value["tables"][0]["columns"][0]["default_present"])
        self.assertEqual(value["tables"][0]["columns"][0]["default_observation"]["state"], "unmeasured")

    def test_missing_columns_and_changed_types_fail(self):
        for key, value in (("precision", 8), ("nullable", "Y"),
                           ("virtual_column", "YES")):
            with self.subTest(key=key):
                records = copy.deepcopy(self.records)
                records[1][key] = value
                with self.assertRaises(ValueError):
                    self.build(records)
        with self.assertRaisesRegex(ValueError, "coverage"):
            self.build([r for r in self.records if r["kind"] != "column"])

    def test_wrong_source_or_primary_key_cannot_bind(self):
        records = copy.deepcopy(self.records)
        records[0]["db_unique_name"] = "OTHER"
        with self.assertRaisesRegex(ValueError, "identity"):
            self.build(records)
        records = copy.deepcopy(self.records)
        records[-1]["name"] = "OTHER"
        with self.assertRaisesRegex(ValueError, "constraint"):
            self.build(records)
        registration = json.loads((self.root/"registration.json").read_text())
        registration["source_id"] = "oracle_frozen_iud_20261027"
        b.atomic(self.root/"registration.json", b.canonical(registration))
        with self.assertRaisesRegex(ValueError, "binding"):
            self.build()

    def test_query_is_named_owned_dictionary_only(self):
        self.assertNotIn("SHOW CREATE", m.QUERY)
        for token in ("INSERT INTO", "UPDATE ", "DELETE FROM", "CREATE TABLE", "DROP "):
            self.assertNotIn(token, m.QUERY)
        self.assertIn("FROM dba_constraints", m.QUERY)
        self.assertIn("FROM dba_cons_columns", m.QUERY)

    def test_unmeasured_default_is_not_a_null_default_assertion(self):
        doc = self.build()
        doc["database"]["observed_scn"] = "123"
        doc["tables"][0]["columns"][0]["char_length"] = 0
        native = json.loads((self.root/"inventory.json").read_text())
        output = m.source_schema(doc, native, {"unit_only": True})
        column = output["tables"][0]["columns"][0]
        self.assertNotIn("default_sql", column)
        self.assertNotIn("default_present", column)
        self.assertEqual(column["default_observation"]["state"], "unmeasured")
        self.assertEqual(column["precision"], 9)
        self.assertEqual(column["char_length"], 0)
        self.assertEqual(output["tables"][0]["keys"], ["ID"])

if __name__ == "__main__":
    unittest.main()
