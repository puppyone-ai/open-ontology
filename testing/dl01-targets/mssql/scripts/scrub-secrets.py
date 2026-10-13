#!/usr/bin/env python3
"""Replace password-file bytes in a log. Prints only the replacement count."""

from __future__ import annotations

import sys
from pathlib import Path


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: scrub-secrets.py LOG SECRET_DIR", file=sys.stderr)
        return 2
    log = Path(sys.argv[1])
    secret_dir = Path(sys.argv[2])
    text = log.read_text(encoding="utf-8", errors="replace")
    count = 0
    for path in sorted(secret_dir.glob("*.password")):
        secret = path.read_text(encoding="utf-8").strip()
        if len(secret) < 8:
            continue
        found = text.count(secret)
        if found:
            text = text.replace(secret, "[redacted]")
            count += found
    env = secret_dir / "server.env"
    if env.exists():
        for line in env.read_text(encoding="utf-8", errors="replace").splitlines():
            if line.startswith("MSSQL_SA_PASSWORD="):
                secret = line.split("=", 1)[1]
                found = text.count(secret)
                if secret and found:
                    text = text.replace(secret, "[redacted]")
                    count += found
    log.write_text(text, encoding="utf-8")
    print(f"redactions={count}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
