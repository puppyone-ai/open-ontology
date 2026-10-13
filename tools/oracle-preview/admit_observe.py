#!/usr/bin/env python3
"""Current FREEPDB1 reader grants for the fixed admission tool.

Starts the retained Oracle container, reads grants as OO04_READER, asks
sysdba in FREEPDB1 for the same grantee, then stops the container. Writes
report and config files. Does not write admitted, previewdb, or a decision.
"""

from __future__ import annotations

import fcntl
import hashlib
import json
import os
import subprocess
import sys
import time
from pathlib import Path

import product

ROOT = product.ROOT
ORACLE = product.ORACLE_NAME
LOCK = Path("/tmp/oo-dl01-wave1-resources.lock")
OUT = ROOT / "evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/admission-1"
NEED = 6 * 1024 * 1024 * 1024
HEADROOM_FLOOR = 10 * 1024 * 1024 * 1024
PASSWORD = Path("/Users/redux/.oo-dl01w1-04-p2/child-reader")
PRIVILEGE = ROOT / (
    "evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/driver/linux/"
    "p2-native/audit/batch/privilege.json"
)
ARROW = ROOT / (
    "evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/driver/linux/"
    "p2-native/audit/batch/batch.arrow"
)
DECODE = ROOT / (
    "evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/"
    "repair-1/decode/decode.json"
)
BATCH_SPEC = ROOT / (
    "evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/driver/linux/"
    "p2-native/spec/batch.json"
)
ARROW_SHA = "fc547b45aa6743e243de59b16719cb9efffcdd403dfab47d5e9eaec4f65b523b"
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
    for cid in run(["docker", "ps", "-q"]).stdout.decode().split():
        one = run(["docker", "inspect", cid])
        if one.returncode != 0:
            raise SystemExit("inspect failed")
        data = json.loads(one.stdout)[0]
        kind = ((data.get("Config") or {}).get("Labels") or {}).get("oo.kind", "")
        if kind == "rust-builder":
            running += 1
        reserved += int(data["HostConfig"]["Memory"] or 0)
    headroom = mem - reserved
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "capacity-oracle.txt").write_text(
        f"ncpu={ncpu} mem={mem} reserved={reserved} headroom={headroom} "
        f"extra={extra} running_rust_builder={running}\n"
    )
    if ncpu < 16 or running >= 1 or headroom - extra <= HEADROOM_FLOOR:
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
        started = run(["docker", "start", ORACLE], timeout=60)
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
    (OUT / "password-file.txt").write_text(f"path_basename=child-reader mode={mode:04o} bytes={PASSWORD.stat().st_size}\n")
    raw = PASSWORD.read_text()
    secret = raw.strip("\n")
    if not secret or "\n" in secret or "\r" in secret or '"' in secret or "/" in secret or "@" in secret:
        raise SystemExit("reader secret is not a single sqlplus quoted token")
    return secret


def sqlplus(script: str, *, sysdba: bool, secret: str, name: str) -> str:
    args = ["docker", "exec", "-i", ORACLE, "sqlplus", "-s"]
    if sysdba:
        args.extend(["/", "as", "sysdba"])
    else:
        args.append("/nolog")
    last = None
    for _ in range(12):
        last = run(args, stdin=script.encode(), timeout=40)
        out = scrub(last.stdout, secret)
        err = scrub(last.stderr, secret)
        text = out.decode(errors="replace")
        if last.returncode == 0 and "ORA-" not in text and "SP2-" not in text:
            (OUT / f"{name}.stdout").write_bytes(out)
            (OUT / f"{name}.stderr").write_bytes(err)
            (OUT / f"{name}.exit").write_text("0\n")
            return text
        time.sleep(5)
    if last is not None:
        (OUT / f"{name}.stdout").write_bytes(scrub(last.stdout, secret))
        (OUT / f"{name}.stderr").write_bytes(scrub(last.stderr, secret))
        (OUT / f"{name}.exit").write_text(f"{last.returncode}\n")
    raise SystemExit(f"{name} failed")


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


def write_inputs(reader: dict, observer: dict, replay: dict) -> None:
    perms = permissions_from(reader["held"], reader["sys"])
    if not perms:
        raise SystemExit("reader holds no privileges")
    spec = json.loads(BATCH_SPEC.read_text())
    if spec.get("max_rows") != 5 or spec.get("secret_ref") != "secret://oo04p2/driver/source/reader":
        raise SystemExit("batch spec identity changed")
    arrow = ARROW.read_bytes()
    arrow_sha = hashlib.sha256(arrow).hexdigest()
    if len(arrow) != 2120 or arrow_sha != ARROW_SHA:
        raise SystemExit("arrow batch is not the captured file")
    decode = json.loads(DECODE.read_text())
    report = {
        "protocol": "oo.source-admission.report/v1",
        "target_id": "oracle-26ai-free-23.26-freepdb1",
        "source_id": "oo04_t_batch",
        "secret_ref": spec["secret_ref"],
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
            {
                "name": "Catalog",
                "cursor": None,
                "feed": None,
                "cdc": None,
                "token_order": None,
                "budget_unit": None,
                "level": "designed",
                "write_class": "R0",
                "consistency": "S0",
            },
            {
                "name": "BatchFull",
                "cursor": None,
                "feed": None,
                "cdc": None,
                "token_order": None,
                "budget_unit": None,
                "level": "designed",
                "write_class": "R0",
                "consistency": "S1",
            },
            {
                "name": "SnapshotToken",
                "cursor": None,
                "feed": None,
                "cdc": None,
                "token_order": "Total",
                "budget_unit": None,
                "level": "designed",
                "write_class": "R0",
                "consistency": "S1",
            },
            {
                "name": "Budget",
                "cursor": None,
                "feed": None,
                "cdc": None,
                "token_order": None,
                "budget_unit": "Rows",
                "level": "designed",
                "write_class": "R0",
                "consistency": "B",
            },
        ],
        "capture_methods": [
            {
                "method": "C5",
                "write_class": "R0",
                "consistency": "S1",
                "token_order": "Total",
                "retention_window": None,
                "cursor": None,
                "feed": None,
                "cdc": None,
            }
        ],
        "cost_model": {
            "unit": "Rows",
            "confidence": "unknown",
            "estimate": "spec_batch_json_max_rows",
        },
        "r1w": None,
    }
    config = {
        "protocol": "oo.source-admission.config/v1",
        "target_id": report["target_id"],
        "source_id": report["source_id"],
        "secret_ref": report["secret_ref"],
        "compat_mode": report["compat_mode"],
        "permissions": perms,
        "budget": {"unit": "Rows", "hard_limit": 5, "confidence": "unknown"},
        "write_options": {
            "signal_table": False,
            "heartbeat_write": False,
            "create_stream": False,
            "create_trigger": False,
        },
        "debezium": {
            "signal_data_collection": None,
            "heartbeat_action_query": None,
            "snapshot_mode": "never",
            "create_stream": False,
            "create_trigger": False,
        },
        "requested_write_class": "R0",
        "r1w": None,
    }
    (OUT / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    (OUT / "config.json").write_text(json.dumps(config, indent=2) + "\n")
    same = reader["tab"] == replay["grants"] and reader["sys"] == replay["sys_privs"]
    observed = {
        "protocol": "oo.oracle-preview.reader-observation/v1",
        "kind": "current_freepdb1_reader_session",
        "not_scn": 2175138,
        "reader": reader,
        "independent_observer": observer,
        "permission_rows": "grantee OO04_READER only; a user_tab_privs row whose grantee is not the reader is recorded and is not submitted",
        "capture_privilege_replay": {
            "kind": "policy_replay_of_original_native_capture",
            "path": "evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/driver/linux/p2-native/audit/batch/privilege.json",
            "sha256": hashlib.sha256(PRIVILEGE.read_bytes()).hexdigest(),
            "grants": replay["grants"],
            "sys_privs": replay["sys_privs"],
            "any_or_logmining": replay["any_or_logmining"],
            "equals_current_reader": same,
        },
        "arrow": {
            "bytes": 2120,
            "sha256": arrow_sha,
            "schema": decode["schema"],
            "rows": decode["rows"],
            "scn": 2175138,
        },
        "banner": observer["banner"],
    }
    (OUT / "observation.json").write_text(json.dumps(observed, indent=2) + "\n")


def rebuild_from_saved() -> None:
    replay = json.loads(PRIVILEGE.read_text())
    reader_text = (OUT / "reader.stdout").read_text()
    observer_text = (OUT / "observer.stdout").read_text()
    reader = {
        "user": one(reader_text, "USER="),
        "con": one(reader_text, "CON="),
        "service": one(reader_text, "SERVICE="),
        "scn": one(reader_text, "SCN="),
        "tab": lines_of(reader_text, "TAB="),
        "held": lines_of(reader_text, "HELD="),
        "sys": lines_of(reader_text, "SYS="),
        "columns": lines_of(reader_text, "COL="),
    }
    observer = {
        "con": one(observer_text, "CON="),
        "tab": lines_of(observer_text, "TAB="),
        "sys": lines_of(observer_text, "SYS="),
        "inherit": lines_of(observer_text, "INHERIT="),
        "banners": lines_of(observer_text, "BANNER="),
    }
    observer["banner"] = "\n".join(observer["banners"])
    write_inputs(reader, observer, replay)


def main() -> None:
    if len(sys.argv) == 2 and sys.argv[1] == "--rebuild":
        rebuild_from_saved()
        return
    OUT.mkdir(parents=True, exist_ok=True)
    replay = json.loads(PRIVILEGE.read_text())
    secret = password()
    reader_sql = (
        'connect OO04_READER/"' + secret + '"@//127.0.0.1:1521/FREEPDB1\n'
        "whenever sqlerror exit failure\n"
        "set heading off feedback off pagesize 0 linesize 400 trimspool on verify off echo off\n"
        "select 'USER=' || sys_context('USERENV','SESSION_USER') from dual;\n"
        "select 'CON=' || sys_context('USERENV','CON_NAME') from dual;\n"
        "select 'SERVICE=' || sys_context('USERENV','SERVICE_NAME') from dual;\n"
        "select 'SCN=' || dbms_flashback.get_system_change_number from dual;\n"
        "select 'TAB=' || privilege || '|' || owner || '|' || table_name from user_tab_privs order by 1;\n"
        "select 'HELD=' || grantee || '|' || privilege || '|' || owner || '|' || table_name from user_tab_privs order by 1;\n"
        "select 'SYS=' || privilege from user_sys_privs order by 1;\n"
        "select 'COL=' || column_name || '|' || data_type || '|' || nvl(to_char(data_precision),'-') || '|' || "
        "nvl(to_char(data_scale),'-') || '|' || nvl(to_char(char_length),'-') || '|' || nullable "
        "from all_tab_columns where owner = 'OO04_FIX' and table_name = 'T_BATCH' order by column_id;\n"
        "exit\n"
    )
    observer_sql = (
        "whenever sqlerror exit failure\n"
        "alter session set container = freepdb1;\n"
        "set heading off feedback off pagesize 0 linesize 400 trimspool on verify off echo off\n"
        "select 'CON=' || sys_context('USERENV','CON_NAME') from dual;\n"
        "select 'TAB=' || privilege || '|' || owner || '|' || table_name from dba_tab_privs "
        "where grantee = 'OO04_READER' order by 1;\n"
        "select 'SYS=' || privilege from dba_sys_privs where grantee = 'OO04_READER' order by 1;\n"
        "select 'INHERIT=' || grantee || '|' || privilege || '|' || owner || '|' || table_name from dba_tab_privs "
        "where privilege = 'INHERIT PRIVILEGES' and table_name = 'OO04_READER' order by 1;\n"
        "select 'BANNER=' || banner from v$version;\n"
        "exit\n"
    )
    start_oracle()
    try:
        wait_oracle()
        reader_text = sqlplus(reader_sql, sysdba=False, secret=secret, name="reader")
        observer_text = sqlplus(observer_sql, sysdba=True, secret=secret, name="observer")
    finally:
        secret = ""
        reader_sql = ""
        stop_oracle()
    reader = {
        "user": one(reader_text, "USER="),
        "con": one(reader_text, "CON="),
        "service": one(reader_text, "SERVICE="),
        "scn": one(reader_text, "SCN="),
        "tab": lines_of(reader_text, "TAB="),
        "held": lines_of(reader_text, "HELD="),
        "sys": lines_of(reader_text, "SYS="),
        "columns": lines_of(reader_text, "COL="),
    }
    if reader["user"] != "OO04_READER" or reader["con"] != "FREEPDB1":
        raise SystemExit("reader session is not OO04_READER in FREEPDB1")
    if reader["scn"] == "2175138":
        raise SystemExit("current scn collided with the captured scn")
    if wide(reader["tab"] + reader["sys"]):
        raise SystemExit("reader grant contains ANY or LOGMINING")
    observer = {
        "con": one(observer_text, "CON="),
        "tab": lines_of(observer_text, "TAB="),
        "sys": lines_of(observer_text, "SYS="),
        "inherit": lines_of(observer_text, "INHERIT="),
        "banners": lines_of(observer_text, "BANNER="),
    }
    if observer["con"] != "FREEPDB1":
        raise SystemExit("observer is not in FREEPDB1")
    banner_text = "\n".join(observer["banners"])
    if "23.26" not in banner_text or "Free" not in banner_text:
        raise SystemExit("banner is not Oracle Free 23.26")
    observer["banner"] = banner_text
    write_inputs(reader, observer, replay)


if __name__ == "__main__":
    main()
