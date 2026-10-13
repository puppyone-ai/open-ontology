"""Typed oracle helpers for the StarRocks preparation fixture.

Unit tests import this module. It does not open a database connection.
"""

from __future__ import annotations

import re
from decimal import Decimal, getcontext
from typing import Any

# StarRocks DECIMAL precision is 38. The default context (28) cannot quantize it.
getcontext().prec = 80


WRITE_KEYWORDS = {
    "INSERT",
    "UPDATE",
    "DELETE",
    "REPLACE",
    "CREATE",
    "ALTER",
    "DROP",
    "TRUNCATE",
    "GRANT",
    "REVOKE",
    "LOAD",
    "IMPORT",
    "ADMIN",
    "RESTORE",
    "CANCEL",
    "EXPORT",
}
READ_KEYWORDS = {
    "SELECT",
    "SHOW",
    "DESC",
    "DESCRIBE",
    "EXPLAIN",
    "WITH",
    "VALUES",
}
SESSION_KEYWORDS = {"SET", "USE"}
CONTROL_KEYWORDS = {"KILL"}


def grant_visible(text: str, needle: str) -> bool:
    """True when SHOW GRANTS text contains the privilege.

    StarRocks prints database privileges with underscores (CREATE_TABLE).
    The GRANT statement uses spaces (CREATE TABLE).
    """
    folded = text.upper().replace("_", " ")
    wanted = needle.upper().replace("_", " ")
    return wanted in folded


def decide_admission(global_rust: int, own_rust: int) -> str:
    """Return admit or a pending-capacity reason. This task starts no Rust builder."""
    if own_rust >= 1:
        return "pending-capacity-own-rust"
    if global_rust >= 10:
        return "pending-capacity-global-rust"
    return "admit"


def product_identity(
    current_version: str,
    version_comment: str,
    protocol_version: str,
    catalog_tables: list[str] | None = None,
) -> dict[str, Any]:
    """Accept StarRocks only.

    StarRocks 3.5 puts the build id in current_version() and @@version_comment.
    Those fields do not contain the word StarRocks. @@version is the MySQL wire
    compatibility string (8.0.33 on this pin) and is never the product version.
    A build id is accepted only together with information_schema tables that
    MySQL and Doris do not expose.
    """
    current = (current_version or "").strip()
    comment = (version_comment or "").strip()
    protocol = (protocol_version or "").strip()
    tables = {name.strip().lower() for name in (catalog_tables or []) if name.strip()}
    blob = f"{current}\n{comment}".lower()
    named = "starrocks" in blob
    release_shape = bool(re.fullmatch(r"3\.5\.21-[0-9a-f]+", current)) and comment == current
    catalog_ok = "be_tablets" in tables and "tables_config" in tables
    ok = True
    reason = "StarRocks marker present in current_version or version_comment"
    if "doris" in blob:
        ok = False
        reason = "Doris marker present; Doris is a different product"
    elif re.search(r"\bmysql\b", blob):
        ok = False
        reason = "MySQL marker in the product version text"
    elif named:
        reason = "StarRocks marker present in current_version or version_comment"
    elif release_shape and catalog_ok:
        reason = "pinned 3.5.21 build id plus information_schema.be_tablets and tables_config"
    else:
        ok = False
        reason = "version text does not identify StarRocks and StarRocks catalog tables are absent"
    if protocol and "starrocks" not in protocol.lower():
        protocol_note = "protocol_version is not a StarRocks product version"
    else:
        protocol_note = "protocol_version also contains a StarRocks marker"
    return {
        "ok": ok,
        "reason": reason,
        "protocol_note": protocol_note,
        "product_version": current,
        "version_comment": comment,
        "protocol_version": protocol,
        "catalog_tables": sorted(tables),
        "catalog_ok": catalog_ok,
    }


def quantize(value: Decimal, scale: int) -> Decimal:
    quantum = Decimal(1).scaleb(-scale)
    return value.quantize(quantum)


def norm_value(value: Any, column: dict[str, Any]) -> Any:
    if value is None:
        return None
    kind = column["type"]
    if kind == "int":
        return int(value)
    if kind == "decimal":
        return quantize(Decimal(str(value)), int(column["scale"]))
    if kind == "str":
        text = str(value)
        if column["name"] in {"bucket", "metric_day"} and len(text) >= 10 and text[4] == "-" and text[7] == "-":
            return text[:10]
        return text
    raise ValueError(f"unknown column type {kind}")


def norm_row(row: dict[str, Any], columns: list[dict[str, Any]]) -> tuple[Any, ...]:
    return tuple(norm_value(row[column["name"]], column) for column in columns)


def multiset(rows: list[dict[str, Any]], columns: list[dict[str, Any]]) -> list[tuple[Any, ...]]:
    return sorted(norm_row(row, columns) for row in rows)


def parse_encoded_row(line: str, columns: list[dict[str, Any]]) -> dict[str, Any]:
    parts = line.split("|")
    if len(parts) != len(columns):
        raise ValueError(f"expected {len(columns)} fields, got {len(parts)} in {line!r}")
    row: dict[str, Any] = {}
    for column, part in zip(columns, parts):
        if part == r"\N":
            row[column["name"]] = None
        elif column["type"] == "int":
            row[column["name"]] = int(part)
        else:
            row[column["name"]] = part
    return row


def rows_match(actual_lines: list[str], expected_rows: list[dict[str, Any]], columns: list[dict[str, Any]]) -> dict[str, Any]:
    actual_rows = [parse_encoded_row(line, columns) for line in actual_lines if line != ""]
    actual = multiset(actual_rows, columns)
    expected = multiset(expected_rows, columns)
    return {
        "ok": actual == expected,
        "actual": [list(item) for item in actual],
        "expected": [list(item) for item in expected],
    }


def partition_of(bucket: str) -> str:
    if bucket < "2026-02-01":
        return "p202601"
    if bucket < "2026-03-01":
        return "p202602"
    raise ValueError(f"bucket outside fixture partitions: {bucket}")


def partition_summary(rows: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    buckets: dict[str, dict[str, Any]] = {
        "p202601": {"row_count": 0, "amount_sum": Decimal("0")},
        "p202602": {"row_count": 0, "amount_sum": Decimal("0")},
    }
    for row in rows:
        part = partition_of(str(row["bucket"]))
        buckets[part]["row_count"] += 1
        if row["amount"] is not None:
            buckets[part]["amount_sum"] += Decimal(str(row["amount"]))
    return {
        name: {
            "row_count": info["row_count"],
            "amount_sum": format(quantize(info["amount_sum"], 4), "f"),
        }
        for name, info in buckets.items()
    }


def split_marked_results(text: str) -> dict[str, list[str]]:
    current = None
    out: dict[str, list[str]] = {}
    for raw in text.splitlines():
        line = raw.rstrip("\r")
        if line.startswith("@@result "):
            current = line[len("@@result ") :].strip()
            out[current] = []
            continue
        if current is None or line == "":
            continue
        out[current].append(line)
    return out


def statement_keyword(stmt: str) -> str:
    text = stmt.strip()
    text = re.sub(r"/\*.*?\*/", " ", text, flags=re.S)
    text = text.lstrip("( \t\r\n")
    match = re.match(r"([A-Za-z_]+)", text)
    if not match:
        return ""
    return match.group(1).upper()


def statement_class(stmt: str) -> str:
    word = statement_keyword(stmt)
    if word in WRITE_KEYWORDS:
        return "write"
    if word in READ_KEYWORDS:
        return "read"
    if word in SESSION_KEYWORDS:
        return "session"
    if word in CONTROL_KEYWORDS:
        return "control"
    if word == "":
        return "empty"
    return "other"


def extract_audit_user(line: str) -> str:
    match = re.search(r"(?:^|\|)User=([^|]*)", line)
    if not match:
        return ""
    return match.group(1).strip().strip("'")


def extract_audit_stmt(line: str) -> str:
    match = re.search(r"(?:^|\|)Stmt=(.*)$", line)
    if not match:
        return ""
    stmt = match.group(1).strip()
    # 3.5.21 keeps Stmt in the middle. |Digest= and later fields follow it.
    cut = re.split(r"\|[A-Za-z][A-Za-z0-9]*=", stmt, maxsplit=1)
    return cut[0].strip()


def reader_audit(lines: list[str], reader: str = "oo_reader") -> dict[str, Any]:
    classes: dict[str, int] = {}
    writes: list[str] = []
    controls: list[str] = []
    matched = 0
    for line in lines:
        user = extract_audit_user(line)
        short = user.split("@", 1)[0].strip("'\"")
        if short != reader:
            continue
        matched += 1
        stmt = extract_audit_stmt(line)
        kind = statement_class(stmt)
        classes[kind] = classes.get(kind, 0) + 1
        if kind == "write":
            writes.append(stmt[:240])
        if kind == "control":
            controls.append(stmt[:240])
    return {
        "matched": matched,
        "classes": classes,
        "write_statements": writes,
        "control_statements": controls,
        "read_window_ok": matched > 0 and not writes and classes.get("write", 0) == 0,
    }


def redact(text: str, secrets: list[str]) -> str:
    redacted = text
    for secret in secrets:
        if secret:
            redacted = redacted.replace(secret, "***")
    redacted = re.sub(
        r"(?i)(identified\s+by\s+)(['\"])(.*?)(\2)",
        r"\1\2***\4",
        redacted,
    )
    redacted = re.sub(
        r"(?i)(password\s*\(\s*)(['\"])(.*?)(\2)",
        r"\1\2***\4",
        redacted,
    )
    return redacted


def visible_after_append(initial: list[dict[str, Any]], append: dict[str, Any]) -> list[dict[str, Any]]:
    return list(initial) + [append]
