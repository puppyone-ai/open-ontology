#!/usr/bin/env python3
"""Seal a changed Rust C5 snapshot as the next native Loader request."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools/oracle-preview"))
import c5_capture
import c5_witness
import current_capture


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def durable(path, body):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temp = path.with_suffix(path.suffix + ".tmp")
    with temp.open("wb") as stream:
        stream.write(body)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temp, path)


def fields_for_encoder(fields):
    out = []
    for field in fields:
        kind = field["arrow_type"]
        encoder_type = {"timestamp_micros": "timestamp_us"}.get(kind, kind)
        body = {"name": field["name"], "type": encoder_type, "nullable": field["nullable"]}
        for key in ("precision", "scale"):
            if key in field:
                body[key] = field[key]
        out.append(body)
    return out


def produce(config, result_dir, input_path):
    envelope = json.loads(Path(input_path).read_text())
    result = json.loads((Path(result_dir) / "result.json").read_text())
    binding = envelope["binding"]
    pin = envelope["pin"]
    source = config["source_id"]
    base = envelope["acknowledged_source_generation"]
    generation = base + 1
    if (envelope.get("protocol") != "oo.jobs.native-producer/v1"
            or envelope.get("method") != "C5"
            or envelope.get("source_id") != source
            or result.get("source_id") != source
            or result.get("source_generation") != base
            or result.get("empty_diff") is not False
            or result.get("reader_mutations") != 0):
        raise ValueError("changed snapshot identity")

    occurrence = Path(config["output_root"]) / "produced" / binding / str(pin)
    if occurrence.exists():
        raise ValueError("changed occurrence output already exists; retry must use its saved request")
    capture = occurrence / "capture"
    shadow_root = capture / "run" / "shadow" / config["table"]
    arrow_dir = capture / "run" / "arrow"
    sql_dir = capture / "run" / "sql"
    witness_dir = capture / "witness" / source
    sealed = occurrence / "sealed"
    for directory in (shadow_root, arrow_dir, sql_dir, capture / "witness"):
        directory.mkdir(parents=True, exist_ok=False)

    parent_capture = (Path(config["baseline_capture"]) if base == config["baseline_generation"]
                      else Path(config["output_root"]) / "produced" / binding
                      / str(envelope["acknowledged_pin"]) / "capture")
    parent_shadow = c5_capture.load_generation(parent_capture / "run" / "shadow" / config["table"], base)
    profile = json.loads((parent_capture / "run" / "producer-profile.json").read_text())
    durable(capture / "run" / "producer-profile.json", json.dumps(profile, indent=2).encode() + b"\n")
    parent_request = json.loads(Path(config["baseline_request"]).read_text()) if base == config["baseline_generation"] else json.loads(
        (Path(config["output_root"]) / "produced" / binding / str(envelope["acknowledged_pin"]) / "request.json").read_text())
    if parent_request.get("source_generation") != base or parent_request.get("source_id") != source:
        raise ValueError("acknowledged parent request identity")
    parent_arrow = Path(parent_request["arrow_path"])
    if sha256(parent_arrow) != parent_request.get("input_digest"):
        raise ValueError("acknowledged parent Arrow digest")

    fields = parent_request["columns"]
    columns = [field["name"] for field in fields]
    rows = result.get("rows_data")
    dictionary = result.get("columns")
    if not isinstance(rows, list) or not isinstance(dictionary, list) or not rows or len(rows) > config["max_rows"]:
        raise ValueError("changed snapshot row budget")
    if [column.get("name") for column in dictionary] != columns:
        raise ValueError("Oracle dictionary differs from admitted parent")
    cells = {}
    for row in rows:
        if not isinstance(row, dict) or set(row) != set(columns):
            raise ValueError("Oracle row shape")
        key = row[config["key"]]
        if isinstance(key, str) and key.isdigit():
            key = int(key)
            row[config["key"]] = str(key)
        if not isinstance(key, int) or key <= 0:
            raise ValueError("C5 primary key type")
        cells[str(key)] = row
    if len(cells) != len(rows):
        raise ValueError("duplicate C5 primary key")

    profile_columns = profile["columns"][config["table"]]
    if [column["name"] for column in profile_columns] != columns:
        raise ValueError("parent dictionary profile mismatch")
    generation_position = "configured-oracle-scn-" + str(result["scn"])
    applied = c5_capture.apply_snapshot(shadow_root, parent_shadow, generation, generation_position,
                                        columns, columns, cells, config["max_rows"], snapshot_token=True)
    if not applied["committed"] or applied["state"] != "committed":
        raise ValueError("C5 shadow refused changed snapshot")

    arrow_name = config["table"].lower() + "-gen-" + str(generation) + ".arrow"
    request_path = arrow_dir / (arrow_name + ".json")
    request = {"table": config["table"], "generation": generation,
               "columns": fields_for_encoder(fields),
               "rows": [cells[key] for key in sorted(cells, key=int)], "admitted": False}
    durable(request_path, json.dumps(request, indent=2, sort_keys=True).encode() + b"\n")
    encoder = Path("/t/r194/debug/oo-c5-arrow")
    if sha256(encoder) != current_capture.ENCODER_SHA:
        raise ValueError("fixed Arrow encoder drift")
    arrow_path = arrow_dir / arrow_name
    encoded = subprocess.run([str(encoder), "encode", str(request_path), str(arrow_path)], capture_output=True, timeout=30)
    durable(arrow_dir / (arrow_name + ".encode.stdout"), encoded.stdout)
    durable(arrow_dir / (arrow_name + ".encode.stderr"), encoded.stderr)
    durable(arrow_dir / (arrow_name + ".encode.exit"), str(encoded.returncode).encode() + b"\n")
    if encoded.returncode:
        raise ValueError("fixed Arrow encoder refused source rows")

    source_result = dict(result)
    source_result.update(source_generation=generation, acknowledged_source_generation=base,
                         parent_input_digest=parent_request["input_digest"],
                         input_digest=sha256(arrow_path))
    durable(Path(str(arrow_path) + ".source-result.json"), json.dumps(source_result, indent=2).encode() + b"\n")
    durable(Path(str(arrow_path) + ".source-digest.txt"), (result["source_digest"] + "\n").encode())
    durable(sql_dir / "rust-c5-source.sql", (";\n".join(result.get("source_sql", [])) + "\n").encode())
    durable(capture / "capture.json", json.dumps({
        "protocol": "oo.oracle.configured-capture/v1", "actual_scn": result["scn"],
        "generation": generation, "source_reader": "oracle-driver-probe",
        "source_reader_sha256": config["producer_sha256"], "source_dml_ddl": 0,
        "consistency_grade": "S0", "cross_table_consistent": False,
        "capture_method": "c5_diff", "snapshot_digests": {config["table"]: result["source_digest"]},
        "input_digests": {config["table"]: source_result["input_digest"]},
        "tables": {config["table"]: len(rows)}, "cdc": "off", "native_imported": False,
    }, indent=2).encode() + b"\n")
    c5_witness.CAPTURE = capture
    c5_witness.ARROW_DIR = arrow_dir
    c5_witness.PROFILE_PATH = capture / "run" / "producer-profile.json"
    c5_witness.SCHEMA = config["owner"]
    c5_witness.TARGET_ID = config["target_id"]
    c5_witness.SECRET_REF = config["secret_ref"]
    c5_witness.BUDGET_LIMIT = config["max_rows"]
    spec = {"table": config["table"], "generation": generation, "arrow": arrow_name, "source_id": source}
    driver_path = Path("/opt/oracle-driver-probe")
    driver = {"kind": "command-line", "name": "oracle-driver-probe", "version": "oracledb-26.0.0-beta.4",
              "digest": config["producer_sha256"], "bytes": driver_path.stat().st_size,
              "path": str(driver_path), "digest_is_file": True}
    permissions = result.get("permissions")
    if not isinstance(permissions, list):
        raise ValueError("current Oracle grant evidence absent")
    built = c5_witness.build_inputs(spec, driver, permissions)
    if built["unsupported"] or built["shadow"]["consistency_grade"] != "S0":
        raise ValueError("native C5 witness refused schema/grade")
    # Config identity is the operator's bytes; a map-key reorder is not a new configuration.
    baseline_config = json.loads((Path(config["baseline_capture"]) / "witness" / source / "config.json").read_text())
    if built["config"] != baseline_config:
        raise ValueError("Oracle capture configuration changed")
    built["config"] = baseline_config
    c5_witness.write_table(witness_dir, built, driver_path)

    command = ["/opt/admission", "seal-native", "--report", str(witness_dir / "report.json"),
               "--config", str(witness_dir / "config.json"), "--out", str(sealed)]
    admission = subprocess.run(command, capture_output=True, timeout=60)
    durable(occurrence / "admission.stdout", admission.stdout)
    durable(occurrence / "admission.stderr", admission.stderr)
    durable(occurrence / "admission.exit", str(admission.returncode).encode() + b"\n")
    if admission.returncode:
        raise ValueError("fixed native admission refused changed C5 capture")
    registry_path = sealed / "registry.json"
    registry = json.loads(registry_path.read_text())
    entry = registry["bindings"][0]
    request = dict(parent_request)
    request.update({key: entry[key] for key in ("config_digest", "schema_digest", "type_digest", "input_digest", "snapshot")})
    request.update(protocol="oo.pipeline.native-update/v1", source_engine="oracle", source_id=source,
                   source_generation=generation, parent_source_generation=base, full_snapshot=True,
                   batch_id=source + "-g" + str(generation), capture_method="c5_diff", delete_detection="reconcile",
                   consistency_grade="S0", cross_table_consistent=False, position_codec="snapshot-zero/v1",
                   position_lineage="configured.oracle", applied_pos="0", row_identity="none",
                   logical_schema=config["owner"], logical_table=config["table"],
                   columns=[dict(field, logical_name=field["name"]) for field in entry["fields"]],
                   arrow_path=str(arrow_path), tombstones=[], mirror=False,
                   audit_digest=entry["capture_receipt_sha256"],
                   admission_path=entry["admission_receipt_path"], registry_path=str(registry_path),
                   admission_expect={"source_engine": "oracle", "source_id": source,
                                     "source_generation": generation,
                                     "declared_receipt_sha256": entry["admission_receipt_sha256"]})
    request_path = occurrence / "request.json"
    durable(occurrence / "envelope.json", json.dumps(envelope, sort_keys=True).encode())
    durable(request_path, json.dumps(request, indent=2).encode() + b"\n")
    print(json.dumps(request, separators=(",", ":")))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--result-dir", type=Path, required=True)
    parser.add_argument("--input", type=Path, required=True)
    args = parser.parse_args()
    config = json.loads(args.config.read_text())
    produce(config, args.result_dir, args.input)


if __name__ == "__main__":
    try:
        main()
    except (KeyError, OSError, ValueError, subprocess.TimeoutExpired) as error:
        print("native C5 admission refused: " + str(error), file=sys.stderr)
        sys.exit(2)
