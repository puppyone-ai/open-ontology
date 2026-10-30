#!/usr/bin/env python3
"""Compare pinned Oracle snapshots for a real C5 worker occurrence, offline.

This proves retained-snapshot reconciliation, never a fresh live-source check.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sys


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def pinned(ref):
    path = Path(ref["path"])
    if not path.is_absolute() or ".." in path.parts or path.is_symlink():
        raise ValueError("artifact_path")
    if not 0 < path.stat().st_size <= 32 * 1024 * 1024:
        raise ValueError("artifact_budget")
    if digest(path) != ref["sha256"]:
        raise ValueError("artifact_digest")
    return json.loads(path.read_bytes())


def snapshot(ref):
    manifest = pinned(ref)
    if (manifest.get("protocol") != "oo.oracle.sealed-snapshot/v1"
            or manifest.get("complete") is not True
            or manifest.get("scope_complete") is not True):
        raise ValueError("snapshot_incomplete")
    root = Path(ref["path"]).parent
    if digest(root / "journal.sqlite") != manifest["journal_sha256"]:
        raise ValueError("journal_digest")
    counts = 0
    for part in manifest["parts"]:
        if Path(part["file"]).name != part["file"]:
            raise ValueError("part_path")
        body = pinned(dict(path=str(root / part["file"]), sha256=part["sha256"]))
        if len(body["rows"]) != part["rows"]:
            raise ValueError("part_rows")
        counts += part["rows"]
    if counts != manifest["rows"]:
        raise ValueError("snapshot_rows")
    return manifest


def produce(config, incoming):
    if config["protocol"] != "oo.oracle.c5-sealed-nochange/v1":
        raise ValueError("config_protocol")
    parent_request = pinned(config["parent_request"])
    before, after = snapshot(config["parent_snapshot"]), snapshot(config["current_snapshot"])
    for field in ("source_id", "identity", "inventory", "exclusions", "scope"):
        if before["binding"][field] != after["binding"][field]:
            raise ValueError("snapshot_binding")
    if (int(after["binding"]["scn"]) < int(before["binding"]["scn"])
            or before["content_sha256"] != after["content_sha256"]
            or before["rows"] != after["rows"]):
        raise ValueError("snapshot_changed")
    source = after["binding"]["source_id"]
    if (parent_request["source_engine"] != "oracle" or parent_request["capture_method"] != "c5_diff"
            or parent_request["source_id"] != source
            or parent_request["snapshot"] != dict(kind="oracle_scn", value=before["binding"]["scn"])):
        raise ValueError("parent_binding")
    base = parent_request["source_generation"]
    if (incoming.get("protocol") != "oo.jobs.native-producer/v1"
            or incoming.get("method") != "C5" or incoming.get("source_id") != source
            or incoming.get("binding") != config["binding"]
            or type(incoming.get("pin")) is not int or incoming["pin"] < 1
            or type(incoming.get("acknowledged_source_generation")) is not int
            or incoming["acknowledged_source_generation"] != base):
        raise ValueError("occurrence_binding")
    content = after["content_sha256"]
    return dict(protocol="oo.jobs.native-c5-nochange/v1", binding=incoming["binding"],
        pin=incoming["pin"], source_id=source, method="C5", capture_method="c5_diff",
        acknowledged_source_generation=base, source_generation=base, empty_diff=True,
        snapshot=dict(kind="c5_cycle", value=content), source_digest=content,
        parent_digest=parent_request["input_digest"], data_gen=False, apply=False, ack=False,
        position_codec="snapshot-zero/v1", source_connected=False,
        observation_kind="retained_snapshot_reconciliation", source_time_advanced=False,
        retained_snapshot_scn=after["binding"]["scn"],
        current_manifest_sha256=config["current_snapshot"]["sha256"])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path, required=True)
    args = parser.parse_args()
    incoming = json.loads(sys.stdin.buffer.read(1024 * 1024 + 1))
    result = produce(json.loads(args.config.read_bytes()), incoming)
    print(json.dumps(result, sort_keys=True, separators=(",", ":")))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, KeyError) as error:
        print(json.dumps(dict(error=str(error), source_connected=False)), file=sys.stderr)
        raise SystemExit(2)
