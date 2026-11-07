#!/usr/bin/env python3
"""Two real SIGKILLs at the C5 SQL loader boundary in an owned PG17 container.

No external DSN is accepted. Synthetic admin fixtures bypass source admission;
this proves target staging/apply/rebind, not a SourceEngine end-to-end import.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import secrets
import signal
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[3]
FIXTURES = Path(__file__).parent / "fixtures"


def sql(container, statement, log=None):
    result = subprocess.run(
        ["docker", "exec", "-i", container, "psql", "-X", "-At",
         "-v", "ON_ERROR_STOP=1", "-U", "postgres", "-d", "postgres"],
        input=statement, text=True, capture_output=True, check=False,
    )
    if log is not None:
        log.write(result.stdout + result.stderr)
        log.flush()
    if result.returncode:
        raise RuntimeError(result.stderr)
    return result.stdout.strip()


def child(container, phase, marker):
    statement = """
BEGIN;
SET LOCAL ROLE c5_parts_test_runtime;
SELECT parts_fixture.stage(ctx,0,1) FROM parts_fixture.saved WHERE label='large';
SELECT _oo.c5_parts_complete(ctx->>'source',(ctx->>'task')::uuid,
 (ctx->>'job_generation')::bigint,repeat('b',64))
 FROM parts_fixture.saved WHERE label='large';
"""
    if phase == "commit":
        statement += """
RESET ROLE;
UPDATE parts_fixture.saved SET ctx=ctx||parts_fixture.open(ctx->>'source',ctx->>'table')
 WHERE label='large';
SET LOCAL ROLE c5_parts_test_runtime;
SELECT parts_fixture.apply(ctx) FROM parts_fixture.saved WHERE label='large';
"""
    print(sql(container, statement + "COMMIT;"), flush=True)
    Path(marker).write_text(phase)
    while True:
        time.sleep(60)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence", type=Path, required=True)
    args = parser.parse_args()
    evidence = args.evidence.resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    assert evidence.is_relative_to(ROOT / "evidence")
    assert subprocess.run(["git", "check-ignore", "-q", str(evidence)], cwd=ROOT).returncode == 0
    suffix = secrets.token_hex(6)
    container = "oo-m-s1-" + suffix
    volume = container + "-data"
    password = evidence / "postgres-password"
    with open(password, "x", opener=lambda path, flags: os.open(path, flags, 0o600)) as f:
        f.write(secrets.token_urlsafe(32))
    manifest = {}
    log = (evidence / "sql.log").open("w")
    created_volume = False
    created_container = False
    owned_child = None

    def run(statement):
        return sql(container, statement, log)

    def source(path):
        raw = path.read_bytes()
        manifest[str(path.relative_to(ROOT))] = hashlib.sha256(raw).hexdigest()
        return raw.decode()

    def crash(phase):
        nonlocal owned_child
        marker = evidence / (phase + ".ready")
        with (evidence / (phase + ".log")).open("w") as out:
            owned_child = subprocess.Popen(
                [sys.executable, __file__, "--child", container, phase, str(marker)],
                stdout=out, stderr=subprocess.STDOUT,
            )
            deadline = time.monotonic() + 120
            while not marker.exists():
                if owned_child.poll() is not None:
                    raise RuntimeError(f"{phase} child failed; see {phase}.log")
                if time.monotonic() > deadline:
                    raise RuntimeError(f"{phase} checkpoint timed out")
                time.sleep(0.05)
            owned_child.kill()
            assert owned_child.wait(timeout=10) == -signal.SIGKILL
            owned_child = None

    def reclaim():
        # Let the actual 30-second lease expire. Never update jobs tables.
        wait = float(run("""
SELECT greatest(0,extract(epoch FROM lease_until-clock_timestamp()))+0.1
 FROM jobs.tasks WHERE id=(SELECT (ctx->>'task')::uuid FROM parts_fixture.saved WHERE label='large');
"""))
        time.sleep(wait)
        return int(run("""
WITH claim AS MATERIALIZED (
 SELECT claimed.* FROM parts_fixture.saved
 CROSS JOIN LATERAL jobs.claim_selected((ctx->>'project')::uuid,(ctx->>'task')::uuid,
 ARRAY['native_capture'],'c5-parts-apply') claimed WHERE label='large'
), updated AS (
 UPDATE parts_fixture.saved SET ctx=jsonb_set(ctx,'{job_generation}',to_jsonb(claim.generation))
 FROM claim WHERE label='large' AND claim.status='claimed' RETURNING claim.generation
) SELECT generation FROM updated;
"""))

    try:
        source(Path(__file__).resolve())
        subprocess.run(["docker", "volume", "create", "--label", "oo.lane=m", volume],
                       check=True, stdout=subprocess.DEVNULL)
        created_volume = True
        subprocess.run(
            ["docker", "run", "-d", "--name", container, "--label", "oo.lane=m",
             "--network", "none", "--mount", f"type=volume,source={volume},target=/var/lib/postgresql/data",
             "--mount", f"type=bind,source={password},target=/run/pg-password,readonly",
             "-e", "POSTGRES_PASSWORD_FILE=/run/pg-password", "postgres:17"],
            check=True, stdout=subprocess.DEVNULL,
        )
        created_container = True
        deadline = time.monotonic() + 60
        while subprocess.run(["docker", "exec", container, "pg_isready", "-U", "postgres"],
                             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode:
            if time.monotonic() > deadline:
                raise RuntimeError("PG17 startup timed out")
            time.sleep(0.2)
        version = run("SHOW server_version;")
        assert version.startswith("17."), version
        image = subprocess.check_output(
            ["docker", "inspect", "--format", "{{.Image}}", container], text=True).strip()
        jm = ROOT / "crates/oo-jobs/migrations"
        pm = ROOT / "crates/oo-pipeline/migrations"
        setup = [source(jm / "0000_bootstrap_roles.sql"), source(pm / "0000_bootstrap_roles.sql"),
                 "GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE;",
                 "GRANT EXECUTE ON FUNCTION pg_catalog.set_config(text,text,boolean) TO oo_jobs_owner;",
                 "GRANT EXECUTE ON FUNCTION pg_catalog.pg_advisory_xact_lock(bigint) TO oo_jobs_owner;",
                 "SET ROLE oo_jobs_owner;"]
        for path in sorted(jm.glob("*.sql")):
            number = int(path.name[:4])
            if number in (1, 2, 3, 4, 5, 6, 7, 9, 11):
                setup += ["BEGIN;", source(path)]
                if number == 3:
                    digest = manifest[str(path.relative_to(ROOT))]
                    setup.append("INSERT INTO jobs.schema_migrations(version,checksum,phase,filename) "
                                 f"VALUES('jobs-lease-operations/1','{digest}','expand','{path.name}');")
                setup.append("COMMIT;")
        setup += ["RESET ROLE;", source(ROOT / "tools/job-worker/native_schedule.sql"),
                  "GRANT USAGE ON SCHEMA jobs,jw_native TO oo_target_owner;",
                  "GRANT SELECT ON ALL TABLES IN SCHEMA jobs,jw_native TO oo_target_owner;",
                  "CREATE ROLE c5_parts_test_runtime NOLOGIN INHERIT;",
                  "GRANT oo_pipeline_runtime,oo_jobs_runtime TO c5_parts_test_runtime;",
                  "CREATE ROLE overlay_reader NOLOGIN;", "SET ROLE oo_target_owner;"]
        for path in sorted(pm.glob("*.sql")):
            number = int(path.name[:4])
            if 1 <= number <= 75 or number in (103, 104, 119):
                if number == 44:
                    setup.append("RESET ROLE;")
                setup += ["BEGIN;", source(path), "COMMIT;"]
                if number == 44:
                    setup.append("SET ROLE oo_target_owner;")
        fixture = source(FIXTURES / "c5_parts_apply_setup.sql").replace(
            r"\ir native_c5_overlay_helpers.sql", source(FIXTURES / "native_c5_overlay_helpers.sql"))
        # No apply transaction exists until the entire private range is durable.
        fixture = fixture.replace(
            "parts_fixture.claim(parts_fixture.open('parts_large','parts_large'),100001)",
            "parts_fixture.claim(jsonb_build_object('source','parts_large','table','parts_large',"
            "'generation',2,'spec',parts_fixture.spec()),100001)")
        setup += ["RESET ROLE;", fixture]
        run("\n".join(setup))
        task = run("SELECT ctx->>'task' FROM parts_fixture.saved WHERE label='large';")
        epoch1 = int(run("SELECT ctx->>'job_generation' FROM parts_fixture.saved WHERE label='large';"))
        run("""
SELECT parts_fixture.ok(NOT has_table_privilege('oo_pipeline_runtime','_oo.c5_write_meter','SELECT'),
 'meter does not grant runtime source access');
BEGIN;
SET LOCAL ROLE c5_parts_test_runtime;
SELECT parts_fixture.stage(ctx,0,0) FROM parts_fixture.saved WHERE label='large';
ROLLBACK;
SELECT parts_fixture.ok(NOT EXISTS(SELECT FROM _oo.c5_write_meter),'rolled back writes are not committed bytes');
SELECT parts_fixture.ok(NOT EXISTS(SELECT FROM _oo.c5_parts_receipt),'rolled back stage is retryable');
""")
        crash("stage")
        run("""
SELECT parts_fixture.ok((SELECT count(*) FROM _oo.c5_parts_receipt)=2,'staging receipts survived kill');
SELECT parts_fixture.ok((SELECT count(*) FROM _oo.c5_write_meter)=4,'durable inbox and stage bytes');
SELECT parts_fixture.ok(NOT EXISTS(SELECT FROM _oo.c5_parts_apply),'no partial apply');
SELECT parts_fixture.ok((SELECT count(*) FROM _oo.c5_block_receipt)=0,'staging not published');
""")
        epoch2 = reclaim()
        assert epoch2 > epoch1
        crash("commit")
        before = run("SELECT jsonb_agg(to_jsonb(m) ORDER BY phase,ordinal) FROM _oo.c5_write_meter m;")
        epoch3 = reclaim()
        assert epoch3 > epoch2
        run("""
BEGIN;
SET LOCAL ROLE c5_parts_test_runtime;
SELECT _oo.c5_parts_rebind(ctx->>'source',(ctx->>'task')::uuid,
 (ctx->>'job_generation')::bigint) FROM parts_fixture.saved WHERE label='large';
COMMIT;
""")
        after = run("SELECT jsonb_agg(to_jsonb(m) ORDER BY phase,ordinal) FROM _oo.c5_write_meter m;")
        assert before == after, "replay changed target write metering"
        run("""
SELECT parts_fixture.ok((SELECT count(*) FROM _oo.c5_block_receipt)=1,'exactly one range apply');
SELECT parts_fixture.ok((SELECT count(*) FROM _oo.c5_parts_receipt)=2,'no restaging on resume');
SELECT parts_fixture.ok((SELECT count(*) FROM _oo.c5_write_meter)=5,'replay writes zero bytes');
SELECT parts_fixture.ok((SELECT count(*) FROM _oo.c5_overlay)=1,'one delta generation');
SELECT parts_fixture.ok((SELECT delta_rows=100001 AND inherited_rows=1 FROM _oo.c5_overlay),
 'unchanged parent row inherited, not copied');
SELECT parts_fixture.ok(overlay_fixture.read_rows(ctx)='[]','publication gate remains closed')
 FROM parts_fixture.saved WHERE label='large';
SET ROLE oo_target_owner;
DO $$
DECLARE c jsonb; base jsonb; actual jsonb; expected jsonb;
BEGIN
 SELECT ctx INTO c FROM parts_fixture.saved WHERE label='large';
 SELECT ctx INTO base FROM parts_fixture.saved WHERE label='base';
 actual:=overlay_fixture.read_rows(c);
 SELECT jsonb_agg(row ORDER BY row::text COLLATE "C") INTO expected FROM (
  SELECT jsonb_build_array(id::text,
    CASE WHEN id%3=0 THEN '-0' WHEN id%3=1 THEN 'NaN' ELSE '1.25' END,
    '12345678901234567890.123456789012345678',
    CASE WHEN id%5=0 THEN NULL ELSE 'new'||id::text END) row
  FROM (SELECT CASE WHEN v=1 THEN 1 ELSE v+1 END id FROM generate_series(1,100001) v) ids
  UNION ALL SELECT '["200000","2.5","4.000000000000000000","keep"]'::jsonb
 ) rows;
 IF actual IS DISTINCT FROM expected THEN
  RAISE NOTICE 'unexpected rows: %', (SELECT jsonb_agg(value) FROM (
   SELECT value FROM jsonb_array_elements(actual)
   EXCEPT SELECT value FROM jsonb_array_elements(expected) LIMIT 5) differences);
  RAISE NOTICE 'missing rows: %', (SELECT jsonb_agg(value) FROM (
   SELECT value FROM jsonb_array_elements(expected)
   EXCEPT SELECT value FROM jsonb_array_elements(actual) LIMIT 5) differences);
 END IF;
 PERFORM parts_fixture.ok(actual=expected,'row-level exact comparison after two SIGKILLs');
 SELECT jsonb_agg(value ORDER BY value::text COLLATE "C") INTO expected
 FROM jsonb_array_elements('[
  ["1","1.25","1.000000000000000001","old"],
  ["2","0","2.000000000000000000",null],
  ["3","-0","3.000000000000000000","old3"],
  ["200000","2.5","4.000000000000000000","keep"]]'::jsonb);
 PERFORM parts_fixture.ok(overlay_fixture.read_rows(base)=expected,'every parent cell unchanged');
END $$;
RESET ROLE;
""")
        assert run("SELECT ctx->>'task' FROM parts_fixture.saved WHERE label='large';") == task
        meters = json.loads(after)
        summary = {
            "status": "passed", "classification": "target-only SQL loader recovery",
            "postgres": version, "image": image, "task": task, "lease_epochs": [epoch1, epoch2, epoch3],
            "sigkills": 2, "row_comparison": "100002 exact rows", "duplicate_apply": 0,
            "logical_tuple_bytes": sum(m["bytes"] for m in meters),
            "source_admission_verified": False, "published": False, "manifest": manifest,
        }
        (evidence / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
        print(json.dumps({k: v for k, v in summary.items() if k != "manifest"}))
    finally:
        if owned_child is not None and owned_child.poll() is None:
            owned_child.kill()
            owned_child.wait(timeout=10)
        log.close()
        if created_container:
            subprocess.run(["docker", "rm", "-f", container], check=True, stdout=subprocess.DEVNULL)
        if created_volume:
            subprocess.run(["docker", "volume", "rm", volume], check=True, stdout=subprocess.DEVNULL)
        password.unlink()


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--child":
        child(*sys.argv[2:])
    else:
        main()
