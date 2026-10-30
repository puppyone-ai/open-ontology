#!/usr/bin/env python3
"""Real fixed-CLI C1 integration on the checkpoint-7 owner's two containers.

No task-row insertion: jobs deployment, native-fire and native-step own the
actual claim/heartbeat/completion transitions. Keep every failed run directory.
"""
import hashlib
import json
import os
from pathlib import Path
import secrets
import shutil
import subprocess
import sys
import threading
import time
import uuid

ROOT = Path(__file__).resolve().parents[6]
EVIDENCE = Path(os.environ["OO_C1_INTEGRATION_EVIDENCE"])
HOME = Path("/tmp/oo-mysql-c1-20261027")
MYSQL = "oo-mysql-c1-20261027-mysql57"
PG = "oo-mysql-c1-20261027-pg17"
TAG = str(time.time_ns())
OUT = EVIDENCE / f"run-{TAG}"
PRIVATE = HOME / f"formal-{TAG}"
OUT.mkdir(parents=True)
PRIVATE.mkdir(mode=0o700)
COMMANDS = []
REDACTIONS = [p.read_text().strip() for p in (HOME / "secrets").glob("*.pw")]


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def save(name, obj):
    (OUT / name).write_text(json.dumps(obj, indent=2) + "\n")


def private(name, data):
    p = PRIVATE / name
    p.write_text(data)
    p.chmod(0o600)
    return p


def run(name, argv, data=None, check=True):
    started = time.monotonic()
    p = subprocess.run(list(map(str, argv)), cwd=ROOT, input=data,
                       capture_output=True)
    for suffix, content in [("stdout", p.stdout), ("stderr", p.stderr)]:
        text = content.decode(errors="replace")
        for secret in REDACTIONS:
            text = text.replace(secret, "[redacted]")
        (OUT / f"{name}.{suffix}").write_text(text)
    COMMANDS.append(dict(name=name, argv=list(map(str, argv)), exit=p.returncode,
                         seconds=time.monotonic() - started))
    save("commands.json", COMMANDS)
    if check and p.returncode:
        raise RuntimeError(f"{name} exit {p.returncode}; inspect retained logs")
    return p


def pg(name, sql, database="postgres"):
    return run(name, ["docker", "exec", "-i", PG, "psql", "-X", "-U", "postgres",
                     "-d", database, "-At", "-v", "ON_ERROR_STOP=1"],
               sql.encode()).stdout.decode().strip()


def mysql(name, sql):
    return run(name, ["docker", "exec", "-i", MYSQL, "sh", "-c",
                     'MYSQL_PWD="$(cat /secrets/mysql-root.pw)" mysql -uroot -N -B --raw'],
               sql.encode()).stdout.decode().strip()


def main():
    for container in [MYSQL, PG]:
        info = json.loads(subprocess.check_output(["docker", "inspect", container]))[0]
        assert info["Config"]["Labels"]["owner"] == "mysql-c1-20261027"
    # Enable verified TLS on the owned PG fixture without stopping its server.
    for src, dst in [("server-key.pem", "c1-server.key"), ("server-cert.pem", "c1-server.crt")]:
        run("copy-" + dst, ["docker", "cp", HOME / "certs" / src,
                            f"{PG}:/var/lib/postgresql/data/{dst}"])
    run("pg-cert-mode", ["docker", "exec", PG, "sh", "-c",
                         "chown postgres:postgres /var/lib/postgresql/data/c1-server.*; chmod 600 /var/lib/postgresql/data/c1-server.key"])
    pg("pg-enable-tls", """ALTER SYSTEM SET ssl='on';
ALTER SYSTEM SET ssl_cert_file='/var/lib/postgresql/data/c1-server.crt';
ALTER SYSTEM SET ssl_key_file='/var/lib/postgresql/data/c1-server.key';
SELECT pg_reload_conf();""")
    time.sleep(.3)
    runtime = "c1rt_" + TAG
    runtime_secret = secrets.token_hex(24)
    REDACTIONS.append(runtime_secret)
    runtime_file = private("runtime.pw", runtime_secret)
    database = "c1formal_" + TAG
    source_db = "cf_" + TAG
    reader = "cfr_" + TAG[-16:]
    source_secret = secrets.token_hex(24)
    REDACTIONS.append(source_secret)
    source_file = private("source.pw", source_secret)
    pg("create-owned-target", f"CREATE ROLE {runtime} LOGIN INHERIT NOSUPERUSER PASSWORD '{runtime_secret}';\nCREATE DATABASE {database};")
    run("deploy-real-jobs", [sys.executable, ROOT / "product/crates/oo-jobs/tools/deploy.py",
                             "--container", PG, "--database", database, "--loader-role", runtime])
    bins = PRIVATE / "bin"
    bins.mkdir()
    for name in ["oo-pipeline", "mysql-preview", "job-worker", "oo-source-admission"]:
        frozen = EVIDENCE / "frozen-runtime" / name
        shutil.copy2(frozen if frozen.is_file() else ROOT / "product/target/debug" / name, bins / name)
        if name != "oo-source-admission" and not frozen.is_file():
            run("strip-" + name, ["/usr/bin/strip", "-S", "-x", bins / name])
    save("artifacts.json", {name: dict(path=str(p), sha256=sha(p))
                           for name in ["oo-pipeline", "mysql-preview", "job-worker", "oo-source-admission"]
                           for p in [bins / name]})
    target = dict(host="127.0.0.1", port=25487, database=database,
                  runtime_principal="postgres", password_file=str(HOME / "secrets/pg-super.pw"),
                  ca_file=str(HOME / "certs/ca.pem"))
    for command in ["install-native-update", "install-native-mirror-rebind",
                    "install-native-claim-selected", "install-native-c1-typed"]:
        p = run(command, [bins / "oo-pipeline", command],
                json.dumps(dict(protocol="oo.pipeline.install-native-update/v1", target=target)).encode())
        result = json.loads(p.stdout)
        assert result.get("installed") is True or result.get("outcome") == "installed", result
    pg("grant-internal-runtime", f"""GRANT oo_pipeline_runtime TO {runtime} WITH INHERIT TRUE, SET FALSE;
ALTER ROLE {runtime} SET log_statement='all';
ALTER ROLE {runtime} SET log_parameter_max_length=0;""", database)
    worker_config = private("worker.cfg", "\n".join([
        "host=127.0.0.1", "port=25487", f"database={database}", f"runtime_user={runtime}",
        f"runtime_password_file={runtime_file}", "admin_user=postgres",
        f"admin_password_file={HOME / 'secrets/pg-super.pw'}", f"ca_file={HOME / 'certs/ca.pem'}",
        f"migrations={ROOT / 'product/crates/oo-jobs/migrations'}",
    ]) + "\n")
    mysql("source-schema", f"""CREATE DATABASE {source_db} CHARACTER SET utf8mb4 COLLATE utf8mb4_general_ci;
CREATE USER '{reader}'@'%' IDENTIFIED BY '{source_secret}';
GRANT SELECT ON {source_db}.* TO '{reader}'@'%';
GRANT SELECT ON performance_schema.events_statements_summary_by_user_by_event_name TO '{reader}'@'%';
GRANT REPLICATION CLIENT, REPLICATION SLAVE ON *.* TO '{reader}'@'%';
CREATE TABLE {source_db}.items(id BIGINT NOT NULL,code VARCHAR(32) NOT NULL,v VARCHAR(64),amount DECIMAL(12,2) NOT NULL,PRIMARY KEY(code,id)) ENGINE=InnoDB;""")
    for start in range(1, 10001, 1000):
        values = ",".join(f"({i},'key{i:05d}','before-{i}',{i}.25)" for i in range(start, start + 1000))
        mysql(f"seed-{start}", f"INSERT INTO {source_db}.items VALUES {values};")
    binding, project = str(uuid.uuid4()), str(uuid.uuid4())
    source_config = private("source.json", json.dumps(dict(
        database=source_db, host="127.0.0.1", port=23327, user=reader,
        password_file=str(source_file), ca_file=str(HOME / "certs/ca.pem"),
        tls_server_name="localhost", secret_ref="secret://mysql-owned-formal",
        server_id=5731, health_every_ms=200, reconcile_every_ms=1000,
        preserve_dir=str(PRIVATE / "capture"), fixture="owned-formal-c1",
        tables=[source_db + ".items"], columns={source_db + ".items": ["id", "code", "v", "amount"]},
        onboarding=dict(binding=binding, project_id=project, period_ms=86400000,
                        producer=str(bins / "mysql-preview"), loader=str(bins / "oo-pipeline"),
                        admission=str(bins / "oo-source-admission")),
        c1_chunk_rows=100, cycle_deadline_unix_ms=int(time.time() * 1000) + 300000)))
    wrong = run("prepare-wrong-version", [bins / "mysql-preview", "c1-onboarding-prepare",
                 "--config", source_config, "--out", PRIVATE / "wrong-format",
                 "--c1-baseline-format", "chunked-typed/v3"], check=False)
    assert wrong.returncode and not (PRIVATE / "wrong-format").exists()
    prepared = PRIVATE / "prepared"
    run("prepare-typed-v3", [bins / "mysql-preview", "c1-onboarding-prepare",
         "--config", source_config, "--out", prepared, "--c1-baseline-format", "typed-keys/v3"])
    identity = json.loads((prepared / "identity.json").read_text())
    manifest_path = prepared / "native-manifest.json"
    manifest = json.loads(manifest_path.read_text())
    assert manifest["producer"]["args"][0] == "c1-baseline-stream-v3"
    assert manifest["loader"]["args"] == ["import-native-c1-baseline-v3"]
    assert manifest["c1_baseline_format"] == "typed-keys/v3"
    assert (prepared / "source-config.json").is_file()
    source_id = identity["source_id"]
    save("context.json", dict(database=database, source_database=source_db, source_id=source_id,
                             runtime=runtime, binding=binding, project=project,
                             private_dir=str(PRIVATE), worker_config=str(worker_config),
                             manifest_path=str(manifest_path)))

    def worker(name, *args, check=True):
        return run(name, [bins / "job-worker", "--config", worker_config, *args], check=check)

    worker("native-install", "native-install")
    worker("native-put", "native-put", "--manifest", manifest_path)
    worker("native-fire", "native-fire", "--project", project)
    log_start = str(int(time.time()))
    mutation = dict(done=False)

    def mutate():
        try:
            deadline = time.monotonic() + 90
            while not list(PRIVATE.glob("prepared/capture-*/ready")):
                if time.monotonic() > deadline:
                    raise RuntimeError("producer did not subscribe")
                time.sleep(.02)
            sql = ["START TRANSACTION;"]
            for start in range(10001, 14001, 1000):
                values = ",".join(f"({i},'key{i:05d}','insert-{i}',{i}.50)" for i in range(start, start + 1000))
                sql.append(f"INSERT INTO {source_db}.items VALUES {values};")
            sql += [f"UPDATE {source_db}.items SET v=CONCAT('updated-',id),amount=amount+1 WHERE id BETWEEN 1 AND 4000;",
                    f"DELETE FROM {source_db}.items WHERE id BETWEEN 4001 AND 6000;", "COMMIT;"]
            mysql("source-iud", "\n".join(sql))
            mutation["done"] = True
        except Exception as exc:
            mutation["error"] = str(exc)

    thread = threading.Thread(target=mutate)
    thread.start()
    step = worker("native-step", "native-step", "--project", project, "--worker", "owned-formal", check=False)
    thread.join()
    server_log = subprocess.check_output(["docker", "logs", "--since", log_start, PG],
                                         stderr=subprocess.STDOUT).decode(errors="replace")
    calls = [line for line in server_log.splitlines() if any(name in line for name in
             ("jobs.claim_selected(", "jobs.heartbeat(", "jobs.complete_success(",
              "_oo.native_source_instance_register("))]
    (OUT / "real-lease-api-calls.log").write_text("\n".join(calls) + "\n")
    save("mutation.json", mutation)
    worker("native-disable", "native-enable", "--binding", binding, "--enabled", "false", check=False)
    worker("native-state", "native-state", "--binding", binding, check=False)
    pg("real-job-events", "SELECT row_to_json(t) FROM (SELECT id,state,generation,attempt,lease_owner FROM jobs.tasks) t;\nSELECT row_to_json(e) FROM jobs.task_events e;", database)
    pg("target-state", "SELECT row_to_json(r) FROM (SELECT run_id,source_id,state,source_generation FROM _oo.full_import_run) r;\nSELECT row_to_json(r) FROM _oo.native_publication_receipt r;", database)
    assert mutation["done"], mutation
    assert step.returncode == 0, "worker failed; original jobs/parts/receipts retained"
    save("result.json", dict(worker_exit=0, real_jobs=True, mutation=mutation,
                             publication="must verify separately, not inferred from completion"))


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        save("failure.json", {"error": str(exc)})
        print(f"FAILED {OUT}: {exc}", file=sys.stderr)
        raise SystemExit(1)
    print(OUT)
