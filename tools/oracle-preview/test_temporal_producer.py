import copy
import json
import os
from pathlib import Path
import shutil
import sqlite3
import tempfile
import unittest

import producer as p
import bounded_source as b
import buffer_cursor as cursor
import c1_debezium as spool
import rollover


class TemporalProducerTests(unittest.TestCase):
    def test_native_snapshot_journal_normalizes_only_temporal_values_without_editing_input(self):
        table = {"owner": "UNIT", "table": "T", "keys": ["ID"], "columns": [
            {"name": "ID", "type": "NUMBER"},
            {"name": "TS", "type": "TIMESTAMP(6) WITH TIME ZONE"},
        ]}
        original = ["1", "2026-10-28T10:30:00.123456000 +08:00"]
        exporter = p.Exporter.__new__(p.Exporter)
        with sqlite3.connect(":memory:") as db:
            exporter.journal = db
            db.execute("CREATE TABLE rows(table_id,key,row_json)")
            exporter.journal_rows(table, [original])
            self.assertEqual(json.loads(db.execute("SELECT row_json FROM rows").fetchone()[0]),
                             ["1", "2026-10-28T02:30:00.123456000 +00:00"])
            self.assertEqual(original[1], "2026-10-28T10:30:00.123456000 +08:00")
            with self.assertRaisesRegex(ValueError, "column_coverage"):
                exporter.journal_rows(table, [original + ["unmapped"]])

    def decode(self, kind, value, name, literal="int64"):
        column = {"name": "TS", "type": kind, "nullable": "Y"}
        wire = {"type": "struct", "fields": [
            {"field": "TS", "type": literal, "name": name}]}
        return p.decode_row({"TS": value}, {"columns": [column]}, wire)[0]

    def test_exact_epoch_units_negative_values_and_calendar_boundaries(self):
        cases = [
            ("DATE", -1000, "Timestamp", "1969-12-31T23:59:59"),
            ("DATE", 951782400000, "Timestamp", "2000-02-29T00:00:00"),
            ("TIMESTAMP(3)", -1, "Timestamp", "1969-12-31T23:59:59.999000000"),
            ("TIMESTAMP(6)", -1, "MicroTimestamp", "1969-12-31T23:59:59.999999000"),
            ("TIMESTAMP(9)", -1000, "NanoTimestamp", "1969-12-31T23:59:59.999999000"),
            ("TIMESTAMP(6)", 1700000000123456, "MicroTimestamp", "2023-11-14T22:13:20.123456000"),
            ("DATE", -62135596800000, "Timestamp", "0001-01-01T00:00:00"),
            ("DATE", 253402300799000, "Timestamp", "9999-12-31T23:59:59"),
        ]
        for kind, value, name, expected in cases:
            with self.subTest(kind=kind, value=value):
                self.assertEqual(self.decode(kind, value, "io.debezium.time." + name), expected)

    def test_wire_units_are_verified_even_for_null_and_connect_precision_loss_is_refused(self):
        self.assertEqual(self.decode("DATE", 0, "org.apache.kafka.connect.data.Timestamp"),
                         "1970-01-01T00:00:00")
        self.assertIsNone(self.decode("TIMESTAMP(6)", None, "io.debezium.time.MicroTimestamp"))
        for value in (0, None):
            for name, literal in (("io.debezium.time.Timestamp", "int64"),
                                  ("org.apache.kafka.connect.data.Timestamp", "int64"),
                                  ("io.debezium.time.MicroTimestamp", "string")):
                with self.assertRaisesRegex(ValueError, "wire_schema"):
                    self.decode("TIMESTAMP(6)", value, name, literal)
        for value in (True, 1.5, "1000", 2**63):
            with self.assertRaisesRegex(ValueError, "temporal_integer"):
                self.decode("DATE", value, "io.debezium.time.Timestamp")

    def test_no_submicrosecond_rounding_or_source_precision_reinterpretation(self):
        for kind, value, name, error in [
            ("DATE", 1, "Timestamp", "date_fraction"),
            ("TIMESTAMP(2)", 1, "Timestamp", "source_temporal_precision"),
            ("TIMESTAMP(9)", 1, "NanoTimestamp", "submicrosecond"),
            ("TIMESTAMP(9)", -1, "NanoTimestamp", "submicrosecond"),
            ("DATE", 2**63 - 1, "Timestamp", "temporal_range"),
        ]:
            with self.assertRaisesRegex(ValueError, error):
                self.decode(kind, value, "io.debezium.time." + name)

    def test_zoned_log_and_native_before_image_compare_as_the_same_exact_instant(self):
        kind = "TIMESTAMP(6) WITH TIME ZONE"
        expected = "2026-10-28T02:30:00.123456000 +00:00"
        self.assertEqual(self.decode(kind, "2026-10-28T10:30:00.123456+08:00",
                                     "io.debezium.time.ZonedTimestamp", "string"), expected)
        self.assertEqual(p.canonical_temporal("2026-10-28T10:30:00.123456000 +08:00", kind),
                         expected)
        self.assertEqual(self.decode(kind, "2026-10-28T02:30:00.123456Z",
                                     "io.debezium.time.ZonedTimestamp", "string"), expected)
        for bad in ("2026-10-28T02:30:00", "2026-02-29T02:30:00Z",
                    "2026-10-28T02:30:00+14:01", "2026-10-28T02:30:00 Europe/Paris",
                    "2026-10-28T02:30:00.123456789Z"):
            with self.assertRaises(ValueError):
                self.decode(kind, bad, "io.debezium.time.ZonedTimestamp", "string")
        with self.assertRaisesRegex(ValueError, "type_contract"):
            self.decode("TIMESTAMP(6) WITH LOCAL TIME ZONE", None,
                        "io.debezium.time.ZonedTimestamp", "string")

    def test_missing_or_duplicate_column_schema_is_not_guessed_from_the_value(self):
        col = {"name": "TS", "type": "DATE", "nullable": "Y"}
        table = {"columns": [col]}
        field = {"field": "TS", "type": "int64", "name": "io.debezium.time.Timestamp"}
        for wire in (None, {}, {"fields": []}, {"fields": [field, copy.deepcopy(field)]}):
            with self.assertRaisesRegex(ValueError, "type_contract"):
                p.decode_row({"TS": 0}, table, wire)
        schema = {"fields": [{"field": "after", "type": "struct", "fields": [field]}]}
        self.assertEqual(p.decode_row({"TS": 0}, table, p.wire_row(schema, "after")),
                         ["1970-01-01T00:00:00"])
        schema["fields"].append(copy.deepcopy(schema["fields"][0]))
        self.assertIsNone(p.wire_row(schema, "after"))


@unittest.skipUnless(os.environ.get("OO_ORACLE_NATIVE_BIN"), "fixed native codec required")
class TemporalBundleTests(unittest.TestCase):
    def test_temporal_snapshot_transactions_and_rollover_use_real_arrow_codec(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            binary = Path(os.environ["OO_ORACLE_NATIVE_BIN"]).resolve()
            kinds = [("D", "DATE", "Timestamp", "int64"),
                     ("MS", "TIMESTAMP(3)", "Timestamp", "int64"),
                     ("US", "TIMESTAMP(6)", "MicroTimestamp", "int64"),
                     ("NS", "TIMESTAMP(9)", "NanoTimestamp", "int64"),
                     ("TZ", "TIMESTAMP(6) WITH TIME ZONE", "ZonedTimestamp", "string")]
            table = {"owner": "UNIT", "table": "TEMPORAL", "keys": ["ID"], "columns": [
                {"name": "ID", "type": "NUMBER", "precision": 9, "scale": 0, "nullable": "N"},
                *[{"name": name, "type": kind, "nullable": "Y"} for name, kind, _, _ in kinds]]}
            fields = [{"field": name, "type": literal, "name": "io.debezium.time." + logical}
                      for name, _, logical, literal in kinds]
            wire = {"type": "struct", "fields": [
                {"field": side, "type": "struct", "fields": fields} for side in ("before", "after")]}
            source = {"connector": "oracle", "db": "FREEPDB1", "schema": "UNIT",
                      "table": "TEMPORAL", "scn": "90"}
            first = dict(ID="1", D=-1000, MS=-1, US=-1, NS=-1000,
                         TZ="2026-10-28T10:30:00.123456+08:00")
            empty = dict(ID="2", **{name: None for name, *_ in kinds})
            db = spool.connect(root / "live.sqlite")
            for row, state in [(first, "first"), (empty, "last")]:
                spool.receive(db, spool.canonical({"schema": wire, "payload": {
                    "op": "r", "source": dict(source, snapshot=state), "before": None, "after": row}}))

            def transaction(tx, operations):
                for order, (op, before, after) in enumerate(operations, 1):
                    spool.receive(db, spool.canonical({"schema": wire, "payload": {
                        "op": op, "before": before, "after": after,
                        "transaction": {"id": tx, "total_order": order},
                        "source": dict(source, snapshot="false", scn="100", commit_scn="150",
                                       txId=tx, rs_id=tx + str(order), ssn=0)}}))
                spool.receive(db, spool.canonical({"status": "END", "id": tx,
                    "event_count": len(operations), "data_collections": [
                        {"data_collection": "FREEPDB1.UNIT.TEMPORAL", "event_count": len(operations)}]}))

            updated = dict(first, D=0, US=1700000000123456,
                           TZ="2026-10-28T02:30:00.123456Z")
            inserted = dict(updated, ID="3", MS=1000)
            transaction("UNIT_TX1", [("u", first, updated), ("c", None, inserted), ("d", empty, None)])
            with sqlite3.connect(root / "cut1.sqlite") as cut:
                db.backup(cut)
            final = dict(updated, US=1700000000123457)
            transaction("UNIT_TX2", [("u", updated, final)])
            db.close()
            shutil.copyfile(root / "live.sqlite", root / "cut2.sqlite")
            for name in ("offset", "history"):
                (root / name).write_bytes(b"UNIT_ONLY not vendor evidence")
            b.atomic(root / "schema.json", b.canonical({"binding": {
                "source_id": "oracle_temporal_UNIT_ONLY", "inventory": [table],
                "identity": "42|7|" + "A" * 32 + "|FREEPDB1|FREE|UNIT_READER",
                "driver_sha256": b.sha(binary)}}))
            config = {"protocol": "oo.oracle.arrow-producer-config/v1", "engine": "oracle",
                "source_id": "oracle_temporal_UNIT_ONLY", "table": "UNIT.TEMPORAL", "mode": "debezium",
                "chunk_rows": 2, "reader": cursor.reference(binary),
                "schema_manifest": cursor.reference(root / "schema.json"),
                "connector_offsets": cursor.reference(root / "offset"),
                "connector_history": cursor.reference(root / "history"),
                "temporal_policy": {"timezone": "utc_instant"}}
            for index in (1, 2):
                cfg = dict(config, spool=cursor.reference(root / f"cut{index}.sqlite"))
                out = root / f"bundle{index}"
                manifest = p.build(cfg, out)
                self.assertEqual(manifest["final_rows"], 2)
                columns = manifest["typed_tables"]["UNIT.TEMPORAL"]["columns"]
                self.assertEqual([c["oid"] for c in columns[1:]], [1114, 1114, 1114, 1114, 1184])
                cursor.build(cfg, out / "manifest.json", b.sha(out / "manifest.json"), root / f"buffer{index}")
            proof = rollover.prove(cursor.reference(root / "buffer1/index.json"),
                                   cursor.reference(root / "buffer2/index.json"))
            self.assertEqual(proof["parent_rows"], 2)
            self.assertEqual(proof["successor_rows"], 2)
            with sqlite3.connect(root / "bundle2/producer-journal.sqlite") as journal:
                rows = [json.loads(row[0]) for row in journal.execute("SELECT row_json FROM rows ORDER BY key")]
            self.assertEqual(rows[0][3], "2023-11-14T22:13:20.123457000")
            self.assertEqual(rows[0][5], "2026-10-28T02:30:00.123456000 +00:00")
