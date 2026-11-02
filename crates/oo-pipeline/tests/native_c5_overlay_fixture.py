#!/usr/bin/env python3
"""Real SQL fixture in a NEW, socket-only PostgreSQL 17 cluster.

No DSN is accepted: this cannot connect to a customer or existing target.
Run: python3 product/crates/oo-pipeline/tests/native_c5_overlay_fixture.py
PG_BIN may point to an existing PostgreSQL 17 bin directory; nothing is installed.
The stopped fixture cluster and logs are retained in the printed temp directory.
"""

import os
import argparse
from pathlib import Path
import shutil
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    selection = parser.add_mutually_exclusive_group()
    selection.add_argument("--scale-only", action="store_true",
                           help="Run flat multi-chunk/million-row cases independently of lifecycle cases")
    selection.add_argument("--source-order-only", action="store_true",
                           help="Run admitted source-order cases independently of lifecycle cases")
    args = parser.parse_args()
    tests = Path(__file__).resolve().parent
    migrations = tests.parent / "migrations"
    default = "/opt/homebrew/opt/postgresql@17/bin"
    pg = Path(os.environ.get("PG_BIN", default))
    if not (pg / "initdb").is_file():
        located = shutil.which("initdb")
        if not located:
            raise SystemExit("Existing PostgreSQL 17 initdb/pg_ctl/psql required")
        pg = Path(located).parent
    version = subprocess.check_output([pg / "postgres", "--version"], text=True)
    if " 17." not in version:
        raise SystemExit(f"PostgreSQL 17 required, got {version.strip()}")
    root = Path(tempfile.mkdtemp(prefix="oo-c5-overlay-fixture-"))
    print(f"Owned fixture only: {root}", flush=True)
    env = {k: v for k, v in os.environ.items() if not k.startswith("PG")}
    env.update(PGHOST=str(root), PGUSER="postgres", PGDATABASE="overlay_fixture", PGPORT="25489")
    started = False
    with (root / "fixture.log").open("w") as log:
        def run(args, **kwargs):
            return subprocess.run([str(x) for x in args], env=env, check=True,
                                  stdout=log, stderr=subprocess.STDOUT, **kwargs)

        try:
            run([pg / "initdb", "-D", root / "data", "-U", "postgres", "-A", "trust",
                 "--no-locale", "--encoding=UTF8"])
            run([pg / "pg_ctl", "-D", root / "data", "-l", root / "server.log",
                 "-o", f"-k {root} -p 25489 -c listen_addresses='' -c fsync=off", "-w", "start"])
            started = True
            run([pg / "createdb", "overlay_fixture"])
            sql = ["\\set ON_ERROR_STOP on", "\\timing on"]
            sql += [f"\\i {migrations / '0000_bootstrap_roles.sql'}",
                    "GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE;",
                    "CREATE ROLE oo_jobs_owner NOLOGIN; CREATE ROLE oo_jobs_runtime NOLOGIN;",
                    "CREATE ROLE overlay_reader NOLOGIN;",
                    "CREATE SCHEMA jobs; CREATE TABLE jobs.tasks(id uuid PRIMARY KEY,state text,"
                    "generation bigint,lease_until timestamptz,project_id uuid);",
                    "GRANT USAGE ON SCHEMA jobs TO oo_target_owner,oo_pipeline_runtime;",
                    "GRANT SELECT ON jobs.tasks TO oo_target_owner;",
                    "SET ROLE oo_target_owner;"]
            for migration in sorted(migrations.glob("*.sql")):
                number = int(migration.name[:4])
                if 1 <= number <= 66 or number == 70:
                    if number == 44:
                        sql.append("RESET ROLE;")
                    sql.extend(["BEGIN;", f"\\i {migration}", "COMMIT;"])
                    if number == 44:
                        sql.append("SET ROLE oo_target_owner;")
            sql.append("RESET ROLE;")
            if args.scale_only or args.source_order_only:
                case = "scale" if args.scale_only else "source_order"
                sql += [f"\\i {tests / 'fixtures/native_c5_overlay_helpers.sql'}",
                        f"\\i {tests / ('fixtures/native_c5_overlay_' + case + '.sql')}"]
            else:
                sql.append(f"\\i {tests / 'fixtures/native_c5_overlay.sql'}")
            install = root / "run.psql"
            install.write_text("\n".join(sql) + "\n")
            run([pg / "psql", "-X", "-v", "ON_ERROR_STOP=1", "-f", install])
            case = ("scale subset" if args.scale_only
                    else "source-order subset" if args.source_order_only else "full suite")
            print(f"PASS: C5 immutable overlay SQL fixture: {case}", flush=True)
        finally:
            if started:
                run([pg / "pg_ctl", "-D", root / "data", "-m", "fast", "-w", "stop"])
            print(f"SQL evidence: {root / 'fixture.log'}", flush=True)


if __name__ == "__main__":
    main()
