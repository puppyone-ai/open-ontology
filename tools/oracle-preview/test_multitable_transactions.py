"""Synthetic multi-table capture through real offline Arrow; not source acceptance."""
import copy
import contextlib
import json
import os
from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest.mock import patch

import bounded_source as b
import buffer_cursor as c
import c1_debezium as spool
import producer as p
import rollover


class TableScopeTests(unittest.TestCase):
    def test_explicit_table_list_is_closed_nonempty_and_same_owner(self):
        self.assertEqual(p.capture_tables({"tables": ["S.B", "S.A"]}), ["S.A", "S.B"])
        self.assertEqual(p.capture_tables({"table": "S.A"}), ["S.A"])
        for cfg in [{}, {"tables": []}, {"tables": "S.A"}, {"tables": ["S.A", "S.A"]},
                    {"tables": ["S.A", "OTHER.B"]}, {"tables": ["S.*"]},
                    {"table": "S.A", "tables": ["S.A"]},
                    {"tables": ["S.A", None]}, {"tables": ["S.A", {}]}]:
            with self.subTest(config=cfg), self.assertRaises(ValueError):
                p.capture_tables(cfg)

    def test_membership_is_not_truncated_or_limited_by_batch_part_count(self):
        for count in (33, 257, 1025):
            names = [f"S.T{i:04d}" for i in reversed(range(count))]
            with self.subTest(count=count):
                self.assertEqual(p.capture_tables({"tables": names}), sorted(names))
                self.assertEqual(len(p.capture_tables({"tables": names})), count)

    def test_config_file_is_byte_bounded_without_a_table_count_cap(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "config.json"
            raw = json.dumps({"tables": [f"S.T{i}" for i in range(33)]}).encode()
            path.write_bytes(raw)
            with patch.object(p, "MAX_CONFIG_BYTES", len(raw)):
                self.assertEqual(len(p.read_config(path)["tables"]), 33)
                self.assertEqual(p.read_config_bytes(path), raw)
                path.write_bytes(raw + b" ")
                with self.assertRaisesRegex(ValueError, "config_input_size"):
                    p.read_config(path)

@unittest.skipUnless(os.environ.get("OO_ORACLE_NATIVE_BIN"), "fixed native codec required")
class LargeScopeTransactionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.binary = Path(os.environ["OO_ORACLE_NATIVE_BIN"]).resolve()
        self.tables = [{"owner": "UNIT", "table": f"T{i:02d}", "keys": ["ID"], "columns": [
            {"name": "ID", "type": "NUMBER", "precision": 9, "scale": 0, "nullable": "N"},
            {"name": "N", "type": "NUMBER", "precision": 18, "scale": 0, "nullable": "Y"}]}
            for i in range(1, 34)]
        self.names = [p.table_name(table) for table in self.tables]

    def fixture(self, all_tables_changed=False):
        path = self.root / "capture.sqlite"
        with contextlib.closing(spool.connect(path)) as db:
            for number, table in enumerate(self.tables, 1):
                wire = {"type": "struct", "name": "UNIT_ONLY." + table["table"]}
                source = dict(connector="oracle", db="FREEPDB1", schema="UNIT",
                              table=table["table"], scn="90")
                for key in (1, 2):
                    marker = ("first" if number == 1 else "first_in_data_collection") if key == 1 else (
                        "last" if number == 33 else "last_in_data_collection")
                    spool.receive(db, spool.canonical({"schema": wire, "payload": {
                        "op": "r", "before": None, "after": {"ID": str(key), "N": str(number * 100 + key)},
                        "source": dict(source, snapshot=marker)}}))
            ops = ([(i, "u", 1, str(i * 100 + 1), str(-i)) for i in range(1, 34)]
                   if all_tables_changed else [(1, "u", 1, "101", "-101"),
                                                (17, "d", 2, "1702", None),
                                                (33, "c", 3, None, "3303")])
            for ordinal, (number, op, key, before, after) in enumerate(ops, 1):
                table = f"T{number:02d}"
                spool.receive(db, spool.canonical({"schema": {"type": "struct", "name": "UNIT_ONLY." + table},
                    "payload": {"op": op,
                        "before": None if before is None else {"ID": str(key), "N": before},
                        "after": None if after is None else {"ID": str(key), "N": after},
                        "transaction": {"id": "TX33", "total_order": ordinal},
                        "source": dict(connector="oracle", db="FREEPDB1", schema="UNIT", table=table,
                            scn="150", commit_scn="200", snapshot="false", txId="TX33",
                            rs_id=f"UNIT-{ordinal}", ssn=0)}}))
            spool.receive(db, spool.canonical({"status": "END", "id": "TX33", "event_count": len(ops),
                "data_collections": [{"data_collection": f"FREEPDB1.UNIT.T{i:02d}", "event_count": 1}
                                     for i, *_ in ops]}))
        schema = self.root / "schema.json"
        b.atomic(schema, b.canonical({"binding": {"source_id": "oracle_33_UNIT_ONLY",
            "identity": "42|7|" + "A" * 32 + "|FREEPDB1|FREE|UNIT_READER",
            "inventory": self.tables, "driver_sha256": b.sha(self.binary)}}))
        for name in ("offset", "history"):
            (self.root / name).write_bytes(b"UNIT ONLY; not a vendor checkpoint")
        return dict(protocol="oo.oracle.arrow-producer-config/v1", engine="oracle",
            source_id="oracle_33_UNIT_ONLY", tables=self.names, mode="debezium", chunk_rows=2,
            reader=c.reference(self.binary), schema_manifest=c.reference(schema), spool=c.reference(path),
            connector_offsets=c.reference(self.root / "offset"),
            connector_history=c.reference(self.root / "history"))

    def test_33_tables_keep_complete_scope_fill_metadata_and_small_atomic_delta(self):
        config = self.fixture()
        root = self.root / "bundle"
        manifest = p.build(config, root)
        self.assertEqual(sorted(manifest["typed_tables"]), self.names)
        self.assertEqual(manifest["boundary"]["scope"], self.names)
        self.assertEqual(manifest["final_rows"], 66)
        self.assertEqual(len(p.bundle_metadata(root, manifest)["table_schemas"]), 33)
        batches = [json.loads(line)["batch"] for line in (root / "batches.jsonl").read_text().splitlines()]
        self.assertEqual(len([v for v in batches if v["kind"] == "fill"]), 33)
        self.assertTrue(all(len(v["parts"]) == 1 for v in batches[:-1]))
        self.assertEqual([part["table"] for part in batches[-1]["parts"]], ["UNIT.T01", "UNIT.T17", "UNIT.T33"])
        self.assertEqual(manifest["operation_counts"], {"c": 1, "d": 1, "u": 1})
        expected = {f"UNIT.T{i:02d}": [[str(k), str(i * 100 + k)] for k in (1, 2)] for i in range(1, 34)}
        expected["UNIT.T01"][0][1] = "-101"
        expected["UNIT.T17"].pop()
        expected["UNIT.T33"].append(["3", "3303"])
        with contextlib.closing(sqlite3.connect(root / "producer-journal.sqlite")) as db:
            actual = {name: [json.loads(raw) for raw, in db.execute(
                "SELECT row_json FROM rows WHERE table_id=? ORDER BY key", (name,))] for name in self.names}
        self.assertEqual(actual, expected)
        c.build(config, root / "manifest.json", b.sha(root / "manifest.json"), self.root / "buffer")
        c.verify(self.root / "buffer/index.json", b.sha(self.root / "buffer/index.json"))
        parent_spool = self.root / "snapshot-only.sqlite"
        with contextlib.closing(sqlite3.connect(
                f"file:{config['spool']['path']}?mode=ro&immutable=1", uri=True)) as source, \
                contextlib.closing(sqlite3.connect(parent_spool)) as dest:
            source.backup(dest)
            dest.execute("DELETE FROM events")
            dest.execute("DELETE FROM boundaries")
            dest.commit()
        parent_config = dict(config, spool=c.reference(parent_spool))
        parent = self.root / "snapshot-only"
        p.build(parent_config, parent)
        c.build(parent_config, parent / "manifest.json", b.sha(parent / "manifest.json"),
                self.root / "parent-buffer")
        proof = rollover.prove(c.reference(self.root / "parent-buffer/index.json"),
                               c.reference(self.root / "buffer/index.json"))
        self.assertEqual((proof["snapshot_rows"], proof["successor_transactions"]), (66, 1))

    def test_33_parts_remain_one_complete_source_transaction(self):
        config = self.fixture(all_tables_changed=True)
        root = self.root / "wide-transaction"
        manifest = p.build(config, root)
        batches = [json.loads(line)["batch"] for line in (root / "batches.jsonl").read_text().splitlines()]
        transactions = [batch for batch in batches if batch["kind"] == "cdc"]
        self.assertEqual(len(transactions), 1)
        self.assertEqual(len(transactions[0]["parts"]), 33)
        self.assertEqual(sum(part["rows"] for part in transactions[0]["parts"]), 33)
        self.assertEqual(len(manifest["checkpoints"]), 1)
        self.assertEqual(manifest["checkpoints"][0]["event_count"], 33)
        c.build(config, root / "manifest.json", b.sha(root / "manifest.json"), self.root / "buffer")
        c.verify(self.root / "buffer/index.json", b.sha(self.root / "buffer/index.json"))

    def test_33rd_table_missing_or_foreign_cannot_be_ignored(self):
        config = self.fixture()
        for label, sql, error in [
            ("missing", "DELETE FROM snapshots WHERE seq=66", "snapshot_boundary_incomplete"),
            ("foreign", None, "debezium_source_mismatch"),
        ]:
            path = self.root / (label + ".sqlite")
            with contextlib.closing(sqlite3.connect(
                    f"file:{config['spool']['path']}?mode=ro&immutable=1", uri=True)) as source, \
                    contextlib.closing(sqlite3.connect(path)) as dest:
                source.backup(dest)
                if sql:
                    dest.execute(sql)
                else:
                    record = json.loads(dest.execute("SELECT body FROM snapshots WHERE seq=66").fetchone()[0])
                    record["payload"]["source"]["table"] = "UNDECLARED"
                    dest.execute("UPDATE snapshots SET body=? WHERE seq=66", (spool.canonical(record),))
                dest.commit()
            with self.assertRaisesRegex(ValueError, error):
                p.build(dict(config, spool=c.reference(path)), self.root / label)
            self.assertFalse((self.root / label / "manifest.json").exists())


@unittest.skipUnless(os.environ.get("OO_ORACLE_NATIVE_BIN"), "fixed native codec required")
class MultiTableTransactionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.binary = Path(os.environ["OO_ORACLE_NATIVE_BIN"]).resolve()
        columns = [{"name": "ID", "type": "NUMBER", "precision": 9, "scale": 0, "nullable": "N"},
                   {"name": "N", "type": "NUMBER", "precision": 18, "scale": 0, "nullable": "Y"}]
        self.tables = {name: {"owner": "UNIT", "table": name, "keys": ["ID"], "columns": columns}
                       for name in ("A", "B")}
        self.wires = {name: {"type": "struct", "name": f"UNIT_ONLY.{name}"} for name in self.tables}
        self.db = spool.connect(self.root / "live.sqlite")
        self.addCleanup(self.db.close)
        for table, key, value, marker in [("A", "1", "10", "first"),
                                          ("A", "2", "20", "last_in_data_collection"),
                                          ("B", "1", "100", "first_in_data_collection"),
                                          ("B", "2", "200", "last")]:
            spool.receive(self.db, spool.canonical({"schema": self.wires[table], "payload": {
                "op": "r", "before": None, "after": {"ID": key, "N": value},
                "source": self.source(table, snapshot=marker)}}))
        self.transaction("TX1", [("A", "u", ["1", "10"], ["1", "11"]),
                                  ("B", "u", ["1", "100"], ["1", "101"])])
        with contextlib.closing(sqlite3.connect(self.root / "parent.sqlite")) as out:
            self.db.backup(out)
        self.transaction("TX2", [("A", "u", ["1", "11"], ["1", "12"]),
                                  ("B", "d", ["1", "101"], None),
                                  ("A", "u", ["1", "12"], ["1", "13"]),
                                  ("B", "c", None, ["1", "105"])])
        with contextlib.closing(sqlite3.connect(self.root / "child.sqlite")) as out:
            self.db.backup(out)
        schema = self.root / "schema.json"
        b.atomic(schema, b.canonical({"binding": {"source_id": "oracle_multi_UNIT_ONLY",
            "identity": "42|7|" + "A" * 32 + "|FREEPDB1|FREE|UNIT_READER",
            "inventory": list(self.tables.values()), "driver_sha256": b.sha(self.binary)}}))
        for name in ("history", "offset"):
            (self.root / name).write_bytes(b"UNIT ONLY opaque bytes, not vendor checkpoints")
        self.config = {"protocol": "oo.oracle.arrow-producer-config/v1", "engine": "oracle",
            "source_id": "oracle_multi_UNIT_ONLY", "tables": ["UNIT.B", "UNIT.A"], "mode": "debezium",
            "reader": c.reference(self.binary), "schema_manifest": c.reference(schema), "chunk_rows": 1,
            "connector_offsets": c.reference(self.root / "offset"),
            "connector_history": c.reference(self.root / "history")}

    def source(self, table, **extra):
        return dict(connector="oracle", db="FREEPDB1", schema="UNIT", table=table, scn="90", **extra)

    def transaction(self, tx, operations):
        counts = {}
        for order, (table, op, before, after) in enumerate(operations, 1):
            counts[table] = counts.get(table, 0) + 1
            spool.receive(self.db, spool.canonical({"schema": self.wires[table], "payload": {
                "op": op, "before": None if before is None else dict(zip(("ID", "N"), before)),
                "after": None if after is None else dict(zip(("ID", "N"), after)),
                "source": self.source(table, snapshot="false", commit_scn="150", txId=tx,
                                      rs_id=f"{tx}-{order}", ssn=0),
                "transaction": {"id": tx, "total_order": order}}}))
        spool.receive(self.db, spool.canonical({"status": "END", "id": tx, "event_count": len(operations),
            "data_collections": [{"data_collection": f"FREEPDB1.UNIT.{name}", "event_count": count}
                                 for name, count in counts.items()]}))

    def build(self, name, ordered=False):
        config = dict(self.config, spool=c.reference(self.root / f"{name}.sqlite"))
        if ordered:
            config["transaction_key_policy"] = "ordered_final_state/v1"
        out = self.root / (name + "-bundle")
        manifest = p.build(config, out)
        c.build(config, out / "manifest.json", b.sha(out / "manifest.json"), self.root / (name + "-buffer"))
        return out, manifest

    def test_cross_table_events_stay_one_atomic_batch_and_keys_are_table_scoped(self):
        parent, _ = self.build("parent")
        child, manifest = self.build("child", ordered=True)
        frames = [json.loads(line)["batch"] for line in (child / "batches.jsonl").read_text().splitlines()]
        transactions = [batch for batch in frames if batch["kind"] == "cdc"]
        self.assertEqual(len(transactions), 2)
        self.assertEqual(transactions[0]["protocol"], p.TYPED_PROTOCOL)
        self.assertEqual(transactions[1]["protocol"], p.ORDERED_PROTOCOL)
        self.assertEqual([(x["table"], x["rows"]) for x in transactions[0]["parts"]],
                         [("UNIT.A", 1), ("UNIT.B", 1)])
        self.assertEqual([(x["table"], x["rows"]) for x in transactions[1]["parts"]],
                         [("UNIT.A", 2), ("UNIT.B", 2)])
        self.assertEqual([x["sequence"] for x in frames], list(range(1, len(frames) + 1)))
        parts = [part for batch in frames for part in batch["parts"]]
        self.assertEqual(len(parts), len({part["path"] for part in parts}))
        self.assertEqual(manifest["encoded_rows"], 10)
        self.assertEqual(manifest["final_rows"], 4)
        self.assertEqual(manifest["boundary"]["scope"], ["UNIT.A", "UNIT.B"])
        with contextlib.closing(sqlite3.connect(child / "producer-journal.sqlite")) as db:
            actual = [(table, json.loads(row)) for table, row in db.execute(
                "SELECT table_id,row_json FROM rows ORDER BY table_id,key")]
        self.assertEqual(actual, [("UNIT.A", ["1", "13"]), ("UNIT.A", ["2", "20"]),
                                  ("UNIT.B", ["1", "105"]), ("UNIT.B", ["2", "200"])])
        proof = rollover.prove(c.reference(self.root / "parent-buffer/index.json"),
                               c.reference(self.root / "child-buffer/index.json"))
        self.assertEqual((proof["parent_rows"], proof["successor_rows"]), (4, 4))
        self.assertEqual(p.bundle_metadata(child, manifest)["typed_schema_sha256"],
                         p.digest(manifest["typed_tables"]))

    def test_per_table_event_distribution_is_bound_even_when_total_count_matches(self):
        self.build("parent")
        self.build("child", ordered=True)
        index = json.loads((self.root / "child-buffer/index.json").read_text())
        context = json.loads(Path(index["context"]["path"]).read_text())
        payload = json.loads(Path(index["transactions"][-1]["path"]).read_text())
        batch, position = payload["batch"], payload["source_position"]
        c.verify_transition(context, index["context"]["sha256"], position, batch, position["cursor_before"])
        bad = copy.deepcopy(batch)
        bad["parts"][0]["rows"] += 1
        bad["parts"][1]["rows"] -= 1
        forged = dict(position, typed_batch_digest=p.batch_digest(bad))
        with self.assertRaisesRegex(ValueError, "buffer_table_event_count"):
            c.verify_transition(context, index["context"]["sha256"], forged, bad, position["cursor_before"])

    def test_missing_member_extra_table_or_partial_end_cannot_publish(self):
        for label, change, pattern in [
            ("missing", "DELETE FROM snapshots WHERE seq IN (3,4)", "snapshot_boundary|snapshot_table"),
            ("end", "UPDATE boundaries SET expected=5 WHERE tx='TX2'", "transaction_incomplete"),
        ]:
            path = self.root / (label + ".sqlite")
            with contextlib.closing(sqlite3.connect(self.root / "child.sqlite")) as source, \
                    contextlib.closing(sqlite3.connect(path)) as dest:
                source.backup(dest)
                dest.execute(change)
                dest.commit()
            config = dict(self.config, spool=c.reference(path), transaction_key_policy="ordered_final_state/v1")
            with self.assertRaisesRegex(ValueError, pattern):
                p.build(config, self.root / label)
            self.assertFalse((self.root / label / "manifest.json").exists())
        config = dict(self.config, tables=["UNIT.A"], spool=c.reference(self.root / "parent.sqlite"))
        with self.assertRaisesRegex(ValueError, "debezium_source_mismatch"):
            p.build(config, self.root / "foreign")

    def test_single_row_snapshot_uses_vendor_last_marker_without_inventing_first(self):
        path = self.root / "single.sqlite"
        with contextlib.closing(spool.connect(path)) as db:
            spool.receive(db, spool.canonical({"schema": self.wires["A"], "payload": {
                "op": "r", "before": None, "after": {"ID": "1", "N": "10"},
                "source": self.source("A", snapshot="last")}}))
        config = {k: v for k, v in self.config.items() if k != "tables"}
        config.update(table="UNIT.A", spool=c.reference(path))
        manifest = p.build(config, self.root / "single")
        self.assertEqual(manifest["final_rows"], 1)
        self.assertEqual(manifest["checkpoints"], [])

    def test_rows_after_collection_end_or_schema_change_are_rejected(self):
        for label, mutation in (
            ("repeat-first", lambda record: record["payload"]["source"].update(snapshot="first_in_data_collection")),
            ("schema-change", lambda record: record["schema"].update(name="UNIT_CHANGED")),
        ):
            path = self.root / (label + ".sqlite")
            with contextlib.closing(sqlite3.connect(self.root / "parent.sqlite")) as source, \
                    contextlib.closing(sqlite3.connect(path)) as dest:
                source.backup(dest)
                record = json.loads(dest.execute("SELECT body FROM snapshots WHERE seq=4").fetchone()[0])
                mutation(record)
                dest.execute("UPDATE snapshots SET body=? WHERE seq=4", (spool.canonical(record),))
                dest.commit()
            with self.assertRaisesRegex(ValueError, "snapshot_sequence|snapshot_schema_drift"):
                p.build(dict(self.config, spool=c.reference(path)), self.root / label)
