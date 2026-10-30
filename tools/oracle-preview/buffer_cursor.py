#!/usr/bin/env python3
"""Immutable captured-buffer cursor. Never a Debezium resume offset.

This module validates inputs/transitions; only the real target transaction/ACK
owner is allowed to persist the resulting consumption cursor.
"""
from __future__ import annotations
import argparse
import contextlib
import hashlib
import itertools
import json
import os
from pathlib import Path
import sqlite3
import sys

import bounded_source as b
import producer as p

CONTEXT = "oo.c1.oracle-buffer-context/v1"
POSITION = "oo.c1.oracle-buffer-position/v1"
INDEX = "oo.oracle.buffer-index/v1"
EVENT = "oo.oracle.captured-event-id/v1"
MAX_BYTES = 32*1024*1024
EVENT_LINE_BYTES = 64*1024


def reference(path):
    path = Path(path).resolve()
    return {"path": str(path), "sha256": b.sha(path), "bytes": path.stat().st_size}


def file_ref(value, limit=MAX_BYTES):
    path = p.pinned(value, limit)
    if type(value.get("bytes")) is not int or value["bytes"] != path.stat().st_size or value["bytes"] < 1:
        raise ValueError("buffer_artifact_size")
    return path

def json_lines(reference, line_limit=EVENT_LINE_BYTES):
    """Hash the complete artifact while parsing only one bounded record at a time."""
    path = file_ref(reference, None)
    with path.open("rb") as stream:
        before = os.fstat(stream.fileno())
        hasher, seen = hashlib.sha256(), 0
        while True:
            line = stream.readline(line_limit + 1)
            if not line:
                break
            if len(line) > line_limit or not line.endswith(b"\n"):
                raise ValueError("buffer_record_size")
            seen += len(line)
            if seen > reference["bytes"]:
                raise ValueError("buffer_artifact_size")
            hasher.update(line)
            yield json.loads(line)
        after = os.fstat(stream.fileno())
        current = path.stat()
        if (seen != reference["bytes"] or hasher.hexdigest() != reference["sha256"]
                or (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns)
                != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns)
                or (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns)
                != (current.st_dev, current.st_ino, current.st_size, current.st_mtime_ns)):
            raise ValueError("buffer_artifact_digest")

def batch_frames(root, manifest):
    path = Path(root)/"batches.jsonl"
    expected = {"path": str(path.resolve()), "sha256": manifest["artifacts"]["batches.jsonl"],
                "bytes": path.stat().st_size}
    count = 0
    for count, frame in enumerate(json_lines(expected, MAX_BYTES), 1):
        if frame["kind"] != "typed_batch" or frame["batch"]["sequence"] != count:
            raise ValueError("buffer_frame_coverage")
        yield frame["batch"]
    if count != manifest["batch_count"]:
        raise ValueError("buffer_frame_coverage")

def end_collections(end):
    result = {}
    for item in end["data_collections"]:
        name, count = item["data_collection"], item["event_count"]
        if (not isinstance(name, str) or not name or name in result
                or type(count) is not int or count <= 0):
            raise ValueError("buffer_end_collection_count")
        result[name] = count
    if not result or sum(result.values()) != end["event_count"]:
        raise ValueError("buffer_end_collection_count")
    return result


def cursor(ordinal, transaction_id):
    return {"transaction_ordinal": str(ordinal), "transaction_id": transaction_id}


def ordinal(value):
    raw = value.get("transaction_ordinal")
    if (not isinstance(raw, str) or len(raw) > 20 or not raw.isascii()
            or not raw.isdigit() or str(int(raw)) != raw):
        raise ValueError("buffer_cursor_ordinal")
    n = int(raw)
    if n.bit_length() > 64 or (n == 0) != (value.get("transaction_id") is None):
        raise ValueError("buffer_cursor_identity")
    return n


def identity(value):
    fields = value.split("|")
    if len(fields) != 6:
        raise ValueError("buffer_source_identity")
    dbid, reset, guid, pdb, unique, _reader = fields
    if not dbid.isdigit() or not reset.isdigit() or len(guid) != 32 or any(c not in "0123456789ABCDEF" for c in guid):
        raise ValueError("buffer_source_identity")
    return {"engine": "oracle", "dbid": dbid, "resetlogs_change": reset,
            "pdb_guid": guid, "pdb_name": pdb, "db_unique_name": unique}


def captured_id(event, index):
    source = event["source"]
    transaction = event["transaction"]
    if (source.get("connector") != "oracle" or transaction["total_order"] != index
            or source.get("txId") != transaction["id"] or not isinstance(source.get("rs_id"), str)
            or not source["rs_id"] or type(source.get("ssn")) is not int or source["ssn"] < 0
            or not str(source.get("scn", "")).isdigit() or not str(source.get("commit_scn", "")).isdigit()):
        raise ValueError("captured_event_identity")
    return {"transaction_id": transaction["id"], "total_order": index,
            "db": source["db"], "schema": source["schema"], "table": source["table"],
            "scn": str(source["scn"]), "commit_scn": str(source["commit_scn"]),
            "rs_id": source["rs_id"], "ssn": source["ssn"]}


def build(config, bundle_path, bundle_sha, out):
    bundle_path, out = Path(bundle_path).resolve(), Path(out).resolve()
    manifest = p.validate_bundle(bundle_path.parent, bundle_sha)
    if (manifest["boundary"]["mode"] not in ("debezium_snapshot_and_log", "oracle_scn_snapshot_and_log")
            or manifest["config_sha256"] != p.digest(config)):
        raise ValueError("buffer_bundle_config")
    if manifest["boundary"]["mode"] == "oracle_scn_snapshot_and_log":
        import snapshot_log_join
        snapshot_log_join.validate_config(config)
    spool = p.pinned(config["spool"])
    if any(Path(str(spool)+suffix).exists() for suffix in ("-wal", "-shm")):
        raise ValueError("buffer_live_spool")
    offset = file_ref(reference(p.pinned(config["connector_offsets"])), 1024*1024)
    history = file_ref(reference(p.pinned(config["connector_history"])))
    if (b.sha(spool) != manifest["boundary"]["spool_sha256"]
            or b.sha(offset) != manifest["boundary"]["connector_offsets_sha256"]
            or b.sha(history) != manifest["boundary"]["connector_history_sha256"]):
        raise ValueError("buffer_capture_provenance")
    out.mkdir(parents=True, mode=0o700, exist_ok=False)
    (out/"events").mkdir(mode=0o700)
    context = {"protocol": CONTEXT, "engine": "oracle", "source_id": manifest["source_id"],
        "source_identity": identity(manifest["boundary"]["identity"]),
        "bundle": reference(bundle_path), "raw_spool": reference(spool),
        "typed_schema_sha256": p.digest(manifest["typed_tables"]),
        "snapshot_scn": manifest["boundary"]["snapshot_scn"],
        "initial_cursor": cursor(0, None),
        "source_resume": {"kind": "vendor_offset_retained_separately",
            "final_connector_offset": reference(offset), "connector_history": reference(history),
            "target_ack_updates_vendor_offset": False,
            "intermediate_vendor_resume_offsets": "not_available; replay captured buffer instead"},
        "cursor_semantics": "immutable captured transaction order, not Oracle SCN, GTID, or vendor offset"}
    b.atomic(out/"context.json", b.canonical(context)+b"\n")
    context_ref = reference(out/"context.json")
    entries, previous = [], context["initial_cursor"]
    with contextlib.closing(sqlite3.connect(f"file:{spool}?mode=ro&immutable=1", uri=True)) as db:
        db.execute("PRAGMA cache_size=-4096")
        ends = db.execute("SELECT tx,expected,body FROM boundaries ORDER BY rowid")
        cdc_batches = (batch for batch in batch_frames(bundle_path.parent, manifest) if batch["kind"] == "cdc")
        for n, (end_row, batch) in enumerate(itertools.zip_longest(ends, cdc_batches), 1):
            if end_row is None or batch is None:
                raise ValueError("buffer_transaction_coverage")
            tx, count, end_raw = end_row
            end = json.loads(end_raw)
            if (type(count) is not int or count <= 0 or end["status"] != "END"
                    or end["id"] != tx or end["event_count"] != count
                    or sum(part["rows"] for part in batch["parts"]) != count):
                raise ValueError("buffer_end_count")
            if batch["previous_position"] != str(n-1) or batch["position"] != str(n):
                raise ValueError("buffer_target_order")
            event_path = out/"events"/f"{n:08d}.jsonl"
            event_hash = hashlib.sha256()
            commits, collections, typed_collections, seen = set(), {}, {}, 0
            with event_path.open("xb") as stream:
                for order, raw in db.execute("SELECT ord,body FROM events WHERE tx=? ORDER BY ord", (tx,)):
                    seen += 1
                    if order != seen:
                        raise ValueError("buffer_event_order")
                    event = json.loads(raw)
                    event_id = captured_id(event, seen)
                    if event_id["transaction_id"] != tx:
                        raise ValueError("buffer_tx_identity")
                    commits.add(event_id["commit_scn"])
                    table = ".".join(event_id[k] for k in ("db", "schema", "table"))
                    collections[table] = collections.get(table, 0)+1
                    target_table = ".".join(event_id[k] for k in ("schema", "table"))
                    typed_collections[target_table] = typed_collections.get(target_table, 0) + 1
                    event_hash.update(raw+b"\n")
                    # IDs and payload hashes are auditable without publishing row values.
                    line = b.canonical({"protocol": EVENT, "captured_id": event_id,
                                        "event_body_sha256": hashlib.sha256(raw).hexdigest()})+b"\n"
                    if len(line) > EVENT_LINE_BYTES:
                        raise ValueError("buffer_record_size")
                    stream.write(line)
                stream.flush()
                os.fsync(stream.fileno())
            if seen != count or len(commits) != 1:
                raise ValueError("buffer_transaction_incomplete")
            if collections != end_collections(end):
                raise ValueError("buffer_end_collection_count")
            if (len(batch["parts"]) != len(typed_collections)
                    or {part["table"]: part["rows"] for part in batch["parts"]} != typed_collections):
                raise ValueError("buffer_table_event_count")
            expected = manifest["checkpoints"][n-1]
            if (expected["transaction_id"] != tx or expected["event_count"] != count
                    or expected["commit_scn"] != next(iter(commits))
                    or expected["event_sha256"] != event_hash.hexdigest()):
                raise ValueError("buffer_bundle_transaction_mismatch")
            end_path = out/"events"/f"{n:08d}.end.json"
            b.atomic(end_path, end_raw)
            after = cursor(n, tx)
            position = {"protocol": POSITION, "engine": "oracle", "checkpoint_kind": "buffer_consumption",
                "context_sha256": context_ref["sha256"], "bundle_sha256": bundle_sha,
                "spool_sha256": context["raw_spool"]["sha256"],
                "cursor_before": previous, "cursor_after": after,
                "transaction_id": tx, "commit_scn": next(iter(commits)), "event_count": count,
                "end_event_count": count, "event_order_first": 1, "event_order_last": count,
                "events_sha256": event_hash.hexdigest(), "captured_event_ids": reference(event_path),
                "end_evidence": reference(end_path), "typed_batch_digest": p.batch_digest(batch),
                "target_apply_ordinal": batch["position"], "source_offset_write": False}
            payload = {"protocol": "oo.oracle.buffer-consumer-input/v1", "source_engine": "oracle",
                       "source_id": manifest["source_id"], "buffer_context": context_ref,
                       "batch": batch, "source_position": position}
            path = out/f"transaction-{n:08d}.json"
            b.atomic(path, b.canonical(payload)+b"\n")
            entries.append(reference(path))
            previous = after
    index = {"protocol": INDEX, "context": context_ref, "transactions": entries,
             "transaction_count": len(entries), "final_cursor": previous,
             "target_apply": False, "vendor_offset_modified": False}
    b.atomic(out/"index.json", b.canonical(index)+b"\n")
    return index


def verify_transition(context, context_sha, position, batch, current_cursor, replay_digest=None):
    if (hashlib.sha256(b.canonical(context)+b"\n").hexdigest() != context_sha
            or context["protocol"] != CONTEXT or context["engine"] != "oracle"
            or position["protocol"] != POSITION or position["engine"] != "oracle"
            or position["checkpoint_kind"] != "buffer_consumption" or position["source_offset_write"] is not False
            or context["source_resume"]["target_ack_updates_vendor_offset"] is not False
            or position["context_sha256"] != context_sha
            or position["bundle_sha256"] != context["bundle"]["sha256"]
            or position["spool_sha256"] != context["raw_spool"]["sha256"]):
        raise ValueError("buffer_context_mismatch")
    before, after = ordinal(position["cursor_before"]), ordinal(position["cursor_after"])
    if (after != before+1 or position["cursor_after"]["transaction_id"] != position["transaction_id"]
            or batch["kind"] != "cdc" or batch["previous_position"] != str(before)
            or batch["position"] != str(after) or position["target_apply_ordinal"] != str(after)
            or p.batch_digest(batch) != position["typed_batch_digest"]):
        raise ValueError("buffer_transition_order")
    count = position["event_count"]
    if (type(count) is not int or count <= 0 or position["end_event_count"] != count
            or position["event_order_first"] != 1 or position["event_order_last"] != count
            or sum(x["rows"] for x in batch["parts"]) != count):
        raise ValueError("buffer_end_count")
    if current_cursor != position["cursor_before"] and current_cursor != position["cursor_after"]:
        raise ValueError("buffer_stale_cursor")
    end = json.loads(file_ref(position["end_evidence"], 1024*1024).read_text())
    if end["status"] != "END" or end["id"] != position["transaction_id"] or end["event_count"] != count:
        raise ValueError("buffer_end_mismatch")
    seen, collections, typed_collections = 0, {}, {}
    for event in json_lines(position["captured_event_ids"]):
        seen += 1
        captured = event["captured_id"]
        if (event["protocol"] != EVENT or captured["transaction_id"] != position["transaction_id"]
                or type(captured["total_order"]) is not int or captured["total_order"] != seen
                or captured["commit_scn"] != position["commit_scn"]
                or not isinstance(event.get("event_body_sha256"), str)
                or len(event["event_body_sha256"]) != 64
                or any(c not in "0123456789abcdef" for c in event["event_body_sha256"])):
            raise ValueError("buffer_captured_id_mismatch")
        # total_order is part of the ID: exact contiguous order proves
        # uniqueness without retaining one hash per event in memory.
        table = ".".join(captured[k] for k in ("db", "schema", "table"))
        collections[table] = collections.get(table, 0)+1
        target_table = ".".join(captured[k] for k in ("schema", "table"))
        typed_collections[target_table] = typed_collections.get(target_table, 0) + 1
    if seen != count or collections != end_collections(end):
        raise ValueError("buffer_captured_count")
    if (len(batch["parts"]) != len(typed_collections)
            or {part["table"]: part["rows"] for part in batch["parts"]} != typed_collections):
        raise ValueError("buffer_table_event_count")
    payload_hash = p.digest(position)
    if current_cursor == position["cursor_after"]:
        if replay_digest != payload_hash:
            raise ValueError("buffer_replay_conflict")
        return {"replay": True, "cursor": current_cursor, "source_offset_write": False}
    return {"replay": False, "cursor": position["cursor_after"], "payload_sha256": payload_hash,
            "source_offset_write": False}


def verify(index_path, expected_sha):
    index_path = file_ref({"path": str(Path(index_path).resolve()), "sha256": expected_sha,
                           "bytes": Path(index_path).stat().st_size})
    index = json.loads(index_path.read_text())
    if index["protocol"] != INDEX:
        raise ValueError("buffer_index_protocol")
    context = json.loads(file_ref(index["context"]).read_text())
    # Root/consumer must bind this context digest to the real registered identity/lease.
    bundle_root = file_ref(context["bundle"]).parent
    manifest = p.validate_bundle(bundle_root, context["bundle"]["sha256"])
    if (context["protocol"] != CONTEXT or context["engine"] != "oracle"
            or context["source_id"] != manifest["source_id"]
            or context["source_identity"] != identity(manifest["boundary"]["identity"])
            or context["typed_schema_sha256"] != p.digest(manifest["typed_tables"])
            or context["snapshot_scn"] != manifest["boundary"]["snapshot_scn"]
            or context["raw_spool"]["sha256"] != manifest["boundary"]["spool_sha256"]
            or context["source_resume"]["target_ack_updates_vendor_offset"] is not False):
        raise ValueError("buffer_manifest_context")
    # SQLite is paged and the artifact hash is streamed. Capture's explicit
    # disk quota, not a verifier RAM constant, bounds durable spool allocation.
    spool = file_ref(context["raw_spool"], None)
    file_ref(context["source_resume"]["final_connector_offset"], 1024*1024)
    file_ref(context["source_resume"]["connector_history"])
    if (context["source_resume"]["final_connector_offset"]["sha256"] != manifest["boundary"]["connector_offsets_sha256"]
            or context["source_resume"]["connector_history"]["sha256"] != manifest["boundary"]["connector_history_sha256"]):
        raise ValueError("buffer_manifest_offsets")
    current = context["initial_cursor"]
    base = ordinal(current)
    def cdc_frames():
        for batch in batch_frames(bundle_root, manifest):
            if "rollover" in context and batch["kind"] != "cdc":
                raise ValueError("buffer_frame_coverage")
            if batch["kind"] == "cdc":
                yield batch
    if base != 0 and "rollover" not in context:
        raise ValueError("buffer_frame_coverage")
    with contextlib.closing(sqlite3.connect(f"file:{spool}?mode=ro&immutable=1", uri=True)) as db:
        db.execute("PRAGMA cache_size=-4096")
        end_count = db.execute("SELECT COUNT(*) FROM boundaries").fetchone()[0]
        previous = db.execute("SELECT tx FROM boundaries ORDER BY rowid LIMIT 1 OFFSET ?", (base-1,)).fetchone() if base else None
        missing = db.execute("SELECT tx FROM events EXCEPT SELECT tx FROM boundaries").fetchone()
        empty = db.execute("SELECT tx FROM boundaries EXCEPT SELECT tx FROM events").fetchone()
        if (end_count != ordinal(index["final_cursor"]) or base > end_count
                or (base and (previous is None or current["transaction_id"] != previous[0]))
                or missing or empty or len(index["transactions"]) != end_count-base):
            raise ValueError("buffer_spool_coverage")
        ends = db.execute("SELECT tx FROM boundaries ORDER BY rowid LIMIT -1 OFFSET ?", (base,))
        for entry, batch, end in itertools.zip_longest(index["transactions"], cdc_frames(), ends):
            if entry is None or batch is None or end is None:
                raise ValueError("buffer_frame_coverage")
            payload = json.loads(file_ref(entry).read_text())
            if payload["source_id"] != context["source_id"] or payload["buffer_context"] != index["context"]:
                raise ValueError("buffer_payload_context")
            position = payload["source_position"]
            if payload["batch"] != batch or position["transaction_id"] != end[0]:
                raise ValueError("buffer_frame_binding")
            result = verify_transition(context, index["context"]["sha256"], position, payload["batch"], current)
            # Independently re-derive event/body/END evidence from the immutable raw spool.
            hasher = hashlib.sha256()
            with contextlib.closing(json_lines(position["captured_event_ids"])) as ids:
                count = 0
                for order, raw in db.execute("SELECT ord,body FROM events WHERE tx=? ORDER BY ord", (position["transaction_id"],)):
                    count += 1
                    event_id = next(ids)
                    if (order != count or len(raw) > MAX_BYTES
                            or captured_id(json.loads(raw), order) != event_id["captured_id"]
                            or hashlib.sha256(raw).hexdigest() != event_id["event_body_sha256"]):
                        raise ValueError("buffer_spool_evidence_mismatch")
                    hasher.update(raw+b"\n")
                if next(ids, None) is not None or count != position["event_count"] or hasher.hexdigest() != position["events_sha256"]:
                    raise ValueError("buffer_spool_count_or_digest")
            end = db.execute("SELECT body FROM boundaries WHERE tx=?", (position["transaction_id"],)).fetchone()
            if not end or end[0] != Path(position["end_evidence"]["path"]).read_bytes():
                raise ValueError("buffer_raw_end_mismatch")
            current = result["cursor"]
    if current != index["final_cursor"] or len(index["transactions"]) != index["transaction_count"]:
        raise ValueError("buffer_final_cursor")
    return {"protocol": "oo.oracle.buffer-validation/v1", "transactions": index["transaction_count"],
            "final_cursor": current, "spool_evidence_verified": True,
            "vendor_offset_modified": False, "target_apply": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    build_parser = sub.add_parser("build")
    build_parser.add_argument("--config", type=Path, required=True)
    build_parser.add_argument("--bundle", type=Path, required=True)
    build_parser.add_argument("--bundle-sha256", required=True)
    build_parser.add_argument("--out", type=Path, required=True)
    check = sub.add_parser("verify")
    check.add_argument("--index", type=Path, required=True)
    check.add_argument("--sha256", required=True)
    args = parser.parse_args()
    if args.action == "build":
        result = build(json.loads(args.config.read_text()), args.bundle, args.bundle_sha256, args.out)
        print(json.dumps({"index": str(args.out/"index.json"), "index_sha256": b.sha(args.out/"index.json"),
                          "transactions": result["transaction_count"], "target_apply": False}))
    else:
        print(json.dumps(verify(args.index, args.sha256)))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, OSError, sqlite3.Error, StopIteration) as error:
        print(json.dumps({"error": str(error) if isinstance(error, ValueError) else type(error).__name__,
                          "vendor_offset_modified": False, "target_apply": False}), file=sys.stderr)
        sys.exit(2)
