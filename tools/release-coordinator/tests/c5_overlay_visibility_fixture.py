#!/usr/bin/env python3
"""Owned PG17 only; no DSN or customer target is accepted.

Default: new socket-only local cluster, then the actual Rust coordinator tests.
--docker: SQL checks in NEW visibility_fixture in the explicitly owned container.
Retains logs/data, stops only the local cluster this script created.
"""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile

CONTAINER = "oo-c5-incremental-20261025-fixture-pg"
DB = "visibility_fixture"
TESTS = Path(__file__).resolve().parent
PRODUCT = TESTS.parents[2]
MIGRATIONS = PRODUCT / "crates/oo-pipeline/migrations"
HELPERS = PRODUCT / "crates/oo-pipeline/tests/fixtures/native_c5_overlay_helpers.sql"


def setup_sql(docker):
    sql = ["\\set ON_ERROR_STOP on"]
    if not docker:
        sql.append((MIGRATIONS / "0000_bootstrap_roles.sql").read_text())
        sql.append("CREATE ROLE oo_jobs_owner NOLOGIN; CREATE ROLE oo_jobs_runtime NOLOGIN;")
    sql += [
        "GRANT CONNECT,CREATE,TEMP ON DATABASE visibility_fixture TO oo_target_owner;",
        "GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE;",
        "CREATE SCHEMA jobs; CREATE TABLE jobs.tasks(id uuid PRIMARY KEY,state text,"
        "generation bigint,lease_until timestamptz,project_id uuid);",
        "GRANT USAGE ON SCHEMA jobs TO oo_target_owner,oo_pipeline_runtime;",
        "GRANT SELECT ON jobs.tasks TO oo_target_owner;",
        "SET ROLE oo_target_owner;",
    ]
    for migration in sorted(MIGRATIONS.glob("*.sql")):
        number = int(migration.name[:4])
        if 1 <= number <= 67:
            if number == 44:
                sql.append("RESET ROLE;")
            sql.extend(["BEGIN;", migration.read_text(), "COMMIT;"])
            if number == 44:
                sql.append("SET ROLE oo_target_owner;")
    sql += ["RESET ROLE;", HELPERS.read_text(),
            (TESTS / "fixtures/c5_overlay_visibility_before.sql").read_text(),
            "SET ROLE oo_target_owner; BEGIN;",
            (MIGRATIONS / "0068_native_c5_overlay_visibility.sql").read_text(),
            "COMMIT; RESET ROLE;",
            (TESTS / "fixtures/c5_overlay_visibility_after.sql").read_text(),
            "SET ROLE oo_target_owner; BEGIN;",
            (MIGRATIONS / "0075_native_c5_indexed_exclusion.sql").read_text(),
            "COMMIT; RESET ROLE;"]
    return "\n".join(sql)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--docker", action="store_true")
    args = parser.parse_args()
    root = Path(tempfile.mkdtemp(prefix="oo-c5-visibility-"))
    env = {k: v for k, v in os.environ.items() if not k.startswith("PG")}
    pg = Path("/opt/homebrew/opt/postgresql@17/bin")
    started = False
    print(f"Evidence: {root}", flush=True)
    with (root / "fixture.log").open("w") as log:
        def run(command, **kwargs):
            return subprocess.run([str(x) for x in command], env=env, check=True,
                                  stdout=log, stderr=subprocess.STDOUT, **kwargs)
        try:
            if args.docker:
                prefix = ["docker", "exec", "-i", CONTAINER, "psql", "-X", "-U", "postgres"]
                version = subprocess.check_output(prefix + ["-Atc", "SHOW server_version"], text=True)
                if not version.startswith("17."):
                    raise RuntimeError("PG17 required")
                # Fail if it exists: never reset someone else's fixture.
                run(prefix + ["-v", "ON_ERROR_STOP=1", "-c", f"CREATE DATABASE {DB}"])
                psql = prefix + ["-d", DB, "-v", "ON_ERROR_STOP=1"]
            else:
                version = subprocess.check_output([pg / "postgres", "--version"], text=True)
                if " 17." not in version:
                    raise RuntimeError("PG17 required")
                env.update(PGHOST=str(root), PGPORT="25489", PGUSER="postgres", PGDATABASE=DB)
                run([pg / "initdb", "-D", root / "data", "-U", "postgres", "-A", "trust",
                     "--no-locale", "--encoding=UTF8"])
                run([pg / "pg_ctl", "-D", root / "data", "-l", root / "server.log", "-o",
                     f"-k {root} -p 25489 -c listen_addresses='' -c max_prepared_transactions=8",
                     "-w", "start"])
                started = True
                run([pg / "createdb", DB])
                psql = [pg / "psql", "-X", "-v", "ON_ERROR_STOP=1"]
            setup = setup_sql(args.docker)
            (root / "setup.sql").write_text(setup)
            run(psql, input=setup, text=True)
            if args.docker:
                run(psql, input=(TESTS / "fixtures/c5_overlay_visibility_docker.sql").read_text(), text=True)
            else:
                env.update(OO_C5_VISIBILITY_SOCKET=str(root), OO_C5_VISIBILITY_PORT="25489")
                run(["cargo", "test", "--offline", "--locked", "--manifest-path",
                     TESTS.parent / "Cargo.toml", "--lib", "c5_overlay_visibility",
                     "--", "--ignored", "--nocapture"])
            print("PASS: owned PG17 C5 visibility fixture", flush=True)
        finally:
            if started:
                run([pg / "pg_ctl", "-D", root / "data", "-m", "fast", "-w", "stop"])


if __name__ == "__main__":
    main()
