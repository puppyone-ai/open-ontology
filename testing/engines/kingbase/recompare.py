#!/usr/bin/env python3
"""Recheck retained raw rows with the shared typed oracle; never rewrite history."""
import argparse
import hashlib
import json
import re

from run import ROOT, TABLES, canonical_digest


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-id", required=True)
    args = parser.parse_args()
    if not re.fullmatch(r"[a-z0-9-]{1,64}", args.run_id):
        parser.error("invalid run identity")
    out = ROOT / "evidence" / args.run_id
    destination = out / "typed-recomparison.json"
    if destination.exists():
        raise FileExistsError("retained comparison already exists")
    report = {"oracle": "oo-test typed canonical JSON, 10000-row SHA256 chunks",
              "inputs": {}, "cycles": []}
    for path in [ROOT / "tools/oo-test/comparison.py",
                 ROOT / "testing/engines/kingbase/run.py",
                 ROOT / "testing/engines/kingbase/recompare.py"]:
        report["inputs"][str(path.relative_to(ROOT))] = hashlib.sha256(path.read_bytes()).hexdigest()
    for cycle in range(1, 4):
        tables = {}
        for table in TABLES:
            sides = []
            for side in ["source", "target"]:
                path = out / f"c5-{cycle}-{table}-{side}.stdout"
                raw = path.read_bytes()
                report["inputs"][path.name] = hashlib.sha256(raw).hexdigest()
                sides.append(canonical_digest(raw.decode()))
            tables[table] = {"source": sides[0], "target": sides[1], "equal": sides[0] == sides[1]}
        report["cycles"].append({"cycle": cycle, "tables": tables})
    report["pass"] = all(t["equal"] for c in report["cycles"] for t in c["tables"].values())
    destination.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"pass": report["pass"], "cycles": 3, "tables_per_cycle": len(TABLES)}))
    return int(not report["pass"])


if __name__ == "__main__":
    raise SystemExit(main())
