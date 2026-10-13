#!/usr/bin/env python3
"""ClickHouse DL01-W1-11 target preparation.

Runs the native-client preflight record, starts one labeled ClickHouse
container, loads the synthetic fixture as the DBA, and runs reader probes
over HTTP. The reader path rejects DML/DDL locally and the server query_log
is the audit. This does not compile Rust and does not claim the Arrow to
PostgreSQL consumer.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import secrets
import shutil
import subprocess
import threading
import time
import traceback
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

TARGET = Path(__file__).resolve().parents[1]
REPO = Path(__file__).resolve().parents[4]
IMAGE = (
    "clickhouse/clickhouse-server"
    "@sha256:4ea38f8345ad67dc437b41638804421ee6d21e6d8c6f794bf4b28305541de2aa"
)
OWNER = "oo-dl01w1-11"
SESSION = "4e795a4e-818a-4f9b-8846-09b070d1278c"
CONTAINER = "oo-dl01w1-11-clickhouse"
NETWORK = "oo-dl01w1-11-net"
VOLUME = "oo-dl01w1-11-data"
PORT = 18123
BASE = f"http://127.0.0.1:{PORT}"
SECRET_DIR = Path("/tmp/oo-dl01w1-11-secrets")
LOCK_PATH = Path("/tmp/oo-dl01-wave1-resources.lock")
PREFLIGHT_SRC = Path("/tmp/oo-dl01w1-11-preflight")
DBA_USER = "oo_dl01w1_11_dba"
READER_USER = "oo_dl01w1_11_reader"
CANCEL_ID = "oo-dl01w1-11-cancel"
BUDGET_ID = "oo-dl01w1-11-budget"
FORBIDDEN_FIRST = {
    "insert",
    "delete",
    "alter",
    "create",
    "drop",
    "truncate",
    "optimize",
    "kill",
    "grant",
    "revoke",
    "attach",
    "detach",
    "rename",
    "exchange",
    "system",
    "update",
    "replace",
}
FORBIDDEN_AUDIT_KINDS = {
    "Insert",
    "Delete",
    "Alter",
    "Create",
    "Drop",
    "Truncate",
    "Optimize",
    "KillQuery",
    "Rename",
    "Grant",
    "Revoke",
    "System",
    "Update",
}


class PhaseFailure(Exception):
    pass


def run(cmd: list[str], timeout: int = 120) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, text=True, capture_output=True, timeout=timeout)


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


def first_keyword(sql: str) -> str:
    stripped = re.sub(r"/\*.*?\*/", " ", sql, flags=re.S)
    stripped = re.sub(r"--.*?$", " ", stripped, flags=re.M)
    match = re.match(r"\s*([A-Za-z]+)", stripped)
    return match.group(1).lower() if match else ""


def assert_reader_sql(sql: str) -> None:
    keyword = first_keyword(sql)
    if keyword in FORBIDDEN_FIRST:
        raise PhaseFailure(f"reader statement rejected locally: {keyword}")
    if keyword not in {"select", "set", "show", "exists", "describe", "desc", "with"}:
        raise PhaseFailure(f"reader statement kind is not a read: {keyword}")


def statements(sql: str) -> list[str]:
    parts: list[str] = []
    buf: list[str] = []
    for line in sql.splitlines():
        if line.strip().startswith("--"):
            continue
        buf.append(line)
        if line.rstrip().endswith(";"):
            text = "\n".join(buf).strip()
            if text.endswith(";"):
                text = text[:-1].strip()
            if text:
                parts.append(text)
            buf = []
    trailing = "\n".join(buf).strip()
    if trailing:
        raise PhaseFailure("SQL file has an unterminated statement")
    return parts


def load_json(path: Path):
    return json.loads(path.read_text())


def json_each_row(body: str) -> list[dict]:
    rows = []
    for line in body.splitlines():
        line = line.strip()
        if not line:
            continue
        if line.startswith("Code:"):
            raise PhaseFailure(line[:500])
        rows.append(json.loads(line))
    return rows


def coerce(expected, actual):
    if isinstance(expected, bool) or not isinstance(expected, int):
        return actual
    if isinstance(actual, str) and re.fullmatch(r"-?\d+", actual):
        return int(actual)
    return actual


def align(expected, actual):
    if isinstance(expected, dict) and isinstance(actual, dict):
        return {key: align(expected[key], actual.get(key)) for key in expected}
    if isinstance(expected, list) and isinstance(actual, list):
        if len(expected) != len(actual):
            return actual
        return [align(want, got) for want, got in zip(expected, actual)]
    return coerce(expected, actual)


def row_key(row: dict) -> tuple:
    return (row.get("reading_id"), row.get("version"), row.get("name"), row.get("partition"))


def compare_rows(expected: list, actual: list) -> str:
    if len(expected) != len(actual):
        return f"row count expected {len(expected)} actual {len(actual)}"
    exp = sorted(expected, key=row_key)
    act = sorted(actual, key=row_key)
    for index, (want, got) in enumerate(zip(exp, act)):
        got = align(want, got)
        if want != got:
            return f"row {index} expected {want} actual {got}"
    return ""


def self_check() -> None:
    same = [{"reading_id": 1, "version": 1, "amount": "1.0000000000"}]
    mismatch = compare_rows(same, [{"reading_id": 1, "version": 1, "amount": "2"}])
    if not mismatch:
        raise PhaseFailure("unit comparator accepted a wrong decimal")
    if compare_rows(same, [{"reading_id": "1", "version": 1, "amount": "1.0000000000"}]):
        raise PhaseFailure("unit comparator rejected a quoted integer")
    nested = [{"tags": [1, -2, 9223372036854775807]}]
    if compare_rows(nested, [{"tags": ["1", "-2", "9223372036854775807"]}]):
        raise PhaseFailure("unit comparator rejected quoted integers inside an array")
    probe_dir = TARGET / "probes"
    for path in sorted(probe_dir.glob("*.sql")):
        if path.name == "checksum.sql":
            continue
        assert_reader_sql(path.read_text())
    fixture = statements((TARGET / "dba" / "00-fixture.sql").read_text())
    if len(fixture) < 8:
        raise PhaseFailure(f"fixture split into {len(fixture)} statements")
    for statement in fixture:
        if first_keyword(statement) not in FORBIDDEN_FIRST:
            raise PhaseFailure("fixture statement would be accepted as a reader statement")
    base = (probe_dir / "select-readings.sql").read_text()
    final_sql, nofinal_sql = compose_batches(base)
    if "readings FINAL" not in final_sql:
        raise PhaseFailure("final batch is missing FINAL")
    if "FINAL" in nofinal_sql:
        raise PhaseFailure("non-final batch contains FINAL")
    assert_reader_sql(final_sql)
    assert_reader_sql(nofinal_sql)


class Done(Exception):
    """Stop the live phases after the exit code is already chosen."""


def compose_batches(base: str) -> tuple[str, str]:
    select = re.sub(r"/\*.*?\*/", "", base, flags=re.S).strip()
    if not select.endswith("oo_dl01.readings"):
        raise PhaseFailure("batch select must end at the readings table")
    final_sql = (
        "/* oo-probe:batch-final */\n"
        + select
        + " FINAL\nORDER BY reading_id, version\nFORMAT JSONEachRow"
    )
    nofinal_sql = (
        "/* oo-probe:batch-nofinal */\n"
        + select
        + "\nORDER BY reading_id, version\nFORMAT JSONEachRow"
    )
    return final_sql, nofinal_sql


def compose_partition(year_month: int) -> str:
    return (
        f"/* oo-probe:partition-{year_month} */\n"
        "SELECT reading_id, version\n"
        "FROM oo_dl01.readings FINAL\n"
        f"WHERE toYYYYMM(ts) = {year_month}\n"
        "ORDER BY reading_id, version\n"
        "FORMAT JSONEachRow"
    )


class ClickHouse:
    def __init__(self, user: str, password: str) -> None:
        self.user = user
        self.password = password

    def query(self, sql: str, query_id: str | None = None, timeout: int = 20, reader: bool = False):
        if reader:
            assert_reader_sql(sql)
        params = {}
        if query_id:
            params["query_id"] = query_id
        url = BASE + "/"
        if params:
            url += "?" + urllib.parse.urlencode(params)
        request = urllib.request.Request(url, data=sql.encode(), method="POST")
        request.add_header("X-ClickHouse-User", self.user)
        if self.password:
            request.add_header("X-ClickHouse-Key", self.password)
        started = time.monotonic()
        try:
            with urllib.request.urlopen(request, timeout=timeout) as response:
                body = response.read().decode()
                status = response.status
        except urllib.error.HTTPError as error:
            body = error.read().decode("utf-8", "replace")
            status = error.code
        except (urllib.error.URLError, ConnectionError, TimeoutError, OSError) as error:
            reason = getattr(error, "reason", error)
            body = f"connection_error {reason}"
            status = 0
        elapsed = time.monotonic() - started
        return status, body, elapsed


def unauthenticated(sql: str, timeout: int = 10):
    request = urllib.request.Request(BASE + "/", data=sql.encode(), method="POST")
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return response.status, response.read().decode()
    except urllib.error.HTTPError as error:
        return error.code, error.read().decode("utf-8", "replace")
    except (urllib.error.URLError, ConnectionError, TimeoutError, OSError) as error:
        reason = getattr(error, "reason", error)
        return 0, f"connection_error {reason}"


def ping() -> tuple[int, str]:
    try:
        with urllib.request.urlopen(BASE + "/ping", timeout=3) as response:
            return response.status, response.read().decode()
    except urllib.error.HTTPError as error:
        return error.code, error.read().decode("utf-8", "replace")
    except (urllib.error.URLError, ConnectionError, TimeoutError, OSError) as error:
        reason = getattr(error, "reason", error)
        return 0, f"connection_error {reason}"


def sha256_hex(text: str) -> str:
    return hashlib.sha256(text.encode()).hexdigest()


def prepare_secrets() -> tuple[str, str]:
    if SECRET_DIR.exists():
        shutil.rmtree(SECRET_DIR)
    users = SECRET_DIR / "users.d"
    users.mkdir(parents=True, mode=0o700)
    os.chmod(SECRET_DIR, 0o700)
    dba_password = secrets.token_urlsafe(24)
    reader_password = secrets.token_urlsafe(24)
    (SECRET_DIR / "dba.password").write_text(dba_password)
    (SECRET_DIR / "reader.password").write_text(reader_password)
    os.chmod(SECRET_DIR / "dba.password", 0o600)
    os.chmod(SECRET_DIR / "reader.password", 0o600)
    xml = f"""<clickhouse>
  <profiles>
    <oo_reader>
      <readonly>1</readonly>
      <max_execution_time>8</max_execution_time>
      <!-- Server default is 3000000. sleep(30) is the cancel and budget probe. -->
      <function_sleep_max_microseconds_per_block>30000000</function_sleep_max_microseconds_per_block>
      <max_result_rows>100000</max_result_rows>
      <max_result_bytes>20000000</max_result_bytes>
      <output_format_json_quote_64bit_integers>1</output_format_json_quote_64bit_integers>
      <output_format_json_quote_decimals>1</output_format_json_quote_decimals>
      <output_format_decimal_trailing_zeros>1</output_format_decimal_trailing_zeros>
      <log_queries>1</log_queries>
    </oo_reader>
  </profiles>
  <quotas>
    <oo_reader>
      <interval>
        <duration>3600</duration>
        <queries>400</queries>
        <errors>80</errors>
        <result_rows>1000000</result_rows>
        <read_rows>10000000</read_rows>
        <execution_time>300</execution_time>
      </interval>
    </oo_reader>
  </quotas>
  <users>
    <{DBA_USER}>
      <password_sha256_hex>{sha256_hex(dba_password)}</password_sha256_hex>
      <networks><ip>::/0</ip></networks>
      <profile>default</profile>
      <quota>default</quota>
      <!-- ClickHouse 26.8 rejects access_management beside grants. GRANT ALL is the DBA privilege. -->
      <grants>
        <query>GRANT ALL ON *.* WITH GRANT OPTION</query>
      </grants>
    </{DBA_USER}>
    <{READER_USER}>
      <password_sha256_hex>{sha256_hex(reader_password)}</password_sha256_hex>
      <networks><ip>::/0</ip></networks>
      <profile>oo_reader</profile>
      <quota>oo_reader</quota>
      <!-- users.xml is ACCESS_STORAGE_READONLY, so the DBA cannot SQL GRANT this user. -->
      <grants>
        <query>GRANT SELECT ON oo_dl01.*</query>
        <query>GRANT SELECT ON system.columns</query>
        <query>GRANT SELECT ON system.tables</query>
        <query>GRANT SELECT ON system.databases</query>
        <query>GRANT SELECT ON system.parts</query>
        <query>GRANT SELECT ON system.query_log</query>
        <query>GRANT SELECT ON system.mutations</query>
        <query>GRANT SELECT ON system.processes</query>
        <query>GRANT SELECT ON system.grants</query>
        <query>GRANT SELECT ON system.settings</query>
        <query>GRANT SHOW DATABASES ON *.*</query>
      </grants>
    </{READER_USER}>
  </users>
</clickhouse>
"""
    target = users / "oo-users.xml"
    target.write_text(xml)
    os.chmod(target, 0o600)
    return dba_password, reader_password


def rust_census() -> dict:
    processes = run(["ps", "-axo", "comm="])
    names = {"rustc", "cargo", "rustdoc", "rustup"}
    host = sum(1 for line in processes.stdout.splitlines() if line.strip() in names)
    builders = run(["docker", "ps", "-q", "--filter", "label=oo.kind=rust-builder"])
    running = len([line for line in builders.stdout.splitlines() if line.strip()])
    own = run(
        [
            "docker",
            "ps",
            "-q",
            "--filter",
            f"label=oo.owner={OWNER}",
            "--filter",
            "label=oo.kind=rust-builder",
        ]
    )
    own_count = len([line for line in own.stdout.splitlines() if line.strip()])
    return {
        "host_rust_processes": host,
        "running_rust_builders": running,
        "global_rust": host + running,
        "own_rust_builders": own_count,
        "cap_global": 10,
        "cap_own": 1,
    }


class Lock:
    def __init__(self) -> None:
        self.fd = None

    def __enter__(self):
        self.fd = os.open(LOCK_PATH, os.O_CREAT | os.O_RDWR, 0o644)
        import fcntl

        fcntl.flock(self.fd, fcntl.LOCK_EX)
        return self

    def __exit__(self, exc_type, exc, tb):
        import fcntl

        fcntl.flock(self.fd, fcntl.LOCK_UN)
        os.close(self.fd)
        self.fd = None


def labeled(name: str, kind: str) -> list[str]:
    return [
        "--label",
        "oo.project=open-ontology",
        "--label",
        f"oo.owner={OWNER}",
        "--label",
        f"oo.session={SESSION}",
        "--label",
        f"oo.kind={kind}",
    ]


def remove_ours(attempt: Path) -> dict:
    proof = {}
    inspect = run(["docker", "inspect", CONTAINER])
    if inspect.returncode == 0:
        meta = json.loads(inspect.stdout)[0]
        labels = meta.get("Config", {}).get("Labels", {}) or {}
        if labels.get("oo.owner") != OWNER:
            raise PhaseFailure(f"refusing to remove foreign container {CONTAINER}")
        logs = run(["docker", "logs", CONTAINER], timeout=60)
        write_text(attempt / "docker-logs.stdout", logs.stdout)
        write_text(attempt / "docker-logs.stderr", logs.stderr)
        removed = run(["docker", "rm", "-f", CONTAINER], timeout=60)
        proof["container_rm_exit"] = removed.returncode
        proof["container_rm_stderr"] = removed.stderr.strip()
    else:
        proof["container_rm_exit"] = 0
        proof["container_absent"] = True
    for kind, remover in (
        (VOLUME, ["docker", "volume", "rm", VOLUME]),
        (NETWORK, ["docker", "network", "rm", NETWORK]),
    ):
        inspection = run(["docker", "volume" if kind == VOLUME else "network", "inspect", kind])
        if inspection.returncode != 0:
            proof[kind] = "absent"
            continue
        meta = json.loads(inspection.stdout)[0]
        labels = meta.get("Labels") or {}
        if labels.get("oo.owner") != OWNER:
            raise PhaseFailure(f"refusing to remove foreign resource {kind}")
        removed = run(remover, timeout=60)
        proof[kind] = {"exit": removed.returncode, "stderr": removed.stderr.strip()}
    remaining = run(["docker", "ps", "-a", "--filter", f"name={OWNER}", "--format", "{{.Names}} {{.Status}}"])
    proof["remaining_names"] = remaining.stdout.strip()
    volumes = run(["docker", "volume", "ls", "--filter", f"name={VOLUME}", "--format", "{{.Name}}"])
    proof["remaining_volumes"] = volumes.stdout.strip()
    networks = run(["docker", "network", "ls", "--filter", f"name={NETWORK}", "--format", "{{.Name}}"])
    proof["remaining_networks"] = networks.stdout.strip()
    image = run(["docker", "image", "inspect", IMAGE, "--format", "{{.Id}} {{.Os}}/{{.Architecture}}"])
    proof["preserved_image"] = image.stdout.strip()
    proof["preserved_image_exit"] = image.returncode
    return proof


def start_container(memory: str) -> subprocess.CompletedProcess[str]:
    with Lock():
        census = rust_census()
        if census["global_rust"] >= 10 or census["own_rust_builders"] >= 1:
            raise PhaseFailure("pending-capacity " + json.dumps(census))
        for create in (
            ["docker", "network", "create", *labeled(NETWORK, "network"), NETWORK],
            ["docker", "volume", "create", *labeled(VOLUME, "volume"), VOLUME],
        ):
            created = run(create)
            if created.returncode != 0 and "already exists" not in created.stderr:
                raise PhaseFailure(created.stderr.strip() or created.stdout.strip())
        command = [
            "docker",
            "run",
            "-d",
            "--name",
            CONTAINER,
            "--network",
            NETWORK,
            "--memory",
            memory,
            "--memory-swap",
            memory,
            "--cpus",
            "1",
            "--ulimit",
            "nofile=262144:262144",
            "-p",
            f"127.0.0.1:{PORT}:8123",
            "-v",
            f"{VOLUME}:/var/lib/clickhouse",
            "-v",
            f"{TARGET / 'config' / 'limits.xml'}:/etc/clickhouse-server/config.d/oo-limits.xml:ro",
            "-v",
            f"{SECRET_DIR / 'users.d' / 'oo-users.xml'}:/etc/clickhouse-server/users.d/oo-users.xml:ro",
            *labeled(CONTAINER, "clickhouse-source"),
            IMAGE,
        ]
        started = run(command, timeout=60)
        if started.returncode != 0:
            raise PhaseFailure(started.stderr.strip() or started.stdout.strip())
        return started


def wait_ready(attempt: Path, seconds: int = 90) -> None:
    deadline = time.monotonic() + seconds
    last = ""
    while time.monotonic() < deadline:
        state = run(["docker", "inspect", "-f", "{{.State.Status}} {{.State.OOMKilled}} {{.State.ExitCode}}", CONTAINER])
        last = state.stdout.strip()
        if last.startswith("exited") or last.startswith("dead"):
            logs = run(["docker", "logs", CONTAINER], timeout=30)
            write_text(attempt / "start-failed.stdout", logs.stdout)
            write_text(attempt / "start-failed.stderr", logs.stderr)
            raise PhaseFailure(f"container exited during start: {last}")
        status, body = ping()
        if status == 200 and "Ok" in body:
            write_text(attempt / "ping.txt", f"status={status}\n{body}\n")
            return
        time.sleep(1)
    raise PhaseFailure(f"ping timeout, last state {last}")


def save_http(attempt: Path, name: str, status: int, body: str, elapsed: float) -> None:
    write_text(attempt / "http" / f"{name}.status", f"{status}\n")
    write_text(attempt / "http" / f"{name}.elapsed", f"{elapsed:.6f}\n")
    write_text(attempt / "http" / f"{name}.body", body)


def exec_sql(client: ClickHouse, sql_text: str, attempt: Path, prefix: str, reader: bool) -> None:
    for index, statement in enumerate(statements(sql_text), start=1):
        status, body, elapsed = client.query(statement, timeout=60, reader=reader)
        save_http(attempt, f"{prefix}-{index:02d}", status, body, elapsed)
        if status != 200:
            raise PhaseFailure(f"{prefix}-{index:02d} status {status}: {body[:400]}")


def require_rows(attempt: Path, name: str, client: ClickHouse, sql: str, expected, reader: bool = True, timeout: int = 20):
    status, body, elapsed = client.query(sql, reader=reader, timeout=timeout)
    save_http(attempt, name, status, body, elapsed)
    if status != 200:
        return f"{name} status {status}: {body[:400]}"
    try:
        actual = json_each_row(body)
    except PhaseFailure as error:
        return f"{name} {error}"
    if isinstance(expected, dict):
        if len(actual) != 1:
            return f"{name} expected one row, got {len(actual)}"
        got = align(expected, actual[0])
        for key, value in expected.items():
            if key == "engine_full_contains":
                engine_full = actual[0].get("engine_full", "")
                missing = [part for part in value if part not in engine_full]
                if missing:
                    return f"{name} engine_full missing {missing}: {engine_full}"
                continue
            if got.get(key) != value:
                return f"{name} field {key} expected {value!r} actual {got.get(key)!r} raw {actual[0].get(key)!r}"
        return ""
    return compare_rows(expected, actual) and f"{name} {compare_rows(expected, actual)}"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--evidence",
        type=Path,
        default=REPO / "evidence" / SESSION / "DL01" / "clickhouse",
    )
    parser.add_argument("--self-check-only", action="store_true")
    args = parser.parse_args()
    evidence = args.evidence
    evidence.mkdir(parents=True, exist_ok=True)
    self_dir = evidence / "raw" / "self-check"
    try:
        self_check()
    except Exception as error:
        write_text(self_dir / "exit", "1\n")
        write_text(self_dir / "stderr", traceback.format_exc())
        print(f"self-check failed: {error}")
        return 1
    write_text(self_dir / "exit", "0\n")
    write_text(self_dir / "stdout", "labeled unit comparator and reader-sql guard passed\n")
    if args.self_check_only:
        print("self-check-only passed")
        return 0

    attempt_index = 1
    while (evidence / "raw" / f"attempt-{attempt_index:02d}").exists():
        attempt_index += 1
    attempt = evidence / "raw" / f"attempt-{attempt_index:02d}"
    attempt.mkdir(parents=True)
    if PREFLIGHT_SRC.exists():
        shutil.copytree(PREFLIGHT_SRC, attempt / "preflight", dirs_exist_ok=True)
    failures: list[str] = []
    cleanup = {}
    exit_code = 0
    try:
        dba_password, reader_password = prepare_secrets()
        with Lock():
            census = rust_census()
            write_text(attempt / "admission.json", json.dumps(census, indent=2) + "\n")
            if census["global_rust"] >= 10 or census["own_rust_builders"] >= 1:
                write_text(
                    evidence / "pending-capacity.json",
                    json.dumps({"task": "DL01-W1-11", "census": census}, indent=2) + "\n",
                )
                print("pending-capacity")
                exit_code = 3
                raise Done()
        # A previous attempt of this task may have left its own container.
        preexisting = run(["docker", "inspect", CONTAINER])
        if preexisting.returncode == 0:
            remove_ours(attempt / "preclean")
        memory = "1536m"
        try:
            started = start_container(memory)
        except PhaseFailure as error:
            write_text(attempt / "start.stderr", str(error))
            failures.append(str(error))
            if str(error).startswith("pending-capacity"):
                write_text(
                    evidence / "pending-capacity.json",
                    json.dumps({"task": "DL01-W1-11", "detail": str(error)}, indent=2) + "\n",
                )
                exit_code = 3
            else:
                exit_code = 2
            raise Done()
        write_text(attempt / "start.stdout", started.stdout)
        try:
            wait_ready(attempt)
        except PhaseFailure as first_error:
            write_text(attempt / "start-first.failure", str(first_error) + "\n")
            remove_ours(attempt / "start-first-cleanup")
            memory = "2g"
            try:
                started = start_container(memory)
                write_text(attempt / "start-retry.stdout", started.stdout)
                wait_ready(attempt)
            except PhaseFailure as retry_error:
                failures.append(f"first start failed: {first_error}")
                failures.append(f"start retry failed: {retry_error}")
                exit_code = 2
                raise Done()
        write_text(attempt / "memory.txt", memory + "\n")

        anon_status, anon_body = unauthenticated("SELECT 1")
        save_http(attempt, "lockdown-anonymous", anon_status, anon_body, 0)
        if anon_status == 200:
            failures.append("anonymous HTTP read succeeded")
        default_status, default_body, default_elapsed = ClickHouse("default", "").query("SELECT 1", reader=False)
        save_http(attempt, "lockdown-default-empty", default_status, default_body, default_elapsed)
        if default_status == 200:
            failures.append("default user with empty password succeeded from the host")
        wrong_status, wrong_body, wrong_elapsed = ClickHouse(READER_USER, "wrong-password").query(
            "SELECT 1", reader=False
        )
        save_http(attempt, "lockdown-wrong-password", wrong_status, wrong_body, wrong_elapsed)
        if wrong_status == 200:
            failures.append("reader accepted a wrong password")
        if failures:
            exit_code = 2
            raise Done()

        dba = ClickHouse(DBA_USER, dba_password)
        reader = ClickHouse(READER_USER, reader_password)
        ok_status, ok_body, ok_elapsed = reader.query("SELECT 1 FORMAT TabSeparated", reader=True)
        save_http(attempt, "reader-select-1", ok_status, ok_body, ok_elapsed)
        if ok_status != 200 or ok_body.strip() != "1":
            failures.append(f"reader control select failed: {ok_status} {ok_body[:200]}")
            exit_code = 2
            raise Done()

        version_sql = "SELECT version() FORMAT TabSeparated"
        for label, client in (("dba-version", dba), ("reader-version", reader)):
            status, body, elapsed = client.query(version_sql, reader=(label.startswith("reader")))
            save_http(attempt, label, status, body, elapsed)
        native = run(
            [
                "docker",
                "exec",
                CONTAINER,
                "clickhouse-client",
                "--version",
            ]
        )
        write_text(attempt / "native-client.stdout", native.stdout)
        write_text(attempt / "native-client.stderr", native.stderr)
        write_text(attempt / "native-client.exit", f"{native.returncode}\n")

        try:
            exec_sql(dba, (TARGET / "dba" / "00-fixture.sql").read_text(), attempt, "dba-fixture", False)
            exec_sql(dba, (TARGET / "dba" / "01-grants.sql").read_text(), attempt, "dba-grants", False)
        except PhaseFailure as error:
            failures.append(str(error))
            exit_code = 1
            raise Done()

        checksum_sql = (TARGET / "probes" / "checksum.sql").read_text()
        before_status, before_body, before_elapsed = dba.query(checksum_sql, reader=False)
        save_http(attempt, "checksum-before", before_status, before_body, before_elapsed)

        final_sql, nofinal_sql = compose_batches((TARGET / "probes" / "select-readings.sql").read_text())
        checks = [
            ("catalog-columns", reader, (TARGET / "probes" / "catalog-columns.sql").read_text(), load_json(TARGET / "expected" / "catalog-columns.json")),
            ("catalog-table", reader, (TARGET / "probes" / "catalog-table.sql").read_text(), load_json(TARGET / "expected" / "catalog-table.json")),
            ("batch-final", reader, final_sql, load_json(TARGET / "expected" / "batch-final.json")),
            ("batch-nofinal", reader, nofinal_sql, load_json(TARGET / "expected" / "batch-nofinal.json")),
            ("partition-202401", reader, compose_partition(202401), load_json(TARGET / "expected" / "partition-202401.json")),
            ("partition-202403", reader, compose_partition(202403), load_json(TARGET / "expected" / "partition-202403.json")),
            ("parts", reader, (TARGET / "probes" / "parts.sql").read_text(), load_json(TARGET / "expected" / "parts.json")),
            ("settings", reader, (TARGET / "probes" / "settings.sql").read_text(), [
                {"name": "max_execution_time", "value": "8"},
                {"name": "readonly", "value": "1"},
            ]),
        ]
        for name, client, sql, expected in checks:
            problem = require_rows(attempt, name, client, sql, expected, reader=True)
            if problem:
                failures.append(problem)

        for name, path in (
            ("negative-set", TARGET / "probes" / "negative-set.sql"),
            ("negative-settings", TARGET / "probes" / "negative-settings.sql"),
        ):
            status, body, elapsed = reader.query(path.read_text(), reader=True)
            save_http(attempt, name, status, body, elapsed)
            if status == 200 or "READONLY" not in body:
                failures.append(f"{name} did not fail closed: {status} {body[:300]}")

        # One reader HTTP call runs on a side thread so the DBA can kill it.
        # Join that thread before any later reader call. Business reads stay serial.
        cancel_holder: dict = {}

        def run_cancel() -> None:
            cancel_holder["result"] = reader.query(
                (TARGET / "probes" / "cancel.sql").read_text(),
                query_id=CANCEL_ID,
                timeout=25,
                reader=True,
            )

        cancel_thread = threading.Thread(target=run_cancel)
        cancel_thread.start()
        seen = False
        kill_status = 0
        kill_body = ""
        for _ in range(50):
            process_status, process_body, _ = dba.query(
                "SELECT query_id FROM system.processes WHERE query_id = "
                f"'{CANCEL_ID}' FORMAT TabSeparated",
                reader=False,
                timeout=5,
            )
            if process_status == 200 and CANCEL_ID in process_body:
                seen = True
                kill_status, kill_body, _ = dba.query(
                    f"KILL QUERY WHERE query_id = '{CANCEL_ID}' SYNC",
                    reader=False,
                    timeout=20,
                )
                break
            time.sleep(0.1)
        cancel_thread.join(timeout=30)
        if "result" not in cancel_holder:
            failures.append("cancel reader thread did not finish")
            cancel_status, cancel_body, cancel_elapsed = 0, "thread_timeout", 30.0
        else:
            cancel_status, cancel_body, cancel_elapsed = cancel_holder["result"]
        save_http(attempt, "cancel-reader", cancel_status, cancel_body, cancel_elapsed)
        save_http(attempt, "cancel-kill", kill_status, kill_body, 0)
        write_text(attempt / "cancel-seen.txt", f"seen={seen}\n")
        cancelled = "cancelled" in cancel_body.lower() or "QUERY_WAS_CANCELLED" in cancel_body
        if not seen or not cancelled or cancel_elapsed >= 8:
            failures.append(
                f"cancel failed seen={seen} elapsed={cancel_elapsed:.3f} body={cancel_body[:300]}"
            )

        budget_status, budget_body, budget_elapsed = reader.query(
            (TARGET / "probes" / "budget.sql").read_text(),
            query_id=BUDGET_ID,
            timeout=25,
            reader=True,
        )
        save_http(attempt, "budget", budget_status, budget_body, budget_elapsed)
        timed_out = "TIMEOUT_EXCEEDED" in budget_body or "Timeout exceeded" in budget_body
        if budget_status == 200 or not timed_out or not (5 <= budget_elapsed <= 20):
            failures.append(
                f"budget timeout not observed elapsed={budget_elapsed:.3f} body={budget_body[:300]}"
            )

        flush_status, flush_body, flush_elapsed = dba.query("SYSTEM FLUSH LOGS", reader=False, timeout=30)
        save_http(attempt, "dba-flush", flush_status, flush_body, flush_elapsed)
        audit_problem = None
        audit_status, audit_body, audit_elapsed = reader.query(
            (TARGET / "probes" / "audit.sql").read_text(), reader=True, timeout=30
        )
        save_http(attempt, "audit", audit_status, audit_body, audit_elapsed)
        if audit_status != 200:
            failures.append(f"audit status {audit_status}: {audit_body[:400]}")
        else:
            try:
                audit_rows = json_each_row(audit_body)
            except PhaseFailure as error:
                failures.append(f"audit parse {error}")
                audit_rows = []
            kinds = sorted({row.get("query_kind", "") for row in audit_rows})
            write_text(attempt / "audit-kinds.json", json.dumps(kinds, indent=2) + "\n")
            bad = [
                row.get("query_kind")
                for row in audit_rows
                if row.get("query_kind") in FORBIDDEN_AUDIT_KINDS
            ]
            if bad:
                failures.append(f"reader query_log contains forbidden kinds {bad}")
            def marker_ok(marker: str) -> bool:
                return any(
                    marker in row.get("query", "")
                    and row.get("type") == "QueryFinish"
                    and int(row.get("exception_code") or 0) == 0
                    for row in audit_rows
                )

            for marker in ("oo-probe:batch-final", "oo-probe:catalog-columns", "oo-probe:parts"):
                if not marker_ok(marker):
                    failures.append(f"audit missing successful {marker}")
            for marker in ("oo-probe:negative-set", "oo-probe:cancel", "oo-probe:budget"):
                matched = [row for row in audit_rows if marker in row.get("query", "")]
                successful = [
                    row
                    for row in matched
                    if row.get("type") == "QueryFinish" and int(row.get("exception_code") or 0) == 0
                ]
                if not matched or successful:
                    failures.append(f"audit did not record a failed {marker}")

        grants_status, grants_body, grants_elapsed = reader.query(
            (TARGET / "probes" / "grants.sql").read_text(), reader=True
        )
        save_http(attempt, "grants", grants_status, grants_body, grants_elapsed)
        if grants_status != 200:
            failures.append(f"grants status {grants_status}: {grants_body[:300]}")
        else:
            grants = json_each_row(grants_body)
            access = {row.get("access_type", "") for row in grants}
            write_text(attempt / "grant-access.json", json.dumps(sorted(access), indent=2) + "\n")
            if "SELECT" not in access:
                failures.append(f"reader grants missing SELECT: {sorted(access)}")
            write_access = access.intersection(
                {"INSERT", "ALTER", "CREATE", "DROP", "DELETE", "TRUNCATE", "SYSTEM", "OPTIMIZE", "ALL"}
            )
            if write_access:
                failures.append(f"reader has write grants {sorted(write_access)}")

        after_status, after_body, after_elapsed = dba.query(checksum_sql, reader=False)
        save_http(attempt, "checksum-after", after_status, after_body, after_elapsed)
        if before_status != 200 or after_status != 200 or before_body != after_body:
            failures.append("reader activity changed the DBA checksum of readings")

        retire_status, retire_body, retire_elapsed = reader.query(
            (TARGET / "probes" / "retirees.sql").read_text(), reader=True
        )
        save_http(attempt, "retirees", retire_status, retire_body, retire_elapsed)
        mutation_status, mutation_body, mutation_elapsed = dba.query(
            (TARGET / "probes" / "mutations.sql").read_text(), reader=False
        )
        save_http(attempt, "mutations-dba", mutation_status, mutation_body, mutation_elapsed)
        observation = {"retirees_status": retire_status, "mutations_status": mutation_status}
        try:
            visible = json_each_row(retire_body) if retire_status == 200 else []
            mutations = json_each_row(mutation_body) if mutation_status == 200 else []
            visible_ids = sorted(int(row["id"]) for row in visible)
            done = [int(row.get("is_done") or 0) for row in mutations]
            observation.update(
                {
                    "visible_ids": visible_ids,
                    "is_done": done,
                    "lag_observed": (9 in visible_ids) and any(flag == 0 for flag in done),
                    "contradiction": (9 in visible_ids) and any(flag == 1 for flag in done),
                }
            )
            if 10 not in visible_ids:
                failures.append("retirees lost the untouched row 10")
            if observation["contradiction"]:
                failures.append("mutation is done but the deleted row is still visible")
            if not mutations:
                failures.append("DBA delete left no system.mutations row")
        except (PhaseFailure, KeyError, ValueError) as error:
            failures.append(f"retirees observation {error}")
        write_text(attempt / "delete-observation.json", json.dumps(observation, indent=2) + "\n")

        try:
            exec_sql(dba, (TARGET / "dba" / "rollback.sql").read_text(), attempt, "dba-rollback", False)
        except PhaseFailure as error:
            failures.append(str(error))
        rollback_status, rollback_body, rollback_elapsed = reader.query(
            (TARGET / "probes" / "after-rollback.sql").read_text(), reader=True
        )
        save_http(attempt, "after-rollback", rollback_status, rollback_body, rollback_elapsed)
        if rollback_status == 200:
            failures.append("reader still read readings after DBA rollback")

        stats = run(
            ["docker", "stats", "--no-stream", "--format", "{{.Name}} {{.MemUsage}} {{.MemPerc}}", CONTAINER]
        )
        write_text(attempt / "docker-stats.txt", stats.stdout + stats.stderr)
        inspected = run(["docker", "inspect", CONTAINER])
        if inspected.returncode == 0:
            meta = json.loads(inspected.stdout)
            redacted = redact(meta)
            write_text(attempt / "docker-inspect.json", json.dumps(redacted, indent=2) + "\n")
        if failures and exit_code == 0:
            exit_code = 1
    except Done:
        pass
    except Exception as error:
        failures.append(str(error))
        write_text(attempt / "uncaught.txt", traceback.format_exc())
        if exit_code == 0:
            exit_code = 1
    finally:
        try:
            cleanup = remove_ours(attempt / "cleanup")
        except Exception as error:
            cleanup = {"error": str(error)}
            failures.append(f"cleanup failed: {error}")
            if exit_code == 0:
                exit_code = 4
        image_left = run(["docker", "image", "inspect", IMAGE, "--format", "{{.Id}}"])
        summary = {
            "attempt": str(attempt.relative_to(evidence)),
            "exit_code": exit_code,
            "failures": failures,
            "cleanup": cleanup,
            "image_still_present": image_left.returncode == 0,
            "memory_cap": "1536m with one 2g retry only if the first start exits",
        }
        write_text(attempt / "summary.json", json.dumps(summary, indent=2) + "\n")
        write_text(evidence / "raw" / "latest-attempt.txt", attempt.name + "\n")
    return exit_code


def redact(value):
    if isinstance(value, dict):
        cleaned = {}
        for key, item in value.items():
            if re.search(r"password|secret|token|key", key, re.I):
                cleaned[key] = "[redacted]"
            else:
                cleaned[key] = redact(item)
        return cleaned
    if isinstance(value, list):
        return [redact(item) for item in value]
    return value


if __name__ == "__main__":
    raise SystemExit(main())
