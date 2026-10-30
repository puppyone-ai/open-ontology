import copy
import json
import sqlite3
from pathlib import Path
import tempfile
import unittest

import bounded_source as b


TABLE = {"owner": "OWN", "table": "ORDERS", "object": ["1|2026-10-27"],
         "columns": [{"name": "ID", "type": "NUMBER"}, {"name": "CODE", "type": "VARCHAR2"},
                     {"name": "AMOUNT", "type": "NUMBER"}, {"name": "RAW", "type": "RAW"}],
         "keys": ["ID", "CODE"], "issues": []}


class FakeDriver:
    digest = "a"*64

    def __init__(self, rows=None):
        self.rows = rows if rows is not None else [[str(i), str(i % 3), str(i*17), "0000ff"] for i in range(31)]
        self.scn = "123"
        self.table = copy.deepcopy(TABLE)
        self.verify_calls = 0

    def call(self, action, **kw):
        if action == "inventory":
            return {"identity": "DBID|INC|PDBUID|PDB|DB|READER", "source_id": "fixture",
                    "scn": self.scn, "tables": [self.table], "scope": {"owner": "OWN"}}
        if action == "verify":
            self.verify_calls += 1
            part = json.loads(Path(kw["part"]).read_text())
            return {"typed_cells": len(part["rows"])*4, "differences": 0}
        lower = kw["lower"]
        first = next((i+1 for i, r in enumerate(self.rows) if r[:2] == lower), 0)
        rows = self.rows[first:first+kw["chunk_rows"]]
        return {"protocol": "oo.oracle.bounded-chunk/v1", "table": self.table, "scn": kw["scn"],
                "lower_exclusive": lower, "upper_inclusive": rows[-1][:2] if rows else lower,
                "rows": rows, "row_count": len(rows), "logical_bytes": sum(len(b.canonical(r))+1 for r in rows),
                "eof": len(rows) < kw["chunk_rows"], "reader_mutations": 0}


class BoundedTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.driver = FakeDriver()

    def tearDown(self):
        self.temp.cleanup()

    def capture(self, name="a", **kw):
        return b.capture(self.driver, self.root / name, chunk_rows=7, **kw)

    def test_many_chunks_and_resume_pinned_scn(self):
        self.assertFalse(self.capture(stop_after=2)["complete"])
        self.driver.scn = "999"
        result = self.capture()
        self.assertEqual(result["binding"]["scn"], "123")
        self.assertEqual(result["rows"], 31)
        self.assertEqual(result["typed_cells"], 124)
        self.assertEqual(len(result["parts"]), 5)
        self.assertTrue(all(p["rows"] <= 7 for p in result["parts"]))
        self.assertFalse(result["source_ack"])
        b.load_sealed(self.root/"a", b.sha(self.root/"a"/"manifest.json"))

    def test_crash_after_seal_rederived_before_ack(self):
        with self.assertRaisesRegex(RuntimeError, "after_seal"):
            self.capture(failpoint="after_seal")
        result = self.capture()
        self.assertEqual(result["rows"], 31)
        self.assertEqual(self.driver.verify_calls, 5)

    def test_tamper_rejects_resume_and_c5(self):
        self.capture(stop_after=1)
        path = self.root/"a"/"part-00000000.json"
        path.write_text("{}")
        with self.assertRaisesRegex(ValueError, "digest_mismatch"):
            self.capture()

    def test_resume_identity_schema_binary_and_budget_drift(self):
        self.capture(stop_after=1)
        self.driver.table["object"] = ["recreated"]
        with self.assertRaisesRegex(ValueError, "binding_drift"):
            self.capture()
        self.driver.table = copy.deepcopy(TABLE)
        self.driver.digest = "b"*64
        with self.assertRaisesRegex(ValueError, "binding_drift"):
            self.capture()

    def test_no_key_lob_requires_explicit_approval(self):
        self.driver.table["issues"] = ["LOB_not_streamed", "no_key"]
        with self.assertRaisesRegex(ValueError, "inventory_rejected"):
            self.capture()
        with self.assertRaisesRegex(ValueError, "empty_scope"):
            self.capture(exclusions={"OWN.ORDERS": "fixture only; excluded with reason"})

    def test_boundary_and_duplicate_key_guards(self):
        self.driver.rows.insert(1, self.driver.rows[0])
        with self.assertRaises(sqlite3.IntegrityError):
            self.capture()

    def test_c5_updates_deletes_inserts_and_nochange(self):
        self.capture()
        self.driver.rows = [row for row in self.driver.rows if row[0] != "0"]
        self.driver.rows[0][2] = "99999999999999999999999999999999999999"
        self.driver.rows.append(["100", "9", "0.000000000000000000000000001", None])
        self.driver.scn = "124"
        self.capture("b")
        receipt = b.c5_diff(self.root/"a", self.root/"b", self.root/"diff",
                            b.sha(self.root/"a"/"manifest.json"), b.sha(self.root/"b"/"manifest.json"))
        self.assertEqual(receipt["counts"], {"insert": 1, "update": 1, "delete": 1})
        self.capture("c")
        nochange = b.c5_diff(self.root/"b", self.root/"c", self.root/"same",
                             b.sha(self.root/"b"/"manifest.json"), b.sha(self.root/"c"/"manifest.json"))
        self.assertTrue(nochange["empty_diff"])
        self.assertEqual(nochange["method"], "C5")
        self.assertEqual(nochange["c1"], "not_run")

    def test_structured_error_and_eof_guard(self):
        part = self.driver.call("chunk", scn="123", lower=None, chunk_rows=7)
        part["eof"] = True
        with self.assertRaisesRegex(ValueError, "eof"):
            b.check_part(part, TABLE, "123", None, 7, 2048)

    def test_resume_cannot_reset_or_raise_budget(self):
        self.capture(stop_after=1, budget_rows=20)
        with self.assertRaisesRegex(ValueError, "durable_read_budget"):
            self.capture(budget_rows=20)
        with self.assertRaisesRegex(ValueError, "durable_read_budget"):
            self.capture(budget_rows=20)
        with self.assertRaisesRegex(ValueError, "binding_drift"):
            self.capture(budget_rows=40)

    def test_crashed_attempt_remains_debited(self):
        with self.assertRaisesRegex(RuntimeError, "after_seal"):
            self.capture(failpoint="after_seal", budget_rows=20)
        with self.assertRaisesRegex(ValueError, "durable_read_budget"):
            self.capture(budget_rows=20)


if __name__ == "__main__":
    unittest.main()
