#!/usr/bin/env python3
"""Opt-in tiny owned MySQL C5 -> C1 chain; never touches a customer or old fixture."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import secrets
import shutil
import subprocess
import time
import uuid

P = Path(__file__).resolve().parents[3]
MYSQL = "oo-c1-grants-5728-1793240727"
PG = "oo-mysql-c1-20261027-pg17"
HOME = Path("/tmp/oo-mysql-c1-20261027")
os.umask(0o077)


def read(path):
    return json.loads(Path(path).read_text())


def save(path, value):
    with Path(path).open("x") as stream:
        json.dump(value, stream, indent=2)
        stream.write("\n")
        stream.flush()
        os.fsync(stream.fileno())


def pin(path):
    return dict(path=str(Path(path).resolve()), sha256=hashlib.sha256(Path(path).read_bytes()).hexdigest())


def call(out, name, argv, data=None, check=True, env=None):
    assert not (out / (name + ".command.json")).exists()
    started = time.monotonic()
    result = subprocess.run(list(map(str, argv)), input=data, text=True, capture_output=True,
                            timeout=180, env=env)
    (out / (name + ".stdout")).write_text(result.stdout)
    (out / (name + ".stderr")).write_text(result.stderr)
    save(out / (name + ".command.json"), dict(argv=list(map(str, argv)), exit=result.returncode,
                                             seconds=time.monotonic() - started))
    if check:
        assert result.returncode == 0, (name, result.stderr, result.stdout)
        if result.stdout.strip().startswith("{"):
            value = json.loads(result.stdout)
            assert not value.get("error") and value.get("outcome") != "refused", value
    return result


def pg_env():
    return dict(os.environ, PGPASSWORD=(HOME / "secrets/pg-super.pw").read_text().strip(),
                PGSSLMODE="verify-full", PGSSLROOTCERT=str(HOME / "certs/ca.pem"), PGCONNECT_TIMEOUT="5")


def pg_argv(database):
    return ["psql", "-X", "-h", "127.0.0.1", "-p", "25487", "-U", "postgres",
            "-d", database, "-Atq", "-v", "ON_ERROR_STOP=1"]


def pg(out, name, sql, database):
    return call(out, name, pg_argv(database), sql, env=pg_env()).stdout.strip()


def source(out, name, sql):
    return call(out, name, ["docker", "exec", "-i", MYSQL, "mysql",
        "--defaults-extra-file=/fixture/root.cnf", "-N", "-B", "--raw"], sql).stdout.strip()


def worker(c, out, name, *args, check=True):
    return call(out, name, [c["bins"]["job-worker"]["path"], "--config", c["worker_config"], *args], check=check)


def bootstrap(e, *, recovery=False, ca_file=None):
    assert shutil.disk_usage(e).free > 800_000_000
    bins = read(e / "binaries.json")
    assert all(pin(v["path"])["sha256"] == v["sha256"] for v in bins.values())
    out = e / "bootstrap"
    out.mkdir()
    private = e / "private"
    private.mkdir(mode=0o700)
    facts = json.loads(source(out, "source-metadata",
        "SELECT JSON_OBJECT('version',VERSION(),'uuid',@@GLOBAL.server_uuid,'log_bin',@@GLOBAL.log_bin,"
        "'gtid_mode',@@GLOBAL.gtid_mode,'format',@@GLOBAL.binlog_format,'image',@@GLOBAL.binlog_row_image);"))
    assert facts["version"].startswith("5.7.28") and facts["log_bin"] == 1
    port = int(call(out, "source-port", ["docker", "port", MYSQL, "3306/tcp"]).stdout.rsplit(":", 1)[1])
    tag = str(time.time_ns())
    scope = "hf_recovery_" if recovery else "handoff_"
    schema, database, source_id = scope + tag, scope + "target_" + tag, "mysql_" + scope + tag
    runtime, reader = "hf_rt_" + tag[-10:], "hf_ro_" + tag[-10:]
    binding, project = str(uuid.uuid4()), str(uuid.uuid4())
    pw = {name: secrets.token_hex(24) for name in ["runtime", "reader"]}
    for name, value in pw.items():
        (private / (name + ".pw")).write_text(value)
    target = dict(host="127.0.0.1", port=25487, database=database, runtime_principal="postgres",
                  password_file=str(HOME / "secrets/pg-super.pw"), ca_file=str(HOME / "certs/ca.pem"))
    cfg = private / "worker.cfg"
    cfg.write_text("\n".join(["host=127.0.0.1", "port=25487", f"database={database}",
        f"runtime_user={runtime}", f"runtime_password_file={private/'runtime.pw'}", "admin_user=postgres",
        f"admin_password_file={HOME/'secrets/pg-super.pw'}", f"ca_file={HOME/'certs/ca.pem'}",
        f"migrations={P/'crates/oo-jobs/migrations'}"]) + "\n")
    c = dict(database=database, schema=schema, source_id=source_id, binding=binding, project=project,
        runtime=runtime, reader=reader, worker_config=str(cfg), target=target, bins=bins,
        source_port=port, source_uuid=facts["uuid"])
    save(e / "context.json", c)
    save(e / "FIXTURE-INTENT.json", dict(source_container=MYSQL, target_container=PG,
        schema=schema, database=database, source_id=source_id, table="readings", rows=8,
        source_settings_changes=False, customer_touched=False))
    pg(out, "create-target", f"CREATE ROLE {runtime} LOGIN INHERIT NOSUPERUSER PASSWORD '{pw['runtime']}';"
       f"CREATE DATABASE {database};", "postgres")
    program = call(out, "jobs-program", ["python3", P / "crates/oo-jobs/tools/deploy.py",
        "--container", PG, "--database", database, "--loader-role", runtime, "--print-sql"]).stdout
    pg(out, "install-jobs", program, database)
    for command in ["install-native-update", "install-native-mirror-rebind", "install-native-claim-selected"]:
        call(out, command, [bins["oo-pipeline"]["path"], command],
             json.dumps(dict(protocol="oo.pipeline.install-native-update/v1", target=target)))
    pg(out, "grant-runtime", f"GRANT oo_pipeline_runtime TO {runtime} WITH INHERIT TRUE, SET FALSE;", database)
    worker(c, out, "native-install", "native-install")
    call(out, "typed-install", [bins["oo-pipeline"]["path"], "install-native-c1-typed"],
         json.dumps(dict(protocol="oo.pipeline.install-native-update/v1", target=target)))
    source(out, "create-only-owned-source", f"""
CREATE DATABASE {schema} CHARACTER SET utf8mb4;
CREATE USER '{reader}'@'%' IDENTIFIED BY '{pw['reader']}' REQUIRE SSL;
GRANT SELECT ON {schema}.* TO '{reader}'@'%';
GRANT SELECT ON performance_schema.events_statements_summary_by_user_by_event_name TO '{reader}'@'%';
GRANT REPLICATION CLIENT, REPLICATION SLAVE ON *.* TO '{reader}'@'%';
CREATE TABLE {schema}.readings(id BIGINT PRIMARY KEY,label VARCHAR(32),qty BIGINT NOT NULL) ENGINE=InnoDB;
""")
    gold = [[str(i), "seed-" + str(i), str(i * 10)] for i in range(1, 9)]
    save(e / "gold-c5.json", gold)
    seed = f"START TRANSACTION; INSERT INTO {schema}.readings VALUES " + ",".join(
        f"({r[0]},'{r[1]}',{r[2]})" for r in gold) + "; COMMIT;"
    save(e / "SEED-ONCE.json", dict(sql_sha256=hashlib.sha256(seed.encode()).hexdigest(), rows=8))
    source(out, "seed-once", seed)
    selected = dict(schema=schema, table="readings", columns=["id", "label", "qty"])
    ca = ca_file or e.parent / "mysql-c1-grants-20261029/attempt-02/private/ca.crt"
    capture = dict(protocol="oo.mysql-full/v1", source_id=source_id, generation=1, owner=binding,
        secret_ref="secret://owned-handoff/reader", connection=dict(host="127.0.0.1", port=port,
            database=schema, user=reader, secret_file=str(private / "reader.pw"), tls="required",
            ca_file=str(ca), plaintext_authorized=False), audit=dict(mode="grant_enforced"),
        approved_objects=[selected], table=selected, budgets=dict(connect_timeout_ms=5000,
            statement_timeout_ms=5000, deadline_ms=60000, max_rows=100, max_bytes=1048576))
    save(private / "capture.json", capture)
    adapter = e / "bin" / pin(P / "tools/mysql-preview/native-producer.py")["sha256"] / "native-producer.py"
    adapter.parent.mkdir(parents=True)
    shutil.copy2(P / "tools/mysql-preview/native-producer.py", adapter)
    target_runtime = dict(target, runtime_principal=runtime, password_file=str(private / "runtime.pw"),
                          shard=source_id, owner=binding)
    producer = dict(capture_config=str(private / "capture.json"), driver=bins["mysql-preview"],
        admission=bins["oo-source-admission"], state_dir=str(e / "c5-capture"), table="readings",
        prepare=str(P / "tools/mysql-preview/prepare-native.py"), secret_ref=capture["secret_ref"],
        source_audit_policy="grant_enforced", target=target_runtime,
        registry_template=str(e / "registered/{binding}/{pin}/registry.json"))
    save(private / "c5-producer.json", producer)
    manifest = dict(protocol="oo.jobs.native-schedule/v1", binding=binding, project_id=project,
        source_id=source_id, method="C5", cycle="pull", period_ms=1000,
        producer=dict(**pin(adapter), args=["--config", str(private / "c5-producer.json")]),
        loader=dict(**bins["oo-pipeline"], args=["import-native-update"]),
        registration=dict(admission_binary=bins["oo-source-admission"]["path"],
                          admission_sha256=bins["oo-source-admission"]["sha256"],
                          registry_path=producer["registry_template"]))
    save(private / "c5-manifest.json", manifest)
    native = dict(host="127.0.0.1", port=port, database=schema, user=reader,
        secret_ref=capture["secret_ref"], password_file=str(private / "reader.pw"), ca_file=str(ca),
        server_id=573030, health_every_ms=200, reconcile_every_ms=1000,
        fixture="owned-handoff-5728", preserve_dir=str(private / "preserved"),
        tables=[schema + ".readings"], columns={schema + ".readings":selected["columns"]},
        c1_chunk_rows=1, c1_export_parts={"max_rows":100},
        c5_to_c1=dict(protocol="oo.mysql.c5-to-c1/v1", source_id=source_id, binding=binding,
            project_id=project, expected_server_uuid=facts["uuid"],
            baseline="new_snapshot_plus_log_reconciliation", prior_history="not_recoverable_from_new_logs"))
    if recovery:
        native["c1_initial_resume"] = {"protocol":"oo.mysql.initial-resume/v1"}
        native["cycle_deadline_unix_ms"] = int(time.time() * 1000) + 900000
    save(private / "c1-config.json", native)
    candidate = dict(manifest, method="C1", c1_baseline_format="typed-keys/v3",
        producer=dict(**bins["mysql-preview"], args=["c1-baseline-stream-v3", "--config",
            str(private / "c1-config.json"), "--out", str(e / "c1-capture/{task}")]),
        loader=dict(**bins["oo-pipeline"], args=["import-native-c1-baseline-v3"]),
        registration=dict(admission_binary=bins["oo-source-admission"]["path"],
                          admission_sha256=bins["oo-source-admission"]["sha256"]))
    save(private / "c1-manifest.json", candidate)
    print("Owned schema/target ready", schema, database)


def audit(e, out, name, loaded, gold):
    c = read(e / "context.json")
    physical = loaded.get("physical_name") or loaded["entries"][0]["physical_id"]
    mappings = json.loads(pg(out, name + "-mapping",
        f"SELECT jsonb_object_agg(logical_column,physical_name) FROM _oo.physical_mappings "
        f"WHERE source='{c['source_id']}' AND level='c' AND scope_table_physical='{physical}';", c["database"]))
    cols = ",".join('"' + mappings[k] + '"::text' for k in ["id", "label", "qty"])
    actual = [json.loads(line) for line in pg(out, name + "-target",
        f"BEGIN READ ONLY; SELECT jsonb_build_array({cols}) FROM _oo.{physical}; COMMIT;", c["database"]).splitlines()]
    expected = sorted(gold, key=lambda r:int(r[0]))
    assert sorted(actual, key=lambda r:int(r[0])) == expected
    save(out / (name + "-audit.json"), dict(physical=physical, rows=actual, differences=0))
    return physical


def c5(e):
    c = read(e / "context.json")
    out = e / "c5"
    out.mkdir()
    worker(c, out, "put", "native-put", "--manifest", e / "private/c5-manifest.json")
    worker(c, out, "fire", "native-fire", "--project", c["project"])
    worker(c, out, "step", "native-step", "--project", c["project"], "--worker", "owned-handoff-c5")
    worker(c, out, "disable", "native-enable", "--binding", c["binding"], "--enabled", "false")
    c5_finish(e)


def c5_finish(e):
    c = read(e / "context.json")
    out = e / "c5-finish"
    out.mkdir()
    exported = json.loads(pg(out, "confirmed-c5-artifacts",
        f"SELECT jsonb_build_object('request',o.request,'load',w.result) "
        f"FROM jw_native.watermarks w JOIN jw_native.occurrences o ON o.task_id=w.task_id "
        f"JOIN jobs.tasks j ON j.id=w.task_id WHERE w.binding_id='{c['binding']}' AND j.state='succeeded';",
        c["database"]))
    (out / "artifacts").mkdir()
    for name in ["request", "load"]:
        save(out / "artifacts" / (name + ".json"), exported[name])
    loaded = read(out / "artifacts/load.json")
    assert loaded["source_generation"] == 1 and loaded["source_bound"] is True
    audit(e, out, "c5", loaded, read(e / "gold-c5.json"))
    # Publication request contains only actual sealed target metadata, never invented ACKs.
    receipt = json.loads(pg(out, "publication-vector", f"""
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
    receipt.update(collection_mode="per_table", run_ids=[loaded["run_id"]],
                   position_lineage=c["source_id"] + ".readings")
    for item in receipt["tables"]:
        item.update(run_id=loaded["run_id"], rejects=[], capture_method="c5_diff",
                    delete_detection="reconcile", consistency_grade=receipt["consistency_grade"],
                    position_lineage=receipt["position_lineage"])
    save(out / "publication-request.json", receipt)
    encoded = json.dumps(receipt, separators=(",", ":"))
    digest = hashlib.sha256(encoded.encode()).hexdigest()
    published = pg(out, "publish-normal-api",
        f"SELECT _oo.publish_native_per_table_collection('{loaded['run_id']}','{digest}',"
        + "'" + encoded.replace("'", "''") + "'::jsonb);", c["database"])
    assert json.loads(published)["outcome"] == "published"
    save(out / "receipt.json", dict(passed=True, generation=1, capture_method="c5_diff",
                                   row_count=8, publication_api="_oo.publish_native_per_table_collection"))


def handoff(e):
    c = read(e / "context.json")
    out = e / "handoff"
    out.mkdir()
    plan = e / "plan/plan.json"
    digest = pin(plan)["sha256"]
    cli = [c["bins"]["source-onboarding"]["path"], "mysql-handoff", "confirm",
           "--worker", c["bins"]["job-worker"]["path"], "--config", c["worker_config"],
           "--plan", plan, "--confirm-sha256"]
    before = pg(out, "before-refusals",
        f"SELECT jsonb_build_object('binding',to_jsonb(b),'progress',to_jsonb(q),'watermark',to_jsonb(w),"
        f"'handoffs',(SELECT count(*) FROM _oo.mysql_c5_handoff)) "
        f"FROM jw_native.bindings b JOIN jw_native.source_progress q ON q.binding_id=b.id "
        f"JOIN _oo.source_watermark w ON w.source=b.source_id WHERE b.id='{c['binding']}';", c["database"])
    bad = call(out, "wrong-confirmation", [*cli, "0" * 64], check=False)
    assert bad.returncode != 0
    raw = read(e / "private/c1-config.json")
    raw["c5_to_c1"]["expected_server_uuid"] = "11111111-1111-4111-8111-111111111111"
    save(out / "wrong-identity-config.json", raw)
    bad = call(out, "wrong-source-identity",
        [c["bins"]["mysql-preview"]["path"], "c1-handoff-preflight", "--config",
         out / "wrong-identity-config.json"], check=False)
    assert bad.returncode != 0
    # Existing normal put must not bypass the explicit capture-method transition.
    bad = worker(c, out, "implicit-put-refused", "native-put", "--manifest",
                 e / "private/c1-manifest.json", check=False)
    assert bad.returncode != 0
    after = pg(out, "after-refusals",
        f"SELECT jsonb_build_object('binding',to_jsonb(b),'progress',to_jsonb(q),'watermark',to_jsonb(w),"
        f"'handoffs',(SELECT count(*) FROM _oo.mysql_c5_handoff)) "
        f"FROM jw_native.bindings b JOIN jw_native.source_progress q ON q.binding_id=b.id "
        f"JOIN _oo.source_watermark w ON w.source=b.source_id WHERE b.id='{c['binding']}';", c["database"])
    assert json.loads(before) == json.loads(after)
    worker(c, out, "enabled-for-negative", "native-enable", "--binding", c["binding"], "--enabled", "true")
    bad = call(out, "busy-confirmation", [*cli, digest], check=False)
    assert bad.returncode != 0
    worker(c, out, "stop-after-negative", "native-enable", "--binding", c["binding"], "--enabled", "false")
    call(out, "confirm", [*cli, digest])
    old = read(e / "c5-finish/artifacts/load.json")
    audit(e, out, "old-after-confirmation", old, read(e / "gold-c5.json"))
    schema = c["schema"]
    sql = f"""START TRANSACTION;
UPDATE {schema}.readings SET qty=111,label='before-enable' WHERE id=1;
DELETE FROM {schema}.readings WHERE id=2;
UPDATE {schema}.readings SET id=13 WHERE id=3;
INSERT INTO {schema}.readings VALUES(9,'before-insert',900);
COMMIT;"""
    gold = [row[:] for row in read(e / "gold-c5.json") if row[0] != "2"]
    gold[0] = ["1", "before-enable", "111"]
    next(row for row in gold if row[0] == "3")[0] = "13"
    gold.append(["9", "before-insert", "900"])
    save(out / "gold-before-enable.json", gold)
    save(out / "BEFORE-IUD-ONCE.json", dict(sql=sql, sql_sha256=hashlib.sha256(sql.encode()).hexdigest()))
    source(out, "before-iud-once", sql)
    during = f"""START TRANSACTION;
UPDATE {schema}.readings SET qty=112,label='during-fill' WHERE id=1;
DELETE FROM {schema}.readings WHERE id=4;
UPDATE {schema}.readings SET qty=-555 WHERE id=5;
UPDATE {schema}.readings SET id=23 WHERE id=13;
INSERT INTO {schema}.readings VALUES(10,'during-insert',1000);
COMMIT;"""
    gold = [row for row in gold if row[0] != "4"]
    next(row for row in gold if row[0] == "1")[1:] = ["during-fill", "112"]
    next(row for row in gold if row[0] == "5")[2] = "-555"
    next(row for row in gold if row[0] == "13")[0] = "23"
    gold.append(["10", "during-insert", "1000"])
    save(e / "gold-c1.json", gold)
    save(out / "DURING-IUD-ONCE.json", dict(sql=during, sql_sha256=hashlib.sha256(during.encode()).hexdigest()))
    worker(c, out, "enable-c1", "native-enable", "--binding", c["binding"], "--enabled", "true")
    worker(c, out, "fire-c1", "native-fire", "--project", c["project"])
    task = pg(out, "task", f"SELECT task_id FROM jw_native.occurrences WHERE binding_id='{c['binding']}' "
              "ORDER BY pin DESC LIMIT 1;", c["database"])
    save(out / "task.json", dict(task=task))
    # Block only the first private target receipt: the source stream subscribes
    # and reads its first chunk, then waits for a real durable ACK before continuing.
    lock = subprocess.Popen(pg_argv(c["database"]), env=pg_env(), stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
    lock.stdin.write("BEGIN; LOCK TABLE _oo.c1_typed_receipt IN ACCESS EXCLUSIVE MODE;\n\\echo LOCKED\n")
    lock.stdin.flush()
    assert lock.stdout.readline().strip() == "LOCKED"
    with (out / "step.stdout").open("x") as stdout, (out / "step.stderr").open("x") as stderr:
        start = time.monotonic()
        proc = subprocess.Popen([c["bins"]["job-worker"]["path"], "--config", c["worker_config"],
            "native-step", "--project", c["project"], "--worker", "owned-handoff-c1"],
            stdout=stdout, stderr=stderr)
        try:
            ready = e / "c1-capture" / task / "ready"
            for _ in range(400):
                if ready.exists():
                    break
                assert proc.poll() is None, "C1 stopped before subscription"
                time.sleep(.025)
            else:
                raise AssertionError("source subscription deadline")
            # The receipt-table lock prevents capture from finishing even on this tiny source.
            audit(e, out, "old-during-private-fill", old, read(e / "gold-c5.json"))
            source(out, "during-iud-once", during)
            lock.stdin.write("COMMIT;\n\\q\n")
            lock.stdin.flush()
            lock.wait(timeout=5)
            code = proc.wait(timeout=150)
        finally:
            if lock.poll() is None:
                lock.stdin.close()
                lock.wait(timeout=5)
        save(out / "step.command.json", dict(exit=code, seconds=time.monotonic() - start))
    worker(c, out, "disable-c1", "native-enable", "--binding", c["binding"], "--enabled", "false")
    assert code == 0, "Preserve the actual failure; never replay source IUD"
    worker(c, out, "artifacts", "native-artifacts", "--binding", c["binding"], "--out", out / "artifacts")
    loaded = read(out / "artifacts/load.json")
    assert loaded["source_generation"] == 2
    audit(e, out, "new-before-publication", loaded, gold)
    audit(e, out, "old-before-publication", old, read(e / "gold-c5.json"))
    observed = [json.loads(line) for line in source(out, "source-gold",
        f"SELECT JSON_ARRAY(CAST(id AS CHAR),label,CAST(qty AS CHAR)) FROM {schema}.readings ORDER BY id;").splitlines()]
    assert observed == sorted(gold, key=lambda row:int(row[0]))
    pub = dict(protocol="oo.pipeline.native-c1-publication/v1", operation="publish",
        source_id=c["source_id"], task_id=task, request_digest=loaded["request_digest"], target=c["target"])
    result = call(out, "publish", [c["bins"]["oo-pipeline"]["path"], "native-c1-publication"], json.dumps(pub))
    assert json.loads(result.stdout)["publication"]["outcome"] == "published"
    save(out / "receipt.json", dict(passed=True, source_generation_before=1, source_generation_after=2,
        old_publication_readable_during_fill=True, differences=0, source_offset_reset=False,
        prior_c5_is_cdc=False, source_transactions=2, model_calls=0, gateway_verified=False))


def delta(e):
    c = read(e / "context.json")
    out = e / "delta"
    out.mkdir()
    schema = c["schema"]
    gold = [row[:] for row in read(e / "gold-c1.json") if row[0] != "6"]
    next(row for row in gold if row[0] == "9")[2] = "909"
    gold.append(["11", "post-handoff", "1100"])
    sql = f"""START TRANSACTION;
UPDATE {schema}.readings SET qty=909 WHERE id=9;
DELETE FROM {schema}.readings WHERE id=6;
INSERT INTO {schema}.readings VALUES(11,'post-handoff',1100);
COMMIT;"""
    save(out / "gold.json", gold)
    save(out / "IUD-ONCE.json", dict(sql=sql, sql_sha256=hashlib.sha256(sql.encode()).hexdigest()))
    source(out, "post-handoff-iud", sql)
    worker(c, out, "enable", "native-enable", "--binding", c["binding"], "--enabled", "true")
    worker(c, out, "fire", "native-fire", "--project", c["project"])
    worker(c, out, "step", "native-step", "--project", c["project"], "--worker", "owned-handoff-delta")
    worker(c, out, "disable", "native-enable", "--binding", c["binding"], "--enabled", "false")
    worker(c, out, "artifacts", "native-artifacts", "--binding", c["binding"], "--out", out / "artifacts")
    loaded = read(out / "artifacts/load.json")
    assert loaded["source_generation"] == 3
    audit(e, out, "delta", loaded, gold)
    actual = [json.loads(line) for line in source(out, "source",
        f"SELECT JSON_ARRAY(CAST(id AS CHAR),label,CAST(qty AS CHAR)) FROM {schema}.readings ORDER BY id;").splitlines()]
    assert actual == sorted(gold, key=lambda row:int(row[0]))
    task = pg(out, "task", f"SELECT task_id FROM jw_native.watermarks WHERE binding_id='{c['binding']}';", c["database"])
    receipt = call(out, "publish", [c["bins"]["oo-pipeline"]["path"], "native-c1-publication"],
        json.dumps(dict(protocol="oo.pipeline.native-c1-publication/v1", operation="publish",
            source_id=c["source_id"], task_id=task, request_digest=loaded["request_digest"], target=c["target"])))
    assert json.loads(receipt.stdout)["publication"]["outcome"] == "published"
    batches = json.loads(pg(out, "capture-batches", f"SELECT jsonb_agg(to_jsonb(t) ORDER BY seq) "
        f"FROM _oo.c1_typed_receipt t WHERE task='{task}';", c["database"]))
    assert len(batches) == 1 and batches[0]["kind"] == "cdc"
    save(out / "receipt.json", dict(passed=True, generation=3, fill_batches=0, cdc_batches=1,
        previous_end=read(e / "handoff/artifacts/load.json")["applied_pos"],
        new_end=loaded["applied_pos"], differences=0))


def scope_negatives(e):
    c = read(e / "context.json")
    c["bins"] = read(e / "binaries-v3.json")
    if (e / "worker-v4.json").exists():
        c["bins"]["job-worker"] = read(e / "worker-v4.json")
    c["source_id"] = c["source_id"].replace("mysql_handoff_", "mysql_hf_scope_")
    c["binding"], c["project"] = str(uuid.uuid4()), str(uuid.uuid4())
    out = e / "scope-negatives"
    if out.exists():
        saved = read(out / "context.json")
        c["binding"], c["project"] = saved["binding"], saved["project"]
    else:
        out.mkdir()
        save(out / "context.json", c)
    schema = c["schema"]
    seed = f"""CREATE TABLE {schema}.scope_a(id BIGINT PRIMARY KEY,label VARCHAR(32),qty BIGINT NOT NULL);
CREATE TABLE {schema}.scope_b(id BIGINT PRIMARY KEY,label VARCHAR(32),qty BIGINT NOT NULL);
START TRANSACTION;
INSERT INTO {schema}.scope_a VALUES(1,'scope-a',101);
INSERT INTO {schema}.scope_b VALUES(2,'scope-b',202);
COMMIT;"""
    if not (out / "SCOPE-FIXTURE-ONCE.json").exists():
        save(out / "SCOPE-FIXTURE-ONCE.json", dict(sql=seed, source_id=c["source_id"],
            purpose="actual additional published member; not C5/C1 acceptance", settings_changed=False))
        source(out, "seed-owned-extra-members", seed)
    else:
        assert read(out / "seed-owned-extra-members.command.json")["exit"] == 0, "inspect uncertain seed; do not replay"

    def snapshot(name):
        return json.loads(pg(out, name, f"""
SELECT jsonb_build_object(
 'bindings',(SELECT jsonb_agg(to_jsonb(x) ORDER BY id) FROM jw_native.bindings x WHERE source_id='{c["source_id"]}'),
 'progress',(SELECT jsonb_agg(to_jsonb(x)) FROM jw_native.source_progress x WHERE source_id='{c["source_id"]}'),
 'watermark',(SELECT jsonb_agg(to_jsonb(x)) FROM _oo.source_watermark x WHERE source='{c["source_id"]}'),
 'runs',(SELECT jsonb_agg(to_jsonb(x) ORDER BY run_id) FROM _oo.full_import_run x WHERE source_id='{c["source_id"]}'),
 'publications',(SELECT jsonb_agg(to_jsonb(x) ORDER BY run_id) FROM _oo.native_publication_receipt x WHERE source_id='{c["source_id"]}'),
 'handoff',(SELECT jsonb_agg(to_jsonb(x)) FROM _oo.mysql_c5_handoff x WHERE source='{c["source_id"]}'));
""", c["database"]))

    def refused(name, candidate):
        before = snapshot(name + "-before")
        result = worker(c, out, name, "native-mysql-handoff-plan", "--manifest", candidate,
                        "--out", out / (name + "-plan.json"), check=False)
        assert result.returncode != 0 and not (out / (name + "-plan.json")).exists()
        assert snapshot(name + "-after") == before
        return result.stderr

    def prepare(table):
        stage = out / table
        if stage.exists():
            return stage
        stage.mkdir()
        cfg = read(e / "private/capture.json")
        cfg.update(source_id=c["source_id"], owner=c["binding"])
        cfg["table"]["table"] = table
        cfg["approved_objects"] = [cfg["table"]]
        save(stage / "capture.json", cfg)
        adapter = read(e / "private/c5-producer.json")
        adapter.update(capture_config=str(stage / "capture.json"), driver=c["bins"]["mysql-preview"],
            state_dir=str(stage / "capture"), table=table,
            registry_template=str(stage / "registered/{binding}/{pin}/registry.json"))
        adapter["target"].update(shard=c["source_id"], owner=c["binding"])
        save(stage / "producer.json", adapter)
        manifest = read(e / "private/c5-manifest.json")
        manifest.update(source_id=c["source_id"], binding=c["binding"], project_id=c["project"],
            loader=dict(**c["bins"]["oo-pipeline"], args=["import-native-update"]))
        manifest["producer"]["args"] = ["--config", str(stage / "producer.json")]
        manifest["registration"]["registry_path"] = adapter["registry_template"]
        save(stage / "manifest.json", manifest)
        native = read(e / "private/c1-config.json")
        native.update(tables=[schema + "." + table], columns={schema + "." + table:["id", "label", "qty"]})
        native["c5_to_c1"].update(source_id=c["source_id"], binding=c["binding"], project_id=c["project"])
        save(stage / "native.json", native)
        candidate = read(e / "private/c1-manifest.json")
        candidate.update(source_id=c["source_id"], binding=c["binding"], project_id=c["project"],
            loader=dict(**c["bins"]["oo-pipeline"], args=["import-native-c1-baseline-v3"]),
            producer=dict(**c["bins"]["mysql-preview"], args=["c1-baseline-stream-v3", "--config",
                str(stage / "native.json"), "--out", str(stage / "c1/{task}")]))
        save(stage / "candidate.json", candidate)
        return stage

    def capture_publish(stage):
        for label, args in [
            ("put", ["native-put", "--manifest", stage / "manifest.json"]),
            ("fire", ["native-fire", "--project", c["project"]]),
            ("step", ["native-step", "--project", c["project"], "--worker", "scope-negative"]),
            ("disable", ["native-enable", "--binding", c["binding"], "--enabled", "false"])]:
            name = label
            for attempt in range(1, 100):
                if not (stage / (name + ".command.json")).exists():
                    worker(c, stage, name, *args)
                    break
                if read(stage / (name + ".command.json"))["exit"] == 0:
                    break
                name = label + "-" + str(attempt + 1)
        receipt = json.loads(pg(stage, "actual-publication-vector", f"""
SELECT jsonb_build_object('protocol','oo.pipeline.native-publication-receipt/v1','collection_mode','per_table',
 'source_id',w.source,'source_generation',w.source_generation,'target_generation',w.target_generation,
 'position_codec',w.position_codec,'position_lineage',w.position_lineage,'applied_pos','0',
 'consistency',w.consistency,'consistency_grade',w.consistency_grade,'cross_table_consistent',false,
 'run_id',(SELECT r.run_id FROM _oo.full_import_run r WHERE r.source_id=w.source
   AND r.source_generation=w.source_generation ORDER BY r.target_generation DESC LIMIT 1),
 'run_ids',jsonb_agg(r.run_id ORDER BY f.table_name),
 'tables',jsonb_agg(jsonb_build_object('schema',t.schema_ident,'table',t.table_ident,
   'run_id',r.run_id,'physical_name',t.physical_name,'accepted_rows',t.applied_rows,
   'canonical_digest',t.canonical_digest,'rejects','[]'::jsonb,
   'source_generation',r.source_generation,'target_generation',r.target_generation,'applied_pos','0',
   'capture_method','c5_diff','delete_detection','reconcile','consistency_grade','S0',
   'position_lineage',r.source_id||'.'||t.table_ident) ORDER BY f.table_name))
FROM _oo.source_watermark w JOIN _oo.table_freshness f ON f.source=w.source
JOIN _oo.full_import_run r ON r.source_id=f.source AND r.source_generation=f.source_generation
 AND r.target_generation=f.target_generation
JOIN _oo.full_import_table t ON t.run_id=r.run_id AND t.physical_name=f.physical_name
WHERE w.source='{c["source_id"]}' GROUP BY w.source;
""", c["database"]))
        # Per-table collections use B when they contain different capture cuts.
        if len(receipt["tables"]) > 1:
            receipt["consistency_grade"] = "B"
        encoded = json.dumps(receipt, separators=(",", ":"))
        save(stage / "publication-request.json", receipt)
        result = pg(stage, "publish-normal-api",
            f"SELECT _oo.publish_native_per_table_collection('{receipt['run_id']}',"
            f"'{hashlib.sha256(encoded.encode()).hexdigest()}',"
            + "'" + encoded.replace("'", "''") + "'::jsonb);", c["database"])
        assert json.loads(result)["outcome"] == "published"

    a = prepare("scope_a")
    capture_publish(a)
    # First demonstrate a valid fresh plan before introducing either negative.
    worker(c, out, "single-member-plan", "native-mysql-handoff-plan", "--manifest", a / "candidate.json",
           "--out", out / "single-member-plan.json")
    original = (a / "capture.json").read_bytes()
    mutation = read(a / "capture.json")
    mutation["connection"]["database"] = schema + "_retag"
    (a / "capture.json").write_text(json.dumps(mutation))
    save(out / "template-mutation.json", dict(path=str(a / "capture.json"),
        original_sha256=hashlib.sha256(original).hexdigest(), mutation_sha256=pin(a / "capture.json")["sha256"]))
    try:
        assert "handoff_old_template_drift" in refused("same-path-old-config-retag", a / "candidate.json")
    finally:
        (a / "capture.json").write_bytes(original)
    b = prepare("scope_b")
    capture_publish(b)
    assert "exact_published_membership" in refused("additional-published-member", b / "candidate.json")
    sibling = read(b / "manifest.json")
    sibling["binding"] = str(uuid.uuid4())
    save(out / "sibling.json", sibling)
    worker(c, out, "put-disabled-sibling", "native-put", "--manifest", out / "sibling.json")
    worker(c, out, "disable-sibling", "native-enable", "--binding", sibling["binding"], "--enabled", "false")
    assert "single_binding_required" in refused("additional-disabled-binding", b / "candidate.json")
    save(out / "receipt.json", dict(passed=True, actual_published_members=2,
        same_path_old_template_mutation_refused=True, additional_published_member_refused=True,
        disabled_sibling_refused=True, each_refusal_no_effects=True,
        source_id=c["source_id"], retained_fixture=True, c5_to_c1_not_attempted=True))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["bootstrap", "c5", "c5-finish", "handoff", "delta", "scope-negatives"])
    parser.add_argument("--evidence", type=Path, required=True)
    args = parser.parse_args()
    {"bootstrap":bootstrap, "c5":c5, "c5-finish":c5_finish, "handoff":handoff,
     "delta":delta, "scope-negatives":scope_negatives}[args.action](args.evidence.resolve())


if __name__ == "__main__":
    main()
