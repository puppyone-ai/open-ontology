#!/usr/bin/env python3
"""Bind one live five-column ClickHouse capture to the sealed customer catalog.

The catalog file is copied, not regenerated. Digests are read from that file.
The registry has no secret reference and no admission authority.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

EXPECTED_FIELDS = ("reading_id", "ts", "amount", "note", "qty")
SNAPSHOT = "oo-dl01w1-40-customer-five"
RECEIPT_SHA = "65b7f22fe26686f8a1e61d705a78d5a1a69f2a123de0b9dfce827cce2ffb59ef"
ARROW_TOKENS = {
    "UInt64": ("uint64", 64, None),
    "Int64": ("int64", 64, None),
    "Decimal256(76, 10)": ("decimal256", 76, 10),
    'Timestamp(ns, "UTC")': ("timestamp_nanos_utc", None, None),
    "Binary": ("binary", None, None),
}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    digest.update(path.read_bytes())
    return digest.hexdigest()


def require_sha(path: Path, expect: str, label: str) -> None:
    actual = sha256(path)
    if actual != expect:
        raise SystemExit(f"{label} digest mismatch: {actual}")


def column_from_field(field: dict) -> dict:
    token = ARROW_TOKENS.get(field.get("data_type"))
    if token is None:
        raise SystemExit(f"unsupported capture type {field.get('data_type')}")
    arrow_type, precision, scale = token
    column = {
        "arrow_type": arrow_type,
        "logical_name": field["name"],
        "name": field["name"],
        "nullable": field["nullable"],
    }
    if precision is not None:
        column["precision"] = precision
    if scale is not None:
        column["scale"] = scale
    return column


def registry_field(column: dict) -> dict:
    field = {
        "arrow_type": column["arrow_type"],
        "name": column["name"],
        "nullable": column["nullable"],
    }
    if "precision" in column:
        field["precision"] = column["precision"]
    if "scale" in column:
        field["scale"] = column["scale"]
    if column["arrow_type"] == "timestamp_nanos_utc":
        field["timezone"] = "UTC"
    return field


def manifest(columns: list[dict], owner: str, args: argparse.Namespace, catalog: dict) -> dict:
    return {
        "admission": {
            "decision_path": args.decision_container,
            "receipt_path": args.receipt_container,
        },
        "catalog_identity": {
            "config_digest": catalog["config_digest"],
            "input_digest": catalog["input_digest"],
            "schema_digest": catalog["schema_digest"],
            "type_digest": catalog["type_digest"],
        },
        "protocol": "oo.pipeline.native-arrow-import/v1",
        "snapshot": {"kind": "token", "value": SNAPSHOT},
        "source_engine": "clickhouse",
        "source_generation": 1,
        "source_id": "clickhouse_268",
        "tables": [
            {
                "arrow_path": args.arrow_container,
                "batch_sha256": sha256(args.arrow),
                "columns": columns,
                "logical_schema": "readings",
                "logical_table": "final",
                "row_count": 3,
            }
        ],
        "target": {
            "ca_file": args.ca_container,
            "database": "previewdb",
            "host": args.host,
            "owner": owner,
            "password_file": args.password_container,
            "port": 5432,
            "runtime_principal": "oo_admin",
            "shard": "clickhouse_268",
        },
        "target_storage": "postgresql",
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--catalog", type=Path, required=True)
    parser.add_argument("--capture", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--decision", type=Path, required=True)
    parser.add_argument("--arrow", type=Path, required=True)
    parser.add_argument("--count-file", type=Path, required=True)
    parser.add_argument("--driver-sha-file", type=Path, required=True)
    parser.add_argument("--owner", required=True)
    parser.add_argument("--host", required=True)
    parser.add_argument("--report-container", required=True)
    parser.add_argument("--config-container", required=True)
    parser.add_argument("--catalog-container", required=True)
    parser.add_argument("--capture-container", required=True)
    parser.add_argument("--receipt-container", required=True)
    parser.add_argument("--decision-container", required=True)
    parser.add_argument("--arrow-container", required=True)
    parser.add_argument("--ca-container", required=True)
    parser.add_argument("--password-container", required=True)
    parser.add_argument("--registry-out", type=Path, required=True)
    parser.add_argument("--positive-out", type=Path, required=True)
    parser.add_argument("--negative-out", type=Path, required=True)
    args = parser.parse_args()

    require_sha(args.receipt, RECEIPT_SHA, "admission receipt")
    catalog_bytes = args.catalog.read_bytes()
    catalog = json.loads(catalog_bytes)
    if catalog.get("engine") != "clickhouse" or catalog.get("source_id") != "clickhouse_268":
        raise SystemExit("catalog is not the ClickHouse customer catalog")
    if catalog.get("run_id") != "46ef7460-bfca-442e-8e98-8fedbab63615":
        raise SystemExit("catalog run_id is not the customer draft")
    names = [column["logical_name"] for column in catalog["tables"][0]["columns"]]
    if tuple(names) != EXPECTED_FIELDS:
        raise SystemExit(f"catalog columns are {names}")
    if catalog["tables"][0].get("accepted_rows") != 3:
        raise SystemExit("catalog accepted_rows is not 3")
    count_text = args.count_file.read_text(encoding="utf-8").strip()
    if count_text != "3":
        raise SystemExit(f"independent source count is {count_text}")
    capture = json.loads(args.capture.read_text(encoding="utf-8"))
    if capture.get("query_id") != SNAPSHOT or capture.get("status") != 200:
        raise SystemExit("capture is not the customer five-column read")
    if capture.get("rows") != 3:
        raise SystemExit(f"capture rows are {capture.get('rows')}")
    fields = capture.get("fields")
    if not isinstance(fields, list) or [field.get("name") for field in fields] != list(EXPECTED_FIELDS):
        raise SystemExit("capture field order is not the approved five columns")
    arrow_sha = sha256(args.arrow)
    if capture.get("body_sha256") != arrow_sha or capture.get("ipc_sha256") != arrow_sha:
        raise SystemExit("arrow file does not match the capture digest")
    report = json.loads(args.report.read_text(encoding="utf-8"))
    driver_sha = args.driver_sha_file.read_text(encoding="utf-8").strip().split()[0]
    driver = report.get("driver") or {}
    if driver.get("digest") != driver_sha or driver.get("name") != "ch-reader":
        raise SystemExit("report driver digest is not the built ch-reader")
    if report.get("source_id") != "clickhouse_268":
        raise SystemExit("report source is not clickhouse_268")
    decision_sha = sha256(args.decision)
    columns = [column_from_field(field) for field in fields]
    if columns[1]["arrow_type"] != "timestamp_nanos_utc" or columns[2]["arrow_type"] != "decimal256":
        raise SystemExit("ts or amount token changed")
    if columns[3]["nullable"] is not True or columns[3]["arrow_type"] != "binary":
        raise SystemExit("note is not nullable binary")
    catalog_sha = hashlib.sha256(catalog_bytes).hexdigest()
    binding = {
        "admission_receipt_path": args.receipt_container,
        "capture_receipt_path": args.capture_container,
        "capture_receipt_sha256": sha256(args.capture),
        "catalog_path": args.catalog_container,
        "catalog_sha256": catalog_sha,
        "config_digest": catalog["config_digest"],
        "config_path": args.config_container,
        "config_sha256": sha256(args.config),
        "decision_sha256": decision_sha,
        "driver_digest": driver_sha,
        "driver_kind": "native-http",
        "driver_name": "ch-reader",
        "driver_version": "0.1.0",
        "fields": [registry_field(column) for column in columns],
        "input_digest": catalog["input_digest"],
        "objects": [
            {
                "arrow_sha256": arrow_sha,
                "logical_schema": "readings",
                "logical_table": "final",
                "row_count": 3,
            }
        ],
        "protocol": "oo.source-admission.capture-binding/v1",
        "report_path": args.report_container,
        "report_sha256": sha256(args.report),
        "schema_digest": catalog["schema_digest"],
        "snapshot": {"kind": "token", "value": SNAPSHOT},
        "source_engine": "clickhouse",
        "source_generation": 1,
        "source_id": "clickhouse_268",
        "type_digest": catalog["type_digest"],
    }
    registry = {
        "bindings": [binding],
        "protocol": "oo.source-admission.binding-registry/v1",
    }
    positive = manifest(columns, args.owner, args, catalog)
    negative = manifest(columns, "not-a-uuid", args, catalog)
    rendered = json.dumps(registry)
    if "secret://" in rendered or "secret://" in json.dumps(positive) or "secret://" in json.dumps(negative):
        raise SystemExit("plan copied a secret reference")
    for key in ("admitted", "admission_bin", "registry_path", "registry_sha256", "approval", "secret_ref"):
        if key in positive or key in positive["admission"] or key in registry:
            raise SystemExit("manifest grants authority")
    args.registry_out.write_text(json.dumps(registry, indent=2) + "\n")
    args.positive_out.write_text(json.dumps(positive, indent=2) + "\n")
    args.negative_out.write_text(json.dumps(negative, indent=2) + "\n")
    print(hashlib.sha256(args.registry_out.read_bytes()).hexdigest())


if __name__ == "__main__":
    main()
