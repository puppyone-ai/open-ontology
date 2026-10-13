#!/usr/bin/env python3
"""Run the DL01-W1-45 StarRocks product loop on the host.

StarRocks is admitted, read, and released before Postgres starts. The already
built Linux CLI runs inside that one Postgres container, so the task does not
add a second CPU. Secrets stay in mode 600 files and are scrubbed from the
copied evidence. This script does not compile.
"""

from __future__ import annotations

import importlib.util
import json
import os
import shutil
import socket
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

REPO = Path("/Users/redux/puppy-one/open-ontology")
SESSION = "2c023948-0862-4dae-ae8f-6fc30241db62"
TASK = "DL01-W1-45"
PREFIX = "oo-dl01w1-45"
EVIDENCE = REPO / "evidence" / SESSION / "DL01" / "starrocks-product"
EXTRACT = EVIDENCE / "extract"
RUN = EVIDENCE / "run"
PROBE = (
    REPO
    / "evidence"
    / SESSION
    / "DL01"
    / "starrocks-driver-preflight"
    / "build"
    / "bin"
    / "starrocks-probe"
)
DECODE = PROBE.with_name("starrocks-decode")
BINS = EVIDENCE / "build" / "bin"
GATE_PATH = REPO / "testing" / "dl01-driver-preflight" / "starrocks" / "scripts" / "run_gate.py"
ADMIT = GATE_PATH.with_name("admit.py")
PROXY = Path(__file__).with_name("loopback_proxy.pl")
PG_IMAGE = "docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb"
RUST_IMAGE = "docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
PG_NAME = f"{PREFIX}-postgres"
LIBSRC = f"{PREFIX}-libsrc"
PG_PORT = 15445
SECRET_DIR = Path("/tmp/oo-dl01w1-45-pg-secrets")
TOKEN_SRC = Path("/tmp/oo-customer-demo-20261007/llm-token")
ARROW_NAMES = (
    "agg_metrics.arrow",
    "dup_events.arrow",
    "dup_partition_sums.arrow",
    "pk_accounts.arrow",
    "uniq_dim.arrow",
)
SECRETS: list[bytes] = []


def now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def say(text: str) -> None:
    print(f"{now()} {text}", flush=True)


def run(cmd: list[str], timeout: int = 180) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout, check=False)


def save_bytes(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(scrub_bytes(data))


def scrub_bytes(data: bytes) -> bytes:
    for secret in SECRETS:
        if secret:
            data = data.replace(secret, b"[redacted]")
    return data


def scrub_tree(root: Path) -> None:
    if not root.exists():
        return
    for path in root.rglob("*"):
        if not path.is_file() or path.stat().st_size > 32 * 1024 * 1024:
            continue
        data = path.read_bytes()
        cleaned = scrub_bytes(data)
        if cleaned != data:
            path.write_bytes(cleaned)


def load_gate():
    spec = importlib.util.spec_from_file_location("sr_w45_gate", GATE_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError("run_gate.py missing")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    module.EVIDENCE = EXTRACT
    return module


def capacity_wait(kind: str, evidence_name: str, start) -> bool:
    path = EXTRACT if kind == "starrocks" else RUN
    path.mkdir(parents=True, exist_ok=True)
    record = path / evidence_name
    for attempt in range(1, 5):
        ok = start()
        body = {}
        if record.is_file():
            try:
                body = json.loads(record.read_text(encoding="utf-8"))
            except json.JSONDecodeError:
                body = {}
        if ok:
            say(f"{kind} admitted attempt {attempt}")
            return True
        reason = str(body.get("reason") or "")
        blocked = body.get("admitted") is False and any(
            token in reason for token in ("CPU", "headroom", "rust", "already has")
        )
        say(f"{kind} admit refused attempt {attempt}: {reason or 'see admission file'}")
        if not blocked or attempt == 4:
            return False
        say(f"CAPACITY_WAIT 900 {kind} attempt {attempt}")
        time.sleep(900)
    return False


def remember_secret(text: str) -> None:
    raw = text.encode("utf-8")
    SECRETS.append(raw)
    if not raw.endswith(b"\n"):
        SECRETS.append(raw + b"\n")


def write_secret(name: str, text: str) -> Path:
    SECRET_DIR.mkdir(mode=0o700, exist_ok=True)
    os.chmod(SECRET_DIR, 0o700)
    path = SECRET_DIR / name
    path.write_text(text, encoding="utf-8")
    os.chmod(path, 0o600)
    remember_secret(text)
    return path


def make_certs() -> None:
    SECRET_DIR.mkdir(mode=0o700, exist_ok=True)
    san = SECRET_DIR / "san.cnf"
    san.write_text(
        "subjectAltName=DNS:localhost,IP:127.0.0.1\n"
        "basicConstraints=CA:FALSE\n"
        "keyUsage=digitalSignature,keyEncipherment\n"
        "extendedKeyUsage=serverAuth\n",
        encoding="utf-8",
    )
    steps = [
        ["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", str(SECRET_DIR / "ca.key"), "-out", str(SECRET_DIR / "ca.crt"), "-days", "3", "-subj", "/CN=oo-dl01w1-45-ca"],
        ["openssl", "req", "-new", "-newkey", "rsa:2048", "-nodes", "-keyout", str(SECRET_DIR / "server.key"), "-out", str(SECRET_DIR / "server.csr"), "-subj", "/CN=localhost"],
        ["openssl", "x509", "-req", "-in", str(SECRET_DIR / "server.csr"), "-CA", str(SECRET_DIR / "ca.crt"), "-CAkey", str(SECRET_DIR / "ca.key"), "-CAcreateserial", "-out", str(SECRET_DIR / "server.crt"), "-days", "3", "-extfile", str(san)],
    ]
    for cmd in steps:
        proc = run(cmd, timeout=30)
        if proc.returncode != 0:
            save_bytes(RUN / "openssl.stderr", proc.stderr)
            raise RuntimeError("openssl failed")
    for name in ("ca.key", "server.key", "ca.crt", "server.crt"):
        os.chmod(SECRET_DIR / name, 0o600 if name.endswith(".key") else 0o644)


def port_free(port: int) -> bool:
    sock = socket.socket()
    try:
        sock.bind(("127.0.0.1", port))
        return True
    except OSError:
        return False
    finally:
        sock.close()


def extract(gate) -> int:
    if not PROBE.is_file() or not DECODE.is_file():
        say("probe binary missing")
        return 1
    EXTRACT.mkdir(parents=True, exist_ok=True)
    if not capacity_wait("starrocks", "admission.json", gate.start_container):
        return 1
    try:
        if not gate.wait_ready() and not gate.wait_ready():
            say("starrocks not ready")
            return 1
        if not gate.identity():
            say("starrocks identity refused")
            return 1
        accounts = gate.create_accounts()
        if not gate.load_fixture(accounts["dba"]):
            say("fixture load failed")
            return 1
        gate.install(PROBE, "/tmp/starrocks-probe", "755")
        gate.install(DECODE, "/tmp/starrocks-decode", "755")
        code = gate.run_probe(
            "/tmp/starrocks-probe",
            [
                "read",
                "--host",
                "127.0.0.1",
                "--port",
                "9030",
                "--password-file",
                "/tmp/oo-secrets/reader.password",
                "--image-digest",
                gate.PIN,
            ],
            "read",
            timeout=180,
        )
        names = sorted(path.name for path in (EXTRACT / "read").glob("*.arrow"))
        (EXTRACT / "arrow-names.txt").write_text("\n".join(names) + "\n", encoding="utf-8")
        (EXTRACT / "read.exit").write_text(f"{code}\n", encoding="utf-8")
        say(f"extract read exit {code} arrows {names}")
        if code != 0 or any(name not in names for name in ARROW_NAMES):
            return 1
        digest = subprocess.check_output(["shasum", "-a", "256", str(PROBE)], text=True)
        (EXTRACT / "probe.sha256").write_text(digest, encoding="utf-8")
        return 0
    finally:
        gate.release()
        stopped = run(["docker", "ps", "-aq", "--filter", f"name=^{gate.CONTAINER}$"])
        proof = {
            "at": now(),
            "container_ids": stopped.stdout.decode().split(),
            "source_stopped": stopped.stdout.strip() == b"",
        }
        (EXTRACT / "source-stopped.json").write_text(json.dumps(proof, indent=2) + "\n", encoding="utf-8")
        say(f"starrocks stopped {proof['source_stopped']}")


def start_postgres() -> bool:
    if not port_free(PG_PORT):
        say(f"port {PG_PORT} is in use")
        (RUN / "port.json").write_text(json.dumps({"port": PG_PORT, "free": False}) + "\n", encoding="utf-8")
        return False
    command = [
        "docker", "run", "-d", "--name", PG_NAME,
        "--platform", "linux/arm64", "--network", "host",
        "--memory", "1g", "--memory-swap", "1g", "--cpus", "1",
        "--restart", "no", "--pull", "never",
        "--label", "project=open-ontology",
        "--label", f"owner={SESSION}",
        "--label", f"session={SESSION}",
        "--label", "kind=postgres-target",
        "--label", f"oo.prefix={PREFIX}",
        "--label", f"oo.task={TASK}",
        "-e", "POSTGRES_PASSWORD_FILE=/secrets/admin.password",
        "-e", "POSTGRES_DB=oo_w45",
        "-e", "POSTGRES_INITDB_ARGS=--auth-host=scram-sha-256 --auth-local=scram-sha-256",
        "--entrypoint", "bash",
        PG_IMAGE,
        "-c",
        "set -eu; while [ ! -f /certs/ready ]; do sleep 0.2; done; "
        "chown postgres:postgres /certs/server.key /certs/server.crt /certs/ca.crt "
        "/secrets/admin.password /secrets/login.password /secrets/llm-token /secrets/bearer-bad; "
        "chmod 600 /certs/server.key /secrets/admin.password /secrets/login.password /secrets/llm-token /secrets/bearer-bad; "
        "chmod 644 /certs/server.crt /certs/ca.crt; "
        "export PATH=\"$(echo /usr/lib/postgresql/*/bin):$PATH\"; "
        "exec docker-entrypoint.sh postgres -c ssl=on -c ssl_cert_file=/certs/server.crt "
        "-c ssl_key_file=/certs/server.key -c port=15445 -c listen_addresses='*' -c shared_buffers=64MB",
    ]

    def once() -> bool:
        proc = run([sys.executable, str(ADMIT), "acquire-start", "postgres", str(RUN / "admission.json"), "--", *command], timeout=120)
        save_bytes(RUN / "admission.stdout", proc.stdout)
        save_bytes(RUN / "admission.stderr", proc.stderr)
        (RUN / "admission.exit").write_text(f"{proc.returncode}\n", encoding="utf-8")
        return proc.returncode == 0

    return capacity_wait("postgres", "admission.json", once)


def copy_in(src: Path, dest: str) -> None:
    proc = run(["docker", "cp", str(src), f"{PG_NAME}:{dest}"], timeout=180)
    if proc.returncode != 0:
        save_bytes(RUN / "docker-cp.stderr", proc.stderr)
        raise RuntimeError(f"docker cp {src.name} failed")


def install_compat_libs() -> None:
    """The pinned Postgres image is Debian 12. The Linux 1.94 binaries need glibc 2.39.

    Copy the loader and libc from the pinned rust image onto new inodes after
    Postgres is already running. A created container is not a running builder.
    """
    run(["docker", "rm", "-f", LIBSRC], timeout=30)
    created = run(
        ["docker", "create", "--name", LIBSRC, "--platform", "linux/arm64", "--pull", "never", RUST_IMAGE],
        timeout=60,
    )
    save_bytes(RUN / "glibc-create.stderr", created.stderr)
    if created.returncode != 0:
        raise RuntimeError("glibc source create failed")
    host = RUN / "glibc-from-rust-image"
    host.mkdir(parents=True, exist_ok=True)
    remote_names = {
        "ld-linux-aarch64.so.1": "/usr/lib/aarch64-linux-gnu/ld-linux-aarch64.so.1",
        "libc.so.6": "/usr/lib/aarch64-linux-gnu/libc.so.6",
        "libgcc_s.so.1": "/usr/lib/aarch64-linux-gnu/libgcc_s.so.1",
    }
    try:
        for local, remote in remote_names.items():
            proc = run(["docker", "cp", f"{LIBSRC}:{remote}", str(host / local)], timeout=60)
            if proc.returncode != 0 or (host / local).is_symlink():
                save_bytes(RUN / "glibc-cp.stderr", proc.stderr)
                raise RuntimeError(f"glibc copy {local} failed")
    finally:
        run(["docker", "rm", "-f", LIBSRC], timeout=30)
    run(["docker", "exec", PG_NAME, "mkdir", "-p", "/opt/glibc"])
    for local in remote_names:
        copy_in(host / local, f"/opt/glibc/{local}")
    # One perl process renames loader, libgcc, and libc. A later exec would pair the new loader with the old libc.
    script = (
        "set -eu; "
        "cp -a /opt/glibc/ld-linux-aarch64.so.1 /usr/lib/aarch64-linux-gnu/ld-linux-aarch64.so.1.new; "
        "cp -a /opt/glibc/libgcc_s.so.1 /usr/lib/aarch64-linux-gnu/libgcc_s.so.1.new; "
        "cp -a /opt/glibc/libc.so.6 /usr/lib/aarch64-linux-gnu/libc.so.6.new; "
        "ln -sfn /usr/lib/aarch64-linux-gnu/ld-linux-aarch64.so.1 /usr/lib/ld-linux-aarch64.so.1; "
        "ln -sfn /usr/lib/aarch64-linux-gnu/ld-linux-aarch64.so.1 /lib/ld-linux-aarch64.so.1; "
        "perl -e 'for (my $i = 0; $i < @ARGV; $i += 2) { rename($ARGV[$i], $ARGV[$i + 1]) or die $!; }' "
        "/usr/lib/aarch64-linux-gnu/ld-linux-aarch64.so.1.new /usr/lib/aarch64-linux-gnu/ld-linux-aarch64.so.1 "
        "/usr/lib/aarch64-linux-gnu/libgcc_s.so.1.new /usr/lib/aarch64-linux-gnu/libgcc_s.so.1 "
        "/usr/lib/aarch64-linux-gnu/libc.so.6.new /usr/lib/aarch64-linux-gnu/libc.so.6"
    )
    proc = run(["docker", "exec", PG_NAME, "bash", "-c", script], timeout=30)
    save_bytes(RUN / "glibc-install.stderr", proc.stderr)
    save_bytes(RUN / "glibc-install.stdout", proc.stdout)
    (RUN / "glibc-install.exit").write_text(f"{proc.returncode}\n", encoding="utf-8")
    if proc.returncode != 0:
        raise RuntimeError("glibc install failed")
    (RUN / "glibc-provenance.json").write_text(
        json.dumps(
            {
                "rust_image": RUST_IMAGE,
                "postgres_image": PG_IMAGE,
                "reason": "pinned postgres libc is older than GLIBC_2.39 required by the Linux 1.94 binaries",
                "files": sorted(remote_names),
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )


def wait_postgres() -> bool:
    deadline = time.time() + 180
    last = b""
    while time.time() < deadline:
        state = run(["docker", "inspect", "-f", "{{.State.Status}} {{.State.OOMKilled}}", PG_NAME])
        text = state.stdout.decode().strip()
        if not text.startswith("running"):
            save_bytes(RUN / "pg-state.txt", state.stdout + state.stderr)
            say(f"postgres state {text}")
            return False
        probe = run(["docker", "exec", PG_NAME, "pg_isready", "-h", "127.0.0.1", "-p", str(PG_PORT), "-d", "oo_w45"])
        last = probe.stdout + probe.stderr
        if probe.returncode == 0:
            save_bytes(RUN / "pg-ready.txt", last)
            return True
        time.sleep(2)
    save_bytes(RUN / "pg-ready.txt", last)
    return False


def start_model_proxy() -> None:
    copy_in(PROXY, "/opt/oo/loopback_proxy.pl")
    started = run(["docker", "exec", "-d", PG_NAME, "perl", "/opt/oo/loopback_proxy.pl"], timeout=30)
    save_bytes(RUN / "model-proxy.stderr", started.stderr)
    (RUN / "model-proxy.exit").write_text(f"{started.returncode}\n", encoding="utf-8")
    if started.returncode != 0:
        raise RuntimeError("model loopback proxy did not start")
    status = ""
    for _ in range(20):
        probe = run(
            [
                "docker", "exec", PG_NAME, "bash", "-c",
                "exec 3<>/dev/tcp/127.0.0.1/8317 && printf 'GET /v1/models HTTP/1.1\\r\\nHost: 127.0.0.1:8317\\r\\nConnection: close\\r\\n\\r\\n' >&3 && IFS= read -r line <&3 && printf '%s' \"$line\"",
            ],
            timeout=10,
        )
        status = probe.stdout.decode("utf-8", "replace").strip()
        if status.startswith("HTTP/"):
            break
        time.sleep(0.2)
    (RUN / "model-proxy.status").write_text(status + "\n", encoding="utf-8")
    say(f"model loopback status {status or 'closed'}")
    if not status.startswith("HTTP/"):
        raise RuntimeError("model loopback did not answer")


def product(gate) -> int:
    stopped = json.loads((EXTRACT / "source-stopped.json").read_text(encoding="utf-8"))
    if not stopped.get("source_stopped"):
        say("refusing source_detached while StarRocks is running")
        return 1
    if not TOKEN_SRC.is_file() or TOKEN_SRC.stat().st_size == 0:
        say("model token file missing")
        return 1
    if RUN.exists():
        shutil.rmtree(RUN)
    RUN.mkdir(parents=True, exist_ok=True)
    admin = os.urandom(24).hex()
    login = os.urandom(24).hex()
    write_secret("admin.password", admin)
    write_secret("login.password", login)
    write_secret("bearer-bad", "denied")
    make_certs()
    token_copy = SECRET_DIR / "llm-token"
    shutil.copyfile(TOKEN_SRC, token_copy)
    os.chmod(token_copy, 0o600)
    remember_secret(token_copy.read_text(encoding="utf-8"))
    if not start_postgres():
        say("postgres not admitted")
        return 1
    try:
        made = run(["docker", "exec", PG_NAME, "mkdir", "-p", "/certs", "/secrets", "/opt/oo/bin", "/work/arrow", "/work/out"])
        if made.returncode != 0:
            save_bytes(RUN / "mkdir.stderr", made.stderr)
            raise RuntimeError("mkdir inside postgres failed")
        for name in ("ca.crt", "server.crt", "server.key"):
            copy_in(SECRET_DIR / name, f"/certs/{name}")
        for name in ("admin.password", "login.password", "llm-token", "bearer-bad"):
            copy_in(SECRET_DIR / name, f"/secrets/{name}")
        bin_names = {
            "starrocks-preview": BINS / "starrocks-preview",
            "oo-source-admission": BINS / "oo-source-admission",
            "starrocks-decode": DECODE,
            "init-pg": BINS / "init-pg",
            "oo-pipeline": BINS / "oo-pipeline",
            "pg-builder": BINS / "pg-builder",
            "install-column-acl": BINS / "install-column-acl",
            "operator-preview": BINS / "operator-preview",
            "starrocks-probe": PROBE,
        }
        for remote, local in bin_names.items():
            copy_in(local, f"/opt/oo/bin/{remote}")
            run(["docker", "exec", PG_NAME, "chmod", "755", f"/opt/oo/bin/{remote}"])
        for name in ARROW_NAMES:
            copy_in(EXTRACT / "read" / name, f"/work/arrow/{name}")
        config = {
            "protocol": "oo.starrocks-preview.config/v1",
            "image_digest": gate.PIN,
            "secret_ref": "secret://oo-dl01w1-45/reader",
            "row_cap": 1000,
            "role": "oo_r_w45",
            "login": "oo_w45_login",
            "dataset": "oo_w45_sr.pk_accounts",
            "reviewer": "w45-source-owner",
            "reviewed_at": "2026-10-10",
            "source_detached": True,
            "probe_bin": "/opt/oo/bin/starrocks-probe",
            "arrow_dir": "/work/arrow",
            "pg": {
                "host": "127.0.0.1",
                "port": PG_PORT,
                "database": "oo_w45",
                "admin_user": "postgres",
                "admin_password_file": "/secrets/admin.password",
                "login_password_file": "/secrets/login.password",
                "ca_file": "/certs/ca.crt",
            },
            "model": {
                "origin": "http://127.0.0.1:8317",
                "bearer_file": "/secrets/llm-token",
                "bad_bearer_file": "/secrets/bearer-bad",
            },
            "bins": {
                "admission": "/opt/oo/bin/oo-source-admission",
                "decode": "/opt/oo/bin/starrocks-decode",
                "init_pg": "/opt/oo/bin/init-pg",
                "pipeline": "/opt/oo/bin/oo-pipeline",
                "builder": "/opt/oo/bin/pg-builder",
                "acl": "/opt/oo/bin/install-column-acl",
                "operator": "/opt/oo/bin/operator-preview",
            },
        }
        config_path = RUN / "config.json"
        config_path.write_text(json.dumps(config, indent=2) + "\n", encoding="utf-8")
        copy_in(config_path, "/work/config.json")
        ready = run(["docker", "exec", PG_NAME, "touch", "/certs/ready"])
        if ready.returncode != 0:
            save_bytes(RUN / "ready-touch.stderr", ready.stderr)
            return 1
        if not wait_postgres():
            say("postgres did not become ready")
            return 1
        install_compat_libs()
        say("compat libc installed")
        start_model_proxy()
        ldd = run(["docker", "exec", PG_NAME, "ldd", "/opt/oo/bin/starrocks-preview"], timeout=30)
        save_bytes(RUN / "ldd.txt", ldd.stdout + ldd.stderr)
        (RUN / "ldd.exit").write_text(f"{ldd.returncode}\n", encoding="utf-8")
        say("product cli start")
        proc = run(
            [
                "docker", "exec", PG_NAME,
                "/opt/oo/bin/starrocks-preview", "run",
                "--config", "/work/config.json",
                "--out", "/work/out",
            ],
            timeout=900,
        )
        save_bytes(RUN / "cli.stdout", proc.stdout)
        save_bytes(RUN / "cli.stderr", proc.stderr)
        (RUN / "cli.exit").write_text(f"{proc.returncode}\n", encoding="utf-8")
        copied = run(["docker", "cp", f"{PG_NAME}:/work/out/.", str(RUN / "out")], timeout=180)
        save_bytes(RUN / "out-copy.stderr", copied.stderr)
        (RUN / "out-copy.exit").write_text(f"{copied.returncode}\n", encoding="utf-8")
        say(f"product cli exit {proc.returncode}")
        return proc.returncode
    finally:
        logs = run(["docker", "logs", PG_NAME], timeout=60)
        save_bytes(RUN / "postgres.log", logs.stdout + b"\n" + logs.stderr)
        inspect = run(["docker", "inspect", PG_NAME], timeout=30)
        save_bytes(RUN / "postgres-inspect.json", inspect.stdout or inspect.stderr)
        scrub_tree(RUN)
        scrub_tree(EXTRACT)


def cleanup() -> None:
    run(["docker", "rm", "-f", f"{PREFIX}-starrocks", PG_NAME, LIBSRC], timeout=60)
    run([sys.executable, str(ADMIT), "release", "starrocks"])
    run([sys.executable, str(ADMIT), "release", "postgres"])
    run([sys.executable, str(ADMIT), "release", "all"])


def main() -> int:
    gate = load_gate()
    code = 1
    try:
        code = extract(gate)
        if code == 0:
            code = product(gate)
        else:
            say(f"extract exit {code}")
    except subprocess.TimeoutExpired as exc:
        say("timeout during product runner")
        RUN.mkdir(parents=True, exist_ok=True)
        (RUN / "timeout.txt").write_text(scrub_bytes(str(exc).encode()).decode("utf-8", "replace"), encoding="utf-8")
        code = 1
    except Exception as exc:
        say(f"runner error {type(exc).__name__}: {exc}")
        (RUN / "runner-error.txt").write_text(f"{type(exc).__name__}: {exc}\n", encoding="utf-8")
        code = 1
    finally:
        cleanup()
        say(f"cleanup done exit {code}")
    return code


if __name__ == "__main__":
    sys.exit(main())
