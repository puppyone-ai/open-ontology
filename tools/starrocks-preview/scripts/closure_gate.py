"""Pure checks for the sr_w45 closure.

The customer file and the binding registry decide which objects are read.
A secret is removed only when it stands alone. A secret that sits inside a
longer decimal coefficient stays, so the typed cell can be compared.
"""

from __future__ import annotations

import json
import re
from pathlib import Path

CUSTOMER_PROTOCOL = "oo.customer.source-input/v1"
WRITE_VERB = re.compile(
    r"^(INSERT|UPDATE|DELETE|REPLACE|CREATE|ALTER|DROP|TRUNCATE|GRANT|REVOKE|LOAD|IMPORT|ADMIN|RESTORE|EXPORT|OPERATE)\b",
    re.IGNORECASE,
)
FE_WRITE = re.compile(
    r"(?:^|[|\s]Stmt=)(INSERT|UPDATE|DELETE|REPLACE|CREATE|ALTER|DROP|TRUNCATE|GRANT|REVOKE|LOAD|IMPORT|ADMIN|RESTORE|EXPORT|OPERATE)\b",
    re.IGNORECASE,
)


def _token_byte(value: int) -> bool:
    return (48 <= value <= 57) or (65 <= value <= 90) or (97 <= value <= 122) or value in (95, 45)


def scrub_bytes(data: bytes, secrets: list[bytes]) -> bytes:
    for secret in secrets:
        if not secret:
            continue
        out = bytearray()
        start = 0
        while True:
            index = data.find(secret, start)
            if index < 0:
                out.extend(data[start:])
                break
            before = index > 0 and _token_byte(data[index - 1])
            after_at = index + len(secret)
            after = after_at < len(data) and _token_byte(data[after_at])
            if before or after:
                out.extend(data[start:after_at])
                start = after_at
                continue
            out.extend(data[start:index])
            out.extend(b"[redacted]")
            start = after_at
        data = bytes(out)
    return data


def scrub_text(text: str, secrets: list[str]) -> str:
    raw = text.encode("utf-8")
    cleaned = scrub_bytes(raw, [item.encode("utf-8") for item in secrets if item])
    return cleaned.decode("utf-8", "replace")


def bounded_secret_hits(text: str, secrets: list[str]) -> int:
    raw = text.encode("utf-8")
    hits = 0
    for secret in secrets:
        needle = secret.encode("utf-8")
        if not needle:
            continue
        start = 0
        while True:
            index = raw.find(needle, start)
            if index < 0:
                break
            before = index > 0 and _token_byte(raw[index - 1])
            after_at = index + len(needle)
            after = after_at < len(raw) and _token_byte(raw[after_at])
            if not before and not after:
                hits += 1
            start = after_at
    return hits


def decimal_cell(text: str) -> dict:
    negative = text.startswith("-")
    body = text[1:] if negative else text
    if "." in body:
        whole, frac = body.split(".", 1)
        scale = len(frac)
        if whole.lstrip("0") == "":
            coefficient = frac.lstrip("0") or "0"
        else:
            coefficient = whole.lstrip("0") + frac
    else:
        scale = 0
        coefficient = body.lstrip("0") or "0"
    if coefficient == "0":
        negative = False
    return {"kind": "numeric", "negative": negative, "coefficient": coefficient, "scale": scale}


def expect_cell(field: dict, value: object) -> dict:
    if value is None:
        return {"kind": "null"}
    kind = field["type"]
    if kind == "int64":
        return {"kind": "int8", "value": str(int(value))}
    if kind == "utf8":
        return {"kind": "varchar", "value": value}
    if kind.startswith("decimal"):
        return decimal_cell(str(value))
    if kind == "date32" or kind.startswith("date"):
        return {"kind": "date", "value": value}
    if kind.startswith("timestamp"):
        date, clock = str(value).split(" ")
        return {"kind": "timestamp", "value": f"{date}T{clock}.000000"}
    return {"kind": "unmapped", "source_type": kind, "value": value}


def _temporal(fields: list, expected: list, got: list) -> list:
    columns = []
    width = min(len(expected), len(got))
    for index, field in enumerate(fields):
        kind = field["type"]
        if not (kind == "date32" or str(kind).startswith("date") or str(kind).startswith("timestamp")):
            continue
        consumer_kind = "date" if str(kind).startswith("date") else "timestamp"
        rows = []
        equal = len(expected) == len(got)
        for row_index in range(width):
            left = expected[row_index][index] if index < len(expected[row_index]) else None
            right = got[row_index][index] if index < len(got[row_index]) else None
            if isinstance(left, dict) and left.get("kind") == "null":
                match = left == right
            else:
                match = left == right and isinstance(right, dict) and right.get("kind") == consumer_kind
            equal = equal and match
            rows.append({"expected": left, "got": right, "match": match})
        columns.append({
            "column": field["name"],
            "source_type": kind,
            "consumer_kind": consumer_kind,
            "rows": rows,
            "equal": equal,
        })
    return columns


def compare_rows(name: str, decoded_rows: list, fields: list, got_rows: list) -> dict:
    expected = []
    for row in decoded_rows:
        expected.append([expect_cell(field, row.get(field["name"])) for field in fields])
    mismatches = []
    if len(expected) != len(got_rows):
        mismatches.append({"kind": "length", "expected": len(expected), "got": len(got_rows)})
    else:
        left = sorted(json.dumps(row, sort_keys=True) for row in expected)
        right = sorted(json.dumps(row, sort_keys=True) for row in got_rows)
        for index, (raw_left, raw_right) in enumerate(zip(left, right)):
            if raw_left != raw_right:
                mismatches.append({
                    "kind": "row",
                    "index": index,
                    "expected": json.loads(raw_left),
                    "got": json.loads(raw_right),
                })
    temporal = _temporal(fields, expected, got_rows)
    return {
        "table": name,
        "expected_rows": len(expected),
        "got_rows": len(got_rows),
        "equal": not mismatches,
        "cell_mismatches": mismatches,
        "expected": expected,
        "got": got_rows,
        "temporal": temporal,
        "temporal_equal": all(column["equal"] for column in temporal),
    }


def admit_customer(request: dict, registry: dict) -> dict:
    errors = []
    if request.get("protocol") != CUSTOMER_PROTOCOL:
        errors.append("protocol")
    secret_ref = request.get("secret_ref")
    if not isinstance(secret_ref, str) or not secret_ref.startswith("secret://") or secret_ref == "secret://":
        errors.append("secret_ref")
    source_id = request.get("source_id")
    if not isinstance(source_id, str) or not source_id:
        errors.append("source_id")
    budget = request.get("budget") if isinstance(request.get("budget"), dict) else {}
    hard = budget.get("hard_limit")
    if budget.get("unit") != "Rows" or not isinstance(hard, int) or isinstance(hard, bool) or hard < 1:
        errors.append("budget")
        hard = 0
    index: dict[tuple, tuple] = {}
    for binding in registry.get("bindings") or []:
        if binding.get("source_id") != source_id or binding.get("source_engine") != "starrocks":
            continue
        approved_fields = [field.get("name") for field in binding.get("fields") or []]
        for obj in binding.get("objects") or []:
            index[(obj.get("logical_schema"), obj.get("logical_table"))] = (approved_fields, obj, binding.get("capture_receipt_sha256"))
    refused = []
    admitted = []
    seen = set()
    for item in request.get("objects") or []:
        schema = item.get("schema")
        name = item.get("name")
        found = index.get((schema, name))
        if found is None:
            refused.append({"schema": schema, "name": name, "reason": "not_approved", "read_attempted": False})
            continue
        approved_fields, obj, receipt = found
        requested = item.get("fields")
        if requested is not None:
            missing = [field for field in requested if field not in approved_fields]
            if missing:
                refused.append({
                    "schema": schema,
                    "name": name,
                    "reason": "field_not_approved",
                    "fields": missing,
                    "read_attempted": False,
                })
                continue
        row_count = obj.get("row_count")
        if not isinstance(row_count, int) or isinstance(row_count, bool) or row_count < 1 or row_count > hard:
            refused.append({"schema": schema, "name": name, "reason": "budget", "read_attempted": False})
            continue
        if name in seen:
            continue
        seen.add(name)
        admitted.append({
            "schema": schema,
            "name": name,
            "row_count": row_count,
            "arrow_sha256": obj.get("arrow_sha256"),
            "fields": approved_fields,
            "capture_receipt_sha256": receipt,
        })
    return {
        "ok": not errors and bool(admitted),
        "errors": errors,
        "secret_ref": secret_ref if "secret_ref" not in errors else None,
        "admitted": admitted,
        "refused": refused,
        "read_attempted_names": [],
    }


def resolve_secret(secret_ref: str, mapping: dict, exists, mode_of) -> dict:
    path = mapping.get(secret_ref) if isinstance(mapping, dict) else None
    if not isinstance(path, str) or not path.startswith("/"):
        return {"ok": False, "reason": "secret_ref_unknown", "read_attempted": False}
    if not exists(path):
        return {"ok": False, "reason": "secret_file_absent", "read_attempted": False}
    mode = mode_of(path) & 0o777
    if mode & 0o077:
        return {"ok": False, "reason": "secret_file_mode", "read_attempted": False}
    return {"ok": True, "path": path, "read_attempted": False}


def audit_events(events: list) -> dict:
    counts = {
        "rows": 0,
        "handshake": 0,
        "read": 0,
        "session": 0,
        "cancel": 0,
        "cleanup": 0,
        "write": 0,
        "unexpected": 0,
        "failed_write_attempts": 0,
    }
    for event in events:
        if not isinstance(event, dict):
            counts["unexpected"] += 1
            counts["rows"] += 1
            continue
        counts["rows"] += 1
        kind = event.get("class")
        sql = event.get("sql") or ""
        verb = bool(WRITE_VERB.match(str(sql).strip()))
        failed = event.get("ok") is False or bool(event.get("error"))
        if kind == "write" or verb:
            counts["write"] += 1
            if failed or kind != "write":
                counts["failed_write_attempts"] += 1
            continue
        if kind in ("handshake", "read", "session", "cancel", "cleanup"):
            counts[kind] += 1
        else:
            counts["unexpected"] += 1
    counts["write_attempts"] = counts["write"]
    counts["ok"] = (
        counts["write_attempts"] == 0
        and counts["failed_write_attempts"] == 0
        and counts["unexpected"] == 0
        and counts["handshake"] > 0
        and counts["cleanup"] > 0
    )
    return counts


def load_jsonl(path: Path) -> list:
    events = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.strip():
            events.append(json.loads(line))
    return events


READER_USER = re.compile(r"(?:^|\|)User=oo_w45_reader(?:\||$)")


def fe_write_attempts(text: str) -> int:
    # A GRANT issued by root names the reader in the statement text. That line
    # is the DBA window. Only statements whose audit user is the product reader
    # count as product write attempts.
    return sum(
        1
        for line in text.splitlines()
        if READER_USER.search(line) and FE_WRITE.search(line)
    )


def pack_identity(pack: dict, query: dict) -> dict:
    imported = query.get("import") if isinstance(query.get("import"), dict) else {}
    return {
        "pack_digest": pack.get("pack_digest"),
        "model": pack.get("model"),
        "model_input_sha256": pack.get("model_input_sha256"),
        "identity_hash": pack.get("identity_hash"),
        "source_id": pack.get("source_id"),
        "source_engine": pack.get("source_engine"),
        "query_import_hash": imported.get("import_hash"),
        "query_source_id": imported.get("source_id"),
        "query_protocol": query.get("protocol"),
        "type_path": query.get("type_path"),
        "returned_rows": query.get("returned_rows"),
    }


def same_pack(typed: dict, other: dict) -> bool:
    keys = (
        "pack_digest",
        "model_input_sha256",
        "identity_hash",
        "query_import_hash",
        "query_source_id",
        "source_id",
        "source_engine",
    )
    if typed.get("source_id") != "sr_w45" or typed.get("source_engine") != "starrocks":
        return False
    if typed.get("query_protocol") != "oo.operator-preview.result/v1":
        return False
    if typed.get("type_path") != "native_contract":
        return False
    if not typed.get("identity_hash") or typed.get("identity_hash") != typed.get("query_import_hash"):
        return False
    return all(typed.get(key) and typed.get(key) == other.get(key) for key in keys)
