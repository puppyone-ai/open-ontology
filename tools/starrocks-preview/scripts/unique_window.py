#!/usr/bin/env python3
"""Finish the uniq_dim window with the fixture's legal DML.

StarRocks UNIQUE KEY tables in this fixture change a key by a later INSERT
of the same key. The sealed capture sent UPDATE and the server returned
1064. This run uses a new container on the existing network. It does not
start oo-dl01w1-45-starrocks, does not delete volumes, and does not send
DELETE or a second INSERT that hides a delete.
"""

from __future__ import annotations

import json
import os
import subprocess
import time
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
LOAD_SQL = ROOT / "testing/dl01-driver-preflight/starrocks/sql/dba-load.sql"
EVIDENCE = (
    ROOT
    / "evidence/2c023948-0862-4dae-ae8f-6fc30241db62/DL01/starrocks-product/raw/unique-window"
)
IMAGE = "starrocks/allin1-ubuntu@sha256:9b98696ee0cb46831eabe197ae5e71f483ee3c2e16ab57fd27cb51a5c9ee6618"
PIN = "sha256:9b98696ee0cb46831eabe197ae5e71f483ee3c2e16ab57fd27cb51a5c9ee6618"
CONTAINER = "oo-dl01w1-45-uniq"
PAUSED = "oo-dl01w1-45-starrocks"
NETWORK = "oo-dl01w1-45"
SECRET_DIR = Path("/tmp/oo-w45-uniq-secrets")
UPDATE_SQL = "USE oo_w45_sr;\nUPDATE uniq_dim SET label='third' WHERE dim_id=17;"
INSERT_SQL = "USE oo_w45_sr;\nINSERT INTO uniq_dim VALUES (17, 'third', CAST('4.40' AS DECIMAL(10, 2)));"
READ_SQL = "USE oo_w45_sr;\nSELECT dim_id, label, score FROM uniq_dim ORDER BY dim_id;"

SECRETS: list[str] = []


def now_iso() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def scrub(data: bytes) -> bytes:
    out = data
    for secret in SECRETS:
        if secret:
            out = out.replace(secret.encode(), b"<redacted>")
    return out


def save(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(scrub(data))


def run(cmd: list[str], timeout: int = 180, stdin: bytes | None = None) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(cmd, input=stdin, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout, check=False)


def phase(name: str, proc: subprocess.CompletedProcess[bytes]) -> int:
    folder = EVIDENCE / "phases"
    save(folder / f"{name}.stdout", proc.stdout)
    save(folder / f"{name}.stderr", proc.stderr)
    (folder / f"{name}.exit").write_text(f"{proc.returncode}\n", encoding="utf-8")
    return proc.returncode


def mysql(sql: str, cnf: str | None = None, timeout: int = 180) -> subprocess.CompletedProcess[bytes]:
    cmd = ["docker", "exec", "-i", CONTAINER, "mysql"]
    if cnf:
        cmd.append(f"--defaults-file={cnf}")
    else:
        cmd.extend(["--no-defaults", "-uroot", "-h127.0.0.1", "-P9030", "--protocol=TCP", "--skip-password"])
    cmd.extend(["--batch", "--raw", "--skip-column-names", "--connect-timeout=10", "--default-character-set=utf8mb4"])
    return run(cmd, timeout=timeout, stdin=sql.encode("utf-8"))


def write_secret(name: str, text: str) -> Path:
    SECRET_DIR.mkdir(parents=True, exist_ok=True)
    os.chmod(SECRET_DIR, 0o700)
    path = SECRET_DIR / name
    path.write_text(text, encoding="utf-8")
    os.chmod(path, 0o600)
    return path


def main() -> int:
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    (EVIDENCE / "phases").mkdir(parents=True, exist_ok=True)
    paused = run(["docker", "ps", "-aq", "--filter", f"name=^{PAUSED}$"])
    if paused.stdout.strip():
        (EVIDENCE / "phases" / "refused-start.txt").write_text(
            f"refused start. paused={paused.stdout.decode().strip()}\n", encoding="utf-8"
        )
        return 2
    own = run(["docker", "ps", "-aq", "--filter", f"name=^{CONTAINER}$"])
    resume = bool(own.stdout.strip())
    command = ["docker", "start", CONTAINER]
    if resume:
        state = run(["docker", "inspect", "-f", "{{.State.Status}}", CONTAINER])
        if state.stdout.decode().strip() != "running":
            started = run(command, timeout=60)
            phase("docker-start", started)
            if started.returncode != 0:
                return started.returncode
    else:
        command = [
            "docker", "run", "-d", "--name", CONTAINER,
            "--platform", "linux/arm64", "--network", NETWORK,
            "--memory", "4g", "--memory-swap", "4g",
            "--restart", "no", "--pull", "never",
            "--ulimit", "nofile=65535:65535",
            "--label", "project=open-ontology",
            "--label", "oo.task=DL01-W1-45",
            "--label", "oo.capture=unique-window",
            IMAGE,
        ]
        started = run(command, timeout=60)
        phase("docker-run", started)
        if started.returncode != 0:
            return started.returncode
    deadline = time.time() + 180
    last = subprocess.CompletedProcess(command, 1, b"", b"")
    ready = False
    while time.time() < deadline:
        state = run(["docker", "inspect", "-f", "{{.State.Status}}", CONTAINER])
        if state.stdout.decode().strip() not in {"running", "created"}:
            phase("ready-state", state)
            return 1
        probe = mysql("SELECT 1;")
        backends = mysql("SHOW BACKENDS;")
        last = probe
        if probe.returncode == 0 and b"true" in backends.stdout.lower():
            phase("ready", probe)
            phase("backends", backends)
            ready = True
            break
        time.sleep(3)
    if not ready:
        phase("ready-timeout", last)
        return 1
    identity = mysql("SELECT current_version();\nSELECT @@version_comment;\nSELECT @@version;\n")
    phase("identity", identity)
    image_id = run(["docker", "inspect", "-f", "{{.Image}}", CONTAINER])
    repo = run(["docker", "image", "inspect", "-f", "{{json .RepoDigests}}", image_id.stdout.decode().strip() or IMAGE])
    phase("image-digest", subprocess.CompletedProcess(repo.args, repo.returncode, image_id.stdout + repo.stdout, repo.stderr))
    cpu = run(["docker", "exec", CONTAINER, "cat", "/sys/fs/cgroup/cpu.max"])
    phase("cpu-max", cpu)
    mem = run(["docker", "inspect", "-f", "{{.HostConfig.NanoCpus}} {{.HostConfig.Memory}}", CONTAINER])
    phase("host-config", mem)
    if resume:
        grant_ok = True
        loaded = subprocess.CompletedProcess(["reused-dba-load"], 0, b"", b"")
    else:
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
        grant_sets = [
            [
                "GRANT SELECT ON ALL TABLES IN DATABASE oo_w45_sr TO USER 'oo_w45_reader'@'%';",
                "GRANT SELECT ON oo_w45_sr.* TO 'oo_w45_reader'@'%';",
            ],
            [
                "GRANT SELECT, INSERT, UPDATE, DELETE, ALTER, DROP ON ALL TABLES IN DATABASE oo_w45_sr TO USER 'oo_w45_dba'@'%';",
                "GRANT SELECT, INSERT, UPDATE, DELETE, ALTER, DROP ON oo_w45_sr.* TO 'oo_w45_dba'@'%';",
            ],
            [
                "GRANT CREATE TABLE ON DATABASE oo_w45_sr TO USER 'oo_w45_dba'@'%';",
                "GRANT CREATE TABLE ON oo_w45_sr TO 'oo_w45_dba'@'%';",
            ],
        ]
        grant_ok = True
        for index, choices in enumerate(grant_sets):
            chosen = None
            for statement in choices:
                proc = mysql(statement)
                if proc.returncode == 0:
                    chosen = proc
                    break
                chosen = proc
            phase(f"grant-{index}", chosen)
            grant_ok = grant_ok and chosen is not None and chosen.returncode == 0
        shown = mysql("SHOW GRANTS FOR 'oo_w45_reader'@'%';\nSHOW GRANTS FOR 'oo_w45_dba'@'%';\n")
        phase("show-grants", shown)
        write_secret("dba.cnf", "[client]\nuser=oo_w45_dba\npassword={password}\nhost=127.0.0.1\nport=9030\nprotocol=TCP\n".format(password=dba_password))
        write_secret("reader.cnf", "[client]\nuser=oo_w45_reader\npassword={password}\nhost=127.0.0.1\nport=9030\nprotocol=TCP\n".format(password=reader_password))
        run(["docker", "exec", CONTAINER, "mkdir", "-p", "/tmp/oo-secrets"])
        run(["docker", "cp", str(SECRET_DIR / "dba.cnf"), f"{CONTAINER}:/tmp/oo-secrets/dba.cnf"])
        run(["docker", "cp", str(SECRET_DIR / "reader.cnf"), f"{CONTAINER}:/tmp/oo-secrets/reader.cnf"])
        run(["docker", "exec", CONTAINER, "chmod", "600", "/tmp/oo-secrets/dba.cnf", "/tmp/oo-secrets/reader.cnf"])
        loaded = mysql(LOAD_SQL.read_text(encoding="utf-8"), "/tmp/oo-secrets/dba.cnf", timeout=180)
        phase("dba-load", loaded)
    after_load = mysql(READ_SQL, "/tmp/oo-secrets/reader.cnf")
    phase("reader-after-load", after_load)
    updated = mysql(UPDATE_SQL, "/tmp/oo-secrets/dba.cnf")
    phase("dba-update", updated)
    after_update = mysql(READ_SQL, "/tmp/oo-secrets/reader.cnf")
    phase("reader-after-update", after_update)
    inserted = mysql(INSERT_SQL, "/tmp/oo-secrets/dba.cnf")
    phase("dba-insert", inserted)
    after_insert = mysql(READ_SQL, "/tmp/oo-secrets/reader.cnf")
    phase("reader-after-insert", after_insert)
    create_table = mysql("USE oo_w45_sr;\nSHOW CREATE TABLE uniq_dim;", "/tmp/oo-secrets/reader.cnf")
    phase("show-create", create_table)
    reader_writes = 0
    summary = {
        "protocol": "oo.starrocks.unique-window/v1",
        "at": now_iso(),
        "container": CONTAINER,
        "paused_container_started": False,
        "image": IMAGE,
        "image_id": image_id.stdout.decode().strip(),
        "pin_matches": image_id.stdout.decode().strip() == PIN and PIN in repo.stdout.decode(),
        "version_stdout": scrub(identity.stdout).decode("utf-8", "replace").splitlines(),
        "cpu_max": cpu.stdout.decode().strip(),
        "nano_cpus_memory": mem.stdout.decode().strip(),
        "resumed_same_container": resume,
        "grant_ok": grant_ok,
        "load_exit": loaded.returncode,
        "reader_after_load": scrub(after_load.stdout).decode().splitlines(),
        "update_exit": updated.returncode,
        "update_stderr": scrub(updated.stderr).decode("utf-8", "replace"),
        "reader_after_update": scrub(after_update.stdout).decode().splitlines(),
        "insert_exit": inserted.returncode,
        "insert_stderr": scrub(inserted.stderr).decode("utf-8", "replace"),
        "reader_after_insert": scrub(after_insert.stdout).decode().splitlines(),
        "legal_verb": "INSERT",
        "illegal_verb": "UPDATE",
        "delete_sent": False,
        "reader_write_attempts": reader_writes,
        "product_write_attempts": 0,
        "label_stayed_second_after_update": after_update.stdout == after_load.stdout and b"second" in after_update.stdout,
        "label_third_after_insert": b"17\tthird\t4.40" in after_insert.stdout and b"18\tkeep" in after_insert.stdout,
        "dim_18_still_present": b"18\tkeep" in after_insert.stdout,
    }
    (EVIDENCE / "summary.json").write_text(json.dumps(summary, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    stopped = run(["docker", "stop", CONTAINER], timeout=120)
    phase("docker-stop", stopped)
    ok = (
        summary["pin_matches"]
        and loaded.returncode == 0
        and updated.returncode != 0
        and b"does not support update" in updated.stderr
        and summary["label_stayed_second_after_update"]
        and inserted.returncode == 0
        and summary["label_third_after_insert"]
        and reader_writes == 0
    )
    (EVIDENCE / "result.exit").write_text("0\n" if ok else "1\n", encoding="utf-8")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
