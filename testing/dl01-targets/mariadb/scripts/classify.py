#!/usr/bin/env python3
"""Classify MariaDB general_log rows for the reader account.

Unit mode (--self-test) does not open a database. Live mode reads a TSV of
command_type and statement text exported by audit.sh.
"""

import re
import sys

WRITE = re.compile(
    r"\b(INSERT|UPDATE|DELETE|REPLACE|ALTER|CREATE|DROP|TRUNCATE|GRANT|REVOKE|"
    r"RENAME|LOAD|CALL|HANDLER|OPTIMIZE|REPAIR|INSTALL|UNINSTALL|FLUSH|KILL|"
    r"RESET|PURGE|BINLOG|INTO\s+OUTFILE|INTO\s+DUMPFILE)\b",
    re.IGNORECASE,
)
SET_OK = re.compile(
    r"^SET\s+(SESSION\s+)?(NAMES\b|time_zone\b)",
    re.IGNORECASE,
)
READ = {"SELECT", "SHOW", "WITH", "EXPLAIN", "DESCRIBE", "DESC", "USE"}
ALLOWED_COMMAND = {"Connect", "Quit", "Init DB"}


def strip_comments(sql: str) -> str:
    sql = re.sub(r"/\*.*?\*/", " ", sql, flags=re.DOTALL)
    sql = re.sub(r"--[^\n]*", " ", sql)
    return sql.strip()


def classify(command: str, argument: str) -> str:
    if command in ALLOWED_COMMAND:
        return "connect"
    if command != "Query":
        return "unexpected"
    sql = strip_comments(argument)
    if not sql:
        return "empty"
    if WRITE.search(sql):
        return "write"
    token = re.match(r"([A-Za-z]+)", sql)
    if not token:
        return "unexpected"
    word = token.group(1).upper()
    if word in READ:
        return "read"
    if word == "SET":
        if SET_OK.match(sql) and "GLOBAL" not in sql.upper():
            return "session"
        return "write"
    return "unexpected"


def self_test() -> int:
    # Labeled unit test. These strings are not a live server result.
    cases = [
        ("Query", "SELECT SLEEP(25) /*oo-dl01w1-08-cancel*/", "read"),
        ("Query", "SELECT CONCAT('unsigned|', CAST(id AS CHAR))", "read"),
        ("Query", "SHOW GRANTS FOR 'oo_reader'@'localhost'", "read"),
        ("Query", "SET time_zone = '+00:00'", "session"),
        ("Query", "SET NAMES utf8mb4", "session"),
        ("Query", "SET GLOBAL general_log = ON", "write"),
        ("Query", "INSERT INTO k_unsigned VALUES (1, 'x')", "write"),
        ("Query", "UPDATE k_null SET qty = 1", "write"),
        ("Query", "DELETE FROM k_json", "write"),
        ("Query", "CREATE TABLE extra (id INT)", "write"),
        ("Query", "DROP TABLE k_binary", "write"),
        ("Query", "GRANT SELECT ON oo_synth.* TO 'oo_reader'@'%'", "write"),
        ("Query", "SELECT 1; DELETE FROM k_null", "write"),
        ("Query", "/* comment */ INSERT INTO t VALUES (1)", "write"),
        ("Connect", "", "connect"),
        ("Quit", "", "connect"),
        ("Init DB", "oo_synth", "connect"),
        ("Prepare", "SELECT 1", "unexpected"),
    ]
    failed = 0
    for command, argument, expected in cases:
        got = classify(command, argument)
        if got != expected:
            failed += 1
            print(f"unit_fail command={command} expected={expected} got={got}")
    print(f"unit=classify cases={len(cases)} failed={failed}")
    return 1 if failed else 0


def live(path: str) -> int:
    counts = {"connect": 0, "read": 0, "session": 0, "write": 0, "empty": 0, "unexpected": 0}
    rows = 0
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            line = line.rstrip("\n")
            if not line:
                continue
            command, _, argument = line.partition("\t")
            kind = classify(command, argument)
            counts[kind] = counts.get(kind, 0) + 1
            rows += 1
            if kind in {"write", "unexpected", "empty"}:
                print(f"bad_row kind={kind} command={command}")
    print(
        "reader_events={rows} connect={connect} read={read} session={session} "
        "write={write} empty={empty} unexpected={unexpected}".format(rows=rows, **counts)
    )
    if rows == 0 or counts["read"] < 1 or counts["write"] or counts["unexpected"] or counts["empty"]:
        return 1
    return 0


def main() -> int:
    if len(sys.argv) == 2 and sys.argv[1] == "--self-test":
        return self_test()
    if len(sys.argv) == 2:
        return live(sys.argv[1])
    print("usage: classify.py --self-test | classify.py LOG.tsv", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())
