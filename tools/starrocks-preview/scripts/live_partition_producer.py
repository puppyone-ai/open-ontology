#!/usr/bin/env python3
"""Fail-closed StarRocks C4 occurrence against a sealed source partition read."""
import argparse
import csv
import fcntl
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def save(path, value):
    with path.open("w") as out:
        json.dump(value, out, sort_keys=True, indent=2)
        out.write("\n")
        out.flush()
        os.fsync(out.fileno())


def run(root, name, argv):
    result = subprocess.run(argv, capture_output=True, timeout=120)
    (root / (name + ".stdout")).write_bytes(result.stdout)
    (root / (name + ".stderr")).write_bytes(result.stderr)
    (root / (name + ".exit")).write_text(str(result.returncode) + "\n")
    if result.returncode:
        raise RuntimeError(name + " source read failed")
    return result.stdout


def metadata_matches(raw, frozen):
    meta = json.loads(raw)
    if meta.get("protocol") != "oo.starrocks.partition-meta/v1" or any(meta.get("pending", {}).values()):
        return False
    if meta.get("ddl") != frozen["ddl"] or meta.get("create_time") != frozen["create_time"]:
        return False
    if sorted(meta.get("partitions", []), key=lambda x: x["PartitionName"]) != frozen["partitions"]:
        return False
    return True


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--config-sha256", required=True)
    args = parser.parse_args()
    if digest(args.config) != args.config_sha256:
        raise RuntimeError("producer config changed")
    cfg = json.loads(args.config.read_text())
    event = json.load(sys.stdin)
    if event.get("protocol") != "oo.jobs.native-producer/v1" or event.get("method") != "C4" or event.get("source_id") != cfg["source_id"] or event.get("binding") != cfg["binding"]:
        raise RuntimeError("C4 occurrence identity mismatch")
    pin = event.get("pin")
    if pin not in (1, 2):
        raise RuntimeError("unexpected partition cycle")
    name = ("p202605", "p202606")[pin - 1]
    source = Path(cfg["frozen_source"])
    frozen_hashes = json.loads((source / "source-input-sha256.json").read_text())
    if digest(source / "source-input-sha256.json") != cfg["source_input_sha256"]:
        raise RuntimeError("source hash closure changed")
    for rel, expected in cfg["frozen_sha256"].items():
        if digest(source / rel) != expected:
            raise RuntimeError("frozen source observation changed: " + rel)
    for rel, expected in frozen_hashes.items():
        if digest(source / rel) != expected:
            raise RuntimeError("source witness changed: " + rel)
    for key in ("probe", "metadata_probe"):
        if digest(Path(cfg[key]["path"])) != cfg[key]["sha256"]:
            raise RuntimeError(key + " binary changed")
    sealed = Path(cfg["sealed"][name])
    receipt_path = sealed / "admission-receipt.json"
    registry_path = sealed / "registry.json"
    if digest(receipt_path) != cfg["sealed_sha256"][name]["receipt"] or digest(registry_path) != cfg["sealed_sha256"][name]["registry"]:
        raise RuntimeError("sealed admission changed")
    witness = json.loads((source / "sealed-inputs" / name / "witness.json").read_text())
    scope = json.loads((source / "sealed-inputs" / name / "capturer.json").read_text())["partition_scope"]
    if scope["source_id"] != cfg["source_id"] or scope["source_generation"] != pin + 3 or scope["partition_id"] != name or scope["observed_partition_ids"] != ["p202605", "p202606"] or scope["mutations_pending"] is not False:
        raise RuntimeError("frozen partition scope mismatch")
    frozen_arrow = Path(witness["arrow_path"])
    if digest(frozen_arrow) != scope["arrow_sha256"]:
        raise RuntimeError("frozen Arrow changed")
    tsv = source / "partitions-before.stdout"
    with tsv.open() as handle:
        frozen_rows = list(csv.DictReader(handle, delimiter="\t"))
    columns = ("PartitionId", "PartitionName", "VisibleVersion", "State", "PartitionKey", "Range", "RowCount", "DataVersion", "VersionEpoch")
    frozen = {"ddl": (source / "ddl.stdout").read_text().split("\n", 1)[1].split("\t", 1)[1].rstrip("\n"),
              "create_time": json.loads((source / "actual-metadata.json").read_text())["create_time"],
              "partitions": sorted([{key: row[key] for key in columns} for row in frozen_rows], key=lambda x: x["PartitionName"])}
    root = Path(cfg["evidence_root"]) / cfg["binding"] / str(pin)
    root.mkdir(parents=True, exist_ok=True)
    with (root / "producer.lock").open("w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        request_path = root / "request.json"
        if request_path.exists():
            sys.stdout.write(request_path.read_text())
            return
        marker = root / "capture-started.json"
        if marker.exists():
            raise RuntimeError("partial C4 occurrence requires recovery")
        save(marker, event)
        base = ["--host", cfg["source"]["host"], "--password-file", cfg["source"]["password_file"]]
        before = run(root, "metadata-before", [cfg["metadata_probe"]["path"], *base])
        if not metadata_matches(before, frozen):
            raise RuntimeError("source partition metadata drift before read")
        read_dir = root / "fresh-read"
        run(root, "partition-read", [cfg["probe"]["path"], "partition-read", "--host", cfg["source"]["host"], "--port", "9030", "--password-file", cfg["source"]["password_file"], "--image-digest", cfg["source"]["image_digest"], "--max-rows", "1000", "--deadline-ms", "60000", "--partition", name, "--out", str(read_dir)])
        after = run(root, "metadata-after", [cfg["metadata_probe"]["path"], *base])
        if not metadata_matches(after, frozen) or before != after:
            raise RuntimeError("source partition metadata drift after read")
        ready = json.loads((read_dir / "ready.json").read_text())
        if ready.get("ready") is not True or ready.get("partition") != name or ready.get("tables") != [{"name": "dup_events", "rows": scope["row_count"], "ipc_bytes": frozen_arrow.stat().st_size}]:
            raise RuntimeError("fresh partition read shape differs")
        audit = json.loads((read_dir / "audit-counts.json").read_text())
        if audit.get("write") != 0 or audit.get("unexpected") != 0:
            raise RuntimeError("source reader attempted a write")
        if digest(read_dir / "dup_events.arrow") != scope["arrow_sha256"]:
            raise RuntimeError("fresh Arrow differs from sealed source Arrow")
        receipt = json.loads(receipt_path.read_text())
        if receipt["source_id"] != cfg["source_id"] or receipt["source_generation"] != scope["source_generation"] or receipt["arrow_sha256"] != [scope["arrow_sha256"]]:
            raise RuntimeError("sealed admission and source scope differ")
        columns = json.loads((source / "sealed-inputs" / name / "schema.json").read_text())[0]["columns"]
        generation = scope["source_generation"]
        request = {"protocol": "oo.pipeline.native-partition/v1", "source_engine": "starrocks", "source_id": cfg["source_id"], "source_generation": generation, "parent_source_generation": generation - 1,
                   "logical_schema": "oo_w45_sr", "logical_table": "dup_events", "snapshot": receipt["snapshot"], "admission_path": str(receipt_path), "registry_path": str(registry_path),
                   "admission_expect": {"source_engine": "starrocks", "source_id": cfg["source_id"], "source_generation": generation,
                                        "declared_receipt_sha256": digest(receipt_path)},
                   "arrow_path": str(frozen_arrow), "audit_digest": digest(source / "sealed-inputs" / name / "capturer.json"), "batch_id": f"{cfg['binding']}_{generation}",
                   "capture_method": "c4_partition", "consistency_grade": receipt["consistency"], "cross_table_consistent": False,
                   "delete_detection": "partition_replace", "full_snapshot": False, "row_identity": "duplicate", "key_columns": ["bucket"], "upsert_supported": False,
                   "window_closed": True, "position_codec": "partition-signature/v1", "position_lineage": "native-update.batch", "applied_pos": "0", "columns": columns,
                   "tombstones": [], "mirror": False, "quality": ["OUT_OF_PARTITION_DELETE_NOT_EXTRAPOLATED"], "target": cfg["target"]}
        for key in ("run_id", "catalog_sha256", "registry_sha256", "driver_digest", "arrow_schema_sha256",
                    "config_digest", "schema_digest", "type_digest", "input_digest"):
            request[key] = receipt[key]
        save(root / "request.tmp", request)
        os.replace(root / "request.tmp", request_path)
        sys.stdout.write(request_path.read_text())


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        print("starrocks-partition-producer: " + str(exc), file=sys.stderr)
        raise SystemExit(1)
