"""Synthetic captured records plus the real offline Arrow codec, not source acceptance."""
import json
import os
from pathlib import Path
import sqlite3
import tempfile
import unittest

import bounded_source as b
import buffer_cursor as c
import c1_debezium as spool
import producer as p


@unittest.skipUnless(os.environ.get("OO_ORACLE_NATIVE_BIN"), "fixed native codec required")
class OrderedTransactionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.binary = Path(os.environ["OO_ORACLE_NATIVE_BIN"]).resolve()
        self.table = {"owner": "UNIT", "table": "T", "keys": ["ID"], "columns": [
            {"name": "ID", "type": "NUMBER", "precision": 9, "scale": 0, "nullable": "N"},
            {"name": "N", "type": "NUMBER", "precision": 18, "scale": 0, "nullable": "Y"}]}
        self.wire = {"type": "struct", "name": "UNIT_ONLY_NOT_VENDOR"}
        self.source = {"connector": "oracle", "db": "FREEPDB1", "schema": "UNIT",
                       "table": "T", "scn": "90"}
        db = spool.connect(self.root / "capture.sqlite")
        for key, value, state in [("1", "10", "first"), ("2", "20", "last")]:
            spool.receive(db, spool.canonical({"schema": self.wire, "payload": {
                "op": "r", "source": dict(self.source, snapshot=state), "before": None,
                "after": {"ID": key, "N": value}}}))
        operations = [
            ("u", {"ID": "1", "N": "10"}, {"ID": "1", "N": "11"}),
            ("u", {"ID": "1", "N": "11"}, {"ID": "1", "N": "12"}),
            ("d", {"ID": "2", "N": "20"}, None),
            ("c", None, {"ID": "2", "N": "21"}),
        ]
        for order, (op, before, after) in enumerate(operations, 1):
            spool.receive(db, spool.canonical({"schema": self.wire, "payload": {
                "op": op, "before": before, "after": after,
                "transaction": {"id": "UNITTX", "total_order": order},
                "source": dict(self.source, snapshot="false", scn="150", commit_scn="900",
                               txId="UNITTX", rs_id="UNIT-"+str(order), ssn=0)}}))
        spool.receive(db, spool.canonical({"status": "END", "id": "UNITTX", "event_count": 4,
            "data_collections": [{"data_collection": "FREEPDB1.UNIT.T", "event_count": 4}]}))
        db.close()
        schema = self.root / "schema.json"
        b.atomic(schema, b.canonical({"binding": {"source_id": "oracle_ordered_UNIT_ONLY",
            "identity": "42|7|"+"A"*32+"|FREEPDB1|FREE|UNIT_READER",
            "inventory": [self.table], "driver_sha256": b.sha(self.binary)}}))
        for name in ["offset", "history"]:
            (self.root / name).write_bytes(b"UNIT_ONLY opaque bytes, not vendor evidence")
        self.config = {"protocol": "oo.oracle.arrow-producer-config/v1", "engine": "oracle",
            "source_id": "oracle_ordered_UNIT_ONLY", "table": "UNIT.T", "mode": "debezium",
            "chunk_rows": 1, "reader": c.reference(self.binary), "schema_manifest": c.reference(schema),
            "spool": c.reference(self.root / "capture.sqlite"),
            "connector_offsets": c.reference(self.root / "offset"),
            "connector_history": c.reference(self.root / "history")}

    def test_explicit_policy_keeps_all_raw_events_end_counts_and_operation_history(self):
        with self.assertRaisesRegex(ValueError, "repeated_key_transaction"):
            p.build(self.config, self.root / "legacy")
        self.assertFalse((self.root / "legacy/manifest.json").exists())
        config = dict(self.config, transaction_key_policy="ordered_final_state/v1")
        manifest = p.build(config, self.root / "ordered")
        frames = [json.loads(line) for line in (self.root / "ordered/batches.jsonl").read_text().splitlines()]
        batch = frames[-1]["batch"]
        self.assertEqual(batch["protocol"], p.ORDERED_PROTOCOL)
        self.assertEqual(sum(part["rows"] for part in batch["parts"]), 4)
        self.assertEqual(manifest["checkpoints"][0]["event_count"], 4)
        self.assertEqual(manifest["encoded_rows"], 6)
        self.assertEqual(manifest["final_rows"], 2)
        self.assertEqual(manifest["operation_counts"], {"u": 2, "d": 1, "c": 1})
        with sqlite3.connect(self.root / "ordered/producer-journal.sqlite") as db:
            self.assertEqual(db.execute("SELECT count(*) FROM operations").fetchone()[0], 4)
            rows = [json.loads(raw) for raw, in db.execute("SELECT row_json FROM rows ORDER BY key")]
            self.assertEqual(rows, [["1", "12"], ["2", "21"]])
        c.build(config, self.root / "ordered/manifest.json",
                b.sha(self.root / "ordered/manifest.json"), self.root / "buffer")
        index = json.loads((self.root / "buffer/index.json").read_text())
        payload = json.loads(Path(index["transactions"][0]["path"]).read_text())
        self.assertEqual(payload["source_position"]["event_count"], 4)
        self.assertEqual(payload["source_position"]["end_event_count"], 4)
        self.assertEqual(payload["batch"], batch)

    def test_wrong_intermediate_before_image_cannot_be_hidden_by_last_operation(self):
        path = self.root / "capture.sqlite"
        with sqlite3.connect(path) as db:
            event = json.loads(db.execute("SELECT body FROM events WHERE ord=2").fetchone()[0])
            event["before"]["N"] = "999"
            db.execute("UPDATE events SET body=? WHERE ord=2", (spool.canonical(event),))
        db.close()
        config = dict(self.config, spool=c.reference(path), transaction_key_policy="ordered_final_state/v1")
        with self.assertRaisesRegex(ValueError, "independent_journal_before_mismatch"):
            p.build(config, self.root / "bad-before")
        self.assertFalse((self.root / "bad-before/manifest.json").exists())

    def test_unknown_policy_refuses_before_creating_output(self):
        for value in [None, True, "ignore_duplicates"]:
            with self.assertRaisesRegex(ValueError, "transaction_key_policy"):
                p.build(dict(self.config, transaction_key_policy=value), self.root / "wrong-policy")
        self.assertFalse((self.root / "wrong-policy").exists())
