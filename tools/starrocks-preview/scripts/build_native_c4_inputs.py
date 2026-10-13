#!/usr/bin/env python3
"""Bind the two live StarRocks partition reads to source-owned C4 witness inputs.

The native probe used PARTITION SQL. This script refuses a changed partition
set, unstable metadata, mismatched SQL/Arrow cells, or an unapproved reader
statement. It does not set target ACK state or call the Loader.
"""
import hashlib
import importlib.util
import json
import argparse
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
BASE = ROOT / "evidence/2c023948-0862-4dae-ae8f-6fc30241db62/DL01/starrocks-product/raw"
OBS = BASE / "native-c4-observation-20261013"
OUT = OBS / "sealed-inputs"
SOURCE = "sr_w45_native"
SCHEMA = "oo_w45_sr"
DRIVER = Path("/tmp/oo-w45-c4-cargo-target/debug/starrocks-probe")

spec = importlib.util.spec_from_file_location("seal", Path(__file__).with_name("seal_capture_witness.py"))
seal = importlib.util.module_from_spec(spec)
spec.loader.exec_module(seal)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def put(path, value):
    path.write_text(json.dumps(value, sort_keys=True, indent=2, ensure_ascii=False) + "\n")


def main():
    global OBS, OUT
    parser = argparse.ArgumentParser()
    parser.add_argument("--observation", choices=("native-c4-observation-20261013", "native-c4-current-20261013"), default="native-c4-observation-20261013")
    parser.add_argument("--first-generation", type=int, choices=(3, 4), default=3)
    args = parser.parse_args()
    OBS = BASE / args.observation
    OUT = OBS / "sealed-inputs"
    if OUT.exists():
        raise SystemExit("immutable C4 inputs already exist")
    assert DRIVER.is_file()
    metadata = json.loads((OBS / "actual-metadata.json").read_text())
    original = (OBS / "partitions-before.stdout").read_bytes()
    assert original == (OBS / "partitions-after.stdout").read_bytes()
    assert original == (OBS / "partitions-post-arrow.stdout").read_bytes()
    assert len(metadata["partitions"]) == 2
    native_ids = metadata["observed_partition_ids"]
    assert native_ids == sorted(native_ids) and len(set(native_ids)) == 2
    ids = sorted(part["name"] for part in metadata["partitions"])
    OUT.mkdir()
    for generation, part in enumerate(metadata["partitions"], start=args.first_generation):
        name = part["name"]
        root = OUT / name
        root.mkdir()
        read = OBS / name
        decoded = json.loads((OBS / (name + "-decoded") / "dup_events.json").read_text())
        rows = decoded["rows"]
        assert len(rows) == part["row_count"]
        lower, upper = part["partition_range"]["lower"], part["partition_range"]["upper"]
        assert all(lower <= row["bucket"] < upper for row in rows)
        actual = (OBS / (name + ".sql.stdout")).read_text().splitlines()
        cells = ["\t".join("NULL" if row[field["name"]] is None else str(row[field["name"]])
                           for field in decoded["fields"]) for row in rows]
        assert actual == cells
        ready = json.loads((read / "ready.json").read_text())
        assert ready["ready"] is True and ready["command"] == "partition-read"
        assert ready["partition"] == name and ready["tables"] == [{"name": "dup_events", "rows": len(rows), "ipc_bytes": (read / "dup_events.arrow").stat().st_size}]
        audit = seal.reader_audit(read / "audit.jsonl")
        columns = [seal.arrow_column(field) for field in decoded["fields"]]
        put(root / "schema.json", [{"table": "dup_events", "columns": columns}])
        put(root / "types.json", [seal.type_row("dup_events", col) for col in columns])
        scope = {
            "source_id": SOURCE, "source_generation": generation,
            "logical_schema": SCHEMA, "logical_table": "dup_events",
            "partition_id": name, "observed_partition_ids": ids,
            "complete": True, "table_epoch": metadata["table_epoch"],
            "schema_fingerprint": metadata["ddl_sha256"],
            "schema_digest": digest(root / "schema.json"),
            "type_digest": digest(root / "types.json"),
            "arrow_sha256": digest(read / "dup_events.arrow"),
            "row_count": len(rows), "current_signature": part["current_signature"],
            "mutations_pending": False,
            "source_engine": "starrocks", "key_model": "duplicate",
            "routing_column": "bucket", "partition_range": part["partition_range"],
            "native_partition_id": part["native_partition_id"],
            "native_partition_version": part["native_partition_version"],
        }
        capturer = {
            "protocol": "oo.starrocks.capture-tick/v1", "source_engine": "starrocks",
            "source_id": SOURCE, "source_generation": generation,
            "label": f"native-c4-{name}-{generation}", "database": SCHEMA,
            "product_reader_write_attempts": 0, "source_reader_audit": audit,
            "snapshot_token": False,
            "snapshot_token_basis": "same complete source partition set and signature before and after both reads; no cross-table token",
            "native_catalog_path": str(read / "catalog.json"),
            "native_catalog_sha256": digest(read / "catalog.json"),
            "native_columns": [column for column in json.loads((read / "catalog.json").read_text())["native_columns"] if column["table"].lower() == "dup_events"],
            "partition_scope": scope,
            "tables": [{"name": "dup_events", "arrow": "dup_events.arrow", "route": "C4",
                        "consistency": "S0", "delete_mode": "partition_replace",
                        "cursor_safety": "no_cursor", "quality": "OUT_OF_PARTITION_DELETE_NOT_EXTRAPOLATED",
                        "key_model": "duplicate", "rows": ["\x1f".join(seal.cell(row[field["name"]]) for field in decoded["fields"]) for row in rows]}],
        }
        put(root / "capturer.json", capturer)
        witness = {
            "protocol": "oo.source-admission.capture-witness/v1", "source_engine": "starrocks",
            "source_id": SOURCE, "source_generation": generation,
            "snapshot": {"kind": "capture-tick", "value": f"native-c4-{name}-{generation}"},
            "run_id": f"sr-native-c4-{name}-{generation}",
            "logical_schema": SCHEMA, "logical_table": "dup_events",
            "capturer_path": str(root / "capturer.json"),
            "arrow_path": str(read / "dup_events.arrow"),
            "driver_path": str(DRIVER),
            "schema_preimage_path": str(root / "schema.json"),
            "type_preimage_path": str(root / "types.json"),
        }
        put(root / "witness.json", witness)
        prior = BASE / "native-sync/captures/dddddddd-4545-4141-8181-000000001003/2"
        config = json.loads((prior / "config.json").read_text())
        report = json.loads((prior / "report.json").read_text())
        report["capture_witness"] = str(root / "witness.json")
        report["driver"]["digest"] = digest(DRIVER)
        for capability in report["capabilities"]:
            capability["consistency"] = "S0"
        put(root / "config.json", config)
        put(root / "report.json", report)
    print(json.dumps({"partitions": [x["name"] for x in metadata["partitions"]],
                      "generation": [args.first_generation, args.first_generation + 1], "observed_partition_names": ids,
                      "native_partition_ids": native_ids, "driver_sha256": digest(DRIVER)}))


if __name__ == "__main__":
    main()
