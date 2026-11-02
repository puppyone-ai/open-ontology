#!/usr/bin/env python3
"""Pinned Oracle snapshot adapter for the existing native C5 worker.

Consumes a closed source-owned snapshot bundle offline. No source SQL, target
DDL, lease creation or ACK: official codec, admission and Loader own those.
"""
from pathlib import Path
import argparse
import hashlib
import json
import os
import subprocess
import sys


def canonical(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def read_pin(ref, limit=32 * 1024 * 1024):
    path = Path(ref["path"])
    if not path.is_absolute() or ".." in path.parts or not path.is_file():
        raise ValueError("snapshot_artifact")
    if not 0 < path.stat().st_size <= limit:
        raise ValueError("snapshot_artifact_budget")
    data = path.read_bytes()
    if hashlib.sha256(data).hexdigest() != ref["sha256"] or ref.get("bytes", len(data)) != len(data):
        raise ValueError("snapshot_artifact_digest")
    return data


def document(ref, limit=32 * 1024 * 1024):
    return json.loads(read_pin(ref, limit))


def put(path, value):
    raw = canonical(value)
    with path.open("xb") as f:
        os.chmod(path, 0o600)
        f.write(raw)
        f.flush()
        os.fsync(f.fileno())
    return dict(path=str(path), sha256=hashlib.sha256(raw).hexdigest(), bytes=len(raw))


def invoke(binary, argv, body, root, name):
    read_pin(binary, 128 * 1024 * 1024)
    p = subprocess.run([binary["path"], *argv], input=body, capture_output=True,
                       env={"PATH": "/usr/bin:/bin"}, timeout=120)
    (root / (name + ".stdout")).write_bytes(p.stdout)
    (root / (name + ".stderr")).write_bytes(p.stderr)
    put(root / (name + ".command.json"), dict(argv=[binary["path"], *argv], exit=p.returncode))
    if p.returncode:
        raise ValueError(name + "_refused")
    return json.loads(p.stdout)


def produce(config, incoming, out):
    if config["protocol"] != "oo.pipeline.oracle-snapshot-operator/v1":
        raise ValueError("snapshot_operator")
    if sha(__file__) != config["materializer_sha256"]:
        raise ValueError("snapshot_materializer_pin")
    bundle = document(config["bundle"])
    source = bundle["source_id"]
    if (bundle["protocol"] != "oo.oracle.producer-bundle/v1" or bundle["engine"] != "oracle"
            or bundle["boundary"]["mode"] != "sealed_snapshot"
            or bundle["boundary"]["scope_complete"] is not True or len(bundle["typed_tables"]) != 1):
        raise ValueError("snapshot_bundle")
    if (incoming["protocol"] != "oo.jobs.native-producer/v1" or incoming["method"] != "C5"
            or incoming["source_id"] != source or incoming["binding"] != config["binding"]
            or type(incoming["pin"]) is not int or incoming["pin"] < 1
            or type(incoming["acknowledged_source_generation"]) is not int):
        raise ValueError("snapshot_occurrence")
    generation = incoming["acknowledged_source_generation"] + 1
    if generation != config["source_generation"]:
        raise ValueError("snapshot_generation")
    root = Path(out).resolve() / incoming["binding"] / str(incoming["pin"])
    if root.exists():
        if json.loads((root / "input.json").read_text()) != incoming:
            raise ValueError("snapshot_replay_conflict")
        return json.loads((root / "request.json").read_text())
    root.mkdir(mode=0o700, parents=True, exist_ok=False)
    put(root / "input.json", incoming)
    bundle_root = Path(config["bundle"]["path"]).parent
    for name, digest in bundle["artifacts"].items():
        path = bundle_root / name
        if not path.resolve().is_relative_to(bundle_root.resolve()):
            raise ValueError("snapshot_artifact_path")
        read_pin(dict(path=str(path), sha256=digest))
    frames = [json.loads(line) for line in (bundle_root / "batches.jsonl").read_text().splitlines()]
    qualified, spec = next(iter(bundle["typed_tables"].items()))
    schema, table = qualified.split(".", 1)
    if any(f["kind"] != "typed_batch" or f["batch"]["kind"] != "fill"
           or any(p["table"] != qualified or p["spec"] != spec for p in f["batch"]["parts"]) for f in frames):
        raise ValueError("snapshot_frame")
    parts = [dict(path=str(bundle_root / name), sha256=h) for name, h in sorted(bundle["artifacts"].items())
             if name.startswith("inputs/") and name.endswith(".json")]
    if not parts or not 0 <= bundle["final_rows"] <= 100000:
        raise ValueError("snapshot_rows")
    codec = config["source_driver"]
    if codec["sha256"] != bundle["encoder_sha256"]:
        raise ValueError("snapshot_codec")
    arrow = root / "snapshot.arrow"
    encoded = invoke(codec, [], canonical(dict(action="encode-series", inputs=parts, output=str(arrow))), root, "codec")
    if encoded["typed_spec"] != spec or encoded["rows"] != bundle["final_rows"] or encoded["cdc"]:
        raise ValueError("snapshot_codec_shape")
    declarations = encoded["columns"]
    types = []
    for index, column in enumerate(declarations):
        column.pop("logical_name", None)
        column["key"] = index in spec["keys"]
        typed = {k: v for k, v in column.items() if k not in ("key", "nullable")}
        typed.update(table=table, source_type=bundle["source_mappings"][qualified][index]["source_column"]["type"],
                     target_oid=spec["columns"][index]["oid"], target_typmod=spec["columns"][index]["typmod"], caveat=None)
        types.append(typed)
    put(root / "schema-preimage.json", [dict(table=table, columns=declarations)])
    put(root / "type-preimage.json", types)
    put(root / "admission-config.json", config["admission_config"])
    snapshot = dict(kind="oracle_scn", value=bundle["boundary"]["snapshot_scn"])
    capturer = dict(protocol="oo.oracle.c5-producer-catalog/v1", source_engine="oracle", source_id=source,
        source_generation=generation, run_id=config["target"]["owner"], snapshot=snapshot,
        product_reader_write_attempts=0, source_server_contacted=False, snapshot_token=False,
        cross_table_consistency="S0", captured_snapshot=config["bundle"], tables=[dict(
            schema=schema, logical_schema=schema, name=table, row_count=encoded["rows"], arrow_sha256=sha(arrow),
            route="C5", consistency="S0", delete_mode="reconcile", cursor_safety="no_cursor",
            quality_rejects=[])])
    put(root / "capturer.json", capturer)
    put(root / "witness.json", dict(protocol="oo.source-admission.capture-witness/v1", source_engine="oracle",
        source_id=source, source_generation=generation, snapshot=snapshot, run_id=config["target"]["owner"],
        logical_schema=schema, logical_table=table, capturer_path=str(root / "capturer.json"), arrow_path=str(arrow),
        driver_path=codec["path"], schema_preimage_path=str(root / "schema-preimage.json"),
        type_preimage_path=str(root / "type-preimage.json")))
    report = dict(config["admission_report"], capture_witness=str(root / "witness.json"))
    put(root / "report.json", report)
    result = invoke(config["admission"], ["seal-native", "--report", str(root / "report.json"),
        "--config", str(root / "admission-config.json"), "--out", str(root / "sealed")], None, root, "admission")
    if result["admitted"] is not True:
        raise ValueError("snapshot_admission")
    registry = json.loads((root / "sealed/registry.json").read_text())
    b = registry["bindings"][0]
    if b["source_engine"] != "oracle" or b["source_id"] != source or b["source_generation"] != generation:
        raise ValueError("snapshot_admission_identity")
    request = dict(protocol="oo.pipeline.native-update/v1", source_engine="oracle", source_id=source,
        source_generation=generation, parent_source_generation=None if generation == 1 else generation - 1,
        full_snapshot=True, snapshot=b["snapshot"], batch_id=f'{incoming["binding"]}-{incoming["pin"]}',
        capture_method="c5_diff", delete_detection="reconcile", consistency_grade="S0", cross_table_consistent=False,
        position_codec="snapshot-zero/v1", position_lineage="oracle.snapshot", applied_pos="0",
        logical_schema=schema, logical_table=table, columns=[dict(f, logical_name=f["name"]) for f in b["fields"]],
        key_columns=[f["name"] for f in b["fields"] if f["key"]], arrow_path=str(arrow), tombstones=[], mirror=False,
        audit_digest=b["capture_receipt_sha256"], admission_path=b["admission_receipt_path"], target=config["target"],
        **{k: b[k] for k in ("config_digest", "schema_digest", "type_digest", "input_digest")})
    put(root / "request.json", request)
    return request


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    config = json.loads(args.config.read_bytes())
    incoming = json.loads(sys.stdin.buffer.read(1024 * 1024 + 1))
    print(canonical(produce(config, incoming, args.out)).decode())


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, KeyError, subprocess.TimeoutExpired) as error:
        print(json.dumps({"error": str(error) if isinstance(error, ValueError) else type(error).__name__,
                          "source_mutations": False}), file=sys.stderr)
        raise SystemExit(2)
