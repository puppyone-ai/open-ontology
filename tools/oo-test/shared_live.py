"""Live check: suites leasing one owned snapshot clone stay isolated.

python3 tools/oo-test/shared_live.py --engine postgres --version 17 --run-id ID
Each lease leaves behind what a real suite leaves (replication slot,
publication, changed and inserted rows, a new table); the next lease must see
the pristine fixture and gold answers. Owned lane `ta` containers only.
"""
import argparse
import json
import subprocess
import time

import runner
import shared
import snapshots
import t1


def dirty(run, suite):
    if run.engine == "postgres":
        run.sql(f"SELECT pg_create_physical_replication_slot('oo_{suite.lower()}_slot');"
                f"CREATE PUBLICATION oo_{suite.lower()}_pub FOR ALL TABLES;"
                "ALTER SYSTEM SET work_mem='8MB';"
                "UPDATE customers SET name=name||'x' WHERE id=1;"
                f"CREATE TABLE leak_{suite.lower()}(id int);", admin=True)
    else:
        run.sql("UPDATE customers SET name=CONCAT(name,'x') WHERE id=1;"
                f"CREATE TABLE leak_{suite.lower()}(id int);", admin=True)


def observe(run):
    table = "pg_tables WHERE schemaname='public'" if run.engine == "postgres" else \
        "information_schema.tables WHERE table_schema='fixture'"
    column = "tablename" if run.engine == "postgres" else "table_name"
    _, names = run.sql(f"SELECT {column} FROM {table} ORDER BY 1;", admin=True)
    _, name = run.sql("SELECT name FROM customers WHERE id=1;", admin=True)
    return {"tables": names.split(), "name": name.strip(), "gold": run.gold()}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--engine", choices=("postgres", "mysql"), required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--suites", default="T0,T1,T2,T4,T7,T8")
    args = parser.parse_args(argv)
    t1.LANE = "ta"
    root = runner.ROOT / "evidence" / args.run_id
    root.mkdir(mode=0o700, parents=True)
    arch = subprocess.check_output(["docker", "info", "--format", "{{.Architecture}}"], text=True)
    native = "linux/arm64" if arch.strip() in ("arm64", "aarch64") else "linux/amd64"
    item = runner.resolve(args.engine, args.version)
    platform = native if native in item["platforms"] else "linux/amd64"
    options = argparse.Namespace(scale=2000, tables=3, platform=platform, health_timeout=None)
    key, manifest = snapshots.identity(args.engine, args.version, 2000, 3, platform)
    path = root / f"{args.engine}-{args.version}"
    path.mkdir(mode=0o700)
    result = {"engine": args.engine, "version": args.version, "status": "failed", "leases": []}
    run = None
    started = time.monotonic()
    try:
        with snapshots.locked_cache(key) as cache:
            run = snapshots.SnapshotRun.__new__(snapshots.SnapshotRun)
            run.__init__(path, args.engine, args.version, options, cache, key, manifest)
            run.setup()
            run.seed()
            result["setup_seconds"] = round(time.monotonic() - started, 3)
            source = shared.SharedSource(run)
            source.prepare()
            pristine = observe(run)
            for suite in args.suites.split(","):
                with source.lease(suite):
                    seen = observe(run)
                    if seen != pristine:
                        result["diff"] = {k: [pristine[k], seen[k]] for k in seen
                                          if seen[k] != pristine[k]}
                        raise RuntimeError("lease_saw_previous_suite_state:" + suite)
                    dirty(run, suite)
                    if observe(run) == pristine:
                        raise RuntimeError("dirty_step_had_no_effect")
            if observe(run) != pristine:
                raise RuntimeError("final_state_not_pristine")
            result["leases"] = [{k: lease[k] for k in ("suite", "reset_seconds", "removed")}
                                for lease in source.leases]
            result["status"] = "passed"
    except Exception as error:
        import traceback
        result["error"] = str(error) if isinstance(error, RuntimeError) else repr(error)
        result["trace"] = [f"{f.name}:{f.lineno}" for f in traceback.extract_tb(error.__traceback__)]
    finally:
        if run is not None and hasattr(run, "private") and run.cleanup():
            result["status"] = "failed"
            result["cleanup_failed"] = True
        result["wall_seconds"] = round(time.monotonic() - started, 3)
        runner.write_json(path / "shared-live.json", result)
    print(json.dumps(result))
    return 0 if result["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
