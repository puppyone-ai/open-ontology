#!/usr/bin/env python3
"""D22 live assertions on owned fixtures. Unsupported exits are failures, not passes."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import time

import runner
import t1
import t3t4


def target_rows(h, sid, table):
    relation = h.relations[(sid, table)]
    key = "ID" if h.run.engine == "oracle" else "id"
    _, text = h.tgt(f'SELECT row_to_json(x) FROM (SELECT * FROM {relation} ORDER BY "{key}") x;')
    return [json.loads(line) for line in text.splitlines() if line.startswith("{")]


def pair(h, sid, mode="c5"):
    tables = [sid + "_bad", sid + "_good"]
    oracle = h.run.engine == "oracle"
    for table in tables:
        definition = ("id NUMBER(18) PRIMARY KEY,payload VARCHAR2(80) NOT NULL,n NUMBER(10) NOT NULL"
                      if oracle else "id bigint PRIMARY KEY,payload varchar(80) NOT NULL,n int NOT NULL")
        h.src(f"CREATE TABLE {table}({definition});")
        if oracle:
            h.src(f"GRANT SELECT,FLASHBACK ON {table} TO fixture_reader;")
            if mode == "c1":
                h.src(f"ALTER TABLE {table} ADD SUPPLEMENTAL LOG DATA (ALL) COLUMNS;")
        h.src(f"INSERT INTO {table} VALUES(1,'base',7);")
    if mode == "c5":
        h.c5_binding(sid, tables)
    else:
        h.c1_binding(sid, tables)
        h.c1(sid)
    return tables


def cycle(h, sid, run):
    return h.call(sid, run, "--c5", {"cycle": 2, "full_read_every": 1000}, required=False)


def schema_case(h, kind, tag=None, mode="c5"):
    """Strict projection expectations, shared by T4 and T9 (no rebaseline escape)."""
    sid = tag or ("fhp_" + kind)
    bad, good = pair(h, sid, mode)
    before = target_rows(h, sid, bad)
    columns = h.target_columns(h.relations[(sid, bad)])
    oracle, mysql = h.run.engine == "oracle", h.run.engine == "mysql"
    if kind == "add_column":
        statement = f"ALTER TABLE {bad} ADD extra {'VARCHAR2' if oracle else 'varchar'}(40);"
    elif kind == "widen_string":
        statement = (f"ALTER TABLE {bad} MODIFY (payload VARCHAR2(160));" if oracle else
                     f"ALTER TABLE {bad} MODIFY payload varchar(160) NOT NULL;" if mysql else
                     f"ALTER TABLE {bad} ALTER COLUMN payload TYPE varchar(160);")
    elif kind == "widen_type":
        # NUMBER(10) and NUMBER(18) both map to int64; 19 crosses to numeric.
        statement = (f"ALTER TABLE {bad} MODIFY (n NUMBER(19));" if oracle else
                     f"ALTER TABLE {bad} MODIFY n bigint NOT NULL;" if mysql else
                     f"ALTER TABLE {bad} ALTER COLUMN n TYPE bigint;")
    elif kind == "drop_column":
        statement = f"ALTER TABLE {bad} DROP COLUMN n;"
    elif kind == "rename_column":
        statement = f"ALTER TABLE {bad} RENAME COLUMN payload TO label;"
    else:
        raise ValueError(kind)
    h.src(statement)
    h.src(f"UPDATE {good} SET payload='healthy' WHERE id=1;")
    if mode == "c5":
        code, outputs, error = cycle(h, sid, sid + "-changed")
    else:
        code, batches, error = h.c1(sid, required=False)
        outputs = [receipt for batch in batches for receipt in batch["receipts"]]
    by_table = {r.get("table", {}).get("name", "").lower(): r for r in outputs}
    good_rows = target_rows(h, sid, good)
    good_updated = any("healthy" in row.values() for row in good_rows)
    drift = kind in ("drop_column", "rename_column")
    if drift:
        stale = by_table.get(bad, {})
        passed = (code == 3 and stale.get("status") == "stale"
                  and stale.get("stale_reason", "").startswith("schema_drift:")
                  and target_rows(h, sid, bad) == before and good_updated)
    else:
        passed = code == 0 and good_updated and by_table.get(bad, {}).get("status") in ("fresh", "noop")
        after_columns = h.target_columns(h.relations[(sid, bad)])
        if kind == "add_column":
            passed = passed and after_columns == columns
        if kind == "widen_string":
            passed = passed and by_table.get(bad, {}).get("new_generations") == 0
        if kind == "widen_type":
            passed = passed and by_table.get(bad, {}).get("new_generations") == 1
    return {"status": "passed" if passed else "failed", "path": mode,
            "exit": code, "error": error, "tables": outputs,
            "healthy_table_updated": good_updated, "ddl": kind}


def permission(h):
    sid = "fhp_permission"
    bad, good = pair(h, sid)
    before = target_rows(h, sid, bad)
    engine = h.run.engine
    try:
        if engine == "mysql":
            # MySQL schema grants cannot be overridden by a table-level revoke.
            h.src("REVOKE SELECT ON fixture.* FROM 'fixture_reader'@'%';")
            h.src(f"GRANT SELECT ON fixture.{good} TO 'fixture_reader'@'%';")
        else:
            privileges = "SELECT,FLASHBACK" if engine == "oracle" else "SELECT"
            h.src(f"REVOKE {privileges} ON {bad} FROM fixture_reader;")
        h.src(f"UPDATE {good} SET payload='healthy' WHERE id=1;")
        code, outputs, error = cycle(h, sid, sid + "-denied")
        by_table = {r.get("table", {}).get("name", "").lower(): r for r in outputs}
        stale = by_table.get(bad, {})
        passed = (code == 3 and stale.get("status") == "stale"
                  and stale.get("stale_reason") == "source_permission"
                  and stale.get("attempts") == 2 and target_rows(h, sid, bad) == before
                  and any("healthy" in row.values() for row in target_rows(h, sid, good)))
        replay_code, replayed, _ = cycle(h, sid, sid + "-denied")
        replay_ok = replay_code == 3 and len(replayed) == 2 and all(r.get("replay") for r in replayed)
        if engine == "mysql":
            h.src(f"REVOKE SELECT ON fixture.{good} FROM 'fixture_reader'@'%';")
        else:
            h.src(f"REVOKE {privileges} ON {good} FROM fixture_reader;")
        all_stale_code, all_stale, _ = cycle(h, sid, sid + "-all-denied")
        all_stale_ok = all_stale_code == 4 and len(all_stale) == 2 and all(
            r.get("status") == "stale" for r in all_stale)
    finally:
        if engine == "mysql":
            h.src("GRANT SELECT ON fixture.* TO 'fixture_reader'@'%';")
        else:
            privileges = "SELECT,FLASHBACK" if engine == "oracle" else "SELECT"
            h.src(f"GRANT {privileges} ON {bad} TO fixture_reader;")
            h.src(f"GRANT {privileges} ON {good} TO fixture_reader;")
    recovery, recovered, recovery_error = cycle(h, sid, sid + "-recovered")
    return {"status": "passed" if passed and recovery == 0 and replay_ok and all_stale_ok else "failed",
            "exit": code, "error": error, "tables": outputs,
            "replay_exit": replay_code, "replay_ok": replay_ok,
            "all_stale_exit": all_stale_code, "all_stale_ok": all_stale_ok,
            "recovery_exit": recovery, "recovery_error": recovery_error, "recovery": recovered}


def unproven(h):
    sid = "fhp_unproven"
    pair(h, sid)
    # Baselines predating the formal catalog must not acquire proof by inference.
    _, exists = h.tgt("SELECT to_regclass('_oo_load.sealed_column_catalog') IS NOT NULL;")
    if exists.strip() == "t":
        h.tgt("DELETE FROM _oo_load.sealed_column_catalog;")
    code, outputs, error = cycle(h, sid, sid + "-missing")
    passed = (code == 4 and len(outputs) == 2
              and all(r.get("stale_reason") == "schema_unproven" for r in outputs))
    return {"status": "passed" if passed else "failed", "exit": code, "error": error, "tables": outputs}


def unavailable(h):
    sid = "fhp_unavailable"
    pair(h, sid)
    before = h.state()
    h.run.command(["docker", "stop", h.run.name], timeout=120)
    try:
        code, outputs, error = cycle(h, sid, sid + "-offline")
        passed = code not in (0, 3, 4) and before == h.state()
    finally:
        h.run.command(["docker", "start", h.run.name])
        h.wait_source()
    return {"status": "passed" if passed else "failed", "exit": code, "error": error, "tables": outputs}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--engine", required=True, choices=["postgres", "mysql", "oracle"])
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--binary", type=Path, default=runner.ROOT / "target/debug/oo-load")
    args = parser.parse_args()
    if not args.run_id or any(c not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_" for c in args.run_id):
        parser.error("invalid run-id")
    t1.LANE = "fhp"
    os.environ["OO_TEST_LANE"] = "fhp"
    version = {"postgres": "17", "mysql": "8.4", "oracle": "23"}[args.engine]
    path = runner.ROOT / "evidence" / args.run_id
    path.mkdir(mode=0o700, exist_ok=False)
    run = t3t4.Source(path, args.engine, version,
                     argparse.Namespace(platform=None, health_timeout=1200, scale=1, tables=0, worker_image=None))
    if args.engine == "oracle":
        from oracle_t3t4 import OracleHarness
        h = OracleHarness(run, args.binary.resolve())
    else:
        h = t3t4.Harness(run, args.binary.resolve())
    report = {"engine": args.engine, "version": version, "cases": {},
              "binary_sha256": hashlib.sha256(args.binary.read_bytes()).hexdigest(),
              "scope": "C5 live source isolation; publication separately tested on owned PG target"}
    cases = {
        "T9-01": permission,
        "T9-03": lambda h: schema_case(h, "add_column"),
        "T9-04": lambda h: schema_case(h, "widen_string"),
        "T9-05": lambda h: schema_case(h, "widen_type"),
        "T9-06-drop": lambda h: schema_case(h, "drop_column"),
        "T9-06-rename": lambda h: schema_case(h, "rename_column"),
        "T9-07": unproven,
        "T9-11": unavailable,
    }
    try:
        h.prepare()
        report["cases"]["T9-02"] = {"status": "not_run", "reason": "timeout_injection_not_implemented"}
        for name, case in cases.items():
            started = time.monotonic()
            try:
                result = case(h)
            except Exception as error:
                result = {"status": "failed", "error": str(error) if isinstance(error, RuntimeError)
                          else type(error).__name__}
            result["elapsed_seconds"] = round(time.monotonic() - started, 2)
            report["cases"][name] = result
            runner.write_json(path / "t9.json", report)
            print(json.dumps({"case": name, "status": result["status"]}), flush=True)
    finally:
        try:
            h.target.close()
        except Exception as error:
            report["target_cleanup_error"] = type(error).__name__
        if args.engine == "oracle":
            try:
                h.close_worker()
            except Exception as error:
                report["worker_cleanup_error"] = type(error).__name__
        report["cleanup_failures"] = run.cleanup()
        shutil.rmtree(h.store, ignore_errors=True)
        runner.write_json(path / "t9.json", report)
        runner.checksums(path)
    return int(bool(report.get("target_cleanup_error") or report.get("worker_cleanup_error")
                    or report["cleanup_failures"])
               or any(r["status"] != "passed" for r in report["cases"].values()))


if __name__ == "__main__":
    raise SystemExit(main())
