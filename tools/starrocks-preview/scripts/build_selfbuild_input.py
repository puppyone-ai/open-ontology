#!/usr/bin/env python3
"""Build the StarRocks Selfbuild input from the acknowledged native collection."""
import hashlib
import json
import argparse
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
EVIDENCE = ROOT / "evidence/2c023948-0862-4dae-ae8f-6fc30241db62/DL01/starrocks-product/raw"
CAPTURES = EVIDENCE / "native-sync/captures"
COLLECTION = EVIDENCE / "native-sync/collection-gen2.json"
DIFF = EVIDENCE / "native-sync/independent/catchup-gen2-20261013/compare.json"
OUT = EVIDENCE / "native-recovery-20261013/selfbuild-input"
SOURCE = "sr_w45_native"
ROLE = "oo_r_w45_native"


def sha(data):
    return hashlib.sha256(data).hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def save(out, name, value):
    path = out / name
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n")
    return sha(path.read_bytes())


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--generation", type=int, choices=(2, 3), default=3)
    args = parser.parse_args()
    generation_number = args.generation
    collection_path = EVIDENCE / f"native-sync/collection-gen{generation_number}.json"
    diff_path = EVIDENCE / ("native-sync/independent/catchup-gen2-20261013/compare.json" if generation_number == 2
                           else "native-sync/independent/legal-batch-gen3-20261013/compare.json")
    out = OUT if generation_number == 2 else EVIDENCE / "native-batch-repair-20261013/selfbuild-input"
    out.mkdir(parents=True, exist_ok=False)
    raw = collection_path.read_bytes()
    collection = json.loads(raw)
    diff = json.loads(diff_path.read_text())
    assert diff["sampled"] is False and diff["all_equal"] is True
    assert collection["protocol"] == "oo.pipeline.sealed-import-collection/v1"
    assert len(collection["entries"]) == len(collection["binding_vector"]) == 5
    vector_sha = sha(canonical(collection["binding_vector"]).encode())
    generation = {}
    profiles = []
    roles = []
    mappings = []
    rules = []
    for entry, binding in zip(collection["entries"], collection["binding_vector"]):
        table = entry["tables"][0]
        name = table["logical_table"]
        dataset = "oo_w45_sr." + name
        assert binding["dataset"] == dataset and entry["source_id"] == SOURCE
        generation[dataset] = {key: entry[key] for key in ("source_generation", "target_generation", "run_id")}
        assert entry["source_generation"] == generation_number
        matching = [path.parent / "decoded" / f"{name}.json"
                    for path in CAPTURES.glob(f"*/{generation_number}/request.json")
                    if json.loads(path.read_text())["logical_table"] == name]
        assert len(matching) == 1, (name, matching)
        decoded = json.loads(matching[0].read_text())
        rows = decoded["rows"]
        assert len(rows) == table["accepted_rows"] == diff["tables"][name]["expected_rows"]
        assert diff["tables"][name]["target_rows"] == len(rows)
        field_names = [field["name"] for field in decoded["fields"]]
        assert field_names == [column["logical_name"] for column in table["columns"]]
        fields = []
        for field in decoded["fields"]:
            column = field["name"]
            values = [row[column] for row in rows if row[column] is not None]
            texts = [str(value) for value in values]
            fields.append({"name": column, "source_type": field["native_type"],
                           "nullable": field["nullable"], "aggregation": "full_unsampled",
                           "min_max_encoding": "source_text", "min": min(texts) if texts else None,
                           "max": max(texts) if texts else None, "rows": len(rows),
                           "nulls": len(rows) - len(values), "distinct": len(set(map(canonical, values)))})
            roles.append(dataset + "." + column)
        profiles.append({"dataset": dataset, "schema": "oo_w45_sr", "table": name,
                         "rows": len(rows), "full_scan": True, "columns": fields,
                         **generation[dataset]})
        mappings.append({"dataset": dataset, "stored_physical": table["stored_physical"],
                         "scope_namespace": table["stored_namespace"], "actual_storage_schema": "_oo"})
        if name in {"pk_accounts", "uniq_dim", "dup_events"}:
            key = {"pk_accounts": "account_id", "uniq_dim": "dim_id", "dup_events": "event_id"}[name]
            rules.append({"column": dataset + "." + key, "semantic_type": "identifier",
                          "confidence": "high", "pii": "unknown"})
    assert len(roles) == 19 and len(set(roles)) == 19
    collection_sha = sha(raw)
    common = {"source_id": SOURCE, "source_generation": generation_number,
              "target_generation": max(item["target_generation"] for item in generation.values()),
              "generation_semantics": "high_watermark", "table_generations": generation,
              "binding_vector_sha256": vector_sha, "collection_sha256": collection_sha}
    profile = {"protocol": "oo.source.starrocks.profile/v1", "sampled": False,
               "tables": profiles, **common}
    semantics = {"protocol": "oo.source.starrocks.audit/v1", "product": "StarRocks",
                 "version": "3.5.21-85fccf8", "capture_method": "C5",
                 "consistency_grade": "B", "cross_table_consistent": False,
                 "delete_detection": "reconcile", "reader_write_attempts": 0,
                 "quality": {"agg_metrics": ["DELETES_UNDETECTED"],
                             "dup_partition_sums": ["DELETES_UNDETECTED"],
                             "other_tables": ["DELETES_LAGGED"]},
                 "schedule_period_ms": {"uniq_dim": 20000, "other_tables": 6000},
                 "source_full_typed_cell_diff": 0, **common}
    version = f"ont-starrocks-native-c5-g{generation_number}"
    knowledge = {"id": "know-starrocks-w45", "version": "1"}
    policy = {"id": "pol-starrocks-w45", "version": "1"}
    native_model = {
        "dataset": "oo_w45_sr.dup_events",
        "settings": {"catalog_version": "pending", "knowledge": knowledge,
                     "ontology": {"classification": {"auto_accept": "rule_high_confidence"},
                                  "compile": {"budgets": {"readme_tokens": 2000, "table_file_tokens": 4000},
                                              "per_role": True,
                                              "sample_values": {"default": "masked", "max_per_column": 0}},
                                  "default_tier_preference": ["raw"],
                                  "include_schemas": ["oo_w45_sr"],
                                  "lifecycle": {"deprecation_min_releases": 2},
                                  "locale": "en-US", "name": "oo_w45_sr",
                                  "publish": {"include_unreviewed": []},
                                  "spec_version": "oos/v1"},
                     "ontology_version": version, "policy": policy, "project": "starrocks"},
        "visibility": {"catalog_version": "pending",
                       "datasets": [{"dataset": item["dataset"],
                                     "columns": [field["name"] for field in item["columns"]],
                                     "global_stats": "denied", "row_security": "none"}
                                    for item in profiles],
                       "issued_for_compiler": "oo-builder/role-pack/v1",
                       "knowledge": knowledge,
                       "limits": {"max_entries": 128, "max_file_bytes": 262144,
                                  "max_total_bytes": 1048576},
                       "ontology_version": version, "policy": policy,
                       "principal": "fixture-admin", "project": "starrocks",
                       "protocol": "oo.builder.role-visibility/v1",
                       "role": ROLE}}
    files = {"catalog.json": sha(raw)}
    (out / "catalog.json").write_bytes(raw)
    for name, value in {
        "profile.json": profile, "source_semantics.json": semantics,
        "read_logic.json": {"contract": "oo.selfbuild-control-api/read-logic/v1",
                            "executed": False, "catalog_sha256": collection_sha, "mappings": mappings},
        "rules.json": {"columns": rules, "relationships": []},
        "suggestions.json": {"certified": False, "merged": False, "ready": False},
        "generation.json": {"llm_calls": 0, "outcome": "structural_draft"},
        "roles.json": {"contract": "oo.selfbuild-control-api/sealed-roles/v1",
                       "roles": {ROLE: roles}},
        "native-model.json": native_model,
    }.items():
        files[name] = save(out, name, value)
    save(out, "manifest.json", {"contract": "oo.selfbuild-control-api/sealed-bundle/v1", "files": files})
    print(json.dumps({"collection_sha256": collection_sha, "vector_sha256": vector_sha,
                      "tables": len(profiles), "columns": len(roles), "rows": sum(p["rows"] for p in profiles)}))


if __name__ == "__main__":
    main()
