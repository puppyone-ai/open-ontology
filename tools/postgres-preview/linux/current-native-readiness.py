#!/usr/bin/env python3
"""Read-only PG capture/target presence preflight; never a product acceptance gate."""

import argparse
import json
import subprocess
from pathlib import Path


def scalar(container, database, sql):
    return subprocess.check_output(
        ["docker", "exec", container, "psql", "-U", "postgres", "-d", database,
         "-X", "-A", "-t", "-v", "ON_ERROR_STOP=1", "-c", sql],
        text=True, stderr=subprocess.PIPE, timeout=15,
    ).strip()


def inspect(evidence, approved):
    index = json.loads((evidence / "native-admit/index.json").read_text())
    names = [f"{obj['schema']}.{obj['table']}" for obj in approved['approved_objects']]
    if not names or len(set(names)) != len(names):
        raise ValueError("approved object list is empty or repeated")
    for name in names:
        if not all(part.isidentifier() for part in name.split('.')) or len(name.split('.')) != 2:
            raise ValueError("unsafe approved object name")
    actual = {name: int(scalar("oo-dl01w1-46-src", "src", f"SELECT count(*) FROM {name}"))
              for name in names}
    target = {
        name: scalar("oo-dl01w1-46-native", "dst_native_r1",
                     "SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace "
                     f"WHERE n.nspname='{name.split('.')[0]}' AND c.relname='{name.split('.')[1]}' "
                     "AND c.relkind IN ('r','p')")
        for name in names
    }
    indexed = {f"{t['schema']}.{t['table']}": t['rows'] for t in index['tables']}
    page_count = sum(len(t['pages']) for t in index['tables'])
    return {
        "protocol": "oo.postgres-preview.current-native-readiness/v1",
        "source_rows": actual,
        "approved_objects": names,
        "capture_index_rows": indexed,
        "capture_matches_source": indexed == actual,
        "capture_pages": page_count,
        "capture_fidelity_complete": index['fidelity_complete'],
        "capture_delete_grade": index['delete_grade'],
        "target_relation_count": {name: int(count) for name, count in target.items()},
        "native_import_preflight_pass": (indexed == actual
                                         and bool(index['fidelity_complete'])
                                         and index['delete_grade'] != "unknown"
                                         and all(int(count) == 1 for count in target.values())),
        "product_exit_asserted": False,
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    report = inspect(args.evidence, json.loads(args.config.read_text()))
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(json.dumps(report, sort_keys=True))
    return 0 if report['native_import_preflight_pass'] else 3


if __name__ == "__main__":
    raise SystemExit(main())
