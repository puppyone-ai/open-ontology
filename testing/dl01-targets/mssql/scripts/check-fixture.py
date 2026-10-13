#!/usr/bin/env python3
"""Static checks for the SQL Server fixture. This does not connect to SQL Server.

The classifier sample at the bottom is a labeled mock of the audit rule only.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
WRITE_RE = re.compile(
    r"\b(INSERT|UPDATE|DELETE|MERGE|CREATE|ALTER|DROP|GRANT|REVOKE|EXEC|EXECUTE|TRUNCATE|BACKUP|RESTORE|DBCC|WAITFOR)\b",
    re.IGNORECASE,
)
PASSWORD_RE = re.compile(r"PASSWORD\s*=\s*N'([^']*)'", re.IGNORECASE)


def strip_comments(sql: str) -> str:
    sql = re.sub(r"/\*.*?\*/", " ", sql, flags=re.DOTALL)
    return "\n".join(line.split("--", 1)[0] for line in sql.splitlines())


def classify_batch(text: str, error: int | None) -> str:
    """Label a reader batch. Successful writes are the zero-write failure."""
    head = strip_comments(text).strip().upper()
    write = bool(WRITE_RE.search(head)) and not head.startswith("WAITFOR") and not head.startswith("SELECT")
    # CREATE/INSERT/UPDATE/DELETE/ALTER/DROP still match WRITE_RE when they lead.
    if re.match(r"\s*(INSERT|UPDATE|DELETE|MERGE|CREATE|ALTER|DROP|TRUNCATE)\b", head, re.IGNORECASE):
        write = True
    if write and (error is None or error == 0):
        return "successful_write"
    if write:
        return "denied_write"
    if head.startswith("WAITFOR") or head.startswith("SELECT") or head.startswith("USE"):
        return "read_or_wait"
    return "other"


def mock_classifier() -> None:
    """Labeled mock. These rows are not from a SQL Server audit."""
    rows = [
        ("SELECT id FROM dbo.oracle_row", 0, "read_or_wait"),
        ("WAITFOR DELAY '00:00:45'", 0, "read_or_wait"),
        ("INSERT INTO dbo.oracle_row (id, bin_key) VALUES (9, 0x01020304)", 229, "denied_write"),
        ("CREATE TABLE dbo.reader_must_not_create (id int NOT NULL)", 262, "denied_write"),
        ("DELETE FROM dbo.oracle_row WHERE id = 1", 0, "successful_write"),
    ]
    for text, error, want in rows:
        got = classify_batch(text, error)
        if got != want:
            raise SystemExit(f"classifier mock failed: {got} != {want} for {text}")
    denied = [classify_batch(t, e) for t, e, _ in rows]
    if "successful_write" not in denied:
        raise SystemExit("classifier mock did not detect a successful write")
    live_ok = [classify_batch(t, e) for t, e, kind in rows if kind != "successful_write"]
    if any(item == "successful_write" for item in live_ok):
        raise SystemExit("classifier mock marked a denied or read batch as a successful write")


def main() -> int:
    pin = json.loads((ROOT / "PIN.json").read_text(encoding="utf-8"))
    selected = pin["selected"]
    if selected["architecture"] != "amd64" or selected["os"] != "linux":
        raise SystemExit("PIN architecture is not linux/amd64")
    if selected["arm64_manifest"] is not False:
        raise SystemExit("PIN claims an arm64 manifest")
    if not selected["manifest_digest"].startswith("sha256:"):
        raise SystemExit("PIN digest missing")
    if pin["driver"]["compiled_in_this_task"] is not False:
        raise SystemExit("driver pin claims a compile")
    if pin["eula"]["accepted_by_this_session"] is not False:
        raise SystemExit("EULA pin claims acceptance")

    oracle = json.loads((ROOT / "expected" / "oracle.json").read_text(encoding="utf-8"))
    if oracle["executed"] is not False or oracle["verdict"] != "not_run":
        raise SystemExit("oracle json claims a live result")
    read_sql = (ROOT / "reader" / "01-oracle-read.sql").read_text(encoding="utf-8")
    for key in oracle["cases"]:
        if key not in read_sql:
            raise SystemExit(f"oracle key {key} missing from reader SQL")
    if oracle["negative_writes"]["executed"] is not False or oracle["cancel"]["executed"] is not False:
        raise SystemExit("negative or cancel claims execution")
    if oracle["zero_write_audit"]["executed"] is not False:
        raise SystemExit("audit claims execution")

    checklist = (ROOT / "dba" / "00-production-checklist.sql").read_text(encoding="utf-8")
    for line in checklist.splitlines():
        if line.strip() and not line.lstrip().startswith("--"):
            raise SystemExit("production checklist has an executable line")

    def leading_keywords(sql: str) -> list[str]:
        body = strip_comments(sql)
        found = []
        for part in re.split(r"\bGO\b|;", body, flags=re.IGNORECASE):
            tokens = part.split()
            if tokens:
                found.append(tokens[0].upper())
        return found

    forbidden = {"INSERT", "UPDATE", "DELETE", "MERGE", "CREATE", "ALTER", "DROP", "GRANT", "REVOKE", "EXEC", "EXECUTE", "TRUNCATE", "BACKUP", "RESTORE", "DBCC", "WAITFOR"}
    bad = [word for word in leading_keywords(read_sql) if word in forbidden]
    if bad:
        raise SystemExit(f"reader oracle SQL starts a write statement: {bad}")
    observe_sql = (ROOT / "dba" / "05-observe-xe.sql").read_text(encoding="utf-8")
    bad = [word for word in leading_keywords(observe_sql) if word in forbidden]
    if bad:
        raise SystemExit(f"XE observe SQL starts a write statement: {bad}")

    negative = (ROOT / "reader" / "02-negative-writes.sql").read_text(encoding="utf-8")
    for word in ("INSERT", "UPDATE", "DELETE", "CREATE TABLE"):
        if word not in negative.upper():
            raise SystemExit(f"negative SQL missing {word}")
    cancel = strip_comments((ROOT / "reader" / "03-cancel.sql").read_text(encoding="utf-8"))
    if "WAITFOR" not in cancel.upper():
        raise SystemExit("cancel SQL missing WAITFOR")
    if re.search(r"\b(INSERT|UPDATE|DELETE|CREATE|ALTER|DROP|GRANT)\b", cancel, re.IGNORECASE):
        raise SystemExit("cancel SQL contains a write")

    for path in ROOT.rglob("*"):
        if not path.is_file():
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        for match in PASSWORD_RE.finditer(text):
            token = match.group(1)
            if token not in {"__DBA_PASSWORD__", "__READER_PASSWORD__"}:
                raise SystemExit(f"unexpected password literal in {path}")
        for line in text.splitlines():
            if re.search(r"MSSQL_SA_PASSWORD=(?!%s)[A-Za-z0-9!#%+@_-]{8,}", line):
                raise SystemExit(f"SA password literal in {path}")

    mock_classifier()

    gate = subprocess.run(
        ["sh", str(ROOT / "scripts" / "host-gate.sh")],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    sys.stdout.write(gate.stdout)
    sys.stderr.write(gate.stderr)
    print(f"host_gate_exit={gate.returncode}")
    import os

    machine = os.uname().machine
    if machine != "x86_64" and gate.returncode != 42:
        raise SystemExit(f"host {machine} should refuse with exit 42, got {gate.returncode}")
    if machine == "x86_64" and gate.returncode not in {0, 42}:
        raise SystemExit(f"unexpected host gate exit {gate.returncode}")
    print("check_fixture=ok")
    print("classifier_mock=labeled_not_a_server_audit")
    print("sql_server_journey=not_run")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except SystemExit:
        raise
    except Exception as exc:
        print(f"check_fixture_error={exc}", file=sys.stderr)
        raise SystemExit(1)
