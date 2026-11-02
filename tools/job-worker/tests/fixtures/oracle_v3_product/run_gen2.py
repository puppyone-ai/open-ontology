#!/usr/bin/env python3
"""Run the fixed Oracle suffix on its EXISTING C1 target, only after root readiness.

No source connections, IUD, capture restart, target creation, migration installation,
or direct jobs writes. This is a worker integration fixture, not an independent audit.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import uuid


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def document(path):
    return json.loads(Path(path).read_text())


def pinned(ref):
    path = Path(ref["path"])
    assert path.is_absolute() and sha(path) == ref["sha256"], "artifact pin"
    if "bytes" in ref:
        assert path.stat().st_size == ref["bytes"], "artifact size"
    return path


def save(path, value):
    with path.open("x") as out:
        out.write(json.dumps(value, indent=2) + "\n")
    path.chmod(0o600)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence-root", type=Path, required=True)
    parser.add_argument("--worker-pin", type=Path, required=True)
    parser.add_argument("--target-ready", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--execute", action="store_true")
    args = parser.parse_args()
    root, out = args.evidence_root.resolve(), args.out.resolve()
    baseline = document(root / "mysql-integration/oracle-frozen-phase2/DUAL-BASELINE-READY.json")["targets"]["C1"]
    contract_path = root / "oracle-integration/frozen-phase2-source/IMPLEMENTED-CONTRACT.json"
    contract = document(contract_path)
    source = contract["actual_previous"]["source_id"]
    ready = document(args.target_ready)
    assert ready["protocol"] == "oo.oracle.gen2-target-ready/v1"
    assert ready["source_id"] == source == "oracle_frozen_iud_20261027"
    assert ready["target_database"] == baseline["database"]
    assert ready["source_contract_sha256"] == sha(contract_path)
    assert ready["stage_installed"] is True and ready["root_authorizes_worker_integration"] is True
    pipeline = ready["pipeline"]
    pipeline_path = pinned(pipeline)
    worker = document(args.worker_pin)
    worker_path = pinned(worker)
    worker_config = pinned(baseline["worker_config"])
    old_manifest = document(pinned(baseline["native_manifest"]))
    producer = {k: old_manifest["producer"][k] for k in ("path", "sha256")}
    pinned(producer)
    registration = old_manifest["registration"].copy()
    if "admission" in ready:
        registration["admission_binary"] = str(pinned(ready["admission"]))
        registration["admission_sha256"] = ready["admission"]["sha256"]
    else:
        pinned({"path": registration["admission_binary"], "sha256": registration["admission_sha256"]})
    config_path = root / "oracle-integration/frozen-phase2-source/ready/producer-config.json"
    delivery = document(root / "oracle-integration/frozen-phase2-source/DELIVERY.json")
    assert pinned(delivery["C1"]["producer_config"]) == config_path
    config = document(config_path)
    assert config["rollover"] == contract["rollover"] and config["buffer_index"] == contract["buffer_index"]
    for ref in [config["rollover"], config["buffer_index"], config["bundle"], contract["actual_previous"]["parent_ack"]]:
        pinned(ref)
    assert pipeline["sha256"] != config["materializer"]["sha256"], "root must supply a gen2 pipeline"
    out.mkdir(mode=0o700, parents=True, exist_ok=False)
    config["materializer"] = dict(path=str(pipeline_path), sha256=pipeline["sha256"],
                                  args=["native-oracle-c1-materialize"])
    save(out / "producer-config.json", config)
    binding = str(uuid.uuid4())
    manifest = dict(old_manifest, binding=binding, source_id=source)
    manifest["producer"] = dict(producer, args=["c1-baseline-stream-v3", "--config",
        str(out / "producer-config.json"), "--out", str(out / "capture-{task}-{generation}")])
    manifest["loader"] = dict(path=str(pipeline_path), sha256=pipeline["sha256"],
                              args=["import-native-c1-baseline-v3"])
    manifest["registration"] = registration
    save(out / "native-manifest.json", manifest)
    commands = []

    def run(name, *argv, check=True):
        result = subprocess.run([str(worker_path), "--config", str(worker_config), *map(str, argv)],
                                capture_output=True, check=False)
        for suffix, data in [("stdout", result.stdout), ("stderr", result.stderr)]:
            path = out / (name + "." + suffix)
            with path.open("xb") as stream:
                stream.write(data)
            path.chmod(0o600)
        commands.append(dict(name=name, argv=list(map(str, argv)), exit_code=result.returncode))
        if check and result.returncode:
            raise RuntimeError(name + " failed; original evidence retained")
        return result.returncode

    save(out / "inputs.json", dict(worker=worker, pipeline=pipeline, baseline=baseline,
        source_contract_sha256=sha(contract_path), binding=binding, source_mutation=False,
        target_integration="requested" if args.execute else "not_run"))
    if not args.execute:
        print("prepared only; target_integration=not_run")
        return
    registered = False
    try:
        run("native-put", "native-put", "--manifest", out / "native-manifest.json")
        registered = True
        run("native-fire", "native-fire", "--project", manifest["project_id"])
        run("native-step", "native-step", "--project", manifest["project_id"], "--worker", "oracle-gen2-worker")
        run("native-state", "native-state", "--binding", binding)
        run("native-artifacts", "native-artifacts", "--binding", binding, "--out", out / "native-artifacts")
    finally:
        if registered:
            run("native-disable", "native-enable", "--binding", binding, "--enabled", "false", check=False)
        save(out / "commands.json", commands)
    print("worker commands completed; independent 12000-row publication audit remains a root gate")


if __name__ == "__main__":
    main()
