import contextlib
import copy
import hashlib
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest

import oracle_observation as oracle
import oracle_temporal_evidence as evidence


class TemporalEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.columns = [{"name": "NS", "type": "TIMESTAMP(9)", "nullable": "Y"},
                        {"name": "TZ", "type": "TIMESTAMP(6) WITH TIME ZONE", "nullable": "Y"}]
        self.schema = {"tables": [{"owner": "UNIT", "table": "T", "columns": self.columns}]}
        wire = {"type": "struct", "fields": [{"field": "after", "type": "struct", "fields": [
            {"field": "NS", "type": "int64", "name": "io.debezium.time.NanoTimestamp"},
            {"field": "TZ", "type": "string", "name": "io.debezium.time.ZonedTimestamp"}]}]}
        path = self.root / "spool.sqlite"
        with contextlib.closing(sqlite3.connect(path)) as db:
            db.executescript("CREATE TABLE snapshots(seq INTEGER PRIMARY KEY,body TEXT); CREATE TABLE events(body TEXT);")
            db.execute("INSERT INTO snapshots VALUES(1,?)", (json.dumps({
                "schema": wire, "payload": {"source": {"connector": "oracle", "schema": "UNIT", "table": "T"},
                    "before": None, "after": {"NS": -1000, "TZ": "2026-10-28T10:30:00.123456+08:00"}}}),))
            db.commit()
        self.spool = self.pin(path)
        self.producer = {"protocol": "oo.oracle.arrow-producer-config/v1", "mode": "debezium",
            "source_id": "oracle_UNIT", "spool": self.spool, "temporal_policy": {"timezone": "utc_instant"}}
        self.bundle = {"protocol": "oo.oracle.producer-bundle/v1", "source_id": "oracle_UNIT",
            "config_sha256": oracle.digest(self.producer), "boundary": {"spool_sha256": self.spool["sha256"]},
            "source_mappings": {"UNIT.T": [{"source_column": c} for c in self.columns]},
            "typed_tables": {"UNIT.T": {}}}
        self.config, self.publication = self.bind()

    def pin(self, path):
        return {"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                "bytes": path.stat().st_size}

    def write(self, name, value):
        path = self.root / name
        path.write_text(json.dumps(value))
        return self.pin(path)

    def bind(self):
        bundle = self.write("bundle.json", self.bundle)
        context = self.write("context.json", {"protocol": "oo.c1.oracle-buffer-context/v1",
            "source_id": "oracle_UNIT", "bundle": bundle, "raw_spool": self.spool})
        index = self.write("index.json", {"protocol": "oo.oracle.buffer-index/v1", "context": context})
        config = {"source_id": "oracle_UNIT", "temporal_capture": {
            "bundle": bundle, "producer_config": self.write("producer.json", self.producer), "buffer_index": index}}
        publication = {"receipt": {"oracle_buffer": {"bundle_sha256": bundle["sha256"],
                                                      "context_sha256": context["sha256"]}}}
        return config, publication

    def test_exact_capture_proof_enables_only_the_bound_source_precision(self):
        proof = evidence.verify(self.config, self.schema, self.publication)
        self.assertFalse(proof["source_contacted"])
        self.assertFalse(proof["values_published"])
        oracle.check_target(self.columns[0], {"oid": 1114, "typmod": -1}, {},
                            proof["columns"]["UNIT.T.NS"])
        with self.assertRaisesRegex(ValueError, "submicrosecond_source_proof_required"):
            oracle.check_target(self.columns[0], {"oid": 1114, "typmod": -1}, {})
        oracle.check_target(self.columns[1], {"oid": 1184, "typmod": -1}, proof)

    def test_missing_cross_source_and_wrong_publication_are_refused(self):
        with self.assertRaisesRegex(ValueError, "temporal_capture_required"):
            evidence.verify({"source_id": "oracle_UNIT"}, self.schema, self.publication)
        for change in ({"source_id": "foreign"},):
            with self.assertRaisesRegex(ValueError, "temporal_capture_binding"):
                evidence.verify(dict(self.config, **change), self.schema, self.publication)
        bad = copy.deepcopy(self.publication)
        bad["receipt"]["oracle_buffer"]["bundle_sha256"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "temporal_capture_binding"):
            evidence.verify(self.config, self.schema, bad)

    def test_raw_nanos_wrong_units_and_nullability_are_checked(self):
        field = {"type": "int64", "name": "io.debezium.time.NanoTimestamp"}
        for bad in (1, -1, "1000", 1000.0, True):
            with self.assertRaises(ValueError):
                evidence.check_value(self.columns[0], field, bad)
        evidence.check_value(self.columns[0], field, None)
        with self.assertRaisesRegex(ValueError, "temporal_capture_null"):
            evidence.check_value(dict(self.columns[0], nullable="N"), field, None)
        with self.assertRaisesRegex(ValueError, "temporal_capture_wire"):
            evidence.check_value(self.columns[0], dict(field, name="io.debezium.time.MicroTimestamp"), 1000)
        with self.assertRaisesRegex(ValueError, "temporal_capture_precision"):
            evidence.check_value(self.columns[1], {"type": "string", "name": "io.debezium.time.ZonedTimestamp"},
                                 "2026-10-28T10:30:00.123456789+08:00")

    def test_capture_bytes_and_scope_cannot_be_substituted(self):
        with (self.root / "spool.sqlite").open("ab") as stream:
            stream.write(b"changed")
        with self.assertRaisesRegex(ValueError, "temporal_capture_file"):
            evidence.verify(self.config, self.schema, self.publication)
        self.assertIsNone(evidence.verify({}, {"tables": []}, {}))
