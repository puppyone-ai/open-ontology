#!/usr/bin/env python3
"""Live StarRocks C4/C5 schedule for DL01-W1-45.

Uses the sealed probe and decode ELF. Does not call run_gate.main, does not
start a Rust compiler, and does not delete Docker volumes or the task network.
"""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import socket
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

import capture_schedule as cap
import closure_gate

REPO = Path("/Users/redux/puppy-one/open-ontology")
TASK = "DL01-W1-45"
SESSION = "2c023948-0862-4dae-ae8f-6fc30241db62"
PREFIX = "oo-dl01w1-45"
IMAGE = "starrocks/allin1-ubuntu@sha256:9b98696ee0cb46831eabe197ae5e71f483ee3c2e16ab57fd27cb51a5c9ee6618"
PIN = "sha256:9b98696ee0cb46831eabe197ae5e71f483ee3c2e16ab57fd27cb51a5c9ee6618"
PG_IMAGE = "docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb"
CONTAINER = f"{PREFIX}-starrocks"
PG_CONTAINER = f"{PREFIX}-postgres"
NETWORK = PREFIX
SRC = REPO / "testing" / "dl01-driver-preflight" / "starrocks"
EVIDENCE = REPO / "evidence" / SESSION / "DL01" / "starrocks-product" / "raw" / "capture-schedule"
ADMIT = SRC / "scripts" / "admit.py"
PROBE = REPO / "evidence" / SESSION / "DL01" / "starrocks-driver-preflight" / "build" / "bin" / "starrocks-probe"
DECODE = REPO / "evidence" / SESSION / "DL01" / "starrocks-driver-preflight" / "build" / "bin" / "starrocks-decode"
REGISTRY = REPO / "evidence" / SESSION / "DL01" / "starrocks-product" / "raw" / "native-bind" / "registry.json"
EXPECTED = SRC / "fixtures" / "expected.json"
SECRET_DIR = Path("/tmp/oo-dl01w1-45-capture-secrets")
PROBE_SHA = "21bbdc81d436bbffb2951b2245068c1667dccd02ed32a6667b3770b4b7bd1aee"
DECODE_SHA = "016be98241f4e4214bbf4d9e5826e80edaf881ceab4dcb2d62eb42a64ec50b9f"
REGISTRY_SHA = "6833baeb559ef004fcf510cdc260b8a9818ee223389f3d3fe560b2c92011eca2"
TOKEN_PATH = Path("/tmp/oo-customer-demo-20261007/llm-token")
BASE_TABLES = ("dup_events", "pk_accounts", "agg_metrics", "uniq_dim")
TABLES = BASE_TABLES + ("dup_partition_sums",)
APPROVED_COLUMNS = {
    "dup_events": ["event_id", "bucket", "seen", "amount", "note"],
    "pk_accounts": ["account_id", "balance", "nickname", "opened"],
    "agg_metrics": ["metric_day", "metric_name", "qty", "revenue"],
    "uniq_dim": ["dim_id", "label", "score"],
}
FAST = {"config_id": "user-fast", "sync": "partition_refresh", "every_ms": 8000, "window_ms": 8000, "overlap_ms": 0}
SLOW = {"config_id": "user-slow", "sync": "full", "every_ms": 20000, "window_ms": 20000, "overlap_ms": 0}
QUERIES = {
    "dup_events": "SELECT event_id, bucket, seen, amount, note FROM dup_events ORDER BY event_id, bucket, IFNULL(note, ''), IFNULL(CAST(amount AS STRING), '')",
    "pk_accounts": "SELECT account_id, balance, nickname, opened FROM pk_accounts ORDER BY account_id",
    "agg_metrics": "SELECT metric_day, metric_name, qty, revenue FROM agg_metrics ORDER BY metric_day, metric_name",
    "uniq_dim": "SELECT dim_id, label, score FROM uniq_dim ORDER BY dim_id",
    "dup_partition_sums": "SELECT CASE WHEN bucket < '2026-06-01' THEN 'p202605' ELSE 'p202606' END, COUNT(*), CAST(SUM(amount) AS DECIMAL(38, 4)) FROM dup_events GROUP BY CASE WHEN bucket < '2026-06-01' THEN 'p202605' ELSE 'p202606' END ORDER BY 1",
}
PG_DDL = {
    "dup_events": "CREATE TABLE capture.dup_events (event_id bigint, bucket date, seen timestamp, amount numeric(18,4), note text)",
    "pk_accounts": "CREATE TABLE capture.pk_accounts (account_id bigint, balance numeric(38,9), nickname text, opened timestamp)",
    "agg_metrics": "CREATE TABLE capture.agg_metrics (metric_day date, metric_name text, qty bigint, revenue numeric(18,4))",
    "uniq_dim": "CREATE TABLE capture.uniq_dim (dim_id bigint, label text, score numeric(10,2))",
    "dup_partition_sums": "CREATE TABLE capture.dup_partition_sums (part text, row_count bigint, amount_sum numeric(38,4))",
}

SECRETS: list[str] = []
CHECKS: list[dict] = []
PHASES: list[dict] = []
STARTED = {"starrocks": False, "postgres": False}
ORIGIN = time.monotonic()


def now_iso() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def now_ms() -> int:
    return int((time.monotonic() - ORIGIN) * 1000)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def scrub(data: bytes | str) -> str:
    text = data.decode("utf-8", "replace") if isinstance(data, bytes) else data
    text = closure_gate.scrub_text(text, SECRETS)
    folded = []
    for line in text.splitlines():
        upper = line.upper()
        if "IDENTIFIED BY" in upper or "PASSWORD" in upper:
            folded.append("[redacted-credential-line]")
        else:
            folded.append(line)
    return "\n".join(folded) + ("\n" if text.endswith("\n") else "")


def save(path: Path, data: bytes | str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(scrub(data), encoding="utf-8")


def write_json(path: Path, body: object) -> None:
    save(path, json.dumps(body, ensure_ascii=False, indent=2, default=str) + "\n")


def check(name: str, ok: bool, expected: object, actual: object) -> bool:
    CHECKS.append({"name": name, "ok": bool(ok), "expected": expected, "actual": actual, "at": now_iso()})
    print(f"{'ok' if ok else 'FAIL'} {name}", flush=True)
    return bool(ok)


def run(cmd: list[str], timeout: int = 180, stdin: bytes | None = None) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(cmd, input=stdin, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout, check=False)


def phase(name: str, proc: subprocess.CompletedProcess[bytes]) -> int:
    save(EVIDENCE / "phases" / f"{name}.stdout", proc.stdout)
    save(EVIDENCE / "phases" / f"{name}.stderr", proc.stderr)
    (EVIDENCE / "phases" / f"{name}.exit").write_text(f"{proc.returncode}\n", encoding="utf-8")
    PHASES.append({"name": name, "exit": proc.returncode, "at": now_iso()})
    return proc.returncode


def mysql(sql: str, cnf: str | None = None, timeout: int = 180, headers: bool = False) -> subprocess.CompletedProcess[bytes]:
    cmd = ["docker", "exec", "-i", CONTAINER, "mysql"]
    if cnf:
        cmd.append(f"--defaults-file={cnf}")
    else:
        cmd.extend(["--no-defaults", "-uroot", "-h127.0.0.1", "-P9030", "--protocol=TCP", "--skip-password"])
    cmd.append("--batch")
    cmd.append("--raw")
    if not headers:
        cmd.append("--skip-column-names")
    cmd.extend(["--connect-timeout=10", "--default-character-set=utf8mb4"])
    return run(cmd, timeout=timeout, stdin=sql.encode("utf-8"))


def parse_table(proc: subprocess.CompletedProcess[bytes]) -> tuple[list[str], list[list[str]]]:
    lines = [line for line in proc.stdout.decode("utf-8", "replace").splitlines() if line != ""]
    if not lines:
        return [], []
    header = lines[0].split("\t")
    rows = [line.split("\t") for line in lines[1:]]
    return header, rows


def sql_statements(text: str) -> list[str]:
    statements = []
    for chunk in text.split(";"):
        lines = [line for line in chunk.splitlines() if line.strip() and not line.strip().startswith("--")]
        body = "\n".join(lines).strip()
        if body:
            statements.append(body)
    return statements


def wait_until(due_ms: int) -> None:
    delay = (due_ms - now_ms()) / 1000
    if delay > 0:
        time.sleep(delay)


def load_oracle():
    import importlib.util

    spec = importlib.util.spec_from_file_location("sr_oracle", SRC / "scripts" / "oracle.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def admit_start(kind: str, command: list[str]) -> bool:
    out = EVIDENCE / f"admission-{kind}.json"
    proc = run([sys.executable, str(ADMIT), "acquire-start", kind, str(out), "--", *command], timeout=180)
    phase(f"admit-{kind}", proc)
    if proc.returncode != 0:
        snap = run([sys.executable, str(ADMIT), "snapshot", kind, str(EVIDENCE / f"admission-{kind}-retry.json")])
        phase(f"admit-{kind}-snapshot", snap)
        print(f"admission {kind} refused; waiting 900s", flush=True)
        time.sleep(900)
        proc = run([sys.executable, str(ADMIT), "acquire-start", kind, str(out), "--", *command], timeout=180)
        phase(f"admit-{kind}-retry", proc)
    return proc.returncode == 0


def write_secret(name: str, text: str, mode: int) -> Path:
    SECRET_DIR.mkdir(mode=0o700, exist_ok=True)
    os.chmod(SECRET_DIR, 0o700)
    path = SECRET_DIR / name
    path.write_text(text, encoding="utf-8")
    os.chmod(path, mode)
    return path


def install(local: Path, remote: str, mode: str) -> None:
    copied = run(["docker", "cp", str(local), f"{CONTAINER}:{remote}"])
    if copied.returncode != 0:
        raise RuntimeError(scrub(copied.stderr)[:400])
    run(["docker", "exec", CONTAINER, "chmod", mode, remote])


def try_grant(statements: list[str]) -> bool:
    for statement in statements:
        if mysql(statement).returncode == 0:
            return True
    return False


def start_starrocks() -> bool:
    existing = run(["docker", "ps", "-aq", "--filter", f"name=^{CONTAINER}$"])
    if existing.stdout.strip():
        check("stale_starrocks_container", False, "absent", existing.stdout.decode().strip())
        return False
    command = [
        "docker", "run", "-d", "--name", CONTAINER,
        "--platform", "linux/arm64", "--network", NETWORK,
        "--memory", "4g", "--memory-swap", "4g", "--cpus", "1",
        "--restart", "no", "--pull", "never",
        "--ulimit", "nofile=65535:65535",
        "--label", "project=open-ontology",
        "--label", f"owner={SESSION}",
        "--label", f"session={SESSION}",
        "--label", "kind=starrocks-source",
        "--label", f"oo.prefix={PREFIX}",
        "--label", f"oo.task={TASK}",
        "--label", "oo.capture=schedule",
        IMAGE,
    ]
    if not admit_start("starrocks", command):
        return False
    STARTED["starrocks"] = True
    deadline = time.time() + 180
    last = b""
    while time.time() < deadline:
        state = run(["docker", "inspect", "-f", "{{.State.Status}}", CONTAINER])
        if state.stdout.decode().strip() not in {"running", "created"}:
            phase("ready-state", state)
            return False
        probe = mysql("SELECT 1;")
        last = probe.stdout + probe.stderr
        if probe.returncode == 0 and b"true" in mysql("SHOW BACKENDS;").stdout.lower():
            save(EVIDENCE / "phases" / "ready.stdout", last)
            return True
        time.sleep(3)
    save(EVIDENCE / "phases" / "ready.stderr", last)
    return False


def identity() -> bool:
    proc = mysql("SELECT current_version();\nSELECT @@version_comment;\nSELECT @@version;\n")
    phase("identity", proc)
    lines = [line for line in proc.stdout.decode().splitlines() if line]
    version_ok = lines[:3] == ["3.5.21-85fccf8", "3.5.21-85fccf8", "8.0.33"]
    image_id = run(["docker", "inspect", "-f", "{{.Image}}", CONTAINER])
    ident = image_id.stdout.decode().strip()
    repo = run(["docker", "image", "inspect", "-f", "{{json .RepoDigests}}", ident or IMAGE])
    phase("image-digest", subprocess.CompletedProcess(repo.args, repo.returncode, image_id.stdout + repo.stdout, repo.stderr))
    digest_ok = ident == PIN and PIN in repo.stdout.decode()
    check("engine_identity", version_ok, ["3.5.21-85fccf8", "3.5.21-85fccf8", "8.0.33"], lines[:3])
    check("image_digest", digest_ok, PIN, ident)
    check("wire_is_not_engine", version_ok and lines[2] == "8.0.33", "8.0.33 wire beside 3.5.21 engine", lines[:3])
    return version_ok and digest_ok


def create_accounts() -> dict[str, str] | None:
    dba_password = os.urandom(18).hex()
    reader_password = os.urandom(18).hex()
    SECRETS.extend([dba_password, reader_password])
    mysql("CREATE DATABASE IF NOT EXISTS oo_w45_sr;")
    created = mysql(
        "CREATE USER 'oo_w45_dba'@'%' IDENTIFIED BY '{dba}';\n"
        "CREATE USER 'oo_w45_reader'@'%' IDENTIFIED BY '{reader}';\n".format(dba=dba_password, reader=reader_password)
    )
    phase("create-users", created)
    reader_ok = try_grant([
        "GRANT SELECT ON ALL TABLES IN DATABASE oo_w45_sr TO USER 'oo_w45_reader'@'%';",
        "GRANT SELECT ON oo_w45_sr.* TO 'oo_w45_reader'@'%';",
    ])
    dba_ok = try_grant([
        "GRANT SELECT, INSERT, UPDATE, DELETE, ALTER, DROP ON ALL TABLES IN DATABASE oo_w45_sr TO USER 'oo_w45_dba'@'%';",
        "GRANT SELECT, INSERT, UPDATE, DELETE, ALTER, DROP ON oo_w45_sr.* TO 'oo_w45_dba'@'%';",
    ]) and try_grant([
        "GRANT CREATE TABLE ON DATABASE oo_w45_sr TO USER 'oo_w45_dba'@'%';",
        "GRANT CREATE TABLE ON oo_w45_sr TO 'oo_w45_dba'@'%';",
    ])
    grants = mysql("SHOW GRANTS FOR 'oo_w45_reader'@'%';\nSHOW GRANTS FOR 'oo_w45_dba'@'%';\n")
    phase("show-grants", grants)
    text = grants.stdout.decode("utf-8", "replace").upper()
    reader_lines = [line for line in text.splitlines() if "OO_W45_READER" in line]
    reader_write = any(word in line for line in reader_lines for word in ("INSERT", "UPDATE", "DELETE", "CREATE", "DROP", "ALTER", "OPERATE", "ADMIN"))
    if not check("reader_select_only", reader_ok and not reader_write, "SELECT only", reader_lines):
        return None
    if not check("dba_grant", dba_ok, True, dba_ok):
        return None
    write_secret("reader.password", reader_password + "\n", 0o600)
    dba_cnf = write_secret(
        "dba.cnf",
        "[client]\nuser=oo_w45_dba\npassword={password}\nhost=127.0.0.1\nport=9030\nprotocol=TCP\n".format(password=dba_password),
        0o600,
    )
    reader_cnf = write_secret(
        "reader.cnf",
        "[client]\nuser=oo_w45_reader\npassword={password}\nhost=127.0.0.1\nport=9030\nprotocol=TCP\n".format(password=reader_password),
        0o600,
    )
    run(["docker", "exec", CONTAINER, "mkdir", "-p", "/tmp/oo-secrets"])
    install(SECRET_DIR / "reader.password", "/tmp/oo-secrets/reader.password", "600")
    install(dba_cnf, "/tmp/oo-secrets/dba.cnf", "600")
    install(reader_cnf, "/tmp/oo-secrets/reader.cnf", "600")
    return {"dba": "/tmp/oo-secrets/dba.cnf", "reader": "/tmp/oo-secrets/reader.cnf"}


def load_fixture(dba_cnf: str) -> bool:
    ok = True
    for index, statement in enumerate(sql_statements((SRC / "sql" / "dba-load.sql").read_text(encoding="utf-8"))):
        bound = statement if statement.upper().startswith("USE ") else f"USE oo_w45_sr;\n{statement}"
        proc = mysql(bound + ";", dba_cnf, timeout=180)
        phase(f"load-{index:02d}", proc)
        if proc.returncode != 0 and "sketch_hold" in statement:
            check("sketch_ddl", False, "recorded engine rejection", proc.stderr.decode()[:240])
            continue
        if proc.returncode != 0:
            ok = False
            break
    present = mysql("SHOW TABLES FROM oo_w45_sr;", dba_cnf)
    names = present.stdout.decode()
    return check("fixture_tables", ok and all(table in names for table in BASE_TABLES), list(BASE_TABLES), names.strip())


def dba_exec(dba_cnf: str, label: str, statement: str) -> bool:
    proc = mysql(f"USE oo_w45_sr;\n{statement}", dba_cnf, timeout=180)
    phase(label, proc)
    return proc.returncode == 0


def show_partitions(table: str, cnf: str) -> list[dict] | None:
    proc = mysql(f"SHOW PARTITIONS FROM oo_w45_sr.{table};", cnf, headers=True)
    phase(f"partitions-{table}", proc)
    if proc.returncode != 0:
        return None
    header, rows = parse_table(proc)
    try:
        return cap.parse_partition_table(header, rows)
    except cap.ScheduleError as exc:
        write_json(EVIDENCE / "phases" / f"partitions-{table}.error.json", {"reason": exc.reason, "header": header})
        return None


def show_columns(table: str, cnf: str) -> list[dict]:
    proc = mysql(f"DESC oo_w45_sr.{table};", cnf, headers=True)
    phase(f"desc-{table}", proc)
    header, rows = parse_table(proc)
    if proc.returncode != 0 or not header:
        return []
    index = {name.lower(): pos for pos, name in enumerate(header)}
    field_at = index.get("field", 0)
    type_at = index.get("type", 1)
    columns = []
    for row in rows:
        columns.append({"name": row[field_at], "column_type": row[type_at] if type_at < len(row) else ""})
    return columns


def native_dump(cnf: str, dest: Path) -> bool:
    dest.mkdir(parents=True, exist_ok=True)
    ok = True
    for name, sql in QUERIES.items():
        proc = mysql("USE oo_w45_sr;\n" + sql + ";", cnf)
        phase(f"native-{dest.name}-{name}", proc)
        (dest / f"{name}.tsv").write_text(proc.stdout.decode("utf-8", "replace"), encoding="utf-8")
        ok = ok and proc.returncode == 0
    return ok


def probe_read(label: str, deadline_ms: int = 60000) -> int:
    remote = f"/tmp/oo-w45-capture/{label}"
    run(["docker", "exec", CONTAINER, "rm", "-rf", remote])
    run(["docker", "exec", CONTAINER, "mkdir", "-p", remote])
    proc = run(
        [
            "docker", "exec", CONTAINER, "/tmp/starrocks-probe", "read",
            "--host", "127.0.0.1", "--port", "9030",
            "--password-file", "/tmp/oo-secrets/reader.password",
            "--image-digest", PIN,
            "--deadline-ms", str(deadline_ms),
            "--max-rows", "1000",
            "--out", remote,
        ],
        timeout=120,
    )
    code = phase(f"probe-{label}", proc)
    host = EVIDENCE / "reads" / label
    if host.exists():
        shutil.rmtree(host)
    host.mkdir(parents=True, exist_ok=True)
    copied = run(["docker", "cp", f"{CONTAINER}:{remote}/.", str(host)], timeout=60)
    phase(f"probe-copy-{label}", copied)
    return code


def decode_label(label: str) -> int:
    remote_arrow = f"/tmp/oo-w45-arrow-{label}"
    remote_out = f"/tmp/oo-w45-decoded-{label}"
    run(["docker", "exec", CONTAINER, "rm", "-rf", remote_arrow, remote_out])
    run(["docker", "exec", CONTAINER, "mkdir", "-p", remote_arrow, remote_out])
    arrow_dir = EVIDENCE / "reads" / label
    for path in sorted(arrow_dir.glob("*.arrow")):
        install(path, f"{remote_arrow}/{path.name}", "644")
    listing = run(["docker", "exec", CONTAINER, "sh", "-c", f"ls {remote_arrow}/*.arrow"])
    proc = run(["docker", "exec", CONTAINER, "/tmp/starrocks-decode", "--out", remote_out, *listing.stdout.decode().split()], timeout=120)
    code = phase(f"decode-{label}", proc)
    decoded = EVIDENCE / "decoded" / label
    decoded.mkdir(parents=True, exist_ok=True)
    run(["docker", "cp", f"{CONTAINER}:{remote_out}/.", str(decoded)])
    return code


def load_rows(oracle, label: str) -> dict[str, list[dict]]:
    loaded = {}
    for table in TABLES:
        _document, rows = oracle.load_decoded(EVIDENCE / "decoded" / label / f"{table}.json", table)
        loaded[table] = rows
    return loaded


def arrow_hashes(label: str) -> dict[str, str]:
    hashes = {}
    for path in sorted((EVIDENCE / "reads" / label).glob("*.arrow")):
        hashes[path.name] = sha256_file(path)
    write_json(EVIDENCE / "reads" / label / "arrow.sha256.json", hashes)
    return hashes


def key_of(table: str):
    if table == "pk_accounts":
        return lambda row: row["account_id"]
    if table == "uniq_dim":
        return lambda row: row["dim_id"]
    if table == "agg_metrics":
        return lambda row: (row["metric_day"], row["metric_name"])
    if table == "dup_partition_sums":
        return lambda row: row["part"]
    return lambda row: oracle_canon_row(row)


def oracle_canon_row(row: dict) -> str:
    return json.dumps(row, ensure_ascii=False, sort_keys=True, default=str)


def dup_part(row: dict) -> str:
    return "p202605" if str(row["bucket"]) < "2026-06-01" else "p202606"


def cells_equal(oracle, table: str, decoded_rows: list[dict], native_text: str) -> dict:
    native_rows = oracle.parse_tsv(table, native_text)
    left = oracle.canon(decoded_rows)
    right = oracle.canon(native_rows)
    mismatches = []
    if left != right:
        for index, (raw_left, raw_right) in enumerate(zip(left, right)):
            if raw_left != raw_right:
                mismatches.append({"index": index, "decoded": json.loads(raw_left), "native": json.loads(raw_right)})
        if len(left) != len(right):
            mismatches.append({"kind": "length", "decoded": len(left), "native": len(right)})
    return {
        "table": table,
        "equal": left == right,
        "decoded_rows": len(decoded_rows),
        "native_rows": len(native_rows),
        "cell_mismatches": mismatches,
        "decoded": [json.loads(item) for item in left],
        "native": [json.loads(item) for item in right],
        "sampled": False,
    }


def capture_read(oracle, label: str, reader_cnf: str) -> dict[str, list[dict]] | None:
    code = probe_read(label)
    if code != 0:
        check(f"probe_{label}", False, 0, code)
        return None
    if decode_label(label) != 0:
        check(f"decode_{label}", False, 0, "decode")
        return None
    native_dump(reader_cnf, EVIDENCE / "native" / label)
    rows = load_rows(oracle, label)
    arrow_hashes(label)
    compared = {}
    equal = True
    for table in TABLES:
        item = cells_equal(oracle, table, rows[table], (EVIDENCE / "native" / label / f"{table}.tsv").read_text(encoding="utf-8"))
        compared[table] = item
        equal = equal and item["equal"]
    write_json(EVIDENCE / "compare" / f"{label}.json", compared)
    check(f"cells_{label}", equal, "full cell match", {name: item["equal"] for name, item in compared.items()})
    return rows


def plan_live(table: str, scheduler: cap.Scheduler, current, after, dml_count: int, dml_at_watermark: dict[str, int]) -> dict:
    if table == "dup_partition_sums":
        parent = plan_live("dup_events", scheduler, current, after, dml_count, dml_at_watermark)
        return cap.plan_derived(parent)
    mutation = dml_count > dml_at_watermark.get(table, 0)
    return cap.plan_table(
        table,
        scheduler.watermarks.get(table),
        current,
        after,
        scheduler.trusted.get(table, False),
        mutation,
    )


def apply_plan(table: str, plan: dict, old_rows: list[dict], fresh_rows: list[dict], parts: list[dict] | None) -> tuple[list[dict], dict]:
    action = plan["action"]
    updated = dict(plan)
    updated["partitions"] = parts
    updated["token"] = cap.token_of(parts) if parts is not None else hashlib.sha256(json.dumps(cap.canonical(fresh_rows)).encode()).hexdigest()
    if action in {"skip_read", "abort"}:
        updated["diff"] = {"inserts": 0, "deletes": 0, "updates": 0}
        return old_rows, updated
    if action == "partition_replace":
        part_of = dup_part if table == "dup_events" else (lambda _row, name=parts[0]["name"]: name)
        replaced, how = cap.apply_c4(old_rows, fresh_rows, plan.get("changed_partitions") or [], part_of)
        updated["replace"] = how
        if how != "partition_replace":
            updated["method"] = "C5"
            updated["reason"] = how
            updated["trusted_after"] = False
            updated["quality"] = cap.quality_for(plan["model"], "C5", True, False)
            replaced = list(fresh_rows)
        rows = replaced
    else:
        rows = list(fresh_rows)
    updated["diff"] = cap.multiset_diff(old_rows, rows, key_of(table))
    updated["diff"].pop("deleted_rows", None)
    deleted = cap.multiset_diff(old_rows, rows, key_of(table))["deleted_rows"]
    updated["deleted_keys"] = deleted
    return rows, updated


def run_tick(oracle, scheduler: cap.Scheduler, label: str, reader_cnf: str, shadow: dict, dml_count: int, dml_mark: dict[str, int], frozen: dict[str, dict[str, str]]) -> bool:
    opened = scheduler.begin_tick(now_ms())
    write_json(EVIDENCE / "ticks" / f"{label}-begin.json", opened)
    if not opened["admitted"]:
        check(f"tick_{label}_admitted", False, True, opened)
        return False
    guards = {}
    paused = False
    for table in BASE_TABLES:
        live = show_columns(table, reader_cnf)
        guards[table] = cap.schema_guard(APPROVED_COLUMNS[table], live, frozen.get(table))
        paused = paused or not guards[table]["read"]
    write_json(EVIDENCE / "ticks" / f"{label}-schema.json", guards)
    if paused:
        record = scheduler.abort_tick(now_ms(), "schema_guard")
        write_json(EVIDENCE / "ticks" / f"{label}-abort.json", record)
        check(f"schema_pause_{label}", True, "read false", guards)
        return False
    current = {table: show_partitions(table, reader_cnf) for table in BASE_TABLES}
    preview = {table: plan_live(table, scheduler, current.get(table) if table != "dup_partition_sums" else current.get("dup_events"), current.get(table) if table != "dup_partition_sums" else current.get("dup_events"), dml_count, dml_mark) for table in TABLES}
    write_json(EVIDENCE / "ticks" / f"{label}-preview.json", preview)
    needs_read = any(item["action"] not in {"skip_read", "abort"} for item in preview.values())
    fresh = None
    after = current
    if needs_read:
        fresh = capture_read(oracle, label, reader_cnf)
        if fresh is None:
            scheduler.abort_tick(now_ms(), "read_failed")
            return False
        after = {table: show_partitions(table, reader_cnf) for table in BASE_TABLES}
    else:
        check(f"skip_read_{label}", True, "probe not invoked", "skipped")
    plans = {}
    for table in TABLES:
        part_current = current.get(table) if table in BASE_TABLES else current.get("dup_events")
        part_after = after.get(table) if table in BASE_TABLES else after.get("dup_events")
        if table == "dup_partition_sums":
            plans[table] = cap.plan_derived(plans["dup_events"])
            continue
        plans[table] = plan_live(table, scheduler, part_current, part_after, dml_count, dml_mark)
    if any(item["action"] == "abort" for item in plans.values()):
        record = scheduler.abort_tick(now_ms(), "stale_read")
        write_json(EVIDENCE / "ticks" / f"{label}-stale.json", {"plans": plans, "record": record})
        check(f"stale_{label}", True, "watermark held", record["advanced"])
        return False
    commit_tables = {}
    for table, plan in plans.items():
        old_rows = shadow.get(table, [])
        fresh_rows = fresh[table] if fresh is not None else old_rows
        part_current = current.get(table) if table in BASE_TABLES else current.get("dup_events")
        rows, updated = apply_plan(table, plan, old_rows, fresh_rows, part_current)
        if fresh is not None and cap.canonical(rows) != cap.canonical(fresh_rows) and plan["action"] != "skip_read":
            check(f"shadow_{label}_{table}", False, "fresh image", "shadow diverged")
            scheduler.abort_tick(now_ms(), "shadow_diverged")
            return False
        shadow[table] = rows
        commit_tables[table] = updated
        if plan["action"] != "skip_read":
            dml_mark[table] = dml_count
    record = scheduler.commit_tick(now_ms(), commit_tables)
    write_json(EVIDENCE / "ticks" / f"{label}.json", record)
    return record["outcome"] == "committed"


def pg_start() -> bool:
    busy = socket.socket()
    try:
        busy.bind(("127.0.0.1", 15445))
    except OSError as exc:
        check("pg_port", False, "15445 free", str(exc))
        return False
    finally:
        busy.close()
    command = [
        "docker", "run", "-d", "--name", PG_CONTAINER,
        "--memory", "1g", "--memory-swap", "1g", "--cpus", "1",
        "--restart", "no", "--pull", "never",
        "-p", "127.0.0.1:15445:5432",
        "-e", "POSTGRES_HOST_AUTH_METHOD=trust",
        "-e", "POSTGRES_USER=postgres",
        "--label", "project=open-ontology",
        "--label", f"owner={SESSION}",
        "--label", "kind=capture-cell-mirror",
        "--label", f"oo.prefix={PREFIX}",
        "--label", f"oo.task={TASK}",
        PG_IMAGE,
    ]
    if not admit_start("postgres", command):
        return False
    STARTED["postgres"] = True
    deadline = time.time() + 60
    while time.time() < deadline:
        proc = run(["docker", "exec", PG_CONTAINER, "pg_isready", "-U", "postgres"])
        if proc.returncode == 0:
            phase("pg-ready", proc)
            created = run(["docker", "exec", PG_CONTAINER, "psql", "-U", "postgres", "-c", "CREATE DATABASE oo_w45;"])
            phase("pg-createdb", created)
            ddl = "CREATE SCHEMA IF NOT EXISTS capture;\n" + "\n".join(statement + ";" for statement in PG_DDL.values())
            made = run(["docker", "exec", "-i", PG_CONTAINER, "psql", "-U", "postgres", "-d", "oo_w45"], stdin=ddl.encode())
            phase("pg-ddl", made)
            return created.returncode == 0 and made.returncode == 0
        time.sleep(1)
    return False


def pg_replace(label: str, shadow: dict[str, list[dict]]) -> bool:
    statements = ["BEGIN;"]
    copy_rows = []
    for table in TABLES:
        statements.append(f"DELETE FROM capture.{table};")
        columns = ",".join(APPROVED_COLUMNS.get(table, ["part", "row_count", "amount_sum"]))
        statements.append(f"COPY capture.{table} ({columns}) FROM STDIN;")
        lines = []
        for row in shadow[table]:
            ordered = APPROVED_COLUMNS.get(table, ["part", "row_count", "amount_sum"])
            cells = []
            for name in ordered:
                value = row.get(name)
                cells.append(r"\N" if value is None else str(value))
            lines.append("\t".join(cells))
        # A trailing blank line is an extra COPY row. The join supplies the newline.
        copy_rows.append("\n".join(lines))
        statements.append("\\.")
    statements.append("COMMIT;")
    payload = []
    copy_at = 0
    for statement in statements:
        payload.append(statement)
        if statement.startswith("COPY "):
            payload.append(copy_rows[copy_at])
            copy_at += 1
    script = "\n".join(payload) + "\n"
    proc = run(["docker", "exec", "-i", PG_CONTAINER, "psql", "-v", "ON_ERROR_STOP=1", "-U", "postgres", "-d", "oo_w45"], stdin=script.encode(), timeout=60)
    phase(f"pg-load-{label}", proc)
    if proc.returncode != 0:
        return False
    ok = True
    dumped = {}
    for table in TABLES:
        columns = ",".join(APPROVED_COLUMNS.get(table, ["part", "row_count", "amount_sum"]))
        selected = run(["docker", "exec", PG_CONTAINER, "psql", "-U", "postgres", "-d", "oo_w45", "-At", "-c", f"COPY (SELECT {columns} FROM capture.{table}) TO STDOUT WITH (FORMAT text, NULL 'NULL');"])
        phase(f"pg-select-{label}-{table}", selected)
        text = selected.stdout.decode("utf-8", "replace")
        (EVIDENCE / "pg" / label).mkdir(parents=True, exist_ok=True)
        (EVIDENCE / "pg" / label / f"{table}.tsv").write_text(text, encoding="utf-8")
        dumped[table] = text
        ok = ok and selected.returncode == 0
    write_json(EVIDENCE / "pg" / f"{label}-bytes.json", {"tables": list(TABLES)})
    return ok


def pg_cells(oracle, label: str, shadow: dict[str, list[dict]]) -> bool:
    equal = True
    body = {}
    for table in TABLES:
        text = (EVIDENCE / "pg" / label / f"{table}.tsv").read_text(encoding="utf-8")
        native_rows = oracle.parse_tsv(table, text)
        same = oracle.canon(native_rows) == oracle.canon(shadow[table])
        body[table] = {"equal": same, "pg": [json.loads(item) for item in oracle.canon(native_rows)], "shadow": [json.loads(item) for item in oracle.canon(shadow[table])], "sampled": False}
        equal = equal and same
    write_json(EVIDENCE / "pg" / f"{label}-cells.json", body)
    return check(f"pg_cells_{label}", equal, "pg matches shadow", {name: item["equal"] for name, item in body.items()})


def has_id(rows: list[dict], field: str, value: int) -> bool:
    return any(row.get(field) == value for row in rows)


def fe_audit() -> str:
    remote = ""
    for candidate in ("/data/deploy/starrocks/fe/log/fe.audit.log", "/opt/starrocks/fe/log/fe.audit.log"):
        probe = run(["docker", "exec", CONTAINER, "sh", "-c", f"test -f {candidate} && echo yes"])
        if probe.stdout.decode().strip() == "yes":
            remote = candidate
            break
    if not remote:
        return ""
    proc = run(["docker", "exec", CONTAINER, "sh", "-c", f"grep -E 'oo_w45_reader|oo_w45_dba' {remote} || true"], timeout=60)
    save(EVIDENCE / "fe-audit.log", proc.stdout)
    return proc.stdout.decode("utf-8", "replace")


def collect_audits(oracle) -> dict:
    total = {"rows": 0, "handshake": 0, "read": 0, "session": 0, "cancel": 0, "cleanup": 0, "write": 0, "unexpected": 0, "files": []}
    for path in sorted((EVIDENCE / "reads").glob("*/audit.jsonl")):
        item = oracle.audit_counts(path)
        total["files"].append({"path": str(path.relative_to(EVIDENCE)), **{key: item.get(key) for key in ("write", "unexpected", "handshake", "read", "cleanup")}})
        for key in ("rows", "handshake", "read", "session", "cancel", "cleanup", "write", "unexpected"):
            total[key] += int(item.get(key) or 0)
    return total


def secret_scan() -> bool:
    hits = []
    for path in EVIDENCE.rglob("*"):
        if not path.is_file():
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        for secret in SECRETS:
            if secret and secret in text:
                hits.append(str(path.relative_to(EVIDENCE)))
                break
    return check("secret_scan", not hits, [], hits)


def write_sums() -> str:
    rows = []
    for path in sorted(EVIDENCE.rglob("*")):
        if not path.is_file():
            continue
        relative = path.relative_to(EVIDENCE).as_posix()
        if relative in {"SHA256SUMS", "receipt.json"}:
            continue
        rows.append(f"{sha256_file(path)}  {relative}")
    text = "\n".join(rows) + ("\n" if rows else "")
    (EVIDENCE / "SHA256SUMS").write_text(text, encoding="utf-8")
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def exclude_arrows() -> None:
    exclude = REPO / ".git" / "info" / "exclude"
    lines = exclude.read_text(encoding="utf-8").splitlines()
    extra = [f"!{path.relative_to(REPO).as_posix()}" for path in sorted(EVIDENCE.glob("reads/*/*.arrow"))]
    added = [line for line in extra if line not in lines]
    if added:
        exclude.write_text(exclude.read_text(encoding="utf-8").rstrip() + "\n" + "\n".join(added) + "\n", encoding="utf-8")


def cleanup() -> None:
    body = {"at": now_iso(), "removed_containers": [], "volumes_deleted": False, "network_deleted": False}
    for kind, name in (("starrocks", CONTAINER), ("postgres", PG_CONTAINER)):
        if not STARTED[kind]:
            continue
        proc = run(["docker", "rm", "-f", name], timeout=60)
        phase(f"cleanup-{kind}", proc)
        run([sys.executable, str(ADMIT), "release", kind])
        body["removed_containers"].append(name)
    if SECRET_DIR.exists():
        for path in SECRET_DIR.iterdir():
            path.unlink(missing_ok=True)
        SECRET_DIR.rmdir()
    volumes = run(["docker", "volume", "ls", "--format", "{{.Name}}"])
    body["volumes_present"] = [line for line in volumes.stdout.decode().splitlines() if line.startswith(PREFIX)]
    write_json(EVIDENCE / "cleanup.json", body)


def write_reports(summary: dict) -> None:
    failed = [item["name"] for item in CHECKS if not item["ok"]]
    report = [
        "# DL01-W1-45 StarRocks capture schedule",
        "",
        f"Window {summary.get('started_at')} to {summary.get('finished_at')}. Engine 3.5.21-85fccf8. Image `{PIN}`.",
        "This run is the scheduled capture cycle. The sealed preview at e8a3e4c6085ff8871aaf7af845b26ccd913ae530 stays the preview record.",
        "Author outcome is author_same_run. source_acceptance is not_claimed. parent_complete is false. whole_dl_complete is false. fixed_independent_review is not_run. Complete sign-off stays 0/10.",
        "",
        f"Failed checks: {failed if failed else 'none'}.",
        "",
        "Capture methods are the per-table decisions in ticks/*.json. Duplicate grade is S0 only when the partition version stayed still across the read. Primary and unique stay grade B. C1, C2, and unconfigured C3 were refused before read.",
        "",
        "Independent PG on 127.0.0.1:15445 is a typed cell mirror of the captured rows. It is not a sealed catalog, not a role pack, and not a Gateway.",
        "",
        "Model calls for this generation were not made. `/tmp/oo-customer-demo-20261007/llm-token` is a directory and contains no token file.",
    ]
    (EVIDENCE / "REPORT.md").write_text("\n".join(report) + "\n", encoding="utf-8")
    findings = [
        "# Findings",
        "",
        "Probe `catalog.json` still records capture_method C4 and consistency_grade S0 for every object. Those fields are not this cycle's decision. The decision files are ticks/*.json.",
        "",
        "StarRocks 3.5.21 has no change stream. sync cdc, change_feed, auto, and incremental without cursor_safety are refused before a read.",
        "",
        "S0–S10 Self-build, strict OOS YAML, G gates, role Knowledge Pack, and an external Agent multi-turn are not produced here. Shared owners are 05, 06, and 21. The model token file is absent, so a fresh pack would be cost_unknown or noReady rather than a sealed pack.",
        "",
        "Production DBA approval and the source EULA stay pending. The DBA statements in this run are the local fixture role oo_w45_dba.",
        "",
        "Root product SHA256SUMS from the preview seal was not rewritten. This cycle's sums are raw/capture-schedule/SHA256SUMS.",
        "",
        f"Failed checks: {failed if failed else 'none'}.",
    ]
    (EVIDENCE / "FINDINGS.md").write_text("\n".join(findings) + "\n", encoding="utf-8")
    handoff = [
        "# Handoff",
        "",
        "Task DL01-W1-45. Exclusive paths only. Preview commit e8a3e4c6085ff8871aaf7af845b26ccd913ae530 is kept.",
        "",
        "Reusable inputs: probe ELF 21bbdc81d436bbffb2951b2245068c1667dccd02ed32a6667b3770b4b7bd1aee, decode ELF 016be98241f4e4214bbf4d9e5826e80edaf881ceab4dcb2d62eb42a64ec50b9f, registry 6833baeb559ef004fcf510cdc260b8a9818ee223389f3d3fe560b2c92011eca2. Rust source was not edited, so the existing Linux 1.94 build/test/clippy exits under starrocks-driver-preflight/build stay the compiler record.",
        "",
        "Blocked shared request: feed raw/capture-schedule/reads/*/ decoded rows and ticks/*.json to the fixed Self-build control 13ad15b4 and the source-bound import 9d922. Do not treat this cell mirror as that import. Gateway fixed point ac896 still needs a pack built for this generation and a real token file before an external multi-turn.",
        "",
        "Cleanup removes only this run's starrocks and postgres containers. Volumes oo-dl01w1-45-cargo and oo-dl01w1-45-target and network oo-dl01w1-45 stay.",
        "",
        "source_acceptance not_claimed. parent_complete false. whole_dl_complete false. review not_run.",
    ]
    (EVIDENCE / "HANDOFF.md").write_text("\n".join(handoff) + "\n", encoding="utf-8")


def token_record() -> dict:
    kind = "missing"
    if TOKEN_PATH.is_dir():
        names = [item.name for item in TOKEN_PATH.iterdir()]
        kind = "empty_directory" if not names else "directory_with_entries"
    elif TOKEN_PATH.is_file():
        kind = "file"
    body = {"path": str(TOKEN_PATH), "kind": kind, "model_called": False, "pack_produced": False, "reason": "no_token_file"}
    write_json(EVIDENCE / "model-blocker.json", body)
    return body


def pre_refusals() -> None:
    approved = set(TABLES)
    samples = {
        "missing-secret": {"secret_ref": "secret://oo-dl01w1-45/reader", "sync": "partition_refresh", "objects": ["pk_accounts"]},
        "unknown-secret": {"secret_ref": "secret://oo-dl01w1-45/not-issued", "sync": "full", "objects": ["pk_accounts"]},
        "cdc": {"secret_ref": "secret://oo-dl01w1-45/reader", "sync": "cdc", "objects": ["pk_accounts"]},
        "c3": {"secret_ref": "secret://oo-dl01w1-45/reader", "sync": "incremental", "objects": ["pk_accounts"]},
        "transactional": {"secret_ref": "secret://oo-dl01w1-45/reader", "sync": "partition_refresh", "consistency": "transactional", "objects": ["pk_accounts"]},
        "license": {"secret_ref": "secret://oo-dl01w1-45/reader", "sync": "full", "license_required": True, "objects": ["pk_accounts"]},
        "unknown-object": {"secret_ref": "secret://oo-dl01w1-45/reader", "sync": "full", "objects": ["sketch_hold"], "fields_unknown": ["password"]},
    }
    body = {}
    for name, request in samples.items():
        exists = name != "missing-secret"
        body[name] = cap.refuse_request(request, exists, approved)
        check(f"refuse_{name}", bool(body[name]) and all(item["read_attempted"] is False for item in body[name]), "refused before read", body[name][0]["reason"] if body[name] else None)
    write_json(EVIDENCE / "refusals.json", body)


def main() -> int:
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    summary = {"started_at": now_iso(), "protocol": cap.PROTOCOL, "preview_commit": "e8a3e4c6085ff8871aaf7af845b26ccd913ae530"}
    oracle = load_oracle()
    registry_sha = sha256_file(REGISTRY)
    check("registry_sha", registry_sha == REGISTRY_SHA, REGISTRY_SHA, registry_sha)
    check("probe_sha", sha256_file(PROBE) == PROBE_SHA, PROBE_SHA, sha256_file(PROBE))
    check("decode_sha", sha256_file(DECODE) == DECODE_SHA, DECODE_SHA, sha256_file(DECODE))
    token = token_record()
    check("model_not_called", token["model_called"] is False and token["kind"] == "empty_directory", "empty_directory", token["kind"])
    pre_refusals()
    if not start_starrocks() or not identity():
        summary["finished_at"] = now_iso()
        write_json(EVIDENCE / "summary.json", summary)
        write_reports(summary)
        cleanup()
        return 1
    txn = mysql("START TRANSACTION;")
    phase("long-txn", txn)
    check("long_txn_recorded", True, "server response saved", txn.returncode)
    mysql("ROLLBACK;")
    accounts = create_accounts()
    if accounts is None or not load_fixture(accounts["dba"]):
        cleanup()
        return 1
    install(PROBE, "/tmp/starrocks-probe", "755")
    install(DECODE, "/tmp/starrocks-decode", "755")
    linked = run(["docker", "exec", CONTAINER, "/tmp/starrocks-probe", "read"])
    if b"GLIBC" in linked.stderr:
        phase("glibc", linked)
        cleanup()
        return 1
    scheduler = cap.Scheduler()
    shadow: dict[str, list[dict]] = {}
    dml_count = 0
    dml_mark = {table: 0 for table in TABLES}
    frozen: dict[str, dict[str, str]] = {}
    initial = capture_read(oracle, "baseline", accounts["reader"])
    if initial is None:
        cleanup()
        return 1
    expected = json.loads(EXPECTED.read_text(encoding="utf-8"))
    gold = oracle.compare_phase(expected, EVIDENCE / "decoded" / "baseline", EVIDENCE / "native" / "baseline", "initial")
    write_json(EVIDENCE / "compare" / "baseline-gold.json", gold)
    counts = {table: len(initial[table]) for table in TABLES}
    check("baseline_counts", counts == {"dup_events": 6, "pk_accounts": 3, "agg_metrics": 2, "uniq_dim": 2, "dup_partition_sums": 2}, "3/2/6/2/2", counts)
    check("baseline_gold", gold["ok"] is True, True, gold["problems"])
    registry = json.loads(REGISTRY.read_text(encoding="utf-8"))
    fresh_hashes = json.loads((EVIDENCE / "reads" / "baseline" / "arrow.sha256.json").read_text(encoding="utf-8"))
    retag = False
    for binding in registry["bindings"]:
        for obj in binding["objects"]:
            name = obj["logical_table"] + ".arrow"
            if fresh_hashes.get(name) != obj["arrow_sha256"]:
                retag = True
    check("baseline_arrow_registry", retag is False, "bytes match registry", fresh_hashes)
    for table in BASE_TABLES:
        columns = show_columns(table, accounts["reader"])
        frozen[table] = {column["name"]: column["column_type"] for column in columns}
        parts = show_partitions(table, accounts["reader"])
        decision = cap.plan_table(table, None, parts, parts, False, False)
        _rows, updated = apply_plan(table, decision, [], initial[table], parts)
        shadow[table] = initial[table]
        scheduler.seed(table, updated)
    parent = cap.plan_table("dup_events", None, show_partitions("dup_events", accounts["reader"]), show_partitions("dup_events", accounts["reader"]), False, False)
    derived = cap.plan_derived(parent)
    derived["token"] = hashlib.sha256(json.dumps(cap.canonical(initial["dup_partition_sums"])).encode()).hexdigest()
    derived["partitions"] = None
    shadow["dup_partition_sums"] = initial["dup_partition_sums"]
    scheduler.seed("dup_partition_sums", derived)
    write_json(EVIDENCE / "initial-decisions.json", scheduler.watermarks)
    if not pg_start():
        check("pg_started", False, True, False)
    else:
        check("pg_started", True, True, True)
        if pg_replace("baseline", shadow):
            pg_cells(oracle, "baseline", shadow)
    installed = scheduler.install(FAST, now_ms())
    write_json(EVIDENCE / "config-fast.json", {"request": FAST, "result": installed})
    early = scheduler.begin_tick(now_ms())
    write_json(EVIDENCE / "ticks" / "fast-early.json", early)
    check("fast_early_not_due", early["reason"] == "not_due" and early["read_attempted"] is False, "not_due", early["reason"])
    check("fast_early_302_present", has_id(shadow["pk_accounts"], "account_id", 302), True, has_id(shadow["pk_accounts"], "account_id", 302))
    wait_until(scheduler.next_due_ms or 0)
    replay_ok = run_tick(oracle, scheduler, "fast-replay", accounts["reader"], shadow, dml_count, dml_mark, frozen)
    check("fast_replay", replay_ok, True, replay_ok)
    mutate_ok = True
    for index, statement in enumerate(sql_statements((SRC / "sql" / "dba-mutate.sql").read_text(encoding="utf-8"))):
        mutate_ok = dba_exec(accounts["dba"], f"dba-mutate-{index}", statement) and mutate_ok
        dml_count += 1
    check("dba_mutate", mutate_ok, True, mutate_ok)
    held = scheduler.begin_tick(now_ms())
    write_json(EVIDENCE / "ticks" / "fast-hold.json", held)
    check("fast_hold_not_due", held["reason"] == "not_due", "not_due", held.get("reason"))
    check("fast_hold_old_image", has_id(shadow["pk_accounts"], "account_id", 302) and not any(row["event_id"] == 26 for row in shadow["dup_events"]), True, "302 present and 26 absent")
    wait_until(scheduler.next_due_ms or 0)
    catch_ok = run_tick(oracle, scheduler, "catchup", accounts["reader"], shadow, dml_count, dml_mark, frozen)
    if (EVIDENCE / "decoded" / "catchup" / "pk_accounts.json").exists():
        gold_after = oracle.compare_phase(expected, EVIDENCE / "decoded" / "catchup", EVIDENCE / "native" / "catchup", "after")
    else:
        gold_after = {"ok": False, "problems": ["catchup decode missing"], "pk_class": None}
    write_json(EVIDENCE / "compare" / "catchup-gold.json", gold_after)
    check("catchup_tick", catch_ok, True, catch_ok)
    check("catchup_gold", gold_after.get("ok") is True and gold_after.get("pk_class") == "update_and_delete_visible", "update_and_delete_visible", gold_after.get("pk_class"))
    check("catchup_302_absent", not has_id(shadow["pk_accounts"], "account_id", 302), False, has_id(shadow["pk_accounts"], "account_id", 302))
    if STARTED["postgres"] and pg_replace("catchup", shadow):
        pg_cells(oracle, "catchup", shadow)
        absent = run(["docker", "exec", PG_CONTAINER, "psql", "-U", "postgres", "-d", "oo_w45", "-At", "-c", "SELECT count(*) FROM capture.pk_accounts WHERE account_id = 302;"])
        phase("pg-302-catchup", absent)
        check("pg_302_absent_catchup", absent.stdout.decode().strip() == "0", "0", absent.stdout.decode().strip())
    wait_until(scheduler.next_due_ms or 0)
    if scheduler.next_due_ms and now_ms() < scheduler.next_due_ms:
        wait_until(scheduler.next_due_ms)
    before_mark = json.dumps(scheduler.watermarks, sort_keys=True)
    quiet_ok = run_tick(oracle, scheduler, "fast-quiet", accounts["reader"], shadow, dml_count, dml_mark, frozen)
    check("fast_quiet", quiet_ok and not has_id(shadow["pk_accounts"], "account_id", 302), True, quiet_ok)
    if scheduler.next_due_ms and now_ms() < scheduler.next_due_ms:
        wait_until(scheduler.next_due_ms)
    before_mark = json.dumps(scheduler.watermarks, sort_keys=True)
    opened = scheduler.begin_tick(now_ms())
    write_json(EVIDENCE / "ticks" / "interrupt-begin.json", opened)
    check("interrupt_admitted", opened.get("admitted") is True, True, opened.get("reason"))
    if opened.get("admitted"):
        deadline_code = probe_read("interrupt-deadline", deadline_ms=1)
        check("interrupt_deadline_exit", deadline_code != 0, "non-zero", deadline_code)
        ready = EVIDENCE / "reads" / "interrupt-deadline" / "ready.json"
        ready_body = json.loads(ready.read_text(encoding="utf-8")) if ready.exists() else {"ready": False}
        check("interrupt_not_ready", ready_body.get("ready") is not True, False, ready_body.get("ready"))
        record = scheduler.abort_tick(now_ms(), "deadline")
        check("interrupt_watermark", json.dumps(scheduler.watermarks, sort_keys=True) == before_mark and record["advanced"] is False, "unchanged", record["advanced"])
    wrapper = subprocess.Popen(
        ["docker", "exec", CONTAINER, "sh", "-c", "sleep 30 && exec /tmp/starrocks-probe read --host 127.0.0.1 --port 9030 --password-file /tmp/oo-secrets/reader.password --image-digest " + PIN + " --out /tmp/oo-w45-capture/sigkill"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    time.sleep(0.3)
    killer = run(["docker", "exec", CONTAINER, "pkill", "-9", "-f", "oo-w45-capture/sigkill"])
    phase("sigkill", killer)
    try:
        wrapper.wait(timeout=10)
    except subprocess.TimeoutExpired:
        wrapper.kill()
    check("sigkill_exit", wrapper.returncode not in {0, None}, "killed", wrapper.returncode)
    check("watermark_after_sigkill", json.dumps(scheduler.watermarks, sort_keys=True) == before_mark, "unchanged", True)
    fast_finished = now_ms()
    slow_installed = scheduler.install(SLOW, fast_finished)
    write_json(EVIDENCE / "config-slow.json", {"request": SLOW, "result": slow_installed, "fast_finished_ms": fast_finished, "inflight": scheduler.inflight})
    check("configs_disjoint", scheduler.inflight is None and slow_installed["every_ms"] == 20000 and installed["every_ms"] == 8000, "20s after 8s", slow_installed)
    slow_dml = [
        "INSERT INTO dup_events VALUES (27, '2026-06-12', '2026-06-12 01:00:00', CAST('1.0000' AS DECIMAL(18, 4)), 'slow');",
        "UPDATE uniq_dim SET label='third' WHERE dim_id=17;",
        "DELETE FROM uniq_dim WHERE dim_id=18;",
    ]
    slow_ok = True
    for index, statement in enumerate(slow_dml):
        slow_ok = dba_exec(accounts["dba"], f"dba-slow-{index}", statement) and slow_ok
        dml_count += 1
    check("dba_slow", slow_ok, True, slow_ok)
    slow_early = scheduler.begin_tick(now_ms())
    write_json(EVIDENCE / "ticks" / "slow-early.json", slow_early)
    check("slow_early_not_due", slow_early["reason"] == "not_due", "not_due", slow_early.get("reason"))
    check("slow_early_27_absent", not any(row["event_id"] == 27 for row in shadow["dup_events"]), True, "27 absent")
    wait_until(scheduler.next_due_ms or 0)
    slow_tick = run_tick(oracle, scheduler, "slow", accounts["reader"], shadow, dml_count, dml_mark, frozen)
    check("slow_tick", slow_tick, True, slow_tick)
    check("slow_27_present", any(row["event_id"] == 27 for row in shadow["dup_events"]), True, "27 present")
    check("slow_18_absent", not has_id(shadow["uniq_dim"], "dim_id", 18), False, has_id(shadow["uniq_dim"], "dim_id", 18))
    check("slow_302_still_absent", not has_id(shadow["pk_accounts"], "account_id", 302), False, has_id(shadow["pk_accounts"], "account_id", 302))
    if STARTED["postgres"] and pg_replace("slow", shadow):
        pg_cells(oracle, "slow", shadow)
    added = dba_exec(accounts["dba"], "dba-add-column", "ALTER TABLE pk_accounts ADD COLUMN extra_note VARCHAR(8) NULL;")
    check("dba_add_column", added, True, added)
    guard = cap.schema_guard(APPROVED_COLUMNS["pk_accounts"], show_columns("pk_accounts", accounts["reader"]), frozen.get("pk_accounts"))
    write_json(EVIDENCE / "schema-guard.json", guard)
    check("schema_paused", guard["read"] is False and guard["reason"] == "schema_changed", "schema_changed", guard.get("reason"))
    if scheduler.next_due_ms and now_ms() < scheduler.next_due_ms:
        wait_until(scheduler.next_due_ms)
    paused = run_tick(oracle, scheduler, "schema-pause", accounts["reader"], shadow, dml_count, dml_mark, frozen)
    check("schema_probe_not_committed", paused is False, False, paused)
    probe_dir = EVIDENCE / "reads" / "schema-pause"
    check("schema_probe_not_invoked", not probe_dir.exists(), "no read directory", probe_dir.exists())
    dropped = dba_exec(accounts["dba"], "dba-drop-column", "ALTER TABLE pk_accounts DROP COLUMN extra_note;")
    check("dba_drop_column", dropped, True, dropped)
    typed = dba_exec(accounts["dba"], "dba-retype", "ALTER TABLE pk_accounts MODIFY COLUMN nickname INT NULL;")
    check("type_alter_recorded", True, "response saved", typed)
    if typed:
        dba_exec(accounts["dba"], "dba-retype-restore", "ALTER TABLE pk_accounts MODIFY COLUMN nickname VARCHAR(32) NULL;")
    expiry_before = json.dumps(scheduler.watermarks["dup_events"], sort_keys=True)
    expired_sql = dba_exec(accounts["dba"], "dba-drop-partition", "ALTER TABLE dup_events DROP PARTITION p202605;")
    if not expired_sql:
        expired_sql = dba_exec(accounts["dba"], "dba-add-partition", "ALTER TABLE dup_events ADD PARTITION p202608 VALUES LESS THAN (\"2026-09-01\");")
    check("partition_set_changed", expired_sql, True, expired_sql)
    dml_count += 1
    if scheduler.next_due_ms and now_ms() < scheduler.next_due_ms:
        wait_until(scheduler.next_due_ms)
    expiry_ok = run_tick(oracle, scheduler, "expired", accounts["reader"], shadow, dml_count, dml_mark, frozen)
    mark = scheduler.watermarks.get("dup_events", {})
    check("expiry_tick", expiry_ok, True, expiry_ok)
    check("expiry_not_continued", mark.get("continued_from_old") is False and mark.get("previous_token"), "new generation", mark.get("continued_from_old"))
    check("expiry_old_token_kept", expiry_before != json.dumps(mark, sort_keys=True), "token replaced", mark.get("token"))
    check("expiry_302_absent", not has_id(shadow["pk_accounts"], "account_id", 302), False, has_id(shadow["pk_accounts"], "account_id", 302))
    audits = collect_audits(oracle)
    write_json(EVIDENCE / "reader-audit.json", audits)
    check("reader_write_attempts", audits["write"] == 0 and audits["unexpected"] == 0 and audits["handshake"] > 0, 0, audits["write"])
    fe_text = fe_audit()
    fe_writes = closure_gate.fe_write_attempts(fe_text)
    dba_lines = sum(1 for line in fe_text.splitlines() if "User=oo_w45_dba" in line)
    check("fe_reader_writes", fe_writes == 0, 0, fe_writes)
    check("dba_audit_separate", dba_lines > 0, "dba lines present", dba_lines)
    secret_scan()
    methods = {}
    for tick in scheduler.ticks:
        for name, item in (tick.get("tables") or {}).items():
            methods.setdefault(name, [])
            methods[name].append({"tick": tick["tick_id"], "config": tick["config_id"], "method": item.get("method"), "action": item.get("action"), "grade": item.get("grade"), "reason": item.get("reason"), "quality": item.get("quality")})
    write_json(EVIDENCE / "methods.json", methods)
    summary["finished_at"] = now_iso()
    summary["failed"] = [item["name"] for item in CHECKS if not item["ok"]]
    summary["source_acceptance"] = "not_claimed"
    summary["parent_complete"] = False
    summary["whole_dl_complete"] = False
    summary["fixed_independent_review"] = "not_run"
    summary["model"] = token
    write_json(EVIDENCE / "checks.json", {"checks": CHECKS, "failed": summary["failed"]})
    write_json(EVIDENCE / "phases.json", PHASES)
    write_reports(summary)
    exclude_arrows()
    digest = write_sums()
    summary["evidence_sha256"] = digest
    write_json(EVIDENCE / "summary.json", summary)
    write_json(EVIDENCE / "blocked-interfaces.json", {
        "model": token,
        "self_build": {
            "owner": "public Self-build control 13ad15b4",
            "input": "raw/capture-schedule/decoded and ticks",
            "output": "strict OOS YAML S0-S10 with stable ids",
            "error": "not invoked; token file absent",
        },
        "pg_builder": {
            "owner": "06",
            "input": "new arrow sha files under reads/",
            "output": "source-bound sealed import",
            "error": "cell mirror is not the sealed import; native validator and oracle-loss pack were not run",
        },
        "gateway": {
            "owner": "21",
            "fixed": "ac896",
            "output": "external multi-turn query_id audit",
            "error": "no pack for this generation and no token file",
        },
        "approval": ["production DBA", "source EULA", "domain owner for semantic certification"],
    })
    cleanup()
    print(f"capture schedule failed={summary['failed']}", flush=True)
    return 0 if not summary["failed"] else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as exc:
        save(EVIDENCE / "phases" / "crash.txt", repr(exc))
        cleanup()
        raise
