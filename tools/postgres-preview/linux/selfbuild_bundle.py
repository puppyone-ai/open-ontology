#!/usr/bin/env python3
"""Bind a current four-table PostgreSQL collection into a private Selfbuild bundle."""
import argparse
import hashlib
import json
from pathlib import Path


def put(path, value):
    path.write_text(json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n")


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--collection", type=Path, required=True)
    p.add_argument("--profile", type=Path, required=True)
    p.add_argument("--semantics", type=Path, required=True)
    p.add_argument("--out", type=Path, required=True)
    a = p.parse_args()
    collection = json.loads(a.collection.read_text())
    profile = json.loads(a.profile.read_text())
    semantics = json.loads(a.semantics.read_text())
    assert collection["protocol"] == "oo.pipeline.sealed-import-collection/v1"
    assert semantics["consistency_grade"] == "T" and semantics["quality"]["typed_cell_differences"] == 0
    assert profile["collection_sha256"] == semantics["collection_sha256"] == sha(a.collection)
    tables = [t for e in collection["entries"] for t in e["tables"]]
    assert len(tables) == 4 and sum(len(t["columns"]) for t in tables) == 38
    datasets = [t["logical_schema"] + "." + t["logical_table"] for t in tables]
    assert set(datasets) == {"customer_east.delivery_notes", "public.hubs", "public.shipments", "public.tracking_events"}
    out = a.out
    out.mkdir(parents=True, exist_ok=False)
    (out / "catalog.json").write_bytes(a.collection.read_bytes())
    (out / "profile.json").write_bytes(a.profile.read_bytes())
    (out / "source_semantics.json").write_bytes(a.semantics.read_bytes())
    put(out / "generation.json", {"llm_calls": 0, "outcome": "structural_draft"})
    put(out / "suggestions.json", {"certified": False, "merged": False, "ready": False})
    put(out / "read_logic.json", {"contract": "oo.selfbuild-control-api/read-logic/v1", "catalog_sha256": sha(a.collection), "executed": False,
        "mappings": [{"dataset": d, "actual_storage_schema": "_oo", "scope_namespace": t["stored_namespace"], "stored_physical": t["stored_physical"]} for d, t in zip(datasets, tables)]})
    all_columns = [d + "." + c["logical_name"] for d, t in zip(datasets, tables) for c in t["columns"]]
    put(out / "roles.json", {"contract": "oo.selfbuild-control-api/sealed-roles/v1", "roles": {"oo_r_pg46_full": all_columns}})
    ids = [name for name in all_columns if name.rsplit(".", 1)[-1].endswith("_id")]
    put(out / "rules.json", {"columns": [{"column": name, "confidence": "high", "pii": "unknown", "semantic_type": "identifier"} for name in ids],
        "relationships": []})
    project = "postgres"
    ontology_version = "ont-pg46-t-803"
    policy = {"id": "pol-pg46-preview", "version": "1"}
    knowledge = {"id": "know-pg46-preview", "version": "1"}
    settings = {"project": project, "ontology_version": ontology_version, "catalog_version": "pending", "policy": policy, "knowledge": knowledge,
        "ontology": {"spec_version": "oos/v1", "name": "postgres46", "locale": "en-US", "include_schemas": ["customer_east", "public"],
                     "default_tier_preference": ["raw"], "classification": {"auto_accept": "rule_high_confidence"},
                     "compile": {"budgets": {"readme_tokens": 2000, "table_file_tokens": 4000}, "per_role": True,
                                 "sample_values": {"default": "masked", "max_per_column": 0}},
                     "lifecycle": {"deprecation_min_releases": 2}, "publish": {"include_unreviewed": []}}}
    visibility = {"protocol": "oo.builder.role-visibility/v1", "project": project, "ontology_version": ontology_version, "catalog_version": "pending",
        "policy": policy, "knowledge": knowledge, "issued_for_compiler": "oo-builder/role-pack/v1", "principal": "fixture-admin", "role": "oo_r_pg46_full",
        "limits": {"max_entries": 128, "max_file_bytes": 262144, "max_total_bytes": 1048576},
        "datasets": [{"dataset": d, "columns": [c["logical_name"] for c in t["columns"]], "global_stats": "denied", "row_security": "none"} for d, t in zip(datasets, tables)]}
    put(out / "native-model.json", {"dataset": "public.hubs", "settings": settings, "visibility": visibility,
        "declared_price": {"status": "declared_non_billing", "currency": "USD", "per_total_token": "0.000001"}})
    files = {p.name: sha(p) for p in out.iterdir() if p.is_file()}
    put(out / "manifest.json", {"contract": "oo.selfbuild-control-api/sealed-bundle/v1", "files": files})
    print(json.dumps({"bundle": str(out), "files": len(files), "columns": len(all_columns), "manifest_sha256": sha(out / "manifest.json")}))


if __name__ == "__main__":
    main()
