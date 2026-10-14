#!/usr/bin/env python3
"""Seal the accepted generation-3 importer catalog for one new API project.

The catalog bytes stay exact. This does not open a customer endpoint, does not
replay a committed load, and does not invent a period wait.
"""
import argparse
import hashlib
import json
from pathlib import Path

SOURCE = "oracle_oneclick_orders"
PROJECT = "oracle"
ROLE = "oo_r_oracle_orders"
RULES = {
    "KEY_ID": ("identifier", None),
    "AMOUNT": ("money", "unspecified"),
    "OCCURRED": ("event_time", None),
    "LABEL": ("free_text", None),
    "PAYLOAD": ("system", None),
}


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def source_name(logical):
    text = logical[2:] if logical.startswith("q_") else logical
    return bytes.fromhex(text).decode("ascii")


def put(path, value):
    path.write_text(json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n")
    path.chmod(0o600)


def build(catalog_path, out, shadow_path=None):
    raw = Path(catalog_path).read_bytes()
    catalog = json.loads(raw)
    if catalog.get("protocol") != "oo.pipeline.sealed-import-catalog/v1" or catalog.get("engine") != "oracle" or catalog.get("source_id") != SOURCE:
        raise ValueError("catalog is not the owned oracle importer export")
    tables = catalog["tables"]
    if len(tables) != 1:
        raise ValueError("catalog table count")
    table = tables[0]
    dataset = table["logical_schema"] + "." + table["logical_table"]
    columns = []
    for column in table["columns"]:
        name = source_name(column["logical_name"])
        if name not in RULES:
            raise ValueError("unexpected column")
        columns.append((name, column))
    shadow = json.loads(Path(shadow_path).read_text()) if shadow_path else None
    if shadow is not None and (shadow.get("generation") != catalog["source_generation"] or "2" not in {item.split(":", 1)[0] for item in shadow.get("tombstones", [])}):
        raise ValueError("shadow does not match the committed delete")
    out.mkdir(mode=0o700, parents=True, exist_ok=False)
    target = out / "catalog.json"
    target.write_bytes(raw)
    target.chmod(0o600)
    stats = {}
    if shadow is not None:
        for name, _column in columns:
            values = [row[name] for row in shadow["cells"].values()]
            stats[name] = {"rows": len(values), "nulls": values.count(None), "distinct": len(set(values))}
    profile = {
        "protocol": "oo.oracle.native-full-profile/v1", "source_id": SOURCE,
        "source_generation": catalog["source_generation"], "target_generation": catalog["target_generation"],
        "sampled": False, "samples": "none",
        "tables": [{"dataset": dataset, "rows": table["accepted_rows"], "full_scan": True,
                    "columns": [{"name": column["logical_name"], "source_name": name, "source_type": column["source_type"], **stats.get(name, {})} for name, column in columns]}],
    }
    semantics = {
        "protocol": "oo.oracle.native-c5-source-semantics/v1", "source_id": SOURCE, "executed": False,
        "source_generation": catalog["source_generation"], "target_generation": catalog["target_generation"],
        "product": "Oracle", "capture_method": "C5", "c1": "off", "consistency_grade": "S0",
        "delete_detection": "reconcile",
        "source_contract": {"reader": "OO04_READER", "write_attempts": 0, "log_mode": "NOARCHIVELOG", "supplemental_log": "NO", "empty_string": "null"},
        "quality": {"tables": 1, "rows": table["accepted_rows"], "columns": len(columns), "typed_cell_differences": 0, "delete_quality": "DELETES_LAGGED"},
    }
    if shadow is not None:
        semantics["quality"]["typed_cells"] = len(shadow["cells"]) * len(columns)
        semantics["quality"]["key_2_absent"] = "2" not in shadow["cells"]
        semantics["source_contract"]["position"] = shadow["position"]
    put(out / "profile.json", profile)
    put(out / "source_semantics.json", semantics)
    put(out / "generation.json", {"llm_calls": 0, "outcome": "structural_draft"})
    put(out / "suggestions.json", {"certified": False, "merged": False, "ready": False})
    put(out / "read_logic.json", {"catalog_sha256": sha(target), "contract": "oo.selfbuild-control-api/read-logic/v1", "executed": False,
        "mappings": [{"actual_storage_schema": "_oo", "dataset": dataset, "scope_namespace": table["stored_namespace"], "stored_physical": table["stored_physical"]}]})
    logical = [dataset + "." + column["logical_name"] for _name, column in columns]
    put(out / "roles.json", {"contract": "oo.selfbuild-control-api/sealed-roles/v1", "roles": {ROLE: logical}})
    rules = []
    for name, column in columns:
        semantic, unit = RULES[name]
        rule = {"column": dataset + "." + column["logical_name"], "confidence": "high", "pii": "none", "semantic_type": semantic}
        if unit:
            rule["unit"] = unit
        rules.append(rule)
    put(out / "rules.json", {"columns": rules, "relationships": []})
    policy = {"id": "pol-oracle-oneclick", "version": "1"}
    knowledge = {"id": "know-oracle-oneclick", "version": "1"}
    settings = {"catalog_version": "pending", "knowledge": knowledge, "ontology_version": "ont-oracle-oneclick",
        "ontology": {"classification": {"auto_accept": "none"}, "compile": {"budgets": {"readme_tokens": 2000, "table_file_tokens": 4000}, "per_role": True, "sample_values": {"default": "masked", "max_per_column": 0}},
                     "default_tier_preference": ["raw"], "include_schemas": [table["logical_schema"]], "lifecycle": {"deprecation_min_releases": 2},
                     "locale": "en-US", "name": "oracle-oneclick", "publish": {"include_unreviewed": []}, "spec_version": "oos/v1"},
        "policy": policy, "project": PROJECT}
    visibility = {"catalog_version": "pending", "datasets": [{"columns": [column["logical_name"] for _name, column in columns], "dataset": dataset, "global_stats": "denied", "row_security": "none"}],
        "issued_for_compiler": "oo-builder/role-pack/v1", "knowledge": knowledge, "limits": {"max_entries": 128, "max_file_bytes": 262144, "max_total_bytes": 1048576},
        "ontology_version": "ont-oracle-oneclick", "policy": policy, "principal": "fixture-admin", "project": PROJECT, "protocol": "oo.builder.role-visibility/v1", "role": ROLE}
    put(out / "native-model.json", {"dataset": dataset, "declared_price": {"currency": "USD", "per_total_token": "0.000001", "status": "declared_non_billing"}, "settings": settings, "visibility": visibility})
    files = {path.name: sha(path) for path in sorted(out.iterdir()) if path.is_file()}
    put(out / "manifest.json", {"contract": "oo.selfbuild-control-api/sealed-bundle/v1", "files": files})
    return {"catalog_sha256": sha(target), "columns": len(columns), "dataset": dataset, "files": len(files)}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--catalog", type=Path, required=True)
    parser.add_argument("--shadow", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(build(args.catalog, args.out, args.shadow)))


if __name__ == "__main__":
    main()
