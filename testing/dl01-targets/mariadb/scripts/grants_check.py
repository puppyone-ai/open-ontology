#!/usr/bin/env python3
"""Check SHOW GRANTS text. Password hashes are redacted before any other use."""

import re
import sys

REDACTIONS = (
    re.compile(
        r"IDENTIFIED\s+BY\s+(?:PASSWORD\s+)?(?:'[^']*'|\"[^\"]*\")",
        re.IGNORECASE,
    ),
    re.compile(r"IDENTIFIED\s+VIA\s+.*$", re.IGNORECASE | re.MULTILINE),
    re.compile(
        r"USING\s+(?:PASSWORD\s+)?(?:'[^']*'|\"[^\"]*\")",
        re.IGNORECASE,
    ),
    re.compile(r"'[*][0-9A-Za-z./$]+'"),
)
GRANT_BODY = re.compile(r"^GRANT\s+(.+?)\s+ON\s+(\S+)\s+TO\s+", re.IGNORECASE)
READER_OK = {"USAGE", "SELECT"}
FORBIDDEN = re.compile(
    r"\b(INSERT|UPDATE|DELETE|CREATE|DROP|ALTER|INDEX|FILE|SUPER|PROCESS|"
    r"REPLICATION|BINLOG|EXECUTE|TRIGGER|SHUTDOWN|RELOAD|ALL|GRANT\s+OPTION|"
    r"CREATE\s+USER|REFERENCES)\b",
    re.IGNORECASE,
)


def redact(text: str) -> str:
    redacted = text
    for pattern in REDACTIONS:
        redacted = pattern.sub("<redacted>", redacted)
    return redacted


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: grants_check.py GRANTS.txt", file=sys.stderr)
        return 2
    raw = open(sys.argv[1], encoding="utf-8").read()
    text = redact(raw)
    sys.stdout.write(text)
    if text == raw and "IDENTIFIED BY" in raw.upper() and "<redacted>" not in text:
        print("grant_hash_redaction=failed", file=sys.stderr)
        return 1
    reader_select = 0
    reader_lines = 0
    dba_process = 0
    dba_connection = 0
    dba_schema = 0
    for line in text.splitlines():
        line = line.strip()
        if not line:
            continue
        match = GRANT_BODY.search(line)
        if not match:
            print(f"grant_unparsed={line}", file=sys.stderr)
            return 1
        priv = match.group(1).strip()
        target = match.group(2)
        if "oo_reader" in line:
            reader_lines += 1
            if FORBIDDEN.search(priv):
                print(f"reader_forbidden_privilege={priv}", file=sys.stderr)
                return 1
            parts = {part.strip().upper() for part in priv.split(",")}
            if not parts <= READER_OK:
                print(f"reader_privilege_not_allowlisted={priv}", file=sys.stderr)
                return 1
            if parts == {"SELECT"}:
                if target not in {"`oo_synth`.*", "oo_synth.*"}:
                    print(f"reader_select_target={target}", file=sys.stderr)
                    return 1
                reader_select += 1
        if "oo_dba" in line:
            priv_upper = priv.upper()
            if "PROCESS" in priv_upper:
                dba_process += 1
            if "CONNECTION ADMIN" in priv_upper:
                dba_connection += 1
            if "oo_synth" in target:
                dba_schema += 1
    print(
        f"reader_grant_lines={reader_lines} reader_select_grants={reader_select} "
        f"dba_process_grants={dba_process} dba_connection_admin_grants={dba_connection} "
        f"dba_schema_grants={dba_schema}",
        file=sys.stderr,
    )
    if reader_select < 2 or dba_process < 1 or dba_connection < 1 or dba_schema < 1:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
