#!/usr/bin/env python3
"""One owned frozen Oracle snapshot -> existing C5 worker/Loader/publication."""
import importlib.util
import json
import os
from pathlib import Path
import secrets
import shutil
import sqlite3
import subprocess
import sys
import uuid

spec = importlib.util.spec_from_file_location("owned_fixture", Path(__file__).with_name("run.py"))
f = importlib.util.module_from_spec(spec)
spec.loader.exec_module(f)


def main():
    request_path = Path(os.environ["OO_ORACLE_SHARED_REQUEST"])
    shared = json.loads(request_path.read_text())
    source = shared["source_id"]
    assert source == "oracle_frozen_iud_20261027"
    bundle_ref = shared["c5_arrow_bundle"]
    assert f.sha(bundle_ref["path"]) == bundle_ref["sha256"]
    bundle = json.loads(Path(bundle_ref["path"]).read_text())
    assert bundle["source_id"] == source and bundle["final_rows"] == 10000
    runtime, database = "orc5_" + f.TAG, "oracle_c5_" + f.TAG
    password = secrets.token_hex(24)
    f.REDACTIONS.append(password)
    password_file = f.private("runtime.pw", password)
    f.pg("ready", "SHOW listen_addresses")
    f.pg("new-c5-target", f"CREATE ROLE {runtime} LOGIN INHERIT NOSUPERUSER PASSWORD '{password}'; CREATE DATABASE {database};")
    f.run("jobs", ["python3", f.ROOT / "product/crates/oo-jobs/tools/deploy.py",
                  "--container", f.PG, "--database", database, "--loader-role", runtime])
    pins = f.PINS
    for pin in pins.values():
        assert f.sha(pin["path"]) == pin["sha256"]
    loader, worker = pins["oo-pipeline"]["path"], pins["job-worker"]["path"]
    admin = dict(host="127.0.0.1", port=25487, database=database, runtime_principal="postgres",
                 password_file=str(f.HOME / "secrets/pg-super.pw"), ca_file=str(f.HOME / "certs/ca.pem"))
    for command in ["install-native-update", "install-native-mirror-rebind",
                    "install-native-claim-selected", "install-native-c1-typed"]:
        f.run(command, [loader, command], json.dumps(dict(protocol="oo.pipeline.install-native-update/v1", target=admin)).encode())
    f.pg("runtime-grant", f"GRANT oo_pipeline_runtime TO {runtime} WITH INHERIT TRUE, SET FALSE;", database)
    cfg = f.private("worker.cfg", "\n".join([
        "host=127.0.0.1", "port=25487", f"database={database}", f"runtime_user={runtime}",
        f"runtime_password_file={password_file}", "admin_user=postgres",
        f"admin_password_file={f.HOME / 'secrets/pg-super.pw'}", f"ca_file={f.HOME / 'certs/ca.pem'}",
        f"migrations={f.ROOT / 'product/crates/oo-jobs/migrations'}",
    ]) + "\n")
    binding, project = str(uuid.uuid4()), str(uuid.uuid4())
    target = dict(admin, runtime_principal=runtime, password_file=str(password_file), shard=source, owner=binding)
    source_config = json.loads(f.INPUT.read_text())
    report = source_config["admission_report"]
    report["capabilities"] = [dict(name="BatchFull", cursor=None, feed=None, cdc=None, token_order="None",
                                  budget_unit="Rows", level="experimental", write_class="R0", consistency="S0")]
    report["capture_methods"] = [dict(method="C5", write_class="R0", consistency="S0", token_order="None",
                                     retention_window=None, cursor=None, feed=None, cdc=None, delete_detection="reconcile")]
    report["cost_model"]["estimate"] = "closed SCN snapshot bundle, no source connection during target ingestion"
    materializer = f.ROOT / "product/crates/oo-pipeline/tools/oracle_snapshot_materialize.py"
    materializer_sha = f.sha(materializer)
    fixed = f.EV / "artifacts" / materializer_sha / materializer.name
    fixed.parent.mkdir(parents=True, exist_ok=True)
    if not fixed.exists():
        shutil.copy2(materializer, fixed)
    config = dict(protocol="oo.pipeline.oracle-snapshot-operator/v1", source_generation=1, binding=binding,
                  bundle=bundle_ref, source_driver=pins["oo-oracle-source"], admission=pins["oo-source-admission"],
                  admission_config=source_config["admission_config"], admission_report=report, target=target,
                  materializer_sha256=materializer_sha, permission_provenance=source_config["permission_provenance"])
    producer_config = f.private("snapshot-operator.json", json.dumps(config, indent=2) + "\n")
    transport = f.PRIVATE / "materialized"
    python = Path(sys.executable).resolve()
    manifest = dict(protocol="oo.jobs.native-schedule/v1", binding=binding, project_id=project, source_id=source,
        method="C5", cycle="pull", period_ms=1000,
        producer=dict(path=str(python), sha256=f.sha(python),
                      args=["-B", str(fixed), "--config", str(producer_config), "--out", str(transport)]),
        loader=dict(pins["oo-pipeline"], args=["import-native-update"]),
        registration=dict(admission_binary=pins["oo-source-admission"]["path"],
                          admission_sha256=pins["oo-source-admission"]["sha256"],
                          registry_path=str(transport / "{binding}/{pin}/sealed/registry.json")))
    manifest_path = f.private("native-manifest.json", json.dumps(manifest, indent=2) + "\n")
    f.save("context.json", dict(database=database, source_id=source, runtime=runtime, binding=binding,
        project=project, private_dir=str(f.PRIVATE), worker_config=str(cfg), manifest_path=str(manifest_path),
        target_admin=admin, source_mutated=False, method="C5", source_snapshot_scn=bundle["boundary"]["snapshot_scn"]))
    def w(name, *args, check=True):
        return f.run(name, [worker, "--config", cfg, *args], check=check)
    w("native-install", "native-install")
    w("native-put", "native-put", "--manifest", manifest_path)
    w("native-fire", "native-fire", "--project", project)
    step = w("native-step", "native-step", "--project", project, "--worker", "oracle-frozen-c5", check=False)
    w("disable", "native-enable", "--binding", binding, "--enabled", "false", check=False)
    w("state", "native-state", "--binding", binding, check=False)
    tasks = [json.loads(s) for s in f.pg("tasks", "SELECT row_to_json(x) FROM (SELECT id,state,generation FROM jobs.tasks) x", database).splitlines()]
    assert step.returncode == 0 and len(tasks) == 1 and tasks[0]["state"] == "succeeded", "real C5 worker refused"
    loaded = json.loads(f.pg("load", "SELECT result FROM jw_native.watermarks", database))
    request = json.loads(f.pg("request", "SELECT request FROM jw_native.occurrences", database))
    f.save("load.json", loaded)
    f.save("request.json", request)
    assert request["source_engine"] == "oracle" and request["source_generation"] == 1
    entry_root = transport / binding / "1"
    env = dict(os.environ, OO_NATIVE_ADMISSION_BIN=pins["oo-source-admission"]["path"],
               OO_NATIVE_BINDING_REGISTRY=str(entry_root / "sealed/registry.json"),
               OO_NATIVE_BINDING_REGISTRY_SHA256=f.sha(entry_root / "sealed/registry.json"))
    catalog_request = dict(protocol="oo.pipeline.native-sealed-catalog/v1", source_id=source,
        run_id=loaded["run_id"], owner=binding, source_generation=1, target_generation=loaded["target_generation"],
        snapshot=request["snapshot"], target=admin, **{k: request[k] for k in ["config_digest", "schema_digest", "type_digest", "input_digest"]})
    p = subprocess.run([loader, "catalog-import-native"], input=json.dumps(catalog_request).encode(), env=env, capture_output=True)
    (f.OUT / "catalog.json").write_bytes(p.stdout)
    (f.OUT / "catalog.stderr").write_bytes(p.stderr)
    f.save("catalog-command.json", dict(argv=[loader, "catalog-import-native"], exit=p.returncode))
    assert p.returncode == 0
    table = json.loads(f.pg("publication-table", """SELECT jsonb_build_object('schema',t.schema_ident,'table',t.table_ident,
 'run_id',r.run_id,'physical_name',t.physical_name,'accepted_rows',t.applied_rows,'canonical_digest',t.canonical_digest,
 'source_generation',r.source_generation,'target_generation',r.target_generation,'applied_pos',t.applied_pos::text,
 'capture_method','c5_diff','delete_detection','reconcile','consistency_grade','S0','rejects','[]'::jsonb)
 FROM _oo.full_import_run r JOIN _oo.full_import_table t USING(run_id);""", database))
    assert table["accepted_rows"] == 10000
    publication = dict(protocol="oo.pipeline.native-publication-receipt/v1", collection_mode="per_table",
        run_id=loaded["run_id"], run_ids=[loaded["run_id"]], source_id=source, source_generation=1,
        target_generation=loaded["target_generation"], position_codec="snapshot-zero/v1", position_lineage="oracle.snapshot",
        applied_pos="0", consistency="per_table", consistency_grade="S0", cross_table_consistent=False, tables=[table])
    raw = json.dumps(publication, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    import hashlib
    digest = hashlib.sha256(raw.encode()).hexdigest()
    f.save("publication-receipt.json", publication)
    sql = f"SELECT _oo.publish_native_per_table_collection('{loaded['run_id']}','{digest}','{raw.replace(chr(39), chr(39)*2)}'::jsonb);"
    published = json.loads(f.pg("publish", sql, database))
    assert published["outcome"] == "published"
    f.save("result.json", dict(real_jobs=True, task=tasks[0]["id"], state="succeeded", source_id=source, method="C5",
        source_generation=1, rows=10000, source_snapshot_scn=bundle["boundary"]["snapshot_scn"],
        publication=True, publication_sha256=digest, source_mutated=False, baseline_only=True,
        independent_audit="pending", pins=pins, helper=dict(path=str(fixed), sha256=materializer_sha)))
    print(f.OUT)


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        f.save("failure.json", {"error": str(exc)})
        print("FAILED", f.OUT, str(exc))
        raise SystemExit(1)
