#!/usr/bin/env python3
"""Native StarRocks 3.5.21 reader gate for DL01-W1-45.

The probe and decode binaries run inside the StarRocks container. This process
does not start a Rust compiler. Passwords stay in mode 600 files and are
redacted from evidence.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

REPO = Path("/Users/redux/puppy-one/open-ontology")
TASK = "DL01-W1-45"
SESSION = "2c023948-0862-4dae-ae8f-6fc30241db62"
PREFIX = "oo-dl01w1-45"
IMAGE = "starrocks/allin1-ubuntu@sha256:9b98696ee0cb46831eabe197ae5e71f483ee3c2e16ab57fd27cb51a5c9ee6618"
PIN = "sha256:9b98696ee0cb46831eabe197ae5e71f483ee3c2e16ab57fd27cb51a5c9ee6618"
CONTAINER = f"{PREFIX}-starrocks"
NETWORK = PREFIX
# Default Docker address pools on this host are fully subnetted. An explicit
# free /24 avoids that allocator. 10.45.45.0/24 does not overlap the host LAN
# (192.168.0.0/23) or the occupied bridge ranges recorded on 2026-10-10.
SUBNET = "10.45.45.0/24"
GATEWAY = "10.45.45.1"
SRC = REPO / "testing" / "dl01-driver-preflight" / "starrocks"
EVIDENCE = REPO / "evidence" / SESSION / "DL01" / "starrocks-driver-preflight" / "native"
SECRET_DIR = Path("/tmp/oo-dl01w1-45-secrets")
ADMIT = SRC / "scripts" / "admit.py"
ORACLE = SRC / "scripts" / "oracle.py"

SECRETS: list[str] = []
CHECKS: list[dict] = []


def now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def run(cmd: list[str], timeout: int = 180, stdin: bytes | None = None) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(cmd, input=stdin, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout, check=False)


def redact(data: bytes | str) -> str:
    text = data.decode("utf-8", "replace") if isinstance(data, bytes) else data
    gate_dir = REPO / "tools" / "starrocks-preview" / "scripts"
    if str(gate_dir) not in sys.path:
        sys.path.insert(0, str(gate_dir))
    import closure_gate
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
    path.write_text(redact(data), encoding="utf-8")


def add_check(name: str, ok: bool, expected: object, actual: object) -> None:
    CHECKS.append({"name": name, "ok": bool(ok), "expected": expected, "actual": actual, "at": now()})


def phase(name: str, proc: subprocess.CompletedProcess[bytes]) -> int:
    save(EVIDENCE / "phases" / f"{name}.stdout", proc.stdout)
    save(EVIDENCE / "phases" / f"{name}.stderr", proc.stderr)
    (EVIDENCE / "phases" / f"{name}.exit").write_text(f"{proc.returncode}\n", encoding="utf-8")
    return proc.returncode


def mysql(sql: str, cnf: str | None = None, timeout: int = 180) -> subprocess.CompletedProcess[bytes]:
    cmd = ["docker", "exec", "-i", CONTAINER, "mysql"]
    if cnf:
        cmd.append(f"--defaults-file={cnf}")
    else:
        cmd.extend(["--no-defaults", "-uroot", "-h127.0.0.1", "-P9030", "--protocol=TCP", "--skip-password"])
    cmd.extend(["--batch", "--raw", "--skip-column-names", "--connect-timeout=10", "--default-character-set=utf8mb4"])
    return run(cmd, timeout=timeout, stdin=sql.encode("utf-8"))


def write_secret(name: str, text: str, mode: int) -> Path:
    SECRET_DIR.mkdir(mode=0o700, exist_ok=True)
    os.chmod(SECRET_DIR, 0o700)
    path = SECRET_DIR / name
    path.write_text(text, encoding="utf-8")
    os.chmod(path, mode)
    return path


def write_cnf(name: str, user: str, password: str) -> Path:
    return write_secret(
        f"{name}.cnf",
        "[client]\n"
        f"user={user}\n"
        f"password={password}\n"
        "host=127.0.0.1\n"
        "port=9030\n"
        "protocol=TCP\n"
        "default-character-set=utf8mb4\n",
        0o600,
    )


def install(local: Path, remote: str, mode: str) -> None:
    copied = run(["docker", "cp", str(local), f"{CONTAINER}:{remote}"])
    if copied.returncode != 0:
        raise RuntimeError(redact(copied.stderr))
    run(["docker", "exec", CONTAINER, "chmod", mode, remote])


def start_container() -> bool:
    own = run(["docker", "ps", "-aq", "--filter", f"name=^{CONTAINER}$"])
    if own.stdout.strip():
        label = run(["docker", "inspect", "-f", "{{index .Config.Labels \"oo.prefix\"}}", CONTAINER])
        if label.stdout.decode().strip() == PREFIX:
            run(["docker", "rm", "-f", CONTAINER], timeout=60)
        else:
            add_check("container_name", False, PREFIX, label.stdout.decode())
            return False
    net = run(["docker", "network", "inspect", NETWORK])
    if net.returncode != 0:
        created = run(
            [
                "docker", "network", "create",
                "--driver", "bridge",
                "--subnet", SUBNET,
                "--gateway", GATEWAY,
                "--label", "project=open-ontology",
                "--label", f"owner={SESSION}",
                "--label", f"session={SESSION}",
                "--label", "kind=starrocks-source",
                "--label", f"oo.prefix={PREFIX}",
                "--label", f"oo.task={TASK}",
                NETWORK,
            ]
        )
        phase("network", created)
        if created.returncode != 0:
            add_check("network", False, SUBNET, redact(created.stderr)[:500])
            return False
        add_check("network", True, SUBNET, "created")
    else:
        phase("network", net)
        if SUBNET.encode() not in net.stdout:
            add_check("network", False, SUBNET, "present network has a different subnet")
            return False
        add_check("network", True, SUBNET, "present")
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
        "--label", "oo.product=starrocks",
        IMAGE,
    ]
    admitted = run([sys.executable, str(ADMIT), "acquire-start", "starrocks", str(EVIDENCE / "admission.json"), "--", *command], timeout=120)
    phase("admission", admitted)
    return admitted.returncode == 0


def wait_ready() -> bool:
    deadline = time.time() + 180
    last = b""
    while time.time() < deadline:
        state = run(["docker", "inspect", "-f", "{{.State.Status}}", CONTAINER])
        if state.stdout.decode().strip() not in {"running", "created"}:
            add_check("server_ready", False, "running", state.stdout.decode())
            return False
        probe = mysql("SELECT 1;")
        last = probe.stderr
        if probe.returncode == 0:
            backend = mysql("SHOW BACKENDS;")
            if b"true" in backend.stdout.lower():
                save(EVIDENCE / "phases" / "ready.stdout", probe.stdout + b"\n" + backend.stdout)
                add_check("server_ready", True, "FE and BE", "alive")
                return True
        time.sleep(3)
    save(EVIDENCE / "phases" / "ready.stderr", last)
    add_check("server_ready", False, "ready within 180s", "timeout")
    return False


def identity() -> bool:
    sql = "SELECT current_version();\nSELECT @@version_comment;\nSELECT @@version;\n"
    proc = mysql(sql)
    phase("identity", proc)
    lines = [line for line in proc.stdout.decode("utf-8", "replace").splitlines() if line]
    ok = len(lines) >= 3 and lines[0] == "3.5.21-85fccf8" and lines[1] == "3.5.21-85fccf8" and lines[2] == "8.0.33"
    add_check("engine_identity", ok, ["3.5.21-85fccf8", "3.5.21-85fccf8", "8.0.33"], lines[:3])
    # RepoDigests is an image field. A container inspect returns an empty list.
    image_id = run(["docker", "inspect", "-f", "{{.Image}}", CONTAINER])
    ident = image_id.stdout.decode().strip()
    repo = run(["docker", "image", "inspect", "-f", "{{json .RepoDigests}}", ident or IMAGE])
    text = ident + "\n" + repo.stdout.decode().strip()
    save(EVIDENCE / "phases" / "image-digest.stdout", text)
    (EVIDENCE / "phases" / "image-digest.exit").write_text(
        f"{image_id.returncode}:{repo.returncode}\n", encoding="utf-8"
    )
    matched = ident == PIN and PIN in repo.stdout.decode()
    add_check("image_digest", matched, PIN, text)
    return ok and matched


def try_grant(statements: list[str]) -> bool:
    for statement in statements:
        proc = mysql(statement)
        if proc.returncode == 0:
            return True
    return False


def create_accounts() -> dict[str, str]:
    dba_password = os.urandom(18).hex()
    reader_password = os.urandom(18).hex()
    SECRETS.extend([dba_password, reader_password])
    mysql("CREATE DATABASE IF NOT EXISTS oo_w45_sr;")
    created = mysql(
        "CREATE USER 'oo_w45_dba'@'%' IDENTIFIED BY '{dba}';\n"
        "CREATE USER 'oo_w45_reader'@'%' IDENTIFIED BY '{reader}';\n".format(
            dba=dba_password, reader=reader_password
        )
    )
    phase("create-users", created)
    reader_ok = try_grant(
        [
            "GRANT SELECT ON ALL TABLES IN DATABASE oo_w45_sr TO USER 'oo_w45_reader'@'%';",
            "GRANT SELECT ON oo_w45_sr.* TO 'oo_w45_reader'@'%';",
        ]
    )
    dba_ok = try_grant(
        [
            "GRANT SELECT, INSERT, UPDATE, DELETE, ALTER, DROP ON ALL TABLES IN DATABASE oo_w45_sr TO USER 'oo_w45_dba'@'%';",
            "GRANT SELECT, INSERT, UPDATE, DELETE, ALTER, DROP ON oo_w45_sr.* TO 'oo_w45_dba'@'%';",
        ]
    )
    create_ok = try_grant(
        [
            "GRANT CREATE TABLE ON DATABASE oo_w45_sr TO USER 'oo_w45_dba'@'%';",
            "GRANT CREATE TABLE ON oo_w45_sr TO 'oo_w45_dba'@'%';",
        ]
    )
    grants = mysql("SHOW GRANTS FOR 'oo_w45_reader'@'%';\nSHOW GRANTS FOR 'oo_w45_dba'@'%';\n")
    phase("show-grants", grants)
    text = grants.stdout.decode("utf-8", "replace").upper()
    reader_lines = [line for line in text.splitlines() if "OO_W45_READER" in line]
    reader_select = any("SELECT" in line for line in reader_lines)
    reader_write = any(word in line for line in reader_lines for word in ("INSERT", "UPDATE", "DELETE", "CREATE", "DROP", "ALTER", "OPERATE", "ADMIN"))
    add_check("reader_select_grant", reader_ok and reader_select and not reader_write, "SELECT only", reader_select and not reader_write)
    add_check("dba_grant", dba_ok and create_ok, "DBA can load", dba_ok and create_ok)
    write_secret("reader.password", reader_password, 0o600)
    dba_cnf = write_cnf("dba", "oo_w45_dba", dba_password)
    reader_cnf = write_cnf("reader", "oo_w45_reader", reader_password)
    run(["docker", "exec", CONTAINER, "mkdir", "-p", "/tmp/oo-secrets"])
    install(SECRET_DIR / "reader.password", "/tmp/oo-secrets/reader.password", "600")
    install(dba_cnf, "/tmp/oo-secrets/dba.cnf", "600")
    install(reader_cnf, "/tmp/oo-secrets/reader.cnf", "600")
    return {"dba": "/tmp/oo-secrets/dba.cnf", "reader": "/tmp/oo-secrets/reader.cnf"}


def sql_statements(text: str) -> list[str]:
    statements = []
    for chunk in text.split(";"):
        lines = [line for line in chunk.splitlines() if line.strip() and not line.strip().startswith("--")]
        body = "\n".join(lines).strip()
        if body:
            statements.append(body)
    return statements


def load_fixture(dba_cnf: str) -> bool:
    statements = sql_statements((SRC / "sql" / "dba-load.sql").read_text(encoding="utf-8"))
    sketch_failed = False
    for index, statement in enumerate(statements):
        if statement.startswith("--"):
            continue
        # Each mysql invocation is a new session. A leading USE does not
        # survive the statement split, and StarRocks then reports 1046.
        bound = statement if statement.upper().startswith("USE ") else f"USE oo_w45_sr;\n{statement}"
        proc = mysql(bound + ";", dba_cnf, timeout=180)
        phase(f"load-{index:02d}", proc)
        if proc.returncode != 0 and "sketch_hold" in statement:
            sketch_failed = True
            add_check("sketch_ddl", False, "recorded if the server rejects BITMAP/HLL", proc.stderr.decode()[:400])
            continue
        if proc.returncode != 0:
            add_check("fixture_load", False, "load", f"statement {index} exit {proc.returncode}")
            return False
    if not sketch_failed:
        add_check("sketch_ddl", True, "BITMAP/HLL table created", "created")
    present = mysql("SHOW TABLES FROM oo_w45_sr;", dba_cnf)
    names = present.stdout.decode()
    ok = all(table in names for table in ("dup_events", "pk_accounts", "agg_metrics", "uniq_dim"))
    add_check("fixture_tables", ok, "four typed tables", names.strip())
    return ok


def run_probe(binary_remote: str, args: list[str], out_name: str, timeout: int = 180) -> int:
    remote = f"/tmp/oo-w45-out/{out_name}"
    run(["docker", "exec", CONTAINER, "rm", "-rf", remote])
    run(["docker", "exec", CONTAINER, "mkdir", "-p", remote])
    proc = run(
        ["docker", "exec", CONTAINER, binary_remote, *args, "--out", remote],
        timeout=timeout,
    )
    code = phase(out_name, proc)
    host = EVIDENCE / out_name
    host.mkdir(parents=True, exist_ok=True)
    copied = run(["docker", "cp", f"{CONTAINER}:{remote}/.", str(host)], timeout=60)
    if copied.returncode != 0:
        phase(f"{out_name}-copy", copied)
    return code


def native_dump(reader_cnf: str, dest: Path) -> None:
    queries = {
        "dup_events": "SELECT event_id, bucket, seen, amount, note FROM dup_events ORDER BY event_id, bucket, IFNULL(note, ''), IFNULL(CAST(amount AS STRING), '')",
        "pk_accounts": "SELECT account_id, balance, nickname, opened FROM pk_accounts ORDER BY account_id",
        "agg_metrics": "SELECT metric_day, metric_name, qty, revenue FROM agg_metrics ORDER BY metric_day, metric_name",
        "uniq_dim": "SELECT dim_id, label, score FROM uniq_dim ORDER BY dim_id",
        "dup_partition_sums": "SELECT CASE WHEN bucket < '2026-06-01' THEN 'p202605' ELSE 'p202606' END, COUNT(*), CAST(SUM(amount) AS DECIMAL(38, 4)) FROM dup_events GROUP BY CASE WHEN bucket < '2026-06-01' THEN 'p202605' ELSE 'p202606' END ORDER BY 1",
    }
    dest.mkdir(parents=True, exist_ok=True)
    for name, sql in queries.items():
        proc = mysql("USE oo_w45_sr;\n" + sql + ";", reader_cnf)
        phase(f"native-{dest.name}-{name}", proc)
        (dest / f"{name}.tsv").write_text(proc.stdout.decode("utf-8", "replace"), encoding="utf-8")


def decode(binary_remote: str, arrow_dir: Path, decoded_dir: Path) -> int:
    remote_arrow = "/tmp/oo-w45-arrow"
    remote_out = "/tmp/oo-w45-decoded"
    run(["docker", "exec", CONTAINER, "rm", "-rf", remote_arrow, remote_out])
    run(["docker", "exec", CONTAINER, "mkdir", "-p", remote_arrow, remote_out])
    for path in sorted(arrow_dir.glob("*.arrow")):
        install(path, f"{remote_arrow}/{path.name}", "644")
    listing = run(["docker", "exec", CONTAINER, "sh", "-c", f"ls {remote_arrow}/*.arrow"])
    inputs = listing.stdout.decode().split()
    proc = run(["docker", "exec", CONTAINER, binary_remote, "--out", remote_out, *inputs], timeout=120)
    code = phase(f"decode-{decoded_dir.name}", proc)
    decoded_dir.mkdir(parents=True, exist_ok=True)
    run(["docker", "cp", f"{CONTAINER}:{remote_out}/.", str(decoded_dir)])
    return code


def run_oracle(decoded: Path, native: Path, phase_name: str) -> bool:
    proc = run(
        [sys.executable, str(ORACLE), "compare", str(SRC / "fixtures" / "expected.json"), str(decoded), str(native), phase_name],
        timeout=60,
    )
    save(EVIDENCE / f"oracle-{phase_name}.json", proc.stdout or proc.stderr)
    (EVIDENCE / f"oracle-{phase_name}.exit").write_text(f"{proc.returncode}\n", encoding="utf-8")
    add_check(f"oracle_{phase_name}", proc.returncode == 0, "typed match", proc.returncode)
    return proc.returncode == 0


def audit_file(path: Path, label: str, need_cancel: bool) -> bool:
    proc = run([sys.executable, "-c", "import json,sys; from pathlib import Path; import importlib.util; spec=importlib.util.spec_from_file_location('oracle', sys.argv[1]); mod=importlib.util.module_from_spec(spec); spec.loader.exec_module(mod); json.dump(mod.audit_counts(Path(sys.argv[2])), sys.stdout)", str(ORACLE), str(path)])
    save(EVIDENCE / f"audit-{label}.json", proc.stdout or proc.stderr)
    try:
        counts = json.loads(proc.stdout.decode() or "{}")
    except json.JSONDecodeError:
        counts = {}
    ok = bool(counts.get("ok")) and (not need_cancel or counts.get("cancel", 0) >= 1)
    add_check(f"audit_{label}", ok, "write 0 and classes complete", counts)
    return ok


def fe_audit() -> None:
    remote = ""
    for candidate in ("/data/deploy/starrocks/fe/log/fe.audit.log", "/opt/starrocks/fe/log/fe.audit.log"):
        probe = run(["docker", "exec", CONTAINER, "sh", "-c", f"test -f {candidate} && echo yes"])
        if probe.stdout.decode().strip() == "yes":
            remote = candidate
            break
    if not remote:
        add_check("fe_audit", False, "fe.audit.log", "absent")
        return
    proc = run(["docker", "exec", CONTAINER, "sh", "-c", f"grep -F oo_w45_reader {remote} || true"], timeout=60)
    save(EVIDENCE / "fe-audit-reader.log", proc.stdout)
    text = (EVIDENCE / "fe-audit-reader.log").read_text(encoding="utf-8")
    add_check("fe_audit_reader_lines", "oo_w45_reader" in text or text.strip() != "", "reader lines or an empty extract", f"bytes {len(text)}")


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
    add_check("secret_scan", not hits, "no secret bytes", hits)
    return not hits


def release() -> None:
    run(["docker", "rm", "-f", CONTAINER], timeout=60)
    run([sys.executable, str(ADMIT), "release", "starrocks"])
    if SECRET_DIR.exists():
        for path in SECRET_DIR.iterdir():
            path.unlink(missing_ok=True)
        SECRET_DIR.rmdir()


def main() -> int:
    if len(sys.argv) != 5 or sys.argv[1] != "--probe" or sys.argv[3] != "--decode":
        print("usage: run_gate.py --probe BINARY --decode BINARY", file=sys.stderr)
        return 2
    probe = Path(sys.argv[2])
    decode_bin = Path(sys.argv[4])
    if not probe.is_file() or not decode_bin.is_file():
        print("binary missing", file=sys.stderr)
        return 2
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    pending = False
    try:
        if not start_container() or not wait_ready() or not identity():
            return 1
        accounts = create_accounts()
        if not load_fixture(accounts["dba"]):
            return 1
        install(probe, "/tmp/starrocks-probe", "755")
        install(decode_bin, "/tmp/starrocks-decode", "755")
        linked = run(["docker", "exec", CONTAINER, "/tmp/starrocks-probe", "--help"])
        if linked.returncode not in {0, 1} or b"GLIBC" in linked.stderr or b"No such file" in linked.stderr:
            phase("glibc", linked)
            add_check("probe_dynamic_link", False, "dynamic binary runs", "glibc")
            return 1
        read_code = run_probe(
            "/tmp/starrocks-probe",
            ["read", "--host", "127.0.0.1", "--port", "9030", "--password-file", "/tmp/oo-secrets/reader.password", "--image-digest", PIN],
            "read",
        )
        add_check("probe_read", read_code == 0, 0, read_code)
        native = EVIDENCE / "native-initial"
        native_dump(accounts["reader"], native)
        decoded = EVIDENCE / "decoded-initial"
        decode("/tmp/starrocks-decode", EVIDENCE / "read", decoded)
        oracle_ok = run_oracle(decoded, native, "initial")
        block_code = run_probe(
            "/tmp/starrocks-probe",
            ["block", "--host", "127.0.0.1", "--port", "9030", "--password-file", "/tmp/oo-secrets/reader.password", "--image-digest", PIN],
            "block",
        )
        add_check("probe_block", block_code == 0, 0, block_code)
        row_code = run_probe(
            "/tmp/starrocks-probe",
            ["caps", "row", "--max-rows", "1", "--host", "127.0.0.1", "--port", "9030", "--password-file", "/tmp/oo-secrets/reader.password", "--image-digest", PIN],
            "caps-row",
        )
        byte_code = run_probe(
            "/tmp/starrocks-probe",
            ["caps", "byte", "--max-bytes", "1", "--host", "127.0.0.1", "--port", "9030", "--password-file", "/tmp/oo-secrets/reader.password", "--image-digest", PIN],
            "caps-byte",
        )
        add_check("caps", row_code == 0 and byte_code == 0, "row and byte caps", {"row": row_code, "byte": byte_code})
        deadline_code = run_probe(
            "/tmp/starrocks-probe",
            ["deadline", "--host", "127.0.0.1", "--port", "9030", "--password-file", "/tmp/oo-secrets/reader.password", "--image-digest", PIN],
            "deadline",
            timeout=90,
        )
        add_check("deadline", deadline_code == 0, 0, deadline_code)
        cancel_code = run_probe(
            "/tmp/starrocks-probe",
            ["cancel", "--host", "127.0.0.1", "--port", "9030", "--password-file", "/tmp/oo-secrets/reader.password", "--image-digest", PIN],
            "cancel",
            timeout=120,
        )
        if cancel_code == 3:
            pending = True
        add_check("cancel", cancel_code == 0, 0, cancel_code)
        mutate = mysql((SRC / "sql" / "dba-mutate.sql").read_text(encoding="utf-8"), accounts["dba"])
        phase("mutate", mutate)
        add_check("mutate", mutate.returncode == 0, 0, mutate.returncode)
        after_code = run_probe(
            "/tmp/starrocks-probe",
            ["read", "--host", "127.0.0.1", "--port", "9030", "--password-file", "/tmp/oo-secrets/reader.password", "--image-digest", PIN],
            "read-after",
        )
        native_after = EVIDENCE / "native-after"
        native_dump(accounts["reader"], native_after)
        decoded_after = EVIDENCE / "decoded-after"
        decode("/tmp/starrocks-decode", EVIDENCE / "read-after", decoded_after)
        after_ok = run_oracle(decoded_after, native_after, "after")
        audits = []
        for label, need_cancel in (("read", False), ("deadline", False), ("cancel", True), ("read-after", False)):
            path = EVIDENCE / label / "audit.jsonl"
            if path.exists():
                audits.append(audit_file(path, label, need_cancel))
            else:
                add_check(f"audit_{label}", False, "audit.jsonl", "absent")
                audits.append(False)
        fe_audit()
        clean = secret_scan()
        failed = [
            item["name"] for item in CHECKS if not item["ok"]
        ]
        save(EVIDENCE / "checks.json", json.dumps({"at": now(), "checks": CHECKS, "failed": failed}, indent=2))
        if failed or not oracle_ok or not after_ok or not clean or after_code != 0 or read_code != 0:
            return 3 if pending and not any(name != "cancel" for name in failed) else 1
        return 0
    except subprocess.TimeoutExpired as exc:
        save(EVIDENCE / "phases" / "timeout.txt", str(exc))
        add_check("timeout", False, "command finished", "timeout")
        save(EVIDENCE / "checks.json", json.dumps({"at": now(), "checks": CHECKS}, indent=2))
        return 1
    finally:
        failed = [item["name"] for item in CHECKS if not item["ok"]]
        save(EVIDENCE / "checks.json", json.dumps({"at": now(), "checks": CHECKS, "failed": failed}, indent=2))
        release()


if __name__ == "__main__":
    sys.exit(main())
