"""Offline transaction/proof guards. Optional real-packet tests never contact source."""
import copy
import json
import os
from pathlib import Path
import sqlite3
import tempfile
import unittest

import bounded_source as b
import buffer_cursor as c
import producer as p
import snapshot_log_join as join


class TransactionTests(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(":memory:")
        self.db.executescript("""
            CREATE TABLE events(tx TEXT,ord INTEGER,body BLOB);
            CREATE TABLE boundaries(tx TEXT,expected INTEGER,body BLOB);
            CREATE TABLE snapshots(seq INTEGER,body BLOB);
        """)
        self.add("before", "99")
        self.add("equal", "100")
        self.add("cut", "200")

    def tearDown(self):
        self.db.close()

    def add(self, tx, scn):
        self.db.execute("INSERT INTO events VALUES (?,1,?)",
                        (tx, b.canonical({"source": {"commit_scn": scn}})))
        self.db.execute("INSERT INTO boundaries VALUES (?,1,?)",
                        (tx, b.canonical({"status": "END", "id": tx, "event_count": 1})))

    def test_whole_transactions_strictly_after_snapshot_at_actual_cut(self):
        selected = join.selected_transactions(self.db, "100", "200")
        self.assertEqual([t["transaction_id"] for t in selected], ["cut"])

    def test_missing_or_incomplete_end_never_seals(self):
        self.db.execute("DELETE FROM boundaries WHERE tx='cut'")
        with self.assertRaisesRegex(ValueError, "incomplete_end"):
            join.selected_transactions(self.db, "100", "200")
        self.db.execute("INSERT INTO boundaries VALUES ('cut',2,?)",
                        (b.canonical({"status": "END", "id": "cut", "event_count": 2}),))
        with self.assertRaisesRegex(ValueError, "incomplete_end"):
            join.selected_transactions(self.db, "100", "200")

    def test_false_end_and_synthetic_cut_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "actual_closed_cut"):
            join.selected_transactions(self.db, "100", "201")
        self.db.execute("UPDATE boundaries SET body=? WHERE tx='cut'",
                        (b.canonical({"status": "BEGIN", "id": "cut", "event_count": 1}),))
        with self.assertRaisesRegex(ValueError, "incomplete_end"):
            join.selected_transactions(self.db, "100", "200")

    def test_no_fabricated_debezium_snapshot_rows(self):
        self.db.execute("INSERT INTO snapshots VALUES (1,?)", (b"{}",))
        with self.assertRaisesRegex(ValueError, "no_data_capture"):
            join.selected_transactions(self.db, "100", "200")


@unittest.skipUnless(os.environ.get("OO_ORACLE_JOIN_CONFIG"), "set frozen real packet for offline validation")
class FrozenPacketTests(unittest.TestCase):
    def setUp(self):
        self.config = json.loads(Path(os.environ["OO_ORACLE_JOIN_CONFIG"]).read_text())
        self.proof = json.loads(Path(self.config["source_resume_proof"]["path"]).read_text())
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def validate(self, mutate):
        config, proof = copy.deepcopy(self.config), copy.deepcopy(self.proof)
        mutate(proof)
        b.atomic(self.root/"proof.json", b.canonical(proof))
        config["source_resume_proof"] = c.reference(self.root/"proof.json")
        return join.validate_config(config)

    def replace_evidence(self, proof, key, mutate):
        evidence = json.loads(Path(proof[key]["path"]).read_text())
        mutate(evidence)
        b.atomic(self.root/(key+".json"), b.canonical(evidence))
        proof[key] = c.reference(self.root/(key+".json"))

    def test_actual_resumed_packet(self):
        proof = join.validate_config(self.config)
        self.assertEqual(proof["snapshot_scn"], "2547118")
        self.assertEqual(proof["cut_scn"], "2550799")

    def test_source_identity_schema_cut_and_restart_are_bound(self):
        for field, value in (("source_id", "wrong"), ("table", "OTHER.TABLE"),
                             ("identity", "wrong"), ("snapshot_scn", "2547119"),
                             ("cut_scn", "2550800"), ("source_snapshot_restarted", True),
                             ("source_offset_synthesized", True)):
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.validate(lambda p: p.update({field: value}))

    def test_partial_prefix_cannot_be_replaced(self):
        with self.assertRaisesRegex(ValueError, "retained_prefix"):
            self.validate(lambda p: self.replace_evidence(p, "partial_snapshot",
                lambda v: v["parts"][0].__setitem__(1, "0"*64)))

    def test_catalog_gap_cannot_be_hidden_by_claiming_covered(self):
        with self.assertRaisesRegex(ValueError, "source_log_gap"):
            self.validate(lambda p: self.replace_evidence(p, "log_catalog",
                lambda v: v.update(ranges=[r for r in v["ranges"]
                    if int(r["first_scn"]) > int(p["log_start_scn"])])))

    def test_unfinished_snapshot_cannot_be_joined(self):
        with self.assertRaisesRegex(ValueError, "resume_evidence"):
            self.validate(lambda p: self.replace_evidence(p, "snapshot_resume_state",
                lambda v: v.update(status="interrupted")))

    def test_runtime_must_pin_transitive_proof_helpers(self):
        bundle = Path(os.environ["OO_ORACLE_JOIN_CONFIG"]).parents[1]/"joined-bundle-01/manifest.json"
        config = {"protocol": "oo.oracle.producer-argv/v1", "engine": "oracle",
                  "bundle": c.reference(bundle)}
        b.atomic(self.root/"argv.json", b.canonical(config))
        with self.assertRaisesRegex(ValueError, "source_resume_helper_pin"):
            p.serve(self.root/"argv.json", self.root/"not-created", 1)
        self.assertFalse((self.root/"not-created").exists())


if __name__ == "__main__":
    unittest.main()
