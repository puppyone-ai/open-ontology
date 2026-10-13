#!/usr/bin/env python3
"""Superuser physical-cell oracle for the four native generations.

Decodes each sealed Arrow with the already-built os-decode JSON, then reads
_oo physical tables through _oo.physical_mappings. It does not call
_oo.native_update_read_cells and it does not send a truth document.
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

ROOT = Path("/Users/redux/puppy-one/open-ontology")
EV = ROOT / "evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight/native-20261012"
GEN = {"fast-1": 1, "fast-3": 2, "fast-5": 3, "fast-8": 4}
APPROVED = {
    "bin_value",
    "doc_key",
    "double_value",
    "event_ms",
    "flag",
    "long_value",
    "note",
    "payload_json",
    "quantities",
    "scaled_value",
    "seq_key",
    "tags",
    "thing",
}


def psql(sql: str) -> str:
    result = subprocess.run(
        [
            "docker",
            "exec",
            "oo-dl01w1-44-native-pg",
            "psql",
            "-U",
            "postgres",
            "-d",
            "oo_dl01w1_44",
            "-v",
            "ON_ERROR_STOP=1",
            "-At",
            "-c",
            sql,
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout


def quote_ident(name: str) -> str:
    prefix = name[:3]
    rest = name[3:]
    if prefix not in {"pc_", "pt_"} or len(rest) != 60 or not all(ch in "0123456789abcdef" for ch in rest):
        raise SystemExit(f"unexpected physical name {name}")
    return '"' + name + '"'


def expr(logical: str, physical: str, typ: str) -> str:
    col = quote_ident(physical)
    if typ == "float8":
        body = f"CASE WHEN {col} IS NULL THEN NULL ELSE encode(float8send({col}), 'hex') END"
    elif typ == "bytea":
        body = f"CASE WHEN {col} IS NULL THEN NULL ELSE encode({col}, 'hex') END"
    elif typ == "timestamptz":
        body = f"CASE WHEN {col} IS NULL THEN NULL ELSE ((EXTRACT(EPOCH FROM {col}) * 1000000)::bigint)::text END"
    elif typ == "bool":
        body = f"CASE WHEN {col} IS NULL THEN NULL WHEN {col} THEN 'true' ELSE 'false' END"
    elif typ in {"int8", "varchar", "text"}:
        body = f"{col}::text"
    elif typ == "json":
        body = f"{col}::text"
    else:
        raise SystemExit(f"unhandled pg type {typ} for {logical}")
    return f"'{logical}', {body}"


def arrow_cell(value: dict) -> str | None:
    kind = value["t"]
    if kind == "absent":
        return None
    if kind == "null":
        return None
    if kind == "utf8":
        return value["v"]
    if kind == "json":
        return value["v"]
    if kind == "i64":
        return value["v"]
    if kind == "bool":
        return "true" if value["v"] else "false"
    if kind == "bytes":
        return value["hex"]
    if kind == "f64":
        return f"{int(value['bits']):016x}"
    if kind == "ts_us":
        return value["v"]
    if kind == "list_binary":
        items = []
        for item in value["v"]:
            if item["t"] == "null":
                items.append(None)
            elif item["t"] == "utf8":
                items.append(item["v"])
            else:
                raise SystemExit(f"list item {item}")
        return json.dumps(items, ensure_ascii=False, separators=(",", ":"))
    raise SystemExit(f"arrow cell {value}")


def main() -> int:
    mappings = json.loads(
        psql(
            """
            SELECT COALESCE(json_agg(json_build_object(
              'generation', run_generation,
              'level', level,
              'column', logical_column,
              'physical', physical_name
            ) ORDER BY run_generation, level, logical_column), '[]'::json)::text
            FROM _oo.physical_mappings
            WHERE logical_namespace = 'ledger' AND (logical_table = 'ledger_rows' OR logical_table = '')
            """
        )
    )
    types = json.loads(
        psql(
            """
            SELECT COALESCE(json_object_agg(a.attrelid::regclass::text || '.' || a.attname, t.typname), '{}'::json)::text
            FROM pg_attribute a
            JOIN pg_type t ON t.oid = a.atttypid
            WHERE a.attrelid IN (SELECT ('_oo.' || physical_name)::regclass FROM _oo.physical_mappings WHERE level = 't')
              AND a.attnum > 0 AND NOT a.attisdropped
            """
        )
    )
    tombstones = json.loads(
        psql(
            """
            SELECT COALESCE(json_agg(json_build_object(
              'generation', source_generation,
              'key', key_text,
              'batch_id', batch_id
            ) ORDER BY source_generation, key_text), '[]'::json)::text
            FROM _oo.update_tombstone
            """
        )
    )
    diffs: list[str] = []
    cells = 0
    reports = []
    by_gen_table = {}
    by_gen_cols = {}
    for row in mappings:
        if row["level"] == "t":
            by_gen_table[row["generation"]] = row["physical"]
        elif row["level"] == "c":
            by_gen_cols.setdefault(row["generation"], {})[row["column"]] = row["physical"]
    for name, generation in GEN.items():
        decoded = json.loads((EV / "oracle" / "decode" / f"{name}.json").read_text())
        schema_names = [field["name"] for field in decoded["fields"]]
        mapped = by_gen_cols[generation]
        if sorted(schema_names) != sorted(mapped):
            diffs.append(f"schema:{name}:arrow_vs_mapping")
        table = by_gen_table[generation]
        pieces = []
        rel = f"_oo.{table}"
        for logical, physical in sorted(mapped.items()):
            typ = types.get(f"{rel}.{physical}")
            if typ is None:
                diffs.append(f"type_missing:{name}:{logical}")
                continue
            pieces.append(expr(logical, physical, typ))
        sql = (
            "SELECT COALESCE(json_agg(row_json ORDER BY row_json->>'source_doc_id'), '[]'::json)::text FROM ("
            f"SELECT json_build_object({', '.join(pieces)}) AS row_json "
            f"FROM _oo.{quote_ident(table)}) s"
        )
        rows = json.loads(psql(sql))
        (EV / "oracle" / f"pg-{name}.json").write_text(json.dumps(rows, indent=2) + "\n")
        arrow_rows = {row["source_doc_id"]: row for row in decoded["rows"]}
        pg_rows = {row["source_doc_id"]: row for row in rows}
        if len(arrow_rows) != len(decoded["rows"]):
            diffs.append(f"duplicate_arrow:{name}")
        if len(pg_rows) != len(rows):
            diffs.append(f"duplicate_pg:{name}")
        if set(arrow_rows) != set(pg_rows):
            diffs.append(f"keys:{name}:{sorted(set(arrow_rows) ^ set(pg_rows))}")
        for doc_id in sorted(set(arrow_rows) & set(pg_rows)):
            left = arrow_rows[doc_id]
            right = pg_rows[doc_id]
            for logical in ("source_doc_id", "source_index"):
                cells += 1
                if right.get(logical) != left[logical]:
                    diffs.append(f"cell:{name}:{doc_id}:{logical}")
            cells += 1
            raw = right.get("raw_source")
            if raw is None or bytes.fromhex(raw).decode("utf-8") != left["raw_source_utf8"]:
                diffs.append(f"cell:{name}:{doc_id}:raw_source")
            for logical, cell in left["columns"].items():
                present_name = f"present__{logical}"
                cells += 1
                expected_present = "true" if cell["present"] else "false"
                if right.get(present_name) != expected_present:
                    diffs.append(f"cell:{name}:{doc_id}:{present_name}")
                cells += 1
                expected = arrow_cell(cell)
                actual = right.get(logical)
                if cell["t"] == "absent":
                    if actual is not None:
                        diffs.append(f"cell:{name}:{doc_id}:{logical}:absent_stored")
                elif actual != expected:
                    diffs.append(f"cell:{name}:{doc_id}:{logical}")
                if logical == "where_point":
                    diffs.append(f"where_point_visible:{name}:{doc_id}")
        reports.append(
            {
                "name": name,
                "generation": generation,
                "arrow_rows": len(decoded["rows"]),
                "pg_rows": len(rows),
                "columns": len(mapped),
                "physical_table": table,
                "ids": sorted(pg_rows),
            }
        )
    expected_tombs = {1: [], 2: [], 3: [], 4: ["led-c5"]}
    got_tombs = {generation: [] for generation in expected_tombs}
    for row in tombstones:
        got_tombs.setdefault(row["generation"], []).append(row["key"])
    for generation, keys in expected_tombs.items():
        if got_tombs.get(generation, []) != keys:
            diffs.append(f"tombstone_table:{generation}:{got_tombs.get(generation)}")
    if "led-c5" in reports[0]["ids"] or "led-c5" not in reports[1]["ids"] or "led-c5" not in reports[2]["ids"] or "led-c5" in reports[3]["ids"]:
        diffs.append("led-c5-lifecycle")
    verdict = {
        "protocol": "oo.opensearch.native-cell-oracle/v1",
        "diff_count": len(diffs),
        "diffs": diffs,
        "cells_compared": cells,
        "sampled": False,
        "reader_mutations": 0,
        "generations": reports,
        "tombstones": tombstones,
        "approved_fields_retained": sorted(APPROVED),
        "decoder": "os-decode 7cb5a706267893f76e2b9df965c650b81283d0f2d781b4624de9b9c0cb4d6fa2",
        "read_path": "postgres superuser select of _oo.pt_* via _oo.physical_mappings",
        "loader_read_cells": False,
    }
    out = EV / "oracle" / "celldiff.json"
    out.write_text(json.dumps(verdict, indent=2) + "\n")
    print(json.dumps({"diff_count": verdict["diff_count"], "cells_compared": cells, "diffs": diffs[:20]}, indent=2))
    return 0 if not diffs and cells > 0 else 2


if __name__ == "__main__":
    sys.exit(main())
