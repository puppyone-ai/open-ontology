#!/usr/bin/env python3
"""Independent cell oracle. Reads capture ticks and PostgreSQL column::text.

Does not use the import request truth field. Does not print credentials.
"""

from __future__ import annotations

import json
import subprocess
from pathlib import Path

ROOT = Path("/Users/redux/puppy-one/open-ontology/evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product")
RAW = ROOT / "native-2e0bd" / "raw"
REC = ROOT / "native-2e0bd" / "receipts"
SEP = "\u001f"
EXPECTED_OID = {
    "int64": (20, -1),
    "date32": (1082, -1),
    "utf8": (1043, -1),
}


def numeric_typmod(precision: int, scale: int) -> int:
    return (precision << 16) + scale + 4


def psql(sql: str) -> str:
    proc = subprocess.run(
        [
            "docker",
            "exec",
            "-i",
            "oo-dl01w1-43-native-pg",
            "bash",
            "-lc",
            "export PGPASSWORD=\"$(cat /secrets/pg_admin_password)\"; "
            "exec psql -h 127.0.0.1 -U oo_admin -d dorisnative -v ON_ERROR_STOP=1 -At",
        ],
        input=sql,
        text=True,
        capture_output=True,
        check=False,
    )
    if proc.returncode != 0:
        raise SystemExit(proc.stderr[-2000:])
    return proc.stdout


def cells(row: str) -> list:
    return [None if part == r"\N" else part for part in row.split(SEP)]


def tick_rows(path: Path) -> dict[str, list]:
    document = json.loads(path.read_text())
    return {item["name"]: [cells(row) for row in item["rows"]] for item in document["tables"]}


def norm(rows: list) -> list:
    return sorted(json.dumps(row, ensure_ascii=False, separators=(",", ":")) for row in rows)


def main() -> None:
    mappings = json.loads(
        psql(
            """
            SELECT coalesce(jsonb_agg(jsonb_build_object(
              'gen', run_generation,
              'level', level,
              'table', logical_table,
              'column', logical_column,
              'physical', physical_name,
              'scope', scope_table_physical
            )), '[]'::jsonb)::text
            FROM _oo.physical_mappings
            WHERE source = 'dorispreview';
            """
        )
    )
    by_table: dict[tuple, dict] = {}
    for item in mappings:
        if item["level"] != "t":
            continue
        by_table[(item["gen"], item["table"])] = {"physical": item["physical"], "columns": {}}
    for item in mappings:
        if item["level"] != "c":
            continue
        for key, table in by_table.items():
            if table["physical"] == item["scope"] and key[1] == item["table"]:
                table["columns"][item["column"]] = item["physical"]

    generations = {1: tick_rows(ROOT / "capture-live" / "a1.json"), 2: tick_rows(ROOT / "ns2-live" / "ns2.json")}
    b3 = tick_rows(ROOT / "capture-live" / "b3.json")
    tables = ["dup_events", "agg_qty", "uniq_acct", "dec_exact", "part_day", "str_key", "nz_obs"]
    results = []
    mismatches = []
    for generation, rows_by_name in generations.items():
        for table in tables:
            label = f"g{generation}-{table}"
            stdout = json.loads((RAW / f"{label}.stdout").read_text())
            request = json.loads((REC / f"{label}.request.json").read_text())
            target = stdout["target_generation"]
            mapped = by_table[(generation, table)]
            logical = [column["name"] for column in request["columns"]]
            physical_cols = [mapped["columns"][name] for name in logical]
            type_sql = (
                "SELECT coalesce(jsonb_agg(jsonb_build_object("
                "'name', a.attname, 'oid', a.atttypid, 'typmod', a.atttypmod) "
                "ORDER BY a.attnum), '[]'::jsonb)::text "
                "FROM pg_attribute a "
                "JOIN pg_class c ON c.oid = a.attrelid "
                "JOIN pg_namespace n ON n.oid = c.relnamespace "
                f"WHERE n.nspname = '_oo' AND c.relname = '{mapped['physical']}' "
                "AND a.attnum > 0 AND NOT a.attisdropped;"
            )
            types = json.loads(psql(type_sql))
            type_by_name = {item["name"]: item for item in types}
            array = ", ".join(f"{name}::text" for name in physical_cols)
            row_sql = (
                f"SELECT coalesce(jsonb_agg(jsonb_build_array({array})), '[]'::jsonb)::text "
                f"FROM _oo.{mapped['physical']};"
            )
            stored = json.loads(psql(row_sql))
            expected = rows_by_name[table]
            same = norm(stored) == norm(expected)
            type_checks = []
            for spec in request["columns"]:
                got = type_by_name[mapped["columns"][spec["name"]]]
                got_oid = int(got["oid"])
                got_typmod = int(got["typmod"])
                if spec["arrow_type"] == "decimal128":
                    want = (1700, numeric_typmod(spec["precision"], spec["scale"]))
                else:
                    want = EXPECTED_OID[spec["arrow_type"]]
                type_ok = (got_oid, got_typmod) == want
                got["oid"] = got_oid
                got["typmod"] = got_typmod
                type_checks.append(
                    {
                        "column": spec["name"],
                        "oid": got["oid"],
                        "typmod": got["typmod"],
                        "expected_oid": want[0],
                        "expected_typmod": want[1],
                        "match": type_ok,
                    }
                )
                if not type_ok:
                    mismatches.append(f"{label} type {spec['name']}")
            if stdout["physical_name"] != mapped["physical"]:
                mismatches.append(f"{label} physical")
            if not same or stdout["row_count"] != len(expected):
                mismatches.append(f"{label} cells")
            results.append(
                {
                    "label": label,
                    "source_generation": generation,
                    "target_generation": target,
                    "physical_name": mapped["physical"],
                    "row_count": len(stored),
                    "stdout_row_count": stdout["row_count"],
                    "cell_diff": 0 if same else "mismatch",
                    "source_bound": stdout["source_bound"],
                    "load_state": stdout["load_state"],
                    "types": type_checks,
                }
            )

    def has_row(generation: int, table: str, wanted: list) -> bool:
        return wanted in generations[generation][table]

    # Re-read the stored generation that the script just compared.
    stored_by = {}
    for item in results:
        # The comparison already used stored rows. Pull them back from the tick equality flag.
        stored_by[item["label"]] = item["cell_diff"]

    checks = {
        "nz_gen1_contains_id3": ["3", "1.00"] in generations[1]["nz_obs"] or any(
            row[0] == "3" for row in generations[1]["nz_obs"]
        ),
        "nz_gen2_contains_id3": any(row[0] == "3" for row in generations[2]["nz_obs"]),
        "uniq_gen1_contains_id4": any(row[0] == "4" for row in generations[1]["uniq_acct"]),
        "uniq_gen2_contains_id4": any(row[0] == "4" for row in generations[2]["uniq_acct"]),
        "uniq_gen2_id2": next(row for row in generations[2]["uniq_acct"] if row[0] == "2"),
        "part_gen2_east": next(
            (row for row in generations[2]["part_day"] if row[0] == "2026-03-20" and row[1] == "east"),
            None,
        ),
        "dup_twin_gen1": sum(1 for row in generations[1]["dup_events"] if row == ["12", "twin", "6.50"]),
        "dup_twin_gen2": sum(1 for row in generations[2]["dup_events"] if row == ["12", "twin", "6.50"]),
        "ns2_equals_b3": all(norm(generations[2][table]) == norm(b3[table]) for table in tables),
    }
    # The id3/id4 checks above describe the tick. Confirm the stored side by cell_diff==0.
    ledger = {
        "migrations": psql(
            "SELECT version || '|' || checksum || '|' || filename FROM _oo.schema_migrations ORDER BY version;"
        ).strip().splitlines(),
        "freshness": json.loads(
            psql(
                """
                SELECT coalesce(jsonb_agg(jsonb_build_object(
                  'table', table_name, 'source_generation', source_generation,
                  'target_generation', target_generation, 'state', state, 'mode', mode,
                  'physical_name', physical_name
                ) ORDER BY table_name), '[]'::jsonb)::text FROM _oo.table_freshness;
                """
            )
        ),
        "watermark": json.loads(
            psql(
                """
                SELECT jsonb_build_object(
                  'source', source, 'grade', consistency_grade, 'state', state,
                  'cross_table', cross_table_consistent, 'source_generation', source_generation,
                  'target_generation', target_generation, 'codec', position_codec, 'lineage', position_lineage
                )::text FROM _oo.source_watermark;
                """
            )
        ),
        "quality": json.loads(
            psql(
                """
                SELECT coalesce(jsonb_agg(jsonb_build_object(
                  'table', table_name, 'capture', capture_method, 'delete', delete_detection,
                  'reconcile', reconcile_status, 'diff', reconcile_diff_rows
                ) ORDER BY table_name), '[]'::jsonb)::text FROM _oo.table_quality;
                """
            )
        ),
        "batches": json.loads(
            psql(
                """
                SELECT coalesce(jsonb_agg(jsonb_build_object(
                  'table', table_name, 'batch', batch_id, 'source_generation', source_generation,
                  'target_generation', target_generation, 'rows', row_count, 'tombs', tombstone_count
                ) ORDER BY target_generation), '[]'::jsonb)::text FROM _oo.update_batch;
                """
            )
        ),
        "tombstones": int(psql("SELECT count(*) FROM _oo.update_tombstone;").strip()),
        "mirrors": int(psql("SELECT count(*) FROM _oo.control_mirror;").strip()),
        "apply_receipts": int(psql("SELECT count(*) FROM _oo.apply_receipts;").strip()),
    }
    # Stored-side key checks, independent of the tick membership flags.
    def stored_rows(label: str) -> list:
        item = next(row for row in results if row["label"] == label)
        logical_request = json.loads((REC / f"{label}.request.json").read_text())
        names = [column["name"] for column in logical_request["columns"]]
        mapped = by_table[(item["source_generation"], label.split("-", 1)[1])]
        physical_cols = [mapped["columns"][name] for name in names]
        array = ", ".join(f"{name}::text" for name in physical_cols)
        return json.loads(
            psql(
                f"SELECT coalesce(jsonb_agg(jsonb_build_array({array})), '[]'::jsonb)::text "
                f"FROM _oo.{mapped['physical']};"
            )
        )

    nz1 = stored_rows("g1-nz_obs")
    nz2 = stored_rows("g2-nz_obs")
    u1 = stored_rows("g1-uniq_acct")
    u2 = stored_rows("g2-uniq_acct")
    p2 = stored_rows("g2-part_day")
    d1 = stored_rows("g1-dup_events")
    d2 = stored_rows("g2-dup_events")
    stored_checks = {
        "nz_gen1_id3": sum(1 for row in nz1 if row[0] == "3"),
        "nz_gen2_id3": sum(1 for row in nz2 if row[0] == "3"),
        "uniq_gen1_id4": sum(1 for row in u1 if row[0] == "4"),
        "uniq_gen2_id4": sum(1 for row in u2 if row[0] == "4"),
        "uniq_gen2_id2": next(row for row in u2 if row[0] == "2"),
        "part_gen2_20260320_east": next(
            (row for row in p2 if row[0] == "2026-03-20" and row[1] == "east"),
            None,
        ),
        "dup_twin_gen1": sum(1 for row in d1 if row == ["12", "twin", "6.50"]),
        "dup_twin_gen2": sum(1 for row in d2 if row == ["12", "twin", "6.50"]),
    }
    report = {
        "cell_mismatch_labels": mismatches,
        "tables": results,
        "tick_checks": checks,
        "stored_checks": stored_checks,
        "ledger": ledger,
    }
    out = RAW / "cell-oracle.json"
    out.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    print("wrote", out)
    print("mismatches", mismatches)
    print("stored", json.dumps(stored_checks, ensure_ascii=False))
    print("ns2_equals_b3", checks["ns2_equals_b3"])
    print("batches", len(ledger["batches"]), "tombs", ledger["tombstones"], "receipts", ledger["apply_receipts"])
    if mismatches:
        raise SystemExit(1)
    if stored_checks["nz_gen1_id3"] != 1 or stored_checks["nz_gen2_id3"] != 0:
        raise SystemExit("nz id3")
    if stored_checks["uniq_gen1_id4"] != 0 or stored_checks["uniq_gen2_id4"] != 0:
        raise SystemExit("uniq id4")
    if stored_checks["uniq_gen2_id2"] != ["2", "3.5000", "cycle2"]:
        raise SystemExit("uniq id2")
    if stored_checks["part_gen2_20260320_east"] != ["2026-03-20", "east", "1.50"]:
        raise SystemExit("part east")
    if stored_checks["dup_twin_gen1"] != 2 or stored_checks["dup_twin_gen2"] != 2:
        raise SystemExit("dup twin")


if __name__ == "__main__":
    main()
