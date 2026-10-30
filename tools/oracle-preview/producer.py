#!/usr/bin/env python3
"""Hash-bound Oracle Arrow producer. No lease fabrication, target SQL or publish."""
from __future__ import annotations
import argparse
import base64
import contextlib
from datetime import datetime, timedelta
from decimal import Decimal, InvalidOperation
import hashlib
import json
import os
from pathlib import Path
import select
import re
import shutil
import sqlite3
import subprocess
import sys
import time

import bounded_source as bounded

TYPED_PROTOCOL = "oo.c1.typed-batch/v3"
ORDERED_PROTOCOL = "oo.c1.typed-ordered-batch/v1"
MAX_CONFIG_BYTES = 16 * 1024 * 1024

def read_config_bytes(path):
    with Path(path).open("rb") as stream:
        raw = stream.read(MAX_CONFIG_BYTES + 1)
    if len(raw) > MAX_CONFIG_BYTES:
        raise ValueError("config_input_size")
    return raw

def read_config(path):
    return json.loads(read_config_bytes(path))

def batch_protocol_supported(batch):
    return (batch.get("protocol") == TYPED_PROTOCOL
            or batch.get("protocol") == ORDERED_PROTOCOL and batch.get("kind") == "cdc")


def digest(value):
    return hashlib.sha256(json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def pinned(reference, max_bytes=None):
    path = Path(reference["path"])
    if not path.is_absolute() or ".." in path.parts or path.is_symlink() or not path.is_file():
        raise ValueError("pinned_input_path")
    if max_bytes is not None and path.stat().st_size > max_bytes:
        raise ValueError("input_size")
    if bounded.sha(path) != reference["sha256"]:
        raise ValueError("pinned_input_digest")
    return path


def canonical_number(value):
    if not isinstance(value, str) or len(value) > 256:
        raise ValueError("typed_decimal_string_required")
    try:
        dec = Decimal(value)
    except InvalidOperation:
        raise ValueError("decimal_syntax") from None
    if not dec.is_finite() or abs(dec.adjusted()) > 130:
        raise ValueError("number_range")
    raw = format(dec, "f")
    raw = raw.rstrip("0").rstrip(".") if "." in raw else raw
    return "0" if dec == 0 else raw


def temporal_kind(kind):
    if kind == "DATE":
        return None, False
    match = re.fullmatch(r"TIMESTAMP\(([0-9])\)( WITH TIME ZONE)?", kind)
    if match is None:
        raise ValueError("debezium_type_contract_required")
    return int(match[1]), bool(match[2])

def temporal_text(civil, nanos, kind):
    precision, zoned = temporal_kind(kind)
    if precision is None and nanos:
        raise ValueError("date_fraction")
    if precision is not None and nanos % 10 ** (9 - precision):
        raise ValueError("source_temporal_precision")
    if nanos % 1000:
        raise ValueError("submicrosecond_target_contract_required")
    text = (f"{civil.year:04d}-{civil.month:02d}-{civil.day:02d}T"
            f"{civil.hour:02d}:{civil.minute:02d}:{civil.second:02d}")
    if precision is not None:
        text += f".{nanos:09d}"
    return text + (" +00:00" if zoned else "")

def canonical_temporal(value, kind):
    """Compare native snapshot and log before images using the same exact value."""
    _, zoned = temporal_kind(kind)
    if not isinstance(value, str):
        raise ValueError("temporal_syntax")
    match = re.fullmatch(
        r"([0-9]{4})-([0-9]{2})-([0-9]{2})T([0-9]{2}):([0-9]{2}):([0-9]{2})"
        r"(?:\.([0-9]{1,9}))?(Z| ?[+-][0-9]{2}:[0-9]{2})?", value)
    if match is None or bool(match[8]) != zoned:
        raise ValueError("temporal_offset" if zoned else "temporal_syntax")
    if kind == "DATE" and match[7] is not None:
        raise ValueError("date_fraction")
    try:
        civil = datetime(*(int(match[i]) for i in range(1, 7)))
        if zoned:
            zone = match[8].strip()
            if zone != "Z":
                hours, minutes = int(zone[1:3]), int(zone[4:6])
                total = hours * 60 + minutes
                if minutes > 59 or total > (720 if zone[0] == "-" else 840):
                    raise ValueError("temporal_offset")
                civil -= timedelta(minutes=total if zone[0] == "+" else -total)
    except (ValueError, OverflowError) as error:
        raise ValueError("temporal_range") from error
    nanos = int((match[7] or "").ljust(9, "0"))
    return temporal_text(civil, nanos, kind)

def wire_row(schema, field):
    fields = schema.get("fields") if isinstance(schema, dict) else None
    if not isinstance(fields, list):
        return None
    found = [item for item in fields if isinstance(item, dict) and item.get("field") == field]
    return found[0] if len(found) == 1 and found[0].get("type") == "struct" else None

def decode_temporal(value, column, wire):
    kind = column["type"]
    precision, zoned = temporal_kind(kind)
    fields = wire.get("fields") if isinstance(wire, dict) else None
    matches = ([field for field in fields if isinstance(field, dict)
                and field.get("field") == column["name"]] if isinstance(fields, list) else [])
    if len(matches) != 1:
        raise ValueError("debezium_type_contract_required")
    field = matches[0]
    if zoned:
        names, literal = {"io.debezium.time.ZonedTimestamp"}, "string"
    elif precision is None or precision <= 3:
        names, literal = {"io.debezium.time.Timestamp", "org.apache.kafka.connect.data.Timestamp"}, "int64"
    else:
        names, literal = ({"io.debezium.time.MicroTimestamp"} if precision <= 6
                          else {"io.debezium.time.NanoTimestamp"}), "int64"
    if field.get("name") not in names or field.get("type") != literal:
        raise ValueError("debezium_temporal_wire_schema")
    if value is None:
        return None
    if zoned:
        return canonical_temporal(value, kind)
    if type(value) is not int or not -(2**63) <= value < 2**63:
        raise ValueError("debezium_temporal_integer")
    scale = 1000 if precision is None or precision <= 3 else 1_000_000 if precision <= 6 else 1_000_000_000
    seconds, fraction = divmod(value, scale)
    try:
        civil = datetime(1970, 1, 1) + timedelta(seconds=seconds)
    except (ValueError, OverflowError) as error:
        raise ValueError("temporal_range") from error
    return temporal_text(civil, fraction * (1_000_000_000 // scale), kind)

def decode_row(row, table, wire=None):
    if not isinstance(row, dict) or set(row) != {c["name"] for c in table["columns"]}:
        raise ValueError("debezium_column_coverage")
    result = []
    for col in table["columns"]:
        value = row[col["name"]]
        temporal = col["type"] == "DATE" or col["type"].startswith("TIMESTAMP")
        if temporal:
            value = decode_temporal(value, col, wire)
        if value is None:
            if col["nullable"] != "Y":
                raise ValueError("null_required_cell")
            result.append(None)
        elif temporal:
            result.append(value)
        elif col["type"] == "NUMBER":
            result.append(canonical_number(value))
        elif col["type"] == "RAW":
            if not isinstance(value, str):
                raise ValueError("raw_base64")
            result.append(base64.b64decode(value, validate=True).hex())
        elif col["type"] in ("VARCHAR2", "NVARCHAR2", "CHAR", "NCHAR"):
            if not isinstance(value, str):
                raise ValueError("text_cell")
            result.append(value)
        else:
            raise ValueError("debezium_type_contract_required")
    return result


def table_name(table):
    return f'{table["owner"]}.{table["table"]}'


def source_matches(source, table):
    if (source.get("connector") != "oracle" or source.get("schema") != table["owner"]
            or source.get("table") != table["table"]):
        raise ValueError("debezium_source_mismatch")

def capture_tables(config):
    if "tables" not in config:
        names = [config.get("table")]
    else:
        names = config["tables"]
        if "table" in config:
            raise ValueError("ambiguous_table_scope")
    if (not isinstance(names, list) or not names
            or any(not isinstance(name, str) or not re.fullmatch(
                r"[A-Z][A-Z0-9_]*\.[A-Z][A-Z0-9_]*", name) for name in names)
            or len(set(names)) != len(names)):
        raise ValueError("single_table_scope" if "tables" not in config else "table_list_scope")
    if len({name.split(".", 1)[0] for name in names}) != 1:
        raise ValueError("table_owner_scope")
    return sorted(names)

def event_table(source, tables):
    if not isinstance(source, dict) or source.get("connector") != "oracle":
        raise ValueError("debezium_source_mismatch")
    name = f'{source.get("schema")}.{source.get("table")}'
    if name not in tables:
        raise ValueError("debezium_source_mismatch")
    return name


def oracle_position(tx, commit_scn, count, events_sha, offsets):
    refs = {}
    for name in ("offset_before", "offset_after"):
        reference = offsets[name]
        path = pinned(reference, 1024*1024)
        if path.stat().st_size == 0 or reference.get("bytes") != path.stat().st_size:
            raise ValueError("opaque_checkpoint_size")
        refs[name] = {k: reference[k] for k in ("path", "sha256", "bytes")}
    if refs["offset_before"]["sha256"] == refs["offset_after"]["sha256"]:
        raise ValueError("opaque_checkpoint_not_advanced")
    return {"protocol": "oo.c1.oracle-logminer-position/v1", "engine": "oracle",
            "commit_scn": commit_scn, "transaction_id": tx, "event_count": count,
            "end_event_count": count, "event_order_first": 1, "event_order_last": count,
            "events_sha256": events_sha, **refs}


class Exporter:
    def __init__(self, config, output):
        if config.get("protocol") != "oo.oracle.arrow-producer-config/v1" or config.get("engine") != "oracle":
            raise ValueError("producer_config")
        if not isinstance(config.get("source_id"), str) or not config["source_id"]:
            raise ValueError("source_identity")
        if ("transaction_key_policy" in config
                and config["transaction_key_policy"] != "ordered_final_state/v1"):
            raise ValueError("transaction_key_policy")
        self.ordered_keys = config.get("transaction_key_policy") == "ordered_final_state/v1"
        self.config, self.root = config, Path(output).resolve()
        self.producer_sha = bounded.sha(Path(__file__).resolve())
        self.dependency_sha = bounded.sha(Path(bounded.__file__).resolve())
        self.binary = pinned(config["reader"])
        for reference in config.get("provenance", []):
            pinned(reference)
        self.chunk_rows = config.get("chunk_rows", 256)
        if type(self.chunk_rows) is not int or not 1 <= self.chunk_rows <= 4096:
            raise ValueError("chunk_rows")
        self.root.mkdir(parents=True, mode=0o700, exist_ok=False)
        self.parts = self.root / "parts"
        self.inputs = self.root / "inputs"
        self.parts.mkdir(mode=0o700)
        self.inputs.mkdir(mode=0o700)
        self.frames = []
        self.checkpoints = []
        self.specs = {}
        self.artifacts = {}
        self.mappings = {}
        self.part_count = 0
        self.encoded_part_count = 0
        self.total_rows = 0
        self.journal = sqlite3.connect(self.root / "producer-journal.sqlite")
        self.journal.executescript("""
            PRAGMA synchronous=FULL;
            CREATE TABLE rows(table_id TEXT, key TEXT, row_json TEXT, PRIMARY KEY(table_id,key));
            CREATE TABLE operations(tx TEXT, ordinal INTEGER, op TEXT, table_id TEXT,
              before_json TEXT, after_json TEXT, PRIMARY KEY(tx,ordinal));
            CREATE TABLE transaction_keys(tx TEXT, key TEXT, PRIMARY KEY(tx,key));
        """)

    def put(self, path, value):
        bounded.atomic(path, bounded.canonical(value) + b"\n")
        self.artifacts[str(path.relative_to(self.root))] = bounded.sha(path)

    def input_part(self, table, rows, position=None, operations=None):
        path = self.inputs / f"{self.part_count:08d}.json"
        self.part_count += 1
        data = {"table": table, "rows": rows, "temporal_policy": self.config.get("temporal_policy", {})}
        if position is not None:
            data["cdc"] = {"positions": [str(position)]*len(rows), "operations": operations}
        self.put(path, data)
        return {"path": str(path), "sha256": self.artifacts[str(path.relative_to(self.root))]}

    def encode(self, table, inputs, position, previous, kind, append=True):
        index = len(self.frames) + 1
        self.encoded_part_count += 1
        path = self.parts / f"{self.encoded_part_count:08d}.arrow"
        request = ({"action": "encode-part", "part": inputs[0]["path"], "sha256": inputs[0]["sha256"]}
                   if len(inputs) == 1 else {"action": "encode-series", "inputs": inputs})
        request["output"] = str(path)
        if bounded.sha(self.binary) != self.config["reader"]["sha256"]:
            raise ValueError("reader_changed")
        proc = subprocess.run([str(self.binary)], input=bounded.canonical(request),
                              capture_output=True, timeout=90)
        if proc.returncode:
            try:
                error = json.loads(proc.stderr)["error"]
            except (ValueError, KeyError):
                error = "encoder_failure"
            raise ValueError(error)
        receipt = json.loads(proc.stdout)
        if (receipt.get("source_engine") != "oracle" or receipt.get("typed_roundtrip") is not True
                or receipt.get("arrow_sha256") != bounded.sha(path)):
            raise ValueError("encoder_receipt")
        name = table_name(table)
        spec = receipt["typed_spec"]
        if name in self.specs and self.specs[name] != spec:
            raise ValueError("target_type_drift")
        self.specs[name] = spec
        self.mappings[name] = receipt["source_mappings"]
        batch = {"protocol": TYPED_PROTOCOL, "sequence": index, "kind": kind,
                 "previous_position": str(previous), "position": str(position),
                 "parts": [{"table": name, "spec": spec, "path": str(path), "sha256": bounded.sha(path),
                            "bytes": path.stat().st_size, "rows": receipt["rows"]}]}
        self.artifacts[str(path.relative_to(self.root))] = bounded.sha(path)
        self.put(self.parts / f"{self.encoded_part_count:08d}.receipt.json", receipt)
        if append:
            self.frames.append({"kind": "typed_batch", "batch": batch})
        self.total_rows += receipt["rows"]
        return batch

    def journal_rows(self, table, rows):
        names = [c["name"] for c in table["columns"]]
        indices = [names.index(k) for k in table["keys"]]
        with self.journal:
            for row in rows:
                if not isinstance(row, list) or len(row) != len(table["columns"]):
                    raise ValueError("snapshot_column_coverage")
                row = [canonical_temporal(value, column["type"])
                       if value is not None and (column["type"] == "DATE"
                           or column["type"].startswith("TIMESTAMP")) else value
                       for column, value in zip(table["columns"], row)]
                key = bounded.canonical([row[i] for i in indices]).decode()
                self.journal.execute("INSERT INTO rows VALUES (?,?,?)",
                                     (table_name(table), key, bounded.canonical(row).decode()))

    def snapshot(self):
        reference = self.config["snapshot"]
        manifest_path = pinned(reference, 16*1024*1024)
        manifest = bounded.load_sealed(manifest_path.parent, reference["sha256"])
        if manifest["binding"]["source_id"] != self.config["source_id"]:
            raise ValueError("snapshot_source_id")
        for entry in manifest["parts"]:
            path = manifest_path.parent / entry["file"]
            part = json.loads(path.read_text())
            self.journal_rows(part["table"], part["rows"])
            inputs = [self.input_part(part["table"], part["rows"])]
            self.encode(part["table"], inputs, 0, 0, "fill")
        return {"kind": "oracle_scn", "snapshot_scn": manifest["binding"]["scn"],
                "identity": manifest["binding"]["identity"], "input_manifest_sha256": reference["sha256"],
                "source_reader_sha256": manifest["binding"]["driver_sha256"],
                "scope_complete": manifest["scope_complete"], "exclusions": manifest["binding"]["exclusions"],
                "native_log_handoff": "not_claimed", "mode": "sealed_snapshot"}

    def spool(self):
        native_snapshot = self.config["mode"] == "native_snapshot_and_logminer"
        resume_proof = None
        if native_snapshot:
            import snapshot_log_join
            resume_proof = snapshot_log_join.validate_config(self.config)
        path = pinned(self.config["spool"])
        if any(Path(str(path)+suffix).exists() for suffix in ("-wal", "-shm")):
            raise ValueError("live_spool_not_allowed")
        schema_path = pinned(self.config["schema_manifest"], 16*1024*1024)
        schema = json.loads(schema_path.read_text())
        if schema["binding"]["source_id"] != self.config["source_id"]:
            raise ValueError("schema_source_id")
        selected = capture_tables(self.config)
        admitted = [t for t in schema["binding"]["inventory"] if table_name(t) in selected]
        tables = {table_name(t): t for t in admitted}
        if sorted(tables) != selected or len(admitted) != len(tables):
            raise ValueError("table_scope")
        if native_snapshot and len(tables) != 1:
            raise ValueError("native_multitable_resume_contract_required")
        pinned(self.config["connector_offsets"])
        pinned(self.config["connector_history"])
        with contextlib.closing(sqlite3.connect(f"file:{path}?mode=ro&immutable=1", uri=True)) as db:
            snapshot_rows = {name: [] for name in tables}
            scns, first_status, last_status, snapshot_count = set(), None, None, 0
            schema_digests = {}
            snapshot_seen, snapshot_finished = set(), set()
            if native_snapshot:
                native_boundary = self.snapshot()
                scns.add(native_boundary["snapshot_scn"])
                if db.execute("SELECT COUNT(*) FROM snapshots").fetchone()[0] != 0:
                    raise ValueError("native_snapshot_must_not_forge_debezium_rows")
                first_event = db.execute("SELECT body FROM events ORDER BY rowid LIMIT 1").fetchone()
                if first_event is None:
                    raise ValueError("native_snapshot_closed_log_cut_required")
                event = json.loads(first_event[0])
                schema_digests[event_table(event["source"], tables)] = digest(event["schema"])
            for (raw,) in (() if native_snapshot else db.execute("SELECT body FROM snapshots ORDER BY seq")):
                record = json.loads(raw)
                p = record.get("payload", record)
                wanted = event_table(p["source"], tables)
                table = tables[wanted]
                if p["op"] != "r":
                    raise ValueError("snapshot_not_snapshot")
                current_schema = digest(record["schema"])
                if wanted in schema_digests and schema_digests[wanted] != current_schema:
                    raise ValueError("snapshot_schema_drift")
                schema_digests[wanted] = current_schema
                scns.add(p["source"]["scn"])
                status = p["source"]["snapshot"]
                if snapshot_count and last_status == "last":
                    raise ValueError("snapshot_sequence")
                if (status not in ("first", "true", "first_in_data_collection", "last_in_data_collection", "last")
                        or wanted in snapshot_finished
                        or status == "first" and snapshot_count
                        or status == "first_in_data_collection" and wanted in snapshot_seen):
                    raise ValueError("snapshot_sequence")
                snapshot_seen.add(wanted)
                if status in ("last_in_data_collection", "last"):
                    snapshot_finished.add(wanted)
                first_status = status if snapshot_count == 0 else first_status
                last_status = status
                snapshot_count += 1
                rows = snapshot_rows[wanted]
                rows.append(decode_row(p["after"], table, wire_row(record["schema"], "after")))
                if len(rows) == self.chunk_rows:
                    self.journal_rows(table, rows)
                    self.encode(table, [self.input_part(table, rows)], 0, 0, "fill")
                    snapshot_rows[wanted] = []
            if not native_snapshot and (len(scns) != 1 or snapshot_count == 0
                    or not (first_status == "first" or snapshot_count == 1 and first_status == "last")
                    or last_status != "last"):
                raise ValueError("snapshot_boundary_incomplete")
            if set(schema_digests) != set(tables):
                raise ValueError("snapshot_table_coverage")
            for wanted, rows in snapshot_rows.items():
                if rows:
                    table = tables[wanted]
                    self.journal_rows(table, rows)
                    self.encode(table, [self.input_part(table, rows)], 0, 0, "fill")
            selected = {row[0] for row in db.execute("SELECT DISTINCT tx FROM events")}
            ends = {row[0] for row in db.execute("SELECT tx FROM boundaries WHERE expected>0")}
            if selected != ends:
                raise ValueError("incomplete_transaction")
            position = 0
            offset_pairs = self.config.get("transaction_offsets")
            if offset_pairs is not None and set(offset_pairs) != ends:
                raise ValueError("transaction_offset_coverage")
            previous_offset = None
            for tx, expected in db.execute("SELECT tx,expected FROM boundaries WHERE expected>0 ORDER BY rowid"):
                if type(expected) is not int or expected <= 0:
                    raise ValueError("transaction_budget")
                inputs, pending_rows, pending_operations, commit_scns = {}, {}, {}, set()
                hasher = hashlib.sha256()
                ordinal = 0
                repeated_key = False
                position += 1
                with self.journal:
                    for order, raw in db.execute("SELECT ord,body FROM events WHERE tx=? ORDER BY ord", (tx,)):
                        ordinal += 1
                        event = json.loads(raw)
                        wanted = event_table(event["source"], tables)
                        table = tables[wanted]
                        if order != ordinal or digest(event["schema"]) != schema_digests[wanted]:
                            raise ValueError("transaction_order_or_schema")
                        if event["transaction"]["id"] != tx:
                            raise ValueError("transaction_identity")
                        commit_scns.add(event["source"]["commit_scn"])
                        before = decode_row(event["before"], table, wire_row(event["schema"], "before")) if event["before"] is not None else None
                        after = decode_row(event["after"], table, wire_row(event["schema"], "after")) if event["after"] is not None else None
                        op = event["op"]
                        if op not in ("c", "u", "d") or (op == "d" and before is None) or (op != "d" and after is None):
                            raise ValueError("operation_shape")
                        row = before if op == "d" else after
                        key_indices = [next(i for i, c in enumerate(table["columns"]) if c["name"] == k) for k in table["keys"]]
                        key = bounded.canonical([row[i] for i in key_indices]).decode()
                        if op == "u" and (before is None
                                or [before[i] for i in key_indices] != [after[i] for i in key_indices]):
                            raise ValueError("primary_key_update_requires_before_after_key_contract")
                        try:
                            inserted = self.journal.execute(
                                "INSERT OR IGNORE INTO transaction_keys VALUES (?,?)" if self.ordered_keys
                                else "INSERT INTO transaction_keys VALUES (?,?)",
                                (tx, bounded.canonical([wanted, key]).decode()))
                            repeated_key = repeated_key or inserted.rowcount == 0
                        except sqlite3.IntegrityError:
                            raise ValueError("repeated_key_transaction_coalescing_required") from None
                        prior = self.journal.execute("SELECT row_json FROM rows WHERE table_id=? AND key=?", (wanted, key)).fetchone()
                        if op in ("u", "d") and (not prior or json.loads(prior[0]) != before):
                            raise ValueError("independent_journal_before_mismatch")
                        if op == "c" and prior:
                            raise ValueError("duplicate_insert")
                        self.journal.execute("DELETE FROM rows WHERE table_id=? AND key=?", (wanted, key))
                        if op != "d":
                            self.journal.execute("INSERT INTO rows VALUES (?,?,?)", (wanted, key, bounded.canonical(after).decode()))
                        self.journal.execute("INSERT INTO operations VALUES (?,?,?,?,?,?)",
                                             (tx, order, op, wanted, bounded.canonical(before).decode(), bounded.canonical(after).decode()))
                        hasher.update(raw + b"\n")
                        rows = pending_rows.setdefault(wanted, [])
                        operations = pending_operations.setdefault(wanted, [])
                        rows.append(row)
                        operations.append("delete" if op == "d" else "upsert")
                        if len(rows) == self.chunk_rows:
                            inputs.setdefault(wanted, []).append(self.input_part(table, rows, position, operations))
                            pending_rows[wanted], pending_operations[wanted] = [], []
                for wanted, rows in pending_rows.items():
                    if rows:
                        inputs.setdefault(wanted, []).append(
                            self.input_part(tables[wanted], rows, position, pending_operations[wanted]))
                if ordinal != expected or len(commit_scns) != 1:
                    raise ValueError("transaction_incomplete")
                parts = []
                for wanted in sorted(inputs):
                    parts.extend(self.encode(tables[wanted], inputs[wanted], position,
                                             position-1, "cdc", append=False)["parts"])
                batch = {"protocol": TYPED_PROTOCOL, "sequence": len(self.frames) + 1, "kind": "cdc",
                         "previous_position": str(position - 1), "position": str(position), "parts": parts}
                self.frames.append({"kind": "typed_batch", "batch": batch})
                if repeated_key:
                    batch["protocol"] = ORDERED_PROTOCOL
                if offset_pairs is not None:
                    source_position = oracle_position(tx, next(iter(commit_scns)), expected, hasher.hexdigest(), offset_pairs[tx])
                    if previous_offset is not None and previous_offset != source_position["offset_before"]["sha256"]:
                        raise ValueError("opaque_checkpoint_chain")
                    previous_offset = source_position["offset_after"]["sha256"]
                    self.frames[-1]["source_position"] = source_position
                self.checkpoints.append({"application_position": str(position), "transaction_id": tx,
                    "commit_scn": next(iter(commit_scns)), "event_count": expected, "event_sha256": hasher.hexdigest(),
                    "recovery": "replay immutable capture at this transaction; opaque Debezium offset only bound at final cut"})
            if previous_offset is not None and previous_offset != self.config["connector_offsets"]["sha256"]:
                raise ValueError("opaque_final_offset_mismatch")
            return {"mode": "oracle_scn_snapshot_and_log" if native_snapshot else "debezium_snapshot_and_log",
                    "snapshot_scn": next(iter(scns)),
                    "identity": schema["binding"]["identity"], "spool_sha256": self.config["spool"]["sha256"],
                    "schema_manifest_sha256": self.config["schema_manifest"]["sha256"],
                    "independent_native_reader_sha256": schema["binding"]["driver_sha256"],
                    "connector_offsets_sha256": self.config["connector_offsets"]["sha256"],
                    "connector_history_sha256": self.config["connector_history"]["sha256"],
                    "position_codec": "oracle-capture-sequence/v1", "position_semantics": "durable capture ordinal, not SCN/GTID",
                    "initial_position": "0", "final_position": str(position),
                    "transaction_checkpoint_pairs": offset_pairs is not None,
                    "offset_before": (self.frames[-len(self.checkpoints)]["source_position"]["offset_before"]
                                      if offset_pairs is not None and self.checkpoints else None),
                    "scope_complete": False, "scope": capture_tables(self.config),
                    **({"snapshot_origin": "native_as_of_scn", "source_resume_proof": self.config["source_resume_proof"],
                        "native_snapshot": self.config["snapshot"], "cut_scn": resume_proof["cut_scn"],
                        "snapshot_restarted": False} if native_snapshot else {})}

    def finish(self, boundary):
        if bounded.sha(Path(__file__).resolve()) != self.producer_sha:
            raise ValueError("producer_code_changed")
        if bounded.sha(Path(bounded.__file__).resolve()) != self.dependency_sha:
            raise ValueError("producer_dependency_changed")
        self.journal.commit()
        final_count = self.journal.execute("SELECT COUNT(*) FROM rows").fetchone()[0]
        op_counts = dict(self.journal.execute("SELECT op,COUNT(*) FROM operations GROUP BY op"))
        self.journal.close()
        self.artifacts["producer-journal.sqlite"] = bounded.sha(self.root / "producer-journal.sqlite")
        frames_path = self.root / "batches.jsonl"
        bounded.atomic(frames_path, b"".join(bounded.canonical(frame)+b"\n" for frame in self.frames))
        self.artifacts["batches.jsonl"] = bounded.sha(frames_path)
        manifest = {"protocol": "oo.oracle.producer-bundle/v1", "engine": "oracle",
                    "source_id": self.config["source_id"], "reader_sha256": self.config["reader"]["sha256"],
                    "encoder_sha256": self.config["reader"]["sha256"],
                    "reader_usage": "offline Arrow encoding; not a new source read",
                    "provenance": self.config.get("provenance", []),
                    "producer_sha256": self.producer_sha, "config_sha256": digest(self.config),
                    "dependency_sha256": {"bounded_source.py": self.dependency_sha},
                    "boundary": boundary, "typed_tables": self.specs, "source_mappings": self.mappings,
                    "checkpoints": self.checkpoints, "batch_count": len(self.frames),
                    "encoded_rows": self.total_rows, "final_rows": final_count, "operation_counts": op_counts,
                    "artifacts": dict(self.artifacts), "target_apply": False, "admitted": False, "published": False,
                    "scope": "immutable producer input for official typed/v3; no target lease or checkpoint invented"}
        self.put(self.root / "manifest.json", manifest)
        return manifest


def build(config, out):
    exporter = Exporter(config, out)
    try:
        boundary = exporter.snapshot() if config["mode"] == "sealed_snapshot" else exporter.spool() if config["mode"] in ("debezium", "native_snapshot_and_logminer") else None
        if boundary is None:
            raise ValueError("producer_mode")
        return exporter.finish(boundary)
    finally:
        exporter.journal.close()


def validate_bundle(root, expected):
    root = Path(root).resolve()
    manifest = json.loads(pinned({"path": str(root/"manifest.json"), "sha256": expected}, 16*1024*1024).read_text())
    if manifest["protocol"] != "oo.oracle.producer-bundle/v1" or manifest["engine"] != "oracle":
        raise ValueError("bundle_protocol")
    for name, sha in manifest["artifacts"].items():
        if Path(name).is_absolute() or ".." in Path(name).parts:
            raise ValueError("bundle_artifact_path")
        pinned({"path": str(root/name), "sha256": sha})
    return manifest


def batch_digest(batch):
    value = json.loads(json.dumps(batch))
    for part in value["parts"]:
        del part["path"]
    # Match serde_json::to_vec(Value), not the ASCII-escaped file serializer.
    return hashlib.sha256(json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()).hexdigest()

def bundle_metadata(root, manifest):
    """Use sealed codec receipts, not a second independently guessed type map."""
    declarations = {}
    for name in sorted(manifest["artifacts"]):
        if not name.endswith(".receipt.json"):
            continue
        receipt = json.loads(pinned({"path": str(Path(root)/name),
                                    "sha256": manifest["artifacts"][name]}).read_text())
        spec = receipt["typed_spec"]
        matching = [table for table, value in manifest["typed_tables"].items()
                    if value == spec and manifest["source_mappings"][table] == receipt["source_mappings"]]
        columns = []
        for index, field in enumerate(receipt["columns"]):
            field = {k: v for k, v in field.items() if k != "logical_name"}
            field["key"] = index in spec["keys"]
            columns.append(field)
        for table in matching:
            if table in declarations and declarations[table] != columns:
                raise ValueError("metadata_schema_drift")
            declarations[table] = columns
    if set(declarations) != set(manifest["typed_tables"]):
        raise ValueError("metadata_receipt_coverage")
    schemas, types, tables, names = [], [], [], set()
    for qualified, columns in sorted(declarations.items()):
        schema, table = qualified.split(".", 1)
        if table in names:
            raise ValueError("metadata_unqualified_table_collision")
        names.add(table)
        schemas.append({"table": table, "columns": columns})
        mappings = manifest["source_mappings"][qualified]
        spec = manifest["typed_tables"][qualified]
        tables.append({"schema": schema, "table": table, "schema_sha256": digest([schemas[-1]]),
                       "columns": [{"ordinal": i+1, "name": m["name"],
                                    "data_type": m["source_column"]["type"]} for i, m in enumerate(mappings)]})
        for i, column in enumerate(columns):
            field = {k: v for k, v in column.items() if k not in ("nullable", "key")}
            types.append({**field, "table": table, "source_type": mappings[i]["source_column"]["type"],
                          "target_oid": spec["columns"][i]["oid"],
                          "target_typmod": spec["columns"][i]["typmod"], "caveat": None})
    return {"protocol": "oo.oracle.producer-metadata/v1", "schema_preimage": schemas,
            "schema_digest": digest(schemas), "type_preimage": types, "type_digest": digest(types),
            "typed_schema_sha256": digest(manifest["typed_tables"]), "table_schemas": tables}


def validate_typed_ack(ack, batch, context, epoch):
    receipt = ack.get("receipt") or {}
    expected = {"protocol": "oo.c1.typed-ack/v3", "source": context["source_id"],
                "task": context["lease_id"], "generation": context["lease_generation"],
                "epoch": epoch, "sequence": batch["sequence"], "position": batch["position"],
                "digest": batch_digest(batch)}
    if ack.get("durable") is not True or any(receipt.get(k) != v for k, v in expected.items()):
        raise ValueError("typed_ack_mismatch")


def oracle_materialized_request(request, source):
    if not isinstance(request, dict) or request.get("source_id") != source:
        raise ValueError("materialized_source")
    def visit(value):
        if isinstance(value, dict):
            if "source_engine" in value and value["source_engine"] != "oracle":
                raise ValueError("materialized_cross_engine")
            codec = value.get("position_codec")
            if isinstance(codec, str) and ("mysql" in codec or "mariadb" in codec):
                raise ValueError("materialized_cross_engine")
            for item in value.values():
                visit(item)
        elif isinstance(value, list):
            for item in value:
                visit(item)
    visit(request)


def validate_buffer_ack(ack, position):
    checkpoint = ack.get("receipt") or {}
    expected = {"protocol": "oo.c1.typed-ack/v3",
                "checkpoint_kind": "buffer_consumption",
                "context_sha256": position["context_sha256"],
                "bundle_sha256": position["bundle_sha256"],
                "buffer_cursor": position["cursor_after"],
                "source_evidence_sha256": digest(position),
                "source_offset_write": False}
    if ack.get("durable") is not True or any(checkpoint.get(k) != value for k, value in expected.items()):
        raise ValueError("buffer_ack_mismatch")
    committed = checkpoint.get("source_position") or {}
    sanitized = json.loads(json.dumps(position))
    for key in ("captured_event_ids", "end_evidence"):
        sanitized[key].pop("path")
    if (committed.get("engine") != "oracle" or committed.get("source_offset_advanced") is not False
            or committed.get("apply_ordinal") != position["target_apply_ordinal"]
            or committed.get("source_evidence_sha256") != digest(position)
            or committed.get("buffer_position") != sanitized):
        raise ValueError("buffer_ack_mismatch")


def serve(config_path, output, timeout):
    raw = read_config_bytes(config_path)
    config = json.loads(raw)
    if config.get("protocol") != "oo.oracle.producer-argv/v1" or config.get("engine") != "oracle":
        raise ValueError("producer_argv_config")
    root = pinned(config["bundle"], 16*1024*1024).parent
    manifest = validate_bundle(root, config["bundle"]["sha256"])
    if manifest["boundary"]["mode"] not in ("debezium_snapshot_and_log", "oracle_scn_snapshot_and_log"):
        raise ValueError("c1_bundle_required")
    if manifest["boundary"]["mode"] == "oracle_scn_snapshot_and_log":
        helpers = config.get("source_resume_helpers", {})
        for name in ("snapshot_log_join.py", "initial_snapshot.py", "c1_debezium.py"):
            if (name not in helpers or pinned(helpers[name])
                    != Path(__file__).resolve().parent/name):
                raise ValueError("source_resume_helper_pin")
        import snapshot_log_join
        proof = json.loads(pinned(manifest["boundary"]["source_resume_proof"]).read_text())
        snapshot_log_join.validate_config({
            "source_id": manifest["source_id"], "table": proof["table"],
            "snapshot": manifest["boundary"]["native_snapshot"],
            "schema_manifest": manifest["boundary"]["native_snapshot"],
            "spool": proof["selected_log_spool"],
            "connector_offsets": proof["vendor_checkpoint_after"],
            "connector_history": proof["connector_history"],
            "source_resume_proof": manifest["boundary"]["source_resume_proof"]})
    materializer = config.get("materializer")
    if not materializer:
        raise ValueError("official_oracle_materializer_required")
    executable = pinned(materializer)
    if not os.access(executable, os.X_OK):
        raise ValueError("materializer_executable")
    buffer_context = buffer_context_ref = None
    buffer_payloads = {}
    if config.get("buffer_index") is not None:
        module = pinned(config["buffer_cursor"])
        if module != Path(__file__).resolve().parent/"buffer_cursor.py":
            raise ValueError("buffer_module_path")
        import buffer_cursor
        index_path = pinned(config["buffer_index"], 16*1024*1024)
        buffer_cursor.verify(index_path, config["buffer_index"]["sha256"])
        index = json.loads(index_path.read_text())
        buffer_context_ref = index["context"]
        buffer_context = json.loads(pinned(buffer_context_ref).read_text())
        if (buffer_context["bundle"]["sha256"] != config["bundle"]["sha256"]
                or buffer_context["source_id"] != manifest["source_id"]):
            raise ValueError("buffer_argv_context")
        for entry in index["transactions"]:
            payload = json.loads(pinned(entry).read_text())
            buffer_payloads[payload["batch"]["position"]] = {**payload, "_input_ref": entry}
    output=Path(output).resolve()
    output.mkdir(parents=True,mode=0o700,exist_ok=False)
    (output/"parts").mkdir(mode=0o700)
    deadline = time.monotonic() + timeout

    def read(stage):
        remaining = deadline - time.monotonic()
        if remaining <= 0 or not select.select([sys.stdin], [], [], remaining)[0]:
            raise ValueError("producer_deadline")
        line = sys.stdin.buffer.readline(1024*1024 + 1)
        if len(line) > 1024*1024 or not line.endswith(b"\n"):
            raise ValueError("worker_frame_size_or_eof")
        value = json.loads(line)
        if stage and (value.get("protocol") != "oo.jobs.oracle-c1-baseline-ack/v1"
                      or value.get("stage") != stage or value.get("durable") is not True):
            raise ValueError("worker_ack")
        return value

    def send(frame):
        if time.monotonic() >= deadline:
            raise ValueError("producer_deadline")
        print(bounded.canonical(frame).decode(), flush=True)

    context = read(None)
    if (context.get("protocol") != "oo.jobs.oracle-c1-bootstrap/v1" or context.get("engine") != "oracle"
            or context.get("source_id") != manifest["source_id"]
            or type(context.get("source_generation")) is not int or context["source_generation"] < 1
            or not context.get("lease_id") or type(context.get("lease_generation")) is not int
            or context["lease_generation"] < 1):
        raise ValueError("oracle_bootstrap")
    phase = {"descriptor": None, "sequence": 0, "source_epoch": None}
    if (config.get("rollover") is not None or context.get("previous") is not None or context.get("resume") is not None
            or (buffer_context is not None and config.get("rollover_module") is not None)):
        if buffer_context is None or config.get("rollover_module") is None:
            raise ValueError("rollover_module_required")
        module = pinned(config["rollover_module"])
        if module != Path(__file__).resolve().parent/"rollover.py":
            raise ValueError("rollover_module_path")
        import rollover
        phase = rollover.stream_state(config, context, index, buffer_context, manifest)
    elif context["source_generation"] != 1 or (buffer_context is not None and "rollover" in buffer_context):
        raise ValueError("rollover_parent_required")
    delta = phase["descriptor"] is not None
    initial_fill = phase.get("initial_fill")
    admission = config["admission_config"]
    if admission.get("protocol") != "oo.source-admission.config/v1" or admission.get("source_id") != manifest["source_id"]:
        raise ValueError("admission_config")
    metadata = bundle_metadata(root, manifest)
    if config.get("metadata") is not None:
        if json.loads(pinned(config["metadata"], 1024*1024).read_text()) != metadata:
            raise ValueError("metadata_pin_mismatch")
    identity = manifest["boundary"]["identity"]
    start = {"protocol": "oo.oracle.debezium-checkpoint/v1", "engine": "oracle",
             "source_id": manifest["source_id"], "identity": identity,
             "capture_bundle_sha256": config["bundle"]["sha256"], "capture_sequence": "0",
             "snapshot_scn": manifest["boundary"]["snapshot_scn"], "connector_offset_at_this_cut": None,
             "recovery": "immutable capture replay; do not resume source from SCN alone"}
    l0 = "oracle-checkpoint:" + digest(start)
    measured_id = "oracle."+hashlib.sha256(identity.encode()).hexdigest()
    source_identity = identity
    if buffer_context is not None:
        source_identity = buffer_context["source_identity"]
        measured_id = "oracle.{}.{}.{}".format(source_identity["dbid"], source_identity["resetlogs_change"],
                                               source_identity["pdb_guid"].lower())
        start = {"protocol": "oo.oracle.buffer-cursor/v1", "engine": "oracle",
                 "checkpoint_kind": "buffer_consumption", "context_sha256": buffer_context_ref["sha256"],
                 "bundle_sha256": config["bundle"]["sha256"], "cursor": buffer_context["initial_cursor"],
                 "source_offset_write": False}
        l0 = "oracle-buffer:"+buffer_context_ref["sha256"]+":"+buffer_context["initial_cursor"]["transaction_ordinal"]
    l0_pos = int(buffer_context["initial_cursor"]["transaction_ordinal"]) if buffer_context else 0
    send({"kind": "open", "source_engine": "oracle", "source_identity": source_identity,
          "measured_id": measured_id,
          "connection_config_sha256": hashlib.sha256(raw).hexdigest(),
          "config_digest": digest(admission), "admission_config": admission,
          "schema_preimage": metadata["schema_preimage"], "schema_digest": metadata["schema_digest"],
          "type_digest": metadata["type_digest"],
          "tables": sorted(manifest["typed_tables"]), "table_schemas": metadata["table_schemas"],
          "typed_tables": manifest["typed_tables"], "l0": l0, "l0_pos": l0_pos,
          "position_codec": "oracle-capture-sequence/v1", "checkpoint": start,
          **({"checkpoint_kind": "buffer_consumption", "buffer_context": buffer_context_ref,
              "buffer_index": config["buffer_index"]}
             if buffer_context is not None else {}),
          **({"stream_mode": "delta", "rollover": config["rollover"],
              "parent_l_end": phase["descriptor"]["previous"]["l_end"],
              "resume_sequence": phase["sequence"]} if delta else {}),
          **({"stream_mode": "initial_fill", "initial_fill": initial_fill,
              "resume_sequence": phase["sequence"]} if initial_fill is not None else {})})
    opened = read("stage_open")
    epoch = opened.get("source_epoch")
    if type(epoch) is not int or epoch < 1:
        raise ValueError("source_epoch")
    if delta and (epoch != phase["source_epoch"]
                  or opened.get("rollover_sha256") != config["rollover"]["sha256"]
                  or opened.get("seeded_from_request_digest") != phase["descriptor"]["previous"]["request_digest"]
                  or opened.get("resumed_sequence") != phase["sequence"]):
        raise ValueError("rollover_stage_open_ack")
    if initial_fill is not None and (
            (phase["source_epoch"] is not None and epoch != phase["source_epoch"])
            or opened.get("resumed_sequence") != phase["sequence"]
            or opened.get("buffer_context_sha256") != buffer_context_ref["sha256"]
            or opened.get("bundle_sha256") != config["bundle"]["sha256"]
            or opened.get("completed_tables") != phase["completed_tables"]):
        raise ValueError("initial_fill_stage_open_ack")
    send({"kind": "stream_started", "l0": l0, "source_engine": "oracle",
          "position_lineage": measured_id})
    read("stream_started")
    completed = set(phase.get("completed_tables", []))
    with (root/"batches.jsonl").open() as stream:
        for line in stream:
            frame = json.loads(line)
            batch = frame["batch"]
            if (delta or initial_fill is not None) and batch["sequence"] <= phase["sequence"]:
                continue
            if delta and batch["kind"] != "cdc":
                raise ValueError("rollover_delta_only")
            if buffer_context is not None and batch["kind"] == "cdc":
                payload = buffer_payloads.get(batch["position"])
                if not payload or batch_digest(batch) != payload["source_position"]["typed_batch_digest"]:
                    raise ValueError("buffer_batch_binding")
                frame["source_position"] = payload["source_position"]
                frame["buffer_context"] = buffer_context_ref
                frame["buffer_transaction"] = payload["_input_ref"]
            for part in batch["parts"]:
                p = Path(part["path"])
                if not p.is_relative_to(root):
                    raise ValueError("part_outside_bundle")
                pinned({"path": str(p), "sha256": part["sha256"]}, 32*1024*1024)
                destination=output/"parts"/p.name
                if destination.exists():
                    raise ValueError("transport_part_conflict")
                shutil.copyfile(p,destination)
                with destination.open("rb") as copied:
                    os.fsync(copied.fileno())
                pinned({"path":str(destination),"sha256":part["sha256"]},32*1024*1024)
                part["path"]=str(destination)
            if not delta and batch["kind"] == "cdc":
                for table in sorted(manifest["typed_tables"]):
                    if table in completed:
                        continue
                    send({"kind": "snapshot_end", "table": table})
                    read("snapshot_end")
                    completed.add(table)
            send(frame)
            typed_ack = read("typed_batch")
            validate_typed_ack(typed_ack, batch, context, epoch)
            if buffer_context is not None and batch["kind"] == "cdc":
                validate_buffer_ack(typed_ack, frame["source_position"])
    if not delta:
        for table in sorted(manifest["typed_tables"]):
            if table in completed:
                continue
            send({"kind": "snapshot_end", "table": table})
            read("snapshot_end")
    end = {**start, "capture_sequence": manifest["boundary"]["final_position"],
           "transactions": manifest["checkpoints"],
           "connector_offset_at_this_cut": manifest["boundary"]["connector_offsets_sha256"]}
    l_end = "oracle-checkpoint:" + digest(end)
    if buffer_context is not None:
        final_position = manifest["boundary"]["final_position"]
        after = index["final_cursor"]
        end = {**start, "cursor": after, "source_offset_write": False}
        l_end = "oracle-buffer:"+buffer_context_ref["sha256"]+":"+final_position
    send({"kind": "end", "l_end": l_end, "l_end_pos": int(manifest["boundary"]["final_position"]),
          "checkpoint": end, "capture_bundle_sha256": config["bundle"]["sha256"]})
    sealed = read("end")
    if bounded.sha(executable) != materializer["sha256"]:
        raise ValueError("materializer_changed")
    materialize_input = {"protocol": "oo.oracle.materialize-input/v1", "engine": "oracle",
                         "context": context, "source_epoch": epoch, "bundle": config["bundle"],
                         "transport_root":str(output),
                         "sealed_target": sealed, "l0": l0, "l_end": l_end, "checkpoint": end,
                         **({"rollover": config["rollover"]} if delta else {})}
    remaining = min(120, deadline-time.monotonic())
    if remaining <= 0:
        raise ValueError("producer_deadline")
    proc = subprocess.run([str(executable), *materializer.get("args", [])], input=bounded.canonical(materialize_input),
                          capture_output=True, timeout=remaining)
    bounded.atomic(output/"materialize-input.json", bounded.canonical(materialize_input)+b"\n")
    bounded.atomic(output/"materializer.stdout", proc.stdout)
    bounded.atomic(output/"materializer.stderr", proc.stderr)
    bounded.atomic(output/"materializer.exit", str(proc.returncode).encode()+b"\n")
    if proc.returncode:
        raise ValueError("official_materializer_failed")
    result = json.loads(proc.stdout)
    if (result.get("protocol") != "oo.oracle.materialize-result/v1" or result.get("engine") != "oracle"
            or result.get("source_id") != manifest["source_id"] or not isinstance(result.get("request"), dict)
            or "error" in result):
        raise ValueError("official_materializer_receipt")
    oracle_materialized_request(result["request"], manifest["source_id"])
    request = result["request"]
    # A successful callback alone is not admission. Retain and verify the real
    # sealer's schema/type bindings before returning a materialized frame.
    if (request.get("protocol") != "oo.pipeline.native-c1-baseline/v1"
            or request.get("source_engine") != "oracle" or request.get("source_epoch") != epoch
            or request.get("source_generation") != context["source_generation"]
            or not request.get("entries") or not request.get("native_requests")):
        raise ValueError("materialized_admission_required")
    envelope = json.loads(pinned({"path": request["capture_envelope_path"],
                                  "sha256": request["capture_envelope_sha256"]}).read_text())
    if envelope.get("entries") != request["entries"]:
        raise ValueError("materialized_envelope_mismatch")
    native = {(r["logical_schema"], r["logical_table"]): r for r in request["native_requests"]}
    if len(native) != len(manifest["typed_tables"]) or len(request["entries"]) != len(native):
        raise ValueError("materialized_table_coverage")
    for entry in request["entries"]:
        registry = json.loads(pinned({"path": entry["admission_registry_path"],
                                      "sha256": entry["admission_registry_sha256"]}).read_text())
        r = native[(entry["schema"], entry["table"])]
        expected_schema = next(t["schema_sha256"] for t in metadata["table_schemas"]
                               if (t["schema"], t["table"]) == (entry["schema"], entry["table"]))
        if (r["schema_digest"] != expected_schema or r["type_digest"] != metadata["type_digest"]
                or r["config_digest"] != digest(admission)
                or not any(binding["schema_digest"] == r["schema_digest"]
                           and binding["type_digest"] == r["type_digest"]
                           and binding["admission_receipt_path"] == r["admission_path"]
                           for binding in registry["bindings"])):
            raise ValueError("materialized_metadata_mismatch")
    send({"kind": "materialized", "request": result["request"]})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    export = sub.add_parser("bundle")
    export.add_argument("--config", type=Path, required=True)
    export.add_argument("--out", type=Path, required=True)
    validate = sub.add_parser("validate")
    validate.add_argument("--bundle", type=Path, required=True)
    validate.add_argument("--sha256", required=True)
    run = sub.add_parser("serve")
    run.add_argument("--config", type=Path, required=True)
    run.add_argument("--out", type=Path, required=True)
    run.add_argument("--deadline-seconds", type=int, default=300)
    args = parser.parse_args()
    if args.action == "serve":
        if not 1 <= args.deadline_seconds <= 3600:
            raise ValueError("producer_deadline")
        serve(args.config, args.out, args.deadline_seconds)
        return
    if args.action == "bundle":
        manifest = build(read_config(args.config), args.out)
        result = {"bundle": str(args.out.resolve()), "manifest_sha256": bounded.sha(args.out/"manifest.json"),
                  "batches": manifest["batch_count"], "final_rows": manifest["final_rows"],
                  "operation_counts": manifest["operation_counts"], "target_apply": False}
    else:
        manifest = validate_bundle(args.bundle, args.sha256)
        result = {"validated": True, "batch_count": manifest["batch_count"], "target_apply": False}
    print(json.dumps(result))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, OSError, sqlite3.Error, subprocess.TimeoutExpired) as error:
        print(json.dumps({"error": str(error) if isinstance(error, ValueError) else type(error).__name__,
                          "target_apply": False, "ack": False}), file=sys.stderr)
        sys.exit(2)
