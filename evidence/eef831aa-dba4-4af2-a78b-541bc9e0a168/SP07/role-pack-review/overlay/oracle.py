#!/usr/bin/env python3
"""Recompute role-pack hashes from dumped file bytes. Does not import the compiler."""

import hashlib
import json
import sys
from pathlib import Path


def push_string(value: str) -> str:
    out = ['"']
    for ch in value:
        code = ord(ch)
        if ch == '"':
            out.append('\\"')
        elif ch == "\\":
            out.append("\\\\")
        elif ch == "\b":
            out.append("\\b")
        elif ch == "\f":
            out.append("\\f")
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\r":
            out.append("\\r")
        elif ch == "\t":
            out.append("\\t")
        elif code < 0x20:
            out.append("\\u%04x" % code)
        else:
            out.append(ch)
    out.append('"')
    return "".join(out)


def canonical(value) -> str:
    if value is None:
        return "null"
    if value is True:
        return "true"
    if value is False:
        return "false"
    if isinstance(value, int) and not isinstance(value, bool):
        return str(value)
    if isinstance(value, str):
        return push_string(value)
    if isinstance(value, list):
        return "[" + ",".join(canonical(item) for item in value) + "]"
    if isinstance(value, dict):
        keys = sorted(value)
        return "{" + ",".join(push_string(key) + ":" + canonical(value[key]) for key in keys) + "}"
    raise TypeError(type(value).__name__)


def check_pack(directory: Path) -> dict:
    files_dir = directory / "files"
    records = []
    for path in sorted(item.relative_to(files_dir).as_posix() for item in files_dir.rglob("*") if item.is_file()):
        data = (files_dir / path).read_bytes()
        records.append({
            "bytes": len(data),
            "path": path,
            "sha256": hashlib.sha256(data).hexdigest(),
        })
    manifest = next(item for item in records if item["path"] == "manifest.json")
    pack_digest = hashlib.sha256(canonical(records).encode()).hexdigest()
    cache_key = (directory / "cache_key.txt").read_text()
    cache_digest = hashlib.sha256(cache_key.encode()).hexdigest()
    expect_pack = (directory / "pack_digest.txt").read_text().strip()
    expect_manifest = (directory / "manifest_sha256.txt").read_text().strip()
    expect_cache = (directory / "cache_key_sha256.txt").read_text().strip()
    return {
        "pack_match": pack_digest == expect_pack,
        "manifest_match": manifest["sha256"] == expect_manifest,
        "cache_match": cache_digest == expect_cache,
        "pack_digest": pack_digest,
        "manifest_sha256": manifest["sha256"],
        "cache_key_sha256": cache_digest,
        "files": len(records),
    }


def main() -> int:
    root = Path(sys.argv[1])
    report = {}
    failed = False
    for name in ("clerk", "audit", "beta", "ada2"):
        directory = root / name
        if not (directory / "files").is_dir():
            report[name] = {"present": False}
            failed = True
            continue
        result = check_pack(directory)
        report[name] = result
        if not (result["pack_match"] and result["manifest_match"] and result["cache_match"]):
            failed = True
    destination = root / "python-oracle.json"
    destination.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
