#!/usr/bin/env python3
"""Bind an existing full source profile to the exact current sealed collection vector."""

import argparse
import hashlib
import json
from pathlib import Path


IDENTITY = ("source_generation", "target_generation", "run_id")
DIGESTS = ("config_digest", "schema_digest", "type_digest", "input_digest")


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


def keyed(items, key):
    result = {}
    for item in items:
        name = item[key]
        if name in result:
            raise ValueError(f"duplicate {key}: {name}")
        result[name] = item
    return result


def bind(collection, profile, semantics, catalog_bytes):
    inner = collection["collection_sha256"]
    document = {key: value for key, value in collection.items() if key != "collection_sha256"}
    if hashlib.sha256(canonical(document)).hexdigest() != inner:
        raise ValueError("inner collection identity mismatch")
    outer = hashlib.sha256(catalog_bytes).hexdigest()
    vector_digest = hashlib.sha256(canonical(collection["binding_vector"])).hexdigest()
    if profile["binding_vector_sha256"] != vector_digest or semantics["binding_vector_sha256"] != vector_digest:
        raise ValueError("binding vector digest mismatch")
    vector = keyed(collection["binding_vector"], "dataset")
    tables = keyed(profile["tables"], "dataset")
    generations = profile["table_generations"]
    semantic_generations = semantics["table_generations"]
    semantic_datasets = keyed(semantics["dataset_generations"], "dataset")
    if len(vector) != 5 or set(vector) != set(tables) or set(vector) != set(generations) or set(vector) != set(semantic_generations) or set(vector) != set(semantic_datasets):
        raise ValueError("five-table scope mismatch")
    entries = {}
    for entry in collection["entries"]:
        if len(entry["tables"]) != 1:
            raise ValueError("collection entry must bind one table")
        table = entry["tables"][0]
        dataset = f'{table["logical_schema"]}.{table["logical_table"]}'
        if dataset in entries:
            raise ValueError(f"duplicate entry: {dataset}")
        entries[dataset] = (entry, table)
    if set(entries) != set(vector):
        raise ValueError("entry/vector scope mismatch")
    checks = []
    for dataset, expected in vector.items():
        entry, captured = entries[dataset]
        table = tables[dataset]
        if profile["source_id"] != expected["source_id"] or entry["source_id"] != expected["source_id"]:
            raise ValueError(f"source identity mismatch: {dataset}")
        if table["schema"] + "." + table["table"] != dataset or table["rows"] != captured["accepted_rows"] or len(table["columns"]) != len(captured["columns"]):
            raise ValueError(f"table shape mismatch: {dataset}")
        if {c["name"] for c in table["columns"]} != {c["logical_name"] for c in captured["columns"]}:
            raise ValueError(f"column binding mismatch: {dataset}")
        for field in (*IDENTITY, *DIGESTS):
            if entry[field] != expected[field]:
                raise ValueError(f"entry/vector {field} mismatch: {dataset}")
        for field in ("stored_namespace", "stored_physical"):
            if captured[field] != expected[field]:
                raise ValueError(f"physical binding mismatch: {dataset}")
        for repeated in (generations[dataset], semantic_generations[dataset], semantic_datasets[dataset]):
            for field in IDENTITY:
                if repeated[field] != expected[field]:
                    raise ValueError(f"repeated {field} mismatch: {dataset}")
        for field in IDENTITY:
            table[field] = expected[field]
        checks.append({"dataset": dataset, **{field: expected[field] for field in (*IDENTITY, *DIGESTS)}})
    profile["collection_sha256"] = outer
    semantics["collection_sha256"] = outer
    return profile, semantics, {"protocol": "oo.mysql.current-profile-binding-check/v1", "tables": checks,
                     "catalog_file_sha256": outer, "inner_collection_sha256": inner,
                     "binding_vector_sha256": vector_digest}


def main():
    parser = argparse.ArgumentParser()
    for name in ("collection", "profile", "semantics", "output", "semantics_output", "proof"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    catalog_bytes = args.collection.read_bytes()
    collection = json.loads(catalog_bytes)
    profile = json.loads(args.profile.read_text())
    semantics = json.loads(args.semantics.read_text())
    result, semantics_result, proof = bind(collection, profile, semantics, catalog_bytes)
    args.output.write_bytes(json.dumps(result, ensure_ascii=False, indent=2).encode() + b"\n")
    args.semantics_output.write_bytes(json.dumps(semantics_result, ensure_ascii=False, indent=2).encode() + b"\n")
    proof["profile_sha256"] = hashlib.sha256(args.output.read_bytes()).hexdigest()
    proof["semantics_sha256"] = hashlib.sha256(args.semantics_output.read_bytes()).hexdigest()
    args.proof.write_bytes(json.dumps(proof, ensure_ascii=False, indent=2).encode() + b"\n")


if __name__ == "__main__":
    main()
