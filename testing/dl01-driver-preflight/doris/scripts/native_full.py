#!/usr/bin/env python3
"""Read a fresh seven-table Doris capture and compare every source cell.

This operator command is read-only on Doris. DBA windows and target jobs are
separate, explicitly coordinated operations. Existing evidence is never reused
as a successful new capture.
"""
import argparse
import json
from pathlib import Path

import window_9081609e as source


def compare(label):
    parts = []
    for name, columns in source.TABLES:
        pads = ",".join(["''"] * (len(columns.split(",")) - 1))
        parts.extend([f"SELECT '@@{name}', {pads}",
                      f"SELECT {columns} FROM {name} ORDER BY {columns}"])
    path = source.EVIDENCE / f"reader-{label}.sql"
    source.write(path, ";\n".join(parts) + ";\n")
    result = source.mysql("oo_reader", source.READER_SECRETS / "reader.password",
                          [path], f"independent-{label}")
    dumped = {name: [] for name, _ in source.TABLES}
    current = None
    for cells in source.rows_of(result.stdout):
        if cells[0].startswith("@@"):
            current = cells[0][2:]
        elif current:
            dumped[current].append("\x1f".join(
                "\\N" if cell == "NULL" else cell for cell in cells))
    tick = json.loads((source.EVIDENCE / f"{label}.json").read_text())
    mismatch = [name for name, _ in source.TABLES
                if sorted(dumped[name]) != sorted(source.table_rows(tick, name))]
    document = {"mysql_exit": result.returncode, "mismatch_tables": mismatch,
                "table_counts": {name: len(rows) for name, rows in dumped.items()},
                "rows": dumped}
    source.write(source.EVIDENCE / f"{label}-cell-oracle.json",
                 json.dumps(document, indent=2))
    return result.returncode or bool(mismatch)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--evidence", required=True)
    parser.add_argument("--label", required=True)
    parser.add_argument("--every-ms", type=int, required=True)
    args = parser.parse_args()
    evidence = Path(args.evidence).resolve()
    parent = source.REPO / "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product"
    if parent not in evidence.parents:
        raise SystemExit("evidence must be inside the owned Doris product directory")
    if not args.label.isalnum() or args.every_ms < 1000:
        raise SystemExit("invalid label or period")
    source.EVIDENCE = evidence
    evidence.mkdir(parents=True, exist_ok=True)
    if (evidence / f"{args.label}.json").exists():
        raise SystemExit("capture output exists; use a fresh label")
    if source.sha256_file(source.ELF) != source.ELF_SHA:
        raise SystemExit("reader ELF changed")
    code = source.run_tick(args.label, args.every_ms, args.label, 180)
    if code == 0:
        code = compare(args.label)
    if source.secret_leak():
        raise SystemExit("secret leakage in evidence")
    source.sha_tree()
    print(json.dumps({"capture_label": args.label, "exit": int(code)}))
    return int(code)


if __name__ == "__main__":
    raise SystemExit(main())
