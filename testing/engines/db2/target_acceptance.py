"""Run the real oo-load path and compare a quiescent source cut with Agent DB.

Requires fixture.py and its seed, plus a pinned source-only worker wrapper.
The writer uses only the separate admin connection. Never a customer source.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import subprocess
import threading
import time

ROOT = Path(__file__).resolve().parents[3]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--attempt", default="target")
    parser.add_argument("--full", action="store_true")
    args = parser.parse_args()
    if not all(re.fullmatch(r"[a-z0-9-]{1,64}", s) for s in (args.run_id, args.attempt)):
        parser.error("run-id")
    base = ROOT / "evidence" / args.run_id
    source = json.loads((base / "fixture-owned.json").read_text())["container"]
    out = base / args.attempt
    out.mkdir(exist_ok=False)
    target_name = "oo-e-db2-target-" + args.run_id + "-" + args.attempt
    private = out / ".private"
    private.mkdir(mode=0o700)
    password = secrets.token_hex(24)
    source_password = (base / ".private/reader").read_text()
    print("fixture: " + args.attempt, flush=True)
    report = {"version_actual": "11.5.9.0", "scope": "231 tables / 2M events / 300 columns" if args.full else "four-table focused acceptance",
              "T1": "fail", "T8": "fail", "writer_commits": 0,
              "reader_connection_peak": 0, "reader_connection_samples": 0}
    stopped = threading.Event()
    writer = None
    created = False
    errors = []
    last_id = 2_000_000 if args.full else 2000
    moved_id, inserted_id = 3_000_001, 3_000_002

    def secret(path, value):
        with os.fdopen(os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600), "w") as file:
            file.write(value)

    def run(cmd, data=None, timeout=180, log=None):
        p = subprocess.run(cmd, input=data, capture_output=True, text=True, timeout=timeout)
        if log:
            (out / log).write_text((p.stdout + p.stderr).replace(password, "[redacted]")
                                  .replace(source_password, "[redacted]"))
        if p.returncode:
            raise RuntimeError("command_failed:" + cmd[0] + ":" + str(p.returncode))
        return p.stdout

    def admin(sql):
        p = subprocess.run(["docker", "exec", "-i", source, "su", "-", "db2inst1", "-c", "db2 +p -xst"],
                           input="CONNECT TO FIXTURE;\n" + sql + "\n", text=True,
                           capture_output=True, timeout=600)
        # CLP 1 is SQLCODE +100 (no rows), including silent SELECT EOF under -x.
        # SQL errors use exit 4/8; do not infer errors solely from process != 0.
        if p.returncode not in (0, 1) or re.search(r"SQL\d+[NCE]\b", p.stdout + p.stderr):
            raise RuntimeError("admin_sql_failed:" + str(p.returncode))
        return p.stdout

    def pg(sql):
        return run(["docker", "exec", "-i", target_name, "psql", "-X", "-U", "postgres",
                    "-v", "ON_ERROR_STOP=1", "-v", "FETCH_COUNT=1000", "-At"], sql,
                   timeout=600)

    def mutate():
        try:
            while not stopped.is_set():
                forward = report["writer_commits"] % 2 == 0
                admin(f"""
UPDATE FIXTURE.EVENTS SET VALUE='writer-a' WHERE ID=2;
UPDATE FIXTURE.EVENTS SET ID={moved_id} WHERE ID={last_id};
INSERT INTO FIXTURE.EVENTS VALUES ({inserted_id},'inserted',12,NULL);
DELETE FROM FIXTURE.EVENTS WHERE ID=1;
""" if forward else f"""
UPDATE FIXTURE.EVENTS SET VALUE='row-2' WHERE ID=2;
UPDATE FIXTURE.EVENTS SET ID={last_id} WHERE ID={moved_id};
INSERT INTO FIXTURE.EVENTS VALUES (1,'row-1',DECIMAL(1,31,8)/7,CAST('2026-11-05-01.02.03.123456789012' AS TIMESTAMP(12)));
DELETE FROM FIXTURE.EVENTS WHERE ID={inserted_id};
""")
                report["writer_commits"] += 1
                observed = admin("SELECT COUNT(*) FROM SYSIBMADM.APPLICATIONS WHERE AUTHID='OOREADER';")
                counts = [int(line.strip().rstrip(".")) for line in observed.splitlines()
                          if re.fullmatch(r"\s*\d+\.?\s*", line)]
                if len(counts) != 1:
                    raise RuntimeError("reader_connection_probe")
                report["reader_connection_peak"] = max(report["reader_connection_peak"], counts[0])
                report["reader_connection_samples"] += 1
                stopped.wait(.02)
        except Exception as e:
            errors.append(type(e).__name__)

    tables = ["EVENTS", "COMPOSITE", "EMPTY", "NO_KEY"]
    columns = {
        "EVENTS": ["VARCHAR(ID)", "VALUE", "VARCHAR(AMOUNT)", "VARCHAR(INSTANT)"],
        "COMPOSITE": ["K", "VARCHAR(ID)", "VALUE"],
        "EMPTY": ["VARCHAR(ID)", "VALUE"],
        "NO_KEY": ["VALUE"],
    }
    if args.full:
        tables += ["WIDE_ROWS"] + [f"TABLE_{i:03}" for i in range(226)]
        columns["WIDE_ROWS"] = ["VARCHAR(ID)"] + [f"C{i:03}" for i in range(1, 300)]
        for name in tables[5:]:
            columns[name] = ["VARCHAR(ID)", "VALUE"]
    try:
        secret(private / "reader", source_password)
        secret(private / "target-password", password)
        secret(private / "target.env", "POSTGRES_PASSWORD=" + password + "\n")
        pin = next(line.split()[2] for line in (ROOT / "testing/infra/images.pins").read_text().splitlines()
                   if line.startswith("postgres-17.10-bookworm linux/arm64 "))
        image = "postgres@" + pin
        run(["docker", "pull", "--platform", "linux/arm64", image], timeout=600, log="target-pull.log")
        run(["docker", "create", "--name", target_name, "--label", "oo.lane=e",
             "--label", "oo.run_id=" + args.run_id, "--platform", "linux/arm64",
             "--env-file", str(private / "target.env"), "--memory", "512m",
             "-p", "127.0.0.1::5432", "--tmpfs", "/var/lib/postgresql/data", image])
        created = True
        run(["docker", "start", target_name])
        for _ in range(60):
            p = subprocess.run(["docker", "exec", target_name, "pg_isready", "-U", "postgres"],
                               capture_output=True)
            if p.returncode == 0:
                break
            time.sleep(1)
        else:
            raise RuntimeError("target_readiness")
        port = int(run(["docker", "port", target_name, "5432/tcp"]).strip().rsplit(":", 1)[1])
        stack = {
            "version": "oo.stack/v1",
            "sources": [{"id": "fixture", "engine": "db2", "host": "127.0.0.1", "port": 50000,
                         "database": "FIXTURE", "user": "ooreader", "password": {"secret_ref": "reader"},
                         "scope": {"include": ["FIXTURE." + t for t in tables], "exclude": []}}],
            "model": {"provider": "openai-compatible", "base_url": "https://example.invalid/v1",
                      "model": "fixture", "key": {"secret_ref": "model"}},
        }
        (out / "stack.json").write_text(json.dumps(stack))
        target = {"host": "127.0.0.1", "port": port, "database": "postgres", "user": "postgres",
                  "password": {"secret_ref": "target-password"}, "local_fixture": True,
                  "run_id": "db2-concurrent"}
        (out / "target.json").write_text(json.dumps(target))
        command = [str(ROOT / "tools/oo-load/target/debug/oo-load"), "--stack", str(out / "stack.json"),
                   "--source", "fixture", "--target", str(out / "target.json"), "--secrets-dir", str(private),
                   "--chunk-rows", "100000" if args.full else "100", "--db2-worker", str(base / "worker")]
        # A fresh attempt begins from the original primary-key state.
        admin(f"""
DELETE FROM FIXTURE.EVENTS WHERE ID={inserted_id};
UPDATE FIXTURE.EVENTS SET ID={last_id} WHERE ID={moved_id};
UPDATE FIXTURE.EVENTS SET VALUE='row-2' WHERE ID=2;
INSERT INTO FIXTURE.EVENTS SELECT 1,'row-1',DECIMAL(1,31,8)/7,CAST('2026-11-05-01.02.03.123456789012' AS TIMESTAMP(12))
FROM SYSIBM.SYSDUMMY1 WHERE NOT EXISTS (SELECT 1 FROM FIXTURE.EVENTS WHERE ID=1);
""")
        writer = threading.Thread(target=mutate)
        writer.start()
        run(command, timeout=1800, log="load-concurrent.log")
        print("concurrent load complete", flush=True)
        stopped.set()
        writer.join(timeout=180)
        if writer.is_alive() or errors or report["writer_commits"] == 0:
            raise RuntimeError("writer_not_proven")
        # B consistency: quiesce the writer, then explicitly reread the whole cut.
        target["run_id"] = "db2-corrected"
        (out / "target.json").write_text(json.dumps(target))
        raw = run(command, timeout=1800, log="load-corrected.log")
        print("quiescent load complete", flush=True)
        receipts = [json.loads(line) for line in raw.splitlines() if line.startswith("{")]
        if not receipts or any(r.get("version", {}).get("raw") != "11.5.9.0" for r in receipts):
            raise RuntimeError("server_version_not_measured")
        (out / "target-receipts.json").write_text(json.dumps(receipts, indent=2))
        comparisons = {}
        for receipt in receipts:
            table = receipt["table"]["name"]
            print("compare " + table, flush=True)
            expressions = ["COALESCE(HEX(" + c + "),'-')" for c in columns[table]]
            source_raw = admin("SELECT " + "||'|'||".join(expressions) + " FROM FIXTURE." + table + ";")
            (out / ("source-" + table + ".txt")).write_text(source_raw)
            rows = []
            for line in source_raw.splitlines():
                line = line.strip()
                if re.fullmatch(r"[A-F0-9|-]+", line):
                    rows.append([None if c == "-" else bytes.fromhex(c).decode("utf-8")
                                 for c in line.split("|")])
            target_raw = pg("SELECT row_to_json(t) FROM " + receipt["target"] + " t;")
            (out / ("target-" + table + ".jsonl")).write_text(target_raw)
            target_rows = [[None if v is None else str(v) for v in json.loads(line).values()]
                           for line in target_raw.splitlines()]
            canonical = lambda values: sorted(json.dumps(r, ensure_ascii=True) for r in values)
            left, right = canonical(rows), canonical(target_rows)
            if left != right:
                raise RuntimeError("source_target_mismatch:" + table)
            comparisons[table] = {"rows": len(rows), "sha256": hashlib.sha256("\n".join(left).encode()).hexdigest(),
                                  "equal": True}
        if set(comparisons) != set(tables):
            raise RuntimeError("missing_target_table")
        expected = 2_000_000 if args.full else 2000
        if comparisons["EVENTS"]["rows"] != expected:
            raise RuntimeError("event_scale_not_met")
        resumed = run(command, timeout=600, log="load-resume.log")
        if not all(json.loads(line)["replay"] for line in resumed.splitlines() if line.startswith("{")):
            raise RuntimeError("resume_rewrote")
        resources = []
        for label in ("load-concurrent.log", "load-corrected.log", "load-resume.log"):
            for line in (out / label).read_text().splitlines():
                if line.startswith("{"):
                    row = json.loads(line)
                    if row.get("protocol") == "oo.load.resources/v1":
                        resources.append(row["resources"])
        if len(resources) != 3 or not all(r["sessions"] == 1 and r["sleep_micros"] >= 3 * r["read_micros"]
                                         for r in resources):
            raise RuntimeError("resource_budget")
        if report["reader_connection_peak"] != 1 or report["reader_connection_samples"] == 0:
            raise RuntimeError("reader_connection_budget")
        report.update(T1="pass", T8="pass", comparisons=comparisons, resources=resources,
                      target_write_bytes=sum(r["target_write_bytes"] for r in receipts),
                      model_tokens=0, model_calls=0, correction="quiesce_then_full_reread",
                      resume_no_rewrite=True, consistency="B", source_time="unmeasured")
    except Exception as e:
        report["failure"] = str(e)
        raise
    finally:
        stopped.set()
        if writer:
            writer.join(timeout=180)
        if created:
            metrics = subprocess.run(["docker", "exec", target_name, "cat",
                                      "/sys/fs/cgroup/memory.events"], capture_output=True, text=True)
            (out / "target-memory-events.txt").write_text(metrics.stdout)
            cleanup = subprocess.run(["docker", "rm", "-f", target_name], capture_output=True)
            report["target_cleanup_exit"] = cleanup.returncode
        report["writer_errors"] = errors
        (out / "target-acceptance.json").write_text(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
