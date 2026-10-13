#!/usr/bin/env python3
"""Live native ClickHouse gate for DL01-W1-40.

The Rust reader is the only business HTTP client. This process admits the
database, writes private credentials, and reads the JSON files the reader writes.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import secrets
import subprocess
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
TREE = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / "evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-driver-preflight"
SECRET = Path("/tmp/oo-dl01w1-40-secrets")
RUST = "oo-dl01w1-40-rust"
CLICKHOUSE = "oo-dl01w1-40-clickhouse"
NETWORK = "oo-dl01w1-40-net"
VOLUME = "oo-dl01w1-40-data"
IMAGE = "clickhouse/clickhouse-server@sha256:4ea38f8345ad67dc437b41638804421ee6d21e6d8c6f794bf4b28305541de2aa"
ENDPOINT = "http://oo-dl01w1-40-clickhouse:8123"
PORT = 18440
READER = "oo_dl01w1_40_reader"
DBA = "oo_dl01w1_40_dba"
SESSION = "4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de"
OWNER = "oo-dl01w1-40"

FORMAT_TAIL = re.compile(r"(?is)\s+FORMAT\s+[A-Za-z0-9_]+\s*$")


def apply_settings(body: str, settings: list[tuple[str, str]] | None) -> str:
    """Put SETTINGS before a trailing FORMAT clause. ClickHouse rejects the reverse order."""
    body = body.strip()
    if body.endswith(";"):
        body = body[:-1].strip()
    items = list(settings or [])
    if not items:
        return body
    joined = ",\n    ".join(f"{key} = {value}" for key, value in items)
    clause = f"SETTINGS\n    {joined}"
    match = FORMAT_TAIL.search(body)
    if match is None:
        return f"{body}\n{clause}"
    head = body[: match.start()].rstrip()
    tail = body[match.start() :].strip()
    return f"{head}\n{clause}\n{tail}"


ARROW_SETTINGS = [
    ("output_format_arrow_compression_method", "'none'"),
    ("output_format_arrow_string_as_string", "0"),
    ("output_format_arrow_low_cardinality_as_dictionary", "1"),
    ("max_result_rows", "10"),
    ("max_result_bytes", "20000000"),
    ("result_overflow_mode", "'throw'"),
]


class PhaseFailure(RuntimeError):
    pass


def run(cmd: list[str], timeout: int = 60) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, text=True, capture_output=True, timeout=timeout)


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


def sha256_hex(text: str) -> str:
    return hashlib.sha256(text.encode()).hexdigest()


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    digest.update(path.read_bytes())
    return digest.hexdigest()


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


def container_path(path: Path) -> str:
    rel = path.resolve().relative_to(EVIDENCE.resolve())
    return "/evidence/" + rel.as_posix()


def labels() -> list[str]:
    return [
        "--label",
        "oo.project=open-ontology",
        "--label",
        f"oo.owner={OWNER}",
        "--label",
        f"oo.session={SESSION}",
    ]


def owned(name: str) -> bool:
    inspected = run(["docker", "inspect", "-f", '{{index .Config.Labels "oo.owner"}}', name])
    return inspected.returncode == 0 and inspected.stdout.strip() == OWNER


def next_attempt() -> Path:
    raw = EVIDENCE / "raw"
    raw.mkdir(parents=True, exist_ok=True)
    number = 1
    while (raw / f"attempt-{number:02d}").exists():
        number += 1
    path = raw / f"attempt-{number:02d}"
    path.mkdir()
    return path


def prepare_secrets(include_cancel: bool) -> None:
    if SECRET.exists():
        for child in SECRET.rglob("*"):
            if child.is_file():
                child.unlink()
    users = SECRET / "users.d"
    users.mkdir(parents=True, exist_ok=True, mode=0o700)
    os.chmod(SECRET, 0o700)
    dba_password = secrets.token_urlsafe(24)
    reader_password = secrets.token_urlsafe(24)
    write_secret(SECRET / "dba.password", dba_password)
    write_secret(SECRET / "reader.password", reader_password)
    write_secret(SECRET / "dba.user", DBA)
    write_secret(SECRET / "reader.user", READER)
    write_secret(SECRET / "reader.wrong", secrets.token_urlsafe(24))
    write_secret(SECRET / "default.user", "default")
    write_secret(SECRET / "default.password", "")
    cancel = ""
    if include_cancel:
        cancel = "      <cancel_http_readonly_queries_on_client_close>1</cancel_http_readonly_queries_on_client_close>\n"
    xml = f"""<clickhouse>
  <profiles>
    <oo_reader>
      <readonly>1</readonly>
      <max_execution_time>8</max_execution_time>
      <function_sleep_max_microseconds_per_block>30000000</function_sleep_max_microseconds_per_block>
      <max_result_rows>100000</max_result_rows>
      <max_result_bytes>20000000</max_result_bytes>
      <output_format_json_quote_64bit_integers>1</output_format_json_quote_64bit_integers>
      <output_format_json_quote_decimals>1</output_format_json_quote_decimals>
      <output_format_decimal_trailing_zeros>1</output_format_decimal_trailing_zeros>
      <log_queries>1</log_queries>
      <log_comment>oo-dl01w1-40:op:profile</log_comment>
      <output_format_arrow_compression_method>none</output_format_arrow_compression_method>
      <output_format_arrow_string_as_string>0</output_format_arrow_string_as_string>
      <output_format_arrow_low_cardinality_as_dictionary>1</output_format_arrow_low_cardinality_as_dictionary>
      <result_overflow_mode>throw</result_overflow_mode>
      <constraints>
        <max_execution_time>
          <min>1</min>
          <max>8</max>
          <changeable_in_readonly/>
        </max_execution_time>
        <max_result_rows>
          <min>1</min>
          <max>100000</max>
          <changeable_in_readonly/>
        </max_result_rows>
        <max_result_bytes>
          <min>1</min>
          <max>20000000</max>
          <changeable_in_readonly/>
        </max_result_bytes>
      </constraints>
{cancel}    </oo_reader>
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
    <{DBA}>
      <password_sha256_hex>{sha256_hex(dba_password)}</password_sha256_hex>
      <networks><ip>::/0</ip></networks>
      <profile>default</profile>
      <quota>default</quota>
      <grants><query>GRANT ALL ON *.* WITH GRANT OPTION</query></grants>
    </{DBA}>
    <{READER}>
      <password_sha256_hex>{sha256_hex(reader_password)}</password_sha256_hex>
      <networks><ip>::/0</ip></networks>
      <profile>oo_reader</profile>
      <quota>oo_reader</quota>
      <grants>
        <query>GRANT SELECT ON oo_dl01w40.*</query>
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
    </{READER}>
  </users>
</clickhouse>
"""
    target = users / "oo-users.xml"
    target.write_text(xml)
    os.chmod(target, 0o600)


def write_secret(path: Path, text: str) -> None:
    path.write_text(text)
    os.chmod(path, 0o600)


def ping() -> tuple[int, str]:
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{PORT}/ping", timeout=3) as response:
            return response.status, response.read().decode()
    except urllib.error.HTTPError as error:
        return error.code, error.read().decode("utf-8", "replace")
    except (urllib.error.URLError, TimeoutError, OSError) as error:
        return 0, f"connection_error {error}"


def wait_ping(attempt: Path, seconds: int = 90) -> None:
    deadline = time.monotonic() + seconds
    last = ""
    while time.monotonic() < deadline:
        state = run(["docker", "inspect", "-f", "{{.State.Status}} {{.State.OOMKilled}} {{.State.ExitCode}}", CLICKHOUSE])
        last = state.stdout.strip()
        if last.startswith("exited") or last.startswith("dead"):
            save_logs(attempt, "start-failed")
            raise PhaseFailure(f"clickhouse exited during start: {last}")
        status, body = ping()
        if status == 200 and "Ok" in body:
            write_text(attempt / "ping.txt", f"status={status}\n{body}\n")
            return
        time.sleep(1)
    save_logs(attempt, "start-timeout")
    raise PhaseFailure(f"ping timeout, last state {last}")


def save_logs(attempt: Path, name: str) -> None:
    logs = run(["docker", "logs", CLICKHOUSE], timeout=60)
    write_text(attempt / f"{name}.stdout", logs.stdout)
    write_text(attempt / f"{name}.stderr", logs.stderr)


def remove_owned_container(name: str) -> None:
    if not owned(name):
        return
    run(["docker", "rm", "-f", name], timeout=60)


def start_clickhouse(attempt: Path, include_cancel: bool) -> bool:
    prepare_secrets(include_cancel)
    created = run(["docker", "volume", "create", *labels(), "--label", "oo.kind=volume", VOLUME])
    if created.returncode != 0 and "already exists" not in created.stderr:
        raise PhaseFailure(created.stderr.strip() or created.stdout.strip())
    command = [
        "docker", "run", "-d", "--name", CLICKHOUSE, "--platform", "linux/arm64", "--pull", "never",
        "--network", NETWORK, "--memory", "2g", "--memory-swap", "2g", "--cpus", "1",
        "--ulimit", "nofile=262144:262144", "-p", f"127.0.0.1:{PORT}:8123",
        "-v", f"{VOLUME}:/var/lib/clickhouse",
        "-v", f"{TREE / 'config' / 'limits.xml'}:/etc/clickhouse-server/config.d/oo-limits.xml:ro",
        "-v", f"{SECRET / 'users.d' / 'oo-users.xml'}:/etc/clickhouse-server/users.d/oo-users.xml:ro",
        *labels(), "--label", "oo.kind=clickhouse-source", "--label", "oo.task=DL01-W1-40",
        IMAGE,
    ]
    admission = attempt / ("admission.json" if include_cancel else "admission-retry.json")
    started = None
    for wait_n in range(90):
        started = run(
            ["python3", str(TREE / "scripts" / "admit.py"), "acquire-start", "clickhouse", str(admission), "--", *command],
            timeout=90,
        )
        write_text(attempt / f"{admission.name}.exit", f"{started.returncode}\n{started.stderr}\n")
        if started.returncode == 0:
            break
        reason = ""
        if admission.exists():
            try:
                reason = str(json.loads(admission.read_text()).get("reason", ""))
            except json.JSONDecodeError:
                reason = ""
        with (attempt / "admission-wait.log").open("a", encoding="utf-8") as handle:
            handle.write(f"{wait_n} {reason}\n")
        if "reserved CPUs" in reason or "headroom" in reason:
            time.sleep(900)
            continue
        raise PhaseFailure(started.stderr.strip() or reason or "clickhouse was not admitted")
    else:
        raise PhaseFailure("clickhouse was not admitted before the capacity wait ended")
    try:
        wait_ping(attempt)
    except PhaseFailure as error:
        logs = (attempt / "start-failed.stderr").read_text() if (attempt / "start-failed.stderr").exists() else ""
        logs += (attempt / "start-failed.stdout").read_text() if (attempt / "start-failed.stdout").exists() else ""
        if include_cancel and "cancel_http_readonly_queries_on_client_close" in logs:
            remove_owned_container(CLICKHOUSE)
            run(["docker", "volume", "rm", "-f", VOLUME])
            write_text(attempt / "cancel-setting-rejected.txt", "profile setting rejected; restarted without it\n")
            return start_clickhouse(attempt, False)
        raise error
    write_text(attempt / "profile-cancel-setting.txt", f"included={include_cancel}\n")
    return include_cancel


class Client:
    def __init__(self, attempt: Path) -> None:
        self.attempt = attempt
        self.known_settings: set[str] = set()

    def send(
        self,
        role: str,
        command: str,
        query_id: str,
        sql: str,
        *,
        timeout_ms: int = 15000,
        forbidden: bool = False,
        anonymous: bool = False,
        session_id: str = "",
        close_session: bool = False,
        ipc: bool = False,
        password_name: str = "",
    ) -> dict:
        sent = self.attempt / "sent" / f"{query_id}.sql"
        write_text(sent, sql)
        out = self.attempt / "http" / f"{query_id}.json"
        body = self.attempt / "http" / f"{query_id}.body"
        ipc_path = self.attempt / "ipc" / f"{query_id}.arrow"
        args = [
            command,
            "--endpoint", ENDPOINT,
            "--query-id", query_id,
            "--body", container_path(sent),
            "--out", container_path(out),
            "--body-out", container_path(body),
            "--timeout-ms", str(timeout_ms),
        ]
        if anonymous:
            args.append("--anonymous")
        else:
            user_file, password_file = credential_files(role, password_name)
            args.extend(["--user-file", user_file, "--password-file", password_file])
        if forbidden:
            args.append("--send-forbidden")
        if session_id:
            args.extend(["--session-id", session_id, "--session-timeout", "60"])
        if close_session:
            args.append("--close-session")
        if ipc:
            args.extend(["--ipc-out", container_path(ipc_path), "--row-cap", "100"])
        proc = run(["docker", "exec", "-w", "/src", RUST, "/target/driver/debug/ch-reader", *args], timeout=timeout_ms // 1000 + 20)
        record = {
            "query_id": query_id,
            "exit": proc.returncode,
            "stderr": proc.stderr[-800:],
            "status": None,
            "exception_code": None,
            "elapsed_ms": None,
            "error": None,
        }
        if out.exists():
            payload = json.loads(out.read_text())
            record.update(
                {
                    "status": payload.get("status"),
                    "exception_code": payload.get("exception_code"),
                    "elapsed_ms": payload.get("elapsed_ms"),
                    "error": payload.get("error"),
                    "ipc_written": payload.get("ipc_written"),
                }
            )
        return record

    def sql_file(self, name: str, settings: list[tuple[str, str]] | None = None) -> str:
        return apply_settings((TREE / "sql" / name).read_text(), settings)

    def arrow_settings(self, comment: str) -> list[tuple[str, str]]:
        # Readonly 1 cannot SET log_comment or Arrow format settings. Those live on the profile.
        _ = comment
        return []


def credential_files(role: str, password_name: str) -> tuple[str, str]:
    if password_name == "reader.wrong":
        return "/secrets/reader.user", "/secrets/reader.wrong"
    if password_name == "default":
        return "/secrets/default.user", "/secrets/default.password"
    return f"/secrets/{role}.user", f"/secrets/{role}.password"


def json_lines(path: Path) -> list[dict]:
    rows = []
    if not path.exists():
        return rows
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("Code:"):
            continue
        rows.append(json.loads(line))
    return rows


def main() -> int:
    attempt = next_attempt()
    results: list[dict] = []

    def record(name: str, passed: bool, detail: dict | str) -> None:
        results.append({"name": name, "passed": bool(passed), "detail": detail})
        write_text(attempt / "summary.json", json.dumps({"results": results}, indent=2) + "\n")

    client = Client(attempt)
    cancel_included = True
    try:
        if run(["docker", "exec", RUST, "test", "-x", "/target/driver/debug/ch-reader"]).returncode != 0:
            raise PhaseFailure("ch-reader is not built in the rust container")
        cancel_included = start_clickhouse(attempt, True)
        record("clickhouse-start", True, {"profile_cancel_setting": cancel_included})

        lockdown(client, record)
        load_fixture(client, record)
        before = checksum(client, "oo-dl01w1-40-checksum-before")
        record("checksum-before", before["status"] == 200, {"status": before["status"]})
        discover(client, record)
        catalog(client, record)
        read_arrow(client, record, "read-final.sql", "oo-dl01w1-40-arrow-final", "oo-dl01w1-40:op:arrow-final", "gold-final.json", "arrow-final")
        read_arrow(client, record, "read-nofinal.sql", "oo-dl01w1-40-arrow-nofinal", "oo-dl01w1-40:op:arrow-nofinal", "gold-nofinal.json", "arrow-nofinal")
        binary_probe(client, record)
        utf8_probe(client, record)
        retirees(client, record)
        cancel(client, record)
        deadline(client, record)
        capped(client, record, "rowcap.sql", "oo-dl01w1-40-rowcap", "oo-dl01w1-40:neg:rowcap", [("max_result_rows", "1")], "rowcap")
        capped(client, record, "bytecap.sql", "oo-dl01w1-40-bytecap", "oo-dl01w1-40:neg:bytecap", [("max_result_bytes", "1")], "bytecap")
        negatives(client, record)
        session(client, record)
        audit(client, record)
        after = checksum(client, "oo-dl01w1-40-checksum-after")
        same = before["status"] == 200 and after["status"] == 200 and (attempt / "http" / "oo-dl01w1-40-checksum-before.body").read_bytes() == (attempt / "http" / "oo-dl01w1-40-checksum-after.body").read_bytes()
        record("checksum-after", same, {"status": after["status"]})
        truncated(attempt, record)
        broken(client, record)
        grants(client, record)
        rollback(client, record)
    except PhaseFailure as error:
        record("fatal", False, str(error))
    finally:
        cleanup_clickhouse(attempt)
    required = [
        "lockdown-anon",
        "lockdown-default",
        "lockdown-wrong",
        "lockdown-reader",
        "fixture",
        "catalog",
        "arrow-final",
        "decode-final",
        "arrow-nofinal",
        "decode-nofinal",
        "binary",
        "utf8",
        "retirees",
        "cancel",
        "deadline",
        "rowcap",
        "bytecap",
        "neg-insert",
        "neg-kill",
        "neg-set",
        "audit",
        "checksum-after",
        "truncated",
        "broken",
        "session",
        "rollback",
    ]
    by_name = {item["name"]: item["passed"] for item in results}
    missing = [name for name in required if not by_name.get(name)]
    outcome = {"passed": not missing, "missing_or_failed": missing, "attempt": str(attempt)}
    write_text(attempt / "outcome.json", json.dumps(outcome, indent=2) + "\n")
    print(json.dumps(outcome, indent=2))
    return 0 if outcome["passed"] else 2


def lockdown(client: Client, record) -> None:
    anon = client.send("reader", "expect-fail", "oo-dl01w1-40-lockdown-anon", client.sql_file("lockdown-select.sql"), anonymous=True)
    default = client.send(
        "default",
        "expect-fail",
        "oo-dl01w1-40-lockdown-default",
        client.sql_file("lockdown-select.sql"),
        password_name="default",
    )
    wrong = client.send(
        "reader",
        "expect-fail",
        "oo-dl01w1-40-lockdown-wrong",
        client.sql_file("lockdown-select.sql"),
        password_name="reader.wrong",
    )
    ok = client.send("reader", "read", "oo-dl01w1-40-lockdown-reader", client.sql_file("lockdown-select.sql"))
    record("lockdown-anon", anon["status"] == 401 and anon["exception_code"] == 194, anon)
    record("lockdown-default", default["status"] not in (None, 0, 200), default)
    record("lockdown-wrong", wrong["status"] == 403 and wrong["exception_code"] == 516, wrong)
    record("lockdown-reader", ok["exit"] == 0 and ok["status"] == 200, ok)


def load_fixture(client: Client, record) -> None:
    parts = statements((TREE / "sql" / "00-fixture.sql").read_text())
    failed = []
    for index, statement in enumerate(parts, start=1):
        result = client.send("dba", "dba", f"oo-dl01w1-40-fixture-{index:02d}", statement, timeout_ms=60000)
        if result["status"] != 200 or result["exit"] != 0:
            failed.append(result)
            break
    record("fixture", not failed and len(parts) >= 8, {"statements": len(parts), "failed": failed[:1]})


def checksum(client: Client, query_id: str) -> dict:
    return client.send("dba", "dba", query_id, client.sql_file("checksum.sql"), timeout_ms=30000)


def discover(client: Client, record) -> None:
    result = client.send("reader", "read", "oo-dl01w1-40-settings", client.sql_file("settings.sql"))
    rows = json_lines(client.attempt / "http" / "oo-dl01w1-40-settings.body")
    client.known_settings = {str(row.get("name")) for row in rows}
    write_text(client.attempt / "settings.json", json.dumps(rows, indent=2) + "\n")
    record("settings", result["status"] == 200 and bool(client.known_settings), {"names": sorted(client.known_settings)})


def catalog(client: Client, record) -> None:
    columns = client.send("reader", "read", "oo-dl01w1-40-columns", client.sql_file("catalog-columns.sql"))
    table = client.send("reader", "read", "oo-dl01w1-40-table", client.sql_file("catalog-table.sql"))
    parts = client.send("reader", "read", "oo-dl01w1-40-parts", client.sql_file("catalog-parts.sql"))
    out = client.attempt / "catalog-verdict.json"
    proc = run(
        [
            "docker", "exec", "-w", "/src", RUST, "/target/driver/debug/ch-reader",
            "judge-catalog",
            "--columns", container_path(client.attempt / "http" / "oo-dl01w1-40-columns.body"),
            "--table", container_path(client.attempt / "http" / "oo-dl01w1-40-table.body"),
            "--parts", container_path(client.attempt / "http" / "oo-dl01w1-40-parts.body"),
            "--nofinal-gold", "/src/fixtures/gold-nofinal.json",
            "--out", container_path(out),
        ]
    )
    passed = proc.returncode == 0 and out.exists() and json.loads(out.read_text()).get("passed") is True
    record("catalog", columns["status"] == 200 and table["status"] == 200 and parts["status"] == 200 and passed, {"exit": proc.returncode, "stderr": proc.stderr[-400:]})


def read_arrow(client: Client, record, sql_name: str, query_id: str, comment: str, gold: str, phase: str) -> None:
    result = client.send(
        "reader",
        "arrow",
        query_id,
        client.sql_file(sql_name, client.arrow_settings(comment)),
        ipc=True,
        timeout_ms=20000,
    )
    ipc = client.attempt / "ipc" / f"{query_id}.arrow"
    record(phase, result["exit"] == 0 and result["status"] == 200 and ipc.exists(), result)
    if not ipc.exists():
        record(phase.replace("arrow", "decode"), False, "ipc was not written")
        return
    verdict = client.attempt / f"{phase}-decode.json"
    proc = run(
        [
            "docker", "exec", "-w", "/src", RUST, "/target/driver/debug/ch-decode",
            "compare", "--ipc", container_path(ipc), "--gold", f"/src/fixtures/{gold}",
            "--schema", "readings", "--out", container_path(verdict),
        ]
    )
    passed = proc.returncode == 0 and verdict.exists() and json.loads(verdict.read_text()).get("passed") is True
    record(phase.replace("arrow", "decode"), passed, {"exit": proc.returncode, "ipc_sha256": file_sha256(ipc), "stderr": proc.stderr[-400:]})


def binary_probe(client: Client, record) -> None:
    result = client.send("reader", "arrow", "oo-dl01w1-40-binary", client.sql_file("binary.sql"), ipc=True)
    ipc = client.attempt / "ipc" / "oo-dl01w1-40-binary.arrow"
    verdict = client.attempt / "binary-classify.json"
    passed = False
    if result["exit"] == 0 and ipc.exists():
        proc = run(
            [
                "docker", "exec", "-w", "/src", RUST, "/target/driver/debug/ch-decode",
                "classify-bytes", "--ipc", container_path(ipc), "--column", "raw", "--expect-hex", "00ff",
                "--out", container_path(verdict),
            ]
        )
        payload = json.loads(verdict.read_text()) if verdict.exists() else {}
        passed = proc.returncode == 0 and payload.get("classification") == "binary_preserved"
    record("binary", passed, result)


def utf8_probe(client: Client, record) -> None:
    settings = [("output_format_arrow_string_as_string", "1")]
    result = client.send("reader", "expect-fail", "oo-dl01w1-40-utf8", client.sql_file("utf8.sql", settings))
    body_path = client.attempt / "http" / "oo-dl01w1-40-utf8.body"
    body = body_path.read_text() if body_path.exists() else ""
    if result["exception_code"] == 164 and "output_format_arrow_string_as_string" in body:
        record("utf8", True, {"classification": "readonly_blocks_string_as_string", "body": body[:400], **result})
        return
    if result["status"] not in (None, 200) and result["exception_code"] not in (None, 164):
        record("utf8", True, {"classification": "server_rejected", "body": body[:400], **result})
        return
    # A 200 is only acceptable when the independent decoder preserves the bytes or rejects them.
    arrow = client.send("reader", "arrow", "oo-dl01w1-40-utf8-arrow", client.sql_file("utf8.sql", settings), ipc=True)
    ipc = client.attempt / "ipc" / "oo-dl01w1-40-utf8-arrow.arrow"
    verdict = client.attempt / "utf8-classify.json"
    passed = False
    payload: dict = {}
    if ipc.exists():
        proc = run(
            [
                "docker", "exec", "-w", "/src", RUST, "/target/driver/debug/ch-decode",
                "classify-bytes", "--ipc", container_path(ipc), "--column", "raw", "--expect-hex", "00ff",
                "--utf8-constraint", "--out", container_path(verdict),
            ]
        )
        payload = json.loads(verdict.read_text()) if verdict.exists() else {}
        passed = payload.get("passed") is True and payload.get("unsafe_substitution") is not True and proc.returncode == 0
    record("utf8", passed, {"http": result, "arrow": arrow, "classify": payload})


def retirees(client: Client, record) -> None:
    result = client.send(
        "reader",
        "arrow",
        "oo-dl01w1-40-retirees",
        client.sql_file("read-retirees.sql", client.arrow_settings("oo-dl01w1-40:op:retirees")),
        ipc=True,
    )
    mutations = client.send("reader", "read", "oo-dl01w1-40-mutations", client.sql_file("mutations.sql"))
    ipc = client.attempt / "ipc" / "oo-dl01w1-40-retirees.arrow"
    verdict = client.attempt / "retirees-verdict.json"
    passed = False
    if result["exit"] == 0 and ipc.exists() and mutations["status"] == 200:
        proc = run(
            [
                "docker", "exec", "-w", "/src", RUST, "/target/driver/debug/ch-decode",
                "judge-retirees", "--ipc", container_path(ipc),
                "--mutations", container_path(client.attempt / "http" / "oo-dl01w1-40-mutations.body"),
                "--out", container_path(verdict),
            ]
        )
        passed = proc.returncode == 0 and verdict.exists() and json.loads(verdict.read_text()).get("passed") is True
    record("retirees", passed, {"arrow": result, "mutations": mutations})


def cancel(client: Client, record) -> None:
    query_id = "oo-dl01w1-40-cancel"
    sql = client.sql_file("cancel.sql")
    sent = client.attempt / "sent" / f"{query_id}.sql"
    write_text(sent, sql)
    out = client.attempt / "http" / f"{query_id}.json"
    started = client.attempt / "cancel" / "started.flag"
    abort = client.attempt / "cancel" / "abort.flag"
    started.parent.mkdir(parents=True, exist_ok=True)
    started.unlink(missing_ok=True)
    abort.unlink(missing_ok=True)
    args = [
        "docker", "exec", "-w", "/src", RUST, "/target/driver/debug/ch-reader",
        "cancel", "--endpoint", ENDPOINT,
        "--user-file", "/secrets/reader.user", "--password-file", "/secrets/reader.password",
        "--query-id", query_id, "--body", container_path(sent), "--out", container_path(out),
        "--started-flag", container_path(started), "--abort-flag", container_path(abort),
        "--timeout-ms", "25000",
    ]
    proc = subprocess.Popen(args, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    seen = False
    seen_user = ""
    poll_number = 0
    deadline = time.monotonic() + 6
    while time.monotonic() < deadline and proc.poll() is None:
        poll_number += 1
        look = client.send(
            "dba",
            "dba",
            f"oo-dl01w1-40-proc-{poll_number}",
            "SELECT query_id, user FROM system.processes WHERE query_id = 'oo-dl01w1-40-cancel' FORMAT JSONEachRow",
            timeout_ms=5000,
        )
        body_path = client.attempt / "http" / f"{look['query_id']}.body"
        text = body_path.read_text() if body_path.exists() else ""
        if query_id in text and READER in text:
            seen = True
            seen_user = READER
            break
        time.sleep(0.15)
    kill = {}
    kill_body = ""
    observed = False
    if seen:
        kill = client.send(
            "reader",
            "expect-fail",
            "oo-dl01w1-40-cancel-kill",
            client.sql_file("cancel-kill.sql"),
            forbidden=True,
            timeout_ms=8000,
        )
        kill_body_path = client.attempt / "http" / "oo-dl01w1-40-cancel-kill.body"
        kill_body = kill_body_path.read_text() if kill_body_path.exists() else ""
        # HTTP 200 is the cancel only when the kill result names this query and user.
        observed = (
            kill.get("status") == 200
            and kill.get("exception_code") in (None, 0)
            and query_id in kill_body
            and READER in kill_body
            and "finished" in kill_body
        )
        if not observed:
            seen = False
    gone_deadline = time.monotonic() + 4
    gone = False
    while seen and time.monotonic() < gone_deadline:
        poll_number += 1
        look = client.send(
            "dba",
            "dba",
            f"oo-dl01w1-40-proc-{poll_number}",
            "SELECT query_id, user FROM system.processes WHERE query_id = 'oo-dl01w1-40-cancel' FORMAT JSONEachRow",
            timeout_ms=5000,
        )
        body_path = client.attempt / "http" / f"{look['query_id']}.body"
        text = body_path.read_text() if body_path.exists() else ""
        if query_id not in text:
            gone = True
            break
        time.sleep(0.15)
    try:
        stdout, stderr = proc.communicate(timeout=20)
    except subprocess.TimeoutExpired:
        proc.kill()
        stdout, stderr = proc.communicate()
    payload = json.loads(out.read_text()) if out.exists() else {}
    elapsed = payload.get("elapsed_ms") or 0
    passed = (
        seen
        and observed
        and gone
        and seen_user == READER
        and payload.get("status") != 200
        and payload.get("exception_code") == 394
        and 0 < elapsed < 5000
    )
    record(
        "cancel",
        passed,
        {
            "seen": seen,
            "observed_own_kill": observed,
            "gone_within_4s": gone,
            "user": seen_user,
            "kill": kill,
            "kill_body": kill_body[:400],
            "reader_exit": proc.returncode,
            "reader": payload,
            "stderr": (stderr or "")[-400:],
            "stdout": (stdout or "")[-200:],
        },
    )


def deadline(client: Client, record) -> None:
    result = client.send(
        "reader",
        "expect-fail",
        "oo-dl01w1-40-deadline",
        client.sql_file("deadline.sql", [("max_execution_time", "2")]),
        timeout_ms=12000,
    )
    elapsed = result.get("elapsed_ms") or 0
    passed = result["exit"] == 0 and result["exception_code"] == 159 and 1500 <= elapsed <= 7000
    record("deadline", passed, result)


def capped(client: Client, record, sql_name: str, query_id: str, comment: str, settings: list[tuple[str, str]], phase: str) -> None:
    result = client.send(
        "reader",
        "expect-fail",
        query_id,
        client.sql_file(sql_name, settings),
    )
    ipc = client.attempt / "ipc" / f"{query_id}.arrow"
    body_path = client.attempt / "http" / f"{query_id}.body"
    body = body_path.read_text() if body_path.exists() else ""
    passed = (
        result["exit"] == 0
        and result["status"] not in (None, 0, 200)
        and result["exception_code"] not in (None, 164)
        and not ipc.exists()
        and any(word in body.lower() for word in ("row", "byte", "limit", "overflow"))
    )
    record(phase, passed, {**result, "body": body[:400], "comment": comment})


def negatives(client: Client, record) -> None:
    insert = client.send("reader", "expect-fail", "oo-dl01w1-40-neg-insert", client.sql_file("neg-insert.sql"), forbidden=True)
    widen = client.send("reader", "expect-fail", "oo-dl01w1-40-neg-set", client.sql_file("neg-set.sql"), forbidden=True)
    foreign = foreign_kill(client)
    record("neg-insert", insert["exit"] == 0 and insert["status"] == 403 and insert["exception_code"] == 497, insert)
    record("neg-kill", foreign.get("denied") is True, foreign)
    record("neg-set", widen["exit"] == 0 and widen["exception_code"] == 164, widen)


def foreign_kill(client: Client) -> dict:
    """The reader must not cancel a live query owned by the DBA."""
    query_id = "oo-dl01w1-40-dba-hold"
    sent = client.attempt / "sent" / f"{query_id}.sql"
    write_text(sent, "SELECT sleep(3) AS held\nSETTINGS max_execution_time = 8\nFORMAT JSONEachRow\n")
    out = client.attempt / "http" / f"{query_id}.json"
    args = [
        "docker", "exec", "-w", "/src", RUST, "/target/driver/debug/ch-reader",
        "dba", "--endpoint", ENDPOINT,
        "--user-file", "/secrets/dba.user", "--password-file", "/secrets/dba.password",
        "--query-id", query_id, "--body", container_path(sent), "--out", container_path(out),
        "--timeout-ms", "15000",
    ]
    proc = subprocess.Popen(args, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    seen = False
    deadline = time.monotonic() + 6
    poll_number = 0
    while time.monotonic() < deadline and proc.poll() is None:
        poll_number += 1
        look = client.send(
            "dba",
            "dba",
            f"oo-dl01w1-40-hold-{poll_number}",
            f"SELECT query_id, user FROM system.processes WHERE query_id = '{query_id}' FORMAT JSONEachRow",
            timeout_ms=5000,
        )
        body_path = client.attempt / "http" / f"{look['query_id']}.body"
        text = body_path.read_text() if body_path.exists() else ""
        if query_id in text and DBA in text:
            seen = True
            break
        time.sleep(0.15)
    kill = {}
    still_there = False
    if seen:
        kill = client.send(
            "reader",
            "expect-fail",
            "oo-dl01w1-40-neg-kill",
            client.sql_file("neg-kill.sql"),
            forbidden=True,
            timeout_ms=8000,
        )
        poll_number += 1
        look = client.send(
            "dba",
            "dba",
            f"oo-dl01w1-40-hold-{poll_number}",
            f"SELECT query_id, user FROM system.processes WHERE query_id = '{query_id}' FORMAT JSONEachRow",
            timeout_ms=5000,
        )
        body_path = client.attempt / "http" / f"{look['query_id']}.body"
        text = body_path.read_text() if body_path.exists() else ""
        still_there = query_id in text and DBA in text
    try:
        proc.communicate(timeout=20)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.communicate()
    denied = seen and still_there and kill.get("status") == 403 and kill.get("exception_code") == 497
    return {"denied": denied, "seen": seen, "still_there": still_there, "kill": kill}


def session(client: Client, record) -> None:
    session_id = "oo-dl01w1-40-session"
    first = client.send(
        "reader",
        "read",
        "oo-dl01w1-40-handshake",
        client.sql_file("handshake.sql"),
        session_id=session_id,
    )
    second = client.send(
        "reader",
        "read",
        "oo-dl01w1-40-session-ping",
        client.sql_file("session-ping.sql"),
        session_id=session_id,
    )
    closed = client.send(
        "reader",
        "read",
        "oo-dl01w1-40-session-close",
        client.sql_file("session-ping.sql"),
        session_id=session_id,
        close_session=True,
    )
    log = client.send("reader", "read", "oo-dl01w1-40-session-log", client.sql_file("session-log.sql"))
    record(
        "session",
        first["status"] == 200 and second["status"] == 200 and closed["status"] == 200,
        {"handshake": first, "ping": second, "close": closed, "session_log": log},
    )


def audit(client: Client, record) -> None:
    flush = client.send("dba", "dba", "oo-dl01w1-40-flush", client.sql_file("flush.sql"), timeout_ms=30000)
    reader_log = client.send("reader", "read", "oo-dl01w1-40-audit-reader", client.sql_file("audit-reader.sql"))
    dba_log = client.send("dba", "dba", "oo-dl01w1-40-audit-dba", client.sql_file("audit-dba.sql"))
    expect = {
        "reader_user": READER,
        "cancel_query_id": "oo-dl01w1-40-cancel",
        "deadline_query_id": "oo-dl01w1-40-deadline",
        "cancel_max_ms": 5000,
        "deadline_min_ms": 1500,
        "deadline_max_ms": 4500,
        "reader_kill_denied_min": 1,
        "own_cancel_kill_query_id": "oo-dl01w1-40-cancel-kill",
    }
    expect_path = client.attempt / "audit-expect.json"
    write_text(expect_path, json.dumps(expect, indent=2) + "\n")
    out = client.attempt / "audit-verdict.json"
    proc = run(
        [
            "docker", "exec", "-w", "/src", RUST, "/target/driver/debug/ch-reader",
            "judge-audit",
            "--reader-log", container_path(client.attempt / "http" / "oo-dl01w1-40-audit-reader.body"),
            "--dba-log", container_path(client.attempt / "http" / "oo-dl01w1-40-audit-dba.body"),
            "--expect", container_path(expect_path),
            "--out", container_path(out),
        ]
    )
    passed = proc.returncode == 0 and out.exists() and json.loads(out.read_text()).get("passed") is True
    record("audit", flush["status"] == 200 and reader_log["status"] == 200 and dba_log["status"] == 200 and passed, {"flush": flush, "exit": proc.returncode, "stderr": proc.stderr[-400:]})


def truncated(attempt: Path, record) -> None:
    source = attempt / "ipc" / "oo-dl01w1-40-arrow-final.arrow"
    if not source.exists():
        record("truncated", False, "no final ipc to truncate")
        return
    raw = source.read_bytes()
    broken = attempt / "ipc" / "truncated.arrow"
    broken.write_bytes(raw[:-32] if len(raw) > 40 else raw[:4])
    verdict = attempt / "truncated-decode.json"
    proc = run(
        [
            "docker", "exec", "-w", "/src", RUST, "/target/driver/debug/ch-decode",
            "compare", "--ipc", container_path(broken), "--gold", "/src/fixtures/gold-final.json",
            "--schema", "readings", "--out", container_path(verdict),
        ]
    )
    payload = json.loads(verdict.read_text()) if verdict.exists() else {}
    record("truncated", proc.returncode != 0 and payload.get("passed") is False, payload)


def broken(client: Client, record) -> None:
    sent = client.attempt / "sent" / "oo-dl01w1-40-broken.sql"
    write_text(sent, "SELECT 1 AS ok\nFORMAT JSONEachRow\n")
    out = client.attempt / "http" / "oo-dl01w1-40-broken.json"
    proc = run(
        [
            "docker", "exec", "-w", "/src", RUST, "/target/driver/debug/ch-reader",
            "expect-fail", "--endpoint", "http://127.0.0.1:9",
            "--user-file", "/secrets/reader.user", "--password-file", "/secrets/reader.password",
            "--query-id", "oo-dl01w1-40-broken", "--body", container_path(sent),
            "--out", container_path(out), "--timeout-ms", "4000",
        ],
        timeout=20,
    )
    payload = json.loads(out.read_text()) if out.exists() else {}
    record("broken", proc.returncode == 3 and payload.get("status") == 0 and "connection_error" in str(payload.get("error")), payload)


def grants(client: Client, record) -> None:
    result = client.send("reader", "read", "oo-dl01w1-40-grants", client.sql_file("grants.sql"))
    record("grants", result["status"] == 200, result)


def rollback(client: Client, record) -> None:
    result = client.send("dba", "dba", "oo-dl01w1-40-rollback", client.sql_file("rollback.sql"), timeout_ms=30000)
    record("rollback", result["status"] == 200 and result["exit"] == 0, result)


def cleanup_clickhouse(attempt: Path) -> None:
    proof: dict = {}
    if owned(CLICKHOUSE):
        save_logs(attempt, "docker-logs")
        inspected = run(["docker", "inspect", CLICKHOUSE])
        write_text(attempt / "clickhouse-inspect.json", inspected.stdout)
        removed = run(["docker", "rm", "-f", CLICKHOUSE], timeout=60)
        proof["container_rm_exit"] = removed.returncode
    inspected = run(["docker", "volume", "inspect", "-f", "{{index .Labels \"oo.owner\"}}", VOLUME])
    if inspected.stdout.strip() == OWNER:
        volume = run(["docker", "volume", "rm", "-f", VOLUME])
        proof["volume_rm_exit"] = volume.returncode
    else:
        proof["volume_rm_exit"] = None
    image = run(["docker", "image", "inspect", IMAGE, "--format", "{{.Id}} {{.Os}}/{{.Architecture}}"])
    proof["preserved_image"] = image.stdout.strip()
    proof["preserved_image_exit"] = image.returncode
    released = run(["python3", str(TREE / "scripts" / "admit.py"), "release", "clickhouse"])
    proof["release_exit"] = released.returncode
    write_text(attempt / "cleanup.json", json.dumps(proof, indent=2) + "\n")


if __name__ == "__main__":
    raise SystemExit(main())
