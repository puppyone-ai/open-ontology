#!/usr/bin/env python3
"""Export actual native runs through the existing trusted catalog command."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess


def sha(data):
    return hashlib.sha256(data).hexdigest()


def collection(entries, expected_datasets):
    assert entries, "empty collection"
    assert expected_datasets and len(expected_datasets) == len(set(expected_datasets)), "expected datasets"
    datasets = set()
    bindings = []
    for entry in entries:
        assert entry["protocol"] == "oo.pipeline.sealed-import-catalog/v1"
        assert entry["state"] == "awaiting_publish"
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
    body["collection_sha256"] = sha(json.dumps(body, sort_keys=True, separators=(",", ":")).encode())
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
    assert 1 <= len(config["entries"]) <= 32, "collection budget"
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
