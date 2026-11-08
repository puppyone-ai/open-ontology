#!/usr/bin/env python3
"""T9-10: owned PG17 + real Gateway; logical/physical restores and tamper denials.

No existing service is used. The F22 runner creates the initial private cluster.
All credentials, dumps and raw responses remain in the ignored evidence directory.
"""
import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("evidence", type=Path)
    args = parser.parse_args()
    evidence = args.evidence.resolve()
    evidence.relative_to(ROOT / "evidence")
    evidence.mkdir(parents=True, exist_ok=True)
    if (evidence / "ACTIVE-RUNTIME.json").exists():
        raise SystemExit("use a new evidence directory; never reuse a live fixture")
    pg_bin = Path(os.environ.get("OO_CI_PG_BIN", "/opt/homebrew/opt/postgresql@17/bin"))
    log = (evidence / "t9.log").open("w")
    owned = []
    gateway = None
    original_gateway_pid = None
    results = {"protocol": "oo.t9-restore/v1", "complete": False, "checks": []}

    def run(argv, **kwargs):
        return subprocess.run([str(a) for a in argv], check=True, timeout=900,
                              stdout=kwargs.pop("stdout", log), stderr=log, **kwargs)

    def free_port():
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            return sock.getsockname()[1]

    try:
        run([sys.executable, ROOT / "crates/oo-gateway/tests/f22_owned_pg.py", evidence, "--handoff"])
        runtime = json.loads((evidence / "ACTIVE-RUNTIME.json").read_text())
        original_gateway_pid = runtime["gateway_pid"]
        work = Path(runtime["config_dir"])
        fixture = json.loads(Path(runtime["fixture"]).read_text())
        original_port = fixture["port"]
        owned.append(work / "pg")
        env = dict(os.environ, PGPASSWORD=Path(fixture["password_file"]).read_text())
        token = Path(runtime["token_file"]).read_text()
        port = original_port
        origin = runtime["origin"]

        def sql(text, database="f22_owned", target_port=None):
            return run([pg_bin / "psql", "-XqAt", "-v", "ON_ERROR_STOP=1", "-h", "127.0.0.1",
                        "-p", target_port or port, "-U", "postgres", "-d", database],
                       input=text, text=True, stdout=subprocess.PIPE, env=env).stdout.strip()

        def query(publication="current", path="/v1/query"):
            data = json.dumps({"sql": "SELECT amount FROM app.items", "format": "json"}).encode() if path == "/v1/query" else None
            request = urllib.request.Request(origin + path, data=data, headers={
                "Authorization": "Bearer " + token, "Content-Type": "application/json",
                "x-oo-expected-publication-id": publication})
            try:
                with urllib.request.urlopen(request, timeout=30) as response:
                    body = json.load(response)
                    body["publication_binding"] = {
                        "publication_id": response.headers["x-oo-publication-id"],
                        "data_vector_sha256": response.headers["x-oo-data-vector-sha256"],
                    }
                    return response.status, body
            except urllib.error.HTTPError as error:
                return error.code, json.load(error)

        def check(name, expected, publication="current", path="/v1/query"):
            status, body = query(publication, path)
            (evidence / (name + ".json")).write_text(json.dumps({"status": status, "body": body}, indent=2) + "\n")
            if expected == "ok":
                assert status == 200, (name, status, body)
                if path == "/v1/query":
                    assert body["rows"] == initial_rows, (name, body)
            else:
                assert status != 200 and "PUBLICATION_DRIFT" in json.dumps(body), (name, status, body)
                assert "rows" not in body, (name, body)
            results["checks"].append({"name": name, "status": status, "pass": True})
            return body

        status, initial = query()
        assert status == 200
        initial_rows = initial["rows"]
        old_id = initial["publication_binding"]["publication_id"]
        physical = sql("SELECT relation_name FROM oo_publish_target.fence LIMIT 1")
        column = sql("SELECT attname FROM pg_attribute WHERE attrelid=('_oo.' || '" + physical + "')::regclass AND attnum=1")
        sql("CREATE ROLE oo_target_admin NOLOGIN; GRANT USAGE ON SCHEMA _oo,oo_publish_target TO oo_target_admin; "
            "GRANT SELECT,UPDATE ON oo_publish_target.fence TO oo_target_admin;")
        sql(f"GRANT SELECT,TRIGGER ON _oo.{physical} TO oo_target_admin")
        sql(f"CREATE DOMAIN _oo.t9_text AS text; ALTER TABLE _oo.{physical} ALTER COLUMN secret TYPE _oo.t9_text")
        sql((ROOT / "crates/oo-pipeline/migrations/0135_logical_publication_identity.sql").read_text())
        sql("GRANT SELECT ON _oo.publication_identity TO oo_f22_login")
        check("upgrade_requires_republish", "drift")

        def publish():
            sql("SET ROLE oo_target_admin; UPDATE oo_publish_target.fence SET state='open'; RESET ROLE;")

        publish()
        # 0135 alone cannot see a replaced view routine; the Gateway requires 0136.
        check("routine_identity_required", "drift")
        sql((ROOT / "crates/oo-pipeline/migrations/0136_publication_routine_identity.sql").read_text())
        # The fixture's row-security policy calls a non-built-in routine: 0136
        # binds it, so installing tombstones the record until the publisher reopens.
        assert "f22_read_probe" in sql(f"SELECT _oo.publication_relation_identity('_oo','{physical}')::text")
        check("routine_identity_installed", "drift")
        publish()
        check("old_receipt_refused", "drift", old_id)
        fingerprint = sql(f"SELECT _oo.publication_relation_digest('_oo','{physical}')")
        assert sql(f"CREATE TEMP TABLE pg_class(spoof integer); "
                   f"SELECT _oo.publication_relation_digest('_oo','{physical}')") == fingerprint
        results["temporary_catalog_lookalike_ignored"] = True
        view_test = ["cargo", "test", "--locked", "-p", "oo-gateway", "--lib",
                     "publication_binding::identity_tests::logical_view_binding_refuses_",
                     "--", "--ignored", "--nocapture", "--test-threads=1"]
        view_result = run(view_test, env=dict(os.environ, OO_F22_OWNED_FIXTURE=runtime["fixture"]),
                          text=True, stdout=subprocess.PIPE).stdout
        (evidence / "view-test.log").write_text(view_result)
        assert "2 passed; 0 failed; 0 ignored" in view_result
        results["view_receipt_redefinition_dependency_swap_recreate"] = True
        results["view_receipt_replaced_routine"] = True
        baseline = check("logical_binding", "ok")
        publication = baseline["publication_binding"]["publication_id"]
        oid = sql("SELECT '_oo." + physical + "'::regclass::oid")
        type_oid = sql("SELECT '_oo.t9_text'::regtype::oid")
        results["postgres_version"] = sql("SHOW server_version")
        # Privileged globals stay in the private evidence tree, never in argv/logs.
        globals_file = evidence / "globals.sql"
        run([pg_bin / "pg_dumpall", "-h", "127.0.0.1", "-p", port, "-U", "postgres", "--roles-only", "-f", globals_file], env=env)
        globals_file.chmod(0o600)
        globals_text = globals_file.read_text().replace("CREATE ROLE postgres;", "")
        dump = evidence / "target.dump"
        run([pg_bin / "pg_dump", "-h", "127.0.0.1", "-p", port, "-U", "postgres", "-Fc", "-f", dump, "f22_owned"], env=env)
        dump.chmod(0o600)
        logical = evidence / "logical-pg"
        run([pg_bin / "initdb", "-D", logical, "-U", "postgres", "--auth-host=scram-sha-256",
             "--auth-local=trust", "--pwfile", fixture["password_file"]])
        # The fixture certificate is valid for this loopback-only owned cluster.
        with (logical / "postgresql.conf").open("a") as config:
            config.write(f"\nlisten_addresses='127.0.0.1'\nunix_socket_directories=''\nssl=on\nssl_cert_file='{work / 'server.crt'}'\nssl_key_file='{work / 'server.key'}'\n")
        logical_port = free_port()
        owned.append(logical)
        run([pg_bin / "pg_ctl", "-D", logical, "-l", evidence / "logical.log", "-o", f"-p {logical_port}", "-w", "start"])
        sql(globals_text, "postgres", logical_port)
        sql("CREATE DATABASE f22_owned", "postgres", logical_port)
        # Consume OIDs so this is demonstrably not an accidental OID-preserving restore.
        sql("CREATE TABLE public.oid_padding(i integer);", target_port=logical_port)
        run([pg_bin / "pg_restore", "--exit-on-error", "-h", "127.0.0.1", "-p", logical_port,
             "-U", "postgres", "-d", "f22_owned", dump], env=env)

        def restart_gateway(new_port):
            nonlocal gateway, original_gateway_pid, port, origin
            if original_gateway_pid is not None:
                os.kill(original_gateway_pid, 15)
                original_gateway_pid = None
            if gateway is not None:
                gateway.terminate()
                gateway.wait(timeout=15)
            port = new_port
            (work / "endpoint.json").write_text(json.dumps({"host": "127.0.0.1", "port": port, "database": "f22_owned"}))
            (work / "listen.addr").unlink(missing_ok=True)
            gateway = subprocess.Popen([runtime["gateway_binary"], str(work)], stdout=log, stderr=log)
            for _ in range(200):
                assert gateway.poll() is None, "Gateway exited during restart"
                if (work / "listen.addr").exists():
                    origin = "http://" + (work / "listen.addr").read_text().strip()
                    return
                time.sleep(.05)
            raise RuntimeError("Gateway startup timed out")

        restart_gateway(logical_port)
        restored_oid = sql("SELECT '_oo." + physical + "'::regclass::oid")
        assert oid != restored_oid, "restore did not change relation OID"
        results["relation_oid_changed"] = True
        assert type_oid != sql("SELECT '_oo.t9_text'::regtype::oid")
        results["user_type_oid_changed"] = True
        assert sql("SELECT bool_and(valid AND relation_identity_sha256=_oo.publication_relation_digest(schema_name,relation_name)) FROM _oo.publication_identity") == "t"
        results["restored_view_identity_valid"] = True
        # The routine-bound view identity survives the logical restore too.
        assert "t9_scale" in sql("SELECT _oo.publication_relation_identity('_oo','t9_fview')::text")
        assert sql("SELECT count(*) FROM _oo.publication_identity WHERE relation_name='t9_fview' AND valid") == "1"
        results["restored_routine_identity_valid"] = True
        restored = check("pg_dump_pg_restore", "ok", publication)
        assert restored["publication_binding"] == baseline["publication_binding"]
        sql("ALTER EVENT TRIGGER oo_publication_identity_drop DISABLE")
        check("disabled_guard", "drift", publication)
        sql("ALTER EVENT TRIGGER oo_publication_identity_drop ENABLE")
        check("reenabled_guard", "ok", publication)

        def tamper(name, mutation, repair):
            sql(mutation)
            check(name, "drift", publication)
            check(name + "_current", "drift")
            sql(repair)
            # Even repairing the old shape cannot clear a DDL tombstone.
            check(name + "_repair_without_publisher", "drift")
            publish()
            check(name + "_republished", "ok")

        qualified = "_oo." + physical
        tamper("truncate", f"TRUNCATE {qualified}",
               f"INSERT INTO {qualified} VALUES(10,'synthetic')")
        tamper("rename_swap", f"CREATE TABLE _oo.swap (LIKE {qualified}); INSERT INTO _oo.swap VALUES(999,'synthetic'); "
               f"ALTER TABLE {qualified} RENAME TO original; ALTER TABLE _oo.swap RENAME TO {physical}; "
               f"GRANT SELECT({column}) ON {qualified} TO oo_r_f22;",
               f"DROP TABLE {qualified}; ALTER TABLE _oo.original RENAME TO {physical};")
        tamper("column_type", f"ALTER TABLE {qualified} ALTER COLUMN {column} TYPE numeric(20,0)",
               f"ALTER TABLE {qualified} ALTER COLUMN {column} TYPE bigint")
        # Unexposed columns are part of the relation identity too.
        tamper("column_typmod", f"ALTER TABLE {qualified} ALTER COLUMN secret TYPE varchar(32)",
               f"ALTER TABLE {qualified} ALTER COLUMN secret TYPE text")
        sql(f"ALTER TABLE {qualified} RENAME TO saved; CREATE TABLE {qualified}(LIKE _oo.saved); "
            f"INSERT INTO {qualified} VALUES(999,'synthetic'); GRANT SELECT({column}) ON {qualified} TO oo_r_f22;")
        check("same_name_replacement", "drift")
        sql(f"DROP TABLE {qualified}; ALTER TABLE _oo.saved RENAME TO {physical}")
        publish()
        # Actual DROP + CREATE of the same name/shape, including restored ACL/RLS.
        run([pg_bin / "pg_dump", "-h", "127.0.0.1", "-p", port, "-U", "postgres", "-t", qualified,
             "--schema-only", "-f", evidence / "relation.sql", "f22_owned"], env=env)
        sql(f"DROP TABLE {qualified}")
        sql((evidence / "relation.sql").read_text())
        sql(f"INSERT INTO {qualified} VALUES(999,'synthetic')")
        check("drop_recreate_same_shape", "drift")
        sql(f"UPDATE {qualified} SET {column}=10")
        check("drop_recreate_repaired", "drift")
        publish()
        check("drop_recreate_republished", "ok")
        sql("CREATE ROLE lookalike; GRANT USAGE,CREATE ON SCHEMA _oo TO lookalike")
        sql(f"DROP TABLE {qualified}; SET ROLE lookalike; CREATE TABLE {qualified}({column} bigint, secret text); "
            f"INSERT INTO {qualified} VALUES(999,'synthetic'); GRANT SELECT({column}) ON {qualified} TO oo_r_f22; RESET ROLE;")
        check("nonpublisher_lookalike", "drift")
        for action in ["UPDATE _oo.publication_identity SET valid=true", "UPDATE oo_publish_target.fence SET state='open'"]:
            denied = subprocess.run([str(pg_bin / "psql"), "-XqAt", "-v", "ON_ERROR_STOP=1", "-h", "127.0.0.1",
                                     "-p", str(port), "-U", "postgres", "-d", "f22_owned"],
                                    input="SET ROLE lookalike; " + action, text=True, capture_output=True, env=env, timeout=30)
            assert denied.returncode != 0 and "permission denied" in denied.stderr
        results["nonpublisher_cannot_write_identity_or_publish"] = True
        # Physical backup uses the untouched original, not the tampered restored DB.
        gateway.terminate()
        gateway.wait(timeout=15)
        gateway = None
        run([pg_bin / "pg_ctl", "-D", logical, "-m", "fast", "-w", "stop"])
        physical_dir = evidence / "physical-pg"
        run([pg_bin / "pg_basebackup", "-h", "127.0.0.1", "-p", original_port, "-U", "postgres",
             "-D", physical_dir, "-X", "stream", "--checkpoint=fast"], env=env)
        physical_port = free_port()
        owned.append(physical_dir)
        run([pg_bin / "pg_ctl", "-D", physical_dir, "-l", evidence / "physical.log", "-o", f"-p {physical_port}", "-w", "start"])
        restart_gateway(physical_port)
        physical_result = check("pg_basebackup", "ok", publication)
        assert physical_result["publication_binding"] == baseline["publication_binding"]
        sql("ALTER TABLE _oo.table_freshness ADD COLUMN stale_reason text; "
            "UPDATE _oo.table_freshness SET state='stale',stale_reason='SCHEMA_CHANGED';")
        for path, name in [("/v1/query", "stale_query"), ("/v1/freshness", "stale_freshness")]:
            response = check(name, "ok", path=path)
            assert any(w["code"] == "STALE_DATA" and w.get("table") == "app.items" and "SCHEMA_CHANGED" in w["message"]
                       for w in response["warnings"]), response
        results["complete"] = True
    finally:
        if gateway is not None:
            gateway.terminate()
            gateway.wait(timeout=15)
        if original_gateway_pid is not None:
            os.kill(original_gateway_pid, 15)
        for cluster in reversed(owned):
            if (cluster / "postmaster.pid").exists():
                run([pg_bin / "pg_ctl", "-D", cluster, "-m", "fast", "-w", "stop"])
        results["owned_clusters_stopped"] = all(not (p / "postmaster.pid").exists() for p in owned)
        (evidence / "result.json").write_text(json.dumps(results, indent=2) + "\n")
        log.close()
    print(json.dumps(results, indent=2))


if __name__ == "__main__":
    main()
