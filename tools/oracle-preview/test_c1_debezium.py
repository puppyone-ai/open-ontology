import json
from pathlib import Path
import tempfile
import unittest
import c1_debezium as c


def event(order=1, tx="1.2.3", commit="321"):
    return {"schema": {"type": "struct", "name": "test-v1"}, "payload": {"op": "u", "before": {"ID": 1, "N": "0"}, "after": {"ID": 1, "N": "1"},
        "source": {"connector": "oracle", "db": "FREE", "schema": "S", "table": "T", "snapshot": "false",
                   "scn": "123", "commit_scn": commit, "txId": tx},
        "transaction": {"id": tx, "total_order": order, "data_collection_order": order}}}


class C1Tests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.path = Path(self.tmp.name)/"spool.sqlite"
        self.db = c.connect(self.path)

    def tearDown(self):
        self.db.close()
        self.tmp.cleanup()

    def send(self, value):
        c.receive(self.db, c.canonical(value))

    def test_no_end_no_commit_and_gap_no_commit(self):
        self.send(event())
        self.assertEqual(list(c.committed(self.db)), [])
        self.send({"status": "END", "id": "1.2.3", "event_count": 2})
        self.assertEqual(list(c.committed(self.db)), [])
        self.send(event(2))
        self.assertEqual(list(c.committed(self.db))[0]["event_count"], 2)

    def test_replay_idempotent_conflict_and_restart(self):
        self.send(event())
        self.send(event())
        self.db.close()
        self.db = c.connect(self.path)
        self.assertEqual(self.db.execute("SELECT COUNT(*) FROM events").fetchone()[0], 1)
        bad = event()
        bad["payload"]["after"]["N"] = "2"
        with self.assertRaisesRegex(ValueError, "conflict"):
            self.send(bad)
        self.send({"status": "END", "id": "1.2.3", "event_count": 1})
        self.assertEqual(c.receipt(self.path)["committed_counts"]["u"], 1)

    def test_snapshot_is_not_c1(self):
        snap = event()
        snap["payload"]["op"] = "r"
        snap["payload"]["source"]["snapshot"] = "true"
        self.send(snap)
        result = c.receipt(self.path)
        self.assertEqual(result["captured_log_rows"], 0)
        self.assertEqual(result["snapshot_rows_not_c1"], 1)

    def test_scn_only_missing_tx_ddl_and_wrong_engine_refused(self):
        for change in ("tx", "commit", "engine", "ddl"):
            value = event()
            if change == "tx":
                value["payload"]["transaction"] = None
            elif change == "commit":
                value["payload"]["source"]["commit_scn"] = None
            elif change == "engine":
                value["payload"]["source"]["connector"] = "mysql"
            else:
                value = {"ddl": "ALTER TABLE T"}
            with self.assertRaises(ValueError):
                self.send(value)
        self.assertEqual(self.db.execute("SELECT COUNT(*) FROM transport").fetchone()[0], 0)

    def test_schema_drift_and_missing_type_evidence_refused(self):
        self.send(event())
        changed = event(2)
        changed["schema"]["name"] = "test-v2"
        with self.assertRaisesRegex(ValueError, "schema_drift"):
            self.send(changed)
        del changed["schema"]
        with self.assertRaisesRegex(ValueError, "typed_schema"):
            self.send(changed)

    def test_empty_end_does_not_manufacture_checkpoint(self):
        self.send({"status": "END", "id": "no-data", "event_count": 0})
        self.assertEqual(list(c.committed(self.db)), [])
        self.assertEqual(c.receipt(self.path)["incomplete_transactions"], 0)


if __name__ == "__main__":
    unittest.main()
