#!/usr/bin/env python3
"""D22 live assertions on owned fixtures. Unsupported exits are failures, not passes."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import time
import threading
from concurrent.futures import ThreadPoolExecutor

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


def cycle(h, sid, run, mode="c5"):
    if mode == "c1":
        code, receipts, error = h.call(sid, run, "--sync", h.sync[sid], required=False)
        return code, [r for r in receipts if "table" in r], error
    return h.call(sid, run, "--c5", {"cycle": 2, "full_read_every": 1000}, required=False)

def generations(h, sid, table):
    prefix = h.relations[(sid, table)].rsplit("t_", 1)[1]
    _, text = h.tgt(f"SELECT count(*) FROM _oo_load.publications WHERE left(identity,40)='{prefix}';")
    return int(text.strip())


def schema_case(h, kind, tag=None, mode="c5"):
    """Strict projection expectations, shared by T4 and T9 (no rebaseline escape)."""
    sid = tag or ("fhp_" + kind)
    bad, good = pair(h, sid, mode)
    before = target_rows(h, sid, bad)
    before_generations = generations(h, sid, bad)
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
    elif kind == "nullability":
        statement = (f"ALTER TABLE {bad} MODIFY (payload NULL);" if oracle else
                     f"ALTER TABLE {bad} MODIFY payload varchar(80) NULL;" if mysql else
                     f"ALTER TABLE {bad} ALTER COLUMN payload DROP NOT NULL;")
    elif kind == "unpublished_only":
        h.src(f"ALTER TABLE {bad} ADD extra {'VARCHAR2' if oracle else 'varchar'}(40);")
        first_code, _, _ = cycle(h, sid, sid + "-added", mode)
        if first_code != 0:
            raise RuntimeError("unpublished_column_add_failed")
        statement = f"ALTER TABLE {bad} DROP COLUMN extra;"
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
    # A bounded C1 drain can emit several cycle receipts. Check the total
    # and independently count physical generations, not just the last no-op.
    new_generations = sum(r.get("new_generations", 0) for r in outputs
                          if r.get("table", {}).get("name", "").lower() == bad)
    physical_generations = generations(h, sid, bad) - before_generations
    good_rows = target_rows(h, sid, good)
    good_updated = any("healthy" in row.values() for row in good_rows)
    drift = kind in ("drop_column", "rename_column", "nullability")
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
        if kind == "unpublished_only":
            # A C1 drain ends with a no-op cycle; the cycle that observed the
            # drop must be the one classified unpublished_only.
            classes = [r.get("schema_class") for r in outputs
                       if r.get("table", {}).get("name", "").lower() == bad]
            passed = (passed and after_columns == columns
                      and classes and "unpublished_only" in classes
                      and all(r.get("status") != "stale" for r in outputs if "table" in r))
        if kind == "widen_string":
            passed = passed and new_generations == 0 and physical_generations == 0
        if kind == "widen_type":
            passed = passed and new_generations == 1 and physical_generations == 1
    return {"status": "passed" if passed else "failed", "path": mode,
            "exit": code, "error": error, "tables": outputs,
            "healthy_table_updated": good_updated, "ddl": kind,
            "new_generations": new_generations, "physical_new_generations": physical_generations}


def permission(h, mode="c5"):
    sid = "fhp_permission_" + mode
    bad, good = pair(h, sid, mode)
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
        code, outputs, error = cycle(h, sid, sid + "-denied", mode)
        by_table = {r.get("table", {}).get("name", "").lower(): r for r in outputs}
        stale = by_table.get(bad, {})
        passed = (code == 3 and stale.get("status") == "stale"
                  and stale.get("stale_reason") == "source_permission"
                  and stale.get("attempts") == 2 and target_rows(h, sid, bad) == before
                  and any("healthy" in row.values() for row in target_rows(h, sid, good)))
        replay_code, replayed, _ = cycle(h, sid, sid + "-denied", mode)
        replay_ok = replay_code == 3 and len(replayed) == 2 and all(r.get("replay") for r in replayed)
        if engine == "mysql":
            h.src(f"REVOKE SELECT ON fixture.{good} FROM 'fixture_reader'@'%';")
        else:
            h.src(f"REVOKE {privileges} ON {good} FROM fixture_reader;")
        all_stale_code, all_stale, _ = cycle(h, sid, sid + "-all-denied", mode)
        all_stale_ok = all_stale_code == 4 and len(all_stale) == 2 and all(
            r.get("status") == "stale" for r in all_stale)
    finally:
        if engine == "mysql":
            h.src("GRANT SELECT ON fixture.* TO 'fixture_reader'@'%';")
        else:
            privileges = "SELECT,FLASHBACK" if engine == "oracle" else "SELECT"
            h.src(f"GRANT {privileges} ON {bad} TO fixture_reader;")
            h.src(f"GRANT {privileges} ON {good} TO fixture_reader;")
    recovery, recovered, recovery_error = cycle(h, sid, sid + "-recovered", mode)
    return {"status": "passed" if passed and recovery == 0 and replay_ok and all_stale_ok else "failed",
            "exit": code, "error": error, "tables": outputs,
            "replay_exit": replay_code, "replay_ok": replay_ok,
            "all_stale_exit": all_stale_code, "all_stale_ok": all_stale_ok,
            "recovery_exit": recovery, "recovery_error": recovery_error, "recovery": recovered}


def unproven(h, mode="c5"):
    sid = "fhp_unproven_" + mode
    pair(h, sid, mode)
    # Baselines predating the formal catalog must not acquire proof by inference.
    _, exists = h.tgt("SELECT to_regclass('_oo_load.column_catalog') IS NOT NULL;")
    if exists.strip() == "t":
        h.tgt("DELETE FROM _oo_load.column_catalog;")
    code, outputs, error = cycle(h, sid, sid + "-missing", mode)
    passed = (code == 4 and len(outputs) == 2
              and all(r.get("stale_reason") == "schema_unproven" for r in outputs))
    return {"status": "passed" if passed else "failed", "exit": code, "error": error, "tables": outputs}

def timeout(h, mode="c5"):
    sid = "fhp_timeout_" + mode
    bad, good = pair(h, sid, mode)
    before = target_rows(h, sid, bad)
    h.src(f"UPDATE {good} SET payload='healthy' WHERE id=1;")
    original_argv = h.argv
    h.argv = lambda *a, **kw: original_argv(*a, **kw) + ["--statement-timeout-ms", "500"]
    pool = ThreadPoolExecutor(max_workers=1)
    holder = None
    pid = None
    oracle = h.run.engine == "oracle"
    try:
        if oracle:
            # Fixture-owner VPD callback delays parsing only this table. The
            # product reader still issues ordinary SELECTs with its own budget.
            h.root_sql(f"""
ALTER SESSION SET CONTAINER={h.service};
CREATE OR REPLACE FUNCTION SYS.FHP_TIMEOUT_POLICY(s VARCHAR2,t VARCHAR2)
RETURN VARCHAR2 AUTHID DEFINER AS BEGIN DBMS_SESSION.SLEEP(3); RETURN '1=1'; END;
/
BEGIN DBMS_RLS.ADD_POLICY(object_schema=>'FIXTURE',object_name=>'{bad.upper()}',
policy_name=>'FHP_TIMEOUT',function_schema=>'SYS',policy_function=>'FHP_TIMEOUT_POLICY',
statement_types=>'SELECT'); END;
/
""")
        else:
            lock = (f"BEGIN; LOCK TABLE {bad} IN ACCESS EXCLUSIVE MODE; SELECT pg_sleep(90) /*fhp_timeout_holder*/; ROLLBACK;"
                    if h.run.engine == "postgres" else
                    f"LOCK TABLES {bad} WRITE; SELECT SLEEP(90) /*fhp_timeout_holder*/; UNLOCK TABLES;")
            holder = pool.submit(h.src, lock, False)
            for _ in range(100):
                query = ("SELECT pid FROM pg_stat_activity WHERE pid<>pg_backend_pid() AND query LIKE '%fhp_timeout_holder%' AND wait_event='PgSleep';"
                         if h.run.engine == "postgres" else
                         "SELECT ID FROM information_schema.PROCESSLIST WHERE ID<>CONNECTION_ID() AND INFO LIKE '%fhp_timeout_holder%';")
                _, text = h.src(query)
                ids = [line.strip() for line in text.splitlines() if line.strip().isdigit()]
                if ids:
                    pid = int(ids[0])
                    break
                time.sleep(.1)
            if pid is None:
                raise RuntimeError("timeout_lock_not_acquired")
        code, outputs, error = cycle(h, sid, sid + "-blocked", mode)
        by_table = {r.get("table", {}).get("name", "").lower(): r for r in outputs}
        passed = (code == 3 and by_table.get(bad, {}).get("stale_reason") == "source_timeout"
                  and target_rows(h, sid, bad) == before
                  and any("healthy" in row.values() for row in target_rows(h, sid, good)))
    finally:
        h.argv = original_argv
        if oracle:
            h.root_sql(f"""
ALTER SESSION SET CONTAINER={h.service};
BEGIN DBMS_RLS.DROP_POLICY(object_schema=>'FIXTURE',object_name=>'{bad.upper()}',policy_name=>'FHP_TIMEOUT'); END;
/
DROP FUNCTION SYS.FHP_TIMEOUT_POLICY;
""")
        elif pid is not None:
            h.src(f"SELECT pg_terminate_backend({pid});" if h.run.engine == "postgres" else f"KILL CONNECTION {pid};")
        if holder is not None:
            holder.result(timeout=100)
        pool.shutdown()
    recovery, recovered, recovery_error = cycle(h, sid, sid + "-recovered", mode)
    return {"status": "passed" if passed and recovery == 0 else "failed",
            "exit": code, "error": error, "tables": outputs, "recovery_exit": recovery,
            "recovery_error": recovery_error, "recovery": recovered}


def unavailable(h, mode="c5"):
    sid = "fhp_unavailable_" + mode
    pair(h, sid, mode)
    before = h.state()
    h.run.command(["docker", "stop", h.run.name], timeout=120)
    try:
        code, outputs, error = cycle(h, sid, sid + "-offline", mode)
        passed = code not in (0, 3, 4) and before == h.state()
    finally:
        h.run.command(["docker", "start", h.run.name])
        h.wait_source()
    return {"status": "passed" if passed else "failed", "exit": code, "error": error, "tables": outputs}

def position(h, sid, table):
    prefix = h.relations[(sid, table)].rsplit("t_", 1)[1]
    _, text = h.tgt("SELECT jsonb_build_object('position',position,'reason',stale_reason,"
                   "'since',stale_since)::text FROM _oo_load.sync_table_positions "
                   f"WHERE left(identity,40)='{prefix}';")
    return json.loads(text)

BULK_ROWS = 20000


def concurrent_rejoin(h):
    """T1: writer overlaps the full-table repair, then exact source/target equality."""
    sid = "fhp_c1_writer"
    bad, good = pair(h, sid, "c1")
    frozen = position(h, sid, bad)
    before = target_rows(h, sid, bad)
    engine = h.run.engine
    privileges = "SELECT,FLASHBACK" if engine == "oracle" else "SELECT"
    def access(restore):
        if engine == "mysql":
            if restore:
                h.src("GRANT SELECT ON fixture.* TO 'fixture_reader'@'%';")
            else:
                h.src("REVOKE SELECT ON fixture.* FROM 'fixture_reader'@'%';")
                h.src(f"GRANT SELECT ON fixture.{good} TO 'fixture_reader'@'%';")
        else:
            h.src(f"{'GRANT' if restore else 'REVOKE'} {privileges} ON {bad} "
                  f"{'TO' if restore else 'FROM'} fixture_reader;")
    stop, started = threading.Event(), threading.Event()
    pool = ThreadPoolExecutor(max_workers=1)
    writer = None
    try:
        access(False)
        for table in (bad, good):
            h.src(f"INSERT INTO {table} VALUES(2,'during-quarantine',2);"
                  f"UPDATE {table} SET payload='before-rejoin' WHERE id=1;")
        # Enough quarantined rows that the repair snapshot spans real time,
        # so the writer commits inside its correction window (log_events>0).
        last = 100000 + BULK_ROWS - 1
        h.src(f"INSERT INTO {bad} SELECT 100000+LEVEL-1,'bulk',LEVEL FROM dual "
              f"CONNECT BY LEVEL<={BULK_ROWS};" if engine == "oracle" else
              f"SET SESSION cte_max_recursion_depth={BULK_ROWS + 1};"
              f"INSERT INTO {bad} WITH RECURSIVE s(g) AS (SELECT 100000 UNION ALL "
              f"SELECT g+1 FROM s WHERE g<{last}) SELECT g,'bulk',g FROM s;" if engine == "mysql" else
              f"INSERT INTO {bad} SELECT g,'bulk',g FROM generate_series(100000,{last}) g;")
        code, outputs, error = cycle(h, sid, sid + "-quarantine", "c1")
        held = position(h, sid, bad)
        quarantined = (code == 3 and held["position"] == frozen["position"]
                       and held["reason"] == "source_permission"
                       and target_rows(h, sid, bad) == before
                       and h.position(sid) != frozen["position"])
        access(True)
        def write():
            count = 0
            while not stop.is_set() and count < 2000:
                key = 1000 + count // 4
                for table in (bad, good):
                    statement = [
                        f"INSERT INTO {table} VALUES({key},'writer',{key});",
                        f"UPDATE {table} SET id={key + 10000} WHERE id={key};",
                        f"UPDATE {table} SET payload='writer-{count}' WHERE id={key + 10000};",
                        f"DELETE FROM {table} WHERE id={key + 10000};",
                    ][count % 4]
                    h.src(statement)
                count += 1
                started.set()
                time.sleep(.1)
            return count
        writer = pool.submit(write)
        if not started.wait(30):
            raise RuntimeError("writer_did_not_start")
        recovery, recovered, recovery_error = cycle(h, sid, sid + "-repair", "c1")
    finally:
        stop.set()
        written = writer.result(timeout=120) if writer is not None else 0
        pool.shutdown()
        access(True)
    drained, _, drain_error = h.c1(sid, required=False)
    comparison = h.compare(sid)
    proof = next((r.get("alignment") for r in recovered
                  if r.get("table", {}).get("name", "").lower() == bad), None)
    after = position(h, sid, bad)
    passed = (quarantined and recovery == 0 and drained == 0 and h.equal(comparison)
              and written > 0 and proof and proof["log_events"] > 0
              and proof["atomic_rejoin"] and after["reason"] is None and after["since"] is None)
    return {"status": "passed" if passed else "failed", "exit": code, "error": error,
            "frozen": frozen, "quarantined": held, "after": after, "tables": outputs,
            "recovery_exit": recovery, "recovery_error": recovery_error, "recovery": recovered,
            "writer_iterations": written, "alignment": proof, "drain_exit": drained,
            "drain_error": drain_error, "comparison": comparison}


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
              "scope": "C5 and C1 live source isolation; local target, not Gateway acceptance"}
    cases = {
        "T9-01": permission,
        "T9-02": timeout,
        "T9-03": lambda h: schema_case(h, "add_column"),
        "T9-03-unpublished": lambda h: schema_case(h, "unpublished_only"),
        "T9-04": lambda h: schema_case(h, "widen_string"),
        "T9-05": lambda h: schema_case(h, "widen_type"),
        "T9-06-drop": lambda h: schema_case(h, "drop_column"),
        "T9-06-rename": lambda h: schema_case(h, "rename_column"),
        "T9-06-nullability": lambda h: schema_case(h, "nullability"),
        "T9-07": unproven,
        "T9-11": unavailable,
    }
    cases.update({
        "C1-T9-01": lambda h: permission(h, "c1"),
        "C1-T9-02": lambda h: timeout(h, "c1"),
        **{"C1-" + name: (lambda h, kind=kind: schema_case(h, kind, "fhp_c1_" + kind, "c1"))
           for name, kind in [
               ("T9-03", "add_column"), ("T9-03-unpublished", "unpublished_only"),
               ("T9-04", "widen_string"), ("T9-05", "widen_type"),
               ("T9-06-drop", "drop_column"), ("T9-06-rename", "rename_column"),
               ("T9-06-nullability", "nullability")]},
        "C1-T9-07": lambda h: unproven(h, "c1"),
        "C1-T9-11": lambda h: unavailable(h, "c1"),
        "C1-T1-quarantine-rejoin": concurrent_rejoin,
    })
    try:
        h.prepare()
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
