#!/usr/bin/env python3
"""Write a ClickHouse C4 witness for the stored five-column Arrow stream.

The stream is the product-run-12 file. Column types are read from that file.
This script does not relabel timestamp, drop note, or write a receipt.
The driver digest is the sha256 of the file passed in --driver.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

from arrow_stream import rows_from_stream, messages, _schema

ROOT = Path("/Users/redux/puppy-one/open-ontology")
ARROW = ROOT / (
    "evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/"
    "raw/run/product-run-12-customer-chain/out/raw/customer-five.arrow"
)
FIXTURE = ROOT / "tools/source-admission/fixtures/native"
RUN = ROOT / (
    "evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/"
    "raw/run/product-run-19-seal-native"
)
HISTORICAL_READER = "e62dda3084cca369487e20175178f9055371f028d45db6f35638ddc2d6edf9a1"
SNAPSHOT = "oo-dl01w1-40-customer-five"
SCHEMA = "oo_dl01w40"
TABLE = "readings"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_json(path: Path, value: object) -> None:
    path.write_text(json.dumps(value, indent=2) + "\n")


def column(field: dict) -> dict:
    item = {
        "name": field["name"],
        "logical_name": field["name"],
        "nullable": bool(field["nullable"]),
    }
    kind = field["type"]
    if kind == "int":
        width = int(field["bit_width"])
        signed = bool(field["signed"])
        item["arrow_type"] = f"{'int' if signed else 'uint'}{width}"
        item["precision"] = width
    elif kind == "decimal":
        bits = int(field["bit_width"])
        item["arrow_type"] = "decimal256" if bits == 256 else "decimal128"
        item["precision"] = int(field["precision"])
        item["scale"] = int(field["scale"])
    elif kind == "binary":
        item["arrow_type"] = "binary"
    elif kind == "utf8":
        item["arrow_type"] = "utf8"
    elif kind == "timestamp":
        item["arrow_type"] = "timestamp"
    else:
        raise SystemExit(f"arrow type {kind} was not decoded")
    if field["name"] == "reading_id":
        item["key"] = True
    return item


def row_text(row: dict) -> str:
    parts = []
    for key, value in row.items():
        if value is None:
            shown = "null"
        elif isinstance(value, bytes):
            shown = "hex:" + value.hex()
        else:
            shown = str(value)
        parts.append(f"{key}={shown}")
    return "|".join(parts)


def main() -> None:
    global ARROW, RUN, SNAPSHOT
    parser = argparse.ArgumentParser()
    parser.add_argument("--driver", type=Path, required=True)
    parser.add_argument("--arrow", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--snapshot")
    parser.add_argument("--fresh-capture-report", type=Path)
    parser.add_argument("--method", choices=("C4", "C5"), default="C4")
    args = parser.parse_args()
    if args.arrow or args.output or args.snapshot:
        if not all((args.arrow, args.output, args.snapshot, args.fresh_capture_report)):
            raise SystemExit("new capture requires arrow/output/snapshot/capture report together")
        ARROW = args.arrow.resolve()
        RUN = args.output.resolve()
        SNAPSHOT = args.snapshot
    driver = args.driver.expanduser()
    if not driver.is_absolute():
        raise SystemExit("driver path must stay absolute")
    if not driver.is_file():
        raise SystemExit("driver file is missing")
    if not ARROW.is_file():
        raise SystemExit("customer arrow is missing")
    driver_digest = sha256(driver)
    arrow_bytes = ARROW.read_bytes()
    if args.fresh_capture_report:
        capture = json.loads(args.fresh_capture_report.read_text())
        identity = json.loads((args.fresh_capture_report.parent / "identity.json").read_text())
        if (capture.get("status") != 200 or capture.get("ipc_sha256") != sha256(ARROW)
                or identity.get("producer_sha256") != driver_digest
                or identity.get("reuses_historical_capture") is not False):
            raise SystemExit("fresh capture report/bytes/producer binding does not match")
    fields = _schema(messages(arrow_bytes)[0][0])
    names = [field["name"] for field in fields]
    if names != ["reading_id", "ts", "amount", "note", "qty"]:
        raise SystemExit(f"columns {names}")
    rows = rows_from_stream(arrow_bytes)
    if len(rows) != 3:
        raise SystemExit(f"row count {len(rows)}")
    if rows[0]["note"] is not None or not isinstance(rows[2]["note"], bytes):
        raise SystemExit("note null and binary were not both present")

    inputs = RUN / "inputs"
    inputs.mkdir(parents=True, exist_ok=True)
    columns = [column(field) for field in fields]
    if args.fresh_capture_report:
        for item, field in zip(columns, fields):
            if field["type"] == "timestamp":
                if field.get("unit") != 3 or field.get("timezone") != "UTC":
                    raise SystemExit("fresh timestamp requires original UTC nanoseconds")
                item["arrow_type"] = "timestamp_nanos_utc"
    capturer_path = inputs / "capturer.json"
    schema_path = inputs / "schema-preimage.json"
    type_path = inputs / "type-preimage.json"
    witness_path = inputs / "witness.json"
    report_path = inputs / "report.json"
    config_path = inputs / "config.json"

    capturer = {
        "protocol": "oo.clickhouse.capture-tick/v1",
        "source_engine": "clickhouse",
        "source_id": "clickhouse_268",
        "source_generation": 1,
        "label": SNAPSHOT,
        "snapshot": {"kind": "token", "value": SNAPSHOT},
        "database": SCHEMA,
        "logical_schema": SCHEMA,
        "cross_table_consistency": "B",
        "product_reader_write_attempts": 0,
        "snapshot_token": False,
        "tables": [
            {
                "name": TABLE,
                "arrow": ARROW.name,
                "cursor_safety": "no_cursor",
                "consistency": "S0",
                "route": args.method,
                "delete_mode": "partition_replace" if args.method == "C4" else "reconcile",
                "tombstones": [],
                "rows": [row_text(row) for row in rows],
                "row_count": len(rows),
            }
        ],
    }
    schema = [{"table": TABLE, "columns": columns}]
    types = [
        {
            "table": TABLE,
            "name": item["name"],
            "arrow_type": item["arrow_type"],
            **({"precision": item["precision"]} if "precision" in item else {}),
            **({"scale": item["scale"]} if "scale" in item else {}),
        }
        for item in columns
    ]
    witness = {
        "protocol": "oo.source-admission.capture-witness/v1",
        "source_engine": "clickhouse",
        "source_id": "clickhouse_268",
        "source_generation": 1,
        "snapshot": {"kind": "token", "value": SNAPSHOT},
        "run_id": SNAPSHOT if args.fresh_capture_report else "product-run-19-seal-c4",
        "logical_schema": SCHEMA,
        "logical_table": TABLE,
        "capturer_path": str(capturer_path),
        "arrow_path": str(ARROW),
        "driver_path": str(driver),
        "schema_preimage_path": str(schema_path),
        "type_preimage_path": str(type_path),
    }
    report = json.loads((FIXTURE / "clickhouse-attempt06-report.json").read_text())
    config = json.loads((FIXTURE / "clickhouse-attempt06-config.json").read_text())
    report["driver"]["digest"] = driver_digest
    report["capture_methods"] = []
    report["capture_witness"] = str(witness_path)
    if report["native"] != config["native"]:
        raise SystemExit("fixture native profile differs")
    if "secret://" not in report["secret_ref"]:
        raise SystemExit("fixture secret_ref missing")

    write_json(capturer_path, capturer)
    write_json(schema_path, schema)
    write_json(type_path, types)
    write_json(witness_path, witness)
    write_json(report_path, report)
    write_json(config_path, config)
    write_json(inputs / "arrow-schema.json", fields)
    write_json(
        inputs / "producer-gap.json",
        {
            "arrow_sha256": hashlib.sha256(arrow_bytes).hexdigest(),
            "arrow_path": str(ARROW),
            "historical_reader_sha256": HISTORICAL_READER,
            "historical_reader_present": driver_digest == HISTORICAL_READER,
            "driver_path": str(driver),
            "driver_sha256": driver_digest,
            "driver_is_historical_producer": driver_digest == HISTORICAL_READER,
            "fresh_capture_report": str(args.fresh_capture_report) if args.fresh_capture_report else None,
            "reuses_historical_capture": args.fresh_capture_report is None,
            "note": "kept",
            "ts_arrow_type": "timestamp",
            "ts_unit": "nanoseconds",
            "ts_timezone": "UTC",
        },
    )
    for path in (capturer_path, schema_path, type_path, witness_path):
        if b"secret://" in path.read_bytes():
            raise SystemExit(f"secret in {path.name}")
    print(json.dumps({"driver_sha256": driver_digest, "rows": len(rows), "inputs": str(inputs)}))


if __name__ == "__main__":
    main()
