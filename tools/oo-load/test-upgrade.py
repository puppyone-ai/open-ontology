#!/usr/bin/env python3
"""Upgrade from the pre-0129 retained layout (no sealed column catalog).

An older oo-load binary, built from a commit before migration 0129, creates the
baseline and its first C5 (and PostgreSQL C1) cycle. The current binary then
runs one cycle against that target. Unchanged tables must be healthy after that
one cycle, sealed from their legacy lineage; a table whose shape changed since
the legacy fence must stay `schema_unproven` (never guessed). Owned fixtures
only; not Gateway acceptance.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools/oo-test"))
import runner  # noqa: E402
import t1  # noqa: E402
import t3t4  # noqa: E402


def catalog_rows(h):
    _, exists = h.tgt("SELECT to_regclass('_oo_load.column_catalog') IS NOT NULL;")
    if exists.strip() != "t":
        return None
    _, count = h.tgt("SELECT count(*) FROM _oo_load.column_catalog;")
    return int(count.strip())


def create(h, tables):
    for table in tables:
        h.src(f"CREATE TABLE {table}(id bigint PRIMARY KEY,payload varchar(80) NOT NULL,n int NOT NULL);")
        h.src(f"INSERT INTO {table} VALUES(1,'base',7),(2,'base',8);")


def healthy(rows, tables):
    by_name = {(r["table"]["name"] if isinstance(r["table"], dict) else r["table"]).split(".")[-1]: r
               for r in rows if "table" in r}
    return all(by_name.get(t, {}).get("status") in ("fresh", "noop") for t in tables), by_name


def c5_case(h, old, new):
    sid = "fixa_up_c5"
    tables = [sid + "_same", sid + "_grow"]
    changed = sid + "_changed"
    create(h, tables + [changed])
    h.binary = old
    h.c5_binding(sid, tables + [changed])
    before = catalog_rows(h)
    _, legacy = h.tgt("SELECT count(*) FROM _oo_load.c5_state WHERE state->>'lineage' IS NOT NULL;")
    # Rows change; one table's published shape changes behind the legacy fence.
    h.src(f"INSERT INTO {tables[1]} VALUES(3,'after-upgrade',9);")
    h.src(f"ALTER TABLE {changed} {'MODIFY payload varchar(80) NULL' if h.run.engine == 'mysql' else 'ALTER COLUMN payload DROP NOT NULL'};")
    h.binary = new
    code, rows, error = h.c5(sid, required=False, run_id=sid + "-upgraded-1")
    ok, by_name = healthy(rows, tables)
    second, again, error2 = h.c5(sid, required=False, run_id=sid + "-upgraded-2")
    ok2, by_name2 = healthy(again, tables)
    passed = (before is None and int(legacy.strip()) == 3 and code == 3 and ok
              and all(by_name[t].get("schema_upgrade") == "legacy_lineage_sealed" for t in tables)
              and by_name[tables[1]]["status"] == "fresh"
              and by_name.get(changed, {}).get("stale_reason") == "schema_unproven"
              and catalog_rows(h) == 2
              and second == 3 and ok2 and all(by_name2[t].get("schema_upgrade") is None for t in tables)
              and all(by_name2[t]["status"] == "noop" for t in tables))
    return {"status": "passed" if passed else "failed", "catalog_before": before,
            "legacy_lineages": int(legacy.strip()), "first": {"exit": code, "error": error, "tables": rows},
            "second": {"exit": second, "error": error2, "tables": again}}


def c1_case(h, old, new):
    sid = "fixa_up_c1"
    tables = [sid + "_a", sid + "_b"]
    create(h, tables)
    h.binary = old
    h.c1_binding(sid, tables)
    first = h.c1(sid)
    sealed = catalog_rows(h) or 0
    h.binary = new
    h.dml([tables[0]], 1000)
    code, results, error = h.c1(sid, required=False)
    # The first cycle seals from the legacy lineage; drained cycles stay healthy.
    upgraded, by_name = healthy(results[0]["receipts"], tables)
    passed = (code == 0 and upgraded and catalog_rows(h) == sealed + 2
              and all(healthy(step["receipts"], tables)[0] for step in results)
              and all(by_name[t].get("schema_upgrade") == "legacy_lineage_sealed" for t in tables))
    return {"status": "passed" if passed else "failed", "old_cycle": first[0],
            "exit": code, "error": error, "results": results}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--engine", required=True, choices=["postgres", "mysql"])
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--old-binary", type=Path, required=True,
                        help="oo-load built from a commit before migration 0129")
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/oo-load")
    parser.add_argument("--lane", default=os.environ.get("OO_TEST_LANE", "fhp"))
    args = parser.parse_args()
    if not args.run_id or any(not (c.isalnum() or c in "-_") for c in args.run_id):
        parser.error("invalid run-id")
    t1.LANE = args.lane
    version = {"postgres": "17", "mysql": "8.4"}[args.engine]
    path = ROOT / "evidence" / args.run_id
    path.mkdir(mode=0o700, exist_ok=False)
    run = t3t4.Source(path, args.engine, version,
                      argparse.Namespace(platform=None, health_timeout=1200, scale=1, tables=0, worker_image=None))
    old, new = args.old_binary.resolve(), args.binary.resolve()
    h = t3t4.Harness(run, new)
    report = {"engine": args.engine, "version": version, "cases": {},
              "old_binary_sha256": hashlib.sha256(old.read_bytes()).hexdigest(),
              "binary_sha256": hashlib.sha256(new.read_bytes()).hexdigest(),
              "scope": "pre-0129 retained target upgraded in place; local target, not Gateway acceptance"}
    cases = {"upgrade-c5": c5_case}
    if args.engine == "postgres":
        cases["upgrade-c1"] = c1_case
    try:
        h.prepare()
        for name, case in cases.items():
            try:
                result = case(h, old, new)
            except Exception as error:
                result = {"status": "failed", "error": str(error) if isinstance(error, RuntimeError)
                          else type(error).__name__}
            report["cases"][name] = result
            runner.write_json(path / "upgrade.json", report)
            print(json.dumps({"case": name, "status": result["status"]}), flush=True)
    finally:
        try:
            h.target.close()
        except Exception as error:
            report["target_cleanup_error"] = type(error).__name__
        report["cleanup_failures"] = run.cleanup()
        shutil.rmtree(h.store, ignore_errors=True)
        runner.write_json(path / "upgrade.json", report)
        runner.checksums(path)
    return int(bool(report.get("target_cleanup_error") or report["cleanup_failures"])
               or any(r["status"] != "passed" for r in report["cases"].values()))


if __name__ == "__main__":
    raise SystemExit(main())
