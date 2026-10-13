#!/usr/bin/env python3
"""Separate setup, Doris identity checks, and the reader statement journey.

The reader audit is the product journey. A root GRANT in the FE log is DBA
setup. SHOW GRANTS is a read of the grant table. Doris banner rejection is a
probe unit test, not a statement sent to StarRocks.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

VERBS = ("DROP", "ALTER", "GRANT")


def verb(sql: str) -> str:
    words = sql.strip().split()
    return words[0].upper() if words else ""


def classify_jsonl(path: Path) -> dict:
    attempts = []
    show_grants = 0
    statements = 0
    if not path.is_file():
        return {"path": str(path), "present": False, "statements": 0, "ddl_attempts": []}
    for line in path.read_text().splitlines():
        if not line.strip():
            continue
        row = json.loads(line)
        sql = str(row.get("sql") or "")
        statements += 1
        first = verb(sql)
        if first == "SHOW" and sql.upper().startswith("SHOW GRANTS"):
            show_grants += 1
        if first in VERBS:
            attempts.append({"seq": row.get("seq"), "role": row.get("role"), "sql": sql})
    return {
        "path": str(path),
        "present": True,
        "statements": statements,
        "show_grants": show_grants,
        "ddl_attempts": attempts,
    }


def classify_fe(path: Path) -> dict:
    setup_grants = []
    reader_ddl = []
    if not path.is_file():
        return {"path": str(path), "present": False}
    for line in path.read_text().splitlines():
        if "Stmt=" not in line:
            continue
        user = ""
        if "User=" in line:
            user = line.split("User=", 1)[1].split("|", 1)[0]
        stmt = line.split("Stmt=", 1)[1].split("|", 1)[0]
        first = verb(stmt)
        if user == "root" and first == "GRANT":
            setup_grants.append(stmt)
        elif user != "root" and first in VERBS:
            reader_ddl.append({"user": user, "sql": stmt})
    return {
        "path": str(path),
        "present": True,
        "root_setup_grant_statements": setup_grants,
        "non_root_drop_alter_grant": reader_ddl,
    }


def main() -> int:
    root = Path(sys.argv[1])
    out = Path(sys.argv[2])
    reader = classify_jsonl(root / "native/read/audit.jsonl")
    fe = classify_fe(root / "native/fe-audit-reader.log")
    body = {
        "protocol": "oo.starrocks.reader-journey-audit/v1",
        "source_id": "sr_w45",
        "reader_audit": reader,
        "fe_log_separated_from_reader": fe,
        "doris_identity_check": {
            "kind": "probe_unit_test",
            "name": "identity::tests::doris_and_mysql_banners_are_rejected",
            "executed_against_starrocks": False,
            "note": "The probe rejects a Doris banner before a reader session. It is not a DDL statement on the StarRocks reader journey.",
        },
        "reader_ddl_attempt_count": len(reader["ddl_attempts"]) + len(fe.get("non_root_drop_alter_grant") or []),
    }
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(body, indent=2) + "\n")
    if body["reader_ddl_attempt_count"] != 0:
        raise SystemExit("reader journey contains a DROP, ALTER, or GRANT attempt")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
