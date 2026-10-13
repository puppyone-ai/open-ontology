"""Live Oracle C5 capture against the already running non-production instance.

Does not start or replace a container. The platform session is OO04_READER
and is SELECT only. SYSTEM is the existing test DBA and is not SYS.
"""

from __future__ import annotations

import json
import re
import subprocess
import time
from pathlib import Path

import c5_capture as c5
import customer_input

ROOT = Path("/Users/redux/puppy-one/open-ontology")
EVIDENCE = ROOT / "evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/c5-capture"
ORACLE = "oo-dl01w1-04-p2-oracle"
ORACLE_ID = "ffc81586acf3"
PG = "oo-dl01w1-04-p2-pg"
PG_ID = "eb0ec24b748e"
CUSTOMER = Path("/Users/redux/.oo-dl01w1-04-p2/customer-product-window.json")
REGISTRY = Path("/Users/redux/.oo-dl01w1-04-p2/secret-registry.json")
DBA_PASSWORD = Path("/Users/redux/.oo-dl01w1-04-p2/oracle-passwd")
PHYSICAL = (
    "_oo.pt_29e0abf4c6c050b42b133f36a57162d328cb046d09fd56b26d6cf88a173a",
    "_oo.pt_90c1b24d264fc3d5e8dec31cae090d19d3eb4e716618e2832f0c1e5b8c78",
    "_oo.pt_5de9270b8e813035498c5d2d9a53c3e6d861541805bb3f7f56f4fe9e36f3",
)
BASE_COLUMNS = {
    "T_BATCH": ["ID", "N16", "N1", "D", "C6", "V2", "R"],
    "T_WIDE": ["ID", "N_FREE"],
    "T_TS": ["ID", "TS", "TSTZ"],
}
PROOF_DEFAULT = {
    "detect_period_seconds": 2,
    "pull_period_seconds": 2,
    "reconcile_period_seconds": 2,
    "delete_visibility_sla_seconds": 86400,
}
PROOF_CHANGED = {
    "detect_period_seconds": 3,
    "pull_period_seconds": 4,
    "reconcile_period_seconds": 5,
    "delete_visibility_sla_seconds": 86400,
}
HEAD = (
    "WHENEVER SQLERROR EXIT SQL.SQLCODE\n"
    "SET HEADING OFF FEEDBACK OFF PAGESIZE 0 LINESIZE 4000 TRIMSPOOL ON "
    "TAB OFF VERIFY OFF ECHO OFF WRAP OFF SERVEROUTPUT OFF\n"
)
STAMP = re.compile(r"\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{6}\Z")


def run_cmd(args, timeout=60, stdin=None):
    return subprocess.run(args, input=stdin, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout)


def scrub(payload, secrets):
    for secret in secrets:
        if secret:
            payload = payload.replace(secret, b"[redacted]")
    return payload


def require_running(name, expected_id):
    proc = run_cmd(["docker", "inspect", name])
    if proc.returncode != 0:
        raise SystemExit(f"{name} is absent; not creating it")
    data = json.loads(proc.stdout)[0]
    if data["Id"][:12] != expected_id:
        raise SystemExit(f"{name} id changed; not adopting it")
    if not data["State"]["Running"]:
        raise SystemExit(f"{name} is not running; not starting it")
    labels = (data.get("Config") or {}).get("Labels") or {}
    if labels.get("oo.owner") != "dl01w1-04":
        raise SystemExit(f"{name} owner is not dl01w1-04")
    return {
        "name": name,
        "id": data["Id"],
        "running": True,
        "started_at": data["State"].get("StartedAt"),
        "status": data["State"].get("Status"),
    }


def pg_tables():
    sql = "SELECT n.nspname || '.' || c.relname FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace WHERE c.relkind = 'r' AND c.relname LIKE 'pt_%' ORDER BY 1;"
    proc = run_cmd(["docker", "exec", PG, "psql", "-U", "postgres", "-d", "previewdb", "-Atc", sql])
    if proc.returncode != 0:
        raise SystemExit("pg read failed")
    found = [line.strip() for line in proc.stdout.decode().splitlines() if line.strip()]
    missing = [name for name in PHYSICAL if name not in found]
    if missing:
        raise SystemExit("sealed physical table missing")
    return found


def sqlplus(user, password, sql, name, sql_dir, secrets):
    c5.refuse_forbidden_sql(sql)
    path = sql_dir / f"{name}.sql"
    path.write_text(sql)
    if not user.replace("_", "").isalnum() or any(ch in password for ch in "\r\n"):
        raise c5.CaptureError("connection_identity")
    # CONNECT travels on stdin. Source passwords must never enter process argv.
    script = 'exec sqlplus -s /nolog'
    connect = 'CONNECT ' + user + '/"' + password.replace('"', '""') + '"@//127.0.0.1:1521/FREEPDB1\n'
    proc = run_cmd(
        ["docker", "exec", "-i", "-e", "NLS_LANG=AMERICAN_AMERICA.AL32UTF8", ORACLE, "bash", "-lc", script],
        stdin=(connect + sql).encode(),
        timeout=90,
    )
    stdout = scrub(proc.stdout, secrets)
    stderr = scrub(proc.stderr, secrets)
    (sql_dir / f"{name}.stdout").write_bytes(stdout)
    (sql_dir / f"{name}.stderr").write_bytes(stderr)
    (sql_dir / f"{name}.exit").write_text(f"{proc.returncode}\n")
    text = stdout.decode("utf-8", "replace")
    return proc.returncode, text


def lines(text, prefix):
    return [line.strip()[len(prefix):] for line in text.splitlines() if line.strip().startswith(prefix)]


def one(text, prefix):
    found = lines(text, prefix)
    if len(found) != 1:
        raise c5.CaptureError(f"expected one {prefix}")
    return found[0]


def publish_scn(password, sql_dir, secrets, name):
    code, text = sqlplus(
        "system",
        password,
        HEAD + "SELECT 'SCN|' || current_scn FROM v$database;\n"
        "SELECT 'USER|' || SYS_CONTEXT('USERENV','SESSION_USER') FROM dual;\n"
        "SELECT 'LOG|' || log_mode FROM v$database;\n"
        "SELECT 'SUP|' || supplemental_log_data_min FROM v$database;\nEXIT\n",
        name,
        sql_dir,
        secrets,
    )
    if code != 0:
        raise c5.CaptureError("scn_publish_failed")
    if one(text, "USER|") != "SYSTEM":
        raise c5.CaptureError("writer_is_not_system")
    scn = one(text, "SCN|")
    if not scn.isdigit():
        raise c5.CaptureError("scn")
    return scn, one(text, "LOG|"), one(text, "SUP|")


def reader_dictionary(password, sql_dir, secrets, name):
    code, text = sqlplus(
        "OO04_READER",
        password,
        HEAD + "SELECT 'USER|' || SYS_CONTEXT('USERENV','SESSION_USER') FROM dual;\n"
        "SELECT 'CON|' || SYS_CONTEXT('USERENV','CON_NAME') FROM dual;\n"
        "SELECT 'COL|' || table_name || '|' || column_id || '|' || column_name || '|' || data_type || '|' || "
        "NVL(TO_CHAR(data_precision),'-') || '|' || NVL(TO_CHAR(data_scale),'-') || '|' || "
        "NVL(TO_CHAR(char_length),'-') || '|' || nullable\n"
        "FROM all_tab_columns WHERE owner = 'OO04_FIX' AND table_name IN ('T_BATCH','T_WIDE','T_TS') "
        "ORDER BY table_name, column_id;\nEXIT\n",
        name,
        sql_dir,
        secrets,
    )
    if code != 0 or one(text, "USER|") != "OO04_READER" or one(text, "CON|") != "FREEPDB1":
        raise c5.CaptureError("reader_dictionary_failed")
    columns = {}
    for row in lines(text, "COL|"):
        table, position, name, data_type, precision, scale, length, nullable = row.split("|")
        item = {
            "table": table,
            "position": int(position),
            "name": name,
            "data_type": data_type,
            "precision": None if precision == "-" else int(precision),
            "scale": None if scale == "-" else int(scale),
            "char_length": None if length == "-" else int(length),
            "nullable": nullable,
            "flags": c5.column_flags(
                data_type,
                None if precision == "-" else int(precision),
                None if scale == "-" else int(scale),
            ),
        }
        columns.setdefault(table, []).append(item)
    return columns


def batch_select(scn, with_note, historical=True):
    note = " || '|' || NVL(DUMP(c5_note, 16), '<null>')" if with_note else ""
    clause = f" AS OF SCN {scn}" if historical else ""
    return (
        f"SELECT 'B|' || id || '|' || NVL(TO_CHAR(n16, 'FM9999999990.000000'), '<null>') || '|' || "
        f"NVL(TO_CHAR(n1), '<null>') || '|' || NVL(TO_CHAR(d, 'YYYY-MM-DD\"T\"HH24:MI:SS'), '<null>') || '|' || "
        f"NVL(DUMP(c6, 16), '<null>') || '|' || NVL(DUMP(v2, 16), '<null>') || '|' || "
        f"NVL(RAWTOHEX(r), '<null>'){note} || '|END'\n"
        f"FROM oo04_fix.t_batch{clause} ORDER BY id;\n"
    )


def pull_sql(scn, with_note, historical=True):
    if historical and not str(scn).isdigit():
        raise c5.CaptureError("scn")
    wide = f" AS OF SCN {scn}" if historical else ""
    ts = f" AS OF SCN {scn}" if historical else ""
    return (
        HEAD
        + batch_select(scn, with_note, historical)
        + "SELECT 'W|' || id || '|' || NVL(TO_CHAR(n_free, 'TM9'), '<null>') || '|END'\n"
        + f"FROM oo04_fix.t_wide{wide} ORDER BY id;\n"
        + "SELECT 'T|' || id || '|' || TO_CHAR(ts, 'YYYY-MM-DD\"T\"HH24:MI:SS.FF9') || '|' || "
        + "TO_CHAR(tstz, 'YYYY-MM-DD\"T\"HH24:MI:SS.FF9 TZH:TZM') || '|END'\n"
        + f"FROM oo04_fix.t_ts{ts} ORDER BY id;\nEXIT\n"
    )


def cell_or_null(text):
    return None if text == "<null>" else text


def parse_pull(text, with_note):
    batch = {}
    wide = {}
    ts = {}
    for row in lines(text, "B|"):
        parts = row.split("|")
        expect = 9 if with_note else 8
        if len(parts) != expect or parts[-1] != "END":
            raise c5.CaptureError("batch_row")
        ident, n16, n1, stamp, c6, v2, raw = parts[:7]
        cells = {
            "ID": int(ident),
            "N16": cell_or_null(n16),
            "N1": None if n1 == "<null>" else int(n1),
            "D": cell_or_null(stamp),
            "C6": c5.normalize_text_cell("CHAR", c5.parse_dump(c6)),
            "V2": c5.normalize_text_cell("VARCHAR2", c5.parse_dump(v2)),
            "R": cell_or_null(raw),
        }
        if with_note:
            cells["C5_NOTE"] = c5.normalize_text_cell("VARCHAR2", c5.parse_dump(parts[7]))
        batch[ident] = cells
    for row in lines(text, "W|"):
        ident, number, end = row.split("|")
        if end != "END":
            raise c5.CaptureError("wide_row")
        wide[ident] = {"ID": int(ident), "N_FREE": cell_or_null(number)}
    for row in lines(text, "T|"):
        ident, ts_text, tstz, end = row.split("|")
        if end != "END":
            raise c5.CaptureError("ts_row")
        ts[ident] = {"ID": int(ident), "TS": ts_text, "TSTZ": tstz}
    if not batch or not wide or not ts:
        raise c5.CaptureError("empty_pull")
    return {"T_BATCH": batch, "T_WIDE": wide, "T_TS": ts}


def write_arrow_request(directory, table, generation, columns, rows):
    spec_columns = []
    encoded_rows = []
    for column in columns:
        if column == "ID":
            spec_columns.append({"name": "ID", "type": "int16", "nullable": False})
        elif column == "N16":
            spec_columns.append({"name": "N16", "type": "decimal128", "precision": 16, "scale": 6, "nullable": True})
        elif column == "N1":
            spec_columns.append({"name": "N1", "type": "int8", "nullable": True})
        elif column == "D":
            spec_columns.append({"name": "D", "type": "timestamp_us", "nullable": True})
        elif column == "R":
            spec_columns.append({"name": "R", "type": "binary", "nullable": True})
        else:
            spec_columns.append({"name": column, "type": "utf8", "nullable": True})
    for key in sorted(rows, key=lambda item: int(item)):
        encoded = {}
        for column in [item["name"] for item in spec_columns]:
            value = rows[key][column]
            if column == "R" and isinstance(value, str):
                value = value.upper()
            encoded[column] = value
        encoded_rows.append(encoded)
    body = {
        "table": table,
        "generation": generation,
        "columns": spec_columns,
        "rows": encoded_rows,
        "schema_digest": None,
        "admitted": False,
    }
    # The encoder refuses a schema_digest field. Keep it out of the file it reads.
    encoder_body = {key: value for key, value in body.items() if key != "schema_digest"}
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / f"{table.lower()}-gen-{generation}.arrow.json"
    path.write_text(json.dumps(encoder_body, indent=2, sort_keys=True) + "\n")
    return str(path)


def commit_tables(root, previous, generation, scn, pulled, approved, quality):
    committed = {}
    for table, rows in pulled.items():
        columns = approved[table]
        prior = None if previous is None else previous[table]
        result = c5.apply_snapshot(
            root / table,
            prior,
            generation,
            f"oo-dl01w1-04-scn-{scn}",
            columns,
            columns,
            rows,
            8,
            snapshot_token=True,
            quality=quality,
        )
        if not result["committed"]:
            raise c5.CaptureError(result["state"])
        committed[table] = c5.load_generation(root / table, generation)
        if table == "T_BATCH" or generation == 1:
            write_arrow_request(root / "arrow-json", table, generation, columns, committed[table]["cells"])
    return committed


def dba(password, sql, name, sql_dir, secrets):
    code, text = sqlplus("system", password, HEAD + sql + "\nEXIT\n", name, sql_dir, secrets)
    if code != 0:
        raise c5.CaptureError(f"dba_{name}")
    if "USER|" in text and one(text, "USER|") != "SYSTEM":
        raise c5.CaptureError("writer_is_not_system")
    return text


def audit_stamp(password, sql_dir, secrets):
    text = dba(
        password,
        "SELECT 'STAMP|' || TO_CHAR(SYSTIMESTAMP, 'YYYY-MM-DD HH24:MI:SS.FF6') FROM dual;",
        "audit-stamp",
        sql_dir,
        secrets,
    )
    stamp = one(text, "STAMP|")
    if not STAMP.fullmatch(stamp):
        raise c5.CaptureError("stamp")
    return stamp


def loss_flags(dictionary):
    grouped = {}
    for table, columns in dictionary.items():
        for column in columns:
            for flag in column["flags"]:
                grouped.setdefault(flag, []).append(f"OO04_FIX.{table}.{column['name']}")
    for names in grouped.values():
        names.sort()
    return grouped


def write_producer(run, shadow, dictionary, final_scn):
    specs = (
        ("T_BATCH", "oo04_t_batch", 8),
        ("T_WIDE", "oo04_t_wide_text", 7),
        ("T_TS", "oo04_t_ts_text", 7),
    )
    tables = []
    for table, source_id, generation in specs:
        body = c5.load_generation(shadow / table, generation)
        tables.append({
            "source_id": source_id,
            "owner": "OO04_FIX",
            "table": table,
            "generation": body["generation"],
            "position": body["position"],
            "resumed": body["resumed"],
            "method": body["method"],
            "initial_load": body["initial_load"],
            "delete_detection": body["delete_detection"],
            "consistency_grade": body["consistency_grade"],
            "quality": body["quality"],
            "row_count": len(body["cells"]),
            "tombstones": body["tombstones"],
            "columns": dictionary[table],
            "shadow": f"shadow/{table}/gen-{generation}.json",
            "schema_digest": None,
            "arrow_sha256": None,
            "admitted": False,
        })
    catalog = {
        "protocol": "oo.oracle.c5-producer-catalog/v1",
        "source_engine": "oracle",
        "method": "C5",
        "initial_load": "shadow_diff",
        "delete_detection": "reconcile",
        "cdc_default": "off",
        "logminer_used": False,
        "xstream": "not_used",
        "native_loader": False,
        "admitted": False,
        "schema_digest": None,
        "fixed_importer_sha": None,
        "publication": "not_published",
        "source_acceptance": "not_claimed",
        "snapshot": {"kind": "token", "value": f"oo-dl01w1-04-scn-{final_scn}"},
        "scn": int(final_scn),
        "tables": tables,
    }
    profile = {
        "protocol": "oo.oracle.c5-producer-profile/v1",
        "source_engine": "oracle",
        "method": "C5",
        "cdc_default": "off",
        "logminer_used": False,
        "xstream": "not_used",
        "native_loader": False,
        "admitted": False,
        "schema_digest": None,
        "fixed_importer_sha": None,
        "publication": "not_published",
        "source_acceptance": "not_claimed",
        "consistency_grade_with_token": "S0",
        "consistency_grade_without_token": "B",
        "sealed_import_grade_requested": "S1",
        "sealed_import_claimed": False,
        "delete_detection": "reconcile",
        "delete_quality": "DELETES_LAGGED",
        "initial_load": "shadow_diff",
        "log_cdc": False,
        "offset_lost": False,
        "time_ambiguous": False,
        "loss_flags": loss_flags(dictionary),
        "columns": dictionary,
    }
    (run / "producer-catalog.json").write_text(json.dumps(catalog, indent=2, sort_keys=True) + "\n")
    (run / "producer-profile.json").write_text(json.dumps(profile, indent=2, sort_keys=True) + "\n")
    return catalog, profile


def audit_rows(password, sql_dir, secrets, stamp, user, prefix):
    code, text = sqlplus(
        "system",
        password,
        HEAD + "SELECT '" + prefix + "|' || action_name || '|' || return_code || '|' || "
        "NVL(object_schema,'-') || '|' || NVL(object_name,'-')\n"
        "FROM unified_audit_trail\n"
        f"WHERE dbusername = '{user}'\n"
        f"  AND event_timestamp >= TO_TIMESTAMP('{stamp}', 'YYYY-MM-DD HH24:MI:SS.FF6')\n"
        "ORDER BY event_timestamp;\nEXIT\n",
        f"audit-{user.lower()}",
        sql_dir,
        secrets,
    )
    if code != 0:
        raise c5.CaptureError("audit_read_failed")
    rows = []
    for line in lines(text, prefix + "|"):
        action, return_code, schema, name = line.split("|", 3)
        rows.append({
            "action": action,
            "return_code": int(return_code),
            "object_schema": schema,
            "object_name": name,
        })
    return rows


def main():
    evidence = EVIDENCE
    run = evidence / "run"
    sql_dir = run / "sql"
    shadow = run / "shadow"
    run.mkdir(parents=True, exist_ok=True)
    sql_dir.mkdir(parents=True, exist_ok=True)
    result = {
        "protocol": "oo.oracle.c5-live/v1",
        "method": "C5",
        "log_cdc": False,
        "native_loader": False,
        "admitted": False,
        "schema_digest": None,
        "fixed_importer_sha": None,
        "source_acceptance": "not_claimed",
        "docs_daily_seconds": c5.DOCS_DAILY_SECONDS,
        "docs_daily_waited": False,
    }
    secrets = []
    try:
        loaded = customer_input.load_customer(CUSTOMER, REGISTRY)
        if loaded["source"]["reader"] != "OO04_READER" or loaded["source"]["container"] != ORACLE:
            raise c5.CaptureError("customer_source")
        if loaded["budget"]["hard_limit"] != 8:
            raise c5.CaptureError("budget")
        reader_password = Path(loaded["_password_file"]).read_text().strip("\r\n")
        dba_password = DBA_PASSWORD.read_text().strip("\r\n")
        secrets.extend([reader_password.encode(), dba_password.encode()])
        result["oracle"] = require_running(ORACLE, ORACLE_ID)
        result["postgres"] = require_running(PG, PG_ID)
        result["physical_tables_before"] = pg_tables()
        result["periods"] = {
            "docs_default": c5.resolve_periods(None),
            "proof_default": c5.resolve_periods(PROOF_DEFAULT),
            "proof_changed": c5.resolve_periods(PROOF_CHANGED),
        }
        stamp = audit_stamp(dba_password, sql_dir, secrets)
        result["audit_from"] = stamp
        dictionary = reader_dictionary(reader_password, sql_dir, secrets, "dictionary-before")
        for table, expect in BASE_COLUMNS.items():
            names = [column["name"] for column in dictionary[table]]
            if names != expect:
                raise c5.CaptureError(f"dictionary_{table}")
        result["dictionary_before"] = dictionary
        default_periods = result["periods"]["proof_default"]
        changed_periods = result["periods"]["proof_changed"]
        waits = []
        scn, log_mode, supplemental = publish_scn(dba_password, sql_dir, secrets, "scn-baseline")
        result["log_mode"] = log_mode
        result["supplemental_log_data_min"] = supplemental
        waits.append({"group": "proof_default", "cycle": "baseline", **c5.wait_realtime(default_periods)})
        code, text = sqlplus("OO04_READER", reader_password, pull_sql(scn, False), "pull-baseline", sql_dir, secrets)
        if code != 0:
            raise c5.CaptureError("baseline_pull")
        baseline = parse_pull(text, False)
        if sorted(baseline["T_BATCH"]) != ["41", "42", "43", "44", "45"]:
            raise c5.CaptureError("baseline_keys")
        committed = commit_tables(shadow, None, 1, scn, baseline, BASE_COLUMNS, ["DELETES_LAGGED"])
        result["baseline_scn"] = scn
        result["baseline_batch_rows"] = len(baseline["T_BATCH"])
        waits.append({"group": "proof_default", "cycle": "replay", **c5.wait_realtime(default_periods)})
        scn_replay, _, _ = publish_scn(dba_password, sql_dir, secrets, "scn-replay")
        code, text = sqlplus("OO04_READER", reader_password, pull_sql(scn_replay, False), "pull-replay", sql_dir, secrets)
        if code != 0:
            raise c5.CaptureError("replay_pull")
        replay = parse_pull(text, False)
        replay_result = c5.apply_snapshot(
            shadow / "T_BATCH", committed["T_BATCH"], 2, f"oo-dl01w1-04-scn-{scn_replay}",
            BASE_COLUMNS["T_BATCH"], BASE_COLUMNS["T_BATCH"], replay["T_BATCH"], 8,
        )
        result["replay_events"] = replay_result["events"]
        if replay_result["events"]:
            raise c5.CaptureError("replay_not_empty")
        # The empty replay still commits an identical hash generation. Keep it.
        committed["T_BATCH"] = c5.load_generation(shadow / "T_BATCH", 2)
        for table in ("T_WIDE", "T_TS"):
            same = c5.apply_snapshot(
                shadow / table, committed[table], 2, f"oo-dl01w1-04-scn-{scn_replay}",
                BASE_COLUMNS[table], BASE_COLUMNS[table], replay[table], 8,
            )
            if same["events"]:
                raise c5.CaptureError(f"replay_{table}")
            committed[table] = c5.load_generation(shadow / table, 2)

        dba(
            dba_password,
            "INSERT INTO oo04_fix.t_batch (id, n16, n1, d, c6, v2, r) VALUES ("
            "91, 1.25, 1, TO_DATE('2026-10-12 03:04:05', 'YYYY-MM-DD HH24:MI:SS'), '', '', HEXTORAW('0A0B'));\n"
            "COMMIT;\nSELECT 'USER|' || SYS_CONTEXT('USERENV','SESSION_USER') FROM dual;",
            "dba-insert",
            sql_dir,
            secrets,
        )
        waits.append({"group": "proof_changed", "cycle": "insert", **c5.wait_realtime(changed_periods)})
        scn, _, _ = publish_scn(dba_password, sql_dir, secrets, "scn-insert")
        code, text = sqlplus("OO04_READER", reader_password, pull_sql(scn, False), "pull-insert", sql_dir, secrets)
        if code != 0:
            raise c5.CaptureError("insert_pull")
        inserted = parse_pull(text, False)
        if inserted["T_BATCH"]["91"]["C6"] is not None or inserted["T_BATCH"]["91"]["V2"] is not None:
            raise c5.CaptureError("empty_string_not_null")
        gen = 3
        inserted_commit = {}
        for table, rows in inserted.items():
            applied = c5.apply_snapshot(
                shadow / table, committed[table], gen, f"oo-dl01w1-04-scn-{scn}",
                BASE_COLUMNS[table], BASE_COLUMNS[table], rows, 8,
            )
            inserted_commit[table] = applied
            if table != "T_BATCH" and applied["events"]:
                raise c5.CaptureError(f"insert_{table}")
            committed[table] = c5.load_generation(shadow / table, gen)
        result["insert_events"] = inserted_commit["T_BATCH"]["events"]
        if result["insert_events"] != [{"op": "insert", "key": "91"}]:
            raise c5.CaptureError("insert_event")
        write_arrow_request(shadow / "arrow-json", "T_BATCH", gen, BASE_COLUMNS["T_BATCH"], committed["T_BATCH"]["cells"])

        dba(
            dba_password,
            "UPDATE oo04_fix.t_batch SET n16 = 3.5, v2 = 'c5' WHERE id = 91;\nCOMMIT;\n"
            "SELECT 'USER|' || SYS_CONTEXT('USERENV','SESSION_USER') FROM dual;",
            "dba-update",
            sql_dir,
            secrets,
        )
        waits.append({"group": "proof_changed", "cycle": "update", **c5.wait_realtime(changed_periods)})
        scn, _, _ = publish_scn(dba_password, sql_dir, secrets, "scn-update")
        code, text = sqlplus("OO04_READER", reader_password, pull_sql(scn, False), "pull-update", sql_dir, secrets)
        if code != 0:
            raise c5.CaptureError("update_pull")
        updated = parse_pull(text, False)
        gen = 4
        update_events = None
        for table, rows in updated.items():
            applied = c5.apply_snapshot(
                shadow / table, committed[table], gen, f"oo-dl01w1-04-scn-{scn}",
                BASE_COLUMNS[table], BASE_COLUMNS[table], rows, 8,
            )
            if table == "T_BATCH":
                update_events = applied["events"]
            elif applied["events"]:
                raise c5.CaptureError(f"update_{table}")
            committed[table] = c5.load_generation(shadow / table, gen)
        result["update_events"] = update_events
        if update_events != [{"op": "update", "key": "91"}]:
            raise c5.CaptureError("update_event")
        if updated["T_BATCH"]["91"]["V2"] != "c5":
            raise c5.CaptureError("update_value")

        dba(
            dba_password,
            "DELETE FROM oo04_fix.t_batch WHERE id = 91;\nCOMMIT;\n"
            "SELECT 'USER|' || SYS_CONTEXT('USERENV','SESSION_USER') FROM dual;",
            "dba-delete",
            sql_dir,
            secrets,
        )
        waits.append({"group": "proof_changed", "cycle": "delete", **c5.wait_realtime(changed_periods)})
        scn, _, _ = publish_scn(dba_password, sql_dir, secrets, "scn-delete")
        code, text = sqlplus("OO04_READER", reader_password, pull_sql(scn, False), "pull-delete", sql_dir, secrets)
        if code != 0:
            raise c5.CaptureError("delete_pull")
        deleted = parse_pull(text, False)
        gen = 5
        delete_events = None
        for table, rows in deleted.items():
            applied = c5.apply_snapshot(
                shadow / table, committed[table], gen, f"oo-dl01w1-04-scn-{scn}",
                BASE_COLUMNS[table], BASE_COLUMNS[table], rows, 8,
                quality=["DELETES_LAGGED"],
            )
            if table == "T_BATCH":
                delete_events = applied["events"]
            elif applied["events"]:
                raise c5.CaptureError(f"delete_{table}")
            committed[table] = c5.load_generation(shadow / table, gen)
        result["delete_events"] = delete_events
        if delete_events != [{"op": "delete", "key": "91"}]:
            raise c5.CaptureError("delete_event")
        if "91" in committed["T_BATCH"]["cells"]:
            raise c5.CaptureError("delete_visible")
        if "91:5" not in committed["T_BATCH"]["tombstones"]:
            raise c5.CaptureError("tombstone")
        inserted_cells = c5.load_generation(shadow / "T_BATCH", 3)["cells"]
        try:
            c5.apply_snapshot(
                shadow / "T_BATCH", committed["T_BATCH"], 99, "stale",
                BASE_COLUMNS["T_BATCH"], BASE_COLUMNS["T_BATCH"], inserted_cells, 8,
                stale_upsert_generation=5,
            )
        except c5.CaptureError as exc:
            if str(exc) != "stale_upsert":
                raise
            result["stale_upsert"] = "refused"
        else:
            raise c5.CaptureError("stale_upsert_allowed")
        if c5.generation_path(shadow / "T_BATCH", 99).exists() or c5.staging_path(shadow / "T_BATCH", 99).exists():
            raise c5.CaptureError("stale_upsert_committed")

        repeated = c5.apply_snapshot(
            shadow / "T_BATCH", committed["T_BATCH"], 6, committed["T_BATCH"]["position"],
            BASE_COLUMNS["T_BATCH"], BASE_COLUMNS["T_BATCH"], deleted["T_BATCH"], 8,
        )
        result["repeat_replay_events"] = repeated["events"]
        if repeated["events"]:
            raise c5.CaptureError("repeat_replay")
        committed["T_BATCH"] = c5.load_generation(shadow / "T_BATCH", 6)

        interrupted = c5.apply_snapshot(
            shadow / "interrupt", None, 1, "half", BASE_COLUMNS["T_BATCH"], BASE_COLUMNS["T_BATCH"],
            deleted["T_BATCH"], 8, interrupt=True,
        )
        result["interrupt"] = {
            "committed": interrupted["committed"],
            "state": interrupted["state"],
            "staging_left": c5.staging_path(shadow / "interrupt", 1).exists(),
            "generation_left": c5.generation_path(shadow / "interrupt", 1).exists(),
        }
        if interrupted["committed"] or result["interrupt"]["staging_left"] or result["interrupt"]["generation_left"]:
            raise c5.CaptureError("interrupt_committed")

        expired_probe_code, expired_text = sqlplus(
            "OO04_READER",
            reader_password,
            "WHENEVER SQLERROR EXIT SQL.SQLCODE\nSET HEADING OFF FEEDBACK OFF PAGESIZE 0 LINESIZE 200\n"
            "SELECT 'OLD|' || COUNT(*) FROM oo04_fix.t_batch AS OF SCN 100;\nEXIT\n",
            "pull-expired-scn",
            sql_dir,
            secrets,
        )
        result["ancient_scn_exit"] = expired_probe_code
        result["ancient_scn_error"] = next((line.strip() for line in expired_text.splitlines() if "ORA-" in line), "")
        result["ancient_scn_counted_as_expiry"] = False
        age_started = time.monotonic()
        time.sleep(2)
        age = time.monotonic() - age_started
        result["position_age_seconds"] = age
        result["position_state"] = c5.position_state(age, 1)
        if result["position_state"] != "POSITION_EXPIRED":
            raise c5.CaptureError("position_not_expired")
        try:
            c5.resume_expired()
            raise c5.CaptureError("resume_allowed")
        except c5.CaptureError as exc:
            if str(exc) != "resume_forbidden":
                raise
        result["resume_expired"] = "resume_forbidden"
        waits.append({"group": "proof_changed", "cycle": "resnapshot", **c5.wait_realtime(changed_periods)})
        scn, _, _ = publish_scn(dba_password, sql_dir, secrets, "scn-resnapshot")
        code, text = sqlplus("OO04_READER", reader_password, pull_sql(scn, False), "pull-resnapshot", sql_dir, secrets)
        if code != 0:
            raise c5.CaptureError("resnapshot_pull")
        fresh = parse_pull(text, False)
        gen = 7
        resnap = c5.apply_snapshot(
            shadow / "T_BATCH", committed["T_BATCH"], gen, f"oo-dl01w1-04-scn-{scn}",
            BASE_COLUMNS["T_BATCH"], BASE_COLUMNS["T_BATCH"], fresh["T_BATCH"], 8,
            quality=["POSITION_EXPIRED", "DELETES_LAGGED"],
        )
        if resnap["resumed"] or not resnap["committed"]:
            raise c5.CaptureError("resnapshot")
        old = c5.load_generation(shadow / "T_BATCH", 6)
        new = c5.load_generation(shadow / "T_BATCH", 7)
        result["resnapshot"] = {
            "old_generation_readable": old["generation"] == 6,
            "new_generation": new["generation"],
            "resumed": new["resumed"],
            "old_position": old["position"],
            "new_position": new["position"],
            "quality": new["quality"],
        }
        committed["T_BATCH"] = new
        for table in ("T_WIDE", "T_TS"):
            applied = c5.apply_snapshot(
                shadow / table, committed[table], gen, f"oo-dl01w1-04-scn-{scn}",
                BASE_COLUMNS[table], BASE_COLUMNS[table], fresh[table], 8,
                quality=["POSITION_EXPIRED", "DELETES_LAGGED"],
            )
            if applied["events"]:
                raise c5.CaptureError(f"resnapshot_{table}")
            committed[table] = c5.load_generation(shadow / table, gen)

        dba(
            dba_password,
            "ALTER TABLE oo04_fix.t_batch ADD (c5_note VARCHAR2(20));\n"
            "SELECT 'USER|' || SYS_CONTEXT('USERENV','SESSION_USER') FROM dual;",
            "dba-add-column",
            sql_dir,
            secrets,
        )
        waits.append({"group": "proof_changed", "cycle": "schema_pause", **c5.wait_realtime(changed_periods)})
        seen = reader_dictionary(reader_password, sql_dir, secrets, "dictionary-paused")
        paused_names = [column["name"] for column in seen["T_BATCH"]]
        if paused_names != BASE_COLUMNS["T_BATCH"] + ["C5_NOTE"]:
            raise c5.CaptureError("new_column_missing")
        scn, _, _ = publish_scn(dba_password, sql_dir, secrets, "scn-paused")
        code, text = sqlplus("OO04_READER", reader_password, pull_sql(scn, True), "pull-paused", sql_dir, secrets)
        if code != 0:
            raise c5.CaptureError("paused_pull")
        paused_rows = parse_pull(text, True)
        paused = c5.apply_snapshot(
            shadow / "T_BATCH", committed["T_BATCH"], 8, f"oo-dl01w1-04-scn-{scn}",
            paused_names, BASE_COLUMNS["T_BATCH"], paused_rows["T_BATCH"], 8,
        )
        result["schema_pause"] = paused["state"]
        if paused["committed"] or c5.generation_path(shadow / "T_BATCH", 8).exists():
            raise c5.CaptureError("schema_pause_committed")
        approval = {
            "protocol": "oo.oracle.c5-column-approval/v1",
            "object": "OO04_FIX.T_BATCH",
            "column": "C5_NOTE",
            "data_type": "VARCHAR2(20)",
            "approved": True,
            "by": "test-dba-existing-system",
        }
        (run / "column-approval.json").write_text(json.dumps(approval, indent=2) + "\n")
        approved = dict(BASE_COLUMNS)
        approved["T_BATCH"] = BASE_COLUMNS["T_BATCH"] + ["C5_NOTE"]
        waits.append({"group": "proof_changed", "cycle": "schema_approved", **c5.wait_realtime(changed_periods)})
        scn, _, _ = publish_scn(dba_password, sql_dir, secrets, "scn-approved")
        code, text = sqlplus("OO04_READER", reader_password, pull_sql(scn, True), "pull-approved", sql_dir, secrets)
        if code != 0:
            raise c5.CaptureError("approved_pull")
        approved_rows = parse_pull(text, True)
        gen = 8
        final_batch = c5.apply_snapshot(
            shadow / "T_BATCH", committed["T_BATCH"], gen, f"oo-dl01w1-04-scn-{scn}",
            approved["T_BATCH"], approved["T_BATCH"], approved_rows["T_BATCH"], 8,
            quality=["DELETES_LAGGED"],
        )
        if not final_batch["committed"]:
            raise c5.CaptureError("approved_commit")
        final = c5.load_generation(shadow / "T_BATCH", gen)
        write_arrow_request(shadow / "arrow-json", "T_BATCH", gen, approved["T_BATCH"], final["cells"])
        for key in ("41", "42", "43", "44", "45"):
            if final["cells"][key]["C5_NOTE"] is not None:
                raise c5.CaptureError("new_column_value")
            original = {name: baseline["T_BATCH"][key][name] for name in BASE_COLUMNS["T_BATCH"]}
            current = {name: final["cells"][key][name] for name in BASE_COLUMNS["T_BATCH"]}
            if original != current:
                raise c5.CaptureError(f"base_row_changed_{key}")
        result["approved_generation"] = gen
        result["approved_columns"] = approved["T_BATCH"]
        result["final_scn"] = scn

        code, text = sqlplus("OO04_READER", reader_password, pull_sql(scn, True), "pull-independent", sql_dir, secrets)
        if code != 0:
            raise c5.CaptureError("independent_pull")
        independent = parse_pull(text, True)
        mismatches = []
        for key, cells in final["cells"].items():
            if independent["T_BATCH"].get(key) != cells:
                mismatches.append(key)
        for table in ("T_WIDE", "T_TS"):
            current = c5.load_generation(shadow / table, 7)["cells"]
            if independent[table] != current:
                mismatches.append(table)
        result["independent_mismatches"] = mismatches
        result["independent_exit"] = 0 if not mismatches else 1
        if mismatches:
            raise c5.CaptureError("independent_cells")

        reader_audit = audit_rows(dba_password, sql_dir, secrets, stamp, "OO04_READER", "AUD")
        classified = c5.classify_audit(reader_audit)
        result["reader_audit"] = {
            "rows": len(reader_audit),
            "platform_dml": classified["platform_dml"],
            "kernel_audit": classified["kernel_audit"],
            "other": classified["other"],
            "actions": sorted({row["action"] for row in reader_audit}),
        }
        if classified["platform_dml"] != 0:
            raise c5.CaptureError("reader_dml")
        system_audit = audit_rows(dba_password, sql_dir, secrets, stamp, "SYSTEM", "SYS")
        result["system_audit"] = {
            "rows": len(system_audit),
            "policy": "rows_present" if system_audit else "not_enabled",
            "actions": sorted({row["action"] for row in system_audit}),
        }
        result["dba_session_user"] = "SYSTEM"
        dictionary_after = reader_dictionary(reader_password, sql_dir, secrets, "dictionary-after")
        expected_after = dict(BASE_COLUMNS)
        expected_after["T_BATCH"] = BASE_COLUMNS["T_BATCH"] + ["C5_NOTE"]
        for table, expect in expected_after.items():
            names = [column["name"] for column in dictionary_after[table]]
            if names != expect:
                raise c5.CaptureError(f"dictionary_after_{table}")
        note = next(column for column in dictionary_after["T_BATCH"] if column["name"] == "C5_NOTE")
        if note["data_type"] != "VARCHAR2" or "empty_string_is_null" not in note["flags"]:
            raise c5.CaptureError("c5_note_type")
        catalog, profile = write_producer(run, shadow, dictionary_after, scn)
        if catalog["schema_digest"] is not None or profile["schema_digest"] is not None:
            raise c5.CaptureError("hand_filled_digest")
        if catalog["admitted"] or profile["admitted"]:
            raise c5.CaptureError("hand_filled_admission")
        result["producer_catalog"] = "producer-catalog.json"
        result["producer_profile"] = "producer-profile.json"
        result["loss_flags"] = profile["loss_flags"]
        result["waits"] = waits
        result["physical_tables_after"] = pg_tables()
        if result["physical_tables_before"] != result["physical_tables_after"]:
            raise c5.CaptureError("physical_tables_changed")
        _, _, supplemental_after = publish_scn(dba_password, sql_dir, secrets, "scn-final-status")
        result["supplemental_log_data_min_after"] = supplemental_after
        if supplemental_after != "NO":
            raise c5.CaptureError("supplemental_logging_changed")
        result["ok"] = True
        result["exit"] = 0
    except Exception as exc:
        result["ok"] = False
        result["exit"] = 1
        result["error"] = str(exc)
    finally:
        for secret in secrets:
            blob = json.dumps(result).encode()
            if secret and secret in blob:
                raise SystemExit("result contains a secret")
        (run / "result.json").write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    return result["exit"]


if __name__ == "__main__":
    raise SystemExit(main())
