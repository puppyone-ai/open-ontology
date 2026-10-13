#!/usr/bin/env python3
"""Install the real queue in a target database, with checksum-checked replay.

Credentials use libpq service/pgpass files or the owned container's local socket.
No caller-written claim, lease, receipt, or successful migration marker is read.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess


def literal(value):
    return "'" + value.replace("'", "''") + "'"


def program(loader_role):
    migrations = Path(__file__).resolve().parents[1] / "migrations"
    entries = [
        ("0000_bootstrap_roles.sql", "jobs-bootstrap/1", "bootstrap"),
        ("0001_jobs_control_expand.sql", "jobs-control/1", "expand"),
        ("0002_event_project_binding_expand.sql", "jobs-event-project/1", "expand"),
        ("0003_job_lease_operations_expand.sql", "jobs-lease-operations/1", "expand"),
        ("0004_loader_hold_expand.sql", "jobs-loader-hold/1", "expand"),
    ]
    source = {name: (migrations / name).read_bytes() for name, _, _ in entries}
    # Cluster roles can predate this database. Do not grant owner membership to
    # the runtime or accept powerful preexisting roles.
    sql = ["BEGIN; SELECT pg_advisory_xact_lock(6401202601);",
           "DO $roles$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='oo_jobs_owner') THEN CREATE ROLE oo_jobs_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION; END IF; IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='oo_jobs_runtime') THEN CREATE ROLE oo_jobs_runtime NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION; END IF; IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname IN ('oo_jobs_owner','oo_jobs_runtime') AND (rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolbypassrls OR rolreplication)) THEN RAISE EXCEPTION 'unsafe preexisting jobs role' USING ERRCODE='42501'; END IF; END $roles$;",
           "DO $database$ BEGIN EXECUTE format('GRANT CONNECT, CREATE ON DATABASE %I TO oo_jobs_owner',current_database()); EXECUTE format('GRANT CONNECT ON DATABASE %I TO oo_jobs_runtime',current_database()); END $database$;"]
    bootstrap = source[entries[0][0]].decode()
    bootstrap = re.sub(r"CREATE ROLE oo_jobs_(?:owner|runtime)\s+.*?;", "", bootstrap, flags=re.S)
    for name, version, phase in entries:
        digest = hashlib.sha256(source[name]).hexdigest()
        body = bootstrap if phase == "bootstrap" else source[name].decode()
        owner = "RESET ROLE;" if phase == "bootstrap" else "SET LOCAL ROLE oo_jobs_owner;"
        # Bootstrap has no bookkeeping table yet. An untracked existing schema
        # is rejected by CREATE SCHEMA, never silently adopted.
        sql.append(owner)
        sql.append("DO $migration$ DECLARE prior text; BEGIN "
                   "IF to_regclass('jobs.schema_migrations') IS NOT NULL THEN "
                   "SELECT checksum INTO prior FROM jobs.schema_migrations WHERE version=" + literal(version) + "; END IF; "
                   "IF prior IS NOT NULL AND prior <> " + literal(digest) + " THEN RAISE EXCEPTION 'migration checksum mismatch' USING ERRCODE='OO001'; "
                   "ELSIF prior IS NULL THEN EXECUTE " + literal(body) + "; "
                   + ("" if phase == "bootstrap" else
                      "INSERT INTO jobs.schema_migrations(version,checksum,phase,filename) VALUES (" +
                      ",".join(map(literal, (version, digest, phase, name))) + "); ") +
                   "END IF; END $migration$;")
        if phase == "bootstrap":
            # Insert bootstrap bookkeeping immediately after schema creation.
            bootstrap_digest = digest
        if version == "jobs-control/1":
            sql.append("INSERT INTO jobs.schema_migrations(version,checksum,phase,filename) SELECT " +
                       ",".join(map(literal, ("jobs-bootstrap/1", bootstrap_digest, "expand", entries[0][0]))) +
                       " WHERE NOT EXISTS (SELECT 1 FROM jobs.schema_migrations WHERE version='jobs-bootstrap/1');")
    sql.append("RESET ROLE;")
    if loader_role:
        quoted = '"' + loader_role.replace('"', '""') + '"'
        sql.append("GRANT oo_jobs_runtime TO " + quoted + " WITH INHERIT TRUE, SET FALSE;")
    sql.extend(["COMMIT;", "SELECT version,checksum,filename FROM jobs.schema_migrations ORDER BY version;"])
    return "\n".join(sql) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    choice = parser.add_mutually_exclusive_group(required=True)
    choice.add_argument("--service", help="private libpq service name; no password in arguments")
    choice.add_argument("--container", help="owned PostgreSQL fixture container; uses local socket")
    parser.add_argument("--database")
    parser.add_argument("--admin", default="postgres")
    parser.add_argument("--loader-role", help="existing internal loader login, never an Agent login")
    parser.add_argument("--print-sql", action="store_true", help="review only; does not connect")
    args = parser.parse_args()
    sql = program(args.loader_role)
    if args.print_sql:
        print(sql)
        return
    if args.container:
        if not args.database:
            parser.error("--container requires --database")
        cmd = ["docker", "exec", "-i", args.container, "psql", "-X", "-U", args.admin, "-d", args.database]
    else:
        cmd = ["psql", "-X", "service=" + args.service]
    result = subprocess.run(cmd + ["-v", "ON_ERROR_STOP=1"], input=sql.encode(), capture_output=True)
    print(json.dumps({"protocol": "oo.jobs.deploy/v1", "exit_code": result.returncode,
                      "sql_sha256": hashlib.sha256(sql.encode()).hexdigest(),
                      "stdout": result.stdout.decode(errors="replace"),
                      "stderr": result.stderr.decode(errors="replace")}))
    raise SystemExit(result.returncode)


if __name__ == "__main__":
    main()
