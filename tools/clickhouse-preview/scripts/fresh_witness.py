#!/usr/bin/env python3
"""Prepare a witness from new capture evidence, including all original columns.

This writes input claims only. Admission is performed by the shared sealer.
"""
import argparse
import hashlib
import json
from pathlib import Path

TOKENS = {
    "UInt64": ("uint64", 64, None),
    "Int64": ("int64", 64, None),
    "Decimal256(76, 10)": ("decimal256", 76, 10),
    "Binary": ("binary", None, None),
    "Dictionary(Int32, Binary)": ("dictionary_int32_binary", None, None),
    'Timestamp(ns, "UTC")': ("timestamp_nanos_utc", None, None),
    "List(non-null Int64)": ("list_int64", None, None),
    "List(Binary)": ("list_binary", None, None),
}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--capture", type=Path, required=True)
    p.add_argument("--driver", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    a = p.parse_args()
    capture = a.capture.resolve()
    out = a.output.resolve()
    driver = a.driver.resolve()
    identity = json.loads((capture / "identity.json").read_text())
    result = json.loads((capture / "full-capture.json").read_text())
    oracle = json.loads((capture / "full-allcells-verdict.json").read_text())
    arrow = capture / "full.arrow"
    if identity["producer_sha256"] != digest(driver) or identity["reuses_historical_capture"]:
        raise SystemExit("producer evidence mismatch")
    if result["status"] != 200 or result["ipc_sha256"] != digest(arrow) or not oracle["passed"]:
        raise SystemExit("capture/oracle evidence mismatch")
    audit = [json.loads(line) for line in (capture / "reader-final-audit.stdout").read_text().splitlines()]
    if any(item["query_kind"] not in ("Select", "Show") or str(item["written_rows"]) != "0" for item in audit):
        raise SystemExit("source product audit contains write or non-read attempt")
    columns = []
    for field in result["fields"]:
        token, precision, scale = TOKENS[field["data_type"]]
        item = {"name": field["name"], "logical_name": field["name"],
                "arrow_type": token, "nullable": field["nullable"]}
        if precision is not None:
            item["precision"] = precision
        if scale is not None:
            item["scale"] = scale
        if item["name"] == "reading_id":
            item["key"] = True
        columns.append(item)
    snapshot = "oo-dl01w1-40-gpt-fresh-full"
    out.mkdir(parents=True, exist_ok=False)
    capturer = {"protocol": "oo.clickhouse.capture-tick/v1", "source_engine": "clickhouse",
        "source_id": "clickhouse_268", "source_generation": 1, "label": snapshot,
        "snapshot": {"kind": "token", "value": snapshot}, "database": "oo_dl01w40",
        "logical_schema": "oo_dl01w40", "cross_table_consistency": "B",
        "product_reader_write_attempts": 0, "snapshot_token": False,
        "tables": [{"name": "readings", "arrow": arrow.name, "cursor_safety": "no_cursor",
            "consistency": "S0", "route": "C5", "delete_mode": "reconcile", "tombstones": [],
            "rows": [json.dumps(row, sort_keys=True) for row in oracle["rows"]], "row_count": len(oracle["rows"])}]}
    witness = {"protocol": "oo.source-admission.capture-witness/v1", "source_engine": "clickhouse",
        "source_id": "clickhouse_268", "source_generation": 1, "snapshot": capturer["snapshot"],
        "run_id": snapshot, "logical_schema": "oo_dl01w40", "logical_table": "readings",
        "capturer_path": str(out / "capturer.json"), "arrow_path": str(arrow), "driver_path": str(driver),
        "schema_preimage_path": str(out / "schema-preimage.json"), "type_preimage_path": str(out / "type-preimage.json")}
    root = Path(__file__).resolve().parents[3]
    fixtures = root / "tools/source-admission/fixtures/native"
    report = json.loads((fixtures / "clickhouse-attempt06-report.json").read_text())
    config = json.loads((fixtures / "clickhouse-attempt06-config.json").read_text())
    report["driver"]["digest"] = digest(driver)
    report["capture_methods"] = []
    report["capture_witness"] = str(out / "witness.json")
    types = [{"table": "readings", **{k: v for k, v in item.items() if k in ("name", "arrow_type", "precision", "scale")}} for item in columns]
    for name, body in {"capturer": capturer, "witness": witness, "schema-preimage": [{"table": "readings", "columns": columns}],
                       "type-preimage": types, "report": report, "config": config}.items():
        (out / (name + ".json")).write_text(json.dumps(body, indent=2) + "\n")
    print(json.dumps({"input_only": True, "output": str(out), "columns": len(columns), "rows": len(oracle["rows"])}))


if __name__ == "__main__":
    main()
