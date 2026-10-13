#!/usr/bin/env python3
"""DBA fixture window, then product-account reads only.

The product user issues no INSERT and no other object write. A denied insert
is still a write attempt, so it is not issued here. Code 497 stays in
dba_negative_window.py, which is a separate account and a separate client.
"""

import json
import os
import subprocess
import time
from pathlib import Path

BASE = Path("/tmp/oo-dl01w1-40-bind")
CH = os.environ.get("OO_CUSTOMER_CH", "oo-dl01w1-40-clickhouse")
READER = "oo_dl01w1_40_reader"
DBA = "oo_dl01w1_40_dba"
WRITE_KINDS = {
    "Insert",
    "Alter",
    "Create",
    "Drop",
    "Rename",
    "Optimize",
    "Truncate",
    "KillQuery",
    "Grant",
    "Revoke",
    "Move",
    "Undrop",
    "System",
}


def output_dir() -> Path:
    chosen = os.environ.get("OO_CUSTOMER_OUT", "")
    if os.environ.get("OO_RETURN_WINDOW") == "1":
        if not chosen:
            raise SystemExit("return window needs OO_CUSTOMER_OUT")
        path = Path(chosen).resolve()
        if "product-run-12" in str(path):
            raise SystemExit("refusing to write the return window into product-run-12")
        return path
    if chosen:
        return Path(chosen)
    return BASE / "out" / "raw"


OUT = output_dir()
FIXTURE = Path(
    "/Users/redux/puppy-one/open-ontology/testing/dl01-driver-preflight/clickhouse/sql/00-fixture.sql"
)


def run(args: list[str], timeout: int = 60) -> subprocess.CompletedProcess[str]:
    return subprocess.run(args, text=True, capture_output=True, timeout=timeout)


def ch(
    user: str,
    password_file: str,
    sql: str,
    timeout: int = 60,
    query_id: str = "",
) -> subprocess.CompletedProcess[str]:
    script = (
        "export CLICKHOUSE_PASSWORD=\"$(tr -d '[:space:]' < \"$1\")\"; "
        'args=(clickhouse-client --user "$2"); '
        'if [ -n "${4:-}" ]; then args+=(--query_id "$4"); fi; '
        'args+=(--query "$3"); '
        '"${args[@]}"'
    )
    return run(
        ["docker", "exec", CH, "bash", "-lc", script, "bash", password_file, user, sql, query_id],
        timeout=timeout,
    )


def save(name: str, proc: subprocess.CompletedProcess[str]) -> None:
    (OUT / f"{name}.stdout").write_text(proc.stdout)
    (OUT / f"{name}.stderr").write_text(proc.stderr)
    (OUT / f"{name}.exit").write_text(f"{proc.returncode}\n")


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    deadline = time.time() + 180
    while time.time() < deadline:
        ping = ch(DBA, "/etc/oo/dba.password", "SELECT version()")
        if ping.returncode == 0 and ping.stdout.strip():
            (OUT / "clickhouse-version.txt").write_text(ping.stdout)
            break
        time.sleep(2)
    else:
        raise SystemExit("clickhouse did not answer")
    loaded = ch(DBA, "/etc/oo/dba.password", FIXTURE.read_text(), timeout=120)
    save("fixture-load", loaded)
    if loaded.returncode != 0:
        raise SystemExit("fixture load failed")
    probe = (
        "CREATE TABLE IF NOT EXISTS oo_dl01w40.byte_probe (id UInt64, payload String) "
        "ENGINE = MergeTree ORDER BY id; "
        "INSERT INTO oo_dl01w40.byte_probe VALUES (1, unhex('ff00fe'));"
    )
    probed = ch(DBA, "/etc/oo/dba.password", probe)
    save("probe-load", probed)
    if probed.returncode != 0:
        raise SystemExit("non-utf probe load failed")
    counted = ch(
        READER,
        "/etc/oo/reader.password",
        "SELECT count() FROM oo_dl01w40.readings FINAL",
        query_id="oo-dl01w1-40-ro-count",
    )
    save("count-final", counted)
    if counted.returncode != 0:
        raise SystemExit("independent count failed")
    count = counted.stdout.strip()
    (OUT / "independent-count.txt").write_text(count + "\n")
    if count != "3":
        (OUT / "COUNT_MISMATCH").write_text(count + "\n")
        raise SystemExit(f"independent count is {count}")
    (OUT / "FIXTURE_OK").write_text("ok\n")
    parts_sql = (
        "SELECT count(), sum(data_uncompressed_bytes) FROM system.parts "
        "WHERE database = 'oo_dl01w40' AND table = 'readings' AND active"
    )
    before = ch(READER, "/etc/oo/reader.password", parts_sql, query_id="oo-dl01w1-40-ro-parts-before")
    save("parts-before", before)
    extract = (
        "SELECT toString(reading_id) AS reading_id, "
        "toString(toUnixTimestamp64Nano(ts)) AS ts, "
        "toString(amount) AS amount, "
        "if(isNull(note), NULL, hex(note)) AS note, "
        "toString(qty) AS qty "
        "FROM oo_dl01w40.readings FINAL ORDER BY reading_id FORMAT JSONEachRow"
    )
    extracted = ch(READER, "/etc/oo/reader.password", extract, query_id="oo-dl01w1-40-ro-extract")
    save("source-rows", extracted)
    if extracted.returncode != 0:
        raise SystemExit("source extract failed")
    five_script = (
        "export CLICKHOUSE_PASSWORD=\"$(tr -d '[:space:]' < /etc/oo/reader.password)\"; "
        "clickhouse-client --user oo_dl01w1_40_reader "
        "--query_id oo-dl01w1-40-ro-arrow "
        "--query \"SELECT reading_id, ts, amount, note, qty FROM oo_dl01w40.readings FINAL "
        "ORDER BY reading_id FORMAT ArrowStream\" "
        "> /tmp/customer-five.arrow"
    )
    five = run(["docker", "exec", CH, "bash", "-lc", five_script], timeout=60)
    save("customer-five-arrow", five)
    if five.returncode != 0:
        raise SystemExit("five-column arrow extract failed")
    copied_five = run(
        ["docker", "cp", f"{CH}:/tmp/customer-five.arrow", str(OUT / "customer-five.arrow")]
    )
    if copied_five.returncode != 0:
        raise SystemExit(copied_five.stderr)
    arrow_script = (
        "export CLICKHOUSE_PASSWORD=\"$(tr -d '[:space:]' < /etc/oo/reader.password)\"; "
        "clickhouse-client --user oo_dl01w1_40_reader "
        "--query_id oo-dl01w1-40-ro-nonutf "
        "--query \"SELECT payload FROM oo_dl01w40.byte_probe WHERE id = 1 FORMAT ArrowStream\" "
        "> /tmp/non-utf.arrow"
    )
    arrow = run(["docker", "exec", CH, "bash", "-lc", arrow_script], timeout=60)
    save("non-utf-arrow", arrow)
    if arrow.returncode != 0:
        raise SystemExit("arrow extract failed")
    copied = run(["docker", "cp", f"{CH}:/tmp/non-utf.arrow", str(OUT / "non-utf.arrow")])
    if copied.returncode != 0:
        raise SystemExit(copied.stderr)
    flush = ch(DBA, "/etc/oo/dba.password", "SYSTEM FLUSH LOGS", query_id="oo-dl01w1-40-dba-flush")
    save("flush-logs", flush)
    if flush.returncode != 0:
        raise SystemExit("flush logs failed")
    insert_attempts = ch(
        READER,
        "/etc/oo/reader.password",
        "SELECT count() FROM system.query_log "
        "WHERE user = 'oo_dl01w1_40_reader' AND query_kind = 'Insert'",
        query_id="oo-dl01w1-40-ro-insert-count",
    )
    save("reader-insert-count", insert_attempts)
    kinds = ch(
        READER,
        "/etc/oo/reader.password",
        "SELECT query_kind, type, count() FROM system.query_log "
        "WHERE user = 'oo_dl01w1_40_reader' "
        "GROUP BY query_kind, type ORDER BY query_kind, type FORMAT JSONEachRow",
        query_id="oo-dl01w1-40-ro-kinds",
    )
    save("reader-query-kinds", kinds)
    after = ch(READER, "/etc/oo/reader.password", parts_sql, query_id="oo-dl01w1-40-ro-parts-after")
    save("parts-after", after)
    insert_count = insert_attempts.stdout.strip()
    kind_rows = [json.loads(line) for line in kinds.stdout.splitlines() if line.strip()]
    write_kinds = sorted({row.get("query_kind", "") for row in kind_rows if row.get("query_kind") in WRITE_KINDS})
    body = {
        "version": (OUT / "clickhouse-version.txt").read_text().strip(),
        "fixture_exit": loaded.returncode,
        "probe_exit": probed.returncode,
        "count": count,
        "extract_exit": extracted.returncode,
        "five_arrow_exit": five.returncode,
        "arrow_exit": arrow.returncode,
        "reader_insert_issued": False,
        "insert_count": insert_count,
        "query_kinds": kind_rows,
        "parts_before": before.stdout.strip(),
        "parts_after": after.stdout.strip(),
        "preserved_497_log": (
            "evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/"
            "raw/run/product-run-12-customer-chain/out/raw/reader-insert-log.stdout"
        ),
    }
    (OUT / "source-side.json").write_text(json.dumps(body, indent=2) + "\n")
    problems = []
    if insert_attempts.returncode != 0:
        problems.append("insert-count-query")
    if insert_count != "0":
        problems.append(f"insert-count-{insert_count or 'empty'}")
    if write_kinds:
        problems.append("write-kinds-" + ",".join(write_kinds))
    if before.stdout.strip() != after.stdout.strip():
        problems.append("parts-changed")
    if problems:
        (OUT / "SOURCE_SIDE_FAIL").write_text(" ".join(problems) + "\n")
        raise SystemExit(" ".join(problems))
    (OUT / "SOURCE_SIDE_OK").write_text("ok\n")


if __name__ == "__main__":
    main()
