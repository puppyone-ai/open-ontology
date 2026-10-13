#!/usr/bin/env python3
"""Build the MariaDB Selfbuild input from the exact exported native collection."""
import hashlib
import json
import pathlib
import shutil
import sys


def write(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n")


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    source, dest = map(pathlib.Path, sys.argv[1:3])
    flags = set(sys.argv[3:])
    if len(sys.argv) < 3 or len(flags) != len(sys.argv[3:]) or flags - {"--declared-nonbilling-price", "--source-contract"}:
        raise SystemExit("usage: build_selfbuild_bundle.py SOURCE DEST [--declared-nonbilling-price] [--source-contract]")
    dest.mkdir(parents=True, exist_ok=True)
    collection = json.loads((source / "collection.json").read_text())
    profile = json.loads((source / "source-profile.json").read_text())
    tables = [table for entry in collection["entries"] for table in entry["tables"]]
    datasets = [table["logical_schema"] + "." + table["logical_table"] for table in tables]
    columns = [f'{dataset}.{column["logical_name"]}' for dataset, table in zip(datasets, tables) for column in table["columns"]]
    assert len(tables) == 5 and len(columns) == 36
    for dst, src in (("catalog.json", "collection.json"), ("profile.json", "source-profile.json"), ("source_semantics.json", "source-semantics.json")):
        shutil.copyfile(source / src, dest / dst)
    write(dest / "generation.json", {"llm_calls": 0, "outcome": "structural_draft"})
    write(dest / "suggestions.json", {"certified": False, "merged": False, "ready": False})
    write(dest / "roles.json", {"contract": "oo.selfbuild-control-api/sealed-roles/v1", "roles": {"oo_r_mariadb_full": columns}})
    mappings = [{"dataset": dataset, "actual_storage_schema": "_oo", "scope_namespace": table["stored_namespace"], "stored_physical": table["stored_physical"]} for dataset, table in zip(datasets, tables)]
    write(dest / "read_logic.json", {"contract": "oo.selfbuild-control-api/read-logic/v1", "catalog_sha256": sha(dest / "catalog.json"), "executed": False, "mappings": mappings})
    ids = [(dataset, table["columns"][0]["logical_name"]) for dataset, table in zip(datasets, tables)]
    key_file = source.parent.parent / "source-metadata" / "keys.ndjson"
    foreign_keys = [json.loads(line) for line in key_file.read_text().splitlines() if line.strip()]
    relationships = []
    for key in foreign_keys:
        if not key["reference_table"]:
            continue
        relationships.append({"name": key["constraint"], "from_column": f'{key["schema"]}.{key["table"]}.{key["column"]}', "to_column": f'{key["reference_schema"]}.{key["reference_table"]}.{key["reference_column"]}', "cardinality": "many_to_one", "confidence": "high"})
    assert len(relationships) == 4
    rules = {"columns": [{"column": f"{dataset}.{column}", "confidence": "high", "pii": "unknown", "semantic_type": "identifier"} for dataset, column in ids], "relationships": relationships}
    write(dest / "rules.json", rules)
    visibility = [{"dataset": dataset, "columns": [col["logical_name"] for col in table["columns"]], "global_stats": "denied", "row_security": "none"} for dataset, table in zip(datasets, tables)]
    settings = {"catalog_version": "pending", "knowledge": {"id": "know-mariadb-logistics", "version": "1"}, "ontology": {"classification": {"auto_accept": "rule_high_confidence"}, "compile": {"budgets": {"readme_tokens": 2000, "table_file_tokens": 4000}, "per_role": True, "sample_values": {"default": "masked", "max_per_column": 0}}, "default_tier_preference": ["raw"], "include_schemas": sorted({table["logical_schema"] for table in tables}), "lifecycle": {"deprecation_min_releases": 2}, "locale": "en-US", "name": "mariadb_logistics", "publish": {"include_unreviewed": []}, "spec_version": "oos/v1"}, "ontology_version": "ont-mariadb-native-g3", "policy": {"id": "pol-mariadb-logistics", "version": "1"}, "project": "mariadb"}
    model = {"dataset": datasets[0], "settings": settings, "visibility": {"catalog_version": "pending", "datasets": visibility, "issued_for_compiler": "oo-builder/role-pack/v1", "knowledge": settings["knowledge"], "limits": {"max_entries": 128, "max_file_bytes": 262144, "max_total_bytes": 1048576}, "ontology_version": settings["ontology_version"], "policy": settings["policy"], "principal": "fixture-admin", "project": "mariadb", "protocol": "oo.builder.role-visibility/v1", "role": "oo_r_mariadb_full"}}
    if "--declared-nonbilling-price" in flags:
        model["declared_price"] = {"currency": "USD", "per_total_token": "0.00000100", "status": "declared_non_billing"}
    if "--source-contract" in flags:
        caveats = [{"dataset": dataset, "column": column["logical_name"], "caveat": column["caveat"]} for dataset, table in zip(datasets, tables) for column in table["columns"] if column.get("caveat")]
        assert caveats == [{"dataset": "cust_west.k_meter", "column": name, "caveat": "bigint_unsigned_numeric"} for name in ("id", "reading")]
        semantics = json.loads((source / "source-semantics.json").read_text())
        assert semantics["product"] == "MariaDB" and semantics["version"] == "11.4.13"
        assert semantics["consistency_grade"] == "S0" and semantics["delete_detection"] == "reconcile"
        model["source_contract"] = {"schema": "oo.pg-builder.source-contract/v1", "family": "mariadb", "product": "mariadb", "version": semantics["version"], "capture": "sealed_import", "grade": semantics["consistency_grade"], "source_kind": "mariadb", "delete_detection": semantics["delete_detection"], "columns": caveats}
    write(dest / "native-model.json", model)
    write(dest / "manifest.json", {"contract": "oo.selfbuild-control-api/sealed-bundle/v1", "files": {p.name: sha(p) for p in dest.glob("*.json") if p.name != "manifest.json"}})
    print(json.dumps({"bundle": str(dest), "catalog_sha256": sha(dest / "catalog.json"), "manifest_sha256": sha(dest / "manifest.json"), "tables": datasets, "columns": len(columns)}))


if __name__ == "__main__":
    main()
