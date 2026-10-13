#!/usr/bin/env python3
"""Reconcile FE audit rows with the reader client log.

A green FE-only count is not coverage. ready is true only when every
oo_reader client statement is present in the FE audit and none of those FE
rows record a forbidden successful write. Subprocess exit 0 is not an input.
"""

import json
import os
import sys
from collections import Counter
from pathlib import Path

ALLOWED = {"SELECT", "SHOW", "SET", "KILL", "USE"}
FORBIDDEN = {"INSERT", "UPDATE", "DELETE", "CREATE", "DROP", "ALTER", "GRANT"}
OK_STATES = {"OK", "EOF"}


def kind_of(sql):
    text = sql.lstrip()
    if text.startswith("/*"):
        end = text.find("*/")
        if end != -1:
            text = text[end + 2 :].lstrip()
    head = text.split(None, 1)[0].upper() if text else ""
    return head


def norm(sql):
    return " ".join(sql.split())


def parse_fe(text):
    rows = []
    for line in text.splitlines():
        if "AuditLogger" not in line or "User=" not in line:
            continue
        fields = {}
        for part in line.split("|"):
            if "=" in part:
                key, value = part.split("=", 1)
                fields[key.strip()] = value.strip()
        rows.append(fields)
    return rows


def main():
    if len(sys.argv) != 4:
        print(
            "usage: judge.py <redacted-fe-audit> <client-audit.jsonl> <out-json>",
            file=sys.stderr,
        )
        return 2
    fe_rows = parse_fe(Path(sys.argv[1]).read_text(encoding="utf-8", errors="replace"))
    client_rows = []
    for line in Path(sys.argv[2]).read_text(encoding="utf-8", errors="replace").splitlines():
        if not line.strip():
            continue
        client_rows.append(json.loads(line))
    reader_bad = []
    reader_rows = 0
    dba_rows = 0
    fe_reader_sql = Counter()
    for fields in fe_rows:
        user = fields.get("User", "")
        state = fields.get("State", "")
        kind = fields.get("StmtType", "").upper()
        if user == "oo_reader":
            reader_rows += 1
            fe_reader_sql[norm(fields.get("Stmt", ""))] += 1
            if state in OK_STATES and kind not in ALLOWED:
                reader_bad.append({"state": state, "type": kind})
            if state in OK_STATES and kind in FORBIDDEN:
                reader_bad.append({"state": state, "type": kind, "stmt": "forbidden success"})
        elif user in {"root", "oo_dba"}:
            dba_rows += 1
    phases = Counter(row.get("phase", "") for row in client_rows)
    uncovered = []
    covered = 0
    client_statements = [row for row in client_rows if row.get("phase") == "statement" and row.get("user") == "oo_reader"]
    product = os.environ.get("OO_DORIS_PRODUCT_AUDIT") == "1"
    forbidden_attempts = []
    if product:
        for row in client_statements:
            kind = kind_of(row.get("sql", ""))
            if kind in FORBIDDEN:
                forbidden_attempts.append({"source": "client", "kind": kind, "sql": norm(row.get("sql", ""))[:180]})
        for fields in fe_rows:
            if fields.get("User") != "oo_reader":
                continue
            kind = fields.get("StmtType", "").upper()
            if kind in FORBIDDEN:
                forbidden_attempts.append({
                    "source": "fe",
                    "kind": kind,
                    "state": fields.get("State", ""),
                    "sql": norm(fields.get("Stmt", ""))[:180],
                })
        reader_bad.extend(forbidden_attempts)
    remaining = Counter(fe_reader_sql)
    for row in client_statements:
        key = norm(row.get("sql", ""))
        if remaining[key] > 0:
            remaining[key] -= 1
            covered += 1
        else:
            uncovered.append({"kind": kind_of(row.get("sql", "")), "sql": key[:180]})
    # Doris writes the denial text into Stmt for some privilege failures.
    # Match only a single unused ERR row of the same StmtType whose Stmt is
    # that error text. A missing row stays uncovered.
    substituted = []
    still_uncovered = []
    for row in uncovered:
        candidates = []
        for fields in fe_rows:
            if fields.get("_used"):
                continue
            if fields.get("User") != "oo_reader":
                continue
            if fields.get("StmtType", "").upper() != row["kind"]:
                continue
            if fields.get("State") != "ERR":
                continue
            stmt = fields.get("Stmt", "")
            if not stmt or stmt != fields.get("ErrorMessage", ""):
                continue
            candidates.append(fields)
        if len(candidates) != 1:
            still_uncovered.append(row)
            continue
        chosen = candidates[0]
        chosen["_used"] = True
        substituted.append({
            "kind": row["kind"],
            "sql": row["sql"],
            "fe_state": "ERR",
            "fe_stmt": chosen.get("Stmt", "")[:300],
        })
        covered += 1
    uncovered = still_uncovered
    coverage = len(client_statements) > 0 and not uncovered and not reader_bad
    report = {
        "protocol": "oo.doris.audit-reconcile/v1",
        "client_rows": len(client_rows),
        "client_phases": dict(phases),
        "client_statements": len(client_statements),
        "fe_reader_rows": reader_rows,
        "fe_rows_total": len(fe_rows),
        "dba_rows": dba_rows,
        "covered_statements": covered,
        "exact_sql_matches": covered - len(substituted),
        "error_substituted_statements": substituted,
        "uncovered_statements": len(uncovered),
        "uncovered_sample": uncovered[:12],
        "product_reader": product,
        "forbidden_attempts": forbidden_attempts,
        "reader_violations": reader_bad,
        "zero_write_reader": reader_rows > 0 and not reader_bad,
        "statement_coverage": coverage,
        "ready": coverage,
        "source_acceptance": False,
        "subprocess_exit_is_not_source_gate": True,
    }
    Path(sys.argv[3]).write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({
        "client_rows": len(client_rows),
        "client_statements": len(client_statements),
        "fe_reader_rows": reader_rows,
        "uncovered": len(uncovered),
        "ready": coverage,
    }))
    return 0 if coverage else 1


if __name__ == "__main__":
    sys.exit(main())
