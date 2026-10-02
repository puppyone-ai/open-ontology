#!/usr/bin/env python3
"""Count canaries and secret hits in a PostgreSQL log. Prints counts only."""

import hashlib
import json
import re
import sys
from pathlib import Path

KEYWORD = re.compile(r"(?:statement|execute [^:]{0,80}):\s*([A-Za-z]+)", re.IGNORECASE)
ALLOWED = {"select", "with", "start", "rollback"}


def main() -> None:
    log_path = Path(sys.argv[1])
    secret_dir = Path(sys.argv[2])
    out_path = Path(sys.argv[3])
    raw = log_path.read_bytes()
    reader = (secret_dir / "reader.pass").read_bytes().strip()
    admin = (secret_dir / "admin.pass").read_bytes().strip()
    if not reader or not admin:
        raise SystemExit("secret file is empty")
    text = raw.decode("utf-8", errors="replace")
    lines = text.splitlines()
    reader_lines = [line for line in lines if "app=oo-pg-source" in line]
    verbs: dict[str, int] = {}
    other: list[str] = []
    forbidden = 0
    for line in reader_lines:
        lowered = line.lower()
        if "paid;drop" in lowered or "bad;drop" in lowered:
            forbidden += 1
        match = KEYWORD.search(line)
        if match is None:
            continue
        word = match.group(1).lower()
        verbs[word] = verbs.get(word, 0) + 1
        if word not in ALLOWED:
            other.append(word)
    summary = {
        "bytes": len(raw),
        "sha256": hashlib.sha256(raw).hexdigest(),
        "line_count": len(lines),
        "reader_password_hits": raw.count(reader),
        "admin_password_hits": raw.count(admin),
        "comment_canary_hits": raw.count(b"CANARY-COMMENT-SECRET-a098"),
        "token_canary_hits": raw.count(b"CANARY-TOKEN-a098"),
        "reader_app_lines": len(reader_lines),
        "reader_verbs": dict(sorted(verbs.items())),
        "reader_other_verbs": sorted(set(other)),
        "reader_forbidden_ident_lines": forbidden,
    }
    summary["keep_raw"] = (
        summary["reader_password_hits"] == 0 and summary["admin_password_hits"] == 0
    )
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(summary, sort_keys=True))


if __name__ == "__main__":
    main()
