#!/usr/bin/env python3
"""Build a four-table collection request from the latest actual native runs."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess


ROOT = Path(__file__).resolve().parents[3] / "evidence/ad13411e-d04e-4c91-8a20-49caf867aa23/DL01/postgres-product/linux/runner"
DATASETS = {
    "public.hubs": "4611",
    "public.shipments": "4612",
    "public.tracking_events": "4613",
    "customer_east.delivery_notes": "4614",
}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--pipeline", required=True)
    parser.add_argument("--visible-pipeline", required=True)
    parser.add_argument("--admission-binary", required=True)
    parser.add_argument("--output-name", default="collection-current")
    args = parser.parse_args()
    rows = subprocess.check_output([
        "docker", "exec", "oo-dl01w1-46-native", "psql", "-U", "postgres", "-d", "dst_native_r2", "-F", "|", "-Atc",
        "select distinct on (t.schema_ident,t.table_ident) "
        "t.schema_ident,t.table_ident,r.run_id,r.source_generation,r.target_generation,"
        "r.source_id,r.config_digest,r.schema_digest,r.type_digest,r.input_digest "
        "from _oo.full_import_run r join _oo.full_import_table t using(run_id) "
        "order by t.schema_ident,t.table_ident,r.target_generation desc",
    ], text=True).splitlines()
    out = ROOT / "out/native-full-20261013"
    entries = []
    for line in rows:
        schema, table, run, generation, target_generation, source, config, shape, types, content = line.split("|")
        dataset = f"{schema}.{table}"
        if dataset not in DATASETS:
            raise ValueError(f"unexpected dataset {dataset}")
        binding = DATASETS[dataset]
        registry_dir = ROOT / f"out/pg-native-full-operator/00000000-0000-4000-8000-00000000{binding}"
        registries = sorted(registry_dir.glob("*/registry.json"), key=lambda path: int(path.parent.name))
        if not registries:
            raise ValueError(f"missing registry {dataset}")
        registry = registries[-1]
        capture = json.loads((out / f"native-admit/{dataset}/g{generation}.capture.json").read_text())
        request = {
            "protocol": "oo.pipeline.native-sealed-catalog/v1", "run_id": run,
            "source_id": source, "owner": "00000000-0000-4000-8000-000000000046",
            "source_generation": int(generation), "target_generation": int(target_generation),
            "config_digest": config, "schema_digest": shape,
            "type_digest": types, "input_digest": content,
            "snapshot": capture["snapshot"],
            "target": {"host": "oo-dl01w1-46-native", "port": 5432, "database": "dst_native_r2",
                       "runtime_principal": "oo_dl01w1_46_loader", "password_file": "/run/secrets/target",
                       "ca_file": "/work/ca-renewed.pem"},
        }
        request_path = out / f"catalog-request-current-{dataset}.json"
        request_path.write_text(json.dumps(request, separators=(",", ":")) + "\n")
        entries.append({
            "request": f"/work-out/native-full-20261013/{request_path.name}",
            "registry": f"/work-out/pg-native-full-operator/00000000-0000-4000-8000-00000000{binding}/{registry.parent.name}/registry.json",
            "registry_sha256": hashlib.sha256(registry.read_bytes()).hexdigest(),
        })
    if len(entries) != 4 or len(rows) != 4:
        raise ValueError("incomplete four-table collection")
    operator = {
        "protocol": "oo.pipeline.native-collection-operator/v1",
        "pipeline": {"path": args.visible_pipeline, "sha256": hashlib.sha256(Path(args.pipeline).read_bytes()).hexdigest()},
        "admission_binary": args.admission_binary,
        "entries": entries,
        "expected_datasets": [dataset.split(".", 1) for dataset in DATASETS],
    }
    operator_path = ROOT / f"work/{args.output_name}-operator.json"
    operator_path.write_text(json.dumps(operator, separators=(",", ":")) + "\n")
    operator_path.chmod(0o600)
    print(operator_path)


if __name__ == "__main__":
    main()
