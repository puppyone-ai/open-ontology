#!/usr/bin/env python3
"""Redact credential-shaped SQL text. Does not read secret files."""

from __future__ import annotations

import re
import sys
from pathlib import Path

PATTERNS = [
    re.compile(r"(?i)(identified\s+by\s+)(\"[^\"]*\"|'[^']*'|\S+)"),
    re.compile(r"(?i)(connect\s+\w+/)(\"[^\"]*\"|'[^']*'|\S+)"),
    re.compile(r"(?i)ORACLE PASSWORD.*"),
]


def redact(text: str) -> str:
    for pattern in PATTERNS:
        text = pattern.sub(lambda match: match.group(1) + "<redacted>" if match.lastindex else "<redacted>", text)
    return text


def main() -> int:
    root = Path(sys.argv[1])
    for path in root.rglob("*"):
        if not path.is_file():
            continue
        if path.suffix.lower() in {".png", ".jpg", ".zip", ".gz"}:
            continue
        raw = path.read_bytes()
        try:
            text = raw.decode("utf-8")
        except UnicodeDecodeError:
            continue
        cleaned = redact(text)
        if cleaned != text:
            path.write_text(cleaned, encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())
