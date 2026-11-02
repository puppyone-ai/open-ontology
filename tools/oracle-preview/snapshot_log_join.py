#!/usr/bin/env python3
"""Join one sealed native AS OF snapshot with genuine complete LogMiner suffixes.

No source connection, redo parsing, vendor offset editing or target application.
Original raw transport is retained separately; the selected spool is derived.
"""
import contextlib
import json
from pathlib import Path
import sqlite3

import bounded_source as b
import buffer_cursor as c
import c1_debezium as adapter
import initial_snapshot as source
import producer as p

PROOF = "oo.oracle.initial-source-resume-proof/v1"
MODE = "oracle_scn_snapshot_and_log"


def db_read(path):
    path = Path(path)
    if any(Path(str(path)+suffix).exists() for suffix in ("-wal", "-shm")):
        raise ValueError("source_join_live_spool")
    return contextlib.closing(sqlite3.connect(f"file:{path}?mode=ro&immutable=1", uri=True))


def selected_transactions(db, snapshot_scn, cut_scn):
    source.scn(snapshot_scn)
    source.scn(cut_scn)
    complete = list(adapter.committed(db))
    captured = {row[0] for row in db.execute("SELECT DISTINCT tx FROM events")}
    ended = {row[0] for row in db.execute("SELECT tx FROM boundaries")}
    if captured != ended or captured != {tx["transaction_id"] for tx in complete}:
        raise ValueError("source_join_incomplete_end")
    if db.execute("SELECT COUNT(*) FROM snapshots").fetchone()[0] != 0:
        raise ValueError("source_join_no_data_capture_required")
    for tx in complete:
        end = json.loads(db.execute("SELECT body FROM boundaries WHERE tx=?",
                                   (tx["transaction_id"],)).fetchone()[0])
        if (tx["event_count"] < 1 or end["status"] != "END"
                or end["id"] != tx["transaction_id"] or end["event_count"] != tx["event_count"]):
            raise ValueError("source_join_incomplete_end")
        source.scn(tx["commit_scn"])
    selected = [tx for tx in complete if int(snapshot_scn) < int(tx["commit_scn"]) <= int(cut_scn)]
    if (not selected or str(selected[-1]["commit_scn"]) != str(cut_scn)
            or any(int(a["commit_scn"]) > int(z["commit_scn"]) for a, z in zip(selected, selected[1:]))):
        raise ValueError("source_join_actual_closed_cut_required")
    return selected


def select_spool(original, output, snapshot_scn, cut_scn):
    if output.exists():
        raise ValueError("source_join_output_exists")
    with db_read(original) as source, contextlib.closing(sqlite3.connect(output)) as dest:
        selected = selected_transactions(source, snapshot_scn, cut_scn)
        # Keep raw transports byte-for-byte; only the adapter projection is selected.
        for sql, in source.execute("SELECT sql FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'"):
            dest.execute(sql)
        with dest:
            for name in ("transport", "schemas"):
                for row in source.execute("SELECT * FROM "+name):
                    dest.execute("INSERT INTO "+name+" VALUES ("+",".join("?" for _ in row)+")", row)
            for tx in selected:
                identifier = tx["transaction_id"]
                for row in source.execute("SELECT * FROM events WHERE tx=? ORDER BY ord", (identifier,)):
                    dest.execute("INSERT INTO events VALUES (?,?,?)", row)
                row = source.execute("SELECT tx,expected,body FROM boundaries WHERE tx=?", (identifier,)).fetchone()
                dest.execute("INSERT INTO boundaries VALUES (?,?,?)", row)
    output.chmod(0o600)
    return selected


def validate_config(config):
    proof = json.loads(p.pinned(config["source_resume_proof"]).read_text())
    if (proof.get("protocol") != PROOF or proof["source_id"] != config["source_id"]
            or proof["table"] != config["table"] or proof["snapshot"] != config["snapshot"]
            or config["schema_manifest"] != config["snapshot"]
            or proof["selected_log_spool"] != config["spool"]
            or proof["vendor_checkpoint_after"] != config["connector_offsets"]
            or proof["connector_history"] != config["connector_history"]
            or proof["source_offset_synthesized"] is not False
            or proof["source_snapshot_restarted"] is not False
            or proof["source_snapshot_resumed"] is not True
            or not source.scn(proof["log_start_scn"]) <= source.scn(proof["snapshot_scn"]) < source.scn(proof["cut_scn"])):
        raise ValueError("source_join_proof_binding")
    snapshot_path = p.pinned(proof["snapshot"])
    snapshot = b.load_sealed(snapshot_path.parent, proof["snapshot"]["sha256"])
    if (snapshot["binding"]["scn"] != proof["snapshot_scn"]
            or snapshot["binding"]["source_id"] != config["source_id"]
            or snapshot["binding"]["identity"] != proof["identity"]
            or snapshot["binding"]["driver_sha256"] != proof["reader"]["sha256"]
            or snapshot["binding"]["typed_verify"] is not True
            or len(snapshot["binding"]["inventory"]) != 1
            or p.table_name(snapshot["binding"]["inventory"][0]) != config["table"]):
        raise ValueError("source_join_snapshot_binding")
    p.pinned(proof["reader"])
    for key in ("vendor_checkpoint_before", "vendor_checkpoint_after", "connector_history",
                "partial_snapshot", "snapshot_resume_state", "log_admission", "subscription_resume",
                "log_catalog", "log_coverage"):
        p.pinned(proof[key])
    admission = json.loads(p.pinned(proof["log_admission"]).read_text())
    resumed = json.loads(p.pinned(proof["snapshot_resume_state"]).read_text())
    partial = json.loads(p.pinned(proof["partial_snapshot"]).read_text())
    subscription = json.loads(p.pinned(proof["subscription_resume"]).read_text())
    coverage = json.loads(p.pinned(proof["log_coverage"]).read_text())
    catalog = json.loads(p.pinned(proof["log_catalog"]).read_text())
    if (admission["source_id"] != config["source_id"] or admission["table"] != config["table"]
            or admission["log_start_scn"] != proof["log_start_scn"]
            or admission["snapshot_mode"] != "no_data"
            or admission["in_progress_transaction_mode"] != "all"
            or admission["vendor_offset_synthesized"] is not False
            or admission["vendor_offset"]["sha256"] != proof["vendor_checkpoint_before"]["sha256"]
            or admission["connector_history"]["sha256"] != proof["connector_history"]["sha256"]
            or resumed["source_id"] != config["source_id"] or resumed["rebaseline_required"] is not False
            or resumed["snapshot_scn"] != proof["snapshot_scn"]
            or resumed["status"] != "snapshot_sealed_pending_log_handoff" or resumed["attempt"] < 2
            or resumed["snapshot_manifest"] != proof["snapshot"]
            or partial["snapshot_scn"] != proof["snapshot_scn"] or partial["rows"] < 1
            or partial["source_recapture_from_start"] is not False
            or subscription["source_id"] != config["source_id"] or subscription["table"] != config["table"]
            or subscription["snapshot_mode"] != "no_data" or subscription["transaction_boundary_mode"] != "all"
            or catalog["identity"] != proof["identity"] or catalog["source_id"] != config["source_id"]
            or coverage != source.log_coverage(catalog, proof["log_start_scn"], proof["cut_scn"])):
        raise ValueError("source_join_resume_evidence")
    count = len(partial["parts"])
    if (not 0 < count < len(snapshot["parts"])
            or partial["parts"] != [[part["file"], part["sha256"], part["upper"]]
                                    for part in snapshot["parts"][:count]]
            or partial["rows"] != sum(part["rows"] for part in snapshot["parts"][:count])
            or partial["next_lower_exclusive"] != json.loads(snapshot["parts"][count]["lower"])):
        raise ValueError("source_join_retained_prefix")
    with db_read(p.pinned(proof["original_log_spool"])) as original, db_read(p.pinned(config["spool"])) as selected:
        actual = selected_transactions(original, proof["snapshot_scn"], proof["cut_scn"])
        ids = [tx["transaction_id"] for tx in actual]
        if actual != proof["selected_transactions"]:
            raise ValueError("source_join_transaction_cut")
        if [tx for tx, in selected.execute("SELECT tx FROM boundaries ORDER BY rowid")] != ids:
            raise ValueError("source_join_selected_coverage")
        for tx in ids:
            for query in ("SELECT ord,body FROM events WHERE tx=? ORDER BY ord",
                          "SELECT expected,body FROM boundaries WHERE tx=?"):
                if list(original.execute(query, (tx,))) != list(selected.execute(query, (tx,))):
                    raise ValueError("source_join_raw_event_changed")
        if selected.execute("SELECT COUNT(*) FROM events").fetchone()[0] != sum(tx["event_count"] for tx in actual):
            raise ValueError("source_join_extra_event")
        if list(original.execute("SELECT table_id,digest FROM schemas ORDER BY table_id")) != \
                list(selected.execute("SELECT table_id,digest FROM schemas ORDER BY table_id")):
            raise ValueError("source_join_wire_schema")
    return proof


def prepare(snapshot_ref, original_ref, inputs, out):
    """inputs contains measured source/coordinator refs, never invented checkpoints."""
    out = Path(out).resolve()
    out.mkdir(mode=0o700, exist_ok=False)
    inputs = dict(inputs)
    evidence = out/"evidence"
    evidence.mkdir(mode=0o700)
    for key in ("partial_snapshot", "snapshot_resume_state", "log_admission", "subscription_resume",
                "log_catalog", "log_coverage"):
        original = p.pinned(inputs[key])
        copied = evidence/(key+".json")
        b.atomic(copied, original.read_bytes())
        if b.sha(copied) != inputs[key]["sha256"]:
            raise ValueError("source_join_evidence_changed")
        inputs[key] = c.reference(copied)
    snapshot_path = p.pinned(snapshot_ref)
    snapshot = b.load_sealed(snapshot_path.parent, snapshot_ref["sha256"])
    scn = snapshot["binding"]["scn"]
    original = p.pinned(original_ref)
    selected = select_spool(original, out/"selected-log.sqlite", scn, inputs["cut_scn"])
    proof = {**inputs, "protocol": PROOF, "source_id": snapshot["binding"]["source_id"],
             "identity": snapshot["binding"]["identity"], "snapshot_scn": scn,
             "table": p.table_name(snapshot["binding"]["inventory"][0]),
             "snapshot": snapshot_ref, "original_log_spool": original_ref,
             "selected_log_spool": c.reference(out/"selected-log.sqlite"), "selected_transactions": selected,
             "source_offset_synthesized": False, "source_snapshot_restarted": False,
             "source_snapshot_resumed": True, "source_boundary": "AS OF S plus whole committed transactions in (S,C]",
             "multi_table_consistency": "not_claimed; single registered table"}
    b.atomic(out/"proof.json", b.canonical(proof)+b"\n")
    config = {"protocol": "oo.oracle.arrow-producer-config/v1", "engine": "oracle",
              "source_id": proof["source_id"], "table": proof["table"], "mode": "native_snapshot_and_logminer",
              "chunk_rows": 256, "reader": inputs["reader"], "snapshot": snapshot_ref,
              "schema_manifest": snapshot_ref, "spool": proof["selected_log_spool"],
              "connector_offsets": inputs["vendor_checkpoint_after"], "connector_history": inputs["connector_history"],
              "source_resume_proof": c.reference(out/"proof.json"), "provenance": [c.reference(out/"proof.json")]}
    validate_config(config)
    b.atomic(out/"producer-config.json", b.canonical(config)+b"\n")
    return config
