#!/usr/bin/env python3
"""Materialize historical g1 catalog requests from actual sealed target run identities."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[3] / "evidence/ad13411e-d04e-4c91-8a20-49caf867aa23/DL01/postgres-product/linux/runner"
OUT = ROOT / "out/native-full-20261013"
rows = subprocess.check_output([
    "docker", "exec", "oo-dl01w1-46-native", "psql", "-U", "postgres", "-d", "dst_native_r2", "-F", "|", "-Atc",
    "select t.schema_ident,t.table_ident,r.run_id,r.target_generation,r.source_id,r.config_digest,r.schema_digest,r.type_digest,r.input_digest from _oo.full_import_run r join _oo.full_import_table t using(run_id) where r.source_generation=1 order by t.schema_ident,t.table_ident",
], text=True).splitlines()
entries = []
expected = []
for row in rows:
    schema, table, run_id, target_generation, source_id, config_digest, schema_digest, type_digest, input_digest = row.split("|")
    dataset = f"{schema}.{table}"
    binding = {"public.hubs": "4611", "public.shipments": "4612", "public.tracking_events": "4613", "customer_east.delivery_notes": "4614"}[dataset]
    pin = 2 if dataset in ("public.shipments", "public.tracking_events") else 1
    registry = ROOT / f"out/pg-native-full-operator/00000000-0000-4000-8000-00000000{binding}/{pin}/registry.json"
    receipt = OUT / f"native-admit/{dataset}/g1.capture.json"
    snapshot = json.loads(receipt.read_text())["snapshot"]
    request = {
        "protocol": "oo.pipeline.native-sealed-catalog/v1", "run_id": run_id,
        "source_id": source_id, "owner": "00000000-0000-4000-8000-000000000046",
        "source_generation": 1, "target_generation": int(target_generation),
        "config_digest": config_digest, "schema_digest": schema_digest,
        "type_digest": type_digest, "input_digest": input_digest,
        "snapshot": snapshot,
        "target": {"host": "oo-dl01w1-46-native", "port": 5432, "database": "dst_native_r2",
                   "runtime_principal": "oo_dl01w1_46_loader", "password_file": "/run/secrets/target",
                   "ca_file": "/work/ca-renewed.pem"},
    }
    reqpath = OUT / f"catalog-request-baseline-{dataset}.json"
    reqpath.write_text(json.dumps(request, separators=(",", ":")) + "\n")
    entries.append({"request": f"/work-out/native-full-20261013/{reqpath.name}",
                    "registry": f"/work-out/pg-native-full-operator/00000000-0000-4000-8000-00000000{binding}/{pin}/registry.json",
                    "registry_sha256": hashlib.sha256(registry.read_bytes()).hexdigest()})
    expected.append([schema, table])
assert len(entries) == 4
operator = {"protocol": "oo.pipeline.native-collection-operator/v1",
            "pipeline": {"path": "/opt/oo-pipeline-e85f9da6", "sha256": "e85f9da679219aa6625a49d45aee62d8578b69299cae0a3503e5bec86f7b15ac"},
            "admission_binary": "/opt/oo-source-admission-cfc11a13d",
            "entries": entries, "expected_datasets": expected}
path = ROOT / "work/collection-operator-baseline-r2.json"
path.write_text(json.dumps(operator, separators=(",", ":")) + "\n")
path.chmod(0o600)
print(path)
