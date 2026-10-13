#!/usr/bin/env python3
"""Grant the preview login the five approved customer columns, including ts."""

import json
import subprocess
from pathlib import Path

BASE = Path("/tmp/oo-dl01w1-40-bind")
OUT = BASE / "out" / "raw"
COLUMNS = ["reading_id", "ts", "amount", "note", "qty"]


def psql(sql: str, name: str) -> subprocess.CompletedProcess[str]:
    proc = subprocess.run(
        [
            "docker",
            "exec",
            "-i",
            "oo-dl01w1-40-pg",
            "psql",
            "-U",
            "oo_admin",
            "-d",
            "previewdb",
            "-v",
            "ON_ERROR_STOP=1",
            "-X",
            "-A",
            "-t",
            "-q",
        ],
        input=sql,
        text=True,
        capture_output=True,
    )
    (OUT / f"{name}.stdout").write_text(proc.stdout)
    (OUT / f"{name}.stderr").write_text(proc.stderr)
    (OUT / f"{name}.exit").write_text(f"{proc.returncode}\n")
    return proc


def ident(value: str, prefix: str) -> str:
    rest = value.removeprefix(prefix)
    if not value.startswith(prefix) or len(value) != 63 or len(rest) != 60:
        raise SystemExit(f"unexpected physical name {value}")
    if any(char not in "0123456789abcdef" for char in rest):
        raise SystemExit(f"unexpected physical name {value}")
    return '"' + value + '"'


def main() -> None:
    policy = json.loads((BASE / "out" / "builder" / "policy.json").read_text())
    if policy["project"] != "clickhouse_268" or policy["login"] != "oo_preview_login":
        raise SystemExit("policy identity is not the customer binding")
    # sealed.rs stores the visibility slug in policy.role and the full oo_r_* role on the authority.
    authority = json.loads((BASE / "out" / "builder" / "authority.json").read_text())
    if authority.get("role") != "oo_r_reader" or policy.get("role") != "reader":
        raise SystemExit(
            f"role pack is authority={authority.get('role')} policy={policy.get('role')}"
        )
    names = [column["name"] for column in policy["table"]["columns"]]
    if names != COLUMNS:
        raise SystemExit(f"policy columns are {names}")
    digest = policy["config_digest"]
    if len(digest) != 64 or any(char not in "0123456789abcdef" for char in digest):
        raise SystemExit("config digest is not hex")
    catalog = json.loads((BASE / "plan" / "catalog.json").read_text())
    if digest != catalog["config_digest"]:
        raise SystemExit("policy config digest is not the customer catalog")
    listed = psql(
        f"""
        SELECT level || '|' || logical_column || '|' || physical_name
        FROM _oo.physical_mappings
        WHERE source = 'clickhouse_268'
          AND config_digest = '{digest}'
          AND run_generation = 1
          AND logical_namespace = 'readings'
          AND logical_table = 'final'
        ORDER BY 1;
        """,
        "provision-map",
    )
    if listed.returncode != 0:
        raise SystemExit(listed.stderr)
    table = None
    granted = {}
    for line in listed.stdout.splitlines():
        if not line.strip():
            continue
        level, logical, physical = line.split("|", 2)
        if level == "t" and logical == "":
            table = physical
        elif level == "c":
            granted[logical] = physical
    if table is None or set(granted) != set(COLUMNS):
        raise SystemExit(f"mapping set table={table} columns={sorted(granted)}")
    table_sql = ident(table, "pt_")
    column_sql = ", ".join(ident(granted[name], "pc_") for name in COLUMNS)
    created = psql(
        f"""
        DO $body$
        DECLARE
          pw text := btrim(pg_read_file('/var/lib/postgresql/login_password'), chr(13) || chr(10));
        BEGIN
          IF pw IS NULL OR length(pw) <> 48 OR pw !~ '^[0-9a-f]+$' THEN
            RAISE EXCEPTION 'login password file is unusable';
          END IF;
          EXECUTE format(
            'CREATE ROLE oo_preview_login LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION PASSWORD %L',
            pw
          );
          EXECUTE 'CREATE ROLE oo_r_reader NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION';
        END
        $body$;
        GRANT CONNECT ON DATABASE previewdb TO oo_preview_login;
        GRANT USAGE ON SCHEMA _oo TO oo_preview_login;
        GRANT SELECT ON TABLE _oo.physical_mappings TO oo_preview_login;
        GRANT EXECUTE ON FUNCTION _oo.lookup_physical_mapping(text, text, bigint, text, text, text, text) TO oo_preview_login;
        GRANT pg_read_all_stats TO oo_preview_login WITH INHERIT TRUE;
        GRANT USAGE ON SCHEMA _oo TO oo_r_reader;
        GRANT SELECT ({column_sql}) ON TABLE _oo.{table_sql} TO oo_r_reader;
        GRANT oo_r_reader TO oo_preview_login WITH INHERIT FALSE, SET TRUE;
        """,
        "provision",
    )
    if created.returncode != 0:
        raise SystemExit(created.stderr)
    (OUT / "PROVISION_OK").write_text(table + "\n")
    print("PROVISION_OK")


if __name__ == "__main__":
    main()
