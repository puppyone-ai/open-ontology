#!/usr/bin/env python3
"""Target-only C5 parts SQL tests in a NEW socket-only PG17 cluster.

No source, customer DSN, cloud, Docker mutation, or model calls. Retains the
cluster and exact migration bytes/logs under the requested evidence directory.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence", type=Path,
                        default=Path(tempfile.gettempdir()) / "oo-c5-parts-apply-evidence")
    parser.add_argument("--minimal-prerequisites", action="store_true",
                        help="Install through 0075 plus 0103/0104; no identity, Oracle or later C1 chain")
    args = parser.parse_args()
    product = Path(__file__).resolve().parents[4]
    pg = Path("/opt/homebrew/opt/postgresql@17/bin")
    version = subprocess.check_output([pg / "postgres", "--version"], text=True).strip()
    assert " 17." in version, version
    evidence = args.evidence / (time.strftime("%Y%m%dT%H%M%S") +
                                ("-minimal" if args.minimal_prerequisites else ""))
    evidence.mkdir(parents=True, exist_ok=False)
    root = Path(tempfile.mkdtemp(prefix="c5pa-", dir="/tmp"))
    env = {k: v for k, v in os.environ.items() if not k.startswith("PG")}
    env.update(PGHOST=str(root), PGPORT="25489", PGUSER="postgres", PGDATABASE="c5_parts_apply")
    log = (evidence / "fixture.log").open("w")
    manifest = {}
    started = False
    stop = threading.Event()
    heartbeat = None

    def run(command, **kwargs):
        return subprocess.run([str(x) for x in command], env=env, check=True,
                              stdout=log, stderr=subprocess.STDOUT, **kwargs)

    def include(path):
        relative = path.relative_to(product)
        dest = evidence / "sources" / relative
        dest.parent.mkdir(parents=True, exist_ok=True)
        data = path.read_bytes()
        dest.write_bytes(data)
        manifest[str(relative)] = hashlib.sha256(data).hexdigest()
        return f"\\i {dest}"

    try:
        include(Path(__file__).resolve())
        run([pg / "initdb", "-D", root / "data", "-U", "postgres", "-A", "trust",
             "--no-locale", "--encoding=UTF8"])
        run([pg / "pg_ctl", "-D", root / "data", "-l", evidence / "server.log",
             "-o", f"-k {root} -p 25489 -c listen_addresses='' -c log_lock_waits=on", "-w", "start"])
        started = True
        run([pg / "createdb", "c5_parts_apply"])
        jm = product / "crates/oo-jobs/migrations"
        pm = product / "crates/oo-pipeline/migrations"
        sql = ["\\set ON_ERROR_STOP on", "\\timing on",
               include(jm / "0000_bootstrap_roles.sql"),
               include(pm / "0000_bootstrap_roles.sql"),
               "GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE;",
               "GRANT EXECUTE ON FUNCTION pg_catalog.set_config(text,text,boolean) TO oo_jobs_owner;",
               "GRANT EXECUTE ON FUNCTION pg_catalog.pg_advisory_xact_lock(bigint) TO oo_jobs_owner;",
               "SET ROLE oo_jobs_owner;"]
        for path in sorted(jm.glob("*.sql")):
            number = int(path.name[:4])
            if number in (1, 2, 3, 4, 5, 6, 7, 9, 11):
                sql += ["BEGIN;", include(path)]
                if number == 3:
                    sha = manifest[str(path.relative_to(product))]
                    sql.append("INSERT INTO jobs.schema_migrations(version,checksum,phase,filename) "
                               f"VALUES('jobs-lease-operations/1','{sha}','expand','{path.name}');")
                sql.append("COMMIT;")
        sql += ["RESET ROLE;", include(product / "tools/job-worker/native_schedule.sql"),
                "GRANT USAGE ON SCHEMA jobs,jw_native TO oo_target_owner;",
                "GRANT SELECT ON ALL TABLES IN SCHEMA jobs,jw_native TO oo_target_owner;",
                "CREATE ROLE c5_parts_test_runtime NOLOGIN INHERIT;",
                "GRANT oo_pipeline_runtime,oo_jobs_runtime TO c5_parts_test_runtime;",
                "CREATE ROLE overlay_reader NOLOGIN;",
                "SET ROLE oo_target_owner;"]
        for path in sorted(pm.glob("*.sql")):
            number = int(path.name[:4])
            if args.minimal_prerequisites and 76 <= number <= 102:
                continue
            if 1 <= number <= 104:
                if number == 44:
                    sql.append("RESET ROLE;")
                if number == 104:
                    sql += ["\\t on", f"\\o {evidence / 'functions-before.json'}",
                            "SELECT jsonb_object_agg(oid::text,md5(pg_get_functiondef(oid))) "
                            "FROM pg_proc WHERE pronamespace IN ('_oo'::regnamespace,'jobs'::regnamespace) "
                            "AND prokind='f';", "\\o", "\\t off"]
                sql += ["BEGIN;", include(path), "COMMIT;"]
                if number == 44:
                    sql.append("SET ROLE oo_target_owner;")
        sql.append("RESET ROLE;")
        sql += ["\\t on", f"\\o {evidence / 'functions-after.json'}",
                "SELECT jsonb_object_agg(oid::text,md5(pg_get_functiondef(oid))) "
                "FROM pg_proc WHERE pronamespace IN ('_oo'::regnamespace,'jobs'::regnamespace) "
                "AND prokind='f';", "\\o", "\\t off"]
        install = evidence / "install.psql"
        install.write_text("\n".join(sql) + "\n")
        run([pg / "psql", "-X", "-v", "ON_ERROR_STOP=1", "-f", install])
        before = json.loads((evidence / "functions-before.json").read_text().splitlines()[0])
        after = json.loads((evidence / "functions-after.json").read_text().splitlines()[0])
        assert all(after.get(oid) == digest for oid, digest in before.items()), "historical function changed"
        fixtures = Path(__file__).resolve().parent
        # Copy companions before execution so \ir resolves only frozen files.
        for name in ("native_c5_overlay_helpers.sql", "native_c5_overlay.sql",
                     "native_c5_overlay_source_order.sql", "native_c5_overlay_scale.sql",
                     "c5_parts_stage.sql"):
            include(fixtures / name)
        for path in sorted(fixtures.glob("c5_parts_apply*.sql")):
            include(path)
        frozen = evidence / "sources" / fixtures.relative_to(product)
        def renew():
            with (evidence / "heartbeats.log").open("w") as hlog:
                while not stop.wait(2):
                    # Real API on another connection: no direct jobs writes.
                    subprocess.run([str(pg / "psql"), "-X", "-At", "-v", "ON_ERROR_STOP=1", "-c",
                        "SET ROLE oo_jobs_runtime; SET statement_timeout='5s'; "
                        "SELECT (jobs.heartbeat(project_id,id,lease_owner,generation)).status "
                        "FROM jobs.tasks WHERE state='running' AND lease_owner='c5-parts-apply';"],
                        env=env, stdout=hlog, stderr=subprocess.STDOUT, check=False)
        heartbeat = threading.Thread(target=renew, daemon=True)
        heartbeat.start()
        run([pg / "psql", "-X", "-v", "ON_ERROR_STOP=1",
             "-f", frozen / "c5_parts_apply_setup.sql", "-f", frozen / "c5_parts_apply.sql",
             "-f", frozen / "c5_parts_apply_edges.sql",
             "-f", frozen / "c5_parts_apply_legacy.sql",
             "-f", frozen / "c5_parts_apply_observation.sql"])
        stop.set()
        heartbeat.join()
        heartbeats = (evidence / "heartbeats.log").read_text()
        assert "extended" in heartbeats and "ERROR" not in heartbeats, "independent heartbeat failed"
        result = subprocess.check_output([str(pg / "psql"), "-X", "-At", "-v", "ON_ERROR_STOP=1", "-c",
            "SELECT jsonb_build_object('classification','target-only',"
            "'checks',(SELECT jsonb_agg(name ORDER BY name) FROM parts_fixture.passed),"
            "'max_stage_rows',(SELECT max(expected_rows) FROM _oo.c5_parts_capture),"
            "'max_tombstone_count',(SELECT max(tombstone_count) FROM _oo.update_batch),"
            "'parts_overlays',(SELECT count(*) FROM _oo.c5_overlay WHERE scope->>'parts_task' IS NOT NULL),"
            "'succeeded_jobs',(SELECT count(*) FROM jobs.tasks WHERE state='succeeded'),"
            "'pending_acks',(SELECT count(*) FROM jw_native.pending_acks),"
            "'published_runs',(SELECT count(*) FROM _oo.full_import_run WHERE state='published'));"],
            env=env, text=True)
        summary = json.loads(result)
        summary.update(unchanged_historical_functions=len(before),
                       independent_heartbeats=heartbeats.splitlines().count("extended"))
        (evidence / "validation.json").write_text(json.dumps(summary, indent=2) + "\n")
        outcome = "passed"
    except Exception:
        outcome = "failed"
        raise
    finally:
        stop.set()
        if heartbeat:
            heartbeat.join(timeout=10)
        if started:
            run([pg / "pg_ctl", "-D", root / "data", "-m", "fast", "-w", "stop"])
        (evidence / "manifest.json").write_text(json.dumps({
            "classification": "target-only synthetic SQL fixture",
            "source_admission_verified": False, "public_publication": False, "job_ack": False,
            "postgres": version, "cluster": str(root), "outcome": locals().get("outcome", "failed"),
            "minimal_prerequisites": args.minimal_prerequisites,
            "sha256": manifest}, indent=2) + "\n")
        log.close()
        print(evidence, flush=True)


if __name__ == "__main__":
    main()
