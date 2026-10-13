#!/usr/bin/env python3
"""Independent unsampled native-client versus physical PostgreSQL typed cells."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[3]
OUT = ROOT / "evidence/2c023948-0862-4dae-ae8f-6fc30241db62/DL01/starrocks-product/raw/native-sync"
ORACLE = ROOT / "testing/dl01-driver-preflight/starrocks/scripts/oracle.py"
QUERIES = {
    "dup_events": "SELECT event_id,bucket,seen,amount,note FROM dup_events",
    "pk_accounts": "SELECT account_id,balance,nickname,opened FROM pk_accounts",
    "agg_metrics": "SELECT metric_day,metric_name,qty,revenue FROM agg_metrics",
    "uniq_dim": "SELECT dim_id,label,score FROM uniq_dim",
    "dup_partition_sums": "SELECT CASE WHEN bucket < '2026-06-01' THEN 'p202605' ELSE 'p202606' END,COUNT(*),CAST(SUM(amount) AS DECIMAL(38,4)) FROM dup_events GROUP BY 1",
}


def run(directory, label, command, sql):
    result = subprocess.run(command, input=sql.encode(), capture_output=True, timeout=120)
    for suffix, value in (("stdout", result.stdout), ("stderr", result.stderr), ("exit", (str(result.returncode) + "\n").encode())):
        (directory / (label + "." + suffix)).write_bytes(value)
    if result.returncode:
        raise RuntimeError(label + " failed; raw exit retained")
    return result.stdout.decode()


def physical(value):
    if not re.fullmatch(r"(?:pt|pc|pn)_[0-9a-f]{60}", value):
        raise RuntimeError("unexpected physical identifier")
    return '"' + value + '"'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--phase", required=True)
    parser.add_argument("--capture-only", action="store_true")
    args = parser.parse_args()
    if not re.fullmatch(r"[a-z0-9_-]{1,48}", args.phase):
        raise RuntimeError("phase identifier")
    directory = OUT / "independent" / args.phase
    directory.mkdir(parents=True, exist_ok=False)
    spec = importlib.util.spec_from_file_location("oracle", ORACLE)
    oracle = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(oracle)
    source = ["docker", "exec", "-i", "oo-dl01w1-45-native-sync", "mysql", "--defaults-file=/tmp/oo-secrets/reader.cnf", "--batch", "--raw", "--skip-column-names", "--default-character-set=utf8mb4"]
    target = ["docker", "exec", "-i", "oo-dl01w1-45-native-pg", "psql", "-X", "-v", "ON_ERROR_STOP=1", "-U", "postgres", "-d", "oo_w45_native_live", "-At"]
    comparisons = {}
    for table, sql in QUERIES.items():
        expected = oracle.parse_tsv(table, run(directory, table + "-source", source, "USE oo_w45_sr;\n" + sql + ";"))
        entry = {"expected": expected, "expected_rows": len(expected), "oracle_sha256": hashlib.sha256(ORACLE.read_bytes()).hexdigest()}
        if not args.capture_only:
            lookup = "SELECT run_generation,logical_column,physical_name,level FROM _oo.physical_mappings WHERE source='sr_w45_native' AND logical_namespace='oo_w45_sr' AND logical_table='" + table + "' AND run_generation=(SELECT max(run_generation) FROM _oo.physical_mappings WHERE source='sr_w45_native' AND logical_namespace='oo_w45_sr' AND logical_table='" + table + "') ORDER BY level,logical_column;"
            rows = [line.split('|') for line in run(directory, table + "-mapping", target, lookup).splitlines()]
            tables = [r for r in rows if len(r) == 4 and r[3] == 't']
            if len(tables) != 1:
                raise RuntimeError(table + " latest table mapping absent")
            table_name = physical(tables[0][2])
            columns = {r[1]: physical(r[2]) for r in rows if len(r) == 4 and r[3] == 'c'}
            if set(columns) != set(oracle.COLUMNS[table]):
                raise RuntimeError(table + " logical column mapping mismatch")
            select = ','.join(columns[c] + '::text' for c in oracle.COLUMNS[table])
            text = run(directory, table + "-target", target, "SELECT COALESCE(jsonb_agg(v ORDER BY v::text),'[]'::jsonb)::text FROM (SELECT jsonb_build_array(" + select + ") v FROM _oo." + table_name + ") cells;")
            got = [oracle.norm_row(table, dict(zip(oracle.COLUMNS[table], row))) for row in json.loads(text)]
            entry.update({"target": got, "target_rows": len(got), "source_generation": int(tables[0][0]), "full_typed_cells_equal": oracle.canon(expected) == oracle.canon(got), "mismatches": [] if oracle.canon(expected) == oracle.canon(got) else {"source": oracle.canon(expected), "target": oracle.canon(got)}})
            run(directory, table + "-target-types", target, "SELECT attname,format_type(atttypid,atttypmod),attnotnull FROM pg_attribute WHERE attrelid='_oo." + table_name.replace('"', '') + "'::regclass AND attnum>0 AND NOT attisdropped ORDER BY attnum;")
        comparisons[table] = entry
    (directory / "compare.json").write_text(json.dumps({"protocol": "oo.starrocks.independent-full-cells/v1", "sampled": False, "capture_only": args.capture_only, "tables": comparisons, "all_equal": None if args.capture_only else all(c["full_typed_cells_equal"] for c in comparisons.values())}, indent=2) + "\n")
    print("native source full approved cells captured" if args.capture_only else "full typed diff0=" + str(all(c["full_typed_cells_equal"] for c in comparisons.values())))
    return 0 if args.capture_only or all(c["full_typed_cells_equal"] for c in comparisons.values()) else 1


if __name__ == "__main__":
    raise SystemExit(main())
