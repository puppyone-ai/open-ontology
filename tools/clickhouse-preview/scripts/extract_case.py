#!/usr/bin/env python3
"""Read the live ClickHouse fixture for one approved field case.

The customer decision is already written. This script executes only the SQL
that decision marks as a source read. Refused requests stay on the host.
Counts come from the DBA account so the audit statement is not a reader query.
Row values stay in the raw stdout files. The structural files keep counts,
hashes, type names, and equality flags.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import time
from pathlib import Path

CH = "oo-dl01w1-40-clickhouse"
READER = "oo_dl01w1_40_reader"
DBA = "oo_dl01w1_40_dba"
NINE = (
    "SELECT toString(reading_id) AS reading_id, toString(version) AS version, "
    "toString(amount) AS amount, "
    "if(isNull(note), NULL, hex(note)) AS note, "
    "hex(code) AS code, "
    "if(isNull(code_or_null), NULL, hex(code_or_null)) AS code_or_null, "
    "toString(qty) AS qty, hex(label) AS label, toString(wide_u64) AS wide_u64 "
    "FROM oo_dl01w40.readings FINAL ORDER BY reading_id FORMAT JSONEachRow"
)
TYPES = (
    "SELECT name, type FROM system.columns "
    "WHERE database = 'oo_dl01w40' AND table = 'readings' "
    "ORDER BY position FORMAT JSONEachRow"
)


def run(args: list[str], timeout: int = 60) -> subprocess.CompletedProcess[str]:
    return subprocess.run(args, text=True, capture_output=True, timeout=timeout)


def ch(user: str, password_file: str, sql: str, timeout: int = 60) -> subprocess.CompletedProcess[str]:
    script = (
        "export CLICKHOUSE_PASSWORD=\"$(tr -d '[:space:]' < \"$1\")\"; "
        'clickhouse-client --user "$2" --query "$3"'
    )
    return run(
        ["docker", "exec", CH, "bash", "-lc", script, "bash", password_file, user, sql],
        timeout=timeout,
    )


def save(out: Path, name: str, proc: subprocess.CompletedProcess[str]) -> None:
    (out / f"{name}.stdout").write_text(proc.stdout)
    (out / f"{name}.stderr").write_text(proc.stderr)
    (out / f"{name}.exit").write_text(f"{proc.returncode}\n")


def reader_count() -> str:
    proc = ch(
        DBA,
        "/etc/oo/dba.password",
        "SELECT count() FROM system.query_log WHERE user = 'oo_dl01w1_40_reader'",
    )
    if proc.returncode != 0:
        raise SystemExit("reader query_log count failed")
    return proc.stdout.strip()


def needle_count(needle: str) -> str:
    proc = ch(
        DBA,
        "/etc/oo/dba.password",
        "SELECT count() FROM system.query_log "
        f"WHERE user = 'oo_dl01w1_40_reader' AND position(query, '{needle}') > 0",
    )
    if proc.returncode != 0:
        raise SystemExit(f"query_log needle count failed for {needle}")
    return proc.stdout.strip()


def lines(path: Path) -> list[dict]:
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.strip():
            rows.append(json.loads(line))
    return rows


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--decision", required=True, type=Path)
    parser.add_argument("--fixture", required=True, type=Path)
    parser.add_argument("--old-nine", required=True, type=Path)
    args = parser.parse_args()
    out = args.out
    out.mkdir(parents=True, exist_ok=True)
    decision = json.loads(args.decision.read_text(encoding="utf-8"))
    if decision.get("source_read") is not True or not decision.get("sql"):
        raise SystemExit("approved decision has no source read")
    deadline = time.time() + 180
    version = ""
    while time.time() < deadline:
        ping = ch(DBA, "/etc/oo/dba.password", "SELECT version()")
        if ping.returncode == 0 and ping.stdout.strip():
            version = ping.stdout.strip()
            (out / "clickhouse-version.txt").write_text(ping.stdout)
            break
        time.sleep(2)
    else:
        raise SystemExit("clickhouse did not answer")
    loaded = ch(DBA, "/etc/oo/dba.password", args.fixture.read_text(encoding="utf-8"), timeout=120)
    save(out, "fixture-load", loaded)
    if loaded.returncode != 0:
        raise SystemExit("fixture load failed")
    flush = ch(DBA, "/etc/oo/dba.password", "SYSTEM FLUSH LOGS")
    save(out, "flush-before-refusal", flush)
    before = reader_count()
    (out / "query-log-before-refusal.txt").write_text(before + "\n")
    flush = ch(DBA, "/etc/oo/dba.password", "SYSTEM FLUSH LOGS")
    save(out, "flush-after-refusal", flush)
    after = reader_count()
    (out / "query-log-after-refusal.txt").write_text(after + "\n")
    types = ch(READER, "/etc/oo/reader.password", TYPES)
    save(out, "columns", types)
    if types.returncode != 0:
        raise SystemExit("column types failed")
    approved = ch(READER, "/etc/oo/reader.password", decision["sql"])
    save(out, "approved-extract", approved)
    if approved.returncode != 0:
        raise SystemExit("approved extract failed")
    nanos = decision.get("nanos_sql") or []
    if len(nanos) != 2:
        raise SystemExit("approved decision has no nanos checks")
    decimal = ch(READER, "/etc/oo/reader.password", nanos[0])
    save(out, "nanos-decimal", decimal)
    roundtrip = ch(READER, "/etc/oo/reader.password", nanos[1])
    save(out, "nanos-roundtrip", roundtrip)
    if decimal.returncode != 0 or roundtrip.returncode != 0:
        raise SystemExit("nanos extract failed")
    nine = ch(READER, "/etc/oo/reader.password", NINE)
    save(out, "nine-column", nine)
    if nine.returncode != 0:
        raise SystemExit("nine-column extract failed")
    flush = ch(DBA, "/etc/oo/dba.password", "SYSTEM FLUSH LOGS")
    save(out, "flush-after-extract", flush)
    needles = {
        "labels": needle_count("labels"),
        "code_fixed": needle_count("code_fixed"),
        "ts_raw": needle_count("ts_raw"),
        "FixedString": needle_count("FixedString"),
        "oo_dl01w40.other": needle_count("oo_dl01w40.other"),
    }
    (out / "query-log-needles.json").write_text(json.dumps(needles, indent=2) + "\n")
    type_rows = lines(out / "columns.stdout")
    type_map = {row["name"]: row["type"] for row in type_rows}
    (out / "structural-types.json").write_text(
        json.dumps(
            {
                "columns": type_map,
                "fixed_string_present": any("FixedString" in value for value in type_map.values()),
                "ts": type_map.get("ts"),
                "amount": type_map.get("amount"),
                "reading_id": type_map.get("reading_id"),
                "wide_u64": type_map.get("wide_u64"),
                "note": type_map.get("note"),
                "qty": type_map.get("qty"),
                "tags": type_map.get("tags"),
                "labels": type_map.get("labels"),
            },
            indent=2,
        )
        + "\n"
    )
    left = lines(out / "nanos-decimal.stdout")
    right = lines(out / "nanos-roundtrip.stdout")
    max_digits = 0
    integer_equal = len(left) > 0 and len(left) == len(right)
    decimal_equal = integer_equal
    roundtrip_equal = integer_equal
    for one, two in zip(left, right):
        ts = str(one.get("nanos", ""))
        dec = str(one.get("nanos_decimal", ""))
        back = str(two.get("nanos_roundtrip", ""))
        again = str(two.get("nanos", ""))
        if not ts.isdigit() or not dec.isdigit() or not back.isdigit():
            integer_equal = False
        if ts != dec:
            decimal_equal = False
        if ts != again or ts != back:
            roundtrip_equal = False
        max_digits = max(max_digits, len(ts), len(dec), len(back))
    (out / "structural-nanos.json").write_text(
        json.dumps(
            {
                "rows": len(left),
                "scale": 0,
                "max_digits": max_digits,
                "max_digits_within_20": max_digits <= 20,
                "integer_equal": integer_equal,
                "decimal_equal": decimal_equal,
                "roundtrip_equal": roundtrip_equal,
                "representation": "nanoseconds_since_epoch_numeric_20_0",
            },
            indent=2,
        )
        + "\n"
    )
    new = (out / "nine-column.stdout").read_bytes()
    old = args.old_nine.read_bytes()
    (out / "nine-column-hash.json").write_text(
        json.dumps(
            {
                "equal": new == old,
                "new_sha256": hashlib.sha256(new).hexdigest(),
                "old_sha256": hashlib.sha256(old).hexdigest(),
                "new_bytes": len(new),
                "old_bytes": len(old),
                "approved_rows": len(lines(out / "approved-extract.stdout")),
            },
            indent=2,
        )
        + "\n"
    )
    (out / "refusal-window.json").write_text(
        json.dumps(
            {
                "reader_queries_before": before,
                "reader_queries_after": after,
                "delta": int(after) - int(before),
                "refused_sql_executed": False,
            },
            indent=2,
        )
        + "\n"
    )
    (out / "clickhouse-version.txt").write_text(version + "\n")
    problems = []
    if before != after:
        problems.append("refusal_window")
    if any(value != "0" for value in needles.values()):
        problems.append("needle")
    if not integer_equal or not decimal_equal or not roundtrip_equal or max_digits > 20 or not left:
        problems.append("nanos")
    if new != old:
        problems.append("nine_column")
    if type_map.get("ts") is None or "DateTime64(9" not in type_map["ts"]:
        problems.append("ts_type")
    if type_map.get("amount") is None or "Decimal(76" not in type_map["amount"]:
        problems.append("amount_type")
    if type_map.get("reading_id") != "UInt64" or type_map.get("wide_u64") != "UInt64":
        problems.append("uint64_type")
    if "Array" not in (type_map.get("tags") or "") or "Array" not in (type_map.get("labels") or ""):
        problems.append("array_type")
    if any("FixedString" in value for value in type_map.values()):
        problems.append("fixed_string_column")
    (out / "extract-problems.txt").write_text((" ".join(problems) + "\n") if problems else "")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
