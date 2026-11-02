#!/usr/bin/env python3
"""Run real migration SQL in a newly owned, network-isolated PostgreSQL 17.

Receipts/data are synthetic fixtures, not a source-to-loader end-to-end test.
This does not use an existing database, host port, credentials or Docker volume.
"""

import hashlib
import pathlib
import subprocess
import tempfile
import time
import uuid


HERE = pathlib.Path(__file__).resolve().parent
MIGRATIONS = HERE.parent.parent / "migrations"


def run(args, **kwargs):
    return subprocess.run(args, text=True, capture_output=True, check=True, **kwargs)


def main():
    name = "oo-c5-init-0065-" + uuid.uuid4().hex[:12]
    log = pathlib.Path(tempfile.mkdtemp(prefix=name + "-")) / "fixture.log"
    run([
        "docker", "run", "-d", "--rm", "--name", name,
        "--label", "oo.task=c5-init-0065", "--network", "none",
        "-e", "POSTGRES_HOST_AUTH_METHOD=trust", "postgres:17",
    ])

    def sql(text, label):
        result = subprocess.run(
            ["docker", "exec", "-i", name, "psql", "-X", "-U", "postgres",
             "-v", "ON_ERROR_STOP=1"],
            input=text, text=True, capture_output=True,
        )
        with log.open("a") as out:
            out.write(f"\n--- {label} (exit {result.returncode}, SQL SHA256 {hashlib.sha256(text.encode()).hexdigest()}) ---\n")
            out.write(result.stdout + result.stderr)
        if result.returncode:
            raise RuntimeError(f"{label}\n{result.stdout[-2000:]}\n{result.stderr}")

    try:
        for _ in range(100):
            ready = subprocess.run(
                ["docker", "exec", name, "pg_isready", "-h", "127.0.0.1", "-U", "postgres"],
                capture_output=True,
            )
            if ready.returncode == 0:
                break
            time.sleep(0.1)
        else:
            raise RuntimeError("owned PostgreSQL did not become ready")
        for path in sorted(MIGRATIONS.glob("*.sql")):
            version = int(path.name[:4])
            if version > 65:
                continue
            role = "" if version in (0, 44) else "SET LOCAL ROLE oo_target_owner;"
            sql(f"BEGIN; {role}\n{path.read_text()}\nCOMMIT;", path.name)
            if version == 0:
                sql("""
                    CREATE ROLE oo_jobs_owner NOLOGIN;
                    CREATE ROLE oo_jobs_runtime NOLOGIN;
                    CREATE ROLE oo_target_admin NOLOGIN;
                    GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE;
                    CREATE SCHEMA jobs;
                    CREATE TABLE jobs.tasks(id uuid PRIMARY KEY,state text,generation bigint,
                        lease_until timestamptz,project_id uuid);
                    GRANT USAGE ON SCHEMA jobs TO oo_target_owner,oo_pipeline_runtime;
                    GRANT SELECT ON jobs.tasks TO oo_target_owner;
                """, "fixture-only prerequisite roles")
        sql((HERE / "c5_multipart_initialize_fixture.sql").read_text(), "0065 assertions")
        print("PASS: real migrations 0000..0065 and initializer/observation SQL fixtures")
    finally:
        # Only the randomly named container created by this invocation is stopped.
        subprocess.run(["docker", "stop", name], capture_output=True, check=True)
        print(f"Evidence: {log}")


if __name__ == "__main__":
    main()
