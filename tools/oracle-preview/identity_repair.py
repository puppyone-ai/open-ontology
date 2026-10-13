#!/usr/bin/env python3
"""DL01-W1-04 review-67 measurements.

The child writes the readonly audit, the deadline, and the OCI cancel.
This process does not copy a foreign admission receipt, does not publish an SCN,
and does not run dba-mutate.sql.
"""
import hashlib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import full_source as fs

SRC = fs.SRC
LIVE = SRC / "evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/live-chain"
ID = LIVE / "identity-67"
CONFIG = Path("/Users/redux/.oo-dl01w1-04-p2/customer-product-window.json")
REGISTRY = Path("/Users/redux/.oo-dl01w1-04-p2/secret-registry.json")
SCN = "6395124"
CHILD = "/target/oracle-customer/debug/oo-oracle-p2-child"
SEMANTICS = "/target/oracle-semantics/debug/oo-source-semantics-pack"
FOREIGN = fs.RECEIPT_SHA
CHARACTER = {"CHAR", "NCHAR", "VARCHAR2", "NVARCHAR2", "CLOB", "NCLOB"}
COLUMN_MAP = {
    "T_BATCH": {"ID": "id", "N16": "n16", "N1": "n1", "D": "d", "C6": "c6", "V2": "v2", "R": "r"},
    "T_WIDE": {"ID": "id", "N_FREE": "n_free_text"},
    "T_TS": {"ID": "id", "TS": "ts_text", "TSTZ": "tstz_text"},
}
OBJECTS = (
    {"name": "batch", "import_dir": "import-batch", "catalog_dir": "catalog-batch", "builder": "builder", "logical": "t_batch", "rows": 5, "dictionary": "T_BATCH"},
    {"name": "wide", "import_dir": "import-wide", "catalog_dir": "catalog-wide", "builder": "builder-wide", "logical": "t_wide_text", "rows": 4, "dictionary": "T_WIDE"},
    {"name": "ts", "import_dir": "import-ts", "catalog_dir": "catalog-ts", "builder": "builder-ts", "logical": "t_ts_text", "rows": 2, "dictionary": "T_TS"},
)


def say(text):
    print(text, flush=True)


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_json(path, doc):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n")


def save(directory, name, proc):
    directory.mkdir(parents=True, exist_ok=True)
    (directory / f"{name}.stdout").write_bytes(proc.stdout or b"")
    (directory / f"{name}.stderr").write_bytes(proc.stderr or b"")
    (directory / f"{name}.exit").write_text(str(proc.returncode) + "\n")


def prepare_customer():
    os.environ["OO_ORACLE_CUSTOMER_CONFIG"] = str(CONFIG)
    os.environ["OO_ORACLE_SECRET_REGISTRY"] = str(REGISTRY)
    fs.EVID = ID
    fs.CHILD = CHILD
    return fs.ensure_customer()


def run_refusals():
    out = ID / "refusals"
    out.mkdir(parents=True, exist_ok=True)
    base = json.loads(CONFIG.read_text())
    cases = []
    unknown_object = json.loads(json.dumps(base))
    unknown_object["selections"][0]["object"] = "OO04_FIX.T_NOT_GRANTED"
    cases.append(("unknown-object", unknown_object, "unapproved object"))
    unknown_field = json.loads(json.dumps(base))
    unknown_field["selections"][0]["fields"] = ["ID", "N16", "N1", "D", "C6", "V2", "NOT_A_FIELD"]
    cases.append(("unknown-field", unknown_field, "field"))
    bad_budget = json.loads(json.dumps(base))
    bad_budget["budget"]["hard_limit"] = 9
    cases.append(("wrong-budget", bad_budget, "budget"))
    ok = True
    for name, doc, needle in cases:
        directory = out / name
        directory.mkdir(parents=True, exist_ok=True)
        config = directory / "customer.json"
        registry = directory / "registry-not-opened.json"
        if registry.exists():
            raise SystemExit(f"{registry} already exists")
        config.write_text(json.dumps(doc) + "\n")
        proc = subprocess.run(
            [sys.executable, str(SRC / "tools/oracle-preview/customer_input.py"), "refuse", str(config), str(registry)],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False,
        )
        save(directory, "refuse", proc)
        body = {}
        try:
            body = json.loads(proc.stdout.decode() or "{}")
        except json.JSONDecodeError:
            body = {}
        step = next((item for item in body.get("trace", []) if item.get("step") == "registry_before_open"), {})
        passed = (
            proc.returncode != 0
            and needle in body.get("error", "")
            and body.get("registry_opened") is False
            and body.get("password_read") is False
            and body.get("source_io") is False
            and step.get("registry_io") is False
            and step.get("no_password_read") is True
            and step.get("oracle_read") is False
            and not registry.exists()
            and b"FileNotFoundError" not in proc.stdout
            and b"FileNotFoundError" not in proc.stderr
        )
        (directory / "passed.txt").write_text("true\n" if passed else "false\n")
        say(f"refusal {name} exit={proc.returncode} passed={passed}")
        ok = ok and passed
    return ok


def child_cmd(name, audit):
    return [
        "docker", "run", "--name", name, "-i",
        "--network", fs.NET_SRC,
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
        "-v", f"{fs.CUSTOMER_DOC['_password_file']}:/run/secrets/source-password:ro",
        "-v", f"{audit}:/audit",
        *fs.LABELS,
        "--label", "oo.kind=oracle-child",
        fs.IMAGE,
    ]


def request_for(statement, max_rows):
    return {
        "protocol": "oo.source.driver-oracle/v2",
        "statement_ref": statement,
        "generation": 2,
        "max_rows": max_rows,
        "source_host": fs.ORA,
        "source_user": fs.CUSTOMER_DOC["source"]["reader"],
        "source_service": fs.CUSTOMER_DOC["source"]["service"],
        "image_id": fs.IMAGE_ID,
        "decoy_host": "oo-dl01w1-04-p2-decoy",
        "lineage_pad": False,
    }


def run_child(statement, audit, max_rows, timeout, name):
    audit.mkdir(parents=True, exist_ok=True)
    os.chmod(audit, 0o777)
    (audit / "published-scn").write_text(SCN + "\n")
    fs.run(["docker", "rm", "-f", name], timeout=30)
    request = request_for(statement, max_rows)
    (audit / "request.json").write_text(json.dumps(request) + "\n")
    proc = fs.run(child_cmd(name, audit), timeout=timeout, input_bytes=json.dumps(request).encode() + b"\n")
    save(audit, "child", proc)
    fs.run(["docker", "rm", "-f", name], timeout=30)
    return proc


def session_status(sid, serial, label):
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
    proc = fs.sqlplus(sql, label)
    return proc.stdout.decode("utf-8", "replace")


def still_active(audit, label):
    session = audit / "session.txt"
    if not session.exists():
        return True, ""
    sid, serial = session.read_text().strip().split(",")
    text = session_status(sid, serial, label)
    (audit / f"{label}.txt").write_text(text)
    active = "status|ACTIVE|" in text and "oo-dl01w1-04-p2-cancel" in text
    return active, text


def validate(directory):
    proc = fs.run([
        "docker", "run", "--rm", "--network", "none",
        "--platform", "linux/arm64", "--pull", "never",
        "--memory", "512m", "--cpus", "0.5",
        "--cap-drop", "ALL", "--security-opt", "no-new-privileges",
        "--entrypoint", fs.ADMISSION,
        "-v", "oo-dl01w1-04-target:/target:ro",
        "-v", f"{directory}:/in:ro",
        *fs.LABELS, "--label", "oo.kind=admission",
        fs.IMAGE, "validate", "--report", "/in/report.json", "--config", "/in/config.json",
    ], timeout=60)
    save(directory, "validate", proc)
    (directory / "decision.json").write_bytes(proc.stdout)
    return proc


def copy_identity(spec):
    admit = ID / spec["name"] / "admit"
    admit.mkdir(parents=True, exist_ok=True)
    imported = LIVE / spec["import_dir"]
    shutil.copy(imported / "report.json", admit / "report.json")
    shutil.copy(imported / "config.json", admit / "config.json")
    proc = validate(admit)
    audit = ID / spec["name"] / "audit"
    ident = audit / "identity"
    ident.mkdir(parents=True, exist_ok=True)
    shutil.copy(imported / "manifest.json", ident / "manifest.json")
    shutil.copy(imported / "registry.json", ident / "registry.json")
    shutil.copy(LIVE / spec["catalog_dir"] / "catalog.json", ident / "catalog.json")
    shutil.copy(LIVE / spec["builder"] / "out-fresh" / "model-input.json", ident / "model-input.json")
    shutil.copy(admit / "decision.json", ident / "decision.json")
    shutil.copy(imported / "batch.arrow", ident / "batch.arrow")
    for path in ident.rglob("*"):
        if path.is_file():
            os.chmod(path, 0o644)
    os.chmod(ident, 0o755)
    os.chmod(audit, 0o777)
    return proc, audit


def run_audits():
    results = []
    for spec in OBJECTS:
        proc, audit = copy_identity(spec)
        child = run_child("oracle:readonly-audit", audit, 1, 90, f"oo-dl01w1-04-id67-audit-{spec['name']}")
        receipt_path = audit / "readonly-audit.json"
        stdout = (audit / "child.stdout").read_bytes()
        same = receipt_path.is_file() and stdout == receipt_path.read_bytes()
        body = {}
        if receipt_path.is_file():
            body = json.loads(receipt_path.read_text())
        decision_sha = sha256_file(ID / spec["name"] / "admit" / "validate.stdout")
        text = receipt_path.read_text() if receipt_path.is_file() else ""
        passed = (
            proc.returncode in (0, 2)
            and child.returncode == 0
            and same
            and bool(stdout)
            and body.get("ok") is True
            and body.get("protocol") == "oo.oracle-capture.readonly-audit/v1"
            and body.get("scn") == SCN
            and body.get("oracle_count") in {str(spec["rows"]), f"{spec['rows']}.0"}
            and body.get("manifest_row_count") == spec["rows"]
            and body.get("mounted_file_is_foreign_receipt") is False
            and body.get("replaces_admission_slot") is False
            and body.get("process_kill") is False
            and body.get("foreign_admission_sha256") == FOREIGN
            and body.get("files", {}).get("decision.json") == decision_sha
            and bool(body.get("schema_digest"))
            and body.get("schema_digest") == body.get("model_input_schema_digest")
            and "oo.native-readonly-completion/receipt/v1" not in text
            and "clickhouse_and_opensearch" not in text
        )
        (audit / "passed.txt").write_text("true\n" if passed else "false\n")
        say(f"audit {spec['name']} validate={proc.returncode} child={child.returncode} ok={body.get('ok')} passed={passed}")
        results.append({"name": spec["name"], "passed": passed, "body": body})
    return results


def run_deadline():
    audit = ID / "deadline"
    proc = run_child("oracle:deadline-select", audit, 1, 40, "oo-dl01w1-04-id67-deadline")
    active, _text = still_active(audit, "deadline-after")
    raw = audit / "read-zero.json"
    stdout = (audit / "child.stdout").read_bytes() if (audit / "child.stdout").exists() else b""
    body = json.loads(raw.read_text()) if raw.is_file() else {}
    passed = (
        proc.returncode == 3
        and not active
        and raw.is_file()
        and stdout == raw.read_bytes()
        and len(stdout) > 0
        and len((audit / "child.stderr").read_bytes()) > 0
        and body.get("kind") == "deadline"
        and body.get("process_kill") is False
        and body.get("not_a_cancel") is True
        and body.get("client_timeout") is True
        and body.get("cancel_api") == "absent"
        and body.get("native_cancel_capability") == "not_called"
        and body.get("rows_committed") == 0
        and "elapsed_ms" in body
        and body.get("published_scn") == SCN
    )
    (audit / "passed.txt").write_text("true\n" if passed else "false\n")
    say(f"deadline exit={proc.returncode} active={active} passed={passed}")
    return passed


def run_oci():
    audit = ID / "oci-cancel"
    audit.mkdir(parents=True, exist_ok=True)
    os.chmod(audit, 0o777)
    (audit / "published-scn").write_text(SCN + "\n")
    name = "oo-dl01w1-04-id67-oci"
    fs.run(["docker", "rm", "-f", name], timeout=30)
    request = request_for("oracle:oci-cancel", 1)
    (audit / "request.json").write_text(json.dumps(request) + "\n")
    proc = subprocess.Popen(child_cmd(name, audit), stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    proc.stdin.write(json.dumps(request).encode() + b"\n")
    proc.stdin.close()
    seen = None
    import time
    deadline = time.time() + 45
    while time.time() < deadline:
        session = audit / "session.txt"
        if session.exists():
            sid, serial = session.read_text().strip().split(",")
            text = session_status(sid, serial, "oci-before")
            (audit / "oci-before.txt").write_text(text)
            if "status|ACTIVE|" in text and "oo-dl01w1-04-p2-cancel" in text:
                seen = text
                break
        time.sleep(1)
    killed = False
    if seen:
        (audit / "cancel-now").write_text("1\n")
        say("oci_cancel_flag_written")
    try:
        stdout, stderr = proc.communicate(timeout=60)
    except subprocess.TimeoutExpired:
        killed = True
        fs.run(["docker", "kill", name], timeout=20)
        stdout, stderr = proc.communicate(timeout=20)
    finished = subprocess.CompletedProcess(args=["oci"], returncode=proc.returncode if proc.returncode is not None else 1, stdout=stdout, stderr=stderr)
    save(audit, "child", finished)
    (audit / "process-kill.txt").write_text("true\n" if killed else "false\n")
    fs.run(["docker", "rm", "-f", name], timeout=30)
    active, _text = still_active(audit, "oci-after")
    raw = audit / "read-zero.json"
    body = json.loads(raw.read_text()) if raw.is_file() else {}
    passed = (
        seen is not None
        and not killed
        and finished.returncode == 0
        and not active
        and raw.is_file()
        and stdout == raw.read_bytes()
        and len(stdout) > 0
        and len(stderr) > 0
        and b"native_cancel_capability=request_cancel" in stderr
        and body.get("cancel_api") == "oci_marker_interrupt"
        and body.get("native_cancel_capability") == "request_cancel"
        and "Cancelled" in str(body.get("error_kind"))
        and body.get("rows_committed") == 0
        and body.get("not_call_timeout") is True
        and body.get("ready") is False
        and body.get("process_kill") is False
        and body.get("published_scn") == SCN
    )
    (audit / "passed.txt").write_text("true\n" if passed else "false\n")
    say(f"oci exit={finished.returncode} killed={killed} active={active} passed={passed}")
    return passed


def run_completion():
    audit = ID / "completion-batch"
    audit.mkdir(parents=True, exist_ok=True)
    os.chmod(audit, 0o777)
    (audit / "dba-mutated.flag").write_text("no-dba-mutate\n")
    (audit / "NOT-RUN.txt").write_text("dba-mutate.sql was not executed. The flag only releases the read.\n")
    request = request_for("oracle:batch-asof", 5)
    request["source_object"] = "OO04_FIX.T_BATCH"
    audit.mkdir(parents=True, exist_ok=True)
    (audit / "published-scn").write_text(SCN + "\n")
    name = "oo-dl01w1-04-id67-batch"
    fs.run(["docker", "rm", "-f", name], timeout=30)
    (audit / "request.json").write_text(json.dumps(request) + "\n")
    proc = fs.run(child_cmd(name, audit), timeout=180, input_bytes=json.dumps(request).encode() + b"\n")
    save(audit, "child", proc)
    fs.run(["docker", "rm", "-f", name], timeout=30)
    text = (audit / "completion.txt").read_text() if (audit / "completion.txt").exists() else ""
    passed = proc.returncode == 0 and "cancel_api=absent\n" in text and "scn=6395124\n" in text and "admits_import=false\n" in text and "cancel_api=oci_marker_interrupt\n" not in text
    (audit / "passed.txt").write_text("true\n" if passed else "false\n")
    say(f"completion exit={proc.returncode} passed={passed}")
    return passed


def fact_for(schema, table, sealed_name, source_type, row):
    _column, data_type, precision, scale, _char_len = row
    fact = {
        "schema": schema,
        "table": table,
        "name": sealed_name,
        "native_type": source_type,
    }
    if data_type in CHARACTER:
        fact["empty_string_is_null"] = True
    if data_type == "NUMBER" and precision == "-":
        fact["unconstrained_number"] = True
    if data_type.startswith("TIMESTAMP") and scale.isdigit() and int(scale) > 6:
        fact["fractional_seconds"] = int(scale)
    if "WITH TIME ZONE" in data_type:
        fact["offset_lost"] = False
    return fact


def run_semantics(audits):
    out = ID / "semantics"
    out.mkdir(parents=True, exist_ok=True)
    problems = []
    for spec, audit in zip(OBJECTS, audits):
        body = audit["body"]
        rows = body.get("dictionary_columns") or []
        parsed = {}
        for line in rows:
            parts = line.split("|")
            if len(parts) != 5:
                continue
            parsed[parts[0]] = parts
        catalog = json.loads((LIVE / spec["catalog_dir"] / "catalog.json").read_text())
        table = catalog["tables"][0]
        facts = []
        mapping = COLUMN_MAP[spec["dictionary"]]
        for column in table["columns"]:
            oracle_name = next(key for key, value in mapping.items() if value == column["logical_name"])
            if oracle_name not in parsed:
                problems.append(f"{spec['name']} missing dictionary {oracle_name}")
                continue
            facts.append(fact_for(table["logical_schema"], table["logical_table"], column["logical_name"], column["source_type"], parsed[oracle_name]))
        banner = body.get("banner") or ""
        version = ""
        for token in banner.replace("-", " ").split():
            if token[:1].isdigit() and token.count(".") >= 2:
                version = token
                break
        if not version:
            problems.append(f"{spec['name']} banner has no version token")
            continue
        reviewed = json.loads((LIVE / spec["builder"] / "out-fresh" / "reviewed.json").read_text())
        dataset_name = f"{table['logical_schema']}.{table['logical_table']}"
        dataset = next(item for item in reviewed["effective_oos"]["datasets"] if item["dataset"] == dataset_name)
        decisions = []
        for column in dataset["columns"]:
            decisions.append({
                "dataset": dataset_name,
                "column": column["name"],
                "pii": column["pii"],
                "semantic_type": column["semantic_type"],
                "reviewer": {"by": column["classification"]["by"], "at": column["classification"]["at"]},
            })
        directory = out / spec["name"]
        directory.mkdir(parents=True, exist_ok=True)
        write_json(directory / "dictionary.json", {"columns": rows, "banner": banner})
        write_json(directory / "target.json", {
            "family": "oracle",
            "product": "oracle",
            "version": version,
            "capture": "sealed_import",
            "source_consistency": "S1",
            "delete_detection": "reconcile",
            "source_kind": "oracle_table",
            "chunk": {"key_kind": "integer"},
            "columns": facts,
            "constraints": [],
        })
        shutil.copy(LIVE / spec["builder"] / "settings.json", directory / "settings.json")
        shutil.copy(LIVE / spec["builder"] / "out-fresh" / "visibility.json", directory / "visibility.json")
        write_json(directory / "decisions.json", decisions)
        shutil.copy(LIVE / spec["catalog_dir"] / "catalog.json", directory / "catalog.json")
        produced = directory / "out"
        produced.mkdir(parents=True, exist_ok=True)
        os.chmod(directory, 0o777)
        os.chmod(produced, 0o777)
        proc = fs.run([
            "docker", "run", "--rm", "--network", "none",
            "--platform", "linux/arm64", "--pull", "never",
            "--user", "65534:65534",
            "--memory", "512m", "--cpus", "0.5",
            "--cap-drop", "ALL", "--security-opt", "no-new-privileges",
            "--entrypoint", SEMANTICS,
            "-v", "oo-dl01w1-04-target:/target:ro",
            "-v", f"{directory}:/in",
            *fs.LABELS, "--label", "oo.kind=semantics-pack",
            fs.IMAGE,
            "/in/catalog.json", "/in/target.json", "/in/settings.json", "/in/visibility.json", "/in/decisions.json", "/in/out",
        ], timeout=60)
        save(directory, "semantics", proc)
        result_path = produced / "result.json"
        result = json.loads(result_path.read_text()) if result_path.is_file() else {}
        expected_identity = json.loads((LIVE / spec["builder"] / "out-fresh" / "result.json").read_text())["identity_hash"]
        identity = (produced / "identity-hash.txt").read_text().strip() if (produced / "identity-hash.txt").exists() else ""
        digest = (produced / "schema-digest.txt").read_text().strip() if (produced / "schema-digest.txt").exists() else ""
        page = ""
        for path in produced.glob("tables__*.md"):
            page += path.read_text()
        columns = {item.get("column"): item for item in result.get("columns") or []}
        passed = proc.returncode == 0 and result.get("status") == "packed" and identity == expected_identity and digest == catalog["schema_digest"]
        if spec["name"] == "batch":
            for name in ("batch.t_batch.c6", "batch.t_batch.v2"):
                passed = passed and "empty_string_is_null" in (columns.get(name, {}).get("caveats") or [])
            passed = passed and "empty_string_is_null" not in (columns.get("batch.t_batch.id", {}).get("caveats") or [])
            passed = passed and "empty_string_is_null" in page
        if spec["name"] == "wide":
            note = columns.get("batch.t_wide_text.n_free_text", {}).get("native_note")
            passed = passed and note == "unconstrained_number" and "unconstrained_number" in page
        if spec["name"] == "ts":
            for name in ("batch.t_ts_text.ts_text", "batch.t_ts_text.tstz_text"):
                caveats = columns.get(name, {}).get("caveats") or []
                passed = passed and "precision_loss" in caveats and "offset_lost" not in caveats
            passed = passed and "precision_loss" in page and "offset_lost" not in page
        result_text = result_path.read_text() if result_path.is_file() else ""
        passed = passed and "time_ambiguous" not in page and "time_ambiguous" not in result_text
        (directory / "passed.txt").write_text("true\n" if passed else "false\n")
        say(f"semantics {spec['name']} exit={proc.returncode} status={result.get('status')} passed={passed}")
        if not passed:
            problems.append(spec["name"])
    return problems


def reader_module():
    sql = (
        "SET PAGESIZE 0 FEEDBACK OFF HEADING OFF LINESIZE 240\n"
        "ALTER SESSION SET CONTAINER = FREEPDB1;\n"
        "SELECT 'module_writes|' || COUNT(*) FROM v$sql WHERE module = 'oo-dl01w1-04-p2-reader' AND (\n"
        "  UPPER(sql_text) LIKE '%INSERT %' OR UPPER(sql_text) LIKE '%UPDATE %' OR UPPER(sql_text) LIKE '%DELETE %'\n"
        "  OR UPPER(sql_text) LIKE '%MERGE %' OR UPPER(sql_text) LIKE '%TRUNCATE %' OR UPPER(sql_text) LIKE '%DROP %');\n"
        "SELECT 'stmt|' || SUBSTR(REPLACE(REPLACE(sql_text, CHR(10), ' '), CHR(13), ' '), 1, 180)\n"
        "FROM v$sql WHERE module = 'oo-dl01w1-04-p2-reader' AND ROWNUM <= 40;\n"
        "EXIT\n"
    )
    proc = fs.sqlplus(sql, "reader-module")
    say(f"reader-module exit={proc.returncode}")
    return proc.returncode == 0


def preflight():
    sql = (
        "SET PAGESIZE 0 FEEDBACK OFF HEADING OFF LINESIZE 200\n"
        "ALTER SESSION SET CONTAINER = FREEPDB1;\n"
        "SELECT 'pdb|' || open_mode FROM v$pdbs WHERE name = 'FREEPDB1';\n"
        "SELECT 'current|' || current_scn FROM v$database;\n"
        "SELECT 'batch|' || COUNT(*) FROM oo04_fix.t_batch AS OF SCN 6395124;\n"
        "SELECT 'wide|' || COUNT(*) FROM oo04_fix.t_wide AS OF SCN 6395124;\n"
        "SELECT 'ts|' || COUNT(*) FROM oo04_fix.t_ts AS OF SCN 6395124;\n"
        "EXIT\n"
    )
    proc = fs.sqlplus(sql, "preflight")
    text = proc.stdout.decode("utf-8", "replace")
    say(text.strip())
    return proc.returncode == 0 and "batch|5" in text and "wide|4" in text and "ts|2" in text


def elf_sha():
    proc = fs.run([
        "docker", "run", "--rm", "--network", "none", "--pull", "never",
        "--entrypoint", "/usr/bin/sha256sum",
        "-v", "oo-dl01w1-04-target:/target:ro",
        fs.IMAGE, CHILD,
    ], timeout=60)
    (ID / "elf.sha256").write_bytes(proc.stdout)
    say(proc.stdout.decode().strip())
    return proc.returncode == 0


def exclude_arrows():
    exclude = SRC / ".git/info/exclude"
    text = exclude.read_text() if exclude.exists() else ""
    lines = []
    for path in ID.rglob("*.arrow"):
        rel = path.relative_to(SRC).as_posix()
        line = "!" + rel
        if line not in text:
            lines.append(line)
    if lines:
        with exclude.open("a") as handle:
            handle.write("\n# DL01-W1-04 identity-67 arrows\n")
            handle.write("\n".join(lines) + "\n")


def write_reports(status):
    checks = status["checks"]
    if status["ok"]:
        narrative = [
            f"SCN `{SCN}` stayed readable. Batch, wide, and ts counts are 5, 4, and 2.",
            "The child `oracle:readonly-audit` hashed the manifest, registry, sealed catalog, model input, this run's validator stdout, and the Arrow body, then counted `AS OF` that SCN.",
            "schema_digest remains the sealed Arrow schema hash. The pack page does not need the SCN digits.",
            f"Decision files still have no SCN field. `decision_contains_scn` is recorded on each receipt. The frozen import gate still accepts only admission receipt `{FOREIGN}`. This receipt does not replace that slot and does not attach the task 38 body.",
            "",
            "OCI cancel calls `request_cancel` and records `cancel_api=oci_marker_interrupt` only after `ErrorKind::Cancelled`. The batch completion records `cancel_api=absent` because that read does not interrupt. The success path does not kill the container.",
            "Caveat tokens come from `compile_source_semantics` over the sealed catalogs and the reader data-dictionary rows. `empty_string_is_null` is on CHAR and VARCHAR2. `precision_loss` is on TIMESTAMP(9). `unconstrained_number` is on unconstrained NUMBER. `offset_lost` is not set because the sealed text keeps the offset. `time_ambiguous` is not a token of this compiler.",
        ]
    else:
        narrative = [
            "This measurement did not close the four gaps. The checks object below is the raw result. Failed steps stay failed.",
            "`source_acceptance` is `not_claimed`.",
        ]
    report = [
        "# DL01-W1-04 identity-67",
        "",
        *narrative,
        "",
        "```",
        json.dumps(checks, indent=2, sort_keys=True),
        "```",
        "",
        "`source_acceptance` is `not_claimed`.",
        "",
    ]
    if status["ok"]:
        findings = [
            "# Findings",
            "",
            "RC1. The public Decision type has no snapshot field, and the sealed catalog parser rejects an added SCN field. schema_digest is the Arrow schema hash, so it was not replaced with an SCN hash. The new binding is the child readonly-audit receipt. The historical task 38 receipt stays where the frozen gate already required it. It is not this capture's admission proof.",
            "",
            "RC2. Deadline and OCI cancel were run on the successor ELF. Stdout is the measurement JSON. Stderr records `native_cancel_capability`. The OCI success path did not use docker kill. The old closure-2 deadline file and the old ORA-01555 body were not rewritten.",
            "",
            "RC3. Sealed catalog `caveat` stays null because the sealer writes none for these storage types. The source-semantics pack declares the flags that apply to the measured Oracle types. DATE keeps its time in `timestamp_micros`; the compiler note `oracle_date_includes_time` requires native type `date`, which would be `type.ambiguous` against the sealed token.",
            "",
            "RC4. Unknown object, unknown field, and hard_limit 9 each exited through `CustomerRefusal` before the registry was opened. `secret-open.json` was not rerun.",
            "",
            "Passed local typed, guard, widen, 57014, and source-off results stay on the existing runs. This packet does not relabel them.",
            "",
        ]
    else:
        findings = [
            "# Findings",
            "",
            "The measurement did not close RC1–RC4. See `status.json` and the per-step `passed.txt` files. Do not read a failed step as a new chain.",
            "",
            "RC4 refusals, when `refusals/*/passed.txt` says true, were recorded before this measurement and were not rerun.",
            "",
        ]
    handoff = [
        "# Handoff",
        "",
        "Task DL01-W1-04. Author measurement for fixed review 67. `source_acceptance` is `not_claimed`.",
        "Root does the formal grade, parent, release, and the source counter.",
        "Do not treat a Decision copy or the task 38 receipt as this snapshot's admission.",
        "Do not retitle snapshot oo-dl01w1-04-scn-2205514. Do not edit the ORA-01555 body or closure-2/audit/denied-writes.txt.",
        "Do not grant EXECUTE on SYS.DBMS_FLASHBACK. Do not change the root Cargo.lock.",
        "",
        f"checks_passed={status['ok']}",
        "",
    ]
    (ID / "REPORT.md").write_text("\n".join(report))
    (ID / "FINDINGS.md").write_text("\n".join(findings))
    (ID / "HANDOFF.md").write_text("\n".join(handoff))
    write_json(ID / "status.json", status)


def measure():
    ID.mkdir(parents=True, exist_ok=True)
    prepare_customer()
    checks = {
        "elf": elf_sha(),
        "preflight": preflight(),
    }
    audits = run_audits()
    checks["audits"] = [item["name"] for item in audits if item["passed"]]
    checks["deadline"] = run_deadline()
    checks["oci"] = run_oci()
    checks["completion"] = run_completion()
    checks["semantics_problems"] = run_semantics(audits)
    checks["reader_module"] = reader_module()
    exclude_arrows()
    ok = (
        checks["elf"] and checks["preflight"] and len(checks["audits"]) == 3
        and checks["deadline"] and checks["oci"] and checks["completion"]
        and checks["semantics_problems"] == [] and checks["reader_module"]
    )
    status = {"ok": ok, "scn": SCN, "checks": checks, "source_acceptance": "not_claimed"}
    write_reports(status)
    say(f"measure ok={ok}")
    return 0 if ok else 1


def main():
    if (ID / "REPORT.md").is_file() or (ID / "receipt.json").is_file():
        raise SystemExit(
            "identity-67 evidence is frozen. The readonly audit does not admit import. "
            "Refusing to rewrite it."
        )
    if len(sys.argv) != 2 or sys.argv[1] not in {"refusals", "measure"}:
        raise SystemExit("usage: identity_repair.py refusals|measure")
    if sys.argv[1] == "refusals":
        ID.mkdir(parents=True, exist_ok=True)
        return 0 if run_refusals() else 1
    return measure()


if __name__ == "__main__":
    raise SystemExit(main())
