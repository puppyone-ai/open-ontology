#!/usr/bin/env python3
"""Create source-read, unsampled four-table Selfbuild inputs for a sealed collection."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess


SOURCE_CONTAINER = "oo-dl01w1-46-src"
READER = "oo_dl01w1_46_reader"
DATABASE = "src"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def query(sql):
    return subprocess.check_output([
        "docker", "exec", SOURCE_CONTAINER, "psql", "-U", READER, "-d", DATABASE, "-X", "-A", "-t", "-F", "|", "-c", sql,
    ], text=True).strip().splitlines()


def quote(name):
    if not name or not all(char.isascii() and (char.isalnum() or char == "_") for char in name):
        raise ValueError("invalid identifier")
    return '"' + name + '"'


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--source-container", default=SOURCE_CONTAINER)
    parser.add_argument("--database", default=DATABASE)
    parser.add_argument("--reader", default=READER)
    parser.add_argument("--collection", required=True, type=Path)
    parser.add_argument("--typed-diff", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--consistency", choices=["S1", "T"], required=True)
    parser.add_argument("--end-lsn", required=True)
    args = parser.parse_args()
    globals().update(SOURCE_CONTAINER=args.source_container, DATABASE=args.database, READER=args.reader)
    raw = args.collection.read_bytes()
    collection = json.loads(raw)
    bindings = collection["binding_vector"]
    assert collection["protocol"] == "oo.pipeline.sealed-import-collection/v1" and len(bindings) == 4
    expected = {"public.hubs", "public.shipments", "public.tracking_events", "customer_east.delivery_notes"}
    assert {item["dataset"] for item in bindings} == expected
    checked = json.loads(args.typed_diff.read_text())
    assert checked["difference_count_zero"] is True and {item["dataset"] for item in checked["tables"]} == expected
    vector_sha = digest(json.dumps(bindings, sort_keys=True, separators=(",", ":")).encode())
    table_generations = {
        item["dataset"]: {key: item[key] for key in ("source_generation", "target_generation", "run_id")}
        for item in bindings
    }
    profile_tables = []
    for binding in bindings:
        dataset = binding["dataset"]
        schema, table = dataset.split(".", 1)
        catalog = query(
            "SELECT a.attname::text,pg_catalog.format_type(a.atttypid,a.atttypmod)::text,a.attnotnull::text "
            "FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid=a.attrelid "
            "JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace "
            f"WHERE n.nspname='{schema}' AND c.relname='{table}' AND a.attnum>0 AND NOT a.attisdropped ORDER BY a.attnum"
        )
        fields = [line.split("|") for line in catalog]
        assert fields and all(len(item) == 3 for item in fields)
        names = [item[0] for item in fields]
        assert len(names) == len(set(names))
        stats = ["count(*)::bigint"]
        for name in names:
            ident = quote(name)
            stats.extend([f"count(*) FILTER (WHERE {ident} IS NULL)::bigint", f"count(DISTINCT {ident})::bigint"])
        values = query(f"SELECT {','.join(stats)} FROM {quote(schema)}.{quote(table)}")
        assert len(values) == 1
        numbers = [int(value) for value in values[0].split("|")]
        rows = numbers[0]
        assert len(numbers) == 1 + 2 * len(fields)
        matching = next(item for item in checked["tables"] if item["dataset"] == dataset)
        assert rows == matching["rows"] and len(fields) == matching["columns"]
        columns = []
        for index, (name, source_type, not_null) in enumerate(fields):
            nulls, distinct = numbers[1 + 2 * index:3 + 2 * index]
            assert 0 <= nulls <= rows and 0 <= distinct <= rows
            columns.append({"name": name, "source_type": source_type, "nullable": not_null != "t",
                            "aggregation": "full_unsampled", "rows": rows,
                            "nulls": nulls, "distinct": distinct,
                            "null_count": nulls, "distinct_count": distinct})
        profile_tables.append({"dataset": dataset, "schema": schema, "table": table,
                               "rows": rows, "full_scan": True, "columns": columns,
                               **table_generations[dataset]})
    assert sum(item["rows"] for item in profile_tables) == checked["rows"]
    common = {"source_id": bindings[0]["source_id"],
              "source_generation": max(item["source_generation"] for item in bindings),
              "generation_semantics": "high_watermark", "table_generations": table_generations,
              "binding_vector_sha256": vector_sha, "collection_sha256": digest(raw)}
    assert all(item["source_id"] == common["source_id"] for item in bindings)
    profile = {"protocol": "oo.source.postgres.profile/v1", **common,
               "samples": "none", "sampled": False, "tables": profile_tables}
    semantics = {"protocol": "oo.source.postgres.audit/v1", **common,
                 "target_generation": max(item["target_generation"] for item in bindings),
                 "product": "PostgreSQL", "capture_method": "C1",
                 "consistency_grade": args.consistency, "delete_detection": "native",
                 "source_contract": {"reader": READER, "write_attempts": 0,
                                     "cross_table_consistent": args.consistency == "T",
                                     "applied_end_lsn": args.end_lsn},
                 "quality": {"tables": 4, "rows": checked["rows"], "columns": sum(len(item["columns"]) for item in profile_tables),
                             "typed_cells": checked["cells"], "typed_cell_differences": 0}}
    args.out.mkdir(parents=True, exist_ok=True)
    for name, value in [("source-profile.json", profile), ("source-semantics.json", semantics)]:
        (args.out / name).write_text(json.dumps(value, indent=2) + "\n")
    print(json.dumps({"rows": checked["rows"], "columns": semantics["quality"]["columns"],
                      "profile": str(args.out / "source-profile.json"),
                      "semantics": str(args.out / "source-semantics.json")}))


if __name__ == "__main__":
    main()
