#!/usr/bin/env python3
"""Seal immutable Oracle integer ranges for the existing native C5 worker."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def read(ref):
    path = Path(ref["path"])
    if not path.is_absolute() or ".." in path.parts or path.is_symlink():
        raise ValueError("range_artifact_path")
    if path.stat().st_size > 128 * 1024 * 1024 or sha(path) != ref["sha256"]:
        raise ValueError("range_artifact_digest")
    return path


def put(path, value):
    with path.open("x") as stream:
        os.chmod(path, 0o600)
        json.dump(value, stream, sort_keys=True, separators=(",", ":"))
        stream.flush()
        os.fsync(stream.fileno())


def produce(config, incoming):
    if config["protocol"] != "oo.oracle.c5-range-operator/v1":
        raise ValueError("range_operator")
    for ref in config["code_pins"]:
        read(ref)
    snapshot_path = read(config["snapshot"])
    snapshot_raw = snapshot_path.read_text()
    snapshot = json.loads(snapshot_raw)
    source = snapshot["binding"]["source_id"]
    if not snapshot["complete"] or not snapshot["scope_complete"]:
        raise ValueError("range_snapshot_incomplete")
    if (incoming["protocol"] != "oo.jobs.native-producer/v1" or incoming["method"] != "C5"
            or incoming["binding"] != config["binding"] or incoming["source_id"] != source
            or type(incoming["pin"]) is not int or incoming["pin"] < config["first_pin"]
            or type(incoming["acknowledged_source_generation"]) is not int):
        raise ValueError("range_occurrence")
    plan = json.loads(read(config["range_plan"]).read_bytes())
    if (plan["source_id"] != source or plan["source_engine"] != "oracle"
            or plan["current_snapshot_sha256"] != config["snapshot"]["sha256"]):
        raise ValueError("range_plan_snapshot")
    changes = [c for c in plan["chunks"] if c["changed"]]
    ordinal = incoming["pin"] - config["first_pin"]
    if ordinal >= len(changes):
        raise ValueError("range_cycle_complete")
    parent = incoming["acknowledged_source_generation"]
    if parent != config["parent_generation"] + ordinal:
        raise ValueError("range_parent_generation")
    chunk = changes[ordinal]
    arrow = read(chunk["arrow"])
    generation = parent + 1
    root = Path(config["out"]) / incoming["binding"] / str(incoming["pin"])
    if root.exists():
        if json.loads((root / "input.json").read_text()) != incoming:
            raise ValueError("range_replay_conflict")
        request = json.loads((root / "request.json").read_text())
        if request["input_digest"] != sha(arrow):
            raise ValueError("range_replay_digest")
        return request
    root.mkdir(mode=0o700, parents=True, exist_ok=False)
    put(root / "input.json", incoming)
    seed = json.loads(read(config["seed_request"]).read_bytes())
    oracle = dict(identity=snapshot["binding"]["identity"], scn=snapshot["binding"]["scn"],
        manifest_sha256=config["snapshot"]["sha256"],
        source_schema=seed["logical_schema"], source_table=seed["logical_table"])
    input_rows = json.loads(read(config["chunk_inputs"][chunk["chunk_id"]]).read_bytes())["rows"]
    if len(input_rows) != chunk["rows"] or any(len(r) != len(seed["columns"]) for r in input_rows):
        raise ValueError("range_rows")
    lo = chunk["lo_exclusive"]
    hi = None if chunk["chunk_id"] == plan["chunks"][-1]["chunk_id"] else chunk["hi_inclusive"]
    scope = dict(protocol="oo.pipeline.native-c5-range/v1", source_engine="oracle", source_id=source,
        capture_method="c5_diff", schema=seed["logical_schema"], table=seed["logical_table"],
        schema_fingerprint=seed["schema_digest"], chunk_topology_revision=1, chunk_id=chunk["chunk_id"],
        lo=None if lo == 0 else [dict(kind="int", value=lo)],
        hi=None if hi is None else [dict(kind="int", value=hi)],
        prior_membership_revision=1, parent_generation=parent, source_generation=generation,
        arrow_sha256=sha(arrow), row_count=chunk["rows"], delete_detection="reconcile",
        position_codec="snapshot-zero/v1", applied_pos="0", oracle_snapshot=oracle,
        members=[[dict(kind="int", value=int(row[0]))] for row in input_rows])
    files = {name: json.loads(read(ref).read_bytes()) for name, ref in config["seed_files"].items()}
    capturer = files["capturer"]
    capturer.update(source_generation=generation, snapshot=dict(kind="oracle_scn", value=oracle["scn"]),
        range_scope=scope, oracle_snapshot_manifest=snapshot_raw,
        captured_snapshot=config["snapshot"])
    capturer["tables"][0].update(row_count=chunk["rows"], arrow_sha256=sha(arrow))
    put(root / "capturer.json", capturer)
    witness = files["witness"]
    witness.update(source_generation=generation, snapshot=capturer["snapshot"],
                   capturer_path=str(root / "capturer.json"), arrow_path=str(arrow))
    put(root / "witness.json", witness)
    report = files["report"]
    report["capture_witness"] = str(root / "witness.json")
    put(root / "report.json", report)
    admission = read(config["admission"])
    argv = [str(admission), "seal-native", "--report", str(root / "report.json"),
            "--config", str(read(config["admission_config"])), "--out", str(root / "sealed")]
    result = subprocess.run(argv, capture_output=True, timeout=120)
    (root / "admission.stdout").write_bytes(result.stdout)
    (root / "admission.stderr").write_bytes(result.stderr)
    put(root / "admission-command.json", dict(argv=argv, exit=result.returncode))
    if result.returncode or json.loads(result.stdout)["admitted"] is not True:
        raise ValueError("range_admission_refused")
    registry = json.loads((root / "sealed/registry.json").read_text())
    binding = registry["bindings"][0]
    if binding["range_scope"] != scope or binding["source_engine"] != "oracle":
        raise ValueError("range_admission_scope")
    native = dict(seed, source_generation=generation, parent_source_generation=parent,
        full_snapshot=False, snapshot=capturer["snapshot"],
        batch_id=f'{incoming["binding"]}-{incoming["pin"]}', arrow_path=str(arrow),
        admission_path=binding["admission_receipt_path"], audit_digest=binding["capture_receipt_sha256"])
    native.update({k: binding[k] for k in ("config_digest","schema_digest","type_digest","input_digest")})
    native["columns"] = [dict((k, v) for k, v in col.items() if k != "key") for col in native["columns"]]
    request = dict(scope, native_request=native, target=native["target"],
        input_digest=native["input_digest"], arrow_path=str(arrow),
        consistency_grade="S0", cross_table_consistent=False, key_columns=native["key_columns"],
        binding=incoming["binding"], pin=incoming["pin"], c5_completed_layers=[False]*4)
    put(root / "request.json", request)
    return request


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path, required=True)
    args = parser.parse_args()
    result = produce(json.loads(args.config.read_bytes()), json.loads(sys.stdin.buffer.read(1024*1024+1)))
    print(json.dumps(result, sort_keys=True, separators=(",", ":")))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, KeyError, IndexError, subprocess.TimeoutExpired) as error:
        print(json.dumps(dict(error=str(error), source_connected=False)), file=sys.stderr)
        raise SystemExit(2)
