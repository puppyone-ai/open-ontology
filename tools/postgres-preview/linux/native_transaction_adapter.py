#!/usr/bin/env python3
"""Bind one actual PG transaction's pages to four independent admission seals.

The emitted producer document is an immutable worker input, not a job claim or
target ACK. The worker supplies its own lease and pins the registry vector.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess


def read(path):
    return json.loads(path.read_bytes())


def sha(data):
    return hashlib.sha256(data).hexdigest()


def target_watermark():
    raw = subprocess.check_output([
        "docker", "exec", "oo-dl01w1-46-native", "psql", "-U", "postgres",
        "-d", "dst_native_r2", "-X", "-A", "-t", "-F", "|", "-c",
        "SELECT source_generation, applied_pos FROM _oo.source_watermark "
        "WHERE source='oo_dl01w1_46'",
    ], text=True).strip().splitlines()
    if len(raw) != 1:
        raise ValueError("target watermark cardinality")
    generation, position = raw[0].split("|")
    return int(generation), int(position)


def latest_scope(root):
    captures = sorted((root / "native-admit").glob("*/*.capture.json"))
    scopes = {}
    by_lsn = {}
    for path in captures:
        scope = read(path).get("transaction_scope")
        if scope is None or scope.get("complete") is not True:
            continue
        key = (scope["end_lsn"], scope["xid"])
        if key in scopes and scopes[key] != scope:
            raise ValueError("conflicting transaction scopes")
        if scope["end_lsn"] in by_lsn and by_lsn[scope["end_lsn"]] != scope:
            raise ValueError("conflicting transaction scopes")
        scopes[key] = scope
        by_lsn[scope["end_lsn"]] = scope
    if not scopes:
        raise ValueError("complete transaction scope required")
    def position(scope):
        high, low = scope["end_lsn"].split("/")
        return (int(high, 16) << 32) | int(low, 16)
    return max(scopes.values(), key=position)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--binding", required=True)
    parser.add_argument("--pin", required=True, type=int)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    if not re.fullmatch(r"[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}", args.binding):
        raise ValueError("binding UUID")
    if args.pin != 1 or args.out.exists():
        raise ValueError("first immutable group pin required")
    scope = latest_scope(args.root)
    scoped_tables = scope["tables"]
    actual_tables = [(item["logical_schema"], item["logical_table"]) for item in scoped_tables]
    if (not actual_tables or len(actual_tables) != len(set(actual_tables))
            or not set(actual_tables) <= {
                ("customer_east", "delivery_notes"), ("public", "hubs"),
                ("public", "shipments"), ("public", "tracking_events")}):
        raise ValueError("transaction table set")
    entries = []
    for scoped in scoped_tables:
        table = scoped["logical_schema"] + "." + scoped["logical_table"]
        generation = int(scoped["source_generation"])
        seal = args.root / "sealed-current" / table / str(generation)
        if not seal.is_dir():
            raise ValueError(f"missing admitted seal: {table}/{generation}")
        source = args.root / "native-admit" / table / f"g{generation}"
        captured = read(Path(str(source) + ".capture.json"))
        request = read(Path(str(source) + ".request.json"))
        decision = read(seal / "decision.json")
        registry = read(seal / "registry.json")
        if decision["admitted"] is not True or decision["blocks"] or len(registry["bindings"]) != 1:
            raise ValueError(f"not independently admitted: {table}")
        bound = registry["bindings"][0]
        if captured["transaction_scope"] != scope or bound["transaction_scope"] != scope:
            raise ValueError("transaction scope mismatch")
        if captured["cross_table_consistency"] != "T" or request["consistency_grade"] != "T" or request["cross_table_consistent"] is not True:
            raise ValueError("T grade")
        if (request["logical_schema"] + "." + request["logical_table"] != table
                or request["source_generation"] != generation
                or bound["source_generation"] != generation
                or request["source_id"] != bound["source_id"]
                or request["snapshot"] != bound["snapshot"]
                or request["input_digest"] != bound["input_digest"]):
            raise ValueError("captured/sealed identity")
        if sha(Path(str(source) + ".arrow").read_bytes()) != bound["input_digest"]:
            raise ValueError("Arrow digest")
        by_name = {field["name"]: field for field in bound["fields"]}
        for column in request["columns"]:
            field = by_name.pop(column["name"])
            for name in ("source_oid", "source_typmod", "source_format"):
                column[name] = field[name]
        if by_name:
            raise ValueError("missing source columns")
        for name in ("schema_digest", "type_digest", "config_digest"):
            request[name] = bound[name]
        request["audit_digest"] = bound["capture_receipt_sha256"]
        request["admission_path"] = bound["admission_receipt_path"]
        request["admission_expect"] = {"declared_receipt_sha256": bound["admission_receipt_sha256"]}
        request["run_id"] = bound["run_id"]
        request["max_arrow_bytes"] = 16 * 1024 * 1024
        request["target"]["ca_file"] = "/work/ca-renewed.pem"
        entries.append(request)
    if scope["complete"] is not True or len(scope["tables"]) != len(entries):
        raise ValueError("incomplete transaction")
    if [(item["logical_schema"], item["logical_table"]) for item in entries] != actual_tables:
        raise ValueError("transaction table set")
    prior_generation, prior_position = target_watermark()
    group_generation = prior_generation + 1
    high, low = scope["end_lsn"].split("/")
    end_lsn = (int(high, 16) << 32) | int(low, 16)
    if group_generation != 4 or prior_position >= end_lsn:
        raise ValueError("target generation or LSN")
    for request, scoped in zip(entries, scope["tables"]):
        if (request["source_generation"] != scoped["source_generation"]
                or request["parent_source_generation"] != scoped["parent_source_generation"]
                or request["applied_pos"] != str(end_lsn)
                or request["input_digest"] != scoped["arrow_sha256"]
                or sha(json.dumps(request["tombstones"], separators=(",", ":"), ensure_ascii=False).encode()) != scoped["tombstones_sha256"]):
            raise ValueError("per-table source facts")
    body = {"protocol": "oo.jobs.native-transaction-producer/v1", "binding": args.binding,
            "pin": args.pin, "source_id": scope["source_id"], "capture_method": "c1_cdc",
            "source_generation": group_generation, "transaction_scope": scope, "entries": entries}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(body, separators=(",", ":"), sort_keys=True) + "\n")
    print(f"{args.out} {sha(args.out.read_bytes())}")


if __name__ == "__main__":
    main()
