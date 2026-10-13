#!/usr/bin/env python3
"""Build a Doris-only Selfbuild bundle from one acknowledged seven-table vector.

The full source tick and independent target comparison must describe the same
current rows. This command creates no model response and grants no privileges.
"""

import argparse
import hashlib
import json
from pathlib import Path


SOURCE = "dorispreview"
SCHEMA = "oo_dl01w1_43"
TABLES = ("dup_events", "agg_qty", "uniq_acct", "dec_exact", "part_day", "str_key", "nz_obs")
ROLE = "oo_r_dorispreview"
SOURCE_FIELDS = {
    "dup_events": (("event_id", "bigint", False), ("kind", "varchar(16)", False),
                   ("qty", "decimal(10,2)", True)),
    "agg_qty": (("sku", "varchar(16)", False), ("qty", "decimal(10,2) sum", True),
                ("note", "varchar(32) replace", True)),
    "uniq_acct": (("id", "bigint", False), ("balance", "decimal(18,4)", True),
                  ("label", "varchar(32)", True)),
    "dec_exact": (("id", "int", False), ("a18", "decimal(18,4)", True),
                  ("b18", "decimal(18,4)", True), ("d38", "decimal(38,18)", True)),
    "part_day": (("day", "date", False), ("region", "varchar(8)", False),
                 ("amount", "decimal(12,2)", True)),
    "str_key": (("k", "varchar(16)", False), ("note", "varchar(8)", True)),
    "nz_obs": (("id", "int", False), ("qty", "decimal(10,2)", True)),
}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


def save(directory, name, value):
    path = directory / name
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n")
    return sha(path.read_bytes())


def require(ok, detail):
    if not ok:
        raise ValueError(detail)


def main():
    parser = argparse.ArgumentParser()
    for name in ("collection", "capture", "diff", "out"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--unknown-money-unit", action="store_true",
                        help="seal amount and balance as money with source-unspecified unit; keep them out of the preview role")
    args = parser.parse_args()
    require(not args.out.exists(), "output already exists")
    collection_bytes = args.collection.read_bytes()
    collection = json.loads(collection_bytes)
    capture = json.loads(args.capture.read_bytes())
    diff = json.loads(args.diff.read_bytes())
    require(collection.get("protocol") == "oo.pipeline.sealed-import-collection/v1", "collection protocol")
    entries = collection.get("entries")
    bindings = collection.get("binding_vector")
    require(isinstance(entries, list) and len(entries) == len(bindings) == len(TABLES), "seven bindings")
    require(capture.get("protocol") == "oo.doris.capture-tick/v1" and capture.get("product_reader_write_attempts") == 0,
            "actual read-only Doris capture")
    require(diff.get("sampled") is False and diff.get("all_equal") is True, "independent full typed-cell diff")
    actual = {item["name"]: item for item in capture["tables"]}
    require(set(actual) == set(TABLES), "actual seven-table capture")
    require(set(diff.get("tables", {})) == set(TABLES), "independent seven-table comparison")
    generation = {}
    profiles = []
    mappings = []
    roles = []
    rules = []
    grades = {}
    seen = set()
    for entry, binding in zip(entries, bindings):
        require(entry.get("engine") == "doris" and entry.get("source_id") == SOURCE, "source identity")
        require(len(entry.get("tables", [])) == 1, "single-table catalog entry")
        table = entry["tables"][0]
        name = table["logical_table"]
        dataset = SCHEMA + "." + name
        require(name in TABLES and name not in seen and table["logical_schema"] == SCHEMA,
                "logical table identity")
        seen.add(name)
        require(binding.get("dataset") == dataset, "binding dataset")
        for key in ("source_id", "run_id", "source_generation", "target_generation",
                    "config_digest", "schema_digest", "type_digest", "input_digest"):
            require(binding.get(key) == entry.get(key), "binding identity " + key)
        require(binding.get("stored_namespace") == table.get("stored_namespace")
                and binding.get("stored_physical") == table.get("stored_physical"), "physical binding")
        source_rows = [row.split("\x1f") for row in actual[name]["rows"]]
        columns = table["columns"]
        require([item["logical_name"] for item in columns] == [field[0] for field in SOURCE_FIELDS[name]],
                "Doris DDL column order")
        require(all(len(row) == len(columns) for row in source_rows), "source typed cell width")
        require(len(source_rows) == table["accepted_rows"] == diff["tables"][name]["expected_rows"]
                == diff["tables"][name]["target_rows"], "unsampled row count")
        fields = []
        for index, column in enumerate(columns):
            field = column["logical_name"]
            _, source_type, nullable = SOURCE_FIELDS[name][index]
            values = [row[index] for row in source_rows if row[index] != "\\N"]
            fields.append({"name": field, "source_type": source_type,
                           "nullable": nullable, "aggregation": "full_unsampled",
                           "min_max_encoding": "source_text", "min": min(values) if values else None,
                           "max": max(values) if values else None, "rows": len(source_rows),
                           "nulls": len(source_rows) - len(values), "distinct": len(set(values))})
            roles.append(dataset + "." + field)
        gen = {key: entry[key] for key in ("source_generation", "target_generation", "run_id")}
        generation[dataset] = gen
        profiles.append({"dataset": dataset, "schema": SCHEMA, "table": name,
                         "rows": len(source_rows), "full_scan": True, "columns": fields, **gen})
        mappings.append({"dataset": dataset, "stored_physical": table["stored_physical"],
                         "scope_namespace": table["stored_namespace"], "actual_storage_schema": "_oo"})
        grades[name] = {"capture_method": actual[name]["route"],
                        "consistency": actual[name]["consistency"],
                        "delete_detection": actual[name]["delete_mode"],
                        "quality": actual[name].get("quality")}
        if name == "uniq_acct":
            rules.append({"column": dataset + ".id", "semantic_type": "identifier",
                          "confidence": "high", "pii": "unknown"})
            if args.unknown_money_unit:
                rules.append({"column": dataset + ".balance", "semantic_type": "money",
                              "unit": "unspecified", "confidence": "high", "pii": "unknown"})
        if name == "part_day":
            rules.append({"column": dataset + ".day", "semantic_type": "date",
                          "confidence": "high", "pii": "unknown"})
            if args.unknown_money_unit:
                rules.append({"column": dataset + ".amount", "semantic_type": "money",
                              "unit": "unspecified", "confidence": "high", "pii": "unknown"})
    require(seen == set(TABLES) and len(roles) == len(set(roles)) == 20, "exact Doris coverage")
    preview_roles = ([field for field in roles if field not in
                      (SCHEMA + ".uniq_acct.balance", SCHEMA + ".part_day.amount")]
                     if args.unknown_money_unit else roles)
    vector_sha = sha(canonical(bindings))
    collection_sha = sha(collection_bytes)
    high_source = max(item["source_generation"] for item in entries)
    high_target = max(item["target_generation"] for item in entries)
    common = {"source_id": SOURCE, "source_generation": high_source,
              "target_generation": high_target, "generation_semantics": "high_watermark",
              "table_generations": generation, "binding_vector_sha256": vector_sha,
              "collection_sha256": collection_sha}
    profile = {"protocol": "oo.source.doris.profile/v1", "sampled": False,
               "samples": "none", "tables": profiles, **common}
    semantics = {"protocol": "oo.source.doris.audit/v1", "product": "Apache Doris",
                 "version": capture["backend_version"], "capture_methods": {name: grades[name] for name in TABLES},
                 "consistency_grade": "per_table", "cross_table_consistent": False,
                 "reader_write_attempts": 0, "source_full_typed_cell_diff": 0,
                 "quality": {"warnings": {name: grades[name]["quality"] for name in TABLES}}, **common}
    knowledge = {"id": "know-doris-dl01w1-43", "version": "1"}
    policy = {"id": "pol-doris-dl01w1-43", "version": "1"}
    version = ("ont-doris-dl01w1-43-native-unspecified-unit" if args.unknown_money_unit
               else "ont-doris-dl01w1-43-native")
    native_model = {"dataset": SCHEMA + ".dup_events",
                    "settings": {"catalog_version": "pending", "knowledge": knowledge,
                                 "ontology": {"classification": {"auto_accept": "rule_high_confidence"},
                                              "compile": {"budgets": {"readme_tokens": 2000,
                                                                       "table_file_tokens": 4000},
                                                          "per_role": True,
                                                          "sample_values": {"default": "masked",
                                                                            "max_per_column": 0}},
                                              "default_tier_preference": ["raw"],
                                              "include_schemas": [SCHEMA],
                                              "lifecycle": {"deprecation_min_releases": 2},
                                              "locale": "en-US", "name": SCHEMA,
                                              "publish": {"include_unreviewed": []},
                                              "spec_version": "oos/v1"},
                                 "ontology_version": version, "policy": policy,
                                 "project": "doris"},
                    "visibility": {"catalog_version": "pending",
                                   "datasets": [{"dataset": item["dataset"],
                                                 "columns": [field["name"] for field in item["columns"]
                                                             if item["dataset"] + "." + field["name"] in preview_roles],
                                                 "global_stats": "denied", "row_security": "none"}
                                                for item in profiles],
                                   "issued_for_compiler": "oo-builder/role-pack/v1",
                                   "knowledge": knowledge,
                                   "limits": {"max_entries": 128, "max_file_bytes": 262144,
                                              "max_total_bytes": 1048576},
                                   "ontology_version": version, "policy": policy,
                                   "principal": "fixture-admin", "project": "doris",
                                   "protocol": "oo.builder.role-visibility/v1", "role": ROLE}}
    if args.unknown_money_unit:
        native_model["declared_price"] = {"status": "declared_non_billing", "currency": "USD",
                                          "per_total_token": "0.00001000"}
    args.out.mkdir(parents=True)
    (args.out / "catalog.json").write_bytes(collection_bytes)
    files = {"catalog.json": collection_sha}
    artifacts = {
        "profile.json": profile, "source_semantics.json": semantics,
        "read_logic.json": {"contract": "oo.selfbuild-control-api/read-logic/v1",
                            "executed": False, "catalog_sha256": collection_sha, "mappings": mappings},
        "rules.json": {"columns": rules, "relationships": []},
        "suggestions.json": {"certified": False, "merged": False, "ready": False},
        "generation.json": {"llm_calls": 0, "outcome": "structural_draft"},
        "roles.json": {"contract": "oo.selfbuild-control-api/sealed-roles/v1",
                       "roles": {ROLE: preview_roles}},
        "native-model.json": native_model,
    }
    for name, value in artifacts.items():
        files[name] = save(args.out, name, value)
    save(args.out, "manifest.json", {"contract": "oo.selfbuild-control-api/sealed-bundle/v1",
                                     "files": files})
    print(json.dumps({"collection_sha256": collection_sha, "vector_sha256": vector_sha,
                      "tables": len(profiles), "columns": len(roles), "visible_columns": len(preview_roles),
                      "rows": sum(item["rows"] for item in profiles)}))


if __name__ == "__main__":
    main()
