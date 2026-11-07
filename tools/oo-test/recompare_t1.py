#!/usr/bin/env python3
"""Re-audit retained PostgreSQL T1 SQL outputs; never overwrite earlier reports."""
import argparse
import hashlib
import json
from pathlib import Path

import t1
from runner import ROOT, write_json


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def recompare(path):
    original = json.loads((path / "t1-t8.json").read_text())
    if original["T1"] != "passed":
        raise RuntimeError("only completed original captures can be re-audited")
    captures = json.loads((path / "corrected-receipts.json").read_text())
    calls = {}
    loads = []
    for file in path.glob("*.json"):
        if not file.stem.isdigit():
            continue
        record = json.loads(file.read_text())
        calls[int(file.stem)] = record
        if record.get("argv") and Path(record["argv"][0]).name == "oo-load":
            loads.append(int(file.stem))
    if len(loads) != 3 or len(captures) != len(original["tables"]):
        raise RuntimeError("capture command sequence incomplete")
    first = max(loads) + 1
    out = path / "typed-comparison"
    out.mkdir(exist_ok=False)
    tables = []
    for index, receipt in enumerate(captures):
        table = receipt["table"]["name"]
        if table != original["tables"][index]["table"] or not table.isidentifier():
            raise RuntimeError("table capture order changed")
        sequence = [first + index * 3 + offset for offset in range(3)]
        if any(calls[n]["exit"] != 0 for n in sequence):
            raise RuntimeError("original SQL query failed")
        files = [path / f"{n:04d}.stdout" for n in sequence]
        metadata, left, right = [file.read_text() for file in files]

        class Source:
            def sql(self, query):
                return 0, metadata if "information_schema.columns" in query else left

        class Target:
            def sql(self, query):
                return right

        result = t1.compare_pg(Source(), Target(), receipt, table)
        result["captured_inputs"] = [
            {"file": file.name, "sha256": digest(file)} for file in files
        ]
        result["hook_sha256"] = digest(Path(t1.__file__))
        write_json(out / f"{table}.json", result)
        rows = [sum(chunk["rows"][side] for chunk in result["chunks"]) for side in (0, 1)]
        if not result["equal"] or rows[0] != rows[1]:
            raise RuntimeError("typed comparison difference")
        tables.append({"table": table, "rows": rows[0], "equal": True})
    result = {"status": "passed", "basis": "retained successful source and target SQL outputs",
              "original_manifest_sha256": digest(path / "SHA256SUMS"), "tables": tables}
    write_json(out / "summary.json", result)
    (out / "SHA256SUMS").write_text("".join(
        f"{digest(file)}  {file.name}\n" for file in sorted(out.glob("*.json"))
    ))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--versions", help="comma-separated PostgreSQL majors; default all captured versions")
    args = parser.parse_args()
    path = args.evidence.resolve()
    if not path.is_relative_to(ROOT / "evidence"):
        parser.error("evidence must be inside this worktree")
    directories = sorted(path.glob("postgres-*"))
    if args.versions:
        versions = args.versions.split(",")
        if any(v not in ("13", "14", "15", "16", "17", "18") for v in versions):
            parser.error("expected PostgreSQL majors 13..18")
        directories = [path / f"postgres-{v}" for v in versions]
    results = []
    for directory in directories:
        result = recompare(directory)
        results.append({"version": directory.name, "status": result["status"], "tables": len(result["tables"])})
    if not results:
        raise RuntimeError("no completed PostgreSQL evidence found")
    print(json.dumps(results))


if __name__ == "__main__":
    main()
