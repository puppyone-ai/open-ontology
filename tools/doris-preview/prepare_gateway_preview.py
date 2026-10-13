#!/usr/bin/env python3
"""Prepare a source-bound, unpublished Doris Gateway preview from sealed artifacts.

This creates private operator inputs and exact-column SQL. It does not execute
the SQL, issue a token, or publish a Pack.
"""

import hashlib
import json
import os
from pathlib import Path
import secrets
import sys


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


def write(path, value, mode=0o600):
    path.parent.mkdir(parents=True, exist_ok=True)
    data = value if isinstance(value, bytes) else (json.dumps(value, indent=2, ensure_ascii=False) + "\n").encode()
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, mode)
    with os.fdopen(fd, "wb") as output:
        output.write(data)


def quote(value):
    if not value or not all(c.islower() or c.isdigit() or c == "_" for c in value):
        raise ValueError("unsafe SQL identifier")
    return '"' + value + '"'


def main():
    if len(sys.argv) != 4:
        raise SystemExit("usage: prepare_gateway_preview.py MAINTENANCE_DIR COLLECTION_JSON OUTPUT_DIR")
    source = Path(sys.argv[1]); collection = json.loads(Path(sys.argv[2]).read_text()); out = Path(sys.argv[3])
    command = json.loads((source / "command.json").read_text())
    reviewed = json.loads((source / "reviewed.json").read_text())
    effective = json.loads((source / "effective.json").read_text())
    visible = json.loads((source / "visibility.json").read_text())
    pack = json.loads((source / "role-pack.json").read_text())
    vector = collection["binding_vector"]
    assert vector == command["catalog"]["binding_vector"]
    allowed = {(x["dataset"], col) for x in visible["datasets"] for col in x["columns"]}
    approved = {(x["dataset"], x["column"]) for x in command["decisions"] if x["pii"] == "none"}
    assert allowed <= approved and len(allowed) == 18
    assert approved - allowed <= {("oo_dl01w1_43.part_day", "amount"), ("oo_dl01w1_43.uniq_acct", "balance")}
    if effective["ontology_version"].endswith("unspecified-unit"):
        columns = {(dataset["dataset"], column["name"]): column
                   for dataset in effective["datasets"] for column in dataset["columns"]}
        for key in (("oo_dl01w1_43.part_day", "amount"), ("oo_dl01w1_43.uniq_acct", "balance")):
            column = columns[key]
            if (column["semantic_type"], column["unit"], column["pii"], column["classification"], column["provenance"]) != ("money", "unspecified", None, {"at": None, "by": None, "state": "pending"}, "declared"):
                raise ValueError(f"source money declaration lost in effective Ontology: {key}")
    assert {x["dataset"] for x in vector} == {x[0] for x in allowed}
    role = visible["role"]; login = "oo_43_target_ro"
    assert role == "oo_r_dorispreview"
    settings = command["settings"]; cv = settings["catalog_version"]
    binding = {
        "allow": [{"dataset": d, "column": c, "explicit_allow": False} for d, c in sorted(allowed)],
        "artifact_sha256": hashlib.sha256(canonical(command["catalog"])).hexdigest(),
        "binding_vector": vector,
        "catalog_version": cv,
        "config_digest": vector[0]["config_digest"],
        "generation": max(x["target_generation"] for x in vector),
        "knowledge_id": settings["knowledge"]["id"],
        "knowledge_version": settings["knowledge"]["version"],
        "login": login,
        "ontology_version": settings["ontology_version"],
        "policy_id": settings["policy"]["id"],
        "policy_version": settings["policy"]["version"],
        "project": settings["project"],
        "role": "dorispreview",
        "schema": "oo_dl01w1_43",
        "schema_hash": cv,
        "source_id": "dorispreview",
    }
    rd = out / "reviewed" / role
    write(rd / "native-input.json", command)
    write(rd / "command.json", reviewed)
    write(rd / "binding.json", binding)
    pd = out / "packs"
    write(pd / (role + ".json"), pack)
    for item in pack["files"]:
        data = item["text"].encode()
        assert hashlib.sha256(data).hexdigest() == item["sha256"]
    # Map only approved logical columns to the sealed physical columns.
    tables = {}
    for entry in collection["entries"]:
        for table in entry["tables"]:
            dataset = table["logical_schema"] + "." + table["logical_table"]
            if dataset not in {x["dataset"] for x in vector}:
                continue
            physical = next(x for x in vector if x["dataset"] == dataset)["stored_physical"]
            columns = [x["stored_physical"] for x in table["columns"] if (dataset, x["logical_name"]) in allowed]
            if columns:
                tables[dataset] = (physical, columns)
    assert len(tables) == 7 and sum(len(x[1]) for x in tables.values()) == 18
    statements = ["BEGIN", f"CREATE ROLE {quote(role)} NOLOGIN NOINHERIT", f"GRANT {quote(role)} TO {quote(login)} WITH INHERIT FALSE, SET TRUE", f"GRANT USAGE ON SCHEMA _oo TO {quote(role)}"]
    for dataset, (physical, columns) in sorted(tables.items()):
        statements.append(f"GRANT SELECT ({','.join(quote(x) for x in columns)}) ON TABLE _oo.{quote(physical)} TO {quote(role)}")
    for table in ("full_import_run", "full_import_table", "table_freshness", "physical_mappings", "full_import_quality", "table_quality", "source_watermark"):
        statements.append(f"GRANT SELECT ON TABLE _oo.{quote(table)} TO {quote(login)}")
    statements += ["COMMIT"]
    write(out / "data-acl.sql", (";\n".join(statements) + ";\n").encode())
    write(out / "binding-receipt.json", {"artifact_sha256": binding["artifact_sha256"], "collection_sha256": collection["collection_sha256"], "allow_columns": 18, "tables": 7, "role_pack_digest": pack["pack_digest"]})
    print(json.dumps({"prepared": True, "role": role, "columns": 18, "tables": 7, "binding_sha256": hashlib.sha256((rd / "binding.json").read_bytes()).hexdigest()}))


if __name__ == "__main__":
    main()
