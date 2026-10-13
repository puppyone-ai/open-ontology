#!/usr/bin/env python3
"""Load the finished capture's decoded rows into a new local Postgres.

Does not start StarRocks and does not repeat the capture run. The in-run
COPY attempts remain the failed pg-load-*.exit records.
"""

from __future__ import annotations

import importlib.util
import json
import socket
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

REPO = Path("/Users/redux/puppy-one/open-ontology")
SESSION = "2c023948-0862-4dae-ae8f-6fc30241db62"
PREFIX = "oo-dl01w1-45"
EVIDENCE = REPO / "evidence" / SESSION / "DL01" / "starrocks-product" / "raw" / "capture-schedule"
SRC = REPO / "testing" / "dl01-driver-preflight" / "starrocks"
ADMIT = SRC / "scripts" / "admit.py"
OUT = EVIDENCE / "pg-mirror"
PG_IMAGE = "docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb"
PG_CONTAINER = f"{PREFIX}-postgres"
LABELS = ("baseline", "catchup", "slow", "expired")
TABLES = ("dup_events", "pk_accounts", "agg_metrics", "uniq_dim", "dup_partition_sums")
COLUMNS = {
    "dup_events": ["event_id", "bucket", "seen", "amount", "note"],
    "pk_accounts": ["account_id", "balance", "nickname", "opened"],
    "agg_metrics": ["metric_day", "metric_name", "qty", "revenue"],
    "uniq_dim": ["dim_id", "label", "score"],
    "dup_partition_sums": ["part", "row_count", "amount_sum"],
}
DDL = {
    "dup_events": "CREATE TABLE capture.dup_events (event_id bigint, bucket date, seen timestamp, amount numeric(18,4), note text)",
    "pk_accounts": "CREATE TABLE capture.pk_accounts (account_id bigint, balance numeric(38,9), nickname text, opened timestamp)",
    "agg_metrics": "CREATE TABLE capture.agg_metrics (metric_day date, metric_name text, qty bigint, revenue numeric(18,4))",
    "uniq_dim": "CREATE TABLE capture.uniq_dim (dim_id bigint, label text, score numeric(10,2))",
    "dup_partition_sums": "CREATE TABLE capture.dup_partition_sums (part text, row_count bigint, amount_sum numeric(38,4))",
}


def now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def run(cmd: list[str], timeout: int = 60, stdin: bytes | None = None) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(cmd, input=stdin, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout, check=False)


def save(name: str, proc: subprocess.CompletedProcess[bytes]) -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / f"{name}.stdout").write_bytes(proc.stdout)
    (OUT / f"{name}.stderr").write_bytes(proc.stderr)
    (OUT / f"{name}.exit").write_text(f"{proc.returncode}\n", encoding="utf-8")


def oracle():
    spec = importlib.util.spec_from_file_location("sr_oracle", SRC / "scripts" / "oracle.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def cell(value: object) -> str:
    if value is None:
        return r"\N"
    return str(value)


def script_for(rows_by_table: dict[str, list[dict]]) -> str:
    lines = ["BEGIN;"]
    for table in TABLES:
        columns = COLUMNS[table]
        lines.append(f"DELETE FROM capture.{table};")
        lines.append(f"COPY capture.{table} ({','.join(columns)}) FROM STDIN;")
        for row in rows_by_table[table]:
            lines.append("\t".join(cell(row.get(name)) for name in columns))
        lines.append("\\.")
    lines.append("COMMIT;")
    return "\n".join(lines) + "\n"


def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    mod = oracle()
    probe = socket.socket()
    try:
        probe.bind(("127.0.0.1", 15445))
    except OSError as exc:
        (OUT / "port.txt").write_text(str(exc), encoding="utf-8")
        return 1
    finally:
        probe.close()
    command = [
        "docker", "run", "-d", "--name", PG_CONTAINER,
        "--memory", "1g", "--memory-swap", "1g",
        "--restart", "no", "--pull", "never",
        "-p", "127.0.0.1:15445:5432",
        "-e", "POSTGRES_HOST_AUTH_METHOD=trust",
        "-e", "POSTGRES_USER=postgres",
        "--label", "project=open-ontology",
        "--label", "kind=capture-cell-mirror",
        "--label", f"oo.prefix={PREFIX}",
        "--label", "oo.task=DL01-W1-45",
        "--label", "oo.capture=frozen-mirror",
        PG_IMAGE,
    ]
    started = run([sys.executable, str(ADMIT), "acquire-start", "postgres", str(OUT / "admission.json"), "--", *command], timeout=180)
    save("admit", started)
    if started.returncode != 0:
        return started.returncode
    quota = run(["docker", "inspect", "-f", "{{.HostConfig.NanoCpus}} {{.HostConfig.Memory}}", PG_CONTAINER])
    save("inspect-quota", quota)
    cgroup = run(["docker", "exec", PG_CONTAINER, "sh", "-c", "cat /sys/fs/cgroup/cpu.max 2>/dev/null || echo absent"])
    save("cpu-max", cgroup)
    ready = False
    for _ in range(60):
        probe_ready = run(["docker", "exec", PG_CONTAINER, "pg_isready", "-U", "postgres"])
        if probe_ready.returncode == 0:
            save("pg-ready", probe_ready)
            ready = True
            break
        subprocess.run(["sleep", "1"], check=False)
    if not ready:
        run(["docker", "rm", "-f", PG_CONTAINER])
        run([sys.executable, str(ADMIT), "release", "postgres"])
        return 1
    created = run(["docker", "exec", PG_CONTAINER, "psql", "-U", "postgres", "-c", "CREATE DATABASE oo_w45;"])
    save("createdb", created)
    ddl = "CREATE SCHEMA IF NOT EXISTS capture;\n" + "\n".join(statement + ";" for statement in DDL.values()) + "\n"
    made = run(["docker", "exec", "-i", PG_CONTAINER, "psql", "-v", "ON_ERROR_STOP=1", "-U", "postgres", "-d", "oo_w45"], stdin=ddl.encode())
    save("ddl", made)
    report = {"started_at": now(), "source_restarted": False, "labels": {}}
    failed = created.returncode != 0 or made.returncode != 0
    for label in LABELS:
        decoded_dir = EVIDENCE / "decoded" / label
        native_dir = EVIDENCE / "native" / label
        rows = {}
        for table in TABLES:
            _document, loaded = mod.load_decoded(decoded_dir / f"{table}.json", table)
            rows[table] = loaded
        body = script_for(rows)
        (OUT / f"{label}.copy.sql").write_text(body, encoding="utf-8")
        loaded_proc = run(["docker", "exec", "-i", PG_CONTAINER, "psql", "-v", "ON_ERROR_STOP=1", "-U", "postgres", "-d", "oo_w45"], stdin=body.encode(), timeout=60)
        save(f"load-{label}", loaded_proc)
        label_report = {"load_exit": loaded_proc.returncode, "tables": {}}
        for table in TABLES:
            columns = ",".join(COLUMNS[table])
            selected = run(["docker", "exec", PG_CONTAINER, "psql", "-U", "postgres", "-d", "oo_w45", "-At", "-c", f"COPY (SELECT {columns} FROM capture.{table}) TO STDOUT WITH (FORMAT text, NULL 'NULL');"])
            save(f"select-{label}-{table}", selected)
            text = selected.stdout.decode("utf-8", "replace")
            (OUT / label).mkdir(parents=True, exist_ok=True)
            (OUT / label / f"{table}.tsv").write_text(text, encoding="utf-8")
            native_text = (native_dir / f"{table}.tsv").read_text(encoding="utf-8")
            got = mod.parse_tsv(table, text)
            native = mod.parse_tsv(table, native_text)
            same = mod.canon(got) == mod.canon(native) == mod.canon(rows[table])
            label_report["tables"][table] = {
                "equal": same,
                "rows": len(got),
                "cell_mismatches": [] if same else {"pg": mod.canon(got), "native": mod.canon(native), "decoded": mod.canon(rows[table])},
                "sampled": False,
            }
            failed = failed or not same or selected.returncode != 0
        if label == "catchup":
            absent = run(["docker", "exec", PG_CONTAINER, "psql", "-U", "postgres", "-d", "oo_w45", "-At", "-c", "SELECT count(*) FROM capture.pk_accounts WHERE account_id = 302;"])
            save("pk-302", absent)
            label_report["pk_302"] = absent.stdout.decode().strip()
            failed = failed or label_report["pk_302"] != "0"
        report["labels"][label] = label_report
        failed = failed or loaded_proc.returncode != 0
    report["finished_at"] = now()
    report["failed"] = failed
    report["in_run_pg_load_preserved"] = {
        "baseline": (EVIDENCE / "phases" / "pg-load-baseline.exit").read_text(encoding="utf-8").strip(),
        "catchup": (EVIDENCE / "phases" / "pg-load-catchup.exit").read_text(encoding="utf-8").strip(),
        "slow": (EVIDENCE / "phases" / "pg-load-slow.exit").read_text(encoding="utf-8").strip(),
    }
    (OUT / "report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    removed = run(["docker", "rm", "-f", PG_CONTAINER], timeout=60)
    save("cleanup", removed)
    run([sys.executable, str(ADMIT), "release", "postgres"])
    print(json.dumps({"failed": failed, "labels": {name: {table: item["equal"] for table, item in body["tables"].items()} for name, body in report["labels"].items()}}))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
