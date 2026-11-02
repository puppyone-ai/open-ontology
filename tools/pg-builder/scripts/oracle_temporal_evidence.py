"""Verify exact temporal conversion against the published, immutable Oracle capture."""
import contextlib
import hashlib
import json
from pathlib import Path
import re
import sqlite3

import oracle_observation as oracle


def needs_proof(column):
    kind = column["type"]
    match = re.fullmatch(r"TIMESTAMP\(([0-9])\)( WITH TIME ZONE)?", kind)
    return bool(match and (int(match[1]) > 6 or match[2]))


def check_value(column, field, value):
    kind = column["type"]
    match = re.fullmatch(r"TIMESTAMP\(([0-9])\)( WITH TIME ZONE)?", kind)
    oracle.require(match is not None, "temporal_capture_type")
    precision, zoned = int(match[1]), bool(match[2])
    logical = "ZonedTimestamp" if zoned else "NanoTimestamp" if precision > 6 else "MicroTimestamp"
    oracle.require(field.get("type") == ("string" if zoned else "int64")
                   and field.get("name") == "io.debezium.time." + logical,
                   "temporal_capture_wire")
    if value is None:
        oracle.require(column.get("nullable") == "Y", "temporal_capture_null")
        return
    if zoned:
        oracle.require(isinstance(value, str), "temporal_capture_value")
        parsed = re.fullmatch(
            r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}"
            r"(?:\.([0-9]{1,9}))?(?:Z|[+-][0-9]{2}:[0-9]{2})", value)
        oracle.require(parsed is not None, "temporal_capture_value")
        nanos = int((parsed[1] or "").ljust(9, "0"))
    else:
        oracle.require(type(value) is int and -(2**63) <= value < 2**63, "temporal_capture_value")
        nanos = value % 1_000_000_000
    oracle.require(nanos % 1000 == 0 and nanos % 10 ** (9 - precision) == 0,
                   "temporal_capture_precision")


def verify(config, schema, publication):
    selected = {}
    for table in schema["tables"]:
        columns = [column for column in table["columns"] if needs_proof(column)]
        if columns:
            selected[table["owner"] + "." + table["table"]] = columns
    if not selected:
        return None
    reference = config.get("temporal_capture")
    oracle.require(isinstance(reference, dict) and set(reference) ==
                   {"bundle", "producer_config", "buffer_index"}, "temporal_capture_required")
    bundle, producer, index = (oracle.read_pin(reference[key]) for key in
                               ("bundle", "producer_config", "buffer_index"))
    context = oracle.read_pin(index["context"])
    receipt = publication.get("receipt", publication)
    binding = receipt.get("oracle_buffer", {})
    oracle.require(bundle.get("protocol") == "oo.oracle.producer-bundle/v1"
                   and producer.get("protocol") == "oo.oracle.arrow-producer-config/v1"
                   and index.get("protocol") == "oo.oracle.buffer-index/v1"
                   and context.get("protocol") == "oo.c1.oracle-buffer-context/v1"
                   and bundle.get("source_id") == context.get("source_id") ==
                   producer.get("source_id") == config["source_id"]
                   and producer.get("mode") == "debezium"
                   and bundle.get("config_sha256") == oracle.digest(producer)
                   and binding.get("bundle_sha256") == reference["bundle"]["sha256"]
                   and binding.get("context_sha256") == index["context"]["sha256"]
                   and context["bundle"]["sha256"] == reference["bundle"]["sha256"]
                   and context["raw_spool"]["sha256"] == producer["spool"]["sha256"]
                   == bundle["boundary"]["spool_sha256"], "temporal_capture_binding")
    policy = producer.get("temporal_policy", {})
    if any(c["type"].endswith(" WITH TIME ZONE") for cols in selected.values() for c in cols):
        oracle.require(policy == {"timezone": "utc_instant"}, "timezone_policy_required")
    for table, columns in selected.items():
        mappings = bundle["source_mappings"].get(table, [])
        for column in columns:
            matches = [m for m in mappings if m["source_column"].get("name") == column["name"]]
            oracle.require(len(matches) == 1 and all(
                matches[0]["source_column"].get(key) == column.get(key)
                for key in ("name", "type", "precision", "scale", "nullable")),
                "temporal_capture_column")
    pin = context["raw_spool"]
    path = Path(pin["path"])
    oracle.require(path.is_absolute() and not path.is_symlink() and path.is_file()
                   and path.stat().st_size == pin["bytes"] <= 512 * 1024 * 1024
                   and not any(Path(str(path) + suffix).exists() for suffix in ("-wal", "-shm")),
                   "temporal_capture_file")
    checksum = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            checksum.update(chunk)
    oracle.require(checksum.hexdigest() == pin["sha256"], "temporal_capture_digest")
    observed = set()
    with contextlib.closing(sqlite3.connect(f"file:{path}?mode=ro&immutable=1", uri=True)) as db:
        for query in ("SELECT body FROM snapshots ORDER BY seq", "SELECT body FROM events ORDER BY rowid"):
            for raw, in db.execute(query):
                record = json.loads(raw)
                payload = record.get("payload", record)
                source = payload["source"]
                oracle.require(source.get("connector") == "oracle", "temporal_capture_scope")
                table = source["schema"] + "." + source["table"]
                oracle.require(table in bundle["typed_tables"], "temporal_capture_scope")
                if table not in selected:
                    continue
                for side in ("before", "after"):
                    row = payload.get(side)
                    if row is None:
                        continue
                    structures = [s for s in record["schema"].get("fields", [])
                                  if s.get("field") == side and s.get("type") == "struct"]
                    oracle.require(len(structures) == 1, "temporal_capture_wire")
                    for column in selected[table]:
                        fields = [f for f in structures[0]["fields"] if f.get("field") == column["name"]]
                        oracle.require(len(fields) == 1 and column["name"] in row, "temporal_capture_wire")
                        check_value(column, fields[0], row[column["name"]])
                        observed.add(table + "." + column["name"])
    columns = {table + "." + column["name"]: {"source_type": column["type"], "submicroseconds_zero": True}
               for table, items in selected.items() for column in items}
    oracle.require(observed == set(columns), "temporal_capture_coverage")
    return {"protocol": "oo.oracle.temporal-capture-proof/v1", "source_id": config["source_id"],
            "capture_bundle_sha256": reference["bundle"]["sha256"],
            "context_sha256": index["context"]["sha256"], "raw_spool_sha256": pin["sha256"],
            "columns": columns, "temporal_policy": policy,
            "verifier_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
            "source_contacted": False, "values_published": False}
