"""Offline contract fixtures; no simulated ACK is target acceptance."""
import copy
import contextlib
import json
import os
from pathlib import Path
import select
import signal
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import bounded_source as b
import buffer_cursor as c
import c1_debezium as spool
import producer as p
import rollover as r

class SourcePrefixTests(unittest.TestCase):
    def test_streamed_prefix_keeps_complete_late_comparisons_without_file_cap(self):
        with tempfile.TemporaryDirectory() as temp:
            paths = [Path(temp)/name for name in ("parent.sqlite", "child.sqlite")]
            for path, transactions in zip(paths, (3, 4)):
                with contextlib.closing(sqlite3.connect(path)) as db, db:
                    db.executescript("""
                        CREATE TABLE snapshots(seq INTEGER,body BLOB);
                        CREATE TABLE boundaries(tx TEXT,expected INTEGER,body BLOB);
                        CREATE TABLE events(tx TEXT,ord INTEGER,body BLOB);
                        CREATE TABLE schemas(table_id TEXT,digest TEXT);
                    """)
                    db.execute("INSERT INTO snapshots VALUES (1,?)", (b"snapshot",))
                    db.execute("INSERT INTO schemas VALUES ('UNIT.T','schema') ")
                    for tx in range(transactions):
                        db.execute("INSERT INTO boundaries VALUES (?,1,?)", (str(tx), b"END"))
                        db.execute("INSERT INTO events VALUES (?,1,?)", (str(tx), b"event"))
            with patch.object(c, "file_ref", wraps=c.file_ref) as checked:
                proof = r.source_prefix(*(c.reference(path) for path in paths))
                self.assertEqual((proof["parent_transactions"], proof["successor_transactions"],
                                  proof["parent_events"]), (3, 4, 3))
                self.assertEqual([call.args[1] for call in checked.call_args_list], [None, None])
            with contextlib.closing(sqlite3.connect(paths[1])) as db, db:
                db.execute("UPDATE events SET body=? WHERE tx='2'", (b"bad last event",))
            with self.assertRaisesRegex(ValueError, "rollover_event_prefix"):
                r.source_prefix(*(c.reference(path) for path in paths))
            with contextlib.closing(sqlite3.connect(paths[1])) as db, db:
                db.execute("UPDATE events SET body=? WHERE tx='2'", (b"event",))
                db.execute("DELETE FROM boundaries WHERE tx IN ('2','3')")
            with self.assertRaisesRegex(ValueError, "rollover_transaction_prefix"):
                r.source_prefix(*(c.reference(path) for path in paths))

@unittest.skipUnless(os.environ.get("OO_ORACLE_NATIVE_BIN"), "fixed native codec required")
class RolloverTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory()
        cls.root = Path(cls.temp.name).resolve()
        cls.binary = Path(os.environ["OO_ORACLE_NATIVE_BIN"]).resolve()
        cls.source = "oracle_rollover_UNIT_ONLY"
        cls.table = {"owner": "UNIT", "table": "T", "keys": ["ID"], "columns": [
            {"name": "ID", "type": "NUMBER", "precision": 9, "scale": 0, "nullable": "N"},
            {"name": "N", "type": "NUMBER", "precision": 38, "scale": 9, "nullable": "Y"}]}
        cls.identity = "42|7|"+"A"*32+"|FREEPDB1|FREE|UNIT_READER"
        cls.admission = {"protocol": "oo.source-admission.config/v1", "source_id": cls.source}
        schema = cls.root/"schema.json"
        b.atomic(schema, b.canonical({"binding": {"source_id": cls.source, "identity": cls.identity,
            "inventory": [cls.table], "driver_sha256": b.sha(cls.binary)}}))
        history = cls.root/"history"; history.write_bytes(b"UNIT history, not vendor evidence")
        cls.indices = []
        for number, transactions in enumerate((0, 2, 3)):
            directory = cls.root/str(number); directory.mkdir()
            db = spool.connect(directory/"capture.sqlite")
            wire_schema = {"type": "struct", "name": "UNIT-not-source-data"}
            source = {"connector": "oracle", "db": "FREEPDB1", "schema": "UNIT", "table": "T", "scn": "90"}
            for key, state in (("1", "first"), ("2", "last")):
                spool.receive(db, spool.canonical({"schema": wire_schema, "payload": {
                    "op": "r", "source": {**source, "snapshot": state}, "before": None,
                    "after": {"ID": key, "N": "10"}}}))
            for tx, key, before, after in (("UNIT-TX-A", "1", "10", "11"),
                    ("UNIT-TX-B", "2", "10", "11"), ("UNIT-TX-C", "1", "11", "12"))[:transactions]:
                spool.receive(db, spool.canonical({"schema": wire_schema, "payload": {
                    "op": "u", "before": {"ID": key, "N": before}, "after": {"ID": key, "N": after},
                    "transaction": {"id": tx, "total_order": 1},
                    "source": {**source, "snapshot": "false", "scn": "150", "commit_scn": "900",
                               "txId": tx, "rs_id": "UNIT-rs", "ssn": 0}}}))
                spool.receive(db, spool.canonical({"status": "END", "id": tx, "event_count": 1,
                    "data_collections": [{"data_collection": "FREEPDB1.UNIT.T", "event_count": 1}]}))
            db.close()
            offset = directory/"offset"; offset.write_bytes(b"UNIT opaque offset "+str(number).encode())
            config = {"protocol": "oo.oracle.arrow-producer-config/v1", "engine": "oracle",
                "source_id": cls.source, "table": "UNIT.T", "mode": "debezium", "chunk_rows": 2,
                "reader": c.reference(cls.binary), "schema_manifest": c.reference(schema),
                "spool": c.reference(directory/"capture.sqlite"), "connector_offsets": c.reference(offset),
                "connector_history": c.reference(history)}
            p.build(config, directory/"bundle")
            c.build(config, directory/"bundle/manifest.json", b.sha(directory/"bundle/manifest.json"), directory/"buffer")
            cls.indices.append(c.reference(directory/"buffer/index.json"))
        index, context, _ = r.input_state(cls.indices[0])
        instance = {"protocol": "oo.pipeline.native-source-instance/v1", "engine": "oracle",
            "source": cls.source, "source_epoch": 1, "lease_id": "00000000-0000-4000-8000-000000000001",
            "measured_id": "oracle.42.7."+"a"*32, "config_digest": p.digest(cls.admission),
            "schema_digest": context["typed_schema_sha256"]}
        position = {"checkpoint_kind": "buffer_consumption", "context_sha256": index["context"]["sha256"],
            "bundle_sha256": context["bundle"]["sha256"], "spool_sha256": context["raw_spool"]["sha256"],
            "typed_schema_sha256": context["typed_schema_sha256"], "source_identity": context["source_identity"],
            "final_cursor": index["final_cursor"], "source_offset_write": False,
            "connector_offsets_sha256": context["source_resume"]["final_connector_offset"]["sha256"],
            "connector_history_sha256": context["source_resume"]["connector_history"]["sha256"]}
        cls.ack = {"protocol": "oo.oracle.c1-ack-state/v1", "source_engine": "oracle", "source_id": cls.source,
            "source_generation": 1, "target_generation": 1, "source_epoch": 1, "request_digest": "b"*64,
            "acknowledged": True, "task_state": "succeeded", "applied_pos": "0"}
        cls.ack["receipt"] = {**{k: cls.ack[k] for k in ("source_id", "source_generation", "target_generation",
            "source_epoch", "request_digest")}, "source_instance": instance, "oracle_buffer": position,
            "l_end": "oracle-buffer:"+index["context"]["sha256"]+":0"}
        cls.ack_path = cls.root/"unit-parent-ack.json"; b.atomic(cls.ack_path, b.canonical(cls.ack))
        cls.result = r.prepare(*cls.indices[:2], c.reference(cls.ack_path), cls.root/"delta")

    @classmethod
    def tearDownClass(cls):
        cls.temp.cleanup()

    def setUp(self):
        self.temp_case = tempfile.TemporaryDirectory()
        self.case = Path(self.temp_case.name).resolve()
        self.index = r.document(self.result["buffer_index"])
        self.context = r.document(self.index["context"])
        self.manifest = r.document(self.result["bundle"])
        self.previous = r.document(self.result["rollover"])["previous"]
        self.bootstrap = {"protocol": "oo.jobs.oracle-c1-bootstrap/v1", "engine": "oracle",
            "source_id": self.source, "lease_id": "00000000-0000-4000-8000-000000000002",
            "lease_generation": 2, "source_generation": 2, "previous": self.previous, "resume": None}
        self.config = {"protocol": "oo.oracle.producer-argv/v1", "engine": "oracle",
            "python": c.reference(Path(sys.executable).resolve()), "helper": c.reference(Path(p.__file__).resolve()),
            "bounded_source": c.reference(Path(b.__file__).resolve()), "buffer_cursor": c.reference(Path(c.__file__).resolve()),
            "rollover_module": c.reference(Path(r.__file__).resolve()), "admission_config": self.admission,
            **{k: self.result[k] for k in ("bundle", "buffer_index", "rollover")}}
        refuse = self.case/"refusing-materializer"
        refuse.write_text("#!"+sys.executable+"\nimport sys\nsys.exit(2)\n"); refuse.chmod(0o700)
        self.config["materializer"] = c.reference(refuse)

    def tearDown(self):
        self.temp_case.cleanup()

    def phase(self):
        return r.stream_state(self.config, self.bootstrap, self.index, self.context, self.manifest)

    def ack_for(self, number, generation=1):
        payload = r.document(self.index["transactions"][number-1])
        batch, position = payload["batch"], payload["source_position"]
        committed = copy.deepcopy(position)
        for key in ("captured_event_ids", "end_evidence"):
            committed[key].pop("path")
        return {"protocol": "oo.jobs.oracle-c1-baseline-ack/v1", "stage": "typed_batch", "durable": True,
            "receipt": {"protocol": "oo.c1.typed-ack/v3", "source": self.source,
                "task": self.bootstrap["lease_id"], "generation": generation, "epoch": 1,
                "sequence": number, "position": batch["position"], "digest": p.batch_digest(batch),
                "checkpoint_kind": "buffer_consumption", "context_sha256": self.index["context"]["sha256"],
                "bundle_sha256": self.result["bundle"]["sha256"], "buffer_cursor": position["cursor_after"],
                "source_evidence_sha256": p.digest(position), "source_offset_write": False,
                "source_position": {"engine": "oracle", "source_offset_advanced": False,
                    "apply_ordinal": batch["position"], "source_evidence_sha256": p.digest(position),
                    "buffer_position": committed}}}

    def resume(self, number, state="open"):
        ref = None
        if number:
            ack = self.case/"resume-ack.json"; b.atomic(ack, b.canonical(self.ack_for(number)))
            ref = c.reference(ack)
        self.bootstrap["resume"] = {"protocol": r.RESUME, "engine": "oracle", "source_id": self.source,
            "task_id": self.bootstrap["lease_id"], "source_generation": 2, "source_epoch": 1,
            "lease_generation": 1, "state": state, "config_digest": p.digest(self.admission),
            "buffer_index": self.result["buffer_index"], "buffer_context": self.index["context"],
            "bundle_sha256": self.result["bundle"]["sha256"], "last_ack": ref}

    def peer(self, bad_open=False, bad_typed=False):
        config = self.case/"config.json"; b.atomic(config, b.canonical(self.config))
        process = subprocess.Popen([str(self.binary), "c1-baseline-stream-v3", "--config", str(config),
            "--out", str(self.case/"transport")], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, start_new_session=True)
        frames = []
        try:
            process.stdin.write(b.canonical(self.bootstrap)+b"\n"); process.stdin.flush()
            for _ in range(20):
                self.assertTrue(select.select([process.stdout], [], [], 10)[0])
                line = process.stdout.readline()
                if not line:
                    break
                frame = json.loads(line); frames.append(frame)
                if frame["kind"] == "typed_batch":
                    ack = self.ack_for(frame["batch"]["sequence"], self.bootstrap["lease_generation"])
                    if bad_typed: ack["receipt"]["digest"] = "c"*64
                else:
                    stage = {"open": "stage_open", "stream_started": "stream_started", "end": "end"}[frame["kind"]]
                    ack = {"protocol": "oo.jobs.oracle-c1-baseline-ack/v1", "stage": stage, "durable": True}
                    if stage == "stage_open":
                        ack.update(source_epoch=1, rollover_sha256="d"*64 if bad_open else self.result["rollover"]["sha256"],
                                   seeded_from_request_digest=self.previous["request_digest"],
                                   resumed_sequence=self.phase()["sequence"])
                process.stdin.write(b.canonical(ack)+b"\n"); process.stdin.flush()
            _, stderr = process.communicate(timeout=10)
            return process.returncode, frames, stderr
        finally:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGKILL); process.communicate()

    def test_suffix_only_keeps_global_ordinal_and_same_scn_transactions_separate(self):
        value = r.verify(self.result["buffer_index"], self.result["rollover"])
        self.assertEqual(value["transactions"], 2)
        payloads = [r.document(ref) for ref in self.index["transactions"]]
        self.assertEqual([v["batch"]["sequence"] for v in payloads], [1, 2])
        self.assertEqual([v["batch"]["position"] for v in payloads], ["1", "2"])
        self.assertEqual([v["source_position"]["commit_scn"] for v in payloads], ["900", "900"])
        self.assertEqual(self.phase()["sequence"], 0)

    def test_third_generation_keeps_nonzero_parent_cursor(self):
        ack = copy.deepcopy(self.ack)
        ack.update(source_generation=2, target_generation=7, applied_pos="2", request_digest="e"*64)
        receipt = ack["receipt"]
        receipt.update(source_generation=2, target_generation=7, request_digest="e"*64,
                       l_end="oracle-buffer:"+self.index["context"]["sha256"]+":2")
        receipt["source_instance"]["lease_id"] = self.bootstrap["lease_id"]
        position = receipt["oracle_buffer"]
        position.update(context_sha256=self.index["context"]["sha256"],
            bundle_sha256=self.result["bundle"]["sha256"],
            spool_sha256=self.context["raw_spool"]["sha256"],
            final_cursor=self.index["final_cursor"],
            connector_offsets_sha256=self.context["source_resume"]["final_connector_offset"]["sha256"])
        ack_path = self.case/"generation2-ack.json"
        b.atomic(ack_path, b.canonical(ack))
        result = r.prepare(self.result["buffer_index"], self.indices[2],
                           c.reference(ack_path), self.case/"generation3")
        descriptor = r.document(result["rollover"])
        self.assertEqual(descriptor["source_generation"], 3)
        self.assertEqual(descriptor["parent_source_generation"], 2)
        self.assertEqual(descriptor["initial_cursor"], c.cursor(2, "UNIT-TX-B"))
        index = r.document(result["buffer_index"])
        self.assertEqual(len(index["transactions"]), 1)
        frame = r.document(index["transactions"][0])
        self.assertEqual((frame["batch"]["sequence"], frame["batch"]["previous_position"],
                          frame["batch"]["position"]), (1, "2", "3"))
        config = {**self.config, **{k: result[k] for k in ("bundle", "buffer_index", "rollover")}}
        bootstrap = {**self.bootstrap, "source_generation": 3, "previous": descriptor["previous"]}
        self.assertEqual(r.stream_state(config, bootstrap, index, r.document(index["context"]),
                                       r.document(result["bundle"]))["sequence"], 0)
        bootstrap["source_generation"] = 4
        with self.assertRaisesRegex(ValueError, "bootstrap_previous"):
            r.stream_state(config, bootstrap, index, r.document(index["context"]), r.document(result["bundle"]))

    def test_bootstrap_wrong_source_previous_epoch_or_config_refuses(self):
        for key, value in (("source_generation", 2), ("source_epoch", 9), ("request_digest", "f"*64)):
            self.bootstrap["previous"] = {**self.previous, key: value}
            with self.assertRaisesRegex(ValueError, "bootstrap_previous"): self.phase()
        self.bootstrap["previous"] = self.previous
        self.config["admission_config"] = {**self.admission, "source_id": "different"}
        with self.assertRaisesRegex(ValueError, "bootstrap_previous"): self.phase()

    def test_resume_skips_only_acknowledged_whole_transaction(self):
        self.resume(1)
        self.assertEqual(self.phase()["sequence"], 1)
        code, frames, stderr = self.peer()
        self.assertEqual(code, 2, stderr)
        self.assertIn(b"official_materializer_failed", stderr)
        self.assertEqual([f["batch"]["position"] for f in frames if f["kind"] == "typed_batch"], ["2"])
        self.assertFalse(any(f["kind"] == "snapshot_end" for f in frames))

    def test_awaiting_publish_replays_only_open_stream_end(self):
        self.resume(2, "awaiting_publish")
        code, frames, stderr = self.peer()
        self.assertEqual(code, 2, stderr)
        self.assertEqual([f["kind"] for f in frames], ["open", "stream_started", "end"])
        self.assertIn(b"official_materializer_failed", stderr)

    def test_future_lease_and_wrong_child_ack_refuse(self):
        self.resume(1)
        self.bootstrap["resume"]["lease_generation"] = 3
        with self.assertRaisesRegex(ValueError, "resume_binding"): self.phase()
        self.resume(1)
        path = self.case/"resume-ack.json"; ack = json.loads(path.read_text())
        ack["receipt"]["context_sha256"] = "f"*64; b.atomic(path, b.canonical(ack))
        self.bootstrap["resume"]["last_ack"] = c.reference(path)
        with self.assertRaisesRegex(ValueError, "buffer_ack"): self.phase()

    def test_partial_ack_cannot_claim_awaiting_publish(self):
        self.resume(1, "awaiting_publish")
        with self.assertRaisesRegex(ValueError, "unsealed"): self.phase()

    def test_stage_seed_echo_must_match_before_any_row(self):
        code, frames, stderr = self.peer(bad_open=True)
        self.assertEqual(code, 2)
        self.assertEqual([f["kind"] for f in frames], ["open"])
        self.assertIn(b"rollover_stage_open_ack", stderr)

    def test_bad_typed_ack_prevents_next_transaction(self):
        code, frames, stderr = self.peer(bad_typed=True)
        self.assertEqual(code, 2)
        self.assertEqual([f["kind"] for f in frames], ["open", "stream_started", "typed_batch"])
        self.assertIn(b"typed_ack_mismatch", stderr)

    def test_mutated_snapshot_end_event_and_offsets_are_rejected(self):
        _, pc, _ = r.input_state(self.indices[0])
        _, cc, _ = r.input_state(self.indices[1])
        mutated = self.case/"changed.sqlite"
        with contextlib.closing(sqlite3.connect(
                "file:"+pc["raw_spool"]["path"]+"?mode=ro&immutable=1", uri=True)) as source, \
                contextlib.closing(sqlite3.connect(mutated)) as dest:
            source.backup(dest)
        with contextlib.closing(sqlite3.connect(mutated)) as db:
            db.execute("UPDATE snapshots SET body=? WHERE seq=1", (b"{}",))
            db.commit()
        with self.assertRaisesRegex(ValueError, "snapshot_changed"):
            r.source_prefix(c.reference(mutated), cc["raw_spool"])
        doc = r.document(self.result["rollover"])
        doc["source_resume"]["successor_final_offset"] = doc["source_resume"]["parent_final_offset"]
        changed = self.case/"changed-rollover.json"; b.atomic(changed, b.canonical(doc))
        with self.assertRaises(ValueError):
            r.verify(self.result["buffer_index"], c.reference(changed))

    def test_unconfirmed_parent_cannot_authorize_generation_two(self):
        ack = copy.deepcopy(self.ack); ack["task_state"] = "running"
        path = self.case/"parent.json"; b.atomic(path, b.canonical(ack))
        with self.assertRaisesRegex(ValueError, "parent_ack"):
            r.parent_authority(self.indices[0], c.reference(path))

if __name__ == "__main__":
    unittest.main()
