#!/usr/bin/env python3
"""Independent MariaDB text oracle. It does not read Arrow or the Rust driver."""

import json
import pathlib
import subprocess
import sys

CONTAINER = "oo-dl01w1-39-mariadb"

def sql_null(expr: str) -> str:
    """The text client prints SQL NULL as the word NULL. Mark it before that happens."""
    return f"IF(({expr}) IS NULL, '<SQLNULL>', {expr})"


TABLES = [
    ("k_binary", ["binary", "utf8"], f"SELECT {sql_null('HEX(id)')}, {sql_null('label')} FROM oo_synth.k_binary ORDER BY id"),
    ("k_collation", ["int32", "utf8", "utf8"], f"SELECT {sql_null('id')}, {sql_null('bin_text')}, {sql_null('uca_text')} FROM oo_synth.k_collation ORDER BY id"),
    ("k_decimal", ["decimal128(38,10)", "decimal256(65,30)"], f"SELECT {sql_null('id')}, {sql_null('payload')} FROM oo_synth.k_decimal ORDER BY id"),
    ("k_empty", ["int32", "utf8", "binary"], f"SELECT {sql_null('id')}, {sql_null('note')}, {sql_null('HEX(blob_key)')} FROM oo_synth.k_empty ORDER BY id"),
    (
        "k_json",
        ["int64", "utf8"],
        f"SELECT {sql_null('id')}, {sql_null('doc')} FROM oo_synth.k_json ORDER BY id",
    ),
    (
        "k_null",
        ["int64", "utf8", "int32", "decimal128(12,2)", "utf8", "timestamp_us_naive", "binary"],
        "SELECT "
        + ", ".join([
            sql_null("id"),
            sql_null("note"),
            sql_null("qty"),
            sql_null("money"),
            sql_null("payload"),
            sql_null("DATE_FORMAT(when_at, '%Y-%m-%d %H:%i:%s.%f')"),
            sql_null("HEX(blob_key)"),
        ])
        + " FROM oo_synth.k_null ORDER BY id",
    ),
    ("k_null_unique", ["int64", "utf8"], f"SELECT {sql_null('id')}, {sql_null('code')} FROM oo_synth.k_null_unique ORDER BY id"),
    (
        "k_temporal",
        ["timestamp_us_naive", "date32", "int64_interval_us", "timestamp_us_+00:00", "utf8"],
        "SELECT "
        + ", ".join([
            sql_null("DATE_FORMAT(id, '%Y-%m-%d %H:%i:%s.%f')"),
            sql_null("DATE_FORMAT(d, '%Y-%m-%d')"),
            sql_null("t"),
            sql_null("DATE_FORMAT(ts, '%Y-%m-%d %H:%i:%s.%f')"),
            sql_null("note"),
        ])
        + " FROM oo_synth.k_temporal ORDER BY id",
    ),
    ("k_unsigned", ["uint64", "utf8"], f"SELECT {sql_null('id')}, {sql_null('label')} FROM oo_synth.k_unsigned ORDER BY id"),
    ("k_zero_probe", ["int32", "date32"], "SELECT " + sql_null("id") + ", " + sql_null("DATE_FORMAT(d, '%Y-%m-%d')") + " FROM oo_synth.k_zero_probe ORDER BY id"),
]


def client(sql: str) -> str:
    result = subprocess.run(
        [
            "docker", "exec", "-i", CONTAINER, "mariadb",
            "--defaults-extra-file=/run/secrets/reader.cnf",
            "--protocol=socket",
            "--socket=/run/mysqld/mysqld.sock",
            "--default-character-set=utf8mb4",
            "--batch", "--raw", "--skip-column-names",
            "oo_synth",
        ],
        input=sql,
        text=True,
        capture_output=True,
    )
    if result.returncode != 0:
        sys.stderr.write(result.stderr)
        raise SystemExit(result.returncode)
    return result.stdout


def parse_rows(text: str) -> list:
    rows = []
    for line in text.splitlines():
        fields = []
        for field in line.split("\t"):
            fields.append(None if field in (r"\N", "<SQLNULL>") else field)
        rows.append(fields)
    return rows


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: oracle.py OUT_DIR", file=sys.stderr)
        return 2
    out = pathlib.Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    tables = []
    for name, arrow, sql in TABLES:
        rows = parse_rows(client(sql))
        tables.append({"name": name, "columns": [], "arrow": arrow, "rows": rows})
    document = {"protocol": "oo.mariadb-driver-preflight.rows/v1", "tables": tables}
    (out / "oracle.json").write_text(json.dumps(document, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    kinds = parse_rows(client(
        "SELECT id, "
        + sql_null("JSON_TYPE(doc)")
        + ", "
        + sql_null("JSON_VALID(doc)")
        + " FROM oo_synth.k_json ORDER BY id"
    ))
    identity = client(
        "SELECT CONCAT_WS('|', @@version, @@version_comment, @@sql_mode, @@time_zone, @@log_bin+0, @@gtid_domain_id, @@gtid_current_pos, @@collation_server)"
    ).strip()
    (out / "oracle-checks.json").write_text(
        json.dumps({"json_kinds": kinds, "identity": identity}, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )
    print(f"oracle_tables={len(tables)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
