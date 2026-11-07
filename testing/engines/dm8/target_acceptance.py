#!/usr/bin/env python3
"""DM8 T1/T8 (and C5) through the real oo-load path, compared with Agent DB.

T1: a separate admin writer inserts, updates, deletes and moves primary keys
while the snapshot load runs. After the writer stops and one late commit
lands, exactly one C5 correction cycle runs on that load (no quiesced full
reload) and must publish; then every source row is compared with Agent DB.
With --c5 three more cycles follow: idle, changed, idle. T8: one reader session, read duty <= 25%.
"""
import argparse
from datetime import datetime, timezone
from decimal import Decimal
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import subprocess
import sys
import threading
import time
from types import SimpleNamespace

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from admin import LANE, ROOT, Admin, private, run_dir  # noqa: E402

UTC = timezone.utc
EVENTS = [("ID", "int"), ("VALUE", "text"), ("AMOUNT", "dec"), ("INSTANT", "ts"),
          ("ZONED", "tsz")]
TYPES = [("ID", "int"), ("I", "int"), ("S", "int"), ("T", "int"), ("B", "int"),
         ("BT", "bool"), ("D", "dec"), ("N", "dec"), ("NN", "dec"), ("F", "float"),
         ("DB", "float"), ("R", "real"), ("DT", "date"), ("TS", "ts"), ("TS9", "ts9"),
         ("TSZ", "tsz"), ("TSL", "tsl"), ("TM", "cast"), ("V", "text"), ("V2", "text"),
         ("C", "text"), ("CL", "text"), ("TX", "text"), ("BL", "bin"), ("BI", "bin"),
         ("VB", "bin"), ("IV", "cast")]
# Agent DB column types the mapping must produce (D17: zoned as timestamptz).
TYPES_TARGET = {
    "ID": "bigint", "I": "integer", "S": "smallint", "T": "smallint", "B": "smallint",
    "BT": "boolean", "D": "numeric(31,8)", "N": "numeric(10,2)", "NN": "text",
    "F": "double precision", "DB": "double precision", "R": "double precision",
    "DT": "date", "TS": "timestamp without time zone", "TS9": "text",
    "TSZ": "timestamp with time zone", "TSL": "timestamp with time zone", "TM": "text",
    "V": "text", "V2": "text", "C": "text", "CL": "text", "TX": "text", "BL": "bytea",
    "BI": "bytea", "VB": "bytea", "IV": "text",
}


def columns(table):
    if table == "EVENTS":
        return EVENTS
    if table == "TYPES":
        return TYPES
    if table == "COMPOSITE":
        return [("K", "text"), ("ID", "int"), ("VALUE", "text")]
    if table == "DEC_KEY":
        return [("ID", "dec"), ("VALUE", "text")]
    if table == "NO_KEY":
        return [("VALUE", "text")]
    if table == "WIDE_ROWS":
        return [("ID", "int")] + [(f"C{i:03}", "text") for i in range(1, 300)]
    return [("ID", "int"), ("VALUE", "text")]


def source_expression(name, kind):
    """Independent disql rendering; zone conversion happens in Python."""
    c = '"' + name + '"'
    hexed = lambda e: f"'x'||NVL(RAWTOHEX(UTL_RAW.CAST_TO_RAW({e})),'')"
    render = {
        "int": f"CAST({c} AS VARCHAR(40))", "bool": f"CAST({c} AS VARCHAR(1))",
        "dec": f"CAST({c} AS VARCHAR(100))", "float": f"CAST({c} AS VARCHAR(100))",
        "real": f"CAST(CAST({c} AS DOUBLE) AS VARCHAR(100))",
        "date": f"TO_CHAR({c},'YYYY-MM-DD')",
        "ts": f"TO_CHAR({c},'YYYY-MM-DD HH24:MI:SS.FF6')",
        "tsl": f"TO_CHAR({c},'YYYY-MM-DD HH24:MI:SS.FF6')",
        "tsz": f"TO_CHAR({c},'YYYY-MM-DD HH24:MI:SS.FF6 TZH:TZM')",
        "text": hexed(c), "bin": f"'x'||NVL(RAWTOHEX({c}),'')",
        "ts9": hexed(f"TO_CHAR({c},'YYYY-MM-DD HH24:MI:SS.FF9')"),
        "cast": hexed(f"CAST({c} AS VARCHAR(256))"),
    }[kind]
    return f"CASE WHEN {c} IS NULL THEN '~' ELSE {render} END"


def decimal_text(value):
    value = Decimal(value)
    return "0" if value == 0 else format(value.normalize(), "f")


def canonical_source(kind, text):
    if text == "~":
        return None
    if kind == "int":
        return str(int(text))
    if kind == "bool":
        return {"1": "true", "0": "false"}[text]
    if kind == "dec":
        return decimal_text(text)
    if kind in ("float", "real"):
        return repr(float(text))
    if kind == "date":
        return text
    if kind == "ts":
        return datetime.fromisoformat(text).isoformat()
    if kind == "tsl":  # session time zone is asserted UTC before comparing
        return datetime.fromisoformat(text).replace(tzinfo=UTC).isoformat()
    if kind == "tsz":
        return datetime.strptime(text, "%Y-%m-%d %H:%M:%S.%f %z").astimezone(UTC).isoformat()
    if kind == "bin":
        return text[1:].lower()
    return bytes.fromhex(text[1:]).decode("utf-8")


def canonical_target(kind, value):
    if value is None:
        return None
    if kind == "int":
        return str(int(value))
    if kind == "bool":
        return "true" if value is True else "false" if value is False else "invalid"
    if kind == "dec":
        return decimal_text(str(value))
    if kind in ("float", "real"):
        return repr(float(value))
    if kind == "date":
        return value
    if kind == "ts":
        stamp = datetime.fromisoformat(value)
        return stamp.isoformat() if stamp.tzinfo is None else "zoned:" + value
    if kind in ("tsz", "tsl"):
        stamp = datetime.fromisoformat(value)
        return stamp.astimezone(UTC).isoformat() if stamp.tzinfo else "naive:" + value
    if kind == "bin":
        return value[2:].lower() if value.startswith("\\x") else "invalid"
    return value


# disql refuses statements over its input buffer (DISQL-10033), whatever the
# line breaks; wider tables are read in column groups keyed by the first
# (primary key) column and joined back in Python.
SELECT_LIMIT = 12000


def source_selects(table, kinds):
    rendered = [source_expression(n, k) for n, k in kinds]
    select = lambda parts: "SELECT 'R|'||" + "||'|'||".join(parts) + f" FROM FIXTURE.{table}"
    if len(select(rendered)) <= SELECT_LIMIT:
        return [select(rendered)]
    if kinds[0][0] != "ID":
        raise ValueError("wide_table_needs_key")
    groups, current = [], []
    for part in rendered[1:]:
        if current and len(select([rendered[0]] + current + [part])) > SELECT_LIMIT:
            groups.append(current)
            current = []
        current.append(part)
    groups.append(current)
    return [select([rendered[0]] + group) for group in groups]


def merge_groups(results):
    """Join column-group rows on their leading unique key, refusing any gap."""
    if len(results) == 1:
        return results[0]
    merged = {}
    for index, rows in enumerate(results):
        keyed = {}
        for row in rows:
            key, _, rest = row.partition("|")
            if key in keyed:
                raise RuntimeError("wide_key_not_unique")
            keyed[key] = rest
        if index == 0:
            merged = {key: [key, rest] for key, rest in keyed.items()}
        elif set(keyed) != set(merged):
            raise RuntimeError("wide_groups_disagree")
        else:
            for key, rest in keyed.items():
                merged[key].append(rest)
    return ["|".join(parts) for parts in merged.values()]


def validate_cycle(receipts, tables, cycle):
    """1 correction (must publish), 2/4 idle (zero writes, generations, payload, source reads), 3 changed."""
    if (len(receipts) != len(tables)
            or {r["table"]["name"] for r in receipts} != set(tables)
            or any(r.get("mode") != "c5" or r.get("protocol") != "oo.sync/v1"
                   or r.get("replay") is not False for r in receipts)):
        raise ValueError("c5_missing_receipts")
    writes = sum(r["target_write_bytes"] for r in receipts)
    generations = sum(r["new_generations"] for r in receipts)
    if cycle in (2, 4) and (writes or generations
                            or any(r["payload_bytes"] or r["source_read_bytes"] for r in receipts)):
        raise ValueError("idle_c5_rewrote_or_reread")
    if cycle == 1 and (writes <= 0 or generations < 1):
        raise ValueError("correction_c5_published_nothing")
    if cycle == 3 and (writes <= 0 or generations != 1):
        raise ValueError("changed_c5_not_published")
    return writes, generations


def resources_ok(resources):
    return bool(resources) and all(
        r["sessions"] == 1 and r["sleep_micros"] >= 3 * r["read_micros"] for r in resources)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--attempt", default="target")
    parser.add_argument("--c5", action="store_true")
    args = parser.parse_args()
    if not re.fullmatch(r"[a-z0-9-]{1,64}", args.attempt):
        parser.error("attempt")
    base = run_dir(args.run_id)
    admin = Admin(args.run_id)
    seed = json.loads((base / "seed-receipt.json").read_text())
    tables, scale, full = seed["tables"], seed["event_rows"], seed["full"]
    out = base / args.attempt
    out.mkdir(exist_ok=False)
    target_name = f"oo-d8-target-{args.run_id}-{args.attempt}"
    password = secrets.token_hex(24)
    reader = (base / ".private/reader").read_text()
    report = {"engine": "dm8", "scope": ("231 tables / 2M events / 300 columns" if full
                                         else "focused six-table fixture"),
              "T1": "fail", "T8": "fail", "writer_commits": 0, "reader_session_peak": 0,
              "reader_session_samples": 0, "execution": "native linux/arm64"}
    stop_writer, stop_sampler = threading.Event(), threading.Event()
    errors, created, threads = [], False, []
    last, moved, inserted = scale, 3_000_001, 3_000_002

    def redact(text):
        return admin.redact(text).replace(password, "[redacted]").replace(reader, "[redacted]")

    def run(cmd, data=None, timeout=1800, log=None):
        p = subprocess.run(cmd, input=data, capture_output=True, text=True, timeout=timeout)
        if log:
            (out / log).write_text(redact(p.stdout + p.stderr))
        if p.returncode:
            raise RuntimeError("command_failed:" + Path(cmd[0]).name + ":" + str(p.returncode))
        return p.stdout

    def pg(sql):
        return run(["docker", "exec", "-i", target_name, "psql", "-X", "-q", "-U", "postgres",
                    "-v", "ON_ERROR_STOP=1", "-v", "FETCH_COUNT=1000", "-At"],
                   "SET TimeZone='UTC';\n" + sql, timeout=1800)

    forward = f"""UPDATE FIXTURE.EVENTS SET VALUE='writer-a' WHERE ID=2;
UPDATE FIXTURE.EVENTS SET ID={moved} WHERE ID={last};
INSERT INTO FIXTURE.EVENTS VALUES ({inserted},'inserted',12,NULL,NULL);
DELETE FROM FIXTURE.EVENTS WHERE ID=1;
UPDATE FIXTURE.COMPOSITE SET K='y' WHERE K='z' AND ID=9;
COMMIT;"""
    restore = f"""UPDATE FIXTURE.EVENTS SET VALUE='row-2' WHERE ID=2;
UPDATE FIXTURE.EVENTS SET ID={last} WHERE ID={moved};
DELETE FROM FIXTURE.EVENTS WHERE ID={inserted};
INSERT INTO FIXTURE.EVENTS SELECT 1,'row-1',CAST(1 AS DECIMAL(31,8))/7,TIMESTAMP '2026-11-05 01:02:04.123456',CAST('2026-11-05 01:02:03.123456 +05:30' AS TIMESTAMP(6) WITH TIME ZONE) FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM FIXTURE.EVENTS WHERE ID=1);
UPDATE FIXTURE.COMPOSITE SET K='z' WHERE K='y' AND ID=9;
UPDATE FIXTURE.EVENTS SET VALUE='row-4' WHERE ID=4;
COMMIT;"""
    # Committed after the load returned, so no snapshot read can have seen
    # it: the correction cycle must publish at least this change.
    late = "UPDATE FIXTURE.EVENTS SET VALUE='after-load' WHERE ID=4;\nCOMMIT;"

    def writer():
        try:
            while not stop_writer.is_set():
                admin.sql(forward if report["writer_commits"] % 2 == 0 else restore)
                report["writer_commits"] += 1
                stop_writer.wait(.02)
        except Exception as e:
            errors.append("writer:" + type(e).__name__)

    def sampler():
        try:
            while not stop_sampler.is_set():
                count = admin.rows("SELECT 'R|'||COUNT(*) FROM V$SESSIONS WHERE USER_NAME='OOREADER'")
                if len(count) != 1:
                    raise RuntimeError("session_probe")
                report["reader_session_peak"] = max(report["reader_session_peak"], int(count[0]))
                report["reader_session_samples"] += 1
                stop_sampler.wait(.2)
        except Exception as e:
            errors.append("sampler:" + type(e).__name__)

    def compare(receipt, label):
        table = receipt["table"]["name"]
        kinds = columns(table)
        source_rows = merge_groups([admin.rows(select, timeout=1800)
                                    for select in source_selects(table, kinds)])
        (out / f"{label}-source-{table}.txt").write_text("\n".join(source_rows))
        left = [[canonical_source(k, v) for (_, k), v in zip(kinds, row.split("|"))]
                for row in source_rows]
        if any(len(row.split("|")) != len(kinds) for row in source_rows):
            raise RuntimeError("source_shape:" + table)
        raw = pg(f"SELECT row_to_json(t) FROM {receipt['target']} t;")
        (out / f"{label}-target-{table}.jsonl").write_text(raw)
        right = []
        for line in raw.splitlines():
            value = json.loads(line, parse_float=Decimal)
            if list(value) != [n for n, _ in kinds]:
                raise RuntimeError("target_columns:" + table)
            right.append([canonical_target(k, value[n]) for n, k in kinds])
        canon = lambda rows: sorted(json.dumps(r, ensure_ascii=True) for r in rows)
        left, right = canon(left), canon(right)
        if left != right:
            missing = sorted(set(left) - set(right))[:5]
            extra = sorted(set(right) - set(left))[:5]
            (out / f"{label}-diff-{table}.json").write_text(json.dumps(
                {"source_only": missing, "target_only": extra}, indent=2))
            raise RuntimeError("source_target_mismatch:" + table)
        return {"rows": len(left), "sha256": hashlib.sha256("\n".join(left).encode()).hexdigest(),
                "equal": True}

    def receipts_of(text, protocol):
        return [r for r in (json.loads(l) for l in text.splitlines() if l.startswith("{"))
                if r.get("protocol") == protocol]

    try:
        private(out / ".private/reader", reader)
        private(out / ".private/target-password", password)
        private(out / ".private/target.env", "POSTGRES_PASSWORD=" + password + "\n")
        pin = next(line.split()[2] for line in (ROOT / "testing/infra/images.pins").read_text().splitlines()
                   if line.startswith("postgres-17.10-bookworm linux/arm64 "))
        image = "postgres@" + pin
        run(["docker", "pull", "--platform", "linux/arm64", image], timeout=600, log="target-pull.log")
        run(["docker", "create", "--name", target_name, "--label", "oo.lane=" + LANE,
             "--label", "oo.run_id=" + args.run_id, "--platform", "linux/arm64",
             "--env-file", str(out / ".private/target.env"), "--memory", "1g",
             "-p", "127.0.0.1::5432", "--tmpfs", "/var/lib/postgresql/data", image])
        created = True
        run(["docker", "start", target_name])
        for _ in range(60):
            if subprocess.run(["docker", "exec", target_name, "pg_isready", "-U", "postgres"],
                              capture_output=True).returncode == 0:
                break
            time.sleep(1)
        else:
            raise RuntimeError("target_readiness")
        time.sleep(2)
        port = int(run(["docker", "port", target_name, "5432/tcp"]).strip().rsplit(":", 1)[1])
        stack = {
            "version": "oo.stack/v1",
            "sources": [{"id": "fixture", "engine": "dm8", "host": "127.0.0.1", "port": 5236,
                         "database": "DAMENG", "user": "OOREADER",
                         "password": {"secret_ref": "reader"},
                         "scope": {"include": ["FIXTURE." + t for t in tables], "exclude": []}}],
            "model": {"provider": "openai-compatible", "base_url": "https://example.invalid/v1",
                      "model": "fixture", "key": {"secret_ref": "model"}},
        }
        (out / "stack.json").write_text(json.dumps(stack))
        target = {"host": "127.0.0.1", "port": port, "database": "postgres", "user": "postgres",
                  "password": {"secret_ref": "target-password"}, "local_fixture": True,
                  "run_id": "dm8-concurrent"}
        (out / "target.json").write_text(json.dumps(target))
        command = [str(ROOT / "tools/oo-load/target/debug/oo-load"), "--stack", str(out / "stack.json"),
                   "--source", "fixture", "--target", str(out / "target.json"),
                   "--secrets-dir", str(out / ".private"),
                   "--chunk-rows", "100000" if full else "500", "--dm8-worker", str(base / "worker")]
        admin.sql(restore)
        session_zone = admin.rows("SELECT 'R|'||SESSIONTIMEZONE FROM DUAL")
        if session_zone != ["+00:00"]:
            raise RuntimeError("admin_session_zone_not_utc")
        threads = [threading.Thread(target=sampler), threading.Thread(target=writer)]
        for thread in threads:
            thread.start()
        load = run(command, log="load-concurrent.log", timeout=3600)
        stop_writer.set()
        threads[1].join(timeout=300)
        if threads[1].is_alive() or report["writer_commits"] == 0 or errors:
            raise RuntimeError("writer_not_proven")
        admin.sql(late)
        report["late_commit"] = "EVENTS ID=4 updated after the load returned"
        loads = receipts_of(load, "oo.load/v1")
        if len(loads) != len(tables):
            raise RuntimeError("load_receipts")
        version = {r["version"]["raw"] for r in loads}
        if len(version) != 1 or not re.fullmatch(r"DM Database Server \S+ V8 \d{11}-\d{8}-\d+-\d+",
                                                 next(iter(version))):
            raise RuntimeError("server_version_not_measured")
        report.update(version_raw=next(iter(version)), version_actual="8")
        (out / "load-receipts.json").write_text(json.dumps(loads, indent=2))
        report["c5_cycles"] = []
        policy = out / "c5.json"
        for cycle in range(1, 5 if args.c5 else 2):
            if cycle == 3:
                admin.sql("UPDATE FIXTURE.EVENTS SET VALUE='c5-changed' WHERE ID=3;\nCOMMIT;")
            target["run_id"] = f"dm8-c5-{cycle}"
            (out / "target.json").write_text(json.dumps(target))
            policy.write_text(json.dumps({"cycle": cycle, "full_read_every": 100}))
            output = run(command + ["--c5", str(policy)], log=f"c5-{cycle}.log", timeout=3600)
            sync = receipts_of(output, "oo.sync/v1")
            writes, generations = validate_cycle(sync, tables, cycle)
            compared = {r["table"]["name"]: compare(r, f"c5-{cycle}") for r in sync}
            report["c5_cycles"].append({
                "cycle": cycle, "role": {1: "correction", 3: "changed"}.get(cycle, "idle"),
                "equal": True, "comparisons": compared, "receipts": sync,
                "target_write_bytes": writes, "new_generations": generations})
            if cycle == 1:
                if compared["EVENTS"]["rows"] != scale:
                    raise RuntimeError("event_scale_not_met")
                report["comparisons"] = compared
                resumed = run(command + ["--c5", str(policy)], log="c5-1-resume.log")
                again = receipts_of(resumed, "oo.sync/v1")
                if len(again) != len(tables) or any(
                        r["target_write_bytes"] or r["new_generations"] for r in again):
                    raise RuntimeError("resume_rewrote")
        stop_sampler.set()
        threads[0].join(timeout=60)
        if errors or report["reader_session_samples"] == 0:
            raise RuntimeError("sampler_not_proven")
        relation = compared_relation(report, "TYPES").replace("'", "''")
        types = pg("SELECT a.attname||'|'||format_type(a.atttypid,a.atttypmod) FROM pg_attribute a "
                   f"WHERE a.attrelid='{relation}'::regclass AND a.attnum>0 "
                   "AND NOT a.attisdropped ORDER BY a.attnum;")
        report["types_target"] = dict(line.split("|", 1) for line in types.splitlines())
        if report["types_target"] != TYPES_TARGET:
            raise RuntimeError("type_mapping")
        resources = []
        for log in sorted(out.glob("*.log")):
            for line in log.read_text().splitlines():
                if line.startswith("{") and '"oo.load.resources/v1"' in line:
                    resources.append(dict(json.loads(line)["resources"], log=log.name))
        report["resources"] = resources
        if not resources_ok(resources) or report["reader_session_peak"] != 1:
            raise RuntimeError("resource_budget")
        report.update(T1="pass", T8="pass", model_tokens=0, model_calls=0,
                      correction="one C5 cycle after the concurrent load, no quiesced reload",
                      resume_no_rewrite=True, consistency="B", source_time="unmeasured")
        if args.c5:
            report["T2"] = "pass"
    except Exception as e:
        report["failure"] = redact(str(e))
        raise
    finally:
        stop_writer.set()
        stop_sampler.set()
        for thread in threads:
            thread.join(timeout=300)
        report["errors"] = errors
        if created:
            events = subprocess.run(["docker", "exec", target_name, "cat",
                                     "/sys/fs/cgroup/memory.events"], capture_output=True, text=True)
            (out / "target-memory-events.txt").write_text(events.stdout)
            report["target_cleanup_exit"] = subprocess.run(
                ["docker", "rm", "-f", target_name], capture_output=True).returncode
        raw = out / "target-acceptance.json"
        raw.write_text(json.dumps(report, indent=2))
        sys.path.insert(0, str(ROOT / "tools/oo-test"))
        from verdicts import write_rows
        run_info = SimpleNamespace(engine="dm8", version="8", id=args.run_id,
                                   receipt={"version_actual": report.get("version_actual"),
                                            "execution": "native"})
        verdicts = [("T1", "snapshot", report["T1"]), ("T8", "snapshot", report["T8"])]
        if args.c5:
            verdicts += [("T2", "c5", report.get("T2", "fail")), ("T8", "c5", report["T8"])]
        cleanup_failed = report.get("target_cleanup_exit", 0) != 0
        if report.get("version_actual"):
            write_rows(run_info, raw, verdicts, cleanup_failed=cleanup_failed)
        else:
            from verdicts import product_sha
            (out / "index-rows.json").write_text(json.dumps([{
                "target": "dm8", "version": "8", "suite": s, "variant": v, "result": "fail",
                "version_measured": False, "emulated": False, "run_id": args.run_id,
                "sha256": hashlib.sha256(raw.read_bytes()).hexdigest(), "evidence": raw.name,
                "observed_at": datetime.now(UTC).isoformat().replace("+00:00", "Z"),
                "product_sha": product_sha()} for s, v, _ in verdicts], indent=2) + "\n")


def compared_relation(report, table):
    receipts = report["c5_cycles"][-1]["receipts"]
    return next(r["target"] for r in receipts if r["table"]["name"] == table)


if __name__ == "__main__":
    main()
