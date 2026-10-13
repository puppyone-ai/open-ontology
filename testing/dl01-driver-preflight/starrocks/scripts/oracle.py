#!/usr/bin/env python3
"""Independent typed oracle for the StarRocks reader gate.

The native input is mysql-client text from the same snapshot. The decoded
input is Arrow IPC decoded by starrocks-decode, which does not link mysql_async.
Neither side is accepted merely because its row count matches.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

TABLES = ("dup_events", "pk_accounts", "agg_metrics", "uniq_dim", "dup_partition_sums")
SCALES = {
    "amount": 4,
    "balance": 9,
    "revenue": 4,
    "score": 2,
    "amount_sum": 4,
}
COLUMNS = {
    "dup_events": ["event_id", "bucket", "seen", "amount", "note"],
    "pk_accounts": ["account_id", "balance", "nickname", "opened"],
    "agg_metrics": ["metric_day", "metric_name", "qty", "revenue"],
    "uniq_dim": ["dim_id", "label", "score"],
    "dup_partition_sums": ["part", "row_count", "amount_sum"],
}
INT_FIELDS = {"event_id", "account_id", "qty", "dim_id", "row_count"}
DATE_FIELDS = {"bucket", "metric_day"}
TIME_FIELDS = {"seen", "opened"}


def norm_decimal(value: object, scale: int) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    if text == "" or text == "\\N":
        return None
    negative = text.startswith("-")
    body = text[1:] if negative else text
    if body.lower().startswith("+"):
        raise ValueError(f"decimal sign {text}")
    if any(mark in body.lower() for mark in ("e", " ", ",")):
        raise ValueError(f"decimal form {text}")
    whole, dot, frac = body.partition(".")
    if dot and "." in frac:
        raise ValueError(f"decimal form {text}")
    if whole == "":
        whole = "0"
    if not whole.isdigit() or (frac and not frac.isdigit()):
        raise ValueError(f"decimal form {text}")
    if len(frac) > scale:
        raise ValueError(f"decimal scale {text} > {scale}")
    frac = frac.ljust(scale, "0")
    rendered = f"{whole}.{frac}" if scale else whole
    if rendered.startswith("0") and whole != "0":
        whole = str(int(whole))
        rendered = f"{whole}.{frac}" if scale else whole
    if negative and rendered.replace(".", "").strip("0") != "":
        rendered = f"-{rendered}"
    return rendered


def norm_time(value: object) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    if text in {"", "\\N", "NULL"}:
        return None
    text = text.replace("T", " ")
    if "." in text:
        head, frac = text.split(".", 1)
        if frac.strip("0") != "":
            raise ValueError(f"datetime fraction {value}")
        text = head
    if len(text) != 19 or text[10] != " ":
        raise ValueError(f"datetime {value}")
    return text


def norm_date(value: object) -> str:
    text = str(value).strip()
    if len(text) != 10 or text[4] != "-" or text[7] != "-":
        raise ValueError(f"date {value}")
    return text


def norm_row(table: str, row: dict) -> dict:
    out = {}
    for name in COLUMNS[table]:
        value = row.get(name)
        if name in INT_FIELDS:
            if value is None or value == "" or value == "\\N":
                raise ValueError(f"{table}.{name} null")
            out[name] = int(value)
        elif name in DATE_FIELDS:
            out[name] = norm_date(value)
        elif name in TIME_FIELDS:
            out[name] = norm_time(value)
        elif name in SCALES:
            out[name] = norm_decimal(value, SCALES[name])
        else:
            if value is None or value == "\\N":
                out[name] = None
            else:
                out[name] = str(value)
    return out


def parse_tsv(table: str, text: str) -> list[dict]:
    rows = []
    for line in text.splitlines():
        if line == "":
            continue
        parts = line.split("\t")
        if len(parts) != len(COLUMNS[table]):
            raise ValueError(f"{table} width {len(parts)}")
        raw = {}
        for name, part in zip(COLUMNS[table], parts):
            # mysql --batch --raw prints a SQL null as the word NULL, not \N.
            raw[name] = None if part in {"\\N", "NULL"} else part
        rows.append(norm_row(table, raw))
    return rows


def canon(rows: list[dict]) -> list[str]:
    return sorted(json.dumps(row, ensure_ascii=False, sort_keys=True) for row in rows)


def load_decoded(path: Path, table: str) -> tuple[dict, list[dict]]:
    document = json.loads(path.read_text(encoding="utf-8"))
    if document.get("protocol") != "oo.source.starrocks.ipc-decode/v1":
        raise ValueError(f"{table} decode protocol")
    meta = document.get("schema_metadata") or {}
    if meta.get("oo.source.protocol") != "oo.source.starrocks/v1":
        raise ValueError(f"{table} ipc protocol")
    if meta.get("oo.starrocks.table") != table:
        raise ValueError(f"{table} ipc table {meta.get('oo.starrocks.table')}")
    rows = []
    for item in document.get("rows") or []:
        rows.append(norm_row(table, item))
    return document, rows


def classify_pk(rows: list[dict], expected: dict) -> str:
    initial = canon([norm_row("pk_accounts", row) for row in expected["pk_accounts"]["rows"]])
    posted = canon([norm_row("pk_accounts", row) for row in expected["after"]["pk_if_update_and_delete_visible"]])
    got = canon(rows)
    if got == posted:
        return "update_and_delete_visible"
    if got == initial:
        return "pre_mutation_still_visible"
    edited = [dict(row) for row in (norm_row("pk_accounts", item) for item in expected["pk_accounts"]["rows"])]
    for row in edited:
        if row["account_id"] == 300:
            row["nickname"] = "edited"
    if got == canon(edited):
        return "update_only"
    deleted = [row for row in (norm_row("pk_accounts", item) for item in expected["pk_accounts"]["rows"]) if row["account_id"] != 302]
    if got == canon(deleted):
        return "delete_only"
    return "torn"


def compare_phase(expected: dict, decoded_dir: Path, native_dir: Path, phase: str) -> dict:
    problems = []
    compared = {}
    for table in TABLES:
        if phase == "after" and table == "pk_accounts":
            continue
        fixture_key = table
        fixture_rows = expected[fixture_key]["rows"]
        if phase == "after" and table == "dup_partition_sums":
            fixture_rows = expected["after"]["dup_partition_sums"]
        if phase == "after" and table == "dup_events":
            fixture_rows = list(expected["dup_events"]["rows"]) + list(expected["after"]["dup_append_visible"])
        want = canon([norm_row(table, row) for row in fixture_rows])
        native = canon(parse_tsv(table, (native_dir / f"{table}.tsv").read_text(encoding="utf-8")))
        _document, decoded_rows = load_decoded(decoded_dir / f"{table}.json", table)
        decoded = canon(decoded_rows)
        ok = want == native == decoded
        compared[table] = {"ok": ok, "rows": len(decoded_rows)}
        if not ok:
            problems.append(table)
        if phase == "initial" and table == "pk_accounts":
            hidden = canon([norm_row(table, row) for row in expected["pk_accounts"]["not_visible"]])
            if any(item in decoded or item in native for item in hidden):
                problems.append("pk_old_visible")
        if phase == "initial" and table == "uniq_dim":
            hidden = canon([norm_row(table, row) for row in expected["uniq_dim"]["not_visible"]])
            if any(item in decoded or item in native for item in hidden):
                problems.append("uniq_old_visible")
    pk_class = None
    if phase == "after":
        native_pk = parse_tsv("pk_accounts", (native_dir / "pk_accounts.tsv").read_text(encoding="utf-8"))
        _document, decoded_pk = load_decoded(decoded_dir / "pk_accounts.json", "pk_accounts")
        native_class = classify_pk(native_pk, expected)
        decoded_class = classify_pk(decoded_pk, expected)
        pk_class = native_class if native_class == decoded_class else "torn"
        compared["pk_accounts"] = {"ok": pk_class != "torn", "class": pk_class, "rows": len(decoded_pk)}
        if pk_class == "torn":
            problems.append("pk_accounts")
        native_dup = parse_tsv("dup_events", (native_dir / "dup_events.tsv").read_text(encoding="utf-8"))
        late = norm_row("dup_events", expected["after"]["dup_append_visible"][0])
        if late not in native_dup:
            problems.append("append_not_in_native")
    return {"phase": phase, "ok": not problems, "problems": problems, "tables": compared, "pk_class": pk_class}


def audit_counts(path: Path) -> dict:
    counts = {"rows": 0, "handshake": 0, "read": 0, "session": 0, "cancel": 0, "cleanup": 0, "write": 0, "unexpected": 0}
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        event = json.loads(line)
        counts["rows"] += 1
        kind = event.get("class")
        if kind in counts:
            counts[kind] += 1
        else:
            counts["unexpected"] += 1
        sql = event.get("sql") or ""
        upper = sql.upper()
        if any(word in upper for word in ("IDENTIFIED BY", "PASSWORD", "SECRET")):
            counts["unexpected"] += 1
    counts["ok"] = counts["write"] == 0 and counts["unexpected"] == 0 and counts["handshake"] > 0 and counts["cleanup"] > 0
    return counts


def self_test() -> None:
    assert norm_decimal("4.5", 4) == "4.5000"
    assert norm_decimal("-0.0003", 4) == "-0.0003"
    null_pk = parse_tsv("pk_accounts", "301\tNULL\tNULL\tNULL\n")
    assert null_pk[0]["balance"] is None and null_pk[0]["nickname"] is None and null_pk[0]["opened"] is None
    null_event = parse_tsv("dup_events", "22\t2026-05-19\tNULL\tNULL\t\n")
    assert null_event[0]["seen"] is None and null_event[0]["amount"] is None and null_event[0]["note"] == ""
    assert norm_decimal("77777777777777.7774", 4) == "77777777777777.7774"
    assert norm_time("2026-05-06T01:02:03") == "2026-05-06 01:02:03"
    expected = json.loads(Path(__file__).resolve().parents[1].joinpath("fixtures/expected.json").read_text(encoding="utf-8"))
    initial = [norm_row("pk_accounts", row) for row in expected["pk_accounts"]["rows"]]
    assert classify_pk(initial, expected) == "pre_mutation_still_visible"
    posted = [norm_row("pk_accounts", row) for row in expected["after"]["pk_if_update_and_delete_visible"]]
    assert classify_pk(posted, expected) == "update_and_delete_visible"
    update_only = [dict(row) for row in initial]
    for row in update_only:
        if row["account_id"] == 300:
            row["nickname"] = "edited"
    assert classify_pk(update_only, expected) == "update_only"
    delete_only = [row for row in initial if row["account_id"] != 302]
    assert classify_pk(delete_only, expected) == "delete_only"
    garbage = dict(posted[0])
    garbage["nickname"] = "torn"
    assert classify_pk([garbage], expected) == "torn"
    print("oracle self-test ok")


def main() -> int:
    if len(sys.argv) == 2 and sys.argv[1] == "--self-test":
        self_test()
        return 0
    if len(sys.argv) != 6 or sys.argv[1] != "compare":
        print("usage: oracle.py --self-test | compare EXPECTED DECODED_DIR NATIVE_DIR PHASE", file=sys.stderr)
        return 2
    expected = json.loads(Path(sys.argv[2]).read_text(encoding="utf-8"))
    report = compare_phase(expected, Path(sys.argv[3]), Path(sys.argv[4]), sys.argv[5])
    json.dump(report, sys.stdout, indent=2)
    sys.stdout.write("\n")
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
