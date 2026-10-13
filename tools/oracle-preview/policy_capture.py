#!/usr/bin/env python3
"""Tighten the owned reader and capture T_BATCH at a DBA-published SCN.

The DBA revokes EXECUTE on SYS.DBMS_FLASHBACK and publishes the SCN.
The reader uses AS OF SCN and does not call the package. Rows are written
as text. Arrow encoding is a later step. This does not write previewdb,
admitted, or admission-1.
"""

from __future__ import annotations

import fcntl
import hashlib
import json
import os
import subprocess
import time
from pathlib import Path

import product

ROOT = product.ROOT
ORACLE = product.ORACLE_NAME
LOCK = Path("/tmp/oo-dl01-wave1-resources.lock")
OUT = ROOT / "evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/policy-2"
NEED = 6 * 1024 * 1024 * 1024
HEADROOM_FLOOR = 10 * 1024 * 1024 * 1024
PASSWORD = Path("/Users/redux/.oo-dl01w1-04-p2/child-reader")
OLD_SCN = "2175138"
OLD_ARROW = "fc547b45aa6743e243de59b16719cb9efffcdd403dfab47d5e9eaec4f65b523b"
OLD_LITERAL = "a6236974c0d71bfc372587e84e39d4dcf93aa994005700157854294abed47aa3"
DRIVER_DIGEST = "d729181747bd69ba53cde2956ea9ddc06cf09d0badaf4a9e89944b7c0b7b4397"


def run(args: list[str], *, stdin: bytes | None = None, timeout: int = 60) -> subprocess.CompletedProcess:
    return subprocess.run(args, input=stdin, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout)


def scrub(payload: bytes, secret: str) -> bytes:
    if not secret:
        return payload
    return payload.replace(secret.encode(), b"[redacted]")


def capacity(extra: int) -> None:
    info = run(["docker", "info", "--format", "{{.NCPU}} {{.MemTotal}}"])
    parts = info.stdout.decode().split()
    if info.returncode != 0 or len(parts) < 2:
        raise SystemExit("docker info failed")
    ncpu = int(parts[0])
    mem = int(parts[1])
    reserved = 0
    running = 0
    own = 0
    for cid in run(["docker", "ps", "-q"]).stdout.decode().split():
        one = run(["docker", "inspect", cid])
        if one.returncode != 0:
            raise SystemExit("inspect failed")
        data = json.loads(one.stdout)[0]
        labels = (data.get("Config") or {}).get("Labels") or {}
        if labels.get("oo.kind") == "rust-builder":
            running += 1
            if labels.get("oo.owner") == "dl01w1-04":
                own += 1
        reserved += int(data["HostConfig"]["Memory"] or 0)
    headroom = mem - reserved
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "capacity-oracle.txt").write_text(
        f"ncpu={ncpu} mem={mem} reserved={reserved} headroom={headroom} "
        f"extra={extra} running_rust_builder={running} own_rust_builder={own}\n"
    )
    if ncpu < 16 or running >= 10 or own >= 1 or headroom - extra <= HEADROOM_FLOOR:
        raise SystemExit(f"capacity refused headroom={headroom} extra={extra} rust={running}")


def oracle_data() -> dict:
    proc = run(["docker", "inspect", ORACLE])
    if proc.returncode != 0:
        raise SystemExit("oracle container is absent")
    data = json.loads(proc.stdout)[0]
    labels = (data.get("Config") or {}).get("Labels") or {}
    if labels.get("oo.owner") != "dl01w1-04" or labels.get("oo.session") != product.OWNER:
        raise SystemExit("refusing foreign oracle container")
    mounts = [mount.get("Name") for mount in data.get("Mounts") or []]
    if "oo-dl01w1-04-p2-oradata" not in mounts:
        raise SystemExit("oracle data volume is not the retained oradata volume")
    return data


def start_oracle() -> None:
    fd = os.open(LOCK, os.O_CREAT | os.O_RDWR, 0o644)
    try:
        fcntl.lockf(fd, fcntl.LOCK_EX)
        data = oracle_data()
        if data["State"]["Running"]:
            raise SystemExit("oracle is already running; not adopting it")
        if int(data["HostConfig"]["Memory"] or 0) != NEED:
            raise SystemExit("oracle memory limit is not 6GiB")
        capacity(NEED)
        started = run(["docker", "start", ORACLE], timeout=90)
        (OUT / "oracle-start.stdout").write_bytes(started.stdout)
        (OUT / "oracle-start.stderr").write_bytes(started.stderr)
        (OUT / "oracle-start.exit").write_text(f"{started.returncode}\n")
        if started.returncode != 0:
            raise SystemExit("oracle start failed")
    finally:
        os.close(fd)


def wait_oracle() -> None:
    sql = b"WHENEVER SQLERROR EXIT FAILURE\nSELECT status FROM v$instance;\nEXIT\n"
    last = None
    for _ in range(40):
        last = run(
            ["docker", "exec", "-i", ORACLE, "sqlplus", "-s", "/", "as", "sysdba"],
            stdin=sql,
            timeout=20,
        )
        text = last.stdout.decode(errors="replace")
        if last.returncode == 0 and "ORA-" not in text and "OPEN" in text:
            (OUT / "oracle-open.stdout").write_bytes(last.stdout)
            (OUT / "oracle-open.exit").write_text("0\n")
            return
        time.sleep(5)
    if last is not None:
        (OUT / "oracle-open.stdout").write_bytes(last.stdout)
        (OUT / "oracle-open.stderr").write_bytes(last.stderr)
        (OUT / "oracle-open.exit").write_text(f"{last.returncode}\n")
    raise SystemExit("oracle did not open")


def stop_oracle() -> None:
    stopped = run(["docker", "stop", "-t", "20", ORACLE], timeout=40)
    (OUT / "oracle-stop.stdout").write_bytes(stopped.stdout)
    (OUT / "oracle-stop.stderr").write_bytes(stopped.stderr)
    (OUT / "oracle-stop.exit").write_text(f"{stopped.returncode}\n")
    state = run(["docker", "inspect", "-f", "{{.State.Status}} {{.State.Running}}", ORACLE])
    (OUT / "oracle-after-stop.txt").write_bytes(state.stdout)
    if stopped.returncode != 0 or state.stdout.decode().strip() != "exited false":
        raise SystemExit("oracle did not return to exited")


def password() -> str:
    mode = PASSWORD.stat().st_mode & 0o777
    (OUT / "password-file.txt").write_text(
        f"path_basename=child-reader mode={mode:04o} bytes={PASSWORD.stat().st_size}\n"
    )
    raw = PASSWORD.read_text()
    secret = raw.strip("\n")
    if not secret or any(ch in secret for ch in "\n\r\"/@ "):
        raise SystemExit("reader secret is not a single sqlplus quoted token")
    return secret


def sqlplus(script: str, *, sysdba: bool, secret: str, name: str, retries: int = 8) -> str:
    args = ["docker", "exec", "-i", ORACLE, "sqlplus", "-s"]
    if sysdba:
        args.extend(["/", "as", "sysdba"])
    else:
        args.append("/nolog")
    last = None
    for _ in range(retries):
        last = run(args, stdin=script.encode(), timeout=40)
        out = scrub(last.stdout, secret)
        err = scrub(last.stderr, secret)
        text = out.decode(errors="replace")
        if last.returncode == 0 and "ORA-" not in text and "SP2-" not in text and "PLS-" not in text:
            (OUT / f"{name}.stdout").write_bytes(out)
            (OUT / f"{name}.stderr").write_bytes(err)
            (OUT / f"{name}.exit").write_text("0\n")
            return text
        time.sleep(3)
    if last is not None:
        (OUT / f"{name}.stdout").write_bytes(scrub(last.stdout, secret))
        (OUT / f"{name}.stderr").write_bytes(scrub(last.stderr, secret))
        (OUT / f"{name}.exit").write_text(f"{last.returncode}\n")
    raise SystemExit(f"{name} failed")


def sqlplus_once(script: str, *, sysdba: bool, secret: str, name: str) -> tuple[int, str]:
    args = ["docker", "exec", "-i", ORACLE, "sqlplus", "-s", "/nolog" if not sysdba else "/"]
    if sysdba:
        args = ["docker", "exec", "-i", ORACLE, "sqlplus", "-s", "/", "as", "sysdba"]
    last = run(args, stdin=script.encode(), timeout=40)
    out = scrub(last.stdout, secret)
    err = scrub(last.stderr, secret)
    (OUT / f"{name}.stdout").write_bytes(out)
    (OUT / f"{name}.stderr").write_bytes(err)
    (OUT / f"{name}.exit").write_text(f"{last.returncode}\n")
    return last.returncode, out.decode(errors="replace")


def lines_of(text: str, prefix: str) -> list[str]:
    found = []
    for line in text.splitlines():
        item = line.strip()
        if item.startswith(prefix):
            found.append(item[len(prefix):])
    return found


def one(text: str, prefix: str) -> str:
    found = lines_of(text, prefix)
    if len(found) != 1 or not found[0]:
        raise SystemExit(f"missing {prefix}")
    return found[0]


def permission(name: str, database: str | None = None, table: str | None = None) -> dict:
    item = {"name": name, "status": "known", "granted": True}
    if database is not None or table is not None:
        item["scope"] = {"database": database, "table": table}
    return item


def permissions_from(held: list[str], sys_privs: list[str]) -> list[dict]:
    items = []
    for row in held:
        grantee, privilege, owner, table = row.split("|", 3)
        if grantee != "OO04_READER":
            continue
        items.append(permission(privilege, owner, table))
    for privilege in sys_privs:
        items.append(permission(privilege))
    return items


def wide(rows: list[str]) -> bool:
    for row in rows:
        privilege = row.split("|", 1)[0]
        if " ANY" in privilege or privilege == "LOGMINING":
            return True
    return False


def pg_baseline() -> None:
    proc = run(
        [
            "docker", "exec", "-u", "postgres", product.PG_NAME,
            "psql", "-d", product.DATABASE, "-tA", "-c",
            "SELECT state || '|' || source_id || '|' || source_generation || '|' || "
            "coalesce(sealed_at::text, 'null') || '|' || input_digest "
            "FROM _oo.full_import_run ORDER BY source_id",
        ]
    )
    (OUT / "pg-before.txt").write_bytes(proc.stdout + proc.stderr)
    (OUT / "pg-before.exit").write_text(f"{proc.returncode}\n")


def dba_sql() -> str:
    return (
        "whenever sqlerror exit failure\n"
        "alter session set container = freepdb1;\n"
        "set heading off feedback off pagesize 0 linesize 400 trimspool on verify off echo off serveroutput off\n"
        "begin\n"
        "  execute immediate 'revoke execute on sys.dbms_flashback from oo04_reader';\n"
        "exception\n"
        "  when others then\n"
        "    if sqlcode not in (-1927, -1951, -1952) then raise; end if;\n"
        "end;\n"
        "/\n"
        "select 'REVOKE=done' from dual;\n"
        "select 'SCN=' || to_char(dbms_flashback.get_system_change_number) from dual;\n"
        "select 'BANNER=' || banner from v$version;\n"
        "select 'EXE=' || count(*) from dba_tab_privs where grantee = 'OO04_READER' "
        "and privilege = 'EXECUTE' and owner = 'SYS' and table_name = 'DBMS_FLASHBACK';\n"
        "exit\n"
    )


def reader_sql(secret: str) -> str:
    return (
        'connect OO04_READER/"' + secret + '"@//127.0.0.1:1521/FREEPDB1\n'
        "whenever sqlerror exit failure\n"
        "set heading off feedback off pagesize 0 linesize 400 trimspool on verify off echo off\n"
        "select 'USER=' || sys_context('USERENV','SESSION_USER') from dual;\n"
        "select 'CON=' || sys_context('USERENV','CON_NAME') from dual;\n"
        "select 'SERVICE=' || sys_context('USERENV','SERVICE_NAME') from dual;\n"
        "select 'TAB=' || privilege || '|' || owner || '|' || table_name from user_tab_privs order by 1;\n"
        "select 'HELD=' || grantee || '|' || privilege || '|' || owner || '|' || table_name from user_tab_privs order by 1;\n"
        "select 'SYS=' || privilege from user_sys_privs order by 1;\n"
        "select 'COL=' || column_name || '|' || data_type || '|' || nvl(to_char(data_precision),'-') || '|' || "
        "nvl(to_char(data_scale),'-') || '|' || nvl(to_char(char_length),'-') || '|' || nullable "
        "from all_tab_columns where owner = 'OO04_FIX' and table_name = 'T_BATCH' order by column_id;\n"
        "exit\n"
    )


def package_sql(secret: str) -> str:
    return (
        'connect OO04_READER/"' + secret + '"@//127.0.0.1:1521/FREEPDB1\n'
        "whenever sqlerror exit failure\n"
        "set heading off feedback off pagesize 0 linesize 200 trimspool on verify off echo off\n"
        "select 'PKG=' || to_char(dbms_flashback.get_system_change_number) from dual;\n"
        "exit\n"
    )


def rows_sql(secret: str, scn: str) -> str:
    if not scn.isdigit() or scn == OLD_SCN:
        raise SystemExit("refusing row query scn")
    return (
        'connect OO04_READER/"' + secret + '"@//127.0.0.1:1521/FREEPDB1\n'
        "whenever sqlerror exit failure\n"
        "set heading off feedback off pagesize 0 linesize 2000 trimspool on verify off echo off wrap off\n"
        f"select 'COUNT=' || count(*) from oo04_fix.t_batch as of scn {scn};\n"
        "select 'ROW=' || id || '|' || nvl(to_char(n16, 'FM9999999990.000000'), 'NULL') || '|' || "
        "nvl(to_char(n1), 'NULL') || '|' || nvl(to_char(d, 'YYYY-MM-DD HH24:MI:SS'), 'NULL') || '|' || "
        "nvl(dump(c6, 16), 'NULL') || '|' || nvl(dump(v2, 16), 'NULL') || '|' || nvl(rawtohex(r), 'NULL') "
        f"from oo04_fix.t_batch as of scn {scn} order by id;\n"
        "exit\n"
    )


def parse_reader(text: str) -> dict:
    reader = {
        "user": one(text, "USER="),
        "con": one(text, "CON="),
        "service": one(text, "SERVICE="),
        "tab": lines_of(text, "TAB="),
        "held": lines_of(text, "HELD="),
        "sys": lines_of(text, "SYS="),
        "columns": lines_of(text, "COL="),
    }
    if reader["user"] != "OO04_READER" or reader["con"] != "FREEPDB1":
        raise SystemExit("reader session is not OO04_READER in FREEPDB1")
    if wide(reader["tab"] + reader["sys"]):
        raise SystemExit("reader grant contains ANY or LOGMINING")
    names = [row.split("|", 3)[1] for row in reader["held"] if row.startswith("OO04_READER|")]
    if "EXECUTE" in names:
        raise SystemExit("reader still holds EXECUTE")
    if "CREATE SESSION" not in reader["sys"]:
        raise SystemExit("reader is missing CREATE SESSION")
    for table in ("T_BATCH", "T_TS", "T_WIDE"):
        if f"OO04_READER|SELECT|OO04_FIX|{table}" not in reader["held"]:
            raise SystemExit(f"reader is missing SELECT on {table}")
        if f"OO04_READER|FLASHBACK|OO04_FIX|{table}" not in reader["held"]:
            raise SystemExit(f"reader is missing FLASHBACK on {table}")
    expect = [
        "ID|NUMBER|4|0|0|N",
        "N16|NUMBER|16|6|0|Y",
        "N1|NUMBER|1|0|0|Y",
        "D|DATE|-|-|0|Y",
        "C6|CHAR|-|-|6|Y",
        "V2|VARCHAR2|-|-|20|Y",
        "R|RAW|-|-|0|Y",
    ]
    if reader["columns"] != expect:
        raise SystemExit(f"column metadata {reader['columns']}")
    return reader


def write_documents(reader: dict, scn: str, rows: list[str], banner: str, package_exit: int) -> None:
    perms = permissions_from(reader["held"], reader["sys"])
    if any(item["name"] == "EXECUTE" for item in perms):
        raise SystemExit("report would contain EXECUTE")
    if not perms:
        raise SystemExit("reader holds no privileges")
    report = {
        "protocol": "oo.source-admission.report/v1",
        "target_id": "oracle-26ai-free-23.26-freepdb1",
        "source_id": "oo04_t_batch",
        "secret_ref": "secret://oo04p2/driver/source/reader",
        "driver": {
            "kind": "native-rust",
            "name": "oracledb",
            "version": "26.0.0-beta.4",
            "digest": DRIVER_DIGEST,
        },
        "compat_mode": "oracle",
        "license_flags": ["UPL-1.0 OR Apache-2.0"],
        "permissions": perms,
        "capabilities": [
            {"name": "Catalog", "cursor": None, "feed": None, "cdc": None, "token_order": None, "budget_unit": None, "level": "designed", "write_class": "R0", "consistency": "S0"},
            {"name": "BatchFull", "cursor": None, "feed": None, "cdc": None, "token_order": None, "budget_unit": None, "level": "designed", "write_class": "R0", "consistency": "S1"},
            {"name": "SnapshotToken", "cursor": None, "feed": None, "cdc": None, "token_order": "Total", "budget_unit": None, "level": "designed", "write_class": "R0", "consistency": "S1"},
            {"name": "Budget", "cursor": None, "feed": None, "cdc": None, "token_order": None, "budget_unit": "Rows", "level": "designed", "write_class": "R0", "consistency": "B"},
        ],
        "capture_methods": [
            {"method": "C5", "write_class": "R0", "consistency": "S1", "token_order": "Total", "retention_window": None, "cursor": None, "feed": None, "cdc": None}
        ],
        "cost_model": {"unit": "Rows", "confidence": "unknown", "estimate": "spec_batch_json_max_rows"},
        "r1w": None,
    }
    config = {
        "protocol": "oo.source-admission.config/v1",
        "target_id": report["target_id"],
        "source_id": report["source_id"],
        "secret_ref": report["secret_ref"],
        "compat_mode": "oracle",
        "permissions": perms,
        "budget": {"unit": "Rows", "hard_limit": 5, "confidence": "unknown"},
        "write_options": {"signal_table": False, "heartbeat_write": False, "create_stream": False, "create_trigger": False},
        "debezium": {"signal_data_collection": None, "heartbeat_action_query": None, "snapshot_mode": "never", "create_stream": False, "create_trigger": False},
        "requested_write_class": "R0",
        "r1w": None,
    }
    report_bytes = (json.dumps(report, indent=2) + "\n").encode()
    config_bytes = (json.dumps(config, indent=2) + "\n").encode()
    (OUT / "report.json").write_bytes(report_bytes)
    (OUT / "config.json").write_bytes(config_bytes)
    (OUT / "rows.txt").write_text("\n".join(rows) + "\n")
    observed = {
        "protocol": "oo.oracle-preview.reader-observation/v2",
        "kind": "current_freepdb1_reader_as_of_published_scn",
        "published_scn": int(scn),
        "scn_publisher": "sysdba FREEPDB1 DBMS_FLASHBACK.GET_SYSTEM_CHANGE_NUMBER",
        "reader_called_package": False,
        "reader_package_probe_exit": package_exit,
        "not_scn": int(OLD_SCN),
        "not_arrow_sha256": OLD_ARROW,
        "not_literal_input_digest": OLD_LITERAL,
        "reader": reader,
        "rows": rows,
        "banner": banner,
        "report_sha256": hashlib.sha256(report_bytes).hexdigest(),
        "config_sha256": hashlib.sha256(config_bytes).hexdigest(),
    }
    (OUT / "observation.json").write_text(json.dumps(observed, indent=2) + "\n")


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    pg_baseline()
    secret = password()
    start_oracle()
    package_exit = 0
    try:
        wait_oracle()
        dba_text = sqlplus(dba_sql(), sysdba=True, secret=secret, name="dba-publish")
        scn = one(dba_text, "SCN=")
        if not scn.isdigit() or scn == OLD_SCN:
            raise SystemExit("dba published an unusable scn")
        if one(dba_text, "REVOKE=") != "done" or one(dba_text, "EXE=") != "0":
            raise SystemExit("execute revoke did not clear the dba grant")
        banner = "\n".join(lines_of(dba_text, "BANNER="))
        if "23.26" not in banner or "Free" not in banner:
            raise SystemExit("banner is not Oracle Free 23.26")
        reader_text = sqlplus(reader_sql(secret), sysdba=False, secret=secret, name="reader")
        reader = parse_reader(reader_text)
        package_exit, package_text = sqlplus_once(package_sql(secret), sysdba=False, secret=secret, name="reader-package")
        if package_exit == 0 or ("ORA-" not in package_text and "PLS-" not in package_text):
            raise SystemExit("reader package call was not rejected")
        row_text = ""
        for attempt in range(1, 6):
            try:
                row_text = sqlplus(rows_sql(secret, scn), sysdba=False, secret=secret, name="reader-rows", retries=2)
                break
            except SystemExit:
                if attempt == 5:
                    raise
                dba_text = sqlplus(dba_sql(), sysdba=True, secret=secret, name=f"dba-publish-{attempt}")
                scn = one(dba_text, "SCN=")
                if not scn.isdigit() or scn == OLD_SCN:
                    raise SystemExit("republished scn is unusable")
        count = one(row_text, "COUNT=")
        rows = lines_of(row_text, "ROW=")
        if count != "5" or len(rows) != 5:
            raise SystemExit(f"row count {count} lines {len(rows)}")
        (OUT / "published-scn.txt").write_text(scn + "\n")
        write_documents(reader, scn, rows, banner, package_exit)
    finally:
        secret = ""
        stop_oracle()


if __name__ == "__main__":
    main()
