#!/usr/bin/env python3
"""Independent typed-cell compare. Parses source raw JSON and PG canonical text itself."""

from __future__ import annotations

import json
import sys


def main() -> int:
    source_path, pg_path, out_path = sys.argv[1:]
    with open(source_path, encoding="utf-8") as handle:
        source = json.load(handle)
    with open(pg_path, encoding="utf-8") as handle:
        pg = json.load(handle)
    fields = source["approved_fields"]
    docs = source["docs"]
    count = source["count"]
    relation = source["relation"]
    diffs: list[str] = []
    if relation != "eq":
        diffs.append(f"relation:{relation}")
    if len(docs) != count:
        diffs.append(f"source_len:{len(docs)}:count:{count}")
    live = [row for row in pg["rows"] if not row["tombstone"]]
    tombstones = [row["doc_key"] for row in pg["rows"] if row["tombstone"]]
    if len(live) != count:
        diffs.append(f"pg_live:{len(live)}:count:{count}")
    by_key = {}
    for doc in docs:
        raw = json.loads(doc["raw"])
        key = raw.get("doc_key")
        if not isinstance(key, str):
            diffs.append("bad_source_key")
            continue
        if key in by_key:
            diffs.append(f"duplicate_source:{key}")
        by_key[key] = raw
    pg_by_key = {row["doc_key"]: json.loads(row["canonical"]) for row in live}
    keys = sorted(set(by_key) | set(pg_by_key))
    compared = 0
    for key in keys:
        left = by_key.get(key)
        right = pg_by_key.get(key)
        if left is None:
            diffs.append(f"extra_pg:{key}")
            continue
        if right is None:
            diffs.append(f"missing_pg:{key}")
            continue
        for field in fields:
            source_value = left[field] if field in left else None
            pg_value = right[field] if field in right else None
            if source_value != pg_value:
                diffs.append(f"cell:{key}:{field}")
            else:
                compared += 1
        for field in right:
            if field not in fields:
                diffs.append(f"typed_extra:{key}:{field}")
    required = source.get("tombstone_required") or []
    for key in required:
        if key not in tombstones:
            diffs.append(f"tombstone_missing:{key}")
        if key in pg_by_key:
            diffs.append(f"tombstone_live:{key}")
    hidden = source.get("hidden_fields") or {}
    for key, field in hidden.items():
        names = field if isinstance(field, list) else [field]
        raw = by_key.get(key)
        typed = pg_by_key.get(key)
        for name in names:
            if raw is None or name not in raw:
                diffs.append(f"hidden_absent_in_source:{key}:{name}")
            if typed is not None and name in typed:
                diffs.append(f"hidden_visible:{key}:{name}")
    report = {
        "diff_count": len(diffs),
        "diffs": diffs,
        "source_count": count,
        "pg_live": len(live),
        "cells_compared": compared,
        "sampled": False,
        "tombstones": tombstones,
    }
    with open(out_path, "w", encoding="utf-8") as handle:
        json.dump(report, handle, indent=2)
        handle.write("\n")
    return 0 if not diffs and compared == count * len(fields) and count > 0 else 2


if __name__ == "__main__":
    raise SystemExit(main())
