#!/usr/bin/env python3
"""Opt-in complete-source handoff fixture on the existing owned MySQL/PG pair."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import uuid

import mysql_c5_handoff_owned as r


def publish_full(e, out, loaded):
    c = r.read(e / "context.json")
    receipt = json.loads(r.pg(out, "publication-vector", f"""
SELECT jsonb_build_object('protocol','oo.pipeline.native-publication-receipt/v1','run_id',r.run_id,
 'source_id',r.source_id,'source_generation',r.source_generation,'target_generation',r.target_generation,
 'position_codec',w.position_codec,'applied_pos',w.applied_pos::text,'consistency',w.consistency,
 'consistency_grade',w.consistency_grade,'cross_table_consistent',w.cross_table_consistent,
 'tables',jsonb_build_array(jsonb_build_object('schema',t.schema_ident,'table',t.table_ident,
 'physical_name',t.physical_name,'accepted_rows',t.applied_rows,'canonical_digest',t.canonical_digest,
 'source_generation',r.source_generation,'target_generation',r.target_generation,'applied_pos',t.applied_pos::text)))
FROM _oo.full_import_run r JOIN _oo.full_import_table t USING(run_id)
JOIN _oo.source_watermark w ON w.source=r.source_id WHERE r.run_id='{loaded["run_id"]}';
""", c["database"]))
    lineage = c["source_id"] + "." + receipt["tables"][0]["table"]
    receipt.update(collection_mode="per_table", run_ids=[loaded["run_id"]], position_lineage=lineage)
    for table in receipt["tables"]:
        table.update(run_id=loaded["run_id"], rejects=[], capture_method="c5_diff",
                     delete_detection="reconcile", consistency_grade=receipt["consistency_grade"],
                     position_lineage=lineage)
    prior = json.loads(r.pg(out, "retained-published-members", f"""
SELECT coalesce(jsonb_agg(e ORDER BY f.schema_name,f.table_name),'[]'::jsonb)
FROM _oo.table_freshness f JOIN _oo.full_import_run r ON r.source_id=f.source
 AND r.source_generation=f.source_generation AND r.target_generation=f.target_generation
JOIN _oo.full_import_table t ON t.run_id=r.run_id AND t.physical_name=f.physical_name
JOIN _oo.native_publication_receipt p ON p.run_id=r.run_id
CROSS JOIN LATERAL jsonb_array_elements(p.table_vector) e
WHERE f.source='{c["source_id"]}' AND r.state='published'
 AND e->>'run_id'=r.run_id::text AND e->>'schema'=f.schema_name AND e->>'table'=f.table_name;
""", c["database"]))
    receipt["tables"] = sorted(prior + receipt["tables"], key=lambda t: (t["schema"], t["table"]))
    receipt["run_ids"] = [table["run_id"] for table in receipt["tables"]]
    encoded = json.dumps(receipt, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    r.save(out / "publication-request.json", receipt)
    digest = hashlib.sha256(encoded.encode()).hexdigest()
    result = r.pg(out, "publish-normal-api",
                  f"SELECT _oo.publish_native_per_table_collection('{loaded['run_id']}','{digest}',"
                  + "'" + encoded.replace("'", "''") + "'::jsonb);", c["database"])
    assert json.loads(result)["outcome"] == "published"


def full_second(e):
    c = r.read(e / "context.json")
    out = e / "full-second"
    out.mkdir()
    schema = c["schema"]
    gold = [[str(i), "second-" + str(i), str(-i * 11)] for i in range(1, 10)]
    r.save(out / "gold-before-seed.json", gold)
    sql = f"CREATE TABLE {schema}.readings_b LIKE {schema}.readings;\nSTART TRANSACTION;INSERT INTO {schema}.readings_b VALUES "
    sql += ",".join(f"({row[0]},'{row[1]}',{row[2]})" for row in gold) + ";COMMIT;"
    r.save(out / "SEED-ONCE.json", dict(sha256=hashlib.sha256(sql.encode()).hexdigest(), rows=9))
    r.source(out, "create-and-seed-owned-table-once", sql)
    binding, project = str(uuid.uuid4()), str(uuid.uuid4())
    capture = r.read(e / "private/capture.json")
    capture["table"]["table"] = "readings_b"
    capture["approved_objects"] = [capture["table"]]
    r.save(out / "capture.json", capture)
    producer = r.read(e / "private/c5-producer.json")
    producer.update(capture_config=str(out / "capture.json"), table="readings_b",
                    state_dir=str(out / "capture-state"))
    r.save(out / "producer.json", producer)
    manifest = r.read(e / "private/c5-manifest.json")
    manifest.update(binding=binding, project_id=project)
    manifest["producer"]["args"] = ["--config", str(out / "producer.json")]
    r.save(out / "manifest.json", manifest)
    r.worker(c, out, "put", "native-put", "--manifest", out / "manifest.json")
    r.worker(c, out, "fire", "native-fire", "--project", project)
    r.worker(c, out, "step", "native-step", "--project", project, "--worker", "owned-complete-c5")
    r.worker(c, out, "disable", "native-enable", "--binding", binding, "--enabled", "false")
    exported = json.loads(r.pg(out, "confirmed-artifacts",
        f"SELECT jsonb_build_object('request',o.request,'load',w.result) FROM jw_native.watermarks w "
        f"JOIN jw_native.occurrences o ON o.task_id=w.task_id JOIN jobs.tasks j ON j.id=w.task_id "
        f"WHERE w.binding_id='{binding}' AND j.state='succeeded';", c["database"]))
    r.save(out / "artifacts.json", exported)
    loaded = exported["load"]
    assert loaded["source_generation"] == 2 and loaded["source_bound"] is True
    r.audit(e, out, "actual-target", loaded, gold)
    publish_full(e, out, loaded)
    r.audit(e, out, "first-table-still-readable", r.read(e / "c5-finish/artifacts/load.json"),
            r.read(e / "gold-c5.json"))
    r.save(out / "receipt.json", dict(passed=True, binding=binding, project=project,
        source_id=c["source_id"], generation=2, table="readings_b", rows=9, differences=0,
        original_table_rows=8, source_writes_owned_only=True, handoff_performed=False))

def finish_second(e):
    c = r.read(e / "context.json")
    failed = e / "full-second"
    out = e / "full-second-publication"
    out.mkdir()
    loaded = r.read(failed / "artifacts.json")["load"]
    publish_full(e, out, loaded)
    r.audit(e, out, "second", loaded, r.read(failed / "gold-before-seed.json"))
    r.audit(e, out, "first", r.read(e / "c5-finish/artifacts/load.json"), r.read(e / "gold-c5.json"))
    r.save(out / "receipt.json", dict(passed=True, tables=2, rows=17, differences=0,
        source_id=c["source_id"], source_generation=2, source_calls=0,
        original_incomplete_publication_refused=True, original_failure_retained=True))

def scope_tests(e, attempt):
    c = r.read(e / "context.json")
    assert attempt.startswith("scope-dry-tests") and attempt.replace("-", "").isalnum()
    out = e / attempt
    out.mkdir()
    second = r.read(e / "full-second/manifest.json")
    source, binding, project = c["source_id"], second["binding"], second["project_id"]
    before_sql = f"SELECT jsonb_build_object('watermark',(SELECT to_jsonb(w) FROM _oo.source_watermark w WHERE source='{source}'),'bindings',(SELECT jsonb_agg(to_jsonb(b) ORDER BY id) FROM jw_native.bindings b WHERE source_id='{source}'));"
    before = r.pg(out, "before", before_sql, c["database"])
    migration = r.P / "crates/oo-pipeline/migrations/0115_native_mysql_complete_handoff.sql"
    program = "BEGIN;SET LOCAL ROLE oo_target_owner;\n" + migration.read_text()
    program += f"""
RESET ROLE;
CREATE FUNCTION pg_temp.expect_refused(command text,reason text) RETURNS void LANGUAGE plpgsql AS $test$
DECLARE caught boolean:=false;
BEGIN
 BEGIN EXECUTE command;
 EXCEPTION WHEN OTHERS THEN
  IF position(reason IN SQLERRM)=0 THEN RAISE; END IF;
  caught:=true;
 END;
 IF NOT caught THEN RAISE EXCEPTION 'expected refusal: %',reason; END IF;
END $test$;
SET LOCAL ROLE {c["runtime"]};
SELECT _oo.mysql_handoff_inspect('{source}','{binding}','{project}');
RESET ROLE;
SELECT pg_temp.expect_refused(
 'SELECT _oo.mysql_handoff_inspect(''{source}'',''{c["binding"]}'',''{c["project"]}'')',
 'mysql_handoff_stop_drain_published_required');
SAVEPOINT disabled;
UPDATE jw_native.bindings SET enabled=true WHERE id='{c["binding"]}';
SELECT pg_temp.expect_refused('SELECT _oo.mysql_handoff_complete_scope(''{source}'',''{binding}'')',
 'mysql_handoff_stop_complete_source_required');
ROLLBACK TO disabled;
SAVEPOINT members;
DELETE FROM _oo.table_freshness WHERE source='{source}' AND table_name='readings';
SELECT pg_temp.expect_refused('SELECT _oo.mysql_handoff_complete_scope(''{source}'',''{binding}'')',
 'mysql_handoff_exact_published_membership_required');
ROLLBACK TO members;
SAVEPOINT state;
UPDATE _oo.table_freshness SET state='awaiting_publish' WHERE source='{source}' AND table_name='readings';
SELECT pg_temp.expect_refused('SELECT _oo.mysql_handoff_complete_scope(''{source}'',''{binding}'')',
 'mysql_handoff_exact_published_membership_required');
ROLLBACK TO state;
SAVEPOINT projection;
UPDATE _oo.table_freshness SET physical_name='pt_unmapped_test' WHERE source='{source}' AND table_name='readings';
SELECT pg_temp.expect_refused('SELECT _oo.mysql_handoff_complete_scope(''{source}'',''{binding}'')',
 'mysql_handoff_complete_source_required');
ROLLBACK TO projection;
SELECT pg_temp.expect_refused(
 'SELECT _oo.mysql_handoff_complete_scope(''{source}'',''{binding}'',''11111111-1111-4111-8111-111111111111'')',
 'mysql_handoff_exact_staged_membership_required');
SELECT jsonb_build_object('bindings',jsonb_array_length(_oo.mysql_handoff_members('{source}')),
 'tables',jsonb_array_length(_oo.mysql_handoff_tables('{source}')),'negative_cases',6);
ROLLBACK;
"""
    (out / "probe.sql").write_text(program)
    process = r.call(out, "scope-contract", r.pg_argv(c["database"]), program,
                     check=False, env=r.pg_env())
    assert process.returncode == 0, process.stderr
    result = process.stdout
    values = [json.loads(line) for line in result.splitlines() if line.startswith("{")]
    assert values[-1] == dict(bindings=2, tables=2, negative_cases=6)
    assert len(values[0]["tables"]) == 2 and len(values[0]["bindings"]) == 2
    assert all(table["columns"] == ["id", "label", "qty"] for table in values[0]["tables"])
    after = r.pg(out, "after", before_sql, c["database"])
    assert before == after
    r.save(out / "receipt.json", dict(passed=True, tables=2, bindings=2, negatives=6,
        real_c5_baseline=True, migration=r.pin(migration), rolled_back=True,
        migration_installed=False, source_calls=0, customer_touched=False,
        original_state_unchanged=True, c1_capture_accepted=False))

def layered(e, table, attempt=None):
    c = r.read(e / "context.json")
    if attempt is not None:
        assert attempt.startswith("layered-") and attempt.replace("-", "").replace("_", "").isalnum()
    out = e / (attempt or "layered-" + table)
    out.mkdir()
    schema, source = c["schema"], c["source_id"]
    original = (r.read(e / "c5-finish/artifacts/request.json") if table == "readings"
                else r.read(e / "full-second/artifacts.json")["request"])
    gold = r.read(e / "gold-c5.json" if table == "readings" else e / "full-second/gold-before-seed.json")
    columns = [json.loads(line) for line in r.source(out, "source-column-metadata",
        f"SELECT JSON_ARRAY(COLUMN_NAME,COLUMN_TYPE,COLLATION_NAME,IS_NULLABLE) "
        f"FROM information_schema.COLUMNS WHERE TABLE_SCHEMA='{schema}' AND TABLE_NAME='{table}' "
        "ORDER BY ORDINAL_POSITION;").splitlines()]
    keys = [dict(name="id", kind="Int", collation=None)]
    fingerprint = hashlib.sha256(json.dumps(dict(columns=columns, keys=keys),
        sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    producer = r.read(e / "private/c5-producer.json")
    connection = r.read(e / "private/capture.json")["connection"]
    config = dict(source_id=source, schema=schema, table=table, columns=["id", "label", "qty"],
        secret_ref=producer["secret_ref"], connection=connection, target=producer["target"],
        state_dir=str(out / "capture-state"), authority_file=str(out / "authority.json"),
        max_rows=100, max_bytes=1048576, incremental_only=True,
        baseline_arrows={"0": original["arrow_path"]}, audit=dict(mode="grant_enforced"))
    inspection_config = out / "inspect.json"
    r.save(inspection_config, dict(connection=connection, schema=schema, table=table,
        secret_ref=producer["secret_ref"], columns=["id", "label", "qty"], server_timeout_ms=5000, client_timeout_ms=10000,
        deadline_ms=60000))
    inspected = json.loads(r.call(out, "inspect", [c["bins"]["mysql-preview"]["path"],
        "c5-inspect", "--config", inspection_config]).stdout)
    assert inspected["metadata_only"] and inspected["business_rows_read"] == 0
    fingerprint = inspected["schema_fingerprint"]
    config.update(schema_fingerprint_codec=inspected["schema_fingerprint_codec"],
                  baseline_keys=inspected["source_key_columns"])
    r.save(out / "layered.json", config)
    adapter_source = r.P / "tools/mysql-preview/layered-producer.py"
    adapter = out / "layered-producer.py"
    shutil.copy2(adapter_source, adapter)
    producer.update(layered_config=str(out / "layered.json"), state_dir=str(out / "producer-state"),
                    max_arrow_bytes=1048576)
    producer.pop("capture_config")
    producer.pop("table")
    r.save(out / "producer.json", producer)
    binding, project = str(uuid.uuid4()), str(uuid.uuid4())
    manifest = r.read(e / "private/c5-manifest.json")
    manifest.update(binding=binding, project_id=project, c5_mode="layered",
        producer=dict(**r.pin(adapter), args=["--config", str(out / "producer.json")]),
        c5_layered=dict(policy=dict(period_ms=[1000, 1000, 1000, 86400000],
            duty_per_mille=1000, threads_running_max=32, execution_window_utc=[0, 1440]),
            incremental_only=True, window_ms=86400000, overlap_ms=600000, recent_column=None,
            server_timeout_ms=5000, client_timeout_ms=10000, execution_timezone="UTC"),
        c5_state=dict(schema=schema, table=table, schema_fingerprint=fingerprint,
            key_columns=["id"], authority_file=config["authority_file"], target=producer["target"],
            initializer="published", baseline_parts={"0": original["arrow_path"]}))
    r.save(out / "manifest.json", manifest)
    r.call(out, "install-c5-parts", [c["bins"]["oo-pipeline"]["path"], "install-native-c5-range-parts"],
        json.dumps(dict(protocol="oo.pipeline.install-native-update/v1", target=c["target"])))
    r.call(out, "install-observer", [c["bins"]["oo-pipeline"]["path"], "install-native-c5-privileged-observer"],
        json.dumps(dict(protocol="oo.pipeline.install-native-update/v1", target=c["target"])))
    if table == "readings_b":
        gold[0][2] = "-111"
        sql = f"UPDATE {schema}.readings_b SET qty=-111 WHERE id=1;"
        r.save(out / "MUTATE-ONCE.json", dict(sql=sql, expected=gold))
        r.source(out, "source-mutate-once", sql)
    r.save(out / "expected-gold.json", gold)
    r.worker(c, out, "put", "native-put", "--manifest", out / "manifest.json")
    r.worker(c, out, "fire", "native-fire", "--project", project)
    result = r.worker(c, out, "step", "native-step", "--project", project, "--worker", "owned-layered-c5")
    r.worker(c, out, "disable", "native-enable", "--binding", binding, "--enabled", "false")
    expected = "complete=nochange" if table == "readings" else "complete=succeeded"
    assert expected in result.stdout, result.stdout
    loaded = json.loads(r.pg(out, "current-published-or-pending-table", f"""
SELECT jsonb_build_object('run_id',r.run_id,'physical_name',t.physical_name,'source_generation',r.source_generation)
FROM _oo.table_freshness f JOIN _oo.full_import_run r ON r.source_id=f.source
 AND r.source_generation=f.source_generation AND r.target_generation=f.target_generation
JOIN _oo.full_import_table t ON t.run_id=r.run_id AND t.physical_name=f.physical_name
WHERE f.source='{source}' AND f.table_name='{table}';
""", c["database"]))
    if table == "readings_b":
        assert loaded["source_generation"] == 3
        publish_full(e, out, loaded)
    r.audit(e, out, "target", loaded, gold)
    r.save(out / "receipt.json", dict(passed=True, source_id=source, binding=binding,
        project=project, table=table, source_generation=loaded["source_generation"],
        nochange=table == "readings", rows=len(gold), differences=0,
        baseline_reimported=False, customer_touched=False))

def candidate(e):
    c = r.read(e / "context.json")
    out = e / "candidate"
    out.mkdir()
    anchor = r.read(e / "layered-readings_b/manifest.json")
    config = r.read(e / "private/c1-config.json")
    tables = [c["schema"] + "." + t for t in ["readings", "readings_b"]]
    config.update(tables=tables, columns={name: ["id", "label", "qty"] for name in tables},
                  c1_initial_resume={"protocol": "oo.mysql.initial-resume/v1"})
    config["c5_to_c1"].update(binding=anchor["binding"], project_id=anchor["project_id"])
    r.save(out / "config.json", config)
    manifest = r.read(e / "private/c1-manifest.json")
    manifest.update(binding=anchor["binding"], project_id=anchor["project_id"])
    manifest["producer"]["args"][2] = str(out / "config.json")
    r.save(out / "manifest.json", manifest)
    snapshot = json.loads(r.pg(out, "scope", f"SELECT _oo.mysql_handoff_inspect("
        f"'{c['source_id']}','{anchor['binding']}','{anchor['project_id']}');", c["database"]))
    assert len(snapshot["bindings"]) == 4 and len(snapshot["tables"]) == 2
    r.save(out / "scope.json", snapshot)
    authorities = {}
    for table in ["readings", "readings_b"]:
        authorities[c["schema"] + "." + table] = json.loads(r.pg(out, "authority-" + table,
            f"SELECT _oo.c5_authority_export('{c['source_id']}','{c['schema']}','{table}');", c["database"]))
    r.save(out / "authorities.json", authorities)
    r.call(out, "plan", [c["bins"]["source-onboarding"]["path"], "mysql-handoff", "plan",
        "--worker", c["bins"]["job-worker"]["path"], "--config", c["worker_config"],
        "--manifest", out / "manifest.json", "--out", out / "plan.json"])

def next_layered(e):
    c = r.read(e / "context.json")
    out = e / "layered-next"
    out.mkdir()
    manifest = r.read(e / "layered-readings_b/manifest.json")
    gold = r.read(e / "layered-readings_b/expected-gold.json")
    gold[0][2] = "-112"
    sql = f"UPDATE {c['schema']}.readings_b SET qty=-112 WHERE id=1;"
    r.save(out / "SOURCE-UPDATE-ONCE.json", dict(sql=sql, expected=gold))
    r.source(out, "source-update-once", sql)
    r.worker(c, out, "enable", "native-enable", "--binding", manifest["binding"], "--enabled", "true")
    r.worker(c, out, "fire", "native-fire", "--project", manifest["project_id"])
    r.worker(c, out, "disable", "native-enable", "--binding", manifest["binding"], "--enabled", "false")
    result = r.worker(c, out, "step", "native-step", "--project", manifest["project_id"],
                     "--worker", "owned-layered-next")
    assert "complete=succeeded" in result.stdout
    loaded = json.loads(r.pg(out, "artifacts", f"SELECT result FROM jw_native.watermarks "
        f"WHERE binding_id='{manifest['binding']}';", c["database"]))
    r.save(out / "load.json", loaded)
    assert loaded["source_generation"] == 4
    publish_full(e, out, loaded)
    r.audit(e, out, "current", loaded, gold)
    r.save(out / "gold.json", gold)
    r.save(out / "receipt.json", dict(passed=True, source_generation=4,
        target_generation=loaded["target_generation"], rows=9, differences=0,
        new_real_cycle=True, previous_publication_retained=True,
        canonical_source_receipt=True, no_source_recapture=True))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["bootstrap", "first", "second", "finish-second",
                                          "scope-tests", "layer-first", "layer-second", "candidate", "next-layered"])
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--ca-file", type=Path)
    parser.add_argument("--attempt")
    args = parser.parse_args()
    if args.action == "bootstrap":
        r.bootstrap(args.out, ca_file=args.ca_file)
    elif args.action == "first":
        r.c5(args.out)
    elif args.action == "second":
        full_second(args.out)
    elif args.action == "finish-second":
        finish_second(args.out)
    elif args.action == "scope-tests":
        scope_tests(args.out, args.attempt or "scope-dry-tests")
    elif args.action == "candidate":
        candidate(args.out)
    elif args.action == "next-layered":
        next_layered(args.out)
    else:
        layered(args.out, "readings" if args.action == "layer-first" else "readings_b", args.attempt)


if __name__ == "__main__":
    main()
