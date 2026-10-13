#!/usr/bin/env python3
"""StarRocks DL01-W1-13 preparation journey.

No Rust compiler is started. SQL credentials stay in /tmp mode 0700 files and
are redacted from every evidence byte this process writes.
"""

from __future__ import annotations

import json
import os
import select
import shutil
import subprocess
import time
from datetime import datetime, timezone
from pathlib import Path

import oracle_lib

REPO = Path("/Users/redux/puppy-one/open-ontology")
TASK = "DL01-W1-13"
SESSION = "6f476a94-2648-4c2a-8154-d0b9f8b086fc"
PREFIX = "oo-dl01w1-13"
BASE = "577160c22de63adb9b38e0147f3bc2da893c4d9d"
IMAGE = "starrocks/allin1-ubuntu@sha256:9b98696ee0cb46831eabe197ae5e71f483ee3c2e16ab57fd27cb51a5c9ee6618"
RUST_IMAGE = "sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
CONTAINER = f"{PREFIX}-starrocks"
NETWORK = PREFIX
TARGET = REPO / "testing" / "dl01-targets" / "starrocks"
EVIDENCE = REPO / "evidence" / SESSION / "DL01" / "starrocks"
RAW = EVIDENCE / "raw"
SECRET_DIR = Path("/tmp/oo-dl01w1-13-secrets")
LOCK_PATH = Path("/tmp/oo-dl01-wave1-resources.lock")
ORACLE = json.loads((TARGET / "oracle" / "expected.json").read_text(encoding="utf-8"))

SECRETS: list[str] = []
MYSQL_BIN = "mysql"
CHECKS: list[dict] = []
PHASES: dict[str, int] = {}


def now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def add_check(name: str, ok: bool, expected: object, actual: object, detail: str = "") -> None:
    CHECKS.append(
        {
            "name": name,
            "ok": bool(ok),
            "expected": expected,
            "actual": actual,
            "detail": detail,
        }
    )


def run(cmd: list[str], timeout: int = 120, stdin: bytes | None = None) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(cmd, input=stdin, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout, check=False)


def redact_text(data: bytes | str) -> str:
    if isinstance(data, bytes):
        text = data.decode("utf-8", "replace")
    else:
        text = data
    return oracle_lib.redact(text, SECRETS)


def save_text(path: Path, data: bytes | str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(redact_text(data), encoding="utf-8")


def save_phase(name: str, code: int, stdout: bytes | str = b"", stderr: bytes | str = b"") -> int:
    PHASES[name] = code
    save_text(RAW / f"{name}.stdout", stdout)
    save_text(RAW / f"{name}.stderr", stderr)
    (RAW / f"{name}.exit").write_text(f"{code}\n", encoding="utf-8")
    return code


def command_phase(name: str, cmd: list[str], timeout: int = 120, stdin: bytes | None = None) -> subprocess.CompletedProcess[bytes]:
    try:
        proc = run(cmd, timeout=timeout, stdin=stdin)
    except subprocess.TimeoutExpired as exc:
        save_phase(name, 124, exc.stdout or b"", exc.stderr or b"timeout")
        raise
    save_phase(name, proc.returncode, proc.stdout, proc.stderr)
    return proc


def quote_ident(secret: str) -> str:
    return secret.replace("\\", "\\\\").replace("'", "''")


def write_cnf(name: str, user: str, password: str) -> Path:
    SECRET_DIR.mkdir(mode=0o700, exist_ok=True)
    os.chmod(SECRET_DIR, 0o700)
    path = SECRET_DIR / f"{name}.cnf"
    path.write_text(
        "[client]\n"
        f"user={user}\n"
        f"password={password}\n"
        "host=127.0.0.1\n"
        "port=9030\n"
        "protocol=TCP\n"
        "default-character-set=utf8mb4\n",
        encoding="utf-8",
    )
    os.chmod(path, 0o600)
    return path


def install_cnf(local: Path, remote_name: str) -> str:
    remote = f"/tmp/oo-secrets/{remote_name}.cnf"
    run(["docker", "exec", CONTAINER, "mkdir", "-p", "/tmp/oo-secrets"])
    copied = run(["docker", "cp", str(local), f"{CONTAINER}:{remote}"])
    if copied.returncode != 0:
        raise RuntimeError(redact_text(copied.stderr))
    run(["docker", "exec", CONTAINER, "chmod", "600", remote])
    return remote


def mysql(remote_cnf: str | None, sql: str, timeout: int = 120, names: bool = False, skip_password: bool = False) -> subprocess.CompletedProcess[bytes]:
    cmd = ["docker", "exec", "-i", CONTAINER, MYSQL_BIN]
    if remote_cnf:
        # defaults-file is exclusive, so an image my.cnf cannot add a password.
        cmd.append(f"--defaults-file={remote_cnf}")
    else:
        cmd.append("--no-defaults")
        cmd.extend(["-uroot", "-h127.0.0.1", "-P9030", "--protocol=TCP"])
        if skip_password:
            cmd.append("--skip-password")
    cmd.extend(["--batch", "--raw", "--connect-timeout=10", "--default-character-set=utf8mb4"])
    if not names:
        cmd.append("--skip-column-names")
    return run(cmd, timeout=timeout, stdin=sql.encode("utf-8"))


def rotate_raw() -> None:
    RAW.mkdir(parents=True, exist_ok=True)
    existing = [path for path in RAW.iterdir() if path.name != ".gitkeep"]
    results = EVIDENCE / "results.json"
    if not existing and not results.exists():
        return
    dest = EVIDENCE / "attempts" / datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    dest.parent.mkdir(parents=True, exist_ok=True)
    if existing:
        shutil.move(str(RAW), str(dest / "raw"))
        RAW.mkdir(parents=True, exist_ok=True)
    if results.exists():
        shutil.copy(results, dest / "results.json")


def preflight() -> None:
    RAW.mkdir(parents=True, exist_ok=True)
    commands = [
        ["uname", "-a"],
        ["sw_vers"],
        ["git", "-C", str(REPO), "rev-parse", "HEAD"],
        ["git", "-C", str(REPO), "rev-parse", f"{BASE}^{{tree}}"],
        ["python3", "--version"],
        ["docker", "info", "--format", "OSType={{.OSType}} Arch={{.Architecture}} NCPU={{.NCPU}} MemTotal={{.MemTotal}}"],
        ["docker", "image", "inspect", RUST_IMAGE, "--format", "id={{.Id}} os={{.Os}} arch={{.Architecture}}"],
    ]
    chunks = [f"when={now()}", "rustc=not_invoked", "cargo=not_invoked", "msrv_applicability=pending_until_rust_consumer"]
    for cmd in commands:
        proc = run(cmd, timeout=30)
        chunks.append(f"$ {' '.join(cmd)}\nexit={proc.returncode}\n{proc.stdout.decode('utf-8', 'replace')}\n{proc.stderr.decode('utf-8', 'replace')}")
    mysql_client = run(["/bin/sh", "-c", "command -v mysql || true"], timeout=10)
    chunks.append(f"host_mysql={mysql_client.stdout.decode().strip() or 'absent'}")
    archive = run(["git", "-C", str(REPO), "archive", BASE, "Cargo.toml", "rust-toolchain.toml"], timeout=60)
    digest = run(["/usr/bin/shasum", "-a", "256"], timeout=30, stdin=archive.stdout)
    chunks.append(
        f"git_archive_exit={archive.returncode} archive_bytes={len(archive.stdout)} "
        f"sha256={digest.stdout.decode().strip()} rust_version_line_from_base_not_compiled"
    )
    save_phase("00-preflight", 0, "\n".join(chunks), archive.stderr)


def rust_census() -> dict:
    ids = run(["docker", "ps", "-q"], timeout=30).stdout.decode().split()
    rows = []
    global_rust = 0
    own_rust = 0
    dev_builders = 0
    for container_id in ids:
        proc = run(
            [
                "docker",
                "inspect",
                "-f",
                "{{.Name}}\t{{.Config.Image}}\t{{index .Config.Labels \"oo.kind\"}}\t{{index .Config.Labels \"oo.prefix\"}}\t{{.State.Status}}",
                container_id,
            ],
            timeout=30,
        )
        line = proc.stdout.decode("utf-8", "replace").strip()
        parts = line.split("\t")
        if len(parts) != 5:
            rows.append(line)
            continue
        name, image, kind, prefix, status = parts
        image_l = image.lower()
        is_rust = kind == "rust-builder" or "rust" in image_l or image.startswith(RUST_IMAGE)
        if is_rust and status == "running":
            global_rust += 1
            if prefix == PREFIX or PREFIX in name:
                own_rust += 1
        if kind == "rust-builder" and status == "running":
            dev_builders += 1
        rows.append(f"{name}\t{image}\t{kind}\t{prefix}\t{status}")
    return {
        "global_rust": global_rust,
        "own_rust": own_rust,
        "dev_builders_running": dev_builders,
        "rows": rows,
        "decision": oracle_lib.decide_admission(global_rust, own_rust),
    }


def image_facts() -> dict:
    proc = run(
        [
            "docker",
            "image",
            "inspect",
            IMAGE,
            "--format",
            "{{.Id}}\t{{.Os}}\t{{.Architecture}}\t{{.Size}}\t{{json .RepoDigests}}",
        ],
        timeout=30,
    )
    if proc.returncode != 0:
        return {"present": False, "error": proc.stderr.decode("utf-8", "replace")}
    image_id, os_name, arch, size, digests = proc.stdout.decode().strip().split("\t", 4)
    return {
        "present": True,
        "id": image_id,
        "os": os_name,
        "architecture": arch,
        "size": size,
        "repo_digests": digests,
    }


def admit_and_start() -> str:
    """Hold the resource lock only for the census and the docker start."""
    import fcntl

    lock_fd = os.open(LOCK_PATH, os.O_RDWR | os.O_CREAT, 0o644)
    fcntl.flock(lock_fd, fcntl.LOCK_EX)
    try:
        census = rust_census()
        facts = image_facts()
        save_text(RAW / "01-admission.txt", json.dumps({"when": now(), "census": census, "image": facts}, indent=2))
        decision = census["decision"]
        if decision != "admit":
            save_phase("01-admission", 5, decision, "")
            return decision
        if not facts.get("present"):
            save_phase("01-admission", 3, "image-absent", facts.get("error", ""))
            return "image-absent"
        if facts.get("architecture") != "arm64" or facts.get("os") != "linux":
            save_phase("01-admission", 4, json.dumps(facts), "")
            add_check("native_arm64_image", False, "linux/arm64", facts, "refusing emulation")
            return "architecture"
        add_check("native_arm64_image", True, "linux/arm64", f"{facts['os']}/{facts['architecture']}", facts["id"])
        own = run(["docker", "ps", "-aq", "--filter", f"name=^{CONTAINER}$"], timeout=30)
        if own.stdout.strip():
            label = run(["docker", "inspect", "-f", "{{index .Config.Labels \"oo.prefix\"}}", CONTAINER], timeout=30)
            if label.stdout.decode().strip() == PREFIX:
                run(["docker", "rm", "-f", CONTAINER], timeout=60)
            else:
                save_phase("01-admission", 5, "name-collision", label.stdout)
                return "name-collision"
        net = run(["docker", "network", "inspect", NETWORK], timeout=30)
        if net.returncode != 0:
            created = run(
                [
                    "docker",
                    "network",
                    "create",
                    "--label",
                    "project=open-ontology",
                    "--label",
                    f"owner={PREFIX}",
                    "--label",
                    f"session={SESSION}",
                    "--label",
                    "kind=starrocks-target",
                    "--label",
                    f"oo.prefix={PREFIX}",
                    NETWORK,
                ],
                timeout=30,
            )
            if created.returncode != 0:
                save_phase("01-admission", created.returncode, created.stdout, created.stderr)
                return "network-failed"
        started = run(
            [
                "docker",
                "run",
                "-d",
                "--name",
                CONTAINER,
                "--platform",
                "linux/arm64",
                "--network",
                NETWORK,
                "--memory",
                "4g",
                "--memory-swap",
                "4g",
                "--cpus",
                "1",
                "--restart",
                "no",
                "--pull",
                "never",
                "--ulimit",
                "nofile=65535:65535",
                "--label",
                "project=open-ontology",
                "--label",
                f"owner={PREFIX}",
                "--label",
                f"session={SESSION}",
                "--label",
                "kind=starrocks-target",
                "--label",
                f"oo.prefix={PREFIX}",
                "--label",
                f"oo.task={TASK}",
                "--label",
                "oo.product=starrocks",
                IMAGE,
            ],
            timeout=60,
        )
        save_phase("02-start", started.returncode, started.stdout, started.stderr)
        return "started" if started.returncode == 0 else "start-failed"
    finally:
        fcntl.flock(lock_fd, fcntl.LOCK_UN)
        os.close(lock_fd)


def container_state() -> dict:
    proc = run(
        [
            "docker",
            "inspect",
            "-f",
            "{{.State.Status}}\t{{.State.ExitCode}}\t{{.State.OOMKilled}}\t{{.HostConfig.Memory}}\t{{.HostConfig.NanoCpus}}\t{{.HostConfig.MemorySwap}}",
            CONTAINER,
        ],
        timeout=30,
    )
    if proc.returncode != 0:
        return {"status": "missing", "error": proc.stderr.decode("utf-8", "replace")}
    fields = proc.stdout.decode().strip().split("\t")
    if len(fields) < 4:
        return {"status": "unknown", "raw": proc.stdout.decode("utf-8", "replace")}
    status, code, oom, memory = fields[:4]
    nano = fields[4] if len(fields) > 4 else ""
    swap = fields[5] if len(fields) > 5 else ""
    return {
        "status": status,
        "exit_code": code,
        "oom_killed": oom,
        "memory": memory,
        "nano_cpus": nano,
        "memory_swap": swap,
    }


def find_client() -> str | None:
    proc = run(
        ["docker", "exec", CONTAINER, "sh", "-c", "command -v mysql || command -v mariadb || true"],
        timeout=30,
    )
    path = proc.stdout.decode().strip().splitlines()
    return path[0] if path and path[0] else None


def wait_ready() -> bool:
    deadline = time.time() + 180
    last = b""
    while time.time() < deadline:
        state = container_state()
        if state.get("status") not in {"running", "created"}:
            save_phase("03-ready", 3, json.dumps(state), last)
            logs = run(["docker", "logs", CONTAINER], timeout=30)
            save_text(RAW / "03-ready.docker-logs", logs.stdout + b"\n" + logs.stderr)
            add_check("server_ready", False, "FE accepts SELECT 1 and BE is alive", state, "container exited")
            return False
        probe = mysql(None, "SELECT 1;", timeout=15, skip_password=True)
        last = probe.stderr
        if probe.returncode == 0:
            backend = mysql(None, "SHOW BACKENDS;", timeout=20, names=True, skip_password=True)
            text = probe.stdout.decode("utf-8", "replace") + "\n" + backend.stdout.decode("utf-8", "replace")
            if "true" in backend.stdout.decode("utf-8", "replace").lower():
                save_phase("03-ready", 0, text, probe.stderr + backend.stderr)
                add_check("server_ready", True, "FE up and BE alive", "alive", "")
                return True
        time.sleep(3)
    logs = run(["docker", "logs", CONTAINER], timeout=30)
    save_phase("03-ready", 3, container_state(), last)
    save_text(RAW / "03-ready.docker-logs", logs.stdout + b"\n" + logs.stderr)
    add_check("server_ready", False, "ready within 180s", "timeout", "")
    return False


def capture_runtime_files() -> None:
    script = r"""
set +e
echo "=== client ==="
mysql --version 2>&1
mariadb --version 2>&1
echo "=== license candidates ==="
find /data /opt /usr/share /etc -iname 'LICENSE*' -o -iname 'NOTICE*' -o -iname 'COPYING*' 2>/dev/null | head -80
echo "=== conf memory ==="
grep -n -E 'JAVA_OPTS|Xmx|mem_limit' /data/deploy/starrocks/fe/conf/fe.conf /data/deploy/starrocks/be/conf/be.conf 2>/dev/null | head -100
echo "=== home ==="
ls -ld /data/deploy/starrocks /data/deploy/starrocks/fe /data/deploy/starrocks/be 2>&1 | head
echo "=== audit log ==="
ls -l /data/deploy/starrocks/fe/log 2>&1 | head -40
"""
    proc = command_phase("04-runtime-files", ["docker", "exec", CONTAINER, "sh", "-c", script], timeout=60)
    text = proc.stdout.decode("utf-8", "replace")
    license_paths = []
    for line in text.splitlines():
        if line.startswith("/") and any(token in line.upper() for token in ("LICENSE", "NOTICE", "COPYING")):
            license_paths.append(line.strip())
    hashes = []
    for path in license_paths[:12]:
        hashed = run(["docker", "exec", CONTAINER, "sha256sum", path], timeout=30)
        hashes.append(hashed.stdout.decode("utf-8", "replace").strip() + hashed.stderr.decode("utf-8", "replace"))
        if "LICENSE" in path.upper() and path.endswith((".txt", "LICENSE", "LICENSE.md")):
            body = run(["docker", "exec", CONTAINER, "cat", path], timeout=30)
            if body.returncode == 0 and len(body.stdout) < 200_000:
                save_text(EVIDENCE / "image-license" / Path(path).name, body.stdout)
    save_text(RAW / "04-license-sha256.txt", "\n".join(hashes) + "\n")
    add_check(
        "license_file_recorded",
        bool(hashes),
        "at least one LICENSE/NOTICE hash from the image",
        hashes[:6],
        "absence is recorded and is not an Apache-2.0 confirmation",
    )


def identity(root_cnf: str | None, skip_password: bool) -> bool:
    statements = {
        "current_version": "SELECT current_version();",
        "version_comment": "SELECT @@version_comment;",
        "protocol_version": "SELECT @@version;",
    }
    values = {}
    raw = []
    ok_sql = True
    for key, sql in statements.items():
        proc = mysql(root_cnf, sql, timeout=30, skip_password=skip_password)
        out = proc.stdout.decode("utf-8", "replace").strip()
        err = proc.stderr.decode("utf-8", "replace")
        raw.append(f"-- {sql}\nexit={proc.returncode}\n{out}\n{err}")
        values[key] = out.splitlines()[-1].strip() if proc.returncode == 0 and out else ""
        ok_sql = ok_sql and proc.returncode == 0
    catalog_sql = (
        "SELECT LOWER(table_name) FROM information_schema.tables "
        "WHERE LOWER(table_schema) = 'information_schema' "
        "AND LOWER(table_name) IN ('be_tablets', 'tables_config', 'fe_metrics', 'be_configs') "
        "ORDER BY 1;"
    )
    catalog = mysql(root_cnf, catalog_sql, timeout=30, skip_password=skip_password)
    catalog_text = catalog.stdout.decode("utf-8", "replace")
    raw.append(f"-- {catalog_sql}\nexit={catalog.returncode}\n{catalog_text}\n{catalog.stderr.decode('utf-8', 'replace')}")
    tables = [line.strip() for line in catalog_text.splitlines() if line.strip()]
    ok_sql = ok_sql and catalog.returncode == 0
    save_text(RAW / "05-identity.txt", "\n".join(raw))
    decision = oracle_lib.product_identity(
        values.get("current_version", ""),
        values.get("version_comment", ""),
        values.get("protocol_version", ""),
        tables,
    )
    add_check("product_identity", bool(decision["ok"] and ok_sql), "StarRocks banner", decision, "")
    save_phase("05-identity", 0 if decision["ok"] and ok_sql else 2, json.dumps(decision), "")
    topo = mysql(root_cnf, "SHOW FRONTENDS;\nSHOW BACKENDS;\n", timeout=30, names=True, skip_password=skip_password)
    save_text(RAW / "05-topology.stdout", topo.stdout)
    save_text(RAW / "05-topology.stderr", topo.stderr)
    return bool(decision["ok"] and ok_sql)


def bootstrap(root_mode: dict) -> dict[str, str] | None:
    dba_password = os.urandom(18).hex()
    reader_password = os.urandom(18).hex()
    root_password = os.urandom(18).hex()
    SECRETS.extend([dba_password, reader_password, root_password])
    dba_sql_password = quote_ident(dba_password)
    reader_sql_password = quote_ident(reader_password)
    root_sql_password = quote_ident(root_password)
    create = (
        "CREATE DATABASE IF NOT EXISTS oo_dl01_sr;\n"
        f"CREATE USER 'oo_dba'@'%' IDENTIFIED BY '{dba_sql_password}';\n"
        f"CREATE USER 'oo_reader'@'%' IDENTIFIED BY '{reader_sql_password}';\n"
    )
    create_ok = True
    create_log = []
    for statement in create.strip().splitlines():
        proc = mysql(root_mode.get("cnf"), statement, timeout=30, skip_password=root_mode["skip"])
        create_log.append(
            f"exit={proc.returncode}\n{redact_text(statement)}\n"
            f"{proc.stdout.decode('utf-8', 'replace')}\n{proc.stderr.decode('utf-8', 'replace')}"
        )
        create_ok = create_ok and proc.returncode == 0
    save_phase("06-create-users", 0 if create_ok else 1, "\n".join(create_log), "")
    if not create_ok:
        add_check("separate_dba_and_reader", False, "two SQL users created", "create failed", "")
        return None
    reader_grants = [
        "GRANT SELECT ON ALL TABLES IN DATABASE oo_dl01_sr TO USER 'oo_reader'@'%';",
        "GRANT SELECT ON oo_dl01_sr.* TO 'oo_reader'@'%';",
    ]
    # Table ALL PRIVILEGES does not include database-level CREATE TABLE.
    # A statement exit of 0 is not enough: SHOW GRANTS must show the privilege.
    # StarRocks prints that privilege as CREATE_TABLE.
    dba_table_grants = [
        "GRANT SELECT, INSERT, UPDATE, DELETE, ALTER, DROP ON ALL TABLES IN DATABASE oo_dl01_sr TO USER 'oo_dba'@'%';",
        "GRANT SELECT, INSERT, UPDATE, DELETE, ALTER, DROP ON ALL TABLES IN DATABASE oo_dl01_sr TO 'oo_dba'@'%';",
    ]
    dba_create_grants = [
        "GRANT CREATE TABLE ON DATABASE oo_dl01_sr TO USER 'oo_dba'@'%';",
        "GRANT CREATE TABLE ON DATABASE oo_dl01_sr TO 'oo_dba'@'%';",
    ]
    reader_ok = try_grants(root_mode, reader_grants, "06-grant-reader", "SELECT", "oo_reader")
    dba_table_ok = try_grants(root_mode, dba_table_grants, "06-grant-dba-tables", "INSERT", "oo_dba")
    dba_create_ok = try_grants(root_mode, dba_create_grants, "06-grant-dba-create", "CREATE TABLE", "oo_dba")
    dba_ok = dba_table_ok and dba_create_ok
    grants = mysql(
        root_mode.get("cnf"),
        "SHOW GRANTS FOR 'oo_reader'@'%';\nSHOW GRANTS FOR 'oo_dba'@'%';\n",
        timeout=20,
        names=True,
        skip_password=root_mode["skip"],
    )
    save_text(RAW / "06-grants.stdout", grants.stdout)
    save_text(RAW / "06-grants.stderr", grants.stderr)
    password_sql = (
        f"ALTER USER 'root'@'%' IDENTIFIED BY '{root_sql_password}';\n"
        f"SET PASSWORD FOR 'root'@'%' = PASSWORD('{root_sql_password}');\n"
    )
    # One of these forms is enough. Try them as separate statements so a
    # parser error on the first does not hide the second.
    root_set = False
    for statement in password_sql.strip().splitlines():
        attempt = mysql(root_mode.get("cnf"), statement, timeout=20, skip_password=root_mode["skip"])
        save_text(RAW / "06-root-password.stdout", attempt.stdout)
        save_text(RAW / "06-root-password.stderr", attempt.stderr)
        if attempt.returncode == 0:
            root_set = True
            break
    (RAW / "06-root-password.exit").write_text("0\n" if root_set else "1\n", encoding="utf-8")
    add_check("root_password_set", root_set, "bootstrap root no longer has an empty password", root_set, "")
    add_check("separate_dba_and_reader", reader_ok and dba_ok, "distinct grants", {"reader": reader_ok, "dba": dba_ok}, "")
    if not (reader_ok and dba_ok):
        return None
    dba_local = write_cnf("dba", "oo_dba", dba_password)
    reader_local = write_cnf("reader", "oo_reader", reader_password)
    return {
        "dba": install_cnf(dba_local, "dba"),
        "reader": install_cnf(reader_local, "reader"),
    }


def try_grants(root_mode: dict, statements: list[str], phase: str, needle: str, user: str) -> bool:
    log = []
    for statement in statements:
        proc = mysql(root_mode.get("cnf"), statement, timeout=20, skip_password=root_mode["skip"])
        shown = mysql(
            root_mode.get("cnf"),
            f"SHOW GRANTS FOR '{user}'@'%';",
            timeout=20,
            names=True,
            skip_password=root_mode["skip"],
        )
        shown_text = shown.stdout.decode("utf-8", "replace")
        visible = oracle_lib.grant_visible(shown_text, needle)
        log.append(
            f"exit={proc.returncode}\n{statement}\n"
            f"{proc.stdout.decode('utf-8', 'replace')}\n{proc.stderr.decode('utf-8', 'replace')}\n"
            f"show_grants_exit={shown.returncode}\n{shown_text}\n"
            f"{shown.stderr.decode('utf-8', 'replace')}\n"
            f"needle={needle} visible={visible}"
        )
        if proc.returncode == 0 and shown.returncode == 0 and visible:
            save_phase(phase, 0, "\n".join(log), "")
            return True
    save_phase(phase, 1, "\n".join(log), "")
    return False


def exec_sql_file(remote_cnf: str, path: Path, phase: str, timeout: int = 180) -> bool:
    sql = path.read_text(encoding="utf-8")
    proc = mysql(remote_cnf, sql, timeout=timeout)
    save_phase(phase, proc.returncode, proc.stdout, proc.stderr)
    return proc.returncode == 0


def read_tables(remote_cnf: str, phase: str) -> dict[str, list[str]] | None:
    sql = (TARGET / "sql" / "reader-positive.sql").read_text(encoding="utf-8")
    proc = mysql(remote_cnf, sql, timeout=120)
    save_phase(phase, proc.returncode, proc.stdout, proc.stderr)
    if proc.returncode != 0:
        return None
    return oracle_lib.split_marked_results(proc.stdout.decode("utf-8", "replace"))


def expect_table(marked: dict[str, list[str]] | None, table: str, rows: list[dict], phase: str) -> bool:
    columns = ORACLE[table]["columns"]
    if marked is None or table not in marked:
        add_check(phase, False, "rows", "missing result", table)
        return False
    compared = oracle_lib.rows_match(marked[table], rows, columns)
    add_check(phase, compared["ok"], compared["expected"], compared["actual"], table)
    return compared["ok"]


def show_partitions(remote_cnf: str, phase: str) -> str:
    proc = mysql(remote_cnf, "SHOW PARTITIONS FROM oo_dl01_sr.dup_events;", timeout=30, names=True)
    save_phase(phase, proc.returncode, proc.stdout, proc.stderr)
    return proc.stdout.decode("utf-8", "replace")


def audit_path() -> str | None:
    proc = run(
        [
            "docker",
            "exec",
            CONTAINER,
            "sh",
            "-c",
            "ls /data/deploy/starrocks/fe/log/fe.audit.log /opt/starrocks/fe/log/fe.audit.log 2>/dev/null | head -1",
        ],
        timeout=20,
    )
    path = proc.stdout.decode().strip()
    return path or None


def copy_audit(path: str | None, dest: Path) -> str:
    if not path:
        save_text(dest, "AUDIT_LOG_MISSING\n")
        return ""
    proc = run(["docker", "exec", CONTAINER, "cat", path], timeout=30)
    text = redact_text(proc.stdout)
    save_text(dest, text)
    return text


def reader_negatives(remote_cnf: str) -> bool:
    statements = [
        "USE oo_dl01_sr; INSERT INTO pk_accounts VALUES (999, CAST('1.000000000' AS DECIMAL(38,9)), 'nope');",
        "USE oo_dl01_sr; UPDATE pk_accounts SET nickname = 'changed' WHERE account_id = 101;",
        "USE oo_dl01_sr; DELETE FROM pk_accounts WHERE account_id = 101;",
        "USE oo_dl01_sr; CREATE TABLE should_not (id INT);",
        "USE oo_dl01_sr; DROP TABLE pk_accounts;",
    ]
    all_rejected = True
    chunks = []
    for index, statement in enumerate(statements, start=1):
        proc = mysql(remote_cnf, statement, timeout=30)
        rejected = proc.returncode != 0
        all_rejected = all_rejected and rejected
        chunks.append(
            f"--- {index} exit={proc.returncode} rejected={rejected}\n{statement}\n"
            f"{proc.stdout.decode('utf-8', 'replace')}\n{proc.stderr.decode('utf-8', 'replace')}"
        )
    save_phase("09-reader-negative", 0 if all_rejected else 2, "\n".join(chunks), "")
    add_check("reader_dml_ddl_rejected", all_rejected, "every reader write rejected", f"{len(statements)} statements", "")
    return all_rejected


def resource_caps() -> bool:
    state = container_state()
    save_text(RAW / "11-resource-caps.json", json.dumps(state, indent=2))
    ok = (
        state.get("memory") == "4294967296"
        and state.get("memory_swap") == "4294967296"
        and state.get("nano_cpus") == "1000000000"
        and state.get("oom_killed") == "false"
    )
    add_check("resource_caps", ok, "memory 4294967296, swap 4294967296, nano_cpus 1000000000, not OOMKilled", state, "")
    return ok


def open_reader_session(remote_cnf: str) -> subprocess.Popen[bytes]:
    return subprocess.Popen(
        [
            "docker",
            "exec",
            "-i",
            CONTAINER,
            "stdbuf",
            "-o0",
            "-e0",
            MYSQL_BIN,
            f"--defaults-file={remote_cnf}",
            "--batch",
            "--raw",
            "--quick",
            "--connect-timeout=10",
            "--default-character-set=utf8mb4",
        ],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )


def session_call(proc: subprocess.Popen[bytes], sql: str, token: str, timeout: float) -> tuple[str, float, bool]:
    if proc.stdin is None or proc.stdout is None or proc.poll() is not None:
        return "", 0.0, False
    proc.stdin.write(f"{sql.rstrip().rstrip(';')};\nSELECT '{token}';\n".encode("utf-8"))
    proc.stdin.flush()
    fd = proc.stdout.fileno()
    os.set_blocking(fd, False)
    buf = b""
    started = time.time()
    token_b = token.encode("utf-8")
    end = started + timeout
    while time.time() < end:
        if token_b in buf:
            return buf.decode("utf-8", "replace"), time.time() - started, True
        ready, _, _ = select.select([fd], [], [], 0.2)
        if proc.poll() is not None and not ready:
            break
        if not ready:
            continue
        try:
            chunk = os.read(fd, 65536)
        except BlockingIOError:
            continue
        if not chunk:
            break
        buf += chunk
    return buf.decode("utf-8", "replace"), time.time() - started, token_b in buf


def process_rows(text: str) -> list[dict[str, str]]:
    header: list[str] | None = None
    rows: list[dict[str, str]] = []
    for line in text.splitlines():
        if not line.strip():
            continue
        cols = line.rstrip("\r").split("\t")
        if header is None:
            if "Id" in cols and "User" in cols and "Info" in cols:
                header = cols
            continue
        if len(cols) < len(header):
            continue
        row = {header[index]: cols[index] for index in range(len(header))}
        if not row.get("Id", "").isdigit():
            continue
        rows.append(row)
    return rows


def cancel_query(reader_cnf: str) -> bool:
    """Kill a running reader statement from a second reader session.

    At 1 CPU the running SELECT holds the only BE query slot. A new mysql
    client blocks in its automatic version probe and never sends SHOW
    PROCESSLIST, so the business statement ends as ERROR 5024. The control
    session finishes that handshake before sleep starts, then sees the marker
    and issues KILL QUERY on the same session. Sleep stays longer than the
    cancel, and query_timeout stays longer than sleep.
    """
    marker = "oo-dl01w1-13-cancel"
    sql = f"SELECT '{marker}', sleep(90);"
    ctrl: subprocess.Popen[bytes] | None = None
    biz: subprocess.Popen[bytes] | None = None
    local_abort = False
    try:
        ctrl = open_reader_session(reader_cnf)
        ready_text, ready_s, ready_ok = session_call(ctrl, "SELECT CONNECTION_ID()", "oo13_ctrl_ready", 15)
        save_text(RAW / "11-cancel-control-ready.txt", f"elapsed_s={ready_s:.3f} ok={ready_ok}\n{ready_text}")
        if not ready_ok:
            add_check("cancel", False, "control session ready before the business statement", ready_text[:500], "")
            save_phase("11-cancel", 2, "control session was not ready", "")
            return False
        biz = subprocess.Popen(
            [
                "docker",
                "exec",
                "-i",
                CONTAINER,
                MYSQL_BIN,
                f"--defaults-file={reader_cnf}",
                "--batch",
                "--raw",
                "--skip-column-names",
                "--connect-timeout=10",
                "--default-character-set=utf8mb4",
            ],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        assert biz.stdin is not None
        started = time.time()
        biz.stdin.write(f"SET query_timeout = 180; USE oo_dl01_sr; {sql}\n".encode("utf-8"))
        biz.stdin.close()
        time.sleep(1.0)
        seen = ""
        query_id = ""
        counted: dict[str, object] = {}
        timeouts = 0
        list_elapsed = None
        for attempt in range(8):
            if biz.poll() is not None:
                break
            text, elapsed, ok = session_call(ctrl, "SHOW FULL PROCESSLIST", f"oo13_ctrl_list_{attempt}", 8)
            if not ok:
                timeouts += 1
                save_text(RAW / f"11-cancel-1-processlist-timeout-{attempt}.txt", text or "timeout")
                continue
            if marker not in text:
                save_text(RAW / f"11-cancel-1-processlist-miss-{attempt}.txt", text)
                time.sleep(0.5)
                continue
            seen = text
            list_elapsed = round(elapsed, 3)
            rows = process_rows(text)
            business = 0
            control = 0
            by_user: dict[str, int] = {}
            for row in rows:
                user = row.get("User", "")
                by_user[user] = by_user.get(user, 0) + 1
                if user != "oo_reader":
                    continue
                if marker in row.get("Info", ""):
                    business += 1
                    query_id = row.get("Id", "")
                else:
                    control += 1
            counted = {
                "business_connections": business,
                "control_connections": control,
                "by_user": by_user,
                "rows": rows,
            }
            break
        try:
            stats = run(
                ["docker", "stats", "--no-stream", "--format", "{{.Name}} {{.MemUsage}} {{.CPUPerc}}", CONTAINER],
                timeout=20,
            )
            save_text(RAW / "11-cancel-during-stats.txt", stats.stdout + stats.stderr)
        except subprocess.TimeoutExpired as exc:
            save_text(RAW / "11-cancel-during-stats.txt", (exc.stdout or b"") + b"\ntimeout")
        save_text(RAW / "11-cancel-1-processlist.txt", seen)
        kill_text = ""
        killed = False
        if query_id.isdigit():
            kill_text, _kill_s, killed = session_call(ctrl, f"KILL QUERY {query_id}", "oo13_ctrl_kill", 10)
        save_text(RAW / "11-cancel-1-kill.stdout", kill_text)
        try:
            out, err = biz.communicate(timeout=20)
        except subprocess.TimeoutExpired:
            local_abort = True
            biz.kill()
            out, err = biz.communicate(timeout=5)
        save_text(RAW / "11-cancel-1.stdout", out)
        save_text(RAW / "11-cancel-1.stderr", err)
        (RAW / "11-cancel-1.client-exit").write_text(f"{biz.returncode}\n", encoding="utf-8")
        after_text, _after_s, after_ok = session_call(ctrl, "SHOW FULL PROCESSLIST", "oo13_ctrl_after", 10)
        save_text(RAW / "11-cancel-1-after.txt", after_text)
        still = (not after_ok) or marker in after_text
        elapsed_s = round(time.time() - started, 3)
        err_text = err.decode("utf-8", "replace")
        summary = {
            "killed": killed,
            "query_id": query_id or None,
            "still_visible": still,
            "client_exit": biz.returncode,
            "elapsed_s": elapsed_s,
            "business_connections": counted.get("business_connections", 0),
            "control_connections": counted.get("control_connections", 0),
            "by_user": counted.get("by_user", {}),
            "kill_channel": "oo_reader",
            "dba_used": False,
            "processlist_timeouts": timeouts,
            "list_elapsed_s": list_elapsed,
            "control_primed_before_business": ready_ok,
            "control_prime_elapsed_s": round(ready_s, 3),
            "control_channel": "preopened oo_reader session; handshake finished before sleep; SHOW FULL PROCESSLIST and KILL QUERY reused that session",
            "query_timeout_s": 180,
            "sleep_s": 90,
            "local_client_abort": local_abort,
            "client_error": err_text[-500:],
            "server_kill_result": kill_text[-500:],
        }
        save_text(RAW / "11-cancel-counts.json", json.dumps(summary, indent=2))
        ok = (
            killed
            and not still
            and biz.returncode not in (None, 0)
            and not local_abort
            and "Query reached its timeout" not in err_text
            and "killed" in err_text.lower()
            and summary["business_connections"] == 1
            and summary["control_connections"] >= 1
            and summary["by_user"].get("oo_dba", 0) == 0
            and elapsed_s < 80
            and timeouts == 0
        )
        add_check("cancel", ok, "second oo_reader session kills the visible business query", summary, sql)
        save_phase("11-cancel", 0 if ok else 2, json.dumps(summary), err)
        return ok
    finally:
        if ctrl is not None and ctrl.poll() is None:
            if ctrl.stdin is not None:
                try:
                    ctrl.stdin.close()
                except OSError:
                    pass
            try:
                ctrl.wait(timeout=3)
            except subprocess.TimeoutExpired:
                ctrl.kill()
        if biz is not None and biz.poll() is None:
            biz.kill()
            try:
                biz.wait(timeout=5)
            except subprocess.TimeoutExpired:
                pass


def cleanup() -> None:
    state = container_state()
    save_text(RAW / "12-before-cleanup-state.txt", json.dumps(state))
    logs = run(["docker", "logs", CONTAINER], timeout=40)
    save_text(RAW / "12-docker-logs.txt", logs.stdout + b"\n" + logs.stderr)
    stats = run(["docker", "stats", "--no-stream", "--format", "{{.Name}} {{.MemUsage}} {{.CPUPerc}}", CONTAINER], timeout=20)
    save_text(RAW / "12-docker-stats.txt", stats.stdout + stats.stderr)
    removed = run(["docker", "rm", "-f", CONTAINER], timeout=60)
    # Only remove the network we created.
    net = run(["docker", "network", "rm", NETWORK], timeout=30)
    save_phase("12-cleanup", 0 if removed.returncode in (0, 1) else removed.returncode, removed.stdout + net.stdout, removed.stderr + net.stderr)
    left = run(["docker", "ps", "-a", "--filter", f"name={PREFIX}", "--format", "{{.Names}} {{.Status}}"], timeout=20)
    save_text(RAW / "12-remaining.txt", left.stdout)
    if SECRET_DIR.exists():
        for path in SECRET_DIR.iterdir():
            try:
                data = path.read_bytes()
                path.write_bytes(b"\0" * len(data))
            except OSError:
                pass
            path.unlink(missing_ok=True)
        SECRET_DIR.rmdir()
    add_check("cleanup", PREFIX.encode() not in left.stdout, "no remaining oo-dl01w1-13 containers", left.stdout.decode(), "image is retained")


def write_results(outcome: str) -> None:
    payload = {
        "task": TASK,
        "session": SESSION,
        "base": BASE,
        "image": IMAGE,
        "when": now(),
        "outcome": outcome,
        "phases": PHASES,
        "checks": CHECKS,
        "msrv": "pending_until_rust_consumer",
        "rust_compiled": False,
    }
    (EVIDENCE / "results.json").write_text(json.dumps(payload, indent=2, default=str) + "\n", encoding="utf-8")


def main() -> int:
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    rotate_raw()
    preflight()
    decision = admit_and_start()
    if decision != "started":
        write_results(decision)
        if decision.startswith("pending"):
            (EVIDENCE / "pending-capacity.json").write_text(json.dumps({"when": now(), "decision": decision}, indent=2) + "\n", encoding="utf-8")
            return 5
        if decision == "architecture":
            return 4
        return 3
    try:
        client = None
        for _ in range(30):
            state = container_state()
            if state.get("status") not in {"running", "created"}:
                break
            client = find_client()
            if client:
                break
            time.sleep(2)
        global MYSQL_BIN
        if not client:
            save_phase("03-client", 3, "mysql client not in image", "")
            add_check("native_client", False, "mysql client in the StarRocks image", "absent", "")
            logs = run(["docker", "logs", CONTAINER], timeout=30)
            save_text(RAW / "03-client.docker-logs", logs.stdout + logs.stderr)
            write_results("client-missing")
            return 3
        MYSQL_BIN = client
        save_phase("03-client", 0, client, "")
        version = run(["docker", "exec", CONTAINER, client, "--version"], timeout=20)
        save_text(RAW / "03-client-version.txt", version.stdout + version.stderr)
        binary_hash = run(["docker", "exec", CONTAINER, "sha256sum", client], timeout=20)
        save_text(RAW / "03-client-sha256.txt", binary_hash.stdout + binary_hash.stderr)
        add_check("native_client", version.returncode == 0, "client version and digest", version.stdout.decode("utf-8", "replace").strip(), client)
        if not wait_ready():
            write_results("not-ready")
            return 3
        capture_runtime_files()
        root_mode = {"cnf": None, "skip": True}
        if not identity(None, True):
            write_results("identity-rejected")
            return 2
        accounts = bootstrap(root_mode)
        if not accounts:
            write_results("accounts-failed")
            return 2
        if not exec_sql_file(accounts["dba"], TARGET / "sql" / "dba-load.sql", "07-dba-load", timeout=180):
            add_check("dba_load", False, "schema and rows loaded by oo_dba", "SQL error", "")
            write_results("load-failed")
            return 2
        add_check("dba_load", True, "schema and rows loaded by oo_dba", "exit 0", "")
        audit = audit_path()
        save_text(RAW / "08-audit-path.txt", audit or "MISSING")
        before = copy_audit(audit, RAW / "08-audit-before.txt")
        marked = read_tables(accounts["reader"], "08-reader-positive")
        time.sleep(1)
        after = copy_audit(audit, RAW / "08-audit-after.txt")
        new_lines = after.splitlines()[len(before.splitlines()) :]
        window = oracle_lib.reader_audit(new_lines)
        save_text(RAW / "08-audit-window.json", json.dumps(window, indent=2))
        add_check(
            "reader_audit_read_window",
            window["read_window_ok"],
            "oo_reader statements present and none are DML/DDL",
            window,
            "",
        )
        dup_ok = expect_table(marked, "dup_events", ORACLE["dup_events"]["visible_initial"], "oracle_dup_initial")
        pk_ok = expect_table(marked, "pk_accounts", ORACLE["pk_accounts"]["visible_initial"], "oracle_pk_initial")
        agg_ok = expect_table(marked, "agg_metrics", ORACLE["agg_metrics"]["visible"], "oracle_agg")
        uniq_ok = expect_table(marked, "uniq_dim", ORACLE["uniq_dim"]["visible"], "oracle_uniq")
        parts_before = show_partitions(accounts["reader"], "08-partitions-initial")
        neg_before_text = copy_audit(audit, RAW / "09-audit-before-negative.txt")
        neg_before_len = len(neg_before_text.splitlines())
        negatives_ok = reader_negatives(accounts["reader"])
        neg_after_text = copy_audit(audit, RAW / "09-audit-after-negative.txt")
        neg_after_len = len(neg_after_text.splitlines())
        neg_slice = oracle_lib.reader_audit(neg_after_text.splitlines()[neg_before_len:])
        save_text(RAW / "09-audit-negative.json", json.dumps(neg_slice, indent=2))
        neg_audit_ok = neg_slice["classes"].get("write", 0) >= 5 and neg_slice["classes"].get("control", 0) == 0
        add_check(
            "negative_audit_separate",
            neg_audit_ok,
            "rejected reader DML/DDL stay in their own audit slice",
            neg_slice["classes"],
            "",
        )
        marked_again = read_tables(accounts["reader"], "09-reader-after-negative")
        unchanged = expect_table(marked_again, "pk_accounts", ORACLE["pk_accounts"]["visible_initial"], "oracle_pk_unchanged_after_negative")
        append_ok = exec_sql_file(accounts["dba"], TARGET / "sql" / "dba-append.sql", "10-dba-append")
        marked_append = read_tables(accounts["reader"], "10-reader-after-append")
        after_rows = oracle_lib.visible_after_append(ORACLE["dup_events"]["visible_initial"], ORACLE["dup_events"]["append"])
        append_match = expect_table(marked_append, "dup_events", after_rows, "oracle_dup_after_append")
        pk_stable = expect_table(marked_append, "pk_accounts", ORACLE["pk_accounts"]["visible_initial"], "oracle_pk_stable_during_append")
        parts_after = show_partitions(accounts["reader"], "10-partitions-after")
        add_check(
            "partition_metadata_captured",
            "p202601" in parts_before and "p202602" in parts_before and "p202601" in parts_after and "p202602" in parts_after,
            "both partitions listed before and after",
            {"before_has_versions": "VisibleVersion" in parts_before, "after_has_versions": "VisibleVersion" in parts_after},
            "row oracle is the consistency proof; VisibleVersion is recorded raw",
        )
        delete_ok = exec_sql_file(accounts["dba"], TARGET / "sql" / "dba-delete.sql", "10-dba-delete")
        marked_delete = read_tables(accounts["reader"], "10-reader-after-delete")
        delete_match = expect_table(marked_delete, "pk_accounts", ORACLE["pk_accounts"]["visible_after_delete"], "oracle_pk_after_delete")
        caps_ok = resource_caps()
        cancel_ok = cancel_query(accounts["reader"])
        final_audit = copy_audit(audit, RAW / "11-audit-final.txt")
        final_lines = final_audit.splitlines()
        final_classes = oracle_lib.reader_audit(final_lines)
        save_text(RAW / "11-audit-reader.json", json.dumps(final_classes, indent=2))
        ops_lines = final_lines[len(before.splitlines()) : neg_before_len] + final_lines[neg_after_len:]
        ops = oracle_lib.reader_audit(ops_lines)
        save_text(RAW / "11-audit-operational.json", json.dumps(ops, indent=2))
        ops_ok = (
            ops["read_window_ok"]
            and ops["classes"].get("control", 0) >= 1
            and ops["classes"].get("write", 0) == 0
        )
        add_check(
            "operational_path_zero_write",
            ops_ok,
            "reader operational statements through cancel contain reads and KILL and no DML/DDL",
            ops,
            "",
        )
        required = [
            dup_ok,
            pk_ok,
            agg_ok,
            uniq_ok,
            negatives_ok,
            unchanged,
            neg_audit_ok,
            append_ok,
            append_match,
            pk_stable,
            delete_ok,
            delete_match,
            window["read_window_ok"],
            caps_ok,
            cancel_ok,
            ops_ok,
        ]
        outcome = "prepared" if all(required) else "checks-failed"
        write_results(outcome)
        return 0 if outcome == "prepared" else 2
    finally:
        cleanup()
        if not (EVIDENCE / "results.json").exists():
            write_results("cleanup-after-error")
        else:
            # Refresh cleanup check into the already written results.
            payload = json.loads((EVIDENCE / "results.json").read_text(encoding="utf-8"))
            payload["phases"] = PHASES
            payload["checks"] = CHECKS
            payload["when_finished"] = now()
            (EVIDENCE / "results.json").write_text(json.dumps(payload, indent=2, default=str) + "\n", encoding="utf-8")


if __name__ == "__main__":
    raise SystemExit(main())
