#!/usr/bin/env python3
"""Actual source read + admission per durable native scheduler occurrence.

Diagnostics and original process bytes stay in owned evidence; stdout is only
an independently sealed NativeUpdate request. No target SQL or jobs operation.
"""
import argparse
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys

HELPER_SHA256 = "06ef18386b6b6d4ad23072b0d9f37d3d20f4a04ef375918ad254be6634f7922f"
helper_path = Path(__file__).resolve().parents[1] / "scripts/seal_capture_witness.py"
if hashlib.sha256(helper_path.read_bytes()).hexdigest() != HELPER_SHA256:
    raise SystemExit("starrocks-native-producer: immutable helper changed")
spec = importlib.util.spec_from_file_location("seal", helper_path)
seal = importlib.util.module_from_spec(spec)
spec.loader.exec_module(seal)



def write(path, value):
    with path.open("w") as handle:
        handle.write(json.dumps(value, sort_keys=True, indent=2) + "\n")
        handle.flush()
        os.fsync(handle.fileno())
    directory_fd = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(directory_fd)
    finally:
        os.close(directory_fd)


def phase(root, name, args):
    result = subprocess.run(args, capture_output=True, timeout=120)
    (root / (name + ".stdout")).write_bytes(result.stdout)
    (root / (name + ".stderr")).write_bytes(result.stderr)
    (root / (name + ".exit")).write_text(str(result.returncode) + "\n")
    if result.returncode:
        raise RuntimeError(name + " refused; retained raw phase")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", required=True, type=Path)
    parser.add_argument("--table", required=True)
    parser.add_argument("--config-sha256", required=True)
    args = parser.parse_args()
    if hashlib.sha256(args.config.read_bytes()).hexdigest() != args.config_sha256:
        raise RuntimeError("operator config immutable digest changed")
    config = json.loads(args.config.read_text())
    event = json.load(sys.stdin)
    if event.get("protocol") != "oo.jobs.native-producer/v1" or event.get("source_id") != config["source_id"] or event.get("method") != "C5":
        raise RuntimeError("source binding mismatch")
    tables = {t["name"]: t for t in config["capture"]["tables"]}
    selected = tables[args.table]
    if selected.get("method", "C5") != "C5":
        raise RuntimeError("C4 requires partition producer; full read cannot impersonate partition refresh")
    occurrence_pin = event["pin"]
    pin = event["acknowledged_source_generation"] + 1
    binding = event["binding"]
    if not isinstance(pin, int) or pin < 1 or not isinstance(binding, str) or len(binding) != 36 or any(c not in "0123456789abcdef-" for c in binding):
        raise RuntimeError("occurrence identity")
    for name in ("probe", "decode", "validator"):
        if seal.sha256(Path(config[name]["path"])) != config[name]["sha256"]:
            raise RuntimeError(name + " immutable digest changed")
    owned = Path(config["evidence_root"]).resolve()
    if not owned.is_absolute() or not owned.is_dir():
        raise RuntimeError("approved evidence root unavailable")
    root = owned / binding / str(occurrence_pin)
    root.mkdir(parents=True, exist_ok=True)
    with (root / "producer.lock").open("w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        request_path = root / "request.json"
        if request_path.exists():
            sys.stdout.write(request_path.read_text())
            return 0
        # Partial occurrence is retained: it must not recapture after an
        # interrupted read and silently substitute new source bytes.
        reads = root / "reads"
        decoded = root / "decoded"
        marker = root / "capture-started.json"
        if marker.exists():
            recovery = root / "source-recovery.json"
            if not recovery.is_file():
                raise RuntimeError("partial capture requires explicit source recovery")
            attested = json.loads(recovery.read_text())
            if attested.get("protocol") != "oo.starrocks.source-recovery/v1" or attested.get("binding") != binding or attested.get("occurrence_pin") != occurrence_pin:
                raise RuntimeError("source recovery does not match this occurrence")
            ready = reads / "ready.json"
            if not ready.is_file() or json.loads(ready.read_text()).get("ready") is not True:
                raise RuntimeError("partial read is incomplete; retained, not recaptured")
        else:
            write(marker, event)
        if not (reads / "ready.json").is_file():
            write(root / "connection.json", config["capture"])
            phase(root, "probe", [config["probe"]["path"], "--config", str(root / "connection.json"), "--out", str(reads)])
            if json.loads((reads / "ready.json").read_text()).get("ready") is not True:
                raise RuntimeError("source not ready")
        phase(root, "decode", [config["decode"]["path"], "--out", str(decoded), *[str(p) for p in sorted(reads.glob("*.arrow"))]])
        audit = seal.reader_audit(reads / "audit.jsonl")
        native_catalog = json.loads((reads / "catalog.json").read_text())
        if not native_catalog.get("native_columns"):
            raise RuntimeError("actual native schema metadata missing")
        native_table = selected.get("derived_from", args.table)
        native_columns = [column for column in native_catalog["native_columns"] if column["table"].lower() == native_table]
        if not native_columns:
            raise RuntimeError("actual native table metadata missing")
        schema_path = root.parent / "approved-native-schema.json"
        if schema_path.exists():
            if json.loads(schema_path.read_text()) != native_columns:
                raise RuntimeError("schema_changed; retained metadata requires review before seal")
        else:
            write(schema_path, native_columns)
        document = json.loads((decoded / (args.table + ".json")).read_text())
        columns = [seal.arrow_column(field) for field in document["fields"]]
        obj = next(o for o in native_catalog["objects"] if o["name"] == args.table)
        # Pinned probe ELF omits logical_dataset_kind; uniq_dim is a native table.
        kind = obj.get("logical_dataset_kind") or ("derived_projection" if args.table == "dup_partition_sums" else "native_table")
        model = "derived_aggregate" if kind == "derived_projection" else obj["key_model"].lower()
        aggregate = model in {"aggregate", "derived_aggregate"}
        quality = "DELETES_UNDETECTED" if aggregate else "DELETES_LAGGED"
        detection = "none" if aggregate else "reconcile"
        capturer = {"protocol": "oo.starrocks.capture-tick/v1", "source_engine": "starrocks", "source_id": config["source_id"], "source_generation": pin, "label": f"{binding}-{pin}", "database": config["capture"]["source"]["database"], "product_reader_write_attempts": audit["write_attempts"], "source_reader_audit": audit, "snapshot_token": False, "snapshot_token_basis": "finished full reads; no transactional snapshot token", "native_catalog_path": str(reads / "catalog.json"), "native_catalog_sha256": seal.sha256(reads / "catalog.json"), "native_columns": native_columns, "logical_dataset_kind": kind, "tables": [{"name": args.table, "arrow": args.table + ".arrow", "route": "C5", "consistency": "B", "delete_mode": detection, "cursor_safety": "no_cursor", "quality": quality, "key_model": model, "rows": ["\x1f".join(seal.cell(row[f["name"]]) for f in document["fields"]) for row in document["rows"]]}]}
        write(root / "capturer.json", capturer)
        write(root / "schema.json", [{"table": args.table, "columns": columns}])
        write(root / "types.json", [seal.type_row(args.table, c) for c in columns])
        snapshot = {"kind": "capture-tick", "value": f"{binding}-{pin}"}
        witness = {"protocol": "oo.source-admission.capture-witness/v1", "source_engine": "starrocks", "source_id": config["source_id"], "source_generation": pin, "snapshot": snapshot, "run_id": f"sr-live-{binding}-{pin}", "logical_schema": config["capture"]["source"]["database"], "logical_table": args.table, "capturer_path": str(root / "capturer.json"), "arrow_path": str(reads / (args.table + ".arrow")), "driver_path": config["probe"]["path"], "schema_preimage_path": str(root / "schema.json"), "type_preimage_path": str(root / "types.json")}
        write(root / "witness.json", witness)
        admission_config = json.loads(Path(config["admission_config"]).read_text())
        report = json.loads(Path(config["admission_report_template"]).read_text())
        for value in (admission_config, report):
            value["source_id"] = config["source_id"]
            value["target_id"] = config["source_id"]
        report["driver"]["digest"] = config["probe"]["sha256"]
        report["capture_witness"] = str(root / "witness.json")
        for capability in report["capabilities"]:
            capability["consistency"] = "B"
        write(root / "config.json", admission_config)
        write(root / "report.json", report)
        phase(root, "seal", [config["validator"]["path"], "seal-native", "--config", str(root / "config.json"), "--report", str(root / "report.json"), "--out", str(root / "sealed")])
        receipt = json.loads((root / "sealed/admission-receipt.json").read_text())
        request = {"protocol": "oo.pipeline.native-update/v1", "source_engine": "starrocks", "source_id": config["source_id"], "source_generation": pin, "parent_source_generation": None if pin == 1 else pin - 1, "logical_schema": config["capture"]["source"]["database"], "logical_table": args.table, "snapshot": receipt["snapshot"], "admission_path": str(root / "sealed/admission-receipt.json"), "registry_path": str(root / "sealed/registry.json"), "arrow_path": witness["arrow_path"], "audit_digest": seal.sha256(root / "capturer.json"), "batch_id": f"{binding}_{pin}", "capture_method": "c5_diff", "consistency_grade": receipt["consistency"], "cross_table_consistent": False, "delete_detection": receipt["delete_detection"], "full_snapshot": True, "upsert_supported": False, "window_closed": True, "position_codec": "snapshot-zero/v1", "position_lineage": "native-update.batch", "applied_pos": "0", "columns": columns, "tombstones": [], "mirror": False, "quality": [quality], "target": config["target"]}
        for key in ("config_digest", "schema_digest", "type_digest", "input_digest"):
            request[key] = receipt[key]
        temporary = root / "request.tmp"
        write(temporary, request)
        os.replace(temporary, request_path)
        directory_fd = os.open(root, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory_fd)
        finally:
            os.close(directory_fd)
        sys.stdout.write(request_path.read_text())
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:
        print("starrocks-native-producer: " + str(exc), file=sys.stderr)
        raise SystemExit(1)
