"""Product SQL Server load/target comparisons; no external systems or models."""
import copy
import hashlib
import json
import re
import threading

import runner
import t1


def fingerprint(rows):
    # Preserve duplicate rows and NULL vs empty, without imposing host collation.
    values = sorted(json.dumps(r, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
                    for r in rows)
    return {"rows": len(rows), "sha256": hashlib.sha256(("\n".join(values) + "\n").encode()).hexdigest()}


def projections(run, table):
    columns = run.json_query(
        "SELECT c.name,ty.name AS kind FROM sys.columns c JOIN sys.types ty "
        "ON ty.user_type_id=c.user_type_id WHERE c.object_id=OBJECT_ID('dbo."
        + table + "') ORDER BY c.column_id")
    left, right = [], []
    for column in columns:
        name, kind = column["name"], column["kind"]
        s, p = f"[{name}]", f'"{name}"'
        if kind == "datetime2":
            s = (f"DATEDIFF_BIG(day,CONVERT(date,'00010101',112),CONVERT(date,{s}))*864000000000"
                 f"+DATEDIFF_BIG(nanosecond,CONVERT(datetime2(7),CONVERT(date,{s})),{s})/100")
        elif kind == "date":
            s = f"DATEDIFF(day,CONVERT(date,'00010101',112),{s})"
        elif kind in ("binary", "varbinary"):
            s = f"LOWER(CONVERT(varchar(max),{s},2))"
            p = f"encode({p},'hex')"
        left.append(f"CONVERT(nvarchar(max),{s}) AS [{name}]")
        right.append(f"{p}::text AS \"{name}\"")
    # sqlcmd has a bounded input line buffer; do not send a 300-column line.
    return ",\n".join(left), ",".join(right)


def source_rows(run, table, projection, where=""):
    return run.json_query(f"SELECT {projection} FROM dbo.[{table}] {where}")


def target_rows(target, relation, projection, where=""):
    if not re.fullmatch(r"oo_load_data\.t_[a-z0-9_]+", relation):
        raise ValueError("unexpected target relation")
    raw = target.sql(f"SELECT COALESCE(json_agg(q),'[]'::json) FROM (SELECT {projection} FROM {relation} {where}) q;")
    return json.loads(raw)


def execute(run, requested):
    t1.LANE = "e"
    target = t1.AgentDB(run)
    result = {suite: "not_implemented" for suite in requested}
    home = run.private / "home"
    store = home / ".config/open-ontology/secrets"
    store.mkdir(parents=True, mode=0o700)
    for name, value in [("source", run.reader_secret), ("source-ca", (run.private / "source-ca").read_text()),
                        ("target", target.password)]:
        t1.private(store / name, value)
    stack = {"version": "oo.stack/v1", "sources": [{
        "id": "fixture", "engine": "sqlserver", "host": "localhost", "port": run.port,
        "database": "fixture", "user": "fixture_reader", "password": {"secret_ref": "source"},
        "tls_ca": {"secret_ref": "source-ca"},
        "scope": {"include": ["dbo." + t for t in run.tables], "exclude": []}}],
        "model": {"provider": "openai-compatible", "base_url": "https://example.invalid/v1",
                  "model": "fixture", "key": {"secret_ref": "unused-model"}}}
    stack_path = run.private / "stack.json"
    runner.write_json(stack_path, stack)
    stop = threading.Event()
    thread = None
    writer = {"commits": 0, "errors": []}
    resources = {}
    try:
        if "T0" in requested:
            result["T0"] = "failed"
            run.command(["cargo", "build", "-p", "oo-cli", "--locked"], timeout=1200)
            _, raw = run.command(
                ["env", "HOME=" + str(home), str(runner.ROOT / "target/debug/oo"),
                 "doctor", "--config", str(stack_path), "--json"], required=False, timeout=1800)
            doctor = json.loads(raw)
            runner.write_json(run.path / "doctor.json", doctor)
            # This engine card does not call a model. Inspect the source result,
            # not the intentionally blocked aggregate (missing model secret).
            if len(doctor["sources"]) != 1 or doctor["sources"][0]["status"] != "ok":
                raise RuntimeError("product source doctor failed")
            result["T0"] = "passed"
        if not any(s in requested for s in ("T1", "T2", "T8", "T7")):
            return result
        target.start()
        result["T1"] = "failed"
        target_config = {"host": "127.0.0.1", "port": target.port, "database": "postgres",
                         "user": "postgres", "password": {"secret_ref": "target"},
                         "local_fixture": True, "run_id": "snapshot"}
        target_path = run.private / "target.json"
        runner.write_json(target_path, target_config)
        run.command(["cargo", "build", "--locked", "--manifest-path", "tools/oo-load/Cargo.toml"],
                    timeout=1200)
        command = [str(runner.ROOT / "tools/oo-load/target/debug/oo-load"),
                   "--stack", str(stack_path), "--source", "fixture", "--target", str(target_path),
                   "--secrets-dir", str(store), "--chunk-rows", "10000"]
        left, right = projections(run, "growth_events")
        a = fingerprint(source_rows(run, "growth_events", left))
        last = min(run.args.scale, 100)
        # One transaction includes insert/update/delete and primary-key update.
        forward = (f"BEGIN TRANSACTION; UPDATE growth_events SET order_id=999 WHERE id=2; "
                   f"UPDATE growth_events SET id={last+1} WHERE id={last}; "
                   f"DELETE growth_events WHERE id=1; INSERT growth_events VALUES({last+2},777); COMMIT;")
        backward = (f"BEGIN TRANSACTION; UPDATE growth_events SET order_id=2 WHERE id=2; "
                    f"UPDATE growth_events SET id={last} WHERE id={last+1}; "
                    f"DELETE growth_events WHERE id={last+2}; INSERT growth_events VALUES(1,1); COMMIT;")
        if run.args.scale < 4:
            raise ValueError("concurrent T1 requires scale >= 4")
        run.query(forward)
        b = fingerprint(source_rows(run, "growth_events", left))
        run.query(backward)
        writer_run = copy.copy(run)
        writer_run.seq = 1_000_000

        def mutate():
            try:
                while not stop.is_set():
                    writer_run.query(forward if writer["commits"] % 2 == 0 else backward)
                    writer["commits"] += 1
                    stop.wait(.5)
            except Exception as error:
                writer["errors"].append(type(error).__name__)

        thread = threading.Thread(target=mutate)
        thread.start()
        # Separate counter range prevents sampler/writer evidence collisions.
        meter = copy.copy(run)
        meter.seq = 2_000_000
        samples = []
        meter_stop = threading.Event()

        def sample():
            while not meter_stop.is_set():
                try:
                    rows = meter.json_query(
                        "SELECT COUNT(*) AS n FROM sys.dm_exec_sessions WHERE program_name='oo-sqlserver-source'")
                    samples.append(rows[0]["n"])
                except Exception as error:
                    samples.append({"error": type(error).__name__})
                meter_stop.wait(1)

        sampler = threading.Thread(target=sample)
        sampler.start()
        try:
            code, raw = run.command(command, timeout=7200, required=False)
            # Command stderr contains the independent connector timing receipt.
            resource_text = (run.path / f"{run.seq:04d}.stderr").read_text()
        finally:
            stop.set()
            thread.join(timeout=1800)
            meter_stop.set()
            sampler.join(timeout=1800)
        runner.write_json(run.path / "writer.json", writer)
        runner.write_json(run.path / "connection-samples.json", samples)
        if code or writer["errors"] or not writer["commits"] or thread.is_alive():
            raise RuntimeError("concurrent product load failed")
        receipts = [json.loads(line) for line in raw.splitlines()]
        relations = {r["table"]["name"]: r["target"] for r in receipts}
        if set(relations) != set(run.tables):
            raise RuntimeError("missing loaded tables")
        comparisons = {}
        for table in run.tables:
            left, right = projections(run, table)
            chunks = []
            for lo in range(1, run.args.scale + 1, 10000) if table == "events" else [None]:
                where = "" if lo is None else f"WHERE id>={lo} AND id<{lo+10000}"
                actual = fingerprint(target_rows(target, relations[table], right, where))
                gold = ([a, b] if table == "growth_events" else
                        [fingerprint(source_rows(run, table, left, where))])
                chunks.append({"equal": actual in gold, "source": gold, "target": actual})
            comparisons[table] = chunks
            runner.write_json(run.path / "target-comparison.json", comparisons)
            if not all(c["equal"] for c in chunks):
                raise RuntimeError("Agent DB differs: " + table)
        result["T1"] = "passed"
        timing = json.loads(resource_text.strip())["resources"]
        result["T8"] = "failed"
        peak = max((v for v in samples if isinstance(v, int)), default=0)
        resources = {"connection_peak_sampled": peak, "connection_limit": 1,
                     "sampling_seconds": 1, "connector": timing,
                     "target_write_bytes": sum(r["target_write_bytes"] for r in receipts),
                     "model_tokens": 0, "model_basis": "not invoked"}
        if not samples or any(not isinstance(v, int) for v in samples) or peak != 1:
            raise RuntimeError("connection budget observation failed")
        if timing["pacing_ns"] < timing["read_ns"] * 3:
            raise RuntimeError("source pacing budget failed")
        result["T8"] = "passed"
        if "T2" in requested:
            result["T2"] = "failed"
            from sqlserver_cycles import cycles
            cycles(run, target, command, stack, stack_path, target_config, target_path, store)
            result["T2"] = "passed"
        if "T7" in requested:
            result["T7"] = "failed"
            from sqlserver_cycles import readonly_audit
            readonly_audit(run, command, stack_path, target_path)
            result["T7"] = "passed"
        return result
    finally:
        stop.set()
        if thread:
            thread.join(timeout=1800)
        try:
            target.close()
        except Exception:
            run.receipt.setdefault("cleanup_failed", []).append(target.name)
            raise
        finally:
            runner.write_json(run.path / "product-suites.json", result)
            runner.write_json(run.path / "product-resources.json", resources)
