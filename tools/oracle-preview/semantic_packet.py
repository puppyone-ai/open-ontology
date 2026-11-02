#!/usr/bin/env python3
"""Bind a genuine Oracle full catalog to its acknowledged target export/profile."""
import argparse
import json
from pathlib import Path
import sqlite3
import sys

import bounded_source as b
import buffer_cursor as c
import producer as p

IDENTITY = ("source_id", "source_generation", "target_generation", "run_id",
            "config_digest", "schema_digest", "type_digest", "input_digest")

def document(ref):
    return json.loads(p.pinned(ref, 32*1024*1024).read_text())

def cell(value, column):
    if not isinstance(value, dict) or set(value) != {"type", "value"}:
        raise ValueError("semantic_typed_cell")
    if value["type"] == "null":
        if value["value"] is not None or column["nullable"] != "Y":
            raise ValueError("semantic_typed_null")
        return None
    raw = value["value"]
    if not isinstance(raw, str):
        raise ValueError("semantic_typed_value")
    kind = column["type"]
    if kind == "NUMBER" and value["type"] in ("int", "decimal"):
        return p.canonical_number(raw)
    if kind == "RAW" and value["type"] == "bytes":
        if len(raw) % 2 or any(char not in "0123456789abcdefABCDEF" for char in raw):
            raise ValueError("semantic_typed_hex")
        return raw.lower()
    if kind in ("VARCHAR2", "NVARCHAR2", "CHAR", "NCHAR") and value["type"] == "text":
        return raw
    raise ValueError("semantic_target_type_contract_required")

def build(config, out):
    docs = {key: document(config[key]) for key in
            ("catalog", "source_schema", "materialize_input", "request", "load", "ack", "onboarding_receipt")}
    catalog, schema, material, request, loaded, ack = (
        docs[k] for k in ("catalog", "source_schema", "materialize_input", "request", "load", "ack"))
    source = config["source_id"]
    root_receipt = docs["onboarding_receipt"]
    if any(doc.get("source_id") != source for doc in (schema, request, loaded, ack)):
        raise ValueError("semantic_packet_source")
    if (material["engine"] != "oracle" or material["context"]["source_id"] != source
            or ack["source_engine"] != "oracle" or ack["acknowledged"] is not True
            or ack["task_state"] != "succeeded" or ack["request_digest"] != loaded["request_digest"]
            or ack["source_generation"] != loaded["source_generation"]
            or ack["target_generation"] != loaded["target_generation"]):
        raise ValueError("semantic_packet_ack")
    if (root_receipt["exit"] != 0 or root_receipt["source_id"] != source
            or root_receipt["target_ack_verified"] is not True
            or root_receipt["native_state"]["task_id"] != material["context"]["lease_id"]
            or root_receipt["native_state"]["source_generation"] != ack["source_generation"]):
        raise ValueError("semantic_packet_task")
    if (catalog.get("protocol") != "oo.pipeline.sealed-import-collection/v1"
            or catalog["collection_sha256"] != p.digest({k: v for k, v in catalog.items() if k != "collection_sha256"})):
        raise ValueError("semantic_packet_collection")
    exported = document(material["sealed_target"]["target_export"])
    if (exported["source_id"] != source or exported["source_engine"] != "oracle"
            or exported["task_id"] != material["context"]["lease_id"]
            or exported["lease_generation"] != root_receipt["native_state"]["lease_generation"]
            or exported["source_epoch"] != ack["source_epoch"]
            or exported["seal"] != material["sealed_target"]["sealed"]):
        raise ValueError("semantic_packet_export")
    bundle_path = p.pinned(material["bundle"])
    bundle = p.validate_bundle(bundle_path.parent, material["bundle"]["sha256"])
    if (bundle["source_id"] != source or schema.get("protocol") != "oo.oracle.inventory/v1"
            or bundle["boundary"]["identity"] != schema["identity"]):
        raise ValueError("semantic_packet_journal_source")
    journal = bundle_path.parent/"producer-journal.sqlite"
    native = {(t["owner"], t["table"]): t for t in schema["tables"]}
    tables, vectors, matched, audit = [], [], set(), []
    with sqlite3.connect(f"file:{journal}?mode=ro&immutable=1", uri=True) as db:
        for entry in catalog["entries"]:
            if (entry["engine"] != "oracle" or entry["source_id"] != source
                    or entry["source_generation"] != loaded["source_generation"]
                    or entry["target_generation"] != loaded["target_generation"]
                    or len(entry["tables"]) != 1):
                raise ValueError("semantic_packet_catalog_identity")
            table = entry["tables"][0]
            load_entries = [v for v in loaded["entries"] if v["run_id"] == entry["run_id"]]
            if len(load_entries) != 1:
                raise ValueError("semantic_packet_run")
            part = load_entries[0]
            raw_table = native[(part["schema"], part["table"])]
            captured_columns = bundle["source_mappings"][part["schema"]+"."+part["table"]]
            if (len(raw_table["columns"]) != len(captured_columns)
                    or any(any(actual.get(k) != v for k, v in expected["source_column"].items())
                           for actual, expected in zip(raw_table["columns"], captured_columns))):
                raise ValueError("semantic_packet_source_schema_drift")
            vector = {k: entry[k] for k in IDENTITY}
            vector.update(dataset=table["logical_schema"]+"."+table["logical_table"],
                          stored_namespace=table["stored_namespace"], stored_physical=table["stored_physical"])
            vectors.append(vector)
            if (table["logical_schema"] != part["storage_schema"] or table["logical_table"] != part["storage_table"]
                    or table["stored_physical"] != part["physical_id"] or table["accepted_rows"] != part["row_count"]):
                raise ValueError("semantic_packet_physical_binding")
            requests = [v for v in request["native_requests"]
                        if (v["logical_schema"], v["logical_table"]) == (part["schema"], part["table"])]
            # The source request run is the binding owner. The native import
            # run is allocated by Loader and comes from its real load result.
            if (len(requests) != 1 or requests[0]["run_id"] != root_receipt["binding"]
                    or requests[0]["source_id"] != source
                    or requests[0]["source_generation"] != entry["source_generation"]
                    or requests[0]["input_digest"] != entry["input_digest"]
                    or b.sha(Path(requests[0]["arrow_path"])) != entry["input_digest"]):
                raise ValueError("semantic_packet_arrow")
            # Keep genuine exported column spelling; do not repair a consumer's
            # normalization assumption by rewriting a sealed catalog.
            columns = {col["name"]: col for col in raw_table["columns"]}
            if set(columns) != {col["logical_name"] for col in table["columns"]}:
                raise ValueError("semantic_packet_explicit_column_mapping_required")
            selected = [row for row in exported["rows"] if
                        (row["schema"], row["table"]) == (part["schema"], part["table"])]
            names = [col["name"] for col in raw_table["columns"]]
            keys = [names.index(name) for name in raw_table["keys"]]
            seen, values = set(), {name: [] for name in names}
            for row in selected:
                if row["deleted"] is not False or set(row["value"]) != set(names):
                    raise ValueError("semantic_packet_live_row")
                cells = [cell(row["value"][name], columns[name]) for name in names]
                key = b.canonical([cells[i] for i in keys]).decode()
                if key in seen or set(row["key"]) != set(raw_table["keys"]):
                    raise ValueError("semantic_packet_key")
                seen.add(key)
                if any(cell(row["key"][name], columns[name]) != cells[names.index(name)] for name in raw_table["keys"]):
                    raise ValueError("semantic_packet_key")
                expected = db.execute("SELECT row_json FROM rows WHERE table_id=? AND key=?",
                                      (part["schema"]+"."+part["table"], key)).fetchone()
                if expected is None or json.loads(expected[0]) != cells:
                    raise ValueError("semantic_packet_typed_difference")
                for name, value in zip(names, cells):
                    values[name].append(value)
            if len(selected) != table["accepted_rows"]:
                raise ValueError("semantic_packet_row_count")
            matched.add((part["schema"], part["table"]))
            profiles = []
            for col in table["columns"]:
                name = col["logical_name"]
                profiles.append({"name": name, "source_name": name, "source_type": columns[name]["type"],
                    "rows": len(selected), "nulls": values[name].count(None),
                    "distinct": len({v for v in values[name] if v is not None}),
                    "distinct_null_policy": "SQL COUNT(DISTINCT): excludes null",
                    "captured_type": col["source_type"]})
            tables.append({**{k: entry[k] for k in IDENTITY}, "dataset": vector["dataset"],
                           "rows": len(selected), "full_scan": True, "columns": profiles})
            audit.append({"run_id": entry["run_id"], "dataset": vector["dataset"],
                          "rows": len(selected), "cells": len(selected)*len(names), "typed_differences": 0})
        if (vectors != catalog["binding_vector"] or matched != set(native)
                or sum(v["rows"] for v in tables) != db.execute("SELECT COUNT(*) FROM rows").fetchone()[0]
                or sum(v["rows"] for v in tables) != len(exported["rows"])):
            raise ValueError("semantic_packet_coverage")
    binding = {"source_id": source, "engine": "oracle", "collection_sha256": config["catalog"]["sha256"],
               "sealed_collection_content_sha256": catalog["collection_sha256"],
               "binding_vector_sha256": p.digest(catalog["binding_vector"])}
    profile = {**binding, "protocol": "oo.source.oracle.profile/v1", "sampled": False,
               "tables": tables, "observation_basis": "full immutable target-stage export bound to actual Loader ACK",
               "target_export": material["sealed_target"]["target_export"]}
    semantics = {**binding, "protocol": "oo.source.oracle.audit/v1", "executed": True,
                 "capture_method": "C1", "checkpoint_kind": "buffer_consumption",
                 "source_offset_write": False, "source_real_time_claimed": False,
                 "source_schema": config["source_schema"], "loader_ack": config["ack"],
                 "source_bundle": material["bundle"], "target_export": material["sealed_target"]["target_export"],
                 "type_dictionary": raw_table["columns"] if len(tables) == 1 else None,
                 "quality": audit, "business_semantics_approved": False, "business_dictionary": None}
    out = Path(out)
    out.mkdir(mode=0o700, exist_ok=False)
    for name, value in (("profile.json", profile), ("semantics.json", semantics), ("typed-audit.json", audit)):
        b.atomic(out/name, b.canonical(value)+b"\n")
    result = {"source_id": source, "catalog": config["catalog"], "source_schema": config["source_schema"],
              "profile": c.reference(out/"profile.json"), "semantics": c.reference(out/"semantics.json"),
              "require_business_dictionary": False}
    b.atomic(out/"oracle-observation-input.json", b.canonical(result)+b"\n")
    return result

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    try:
        print(json.dumps(build(json.loads(args.config.read_text()), args.out)))
    except (ValueError, KeyError, OSError, sqlite3.Error) as error:
        print(json.dumps({"error": str(error) if isinstance(error, ValueError) else type(error).__name__,
                          "target_accessed": False, "source_mutated": False}), file=sys.stderr)
        sys.exit(2)
