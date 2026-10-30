"""Synthetic >100k transaction through the real offline codec, not source acceptance."""
import contextlib
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


@unittest.skipUnless(os.environ.get("OO_ORACLE_NATIVE_BIN"), "fixed offline codec required")
class LargeTransactionTests(unittest.TestCase):
    def test_complete_transaction_and_bad_last_event_preserve_atomic_journal(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            reader = Path(os.environ["OO_ORACLE_NATIVE_BIN"]).resolve()
            path = root / "capture.sqlite"
            columns = [{"name": "ID", "type": "NUMBER", "precision": 9, "scale": 0, "nullable": "N"},
                       {"name": "N", "type": "NUMBER", "precision": 18, "scale": 0, "nullable": "Y"}]
            tables = [{"owner": "UNIT", "table": name, "keys": ["ID"], "columns": columns}
                      for name in ("A", "B")]
            with contextlib.closing(spool.connect(path)) as db:
                for table, key, marker in (("A", "1", "first"), ("A", "2", "last_in_data_collection"),
                                           ("B", "1", "first_in_data_collection"), ("B", "2", "last")):
                    spool.receive(db, spool.canonical({"schema": {"type": "struct", "name": "UNIT_ONLY."+table},
                        "payload": {"op": "r", "before": None, "after": {"ID": key, "N": "0"},
                            "source": dict(connector="oracle", db="DB", schema="UNIT", table=table,
                                           scn="90", snapshot=marker)}}))
                count, size = 101000, 50500
                with db:
                    for order in range(1, count+1):
                        table = "A" if order <= size else "B"
                        number = (order-1) % size + 1
                        spool.ingest(db, {"schema": {"type": "struct", "name": "UNIT_ONLY."+table},
                            "payload": {"op": "u", "before": {"ID": "1", "N": str(number-1)},
                                "after": {"ID": "1", "N": str(number)},
                                "transaction": {"id": "UNIT_TX", "total_order": order},
                                "source": dict(connector="oracle", db="DB", schema="UNIT", table=table,
                                    scn=str(100+order), commit_scn="999999", snapshot="false",
                                    txId="UNIT_TX", rs_id="UNIT-"+str(order), ssn=0)}})
                    spool.ingest(db, {"status": "END", "id": "UNIT_TX", "event_count": count,
                        "data_collections": [{"data_collection": "DB.UNIT."+table, "event_count": size}
                                             for table in ("A", "B")]})
                db.execute("PRAGMA wal_checkpoint(TRUNCATE)")
            schema = root / "schema.json"
            b.atomic(schema, b.canonical({"binding": {
                "source_id": "oracle_large_UNIT_ONLY", "identity": "42|7|"+"A"*32+"|PDB|DB|UNIT_READER",
                "inventory": tables, "driver_sha256": b.sha(reader)}}))
            for name in ("offset", "history"):
                (root / name).write_bytes(b"UNIT ONLY; not a native vendor checkpoint")
            config = dict(protocol="oo.oracle.arrow-producer-config/v1", engine="oracle",
                source_id="oracle_large_UNIT_ONLY", tables=["UNIT.A", "UNIT.B"], mode="debezium",
                chunk_rows=4096, transaction_key_policy="ordered_final_state/v1",
                reader=c.reference(reader), schema_manifest=c.reference(schema),
                spool=c.reference(path), connector_offsets=c.reference(root/"offset"),
                connector_history=c.reference(root/"history"))
            bundle = root / "bundle"
            manifest = p.build(config, bundle)
            batches = list(c.batch_frames(bundle, manifest))
            cdc = [batch for batch in batches if batch["kind"] == "cdc"]
            self.assertEqual(len(cdc), 1)
            self.assertEqual([part["rows"] for part in cdc[0]["parts"]], [size, size])
            self.assertEqual(cdc[0]["protocol"], p.ORDERED_PROTOCOL)
            self.assertEqual(manifest["checkpoints"][0]["event_count"], count)
            c.build(config, bundle/"manifest.json", b.sha(bundle/"manifest.json"), root/"buffer")
            verified = c.verify(root/"buffer/index.json", b.sha(root/"buffer/index.json"))
            self.assertTrue(verified["spool_evidence_verified"])
            self.assertEqual(verified["final_cursor"], c.cursor(1, "UNIT_TX"))
            with contextlib.closing(sqlite3.connect(bundle/"producer-journal.sqlite")) as db:
                actual = [json.loads(row[0]) for row in db.execute("SELECT row_json FROM rows ORDER BY table_id,key")]
            self.assertEqual(actual, [["1", str(size)], ["2", "0"], ["1", str(size)], ["2", "0"]])
            bad = root / "bad.sqlite"
            with contextlib.closing(sqlite3.connect(path.as_uri()+"?mode=ro&immutable=1", uri=True)) as source, \
                    contextlib.closing(sqlite3.connect(bad)) as dest:
                source.backup(dest)
                event = json.loads(dest.execute("SELECT body FROM events WHERE ord=?", (count,)).fetchone()[0])
                event["before"]["N"] = "not_the_prior_value"
                dest.execute("UPDATE events SET body=? WHERE ord=?", (spool.canonical(event), count))
                dest.commit()
            failed = root / "bad-bundle"
            with self.assertRaises(ValueError):
                p.build(dict(config, spool=c.reference(bad)), failed)
            self.assertFalse((failed/"manifest.json").exists())
            with contextlib.closing(sqlite3.connect(failed/"producer-journal.sqlite")) as db:
                unchanged = [json.loads(row[0]) for row in db.execute("SELECT row_json FROM rows ORDER BY table_id,key")]
            self.assertEqual(unchanged, [["1", "0"], ["2", "0"], ["1", "0"], ["2", "0"]])


if __name__ == "__main__":
    unittest.main()
