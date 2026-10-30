import copy
import json
import os
from pathlib import Path
import sqlite3
import sys
import tempfile
import unittest

import bounded_source as b
import oracle_operator as operator
import producer as p

sys.path.insert(0, str(Path(__file__).resolve().parents[2]/"testing/dl01-targets/oracle/scripts"))
import frozen_iud

class FormalSourceTests(unittest.TestCase):
    def test_frozen_catalog_only_allows_safe_added_dictionary_flags(self):
        old = [{"name": "ID", "type": "NUMBER", "precision": 9, "scale": 0, "nullable": "N"}]
        new = [{**old[0], "position": 1, "hidden": "NO", "virtual": "NO", "user_generated": "YES"}]
        self.assertTrue(operator.captured_columns_match(old, new))
        for key, value in (("precision", 8), ("hidden", "YES"), ("virtual", "YES"),
                           ("position", 2), ("unknown_flag", "NO")):
            with self.subTest(key=key):
                self.assertFalse(operator.captured_columns_match(old, [{**new[0], key: value}]))

    def test_exact_oracle_input_not_mysql_alias(self):
        good = {"type": "oracle", "host": "127.0.0.1", "port": 50626,
                "service": "FREEPDB1", "database": "FREE", "user": "R", "secret_ref": "secret://r"}
        operator.validate_input(good)
        for key, value in (("type", "mysql"), ("host", "example.com"), ("port", True),
                           ("secret_ref", "plaintext"), ("service", "")):
            bad = {**good, key: value}
            with self.assertRaises(ValueError):
                operator.validate_input(bad)
        del good["service"]
        with self.assertRaises(ValueError):
            operator.validate_input(good)

    def test_frozen_journal_is_4000_4000_2000_not_old_extension(self):
        with tempfile.TemporaryDirectory() as tmp:
            receipt = frozen_iud.plan(Path(tmp))
            self.assertEqual(receipt["operations"], {"insert": 4000, "update": 4000, "delete": 2000})
            self.assertEqual(receipt["final_rows"], 12000)
            self.assertFalse(receipt["mutation_executed"])
            with sqlite3.connect(receipt["journal"]["path"]) as db:
                self.assertEqual(db.execute("SELECT COUNT(*) FROM operations").fetchone()[0], 10000)
                self.assertEqual(db.execute("SELECT COUNT(*) FROM baseline").fetchone()[0], 10000)
                amount = json.loads(db.execute("SELECT row_json FROM final WHERE id=2001").fetchone()[0])[1]
                self.assertEqual(amount, "-99999999999999997998.123456789")
                self.assertIsNone(db.execute("SELECT row_json FROM final WHERE id=2000").fetchone())
                self.assertEqual(json.loads(db.execute("SELECT row_json FROM final WHERE id=14000").fetchone()[0])[1], "-0.000000001")
            with self.assertRaisesRegex(ValueError, "journal_exists"):
                frozen_iud.plan(Path(tmp))

    @unittest.skipUnless(os.environ.get("OO_ORACLE_REAL_ACK") and os.environ.get("OO_ORACLE_BUFFER_INDEX"),
                         "retained real worker ACK and source buffer required")
    def test_retained_real_worker_ack_and_tamper_rejection(self):
        ack = json.loads(Path(os.environ["OO_ORACLE_REAL_ACK"]).read_text())
        index = json.loads(Path(os.environ["OO_ORACLE_BUFFER_INDEX"]).read_text())
        payload = json.loads(p.pinned(index["transactions"][-1]).read_text())
        wrapper = {"durable": True, "receipt": ack}
        p.validate_typed_ack(wrapper, payload["batch"], {"source_id": payload["source_id"],
                             "lease_id": ack["task"], "lease_generation": ack["generation"]}, ack["epoch"])
        p.validate_buffer_ack(wrapper, payload["source_position"])
        for key, value in (("source_evidence_sha256", "0"*64), ("source_offset_write", True),
                           ("buffer_cursor", {"transaction_ordinal": "0", "transaction_id": None})):
            bad = copy.deepcopy(wrapper)
            bad["receipt"][key] = value
            with self.assertRaisesRegex(ValueError, "buffer_ack"):
                p.validate_buffer_ack(bad, payload["source_position"])

    @unittest.skipUnless(os.environ.get("OO_ORACLE_BUFFER_INDEX"), "retained real bundle required")
    def test_metadata_is_separate_from_typed_schema_and_matches_codec(self):
        index = json.loads(Path(os.environ["OO_ORACLE_BUFFER_INDEX"]).read_text())
        context = json.loads(p.pinned(index["context"]).read_text())
        root = p.pinned(context["bundle"]).parent
        manifest = p.validate_bundle(root, context["bundle"]["sha256"])
        metadata = p.bundle_metadata(root, manifest)
        self.assertEqual(metadata["typed_schema_sha256"], context["typed_schema_sha256"])
        self.assertNotEqual(metadata["schema_digest"], metadata["typed_schema_sha256"])
        self.assertEqual(metadata["schema_digest"], p.digest(metadata["schema_preimage"]))
        self.assertEqual(metadata["type_digest"], p.digest(metadata["type_preimage"]))
        self.assertEqual(metadata["schema_preimage"][0]["columns"][0]["key"], True)
        self.assertTrue(all(t["source_type"] != "mysql" for t in metadata["type_preimage"]))

if __name__ == "__main__":
    unittest.main()
