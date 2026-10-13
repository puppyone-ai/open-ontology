#!/usr/bin/env python3
"""Code 497 in its own account and its own client process.

This is not the product reader. The product user's Insert count is counted
again after this statement and must stay 0. The product-run-12 log is not
opened for writing.
"""

import json
import os
import subprocess
from pathlib import Path

CH = "oo-dl01w1-40-clickhouse"
DENY = "oo_dl01w1_40_dba_neg"
READER = "oo_dl01w1_40_reader"
DBA = "oo_dl01w1_40_dba"
QUERY_ID = "oo-dl01w1-40-dba-neg-497"
INSERT = (
    "INSERT INTO oo_dl01w40.readings "
    "(reading_id, version, amount, code, ts, tags, labels, wide_u64) "
    "VALUES (1, 1, 0, 'x', now64(9), [], [], 0)"
)


def output_dir() -> Path:
    chosen = os.environ.get("OO_CUSTOMER_OUT", "")
    if not chosen or os.environ.get("OO_RETURN_WINDOW") != "1":
        raise SystemExit("the negative window only runs as the return window")
    path = Path(chosen).resolve()
    if "product-run-12" in str(path):
        raise SystemExit("refusing to write the negative window into product-run-12")
    return path


OUT = output_dir()


def run(args: list[str], timeout: int = 60) -> subprocess.CompletedProcess[str]:
    return subprocess.run(args, text=True, capture_output=True, timeout=timeout)


def ch(user: str, password_file: str, sql: str, query_id: str = "") -> subprocess.CompletedProcess[str]:
    script = (
        "export CLICKHOUSE_PASSWORD=\"$(tr -d '[:space:]' < \"$1\")\"; "
        'args=(clickhouse-client --user "$2"); '
        'if [ -n "${4:-}" ]; then args+=(--query_id "$4"); fi; '
        'args+=(--query "$3"); '
        '"${args[@]}"'
    )
    return run(
        ["docker", "exec", CH, "bash", "-lc", script, "bash", password_file, user, sql, query_id]
    )


def save(name: str, proc: subprocess.CompletedProcess[str]) -> None:
    (OUT / f"{name}.stdout").write_text(proc.stdout)
    (OUT / f"{name}.stderr").write_text(proc.stderr)
    (OUT / f"{name}.exit").write_text(f"{proc.returncode}\n")


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    denied = ch(DENY, "/etc/oo/dba-neg.password", INSERT, query_id=QUERY_ID)
    save("dba-neg-insert", denied)
    flush = ch(DBA, "/etc/oo/dba.password", "SYSTEM FLUSH LOGS", query_id="oo-dl01w1-40-dba-neg-flush")
    save("dba-neg-flush", flush)
    logged = ch(
        DBA,
        "/etc/oo/dba.password",
        "SELECT user, type, exception_code, written_rows, query_id, count() "
        "FROM system.query_log WHERE query_id = 'oo-dl01w1-40-dba-neg-497' "
        "GROUP BY user, type, exception_code, written_rows, query_id FORMAT JSONEachRow",
        query_id="oo-dl01w1-40-dba-neg-log",
    )
    save("dba-neg-log", logged)
    product = ch(
        READER,
        "/etc/oo/reader.password",
        "SELECT count() FROM system.query_log "
        "WHERE user = 'oo_dl01w1_40_reader' AND query_kind = 'Insert'",
        query_id="oo-dl01w1-40-ro-insert-count-after-neg",
    )
    save("reader-insert-count-after-neg", product)
    counted = ch(
        READER,
        "/etc/oo/reader.password",
        "SELECT count() FROM oo_dl01w40.readings FINAL",
        query_id="oo-dl01w1-40-ro-count-after-neg",
    )
    save("count-after-neg", counted)
    rows = [json.loads(line) for line in logged.stdout.splitlines() if line.strip()]
    body = {
        "account": DENY,
        "window": "separate-client",
        "query_id": QUERY_ID,
        "statement": INSERT,
        "insert_exit": denied.returncode,
        "log": rows,
        "product_insert_count": product.stdout.strip(),
        "count_after": counted.stdout.strip(),
        "preserved_497_log": (
            "evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/"
            "raw/run/product-run-12-customer-chain/out/raw/reader-insert-log.stdout"
        ),
    }
    (OUT / "dba-negative.json").write_text(json.dumps(body, indent=2) + "\n")
    problems = []
    if denied.returncode == 0:
        problems.append("dba-neg-insert-succeeded")
    if "Code: 497" not in denied.stderr:
        problems.append("dba-neg-not-497")
    if flush.returncode != 0 or logged.returncode != 0 or product.returncode != 0:
        problems.append("dba-neg-log-query")
    if not any(
        row.get("user") == DENY and str(row.get("exception_code")) == "497" for row in rows
    ):
        problems.append("dba-neg-log-missing")
    if any(row.get("user") == READER for row in rows):
        problems.append("dba-neg-logged-as-product")
    if product.stdout.strip() != "0":
        problems.append("product-insert-count-after-neg")
    if counted.stdout.strip() != "3":
        problems.append("count-after-neg")
    if problems:
        (OUT / "DBA_NEG_FAIL").write_text(" ".join(problems) + "\n")
        raise SystemExit(" ".join(problems))
    (OUT / "DBA_NEG_OK").write_text("ok\n")


if __name__ == "__main__":
    main()
