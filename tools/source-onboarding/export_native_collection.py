#!/usr/bin/env python3
"""Export actual native runs through the existing trusted catalog command."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import re


def sha(data):
    return hashlib.sha256(data).hexdigest()


def published_oracle_c5(entry):
    proof = entry.get("oracle_c5_binding")
    assert entry.get("engine") == "oracle" and entry.get("state") == "published", "published engine"
    assert entry.get("capture") == "sealed_import"
    assert isinstance(proof, dict) and set(proof) == {
        "protocol", "source_id", "run_id", "source_generation", "target_generation",
        "captured_rows", "effective_rows", "scope", "publication_receipt_sha256"}, "C5 binding"
    assert proof["protocol"] == "oo.pipeline.oracle-c5-catalog-binding/v1"
    assert all(proof.get(k) == entry.get(k) and k in entry for k in
               ("source_id", "run_id", "source_generation", "target_generation")), "C5 identity"
    assert all(type(proof[k]) is int and proof[k] > 0 for k in
               ("source_generation", "target_generation")), "C5 generation"
    assert all(type(proof[k]) is int and proof[k] >= 0 for k in
               ("captured_rows", "effective_rows")), "C5 rows"
    assert proof["captured_rows"] <= proof["effective_rows"]
    assert isinstance(proof["publication_receipt_sha256"], str) and re.fullmatch(
        "[0-9a-f]{64}", proof["publication_receipt_sha256"]), "C5 publication digest"
    scope = proof["scope"]
    assert isinstance(scope, dict) and scope.get("protocol") == "oo.pipeline.native-c5-range/v1"
    assert scope.get("source_engine") == "oracle" and scope.get("capture_method") == "c5_diff"
    assert scope.get("source_id") == entry["source_id"] and scope.get("source_generation") == entry["source_generation"]
    assert type(scope.get("parent_generation")) is int and scope["parent_generation"] + 1 == entry["source_generation"]
    assert type(scope.get("row_count")) is int and scope["row_count"] == proof["captured_rows"]
    assert scope.get("arrow_sha256") == entry.get("input_digest")
    # Source/Arrow schema and exported logical catalog schema have distinct preimages.
    assert isinstance(scope.get("schema_fingerprint"), str) and re.fullmatch(
        "[0-9a-f]{64}", scope["schema_fingerprint"])
    assert scope.get("delete_detection") == "reconcile" and scope.get("position_codec") == "snapshot-zero/v1"
    assert scope.get("applied_pos") == "0"
    snapshot = scope.get("oracle_snapshot")
    assert isinstance(snapshot, dict) and isinstance(snapshot.get("scn"), str) and re.fullmatch(
        "[1-9][0-9]{0,37}", snapshot["scn"]), "C5 SCN"
    assert isinstance(snapshot.get("manifest_sha256"), str) and re.fullmatch(
        "[0-9a-f]{64}", snapshot["manifest_sha256"])
    assert isinstance(snapshot.get("identity"), str) and snapshot["identity"]
    assert snapshot.get("source_schema") == scope.get("schema") and snapshot.get("source_table") == scope.get("table")
    assert len(entry["tables"]) == 1 and entry["tables"][0]["accepted_rows"] == proof["effective_rows"]


def collection(entries, expected_datasets):
    assert entries, "empty collection"
    assert expected_datasets and len(expected_datasets) == len(set(expected_datasets)), "expected datasets"
    datasets = set()
    bindings = []
    for entry in entries:
        assert entry["protocol"] == "oo.pipeline.sealed-import-catalog/v1"
        if entry["state"] == "published":
            published_oracle_c5(entry)
        else:
            assert entry["state"] == "awaiting_publish" and "oracle_c5_binding" not in entry
        assert entry["shadows_queried"] is False
        for table in entry["tables"]:
            dataset = (table["logical_schema"], table["logical_table"])
            assert dataset not in datasets, "duplicate logical dataset"
            datasets.add(dataset)
            bindings.append({
                "dataset": ".".join(dataset),
                **{key: entry[key] for key in ("source_id", "run_id", "source_generation",
                   "target_generation", "config_digest", "schema_digest", "type_digest", "input_digest")},
                "stored_namespace": table["stored_namespace"],
                "stored_physical": table["stored_physical"],
            })
    assert datasets == set(expected_datasets), "incomplete collection"
    body = {"protocol": "oo.pipeline.sealed-import-collection/v1", "entries": entries,
            "binding_vector": bindings}
    body["collection_sha256"] = sha(json.dumps(body, sort_keys=True, ensure_ascii=False, separators=(",", ":")).encode())
    return body


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--operator", required=True, help="private operator configuration, mode 0600")
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    path = Path(args.operator)
    assert path.is_file() and path.stat().st_mode & 0o077 == 0, "operator permissions"
    config = json.loads(path.read_bytes())
    assert config["protocol"] == "oo.pipeline.native-collection-operator/v1"
    binary = config["pipeline"]["path"]
    assert sha(Path(binary).read_bytes()) == config["pipeline"]["sha256"], "pipeline digest"
    assert isinstance(config["entries"], list) and config["entries"], "empty collection"
    expected_datasets = [tuple(dataset) for dataset in config["expected_datasets"]]
    assert all(len(dataset) == 2 and all(isinstance(name, str) and name for name in dataset)
               for dataset in expected_datasets), "expected datasets"
    entries = []
    for index, item in enumerate(config["entries"]):
        request = Path(item["request"] ).read_bytes()
        expected = json.loads(request)
        registry = Path(item["registry"]).read_bytes()
        assert sha(registry) == item["registry_sha256"], "registry digest"
        env = {"PATH": "/usr/bin:/bin",
               "OO_NATIVE_ADMISSION_BIN": config["admission_binary"],
               "OO_NATIVE_BINDING_REGISTRY": item["registry"],
               "OO_NATIVE_BINDING_REGISTRY_SHA256": item["registry_sha256"]}
        result = subprocess.run([binary, "catalog-import-native"], input=request,
                                capture_output=True, env=env, timeout=60)
        if result.returncode:
            Path(args.out + f".entry-{index}.stderr").write_bytes(result.stderr)
            Path(args.out + f".entry-{index}.stdout").write_bytes(result.stdout)
            raise SystemExit(result.returncode)
        exported = json.loads(result.stdout)
        for key in ("source_id", "run_id", "source_generation", "target_generation",
                    "config_digest", "type_digest", "input_digest"):
            assert exported[key] == expected[key], "export identity"
        entries.append(exported)
    temporary = Path(args.out + ".tmp")
    temporary.write_text(json.dumps(collection(entries, expected_datasets), indent=2) + "\n")
    os.replace(temporary, args.out)


if __name__ == "__main__":
    main()
