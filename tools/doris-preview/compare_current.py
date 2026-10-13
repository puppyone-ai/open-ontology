#!/usr/bin/env python3
"""Compare every current Doris source cell with the sealed target relations."""

import argparse
import json
import re
import subprocess
from pathlib import Path

SOURCE = "dorispreview"
TABLES = {"dup_events", "agg_qty", "uniq_acct", "dec_exact", "part_day", "str_key", "nz_obs"}
IDENT = re.compile(r"[a-z][a-z0-9_]*\Z")


def psql(sql, database):
    result = subprocess.run(
        ["docker", "exec", "-i", "oo-dl01w1-43-native-pg", "psql", "-X", "-U", "oo_admin",
         "-d", database, "-v", "ON_ERROR_STOP=1", "-At"],
        input=sql, text=True, capture_output=True, timeout=60)
    if result.returncode:
        raise RuntimeError("target read refused: " + result.stderr[-500:])
    return json.loads(result.stdout.strip())


def safe(name):
    if not isinstance(name, str) or not IDENT.fullmatch(name):
        raise ValueError("unsafe physical identifier")
    return name


def bag(rows):
    return sorted(json.dumps(row, separators=(",", ":"), ensure_ascii=False) for row in rows)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--collection", type=Path, required=True)
    parser.add_argument("--capture", type=Path, required=True)
    parser.add_argument("--database", required=True)
    parser.add_argument("--table", choices=sorted(TABLES), action="append")
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    selected = set(args.table) if args.table else TABLES
    if args.table and len(args.table) != len(selected):
        raise ValueError("duplicate table selection")
    if args.out.exists():
        raise ValueError("comparison output exists")
    collection = json.loads(args.collection.read_text())
    capture = json.loads(args.capture.read_text())
    if collection.get("protocol") != "oo.pipeline.sealed-import-collection/v1" \
            or capture.get("protocol") != "oo.doris.capture-tick/v1" \
            or capture.get("product_reader_write_attempts") != 0:
        raise ValueError("source/vector identity")
    sources = {item["name"]: item for item in capture["tables"]}
    if set(sources) != TABLES or len(collection.get("entries", [])) != len(selected):
        raise ValueError("selected table coverage")
    results = {}
    for entry in collection["entries"]:
        if entry.get("engine") != "doris" or entry.get("source_id") != SOURCE \
                or len(entry.get("tables", [])) != 1:
            raise ValueError("catalog entry identity")
        table = entry["tables"][0]
        name = table["logical_table"]
        if name not in selected or name in results or table["logical_schema"] != "oo_dl01w1_43":
            raise ValueError("catalog table coverage")
        physical = safe(table["stored_physical"])
        columns = table["columns"]
        logical = [item["logical_name"] for item in columns]
        stored = [safe(item["stored_physical"]) for item in columns]
        if len(stored) != len(set(stored)) or not stored:
            raise ValueError("catalog columns")
        projected = ",".join(column + "::text" for column in stored)
        rows = psql(f"SELECT coalesce(jsonb_agg(jsonb_build_array({projected})), '[]'::jsonb)::text FROM _oo.{physical};", args.database)
        expected = [[None if cell == r"\N" else cell for cell in row.split("\x1f")]
                    for row in sources[name]["rows"]]
        if any(len(row) != len(columns) for row in expected):
            raise ValueError("source cell width")
        db_types = psql("SELECT coalesce(jsonb_object_agg(a.attname, jsonb_build_array(a.atttypid::int,a.atttypmod)), '{}'::jsonb)::text "
                        "FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid "
                        "JOIN pg_namespace n ON n.oid=c.relnamespace "
                        f"WHERE n.nspname='_oo' AND c.relname='{physical}' AND a.attnum>0 AND NOT a.attisdropped;", args.database)
        type_checks = {item["logical_name"]: db_types.get(item["stored_physical"]) == [item["oid"], item["typmod"]]
                       for item in columns}
        freshness = psql("SELECT coalesce(jsonb_agg(jsonb_build_object('source_generation',source_generation,"
                         "'target_generation',target_generation,'physical_name',physical_name)), '[]'::jsonb)::text "
                         "FROM _oo.table_freshness WHERE source='dorispreview' "
                         f"AND schema_name='oo_dl01w1_43' AND table_name='{name}';", args.database)
        binding_current = len(freshness) == 1 and freshness[0] == {
            "source_generation": entry["source_generation"],
            "target_generation": entry["target_generation"],
            "physical_name": physical}
        equal = bag(expected) == bag(rows)
        results[name] = {"expected_rows": len(expected), "target_rows": len(rows),
                         "typed_cells": len(expected) * len(columns),
                         "typed_cell_diff": 0 if equal else "mismatch",
                         "column_types_equal": all(type_checks.values()),
                         "type_checks": type_checks, "binding_current": binding_current,
                         "source_generation": entry["source_generation"],
                         "target_generation": entry["target_generation"]}
    good = set(results) == selected and all(item["typed_cell_diff"] == 0
                                                and item["column_types_equal"]
                                                and item["binding_current"] for item in results.values())
    report = {"protocol": "oo.doris.current-typed-cell-diff/v1", "sampled": False,
              "all_equal": good, "source_id": SOURCE, "tables": results,
              "full_source": selected == TABLES,
              "typed_cells": sum(item["typed_cells"] for item in results.values()),
              "typed_cell_diff": 0 if good else "mismatch"}
    args.out.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"all_equal": good, "typed_cells": report["typed_cells"]}))
    if not good:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
