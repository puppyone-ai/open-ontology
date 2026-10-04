#!/usr/bin/env python3
"""Classify PostgreSQL statement logs. Writes keyword counts, not row data."""

import re
import sys
from collections import Counter
from pathlib import Path

WRITES = {
    "INSERT",
    "UPDATE",
    "DELETE",
    "TRUNCATE",
    "CREATE",
    "ALTER",
    "DROP",
    "GRANT",
    "REVOKE",
    "COMMIT",
    "COPY",
}
READS = {
    "SELECT",
    "WITH",
    "TABLE",
    "VALUES",
    "EXPLAIN",
    "SHOW",
    "SET",
    "RESET",
    "BEGIN",
    "START",
    "ROLLBACK",
    "SAVEPOINT",
    "RELEASE",
    "PREPARE",
    "EXECUTE",
    "DEALLOCATE",
    "FETCH",
    "CLOSE",
    "DISCARD",
    "LISTEN",
    "UNLISTEN",
}

LINE = re.compile(
    r"^(?P<ts>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d+ \S+) "
    r"pid=(?P<pid>\d+) app=(?P<app>\S*) user=(?P<user>\S*) db=(?P<db>\S*) "
    r"(?P<level>[A-Z]+):\s*(?P<rest>.*)$"
)


def keyword(sql: str) -> str:
    text = sql.strip()
    while True:
        if text.startswith("--"):
            newline = text.find("\n")
            text = "" if newline < 0 else text[newline + 1 :]
            text = text.strip()
            continue
        if text.startswith("/*"):
            end = text.find("*/")
            if end < 0:
                return ""
            text = text[end + 2 :].strip()
            continue
        break
    if not text:
        return ""
    token = text.split(None, 1)[0]
    token = token.split("(", 1)[0]
    return token.upper()


def statements(text: str):
    current = None
    for raw in text.splitlines():
        match = LINE.match(raw)
        if match:
            if current is not None:
                yield current
            rest = match.group("rest")
            body = ""
            is_statement = False
            if rest.startswith("statement:"):
                body = rest[len("statement:") :]
                is_statement = True
            elif rest.startswith("execute ") and ":" in rest:
                body = rest.split(":", 1)[1]
                is_statement = True
            current = {
                "user": match.group("user"),
                "app": match.group("app"),
                "db": match.group("db"),
                "level": match.group("level"),
                "body": body,
                "is_statement": is_statement,
            }
            continue
        if current is not None and current["is_statement"]:
            current["body"] += "\n" + raw
    if current is not None:
        yield current


def main() -> int:
    source = Path(sys.argv[1])
    dest = Path(sys.argv[2])
    secrets = Path(sys.argv[3])
    text = source.read_text(errors="replace")
    for name in ("super.pass", "reader.pass"):
        raw = (secrets / name).read_text().strip()
        if raw:
            text = text.replace(raw, "[redacted]")
    text = text.replace("a156-live-comment-body-do-not-carry", "[redacted-comment]")
    reader = Counter()
    admin = Counter()
    reader_writes = []
    reader_other = []
    reader_statements = 0
    for item in statements(text):
        if not item["is_statement"]:
            continue
        word = keyword(item["body"]) or "(empty)"
        if item["user"] == "oo_a156_reader":
            reader_statements += 1
            reader[word] += 1
            if word in WRITES:
                reader_writes.append(word)
            elif word not in READS:
                reader_other.append(word)
        elif item["user"] == "postgres":
            admin[word] += 1
    lines = [
        f"reader_statements={reader_statements}",
        f"reader_writes={len(reader_writes)}",
        f"reader_unclassified={len(reader_other)}",
        "reader_keywords:",
    ]
    for word, count in sorted(reader.items()):
        lines.append(f"  {word}={count}")
    lines.append("admin_keywords:")
    for word, count in sorted(admin.items()):
        lines.append(f"  {word}={count}")
    if reader_writes:
        lines.append("reader_write_keywords=" + ",".join(reader_writes))
    if reader_other:
        lines.append("reader_unclassified_keywords=" + ",".join(reader_other))
    dest.write_text("\n".join(lines) + "\n")
    redacted_log = dest.with_name("postgresql.redacted.log")
    redacted_log.write_text(text)
    if reader_writes or reader_other:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
