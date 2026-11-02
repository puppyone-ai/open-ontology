#!/usr/bin/env python3
"""Build catalog/publication REQUESTS from real worker artifacts; no DB writes.

Does not manufacture Loader, admission, catalog or publication receipts.
"""
import argparse
import hashlib
import json
from pathlib import Path


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--artifacts", type=Path, required=True)
    ap.add_argument("--runtime-handoff", type=Path, required=True)
    ap.add_argument("--publisher", type=Path, required=True, help="latest-loader.json")
    ap.add_argument("--out", type=Path, required=True)
    a = ap.parse_args()
    request = json.loads((a.artifacts / "request.json").read_text())
    loaded = json.loads((a.artifacts / "load.json").read_text())
    ack = json.loads((a.artifacts / "ack.json").read_text())
    runtime = json.loads(a.runtime_handoff.read_text())
    publisher = json.loads(a.publisher.read_text())
    assert sha(publisher["path"]) == publisher["sha256"]
    assert ack["acknowledged"] is True and ack["task_state"] == "succeeded"
    assert ack["receipt"] == loaded and request["source_id"] == loaded["source_id"] == ack["source_id"]
    assert request["source_generation"] == loaded["source_generation"] == ack["source_generation"]
    assert loaded["load_state"] == "awaiting_publish"
    config = dict(line.split("=", 1) for line in Path(runtime["worker_config"]).read_text().splitlines()
                  if line and not line.startswith("#"))
    assert request["target_database"] == config["database"] == runtime["control"]["database"]
    target = dict(host=config["host"], port=int(config["port"]), database=config["database"],
                  runtime_principal=config["admin_user"], password_file=config["admin_password_file"],
                  ca_file=config["ca_file"])
    a.out.mkdir(mode=0o700, exist_ok=False)

    def put(name, value):
        p = a.out / name
        p.write_text(json.dumps(value, indent=2) + "\n")
        p.chmod(0o600)
        return p

    catalog_entries = []
    expected = []
    for i, (entry, native) in enumerate(zip(request["entries"], request["native_requests"], strict=True)):
        schema, table = entry["schema"], entry["table"]
        match = [x for x in loaded["entries"] if x["schema"] == schema and x["table"] == table]
        assert len(match) == 1 and native["logical_schema"] == schema and native["logical_table"] == table
        assert sha(entry["admission_registry_path"]) == entry["admission_registry_sha256"]
        catalog_request = dict(protocol="oo.pipeline.native-sealed-catalog/v1",
            run_id=match[0]["run_id"], source_id=loaded["source_id"],
            owner=loaded["source_instance"]["owner_token"],
            source_generation=loaded["source_generation"], target_generation=loaded["target_generation"],
            snapshot=native["snapshot"], target=target,
            **{key: native[key] for key in ["config_digest", "schema_digest", "type_digest", "input_digest"]})
        p = put(f"catalog-request-{i}.json", catalog_request)
        catalog_entries.append(dict(request=str(p), registry=entry["admission_registry_path"],
                                    registry_sha256=entry["admission_registry_sha256"]))
        expected.append([match[0].get("storage_schema", schema), match[0].get("storage_table", table)])
    put("collection-operator.json", dict(protocol="oo.pipeline.native-collection-operator/v1",
        pipeline=runtime["pins"]["loader"], admission_binary=runtime["pins"]["admission"]["path"],
        entries=catalog_entries, expected_datasets=expected))
    put("publication-request.json", dict(protocol="oo.pipeline.native-c1-publication/v1",
        operation="publish", source_id=loaded["source_id"], task_id=ack["receipt"]["source_instance"]["lease_id"],
        request_digest=ack["request_digest"], target=target))
    put("request-provenance.json", dict(
        files={name: sha(a.artifacts / f"{name}.json") for name in ["request", "load", "ack"]},
        receipts_created=False, database_contacted=False, publication_cli=publisher))
    print(a.out)


if __name__ == "__main__":
    main()
