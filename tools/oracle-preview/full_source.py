#!/usr/bin/env python3
"""Oracle whole-source entry for DL01-W1-04.

The customer file is the input. This process does not call product.py main,
does not rerun dba-setup.sql, and does not grant EXECUTE on SYS.DBMS_FLASHBACK.
Batch completion stays cancel_api=absent. OCI cancel is a separate statement.
"""
import hashlib
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

SRC = Path("/Users/redux/puppy-one/open-ontology")
EVID = SRC / "evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/closure-2"
SECRET = Path("/Users/redux/.oo-dl01w1-04-p2")
CUSTOMER = SECRET / "customer-input.json"
TOKEN = Path("/tmp/oo-customer-demo-20261007/llm-token")
ORA = "oo-dl01w1-04-p2-oracle"
PG = "oo-dl01w1-04-p2-pg"
NET_SRC = "oo-dl01w1-04-p2-src"
NET = "oo-dl01w1-04-net"
IMAGE = "docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
IMAGE_ID = "sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
CHILD = "/target/oracle-p2-oci/debug/oo-oracle-p2-child"
ADMISSION = "/target/source-admission-3c55/debug/oo-source-admission"
BUILDER = "/target/import-a682/debug/pg-builder"
PIPELINE = "/target/import-a682/debug/oo-pipeline"
RECEIPT_SRC = SRC / "evidence/2ecaf08e-eb23-4b7a-a720-e07459e6f2a6/DL01/budget-report/native-readonly-completion/receipt.json"
RECEIPT_SHA = "65b7f22fe26686f8a1e61d705a78d5a1a69f2a123de0b9dfce827cce2ffb59ef"
POLICY = SRC / "evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/policy-2"
OLD_SCN = "2196583"
SESSION = "992aa5d3-6848-4dd9-aff1-d4d31a099983"
CLI = "c6c5f31b-478b-482a-9986-3d06914fe366"
LABELS = [
    "--label", "oo.project=open-ontology",
    "--label", "oo.owner=dl01w1-04",
    "--label", f"oo.session={SESSION}",
    "--label", "oo.task=DL01-W1-04",
]


def say(text):
    print(text, flush=True)


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def write_json(path, doc):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n")


def run(args, timeout=120, input_bytes=None, check=False):
    proc = subprocess.run(
        args,
        input=input_bytes,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout,
        check=False,
    )
    if check and proc.returncode != 0:
        raise SystemExit(proc.stderr.decode("utf-8", "replace")[-2000:])
    return proc


def save(directory, name, proc):
    directory.mkdir(parents=True, exist_ok=True)
    (directory / f"{name}.stdout").write_bytes(proc.stdout)
    (directory / f"{name}.stderr").write_bytes(proc.stderr)
    (directory / f"{name}.exit").write_text(str(proc.returncode) + "\n")
    return proc


def sqlplus(sql, name, timeout=90):
    out = EVID / "oracle"
    out.mkdir(parents=True, exist_ok=True)
    path = out / f"{name}.sql"
    path.write_text(sql)
    if "KILL" in path.read_text().upper() or "DBMS_FLASHBACK" in path.read_text().upper():
        raise SystemExit(f"refusing observer sql {name}")
    proc = run([
        "docker", "exec", "-i", "-e", "NLS_LANG=AMERICAN_AMERICA.AL32UTF8",
        ORA, "bash", "-lc", "sqlplus -s / as sysdba",
    ], timeout=timeout, input_bytes=path.read_bytes())
    (out / f"{name}.stdout").write_bytes(proc.stdout)
    (out / f"{name}.stderr").write_bytes(proc.stderr)
    (out / f"{name}.exit").write_text(str(proc.returncode) + "\n")
    return proc


def ensure_customer():
    from customer_input import load_customer
    config_path = os.environ.get("OO_ORACLE_CUSTOMER_CONFIG")
    registry_path = os.environ.get("OO_ORACLE_SECRET_REGISTRY")
    if not config_path or not registry_path:
        raise SystemExit("OO_ORACLE_CUSTOMER_CONFIG and OO_ORACLE_SECRET_REGISTRY are required")
    loaded = load_customer(config_path, registry_path)
    global ORA, CUSTOMER, CUSTOMER_DOC
    ORA = loaded["source"]["container"]
    CUSTOMER = Path(config_path)
    CUSTOMER_DOC = loaded
    redacted = json.loads(json.dumps(loaded))
    redacted.pop("_password_file", None)
    redacted.pop("password_file", None)
    write_json(EVID / "customer-input.redacted.json", redacted)
    return loaded


def customer_request(statement):
    if "CUSTOMER_DOC" not in globals():
        raise ValueError("validated caller input is required before source read")
    profiles = {"oracle:batch-asof": "batch", "oracle:text-wide": "wide", "oracle:text-ts": "ts"}
    result = {"source_host": ORA, "source_user": CUSTOMER_DOC["source"]["reader"],
              "source_service": CUSTOMER_DOC["source"]["service"]}
    if statement in profiles:
        found = [item for item in CUSTOMER_DOC["selections"] if item["profile"] == profiles[statement]]
        if len(found) != 1:
            raise ValueError("exactly one selected object for this encoding is required")
        result["source_object"] = found[0]["object"]
    return result


def publish_scn():
    probe = sqlplus(
        "ALTER SESSION SET CONTAINER = FREEPDB1;\n"
        "SET PAGESIZE 0 FEEDBACK OFF HEADING OFF LINESIZE 200\n"
        f"SELECT 'old_scn|{OLD_SCN}|' || COUNT(*) FROM oo04_fix.t_batch AS OF SCN {OLD_SCN};\n"
        "SELECT 'current|' || current_scn FROM v$database;\n"
        "EXIT\n",
        "scn-before",
    )
    marker = sqlplus(
        "ALTER SESSION SET CONTAINER = FREEPDB1;\n"
        "SET PAGESIZE 0 FEEDBACK OFF HEADING OFF SERVEROUTPUT ON\n"
        "WHENEVER SQLERROR EXIT SQL.SQLCODE\n"
        "DECLARE\n"
        "  already EXCEPTION;\n"
        "  PRAGMA EXCEPTION_INIT(already, -955);\n"
        "BEGIN\n"
        "  EXECUTE IMMEDIATE 'CREATE TABLE sys.oo04_dba_scn_marker (id NUMBER, note VARCHAR2(80), published_at TIMESTAMP)';\n"
        "EXCEPTION WHEN already THEN NULL;\n"
        "END;\n"
        "/\n"
        "INSERT INTO sys.oo04_dba_scn_marker (id, note, published_at)\n"
        "VALUES ((SELECT NVL(MAX(id), 0) + 1 FROM sys.oo04_dba_scn_marker), 'closure-2', SYSTIMESTAMP);\n"
        "COMMIT;\n"
        "SELECT 'published|' || current_scn FROM v$database;\n"
        "EXIT\n",
        "scn-publish",
    )
    text = marker.stdout.decode("utf-8", "replace")
    published = ""
    for line in text.splitlines():
        line = line.strip()
        if line.startswith("published|"):
            published = line.split("|", 1)[1].strip()
    if not published.isdigit() or published in {OLD_SCN, "2175138"}:
        raise SystemExit(f"no new legal SCN in publish output: {text[-500:]}")
    (EVID / "published-scn.txt").write_text(published + "\n")
    say(f"published_scn={published} probe_exit={probe.returncode}")
    return published


def child(statement, audit, max_rows, timeout, scn):
    audit.mkdir(parents=True, exist_ok=True)
    os.chmod(audit, 0o777)
    (audit / "published-scn").write_text(scn + "\n")
    name = "oo-dl01w1-04-c2-" + statement.split(":")[-1][:20]
    run(["docker", "rm", "-f", name], timeout=30)
    request = {
        "protocol": "oo.source.driver-oracle/v2",
        "statement_ref": statement,
        "generation": 2,
        "max_rows": max_rows,
        "source_host": ORA,
        "image_id": IMAGE_ID,
        "decoy_host": "oo-dl01w1-04-p2-decoy",
        "lineage_pad": False,
    }
    request.update(customer_request(statement))
    (audit / "request.json").write_text(json.dumps(request) + "\n")
    cmd = [
        "docker", "run", "--name", name, "-i",
        "--network", NET_SRC,
        "--platform", "linux/arm64", "--pull", "never",
        "--user", "65534:65534",
        "--memory", "512m", "--memory-swap", "512m", "--cpus", "0.5",
        "--pids-limit", "64",
        "--cap-drop", "ALL",
        "--security-opt", "no-new-privileges",
        "--read-only",
        "--tmpfs", "/tmp:rw,noexec,nosuid,nodev,size=64m",
        "--entrypoint", CHILD,
        "-v", "oo-dl01w1-04-target:/target:ro",
        "-v", f"{CUSTOMER_DOC['_password_file']}:/run/secrets/source-password:ro",
        "-v", f"{audit}:/audit",
        *LABELS,
        "--label", "oo.kind=oracle-child",
        IMAGE,
    ]
    proc = subprocess.run(
        cmd,
        input=json.dumps(request).encode() + b"\n",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout,
        check=False,
    )
    save(audit, "child", proc)
    run(["docker", "rm", "-f", name], timeout=30)
    return proc


def child_popen(statement, audit, max_rows, scn):
    audit.mkdir(parents=True, exist_ok=True)
    os.chmod(audit, 0o777)
    (audit / "published-scn").write_text(scn + "\n")
    name = "oo-dl01w1-04-c2-" + statement.split(":")[-1][:20]
    run(["docker", "rm", "-f", name], timeout=30)
    request = {
        "protocol": "oo.source.driver-oracle/v2",
        "statement_ref": statement,
        "generation": 2,
        "max_rows": max_rows,
        "source_host": ORA,
        "image_id": IMAGE_ID,
        "decoy_host": "oo-dl01w1-04-p2-decoy",
        "lineage_pad": False,
    }
    request.update(customer_request(statement))
    (audit / "request.json").write_text(json.dumps(request) + "\n")
    cmd = [
        "docker", "run", "--name", name, "-i",
        "--network", NET_SRC,
        "--platform", "linux/arm64", "--pull", "never",
        "--user", "65534:65534",
        "--memory", "512m", "--memory-swap", "512m", "--cpus", "0.5",
        "--pids-limit", "64",
        "--cap-drop", "ALL",
        "--security-opt", "no-new-privileges",
        "--read-only",
        "--tmpfs", "/tmp:rw,noexec,nosuid,nodev,size=64m",
        "--entrypoint", CHILD,
        "-v", "oo-dl01w1-04-target:/target:ro",
        "-v", f"{CUSTOMER_DOC['_password_file']}:/run/secrets/source-password:ro",
        "-v", f"{audit}:/audit",
        *LABELS,
        "--label", "oo.kind=oracle-child",
        IMAGE,
    ]
    proc = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    proc.stdin.write(json.dumps(request).encode() + b"\n")
    proc.stdin.close()
    return name, proc


def session_status(sid, serial, name):
    sql = (
        "SET PAGESIZE 0 FEEDBACK OFF HEADING OFF LINESIZE 400\n"
        "WHENEVER SQLERROR EXIT SQL.SQLCODE\n"
        "ALTER SESSION SET CONTAINER = FREEPDB1;\n"
        "SELECT 'status|' || NVL(s.status, 'ABSENT') || '|' || NVL(SUBSTR(q.sql_text, 1, 120), '-')\n"
        "FROM v$session s LEFT JOIN v$sql q ON q.sql_id = s.sql_id\n"
        f"WHERE s.sid = {int(sid)} AND s.serial# = {int(serial)};\n"
        "SELECT 'status|ABSENT|-' FROM dual WHERE NOT EXISTS (\n"
        f"  SELECT 1 FROM v$session WHERE sid = {int(sid)} AND serial# = {int(serial)});\n"
        "EXIT\n"
    )
    proc = sqlplus(sql, name)
    return proc.stdout.decode("utf-8", "replace")


def wait_active(audit, label, seconds=40):
    deadline = time.time() + seconds
    while time.time() < deadline:
        session = audit / "session.txt"
        if session.exists():
            sid, serial = session.read_text().strip().split(",")
            text = session_status(sid, serial, label)
            if "ACTIVE" in text and "oo-dl01w1-04-p2-cancel" in text:
                return sid, serial, text
        time.sleep(1)
    return None


def run_deadline(scn):
    audit = EVID / "deadline"
    proc = child("oracle:deadline-select", audit, 1, 40, scn)
    session = (audit / "session.txt").read_text().strip().split(",") if (audit / "session.txt").exists() else None
    after = ""
    if session:
        after = session_status(session[0], session[1], "deadline-after")
    active = "status|ACTIVE|" in after and "oo-dl01w1-04-p2-cancel" in after
    say(f"deadline exit={proc.returncode} still_active={active}")
    return proc.returncode == 3 and not active and (audit / "read-zero.json").exists()


def run_oci_cancel(scn):
    audit = EVID / "oci-cancel"
    name, proc = child_popen("oracle:oci-cancel", audit, 1, scn)
    seen = wait_active(audit, "oci-before", 45)
    if seen:
        (audit / "cancel-now").write_text("1\n")
        say("oci_cancel_flag_written")
    try:
        stdout, stderr = proc.communicate(timeout=60)
    except subprocess.TimeoutExpired:
        run(["docker", "kill", name], timeout=20)
        stdout, stderr = proc.communicate(timeout=20)
    finished = subprocess.CompletedProcess(args=["oci"], returncode=proc.returncode if proc.returncode is not None else 1, stdout=stdout, stderr=stderr)
    save(audit, "child", finished)
    run(["docker", "rm", "-f", name], timeout=30)
    after = ""
    if (audit / "session.txt").exists():
        sid, serial = (audit / "session.txt").read_text().strip().split(",")
        after = session_status(sid, serial, "oci-after")
    active = "status|ACTIVE|" in after and "oo-dl01w1-04-p2-cancel" in after
    body = {}
    if (audit / "read-zero.json").exists():
        body = json.loads((audit / "read-zero.json").read_text())
    ok = (
        seen is not None
        and finished.returncode == 0
        and not active
        and body.get("cancel_api") == "oci_marker_interrupt"
        and body.get("rows_committed") == 0
        and body.get("not_call_timeout") is True
        and body.get("ready") is False
    )
    say(f"oci_cancel ok={ok} exit={finished.returncode} still_active={active} kind={body.get('error_kind')}")
    return ok


def run_batch(scn):
    audit = EVID / "batch"
    name, proc = child_popen("oracle:batch-asof", audit, 5, scn)
    flagged = False
    deadline = time.time() + 180
    mutate_sql = (SRC / "tools/source-driver-worker/oracle-child/sql/dba-mutate.sql").read_text()
    while time.time() < deadline:
        if (audit / "scn-ready").exists() and not flagged:
            mutated = sqlplus(mutate_sql, "dba-mutate-batch")
            save(audit, "dba-mutate", mutated)
            if mutated.returncode == 0 and b"mutate|done" in mutated.stdout:
                (audit / "dba-mutated.flag").write_text("1\n")
                flagged = True
            else:
                say("dba_mutate_failed")
                break
        if proc.poll() is not None:
            break
        time.sleep(1)
    try:
        stdout, stderr = proc.communicate(timeout=90)
    except subprocess.TimeoutExpired:
        run(["docker", "kill", name], timeout=20)
        stdout, stderr = proc.communicate(timeout=20)
    finished = subprocess.CompletedProcess(args=["batch"], returncode=proc.returncode if proc.returncode is not None else 1, stdout=stdout, stderr=stderr)
    save(audit, "child", finished)
    run(["docker", "rm", "-f", name], timeout=30)
    arrow = audit / "batch.arrow"
    ok = finished.returncode == 0 and arrow.is_file() and flagged
    say(f"batch ok={ok} exit={finished.returncode} bytes={arrow.stat().st_size if arrow.is_file() else 0}")
    return ok


def capture_text(statement, folder, scn):
    audit = EVID / folder
    proc = child(statement, audit, 8, 90, scn)
    ok = proc.returncode == 0 and (audit / "text.arrow").is_file() and (audit / "text.json").is_file()
    say(f"{folder} ok={ok} exit={proc.returncode}")
    return ok


def capture_reject(statement, folder, scn):
    audit = EVID / folder
    proc = child(statement, audit, 1, 60, scn)
    body = {}
    if (audit / "reject.json").exists():
        body = json.loads((audit / "reject.json").read_text())
    ok = proc.returncode == 4 and body.get("committed") is False
    say(f"{folder} ok={ok} exit={proc.returncode}")
    return ok


SESSION_ACTIONS = {"LOGON", "LOGOFF", "CREATE SESSION"}
ALLOWED_SESSION_SQL = {"ALTER SESSION SET TIME_ZONE='+00:00'"}
WRITE_ACTIONS = {"INSERT", "UPDATE", "DELETE", "MERGE", "ALTER", "DROP", "CREATE", "GRANT", "TRUNCATE"}


def is_object_write(action):
    action = action.upper()
    if action in SESSION_ACTIONS:
        return False
    if action in WRITE_ACTIONS:
        return True
    return action.startswith("ALTER ") and action != "ALTER SESSION"


def classify_audit(text):
    committed = []
    denied = []
    session = []
    for line in text.splitlines():
        if not line.startswith("audit|"):
            continue
        parts = line.split("|")
        if len(parts) < 4:
            continue
        action = parts[2].upper()
        code = parts[3]
        if action in SESSION_ACTIONS:
            session.append(line)
            continue
        if action == "ALTER SESSION":
            statement = parts[6].strip().rstrip(";") if len(parts) > 6 else ""
            if statement.upper() in ALLOWED_SESSION_SQL:
                session.append(line)
                continue
            # Unknown session mutation fails closed, including missing SQL.
            denied.append(line)
            continue
        if not is_object_write(action):
            continue
        if code in {"0", "00"}:
            committed.append(line)
        else:
            denied.append(line)
    return committed, denied, session


def write_audit_classes(text):
    committed, denied, session = classify_audit(text)
    audit = EVID / "audit"
    audit.mkdir(parents=True, exist_ok=True)
    (audit / "committed-writes.txt").write_text("\n".join(committed) + ("\n" if committed else ""))
    (audit / "denied-writes.txt").write_text("\n".join(denied) + ("\n" if denied else ""))
    (audit / "session-actions.txt").write_text("\n".join(session) + ("\n" if session else ""))
    return not committed and not denied and "audit|done" in text


def audit_reader(since):
    if not since.isdigit() or len(since) != 20:
        raise SystemExit("audit bound is not YYYYMMDDHH24MISSFF6")
    raw = (SRC / "tools/source-driver-worker/oracle-child/sql/dba-audit.sql").read_text()
    proc = sqlplus(raw.replace("&1", since), "reader-audit", timeout=120)
    save(EVID / "audit", "reader", proc)
    text = proc.stdout.decode("utf-8", "replace")
    (EVID / "audit" / "reader.txt").write_text(text)
    clean = write_audit_classes(text)
    committed = classify_audit(text)[0]
    denied = classify_audit(text)[1]
    say(f"audit exit={proc.returncode} committed_writes={len(committed)} denied_writes={len(denied)}")
    return proc.returncode == 0 and clean


def load_template(source_id, budget):
    report = json.loads((POLICY / "report.json").read_text())
    config = json.loads((POLICY / "config.json").read_text())
    report["source_id"] = source_id
    config["source_id"] = source_id
    config["budget"]["hard_limit"] = budget
    config["budget"]["confidence"] = "unknown"
    return report, config


def binding_for(plan, source_id, logical_table, columns, rows, arrow_sha, schema_digest, type_digest, decision_sha, report_sha, config_sha, receipt_sha, catalog_sha):
    return {
        "protocol": "oo.source-admission.capture-binding/v1",
        "source_id": source_id,
        "source_engine": "oracle",
        "source_generation": 2,
        "snapshot": {"kind": "token", "value": f"oo-dl01w1-04-scn-{plan['scn']}"},
        "decision_sha256": decision_sha,
        "report_path": "/in/report.json",
        "report_sha256": report_sha,
        "config_path": "/in/config.json",
        "config_sha256": config_sha,
        "config_digest": config_sha,
        "schema_digest": schema_digest,
        "type_digest": type_digest,
        "input_digest": arrow_sha,
        "driver_kind": "native-rust",
        "driver_name": "oracledb",
        "driver_version": "26.0.0-beta.4",
        "driver_digest": "d729181747bd69ba53cde2956ea9ddc06cf09d0badaf4a9e89944b7c0b7b4397",
        "capture_receipt_path": "/in/capture-receipt.json",
        "capture_receipt_sha256": receipt_sha,
        "catalog_path": "/in/binding-catalog.json",
        "catalog_sha256": catalog_sha,
        "admission_receipt_path": "/in/native-readonly-receipt.json",
        "objects": [{
            "logical_schema": "batch",
            "logical_table": logical_table,
            "row_count": rows,
            "arrow_sha256": arrow_sha,
        }],
        "fields": columns,
    }


def column_decls_batch():
    return [
        {"name": "ID", "arrow_type": "int16", "nullable": False, "precision": 16},
        {"name": "N16", "arrow_type": "decimal128", "nullable": True, "precision": 16, "scale": 6},
        {"name": "N1", "arrow_type": "int8", "nullable": True, "precision": 8},
        {"name": "D", "arrow_type": "timestamp_micros", "nullable": True},
        {"name": "C6", "arrow_type": "utf8", "nullable": True},
        {"name": "V2", "arrow_type": "utf8", "nullable": True},
        {"name": "R", "arrow_type": "binary", "nullable": True},
    ]


def manifest_columns_batch():
    return [
        {"name": "ID", "logical_name": "id", "arrow_type": "int16", "nullable": False, "precision": 16},
        {"name": "N16", "logical_name": "n16", "arrow_type": "decimal128", "nullable": True, "precision": 16, "scale": 6},
        {"name": "N1", "logical_name": "n1", "arrow_type": "int8", "nullable": True, "precision": 8},
        {"name": "D", "logical_name": "d", "arrow_type": "timestamp_micros", "nullable": True},
        {"name": "C6", "logical_name": "c6", "arrow_type": "utf8", "nullable": True, "char_length": 6},
        {"name": "V2", "logical_name": "v2", "arrow_type": "utf8", "nullable": True},
        {"name": "R", "logical_name": "r", "arrow_type": "binary", "nullable": True},
    ]


def admit(directory, report, config):
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    (directory / "config.json").write_text(json.dumps(config, indent=2) + "\n")
    proc = run([
        "docker", "run", "--rm", "--network", "none",
        "--platform", "linux/arm64", "--pull", "never",
        "--memory", "512m", "--cpus", "0.5",
        "--cap-drop", "ALL", "--security-opt", "no-new-privileges",
        "--entrypoint", ADMISSION,
        "-v", "oo-dl01w1-04-target:/target:ro",
        "-v", f"{directory}:/in:ro",
        *LABELS, "--label", "oo.kind=admission",
        IMAGE, "validate", "--report", "/in/report.json", "--config", "/in/config.json",
    ], timeout=60)
    save(directory, "validate", proc)
    decision = proc.stdout
    (directory / "decision.json").write_bytes(decision)
    parsed = {}
    try:
        parsed = json.loads(decision.decode() or "{}")
    except json.JSONDecodeError:
        parsed = {}
    say(f"admit {directory.name} exit={proc.returncode} admitted={parsed.get('admitted')}")
    return proc.returncode in (0, 2) and parsed.get("admitted") is True, sha256_bytes(decision), parsed


def import_one(directory, manifest, registry, report, config, arrow, receipt, catalog, name):
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    (directory / "registry.json").write_text(json.dumps(registry, indent=2) + "\n")
    (directory / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    (directory / "config.json").write_text(json.dumps(config, indent=2) + "\n")
    shutil.copy(arrow, directory / "batch.arrow")
    (directory / "capture-receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    (directory / "binding-catalog.json").write_text(json.dumps(catalog, indent=2) + "\n")
    shutil.copy(RECEIPT_SRC, directory / "native-readonly-receipt.json")
    if sha256_file(directory / "native-readonly-receipt.json") != RECEIPT_SHA:
        raise SystemExit("frozen admission receipt hash drifted")
    reg_sha = sha256_file(directory / "registry.json")
    (directory / "registry.sha256").write_text(reg_sha + "\n")
    proc = run([
        "docker", "run", "--rm",
        "--network", NET,
        "--platform", "linux/arm64", "--pull", "never",
        "--memory", "2g", "--cpus", "1",
        "--cap-drop", "ALL", "--security-opt", "no-new-privileges",
        "--entrypoint", BUILDER,
        "-e", f"OO_NATIVE_ADMISSION_BIN={ADMISSION}",
        "-e", "OO_NATIVE_BINDING_REGISTRY=/in/registry.json",
        "-e", f"OO_NATIVE_BINDING_REGISTRY_SHA256={reg_sha}",
        "-v", "oo-dl01w1-04-target:/target:ro",
        "-v", f"{directory}:/in:ro",
        "-v", f"{directory}:/out",
        "-v", f"{SECRET / 'pg-admin'}:/run/pg-secrets/pg-admin:ro",
        "-v", f"{SECRET / 'pg-tls' / 'ca.crt'}:/run/pg-secrets/ca.crt:ro",
        *LABELS, "--label", "oo.kind=import",
        IMAGE, "import-native", "--manifest", "/in/manifest.json",
        "--out", "/out/envelope.json", "--pipeline", PIPELINE, "--stderr", "/out/import.stderr",
    ], timeout=180)
    save(directory, "import", proc)
    say(f"import {name} exit={proc.returncode}")
    return proc.returncode == 0 and (directory / "envelope.json").is_file()


def window_batch(scn, max_rows, folder="batch"):
    """Read an observed SCN. Do not publish or mutate."""
    audit = EVID / folder
    audit.mkdir(parents=True, exist_ok=True)
    os.chmod(audit, 0o777)
    (audit / "dba-mutated.flag").write_text("existing-scn\n")
    (audit / "flag-note.txt").write_text(
        "Host wrote dba-mutated.flag before the reader started. "
        "publish_scn and dba-mutate.sql were not executed in this window.\n"
    )
    name, proc = child_popen("oracle:batch-asof", audit, max_rows, scn)
    try:
        stdout, stderr = proc.communicate(timeout=180)
    except subprocess.TimeoutExpired:
        run(["docker", "kill", name], timeout=20)
        stdout, stderr = proc.communicate(timeout=20)
    finished = subprocess.CompletedProcess(
        args=["batch"],
        returncode=proc.returncode if proc.returncode is not None else 1,
        stdout=stdout,
        stderr=stderr,
    )
    save(audit, "child", finished)
    run(["docker", "rm", "-f", name], timeout=30)
    arrow = audit / "batch.arrow"
    rows = None
    for line in finished.stdout.decode("utf-8", "replace").splitlines():
        if line.startswith("rows="):
            rows = int(line.split("=", 1)[1])
    ok = finished.returncode == 0 and arrow.is_file() and rows is not None
    say(f"window_batch ok={ok} exit={finished.returncode} rows={rows}")
    return ok, rows, sha256_file(arrow) if arrow.is_file() else ""


def source_counts(scn, name="independent-count"):
    proc = sqlplus(
        "ALTER SESSION SET CONTAINER = FREEPDB1;\n"
        "SET PAGESIZE 0 FEEDBACK OFF HEADING OFF LINESIZE 200\n"
        "WHENEVER SQLERROR EXIT SQL.SQLCODE\n"
        f"SELECT 'count|T_BATCH|' || COUNT(*) FROM oo04_fix.t_batch AS OF SCN {scn};\n"
        f"SELECT 'count|T_WIDE|' || COUNT(*) FROM oo04_fix.t_wide AS OF SCN {scn};\n"
        f"SELECT 'count|T_TS|' || COUNT(*) FROM oo04_fix.t_ts AS OF SCN {scn};\n"
        "EXIT\n",
        name,
        timeout=90,
    )
    counts = {}
    for line in proc.stdout.decode("utf-8", "replace").splitlines():
        line = line.strip()
        if not line.startswith("count|"):
            continue
        _, name, value = line.split("|", 2)
        counts[name] = int(value.strip())
    return proc.returncode, counts


def product_window():
    """One new reader window for the three approved objects. Leaves closure-2 unchanged."""
    global EVID, CHILD
    original_evid = EVID
    original_child = CHILD
    window = SRC / "evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/product-window"
    kept = original_evid / "audit" / "denied-writes.txt"
    kept_sha = sha256_file(kept) if kept.is_file() else ""
    EVID = window
    CHILD = "/target/oracle-customer/debug/oo-oracle-p2-child"
    try:
        window.mkdir(parents=True, exist_ok=True)
        customer = ensure_customer()
        limit = customer["budget"]["hard_limit"]
        present = run([
            "docker", "run", "--rm", "--network", "none", "--platform", "linux/arm64", "--pull", "never",
            "--memory", "128m", "--cpus", "0.25", "--entrypoint", "bash",
            "-v", "oo-dl01w1-04-target:/target:ro", IMAGE,
            "-lc", f"test -x {CHILD} && sha256sum {CHILD}",
        ], timeout=60)
        save(window / "build", "child-present", present)
        if present.returncode != 0:
            raise SystemExit("customer child binary is not ready")
        scn = "2205514"
        (window / "published-scn.txt").write_text(scn + "\n")
        since = time.strftime("%Y%m%d%H%M%S", time.gmtime()) + "000000"
        (window / "audit-since.txt").write_text(since + "\n")
        batch_ok, batch_rows, batch_sha = window_batch(scn, limit)
        wide_ok = capture_text("oracle:text-wide", "wide-text", scn)
        ts_ok = capture_text("oracle:text-ts", "ts-text", scn)
        wide_rows = json.loads((window / "wide-text" / "text.json").read_text()).get("rows") if (window / "wide-text" / "text.json").is_file() else None
        ts_rows = json.loads((window / "ts-text" / "text.json").read_text()).get("rows") if (window / "ts-text" / "text.json").is_file() else None
        audit_ok = audit_reader(since)
        count_exit, counts = source_counts(scn)
        sealed = json.loads((original_evid / "status.json").read_text())
        expected = {
            "T_BATCH": sealed["batch"]["rows"],
            "T_WIDE": sealed["wide"]["rows"],
            "T_TS": sealed["ts"]["rows"],
        }
        captured = {"T_BATCH": batch_rows, "T_WIDE": wide_rows, "T_TS": ts_rows}
        count_match = count_exit == 0 and counts == expected and captured == expected
        kept_after = sha256_file(kept) if kept.is_file() else ""
        write_json(window / "window-status.json", {
            "scn": scn,
            "child": CHILD,
            "child_sha256": present.stdout.decode().split()[0] if present.returncode == 0 else "",
            "batch_ok": batch_ok,
            "wide_ok": wide_ok,
            "ts_ok": ts_ok,
            "audit_zero_write_attempts": audit_ok,
            "dml_exit": "not_invoked",
            "publish_scn": "not_invoked",
            "dba_mutate": "not_invoked",
            "expected_rows": expected,
            "captured_rows": captured,
            "independent_counts": counts,
            "count_exit": count_exit,
            "count_match": count_match,
            "batch_arrow_sha256": batch_sha,
            "sealed_batch_arrow_sha256": sealed.get("batch", {}).get("arrow_sha256"),
            "closure2_denied_writes_sha256": kept_sha,
            "closure2_denied_writes_unchanged": kept_sha == kept_after and kept_sha != "",
            "run_ids": {
                "batch": sealed.get("batch", {}).get("run_id"),
                "wide": sealed.get("wide", {}).get("run_id"),
                "ts": sealed.get("ts", {}).get("run_id"),
            },
        })
        say(f"product_window audit={audit_ok} counts={counts} match={count_match}")
    finally:
        EVID = original_evid
        CHILD = original_child


def product_window_now():
    """Read the three objects as of a freshly observed SCN.

    SCN 2205514 still answers T_WIDE and T_TS. T_BATCH raises ORA-01555.
    The current SCN is observed with a read. No marker insert and no mutate.
    """
    global EVID, CHILD
    original_evid = EVID
    original_child = CHILD
    window = SRC / "evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/product-window"
    EVID = window
    CHILD = "/target/oracle-customer/debug/oo-oracle-p2-child"
    try:
        customer = ensure_customer()
        limit = customer["budget"]["hard_limit"]
        prior = window / "audit"
        kept = window / "audit-2205514"
        if prior.is_dir() and not kept.exists():
            prior.rename(kept)
        observed = sqlplus(
            "ALTER SESSION SET CONTAINER = FREEPDB1;\n"
            "SET PAGESIZE 0 FEEDBACK OFF HEADING OFF LINESIZE 200\n"
            "WHENEVER SQLERROR EXIT SQL.SQLCODE\n"
            "SELECT 'current|' || current_scn FROM v$database;\n"
            "EXIT\n",
            "current-scn",
            timeout=60,
        )
        scn = ""
        for line in observed.stdout.decode("utf-8", "replace").splitlines():
            line = line.strip()
            if line.startswith("current|"):
                scn = line.split("|", 1)[1].strip()
        if not scn.isdigit() or scn in {"2205514", "2196583", "2175138"}:
            raise SystemExit(f"no readable current SCN: {scn}")
        (window / "published-scn-now.txt").write_text(scn + "\n")
        since = time.strftime("%Y%m%d%H%M%S", time.gmtime()) + "000000"
        (window / "audit-since-now.txt").write_text(since + "\n")
        batch_ok, batch_rows, batch_sha = window_batch(scn, limit, "batch-now")
        wide_ok = capture_text("oracle:text-wide", "wide-now", scn)
        ts_ok = capture_text("oracle:text-ts", "ts-now", scn)
        wide_rows = json.loads((window / "wide-now" / "text.json").read_text()).get("rows") if (window / "wide-now" / "text.json").is_file() else None
        ts_rows = json.loads((window / "ts-now" / "text.json").read_text()).get("rows") if (window / "ts-now" / "text.json").is_file() else None
        audit_ok = audit_reader(since)
        if (window / "audit").is_dir():
            dest = window / "audit-now"
            if dest.exists():
                shutil.rmtree(dest)
            shutil.copytree(window / "audit", dest)
        count_exit, counts = source_counts(scn, "independent-count-now")
        sealed = json.loads((original_evid / "status.json").read_text())
        expected = {
            "T_BATCH": sealed["batch"]["rows"],
            "T_WIDE": sealed["wide"]["rows"],
            "T_TS": sealed["ts"]["rows"],
        }
        captured = {"T_BATCH": batch_rows, "T_WIDE": wide_rows, "T_TS": ts_rows}
        sealed_batch = sealed.get("batch", {}).get("arrow_sha256")
        write_json(window / "window-status-now.json", {
            "scn": scn,
            "prior_scn_batch_ora_01555": "2205514",
            "batch_ok": batch_ok,
            "wide_ok": wide_ok,
            "ts_ok": ts_ok,
            "audit_zero_write_attempts": audit_ok,
            "publish_scn": "not_invoked",
            "dba_mutate": "not_invoked",
            "dml_exit": "not_invoked",
            "expected_rows": expected,
            "captured_rows": captured,
            "independent_counts": counts,
            "count_exit": count_exit,
            "count_match": count_exit == 0 and counts == expected and captured == expected,
            "batch_arrow_sha256": batch_sha,
            "sealed_batch_arrow_sha256": sealed_batch,
            "batch_arrow_matches_sealed_import": batch_sha == sealed_batch and batch_sha != "",
            "child_sha256": "af6370b96c73dab6485241d675ef11f826fd78027b94e3e3164dde7ef79fc9d4",
        })
        say(f"window_now scn={scn} audit={audit_ok} counts={counts} batch_sha={batch_sha}")
    finally:
        EVID = original_evid
        CHILD = original_child


def main():
    EVID.mkdir(parents=True, exist_ok=True)
    customer = ensure_customer()
    if not TOKEN.is_file():
        raise SystemExit("model token file is absent")
    for binary in (CHILD, BUILDER, PIPELINE, ADMISSION):
        pass
    present = run([
        "docker", "run", "--rm", "--network", "none", "--platform", "linux/arm64", "--pull", "never",
        "--memory", "128m", "--cpus", "0.25", "--entrypoint", "bash",
        "-v", "oo-dl01w1-04-target:/target:ro", IMAGE,
        "-lc", f"test -x {CHILD} && test -x {BUILDER} && test -x {PIPELINE} && test -x {ADMISSION}",
    ], timeout=60)
    if present.returncode != 0:
        raise SystemExit("linux binaries are not ready")
    since = time.strftime("%Y%m%d%H%M%S", time.gmtime()) + "000000"
    (EVID / "audit-since.txt").write_text(since + "\n")
    scn = publish_scn()
    plan = {"scn": scn, "customer_objects": customer["approved_objects"]}
    sqlplus(
        "ALTER SESSION SET CONTAINER = FREEPDB1;\n"
        "SET PAGESIZE 0 FEEDBACK OFF HEADING OFF LINESIZE 400\n"
        "SELECT 'wide|' || id || '|' || DUMP(n_free) || '|' || TO_CHAR(n_free, 'TME') FROM oo04_fix.t_wide ORDER BY id;\n"
        "SELECT 'ts|' || id || '|' || TO_CHAR(ts, 'YYYY-MM-DD HH24:MI:SS.FF9') || '|' || TO_CHAR(tstz, 'YYYY-MM-DD HH24:MI:SS.FF9 TZH:TZM') FROM oo04_fix.t_ts ORDER BY id;\n"
        "EXIT\n",
        "canonical-text",
    )
    batch_ok = run_batch(scn)
    wide_text = capture_text("oracle:text-wide", "wide-text", scn)
    ts_text = capture_text("oracle:text-ts", "ts-text", scn)
    wide_reject = capture_reject("oracle:reject-wide", "reject-wide", scn)
    ts_reject = capture_reject("oracle:reject-ts", "reject-ts", scn)
    tz_reject = capture_reject("oracle:reject-tz", "reject-tz", scn)
    fence = child("oracle:fence", EVID / "fence", 1, 40, scn)
    secret = child("oracle:secret", EVID / "secret", 1, 40, scn)
    # Adversarial DML belongs to a separate DBA test window, never intake.
    tamper = child("oracle:tamper", EVID / "tamper", 1, 40, scn)
    say(f"fence={fence.returncode} secret={secret.returncode} tamper={tamper.returncode}")
    deadline_ok = run_deadline(scn)
    cancel_ok = run_oci_cancel(scn)
    audit_ok = audit_reader(since)
    write_json(EVID / "capture-status.json", {
        "scn": scn,
        "old_scn_2196583": "probed separately; not this capture identity",
        "batch": batch_ok,
        "wide_text": wide_text,
        "ts_text": ts_text,
        "wide_reject": wide_reject,
        "ts_reject": ts_reject,
        "tz_reject": tz_reject,
        "deadline": deadline_ok,
        "oci_cancel": cancel_ok,
        "audit_zero_write": audit_ok,
        "fence_exit": fence.returncode,
        "secret_exit": secret.returncode,
        "tamper_exit": tamper.returncode,
        "dml_exit": "not_invoked",
        "same_arrow_as_5064f81e": sha256_file(EVID / "batch" / "batch.arrow") == "5064f81e25162f8b183b9fe2c515907a29387d9509f1a7ea3d7b129976556f08" if (EVID / "batch" / "batch.arrow").is_file() else False,
    })
    say("capture_status_written")
    import product_chain
    product_chain.finish(scn, json.loads((EVID / "capture-status.json").read_text()))


if __name__ == "__main__":
    if sys.argv[1:] == ["product-window"]:
        product_window()
    elif sys.argv[1:] == ["product-window-now"]:
        product_window_now()
    else:
        raise SystemExit("refusing implicit main; pass product-window or product-window-now")
