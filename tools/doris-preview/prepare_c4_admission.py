#!/usr/bin/env python3
"""Bind one real Doris partition tick to a source-admission witness.

This prepares byte-stable source evidence for a trusted sealer. It neither
claims an ACK nor writes to Doris or the target.
"""

import argparse
import hashlib
import json
from pathlib import Path


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def save(path, body):
    if path.exists():
        raise ValueError("immutable witness output exists: " + str(path))
    path.write_text(json.dumps(body, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser()
    for field in ("tick", "reader", "config", "schema-preimage", "type-preimage"):
        parser.add_argument("--" + field, type=Path, required=True)
    parser.add_argument("--generation", type=int, required=True)
    args = parser.parse_args()
    if args.generation < 1:
        raise ValueError("source generation")
    tick = json.loads(args.tick.read_text())
    table = tick["tables"]
    if tick["protocol"] != "oo.doris.capture-tick/v1" or len(table) != 1 \
            or table[0]["name"] != "part_day" or table[0]["route"] != "C4" \
            or table[0]["cursor_safety"] != "no_cursor" \
            or tick["product_reader_write_attempts"] != 0:
        raise ValueError("actual Doris partition capture")
    directory = args.tick.parent
    arrow = directory / table[0]["arrow"]
    scope = dict(tick["partition_scope"])
    scope.update({"source_id": "dorispreview", "source_generation": args.generation,
                  "logical_schema": "oo_dl01w1_43", "logical_table": "part_day",
                  "schema_digest": digest(args.schema_preimage),
                  "type_digest": digest(args.type_preimage),
                  "arrow_sha256": digest(arrow)})
    if len(scope) != 21 or "previous_ack_exists" in scope or "previous_signature" in scope:
        raise ValueError("source C4 scope")
    capturer = dict(tick)
    capturer.update({"source_id": "dorispreview", "source_generation": args.generation,
                     "logical_schema": "oo_dl01w1_43", "logical_table": "part_day",
                     "partition_scope": scope, "reader_capture_sha256": digest(args.tick)})
    save(directory / "capturer.json", capturer)
    witness = {"protocol": "oo.source-admission.capture-witness/v1",
               "source_engine": "doris", "source_id": "dorispreview",
               "source_generation": args.generation,
               "snapshot": {"kind": "capture-tick", "value": tick["label"]},
               "run_id": f"dorispreview-{tick['label']}-{tick['finished_unix_ms']}",
               "logical_schema": "oo_dl01w1_43", "logical_table": "part_day",
               "capturer_path": str(directory / "capturer.json"), "arrow_path": str(arrow),
               "driver_path": str(args.reader),
               "schema_preimage_path": str(args.schema_preimage),
               "type_preimage_path": str(args.type_preimage)}
    save(directory / "witness.json", witness)
    config = json.loads(args.config.read_text())
    report = {"protocol": "oo.source-admission.report/v1", "target_id": "dorispreview",
              "source_id": "dorispreview", "secret_ref": config["secret_ref"],
              "driver": {"kind": "native", "name": "doris-reader", "version": "0.1.0",
                         "digest": digest(args.reader)}, "compat_mode": "doris",
              "license_flags": ["Apache-2.0"], "permissions": config["permissions"],
              "capabilities": [{"name": "BatchFull", "cursor": None, "feed": None,
                                "cdc": None, "token_order": None, "budget_unit": None,
                                "level": "experimental", "write_class": "R0",
                                "consistency": "S0"}],
              "capture_methods": [], "r1w": None,
              "cost_model": {"unit": "Rows", "confidence": "unknown",
                             "estimate": "Doris billing is unpublished; configured observed reader row budget"},
              "capture_witness": str(directory / "witness.json")}
    save(directory / "report.json", report)
    (directory / "config.json").write_bytes(args.config.read_bytes())
    print(json.dumps({name: digest(directory / name) for name in
                      ("report.json", "config.json", "witness.json", "capturer.json")},
                     sort_keys=True))


if __name__ == "__main__":
    main()
