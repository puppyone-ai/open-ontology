#!/usr/bin/env python3
"""Lane M real PostgreSQL C1/C5 cycles; handoff is a separate pending exit."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import threading
import time
import signal

import runner
from verdicts import incremental, write_rows
from t1 import LaneRun, SourceMeter, configure_pg, private

ROOT = runner.ROOT

@runner.timed("seed")
def seed_scale(run, schema, tables):
    """Negative baseline keys leave every existing positive mutation key free."""
    scale = run.args.scale
    if not 1 <= scale <= 10_000_000:
        raise ValueError("invalid T2 scale")
    if scale == 1:
        return
    for table in tables:
        if run.engine == "postgres":
            run.sql(f"INSERT INTO {schema}.{table} SELECT -i,'bulk' "
                    f"FROM generate_series(1,{scale-1}) i;", admin=True)
        elif run.engine == "mysql":
            for start in range(1, scale, 1000):
                values = ",".join(f"({-i},'bulk')" for i in range(start, min(start+1000, scale)))
                run.sql(f"INSERT INTO {schema}.{table} VALUES {values};", admin=True)
        else:
            raise ValueError("full-scale T2 fixture is PG/MySQL only")
        _, count = run.sql(f"SELECT COUNT(*) FROM {schema}.{table};", admin=True)
        if int(count.strip()) != scale:
            raise RuntimeError("T2 baseline row count mismatch")

def c1_summaries(text):
    """Count writes from the aggregate, while still checking every table result."""
    rows = [json.loads(line) for line in text.splitlines() if line.startswith("{")]
    summaries = []
    for row in rows:
        if row.get("protocol") != "oo.sync/v1" or row.get("mode") != "c1" or "error" in row:
            raise RuntimeError("unrecognized C1 receipt")
        if "table" in row:
            if row.get("status") not in ("fresh", "noop") or row.get("stale_reason"):
                raise RuntimeError("C1 table not fresh")
        else:
            if not all(key in row for key in ("durable_position", "target_write_bytes", "new_generations")):
                raise RuntimeError("incomplete C1 summary")
            summaries.append(row)
    if len(summaries) != 1:
        raise RuntimeError("missing or ambiguous C1 summary")
    return summaries


def lsn(value):
    hi, lo = value.split("/")
    return (int(hi, 16) << 32) | int(lo, 16)


def target_sql(run, statement):
    return run.command(["docker", "exec", "-i", "-u", "postgres", run.name,
                        "psql", "-X", "-v", "ON_ERROR_STOP=1", "-A", "-t",
                        "-U", "postgres", "-d", "agent"], data=statement)[1]


def measure(run):
    return int(target_sql(run, "SELECT COALESCE(sum(pg_total_relation_size(c.oid)),0) "
                          "FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace "
                          "WHERE n.nspname='_oo_load' AND c.relkind='r';").strip())


def run_postgres(run, binary):
    report = {"C1": "failed", "C5": "not_implemented", "handoff": "not_implemented",
              "T2": "not_passed", "cycles": [], "consistency": "B",
              "baseline_rows_per_table": run.args.scale,
              "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest()}
    store = run.path / ".sync-private"
    store.mkdir(mode=0o700)
    thread = None
    try:
        run.setup()
        run.sql("CREATE ROLE fixture_reader LOGIN REPLICATION;"
                "REVOKE CREATE ON SCHEMA public FROM PUBLIC;"
                "REVOKE TEMPORARY ON DATABASE fixture FROM PUBLIC;"
                "CREATE TABLE public.items(id bigint PRIMARY KEY,payload text NOT NULL);"
                "CREATE TABLE public.other(id bigint PRIMARY KEY,payload text NOT NULL);"
                "INSERT INTO public.items VALUES(1,'baseline');"
                "INSERT INTO public.other VALUES(1,'baseline');"
                "GRANT USAGE ON SCHEMA public TO fixture_reader;"
                "GRANT SELECT ON ALL TABLES IN SCHEMA public TO fixture_reader;"
                "GRANT EXECUTE ON FUNCTION pg_control_system() TO fixture_reader;"
                "GRANT EXECUTE ON FUNCTION pg_control_checkpoint() TO fixture_reader;"
                "ALTER SYSTEM SET wal_level='logical';"
                "ALTER SYSTEM SET max_slot_wal_keep_size='128MB';", admin=True)
        seed_scale(run, "public", ["items", "other"])
        configure_pg(run, store)
        run.command(["docker", "restart", run.name])
        for _ in range(90):
            if run.sql("SELECT 1", admin=True, required=False)[0] == 0:
                break
            time.sleep(1)
        else:
            raise RuntimeError("source readiness")
        run.sql("CREATE DATABASE agent;", admin=True)
        run.sql("CREATE PUBLICATION oo_t2_pub FOR TABLE public.items,public.other "
                "WITH(publish_via_partition_root=true);", admin=True)
        _, start = run.sql("SELECT lsn FROM pg_create_logical_replication_slot('oo_t2_slot','pgoutput');",
                           admin=True)
        initial = lsn(start.strip())
        _, port_text = run.command(["docker", "port", run.name, "5432/tcp"])
        port = int(port_text.strip().rsplit(":", 1)[1])
        private(store / "source-password", run.reader_secret)
        private(store / "target-password", run.secret)
        source = {"id": "fixture", "engine": "postgres", "host": "127.0.0.1", "port": port,
                  "database": "fixture", "user": "fixture_reader",
                  "password": {"secret_ref": "source-password"}, "tls_ca": {"secret_ref": "source-ca"},
                  "scope": {"include": ["public.items", "public.other"], "exclude": []}}
        stack = {"version": "oo.stack/v1", "sources": [source],
                 "model": {"provider": "openai-compatible", "base_url": "https://example.invalid/v1",
                           "model": "unused", "key": {"secret_ref": "unused"}, "context_window": "auto"}}
        runner.write_json(store / "stack.json", stack)
        runner.write_json(store / "target.json", {
            "host": "127.0.0.1", "port": port, "database": "agent", "user": "postgres",
            "password": {"secret_ref": "target-password"}, "local_fixture": True, "run_id": "baseline"})
        runner.write_json(store / "sync.json", {
            "initial_position": {"kind": "pg_lsn", "lsn": initial}, "baseline_aligned": True,
            "slot": "oo_t2_slot", "publication": "oo_t2_pub", "required_wal_headroom": 1,
            "max_batches": 100})
        command = [str(binary), "--stack", str(store / "stack.json"), "--source", "fixture",
                   "--target", str(store / "target.json"), "--secrets-dir", str(store)]
        _, loaded = run.command(command, timeout=300)
        relations = {r["table"]["name"]: r["target"] for line in loaded.splitlines()
                     if line.startswith("{") for r in [json.loads(line)]}
        previous = initial
        for cycle in range(3):
            errors = []
            if cycle < 2:
                writer_run = copy.copy(run)
                writer_run.seq = 100_000 * (cycle + 1)

                def write():
                    try:
                        transactions = []
                        for index in range(8):
                            key = 100 + cycle * 100 + index * 2
                            transactions.append(
                                f"BEGIN; INSERT INTO items VALUES({key},'new');"
                                f"UPDATE items SET id={key+1},payload='updated' WHERE id={key};"
                                "DELETE FROM items WHERE id=1;"
                                f"INSERT INTO other VALUES({key},'paired');COMMIT;SELECT pg_sleep(0.05);")
                        writer_run.sql("\n".join(transactions), admin=True)
                    except Exception as error:
                        errors.append(type(error).__name__)
                thread = threading.Thread(target=write)
                thread.start()
            before_bytes = measure(run)
            with SourceMeter(run, cycle, 2) as meter:
                _, concurrent = run.command(command + ["--sync", str(store / "sync.json"),
                    "--run-id", f"c1-{cycle}-concurrent"], timeout=300)
                if thread:
                    thread.join(timeout=60)
                    if thread.is_alive() or errors:
                        raise RuntimeError("writer failed")
                    thread = None
                _, drained = run.command(command + ["--sync", str(store / "sync.json"),
                    "--run-id", f"c1-{cycle}-drained"], timeout=300)
            rows = c1_summaries(concurrent) + c1_summaries(drained)
            for row in rows:
                position = row["durable_position"]["lsn"]
                if position < previous:
                    raise RuntimeError("position regressed")
                previous = position
            comparisons = {}
            for table, relation in relations.items():
                _, expected = run.sql(f"SELECT id,payload FROM public.{table} ORDER BY id;")
                actual = target_sql(run, f"SELECT id,payload FROM {relation} ORDER BY id;")
                comparisons[table] = {"equal": expected == actual,
                                      "source_sha256": hashlib.sha256(expected.encode()).hexdigest(),
                                      "target_sha256": hashlib.sha256(actual.encode()).hexdigest()}
                if expected != actual:
                    raise RuntimeError("source target diff")
            after_bytes = measure(run)
            result = {"cycle": cycle + 1, "sync": rows, "comparison": comparisons,
                      "T8": meter.result(), "target_relation_bytes_before": before_bytes,
                      "target_relation_bytes_after": after_bytes}
            report["cycles"].append(result)
            runner.write_json(run.path / "t2.json", report)
            if cycle == 2 and (before_bytes != after_bytes or any(
                    r["target_write_bytes"] or r["new_generations"] for r in rows)):
                raise RuntimeError("no-change target rewrite")
            if result["T8"]["status"] != "passed":
                raise RuntimeError("T8 failed")
        report["C1"] = "passed"
        # Separate binding: this is C5 reconciliation, not an unproven C1->C5
        # or C5->C1 switch. Both targets remain queryable in the same Agent DB.
        source["id"] = "fixture_c5"
        runner.write_json(store / "stack.json", stack)
        target_config = json.loads((store / "target.json").read_text())
        target_config["run_id"] = "c5-base"
        runner.write_json(store / "target.json", target_config)
        command[command.index("--source")+1] = "fixture_c5"
        _, loaded = run.command(command, timeout=300)
        relations = {r["table"]["name"]: r["target"] for line in loaded.splitlines()
                     if line.startswith("{") for r in [json.loads(line)]}
        report["c5_cycles"] = []
        for cycle in range(3):
            errors = []
            if cycle < 2:
                writer_run = copy.copy(run)
                writer_run.seq = 1_000_000 + cycle * 100_000
                def write_c5():
                    try:
                        transactions = []
                        for index in range(8):
                            key = 1000 + cycle * 100 + index
                            transactions.append(
                                f"BEGIN; INSERT INTO items VALUES({key},'c5');"
                                f"UPDATE other SET payload='c5-{cycle}' WHERE id=1;"
                                f"DELETE FROM items WHERE id={101+cycle*100};COMMIT;SELECT pg_sleep(0.05);")
                        writer_run.sql("\n".join(transactions), admin=True)
                    except Exception as error:
                        errors.append(type(error).__name__)
                thread = threading.Thread(target=write_c5)
                thread.start()
            runner.write_json(store / "c5.json", {"cycle": cycle+1, "full_read_every": 10})
            target_config["run_id"] = f"c5-{cycle}-concurrent"
            runner.write_json(store / "target.json", target_config)
            before_bytes = measure(run)
            with SourceMeter(run, cycle+10, 2) as meter:
                code, concurrent = run.command(command + ["--c5", str(store / "c5.json")],
                                               timeout=300, required=False)
                attempt_error = None
                if code:
                    raw = (run.path / f"{run.seq:04d}.stderr").read_text()
                    try:
                        attempt_error = json.loads(raw)["error"]
                    except (ValueError, KeyError):
                        raise RuntimeError("unrecognized C5 failure")
                    if attempt_error != "c5_range_changed_retry":
                        raise RuntimeError("C5 failed")
                if thread:
                    thread.join(timeout=60)
                    if thread.is_alive() or errors:
                        raise RuntimeError("C5 writer failed")
                    thread = None
                target_config["run_id"] = f"c5-{cycle}-final"
                runner.write_json(store / "target.json", target_config)
                _, drained = run.command(command + ["--c5", str(store / "c5.json")], timeout=300)
            receipts = [json.loads(line) for text in (concurrent, drained) for line in text.splitlines()
                        if line.startswith("{")]
            comparisons = {}
            for table, relation in relations.items():
                _, expected = run.sql(f"SELECT id,payload FROM public.{table} ORDER BY id;")
                actual = target_sql(run, f"SELECT id,payload FROM {relation} ORDER BY id;")
                comparisons[table] = {"equal": expected == actual,
                                      "source_sha256": hashlib.sha256(expected.encode()).hexdigest(),
                                      "target_sha256": hashlib.sha256(actual.encode()).hexdigest()}
                if expected != actual:
                    raise RuntimeError("C5 source target diff")
            after_bytes = measure(run)
            result = {"cycle": cycle+1, "attempt_error": attempt_error, "sync": receipts,
                      "comparison": comparisons, "T8": meter.result(),
                      "target_relation_bytes_before": before_bytes, "target_relation_bytes_after": after_bytes}
            report["c5_cycles"].append(result)
            runner.write_json(run.path / "t2.json", report)
            if cycle == 2 and (before_bytes != after_bytes or any(
                    r["payload_bytes"] or r["target_write_bytes"] or r["new_generations"] for r in receipts)):
                raise RuntimeError("C5 no-change rewrote target")
            if result["T8"]["status"] != "passed":
                raise RuntimeError("C5 T8 failed")
        report["C5"] = "passed"
        report["negative_checks"] = []
        before_state = target_sql(run, "SELECT source,position,generation,write_bytes "
                                  "FROM _oo_load.sync_state ORDER BY source;")
        run.sql("SELECT pg_drop_replication_slot('oo_t2_slot');"
                "SELECT lsn FROM pg_create_logical_replication_slot('oo_t2_slot','pgoutput');",
                admin=True)
        source["id"] = "fixture"
        command[command.index("--source")+1] = "fixture"
        runner.write_json(store / "stack.json", stack)
        code, _ = run.command(command + ["--sync", str(store / "sync.json"), "--run-id", "expired-slot"],
                              timeout=300, required=False)
        error = json.loads((run.path / f"{run.seq:04d}.stderr").read_text())["error"]
        if code == 0 or error != "sync_rebaseline_required_retention":
            raise RuntimeError("expired slot did not require rebaseline")
        after_state = target_sql(run, "SELECT source,position,generation,write_bytes "
                                 "FROM _oo_load.sync_state ORDER BY source;")
        if before_state != after_state:
            raise RuntimeError("expired slot changed target")
        report["negative_checks"].append({"case": "expired_slot", "error": error,
                                          "target_state_unchanged": True})
        run.sql("ALTER TABLE items RENAME TO prior_items;"
                "CREATE TABLE items(LIKE prior_items INCLUDING ALL);"
                "INSERT INTO items SELECT * FROM prior_items;"
                "GRANT SELECT ON items TO fixture_reader;", admin=True)
        source["id"] = "fixture_c5"
        command[command.index("--source")+1] = "fixture_c5"
        runner.write_json(store / "stack.json", stack)
        code, negative = run.command(command + ["--c5", str(store / "c5.json"), "--run-id", "recreated-relation"],
                              timeout=300, required=False)
        error = json.loads((run.path / f"{run.seq:04d}.stderr").read_text())["error"]
        isolated = [json.loads(line) for line in negative.splitlines() if line.startswith("{")]
        rejected = any(r.get("table") == {"schema": "public", "name": "items"}
                       and r.get("status") == "stale" and r.get("stale_reason") == "rebaseline_required"
                       for r in isolated)
        if code == 0 or not (error == "c5_rebaseline_required_lineage"
                             or (error == "sync_partial" and rejected)):
            raise RuntimeError("changed relation identity did not require rebaseline")
        report["negative_checks"].append({"case": "recreated_relation", "error": error})
        report["T2"] = "passed"
    finally:
        if thread:
            thread.join(timeout=60)
        runner.write_json(run.path / "t2.json", report)
        # Only generated files; preserve raw evidence and failure reports.
        import shutil
        shutil.rmtree(store)
    return report


def main():
    def interrupted(signum, frame):
        raise KeyboardInterrupt(f"signal {signum}")
    signal.signal(signal.SIGINT, interrupted)
    signal.signal(signal.SIGTERM, interrupted)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--versions", default="13,14,15,16,17,18")
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/oo-load")
    parser.add_argument("--scale", type=int, default=1)
    args = parser.parse_args()
    if not 1 <= args.scale <= 10_000_000:
        parser.error("scale must be 1..10000000")
    if not args.run_id or not all(c.isalnum() or c in "-_" for c in args.run_id):
        parser.error("invalid run ID")
    versions = args.versions.split(",")
    if len(set(versions)) != len(versions) or any(v not in ("13", "14", "15", "16", "17", "18") for v in versions):
        parser.error("versions must be unique PostgreSQL majors 13..18")
    root = ROOT / "evidence" / args.run_id
    root.mkdir(mode=0o700, exist_ok=False)
    results = {}
    for version in versions:
        path = root / ("postgres-" + version)
        path.mkdir()
        run = LaneRun(path, "postgres", version,
                      argparse.Namespace(platform=None, health_timeout=300, scale=args.scale, tables=0))
        results[version] = {"T2": "not_passed", "C1": "not_run", "C5": "not_run"}
        try:
            results[version] = run_postgres(run, args.binary.resolve())
        except Exception as error:
            partial = path / "t2.json"
            results[version] = json.loads(partial.read_text()) if partial.exists() else {
                "C1": "failed", "C5": "not_implemented"}
            results[version].update(error=type(error).__name__, T2="not_passed")
        finally:
            try:
                run.record_version()
            except Exception:
                results[version]["version_error"] = "version_unmeasured"
            results[version]["cleanup_failures"] = run.cleanup()
            runner.write_json(path / "t2.json", results[version])
            write_rows(run, path / "t2.json", incremental(results[version]),
                       cleanup_failed=bool(results[version]["cleanup_failures"]))
            runner.checksums(path)
            runner.write_json(root / "matrix.json", results)
    runner.checksums(root)
    print(json.dumps({"run_id": args.run_id, "results": {
        v: {key: r.get(key) for key in ("C1", "C5", "T2", "handoff", "cleanup_failures")}
        for v, r in results.items()}}))
    if any(r["T2"] != "passed" or r.get("version_error") or r["cleanup_failures"] for r in results.values()):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
