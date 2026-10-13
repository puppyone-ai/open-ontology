"""Host orchestrator for the Oracle source consumption loop.

The historical main() seeded previewdb from catalog.GOLD. load_rows now
refuses that insert. No public Loader API seals the native Arrow batch.
confirm_oracle reads FREEPDB1 and fails when sqlplus returns ORA- or nonzero.
Do not run main(): it drops previewdb when schema-applied.txt is removed.
"""

from __future__ import annotations

import fcntl
import json
import os
import secrets
import socket
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import catalog  # noqa: E402

ROOT = Path("/Users/redux/puppy-one/open-ontology")
EVIDENCE = ROOT / "evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/driver/linux/product-loop"
BINS = ROOT / "evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/driver/linux/product-compile-4/bin"
MIGRATIONS = ROOT / "crates/oo-pipeline/migrations"
SECRET_DIR = Path("/Users/redux/.oo-dl01w1-04-p2")
TOKEN = Path("/tmp/oo-customer-demo-20261007/llm-token")
PG_IMAGE = "docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb"
RUST_IMAGE = "docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
PG_NAME = "oo-dl01w1-04-p2-pg"
PRODUCT_NAME = "oo-dl01w1-04-p2-product"
ORACLE_NAME = "oo-dl01w1-04-p2-oracle"
# Owned non-internal bridge. A new network cannot be created: Docker's address
# pools are fully subnetted. Oracle stays on the internal oo-dl01w1-04-p2-src.
NETWORK = "oo-dl01w1-04-net"
VOLUME = "oo-dl01w1-04-p2-pgdata"
PG_HOST = PG_NAME
DATABASE = "previewdb"
LOGIN = "oo_preview_login"
SHARD = "shard_oo04"
OWNER = "992aa5d3-6848-4dd9-aff1-d4d31a099983"
LABELS = [
    "--label", "oo.project=open-ontology",
    "--label", "oo.owner=dl01w1-04",
    "--label", "oo.session=992aa5d3-6848-4dd9-aff1-d4d31a099983",
    "--label", "oo.task=DL01-W1-04",
]
REPAIR_EVIDENCE = ROOT / "evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/repair-1"
ARROW_BATCH = ROOT / "evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/driver/linux/p2-native/audit/batch/batch.arrow"
REJECTED_LITERAL_DIGEST = "a6236974c0d71bfc372587e84e39d4dcf93aa994005700157854294abed47aa3"
DECODER_GOLD_FINGERPRINT = "cd9f0d7d7ce31bc2a6ddc2ce3b7451f7517f4b452510647e83e12e584f4ddd1d"
MISSING_LOADER_API = (
    "No existing public implemented Loader API accepts this native Oracle Arrow IPC batch, "
    "binds input_digest to those bytes, keeps ID Int16, N16 Decimal128(16,6), N1 Int8, "
    "D Timestamp(Microsecond, None), C6 Utf8, V2 Utf8, and R Binary, and seals _oo.full_import_run. "
    "oo-pipeline commands are import-pg (oo.pipeline.import-pg/v1, a live PostgreSQL snapshot), "
    "catalog-pg, catalog-import, and install-sealed-catalog. "
    "pg-builder run_import accepts only manifest protocol oo.mysql-preview.manifest/v1. "
    "full_import synthetic_plan has no Int8 oid. "
    "This orchestration does not invent a loader, does not insert catalog.GOLD, and does not open a new run."
)


def run(args: list[str], *, cwd: Path | None = None, stdin: bytes | None = None, timeout: int = 180) -> subprocess.CompletedProcess:
    return subprocess.run(args, input=stdin, text=False, stdout=subprocess.PIPE, stderr=subprocess.PIPE, cwd=cwd, timeout=timeout)


def save(name: str, proc: subprocess.CompletedProcess) -> int:
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    (EVIDENCE / f"{name}.stdout").write_bytes(proc.stdout)
    (EVIDENCE / f"{name}.stderr").write_bytes(proc.stderr)
    (EVIDENCE / f"{name}.exit").write_text(f"{proc.returncode}\n")
    return proc.returncode


def docker(args: list[str], name: str, timeout: int = 180) -> subprocess.CompletedProcess:
    proc = run(["docker", *args], timeout=timeout)
    save(name, proc)
    return proc


def write_secret(path: Path) -> None:
    if path.exists():
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    alphabet = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
    value = "".join(secrets.choice(alphabet) for _ in range(24))
    path.write_text(value)
    os.chmod(path, 0o600)


def certificate_text(path: Path) -> str:
    proc = run(["openssl", "x509", "-in", str(path), "-noout", "-text"])
    if proc.returncode != 0:
        return ""
    return proc.stdout.decode(errors="replace")


def server_cert_ok(crt: Path, ca: Path) -> bool:
    if not crt.exists() or not ca.exists() or not crt.is_file() or not ca.is_file():
        return False
    if crt.read_bytes() == ca.read_bytes():
        return False
    text = certificate_text(crt)
    return f"DNS:{PG_HOST}" in text and "CA:TRUE" not in text


def ensure_certs() -> bool:
    # rustls rejects a CA certificate presented as the server leaf. The client
    # trusts a separate CA. The leaf is CA:FALSE and carries the Postgres DNS SAN.
    cert_dir = SECRET_DIR / "pg-tls"
    cert_dir.mkdir(parents=True, exist_ok=True)
    crt = cert_dir / "server.crt"
    key = cert_dir / "server.key"
    ca = cert_dir / "ca.crt"
    if server_cert_ok(crt, ca) and key.exists():
        return False
    ca_key = cert_dir / "ca.key"
    csr = cert_dir / "server.csr"
    ext = cert_dir / "leaf.ext"
    ca_ext = cert_dir / "ca.ext"
    serial = cert_dir / "ca.srl"
    ca_ext.write_text(
        "[req]\n"
        "distinguished_name=dn\n"
        "x509_extensions=v3_ca\n"
        "prompt=no\n"
        "[dn]\n"
        f"CN={PG_HOST}-ca\n"
        "[v3_ca]\n"
        "basicConstraints=critical,CA:TRUE\n"
        "keyUsage=critical,keyCertSign,cRLSign\n"
    )
    ext.write_text(
        "basicConstraints=critical,CA:FALSE\n"
        f"subjectAltName=DNS:{PG_HOST}\n"
        "keyUsage=critical,digitalSignature,keyEncipherment\n"
        "extendedKeyUsage=serverAuth\n"
    )
    commands = [
        [
            "openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes",
            "-keyout", str(ca_key), "-out", str(ca), "-days", "2",
            "-config", str(ca_ext),
        ],
        [
            "openssl", "req", "-new", "-newkey", "rsa:2048", "-nodes",
            "-keyout", str(key), "-out", str(csr),
            "-subj", f"/CN={PG_HOST}",
        ],
        [
            "openssl", "x509", "-req", "-in", str(csr),
            "-CA", str(ca), "-CAkey", str(ca_key), "-CAcreateserial",
            "-out", str(crt), "-days", "2",
            "-extfile", str(ext),
        ],
    ]
    for command in commands:
        proc = run(command)
        if proc.returncode != 0:
            raise SystemExit(proc.stderr.decode())
    os.chmod(key, 0o600)
    os.chmod(ca_key, 0o600)
    csr.unlink(missing_ok=True)
    ext.unlink(missing_ok=True)
    ca_ext.unlink(missing_ok=True)
    serial.unlink(missing_ok=True)
    if "CA:TRUE" not in certificate_text(ca) or not server_cert_ok(crt, ca):
        raise SystemExit("server certificate is not a leaf with the Postgres DNS SAN")
    return True


def proxy_script() -> str:
    # pg-builder reads until EOF. Forward one request, read one complete
    # response, then close the client. Do not half-close the upstream write
    # side: this endpoint treats that FIN as a canceled completion.
    return r'''
import os, socket, threading
LIMIT = 8 * 1024 * 1024
UPSTREAM = os.environ.get("OO_PROXY_UPSTREAM", "host.docker.internal")
UPSTREAM_PORT = int(os.environ.get("OO_PROXY_UPSTREAM_PORT", "8317"))
LISTEN_PORT = int(os.environ.get("OO_PROXY_LISTEN", "8317"))

def read_exact(sock, n, buf):
    while len(buf) < n:
        chunk = sock.recv(min(65536, n - len(buf)))
        if not chunk:
            raise OSError("eof")
        buf += chunk
        if len(buf) > LIMIT:
            raise OSError("limit")
    return buf

def read_http(sock):
    buf = bytearray()
    while b"\r\n\r\n" not in buf:
        chunk = sock.recv(65536)
        if not chunk:
            raise OSError("eof")
        buf += chunk
        if len(buf) > LIMIT:
            raise OSError("limit")
    split = buf.index(b"\r\n\r\n")
    head = bytes(buf[:split])
    body = bytearray(buf[split + 4:])
    length = None
    chunked = False
    for line in head.decode("iso-8859-1").split("\r\n")[1:]:
        if ":" not in line:
            continue
        name, value = line.split(":", 1)
        if name.lower() == "content-length":
            length = int(value.strip())
        elif name.lower() == "transfer-encoding" and "chunked" in value.lower():
            chunked = True
    if length is not None:
        if length < 0 or length > LIMIT:
            raise OSError("limit")
        body = read_exact(sock, length, body)[:length]
    elif chunked:
        while b"\r\n0\r\n\r\n" not in b"\r\n" + bytes(body):
            chunk = sock.recv(65536)
            if not chunk:
                break
            body += chunk
            if len(body) > LIMIT:
                raise OSError("limit")
    else:
        while True:
            chunk = sock.recv(65536)
            if not chunk:
                break
            body += chunk
            if len(body) > LIMIT:
                raise OSError("limit")
    return head + b"\r\n\r\n" + bytes(body)

def handle(client):
    remote = None
    try:
        client.settimeout(180)
        request = read_http(client)
        remote = socket.create_connection((UPSTREAM, UPSTREAM_PORT), 10)
        remote.settimeout(180)
        remote.sendall(request)
        response = read_http(remote)
        client.sendall(response)
    except Exception:
        pass
    finally:
        for sock in (client, remote):
            if sock is None:
                continue
            try:
                sock.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            try:
                sock.close()
            except OSError:
                pass

server = socket.socket()
server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
server.bind(("127.0.0.1", LISTEN_PORT))
server.listen(32)
open("/work/proxy-ready", "w").write("ready\n")
while True:
    client, _addr = server.accept()
    threading.Thread(target=handle, args=(client,), daemon=True).start()
'''


def psql(sql: str, name: str, *, database: str = DATABASE, extra: list[str] | None = None) -> subprocess.CompletedProcess:
    cmd = ["docker", "exec", "-i", "-u", "postgres", PG_NAME, "psql", "-d", database, "-v", "ON_ERROR_STOP=1", "-X", "-q"]
    if extra:
        cmd.extend(extra)
    proc = run(cmd, stdin=sql.encode(), timeout=180)
    save(name, proc)
    return proc


def psql_file(path: Path, name: str) -> None:
    proc = psql(path.read_text(), name)
    if proc.returncode != 0:
        raise SystemExit(f"{name} failed")


def start_pg() -> None:
    write_secret(SECRET_DIR / "pg-admin")
    write_secret(SECRET_DIR / "pg-login")
    rewritten = ensure_certs()
    (EVIDENCE / "pg-network-choice.txt").write_text(
        f"network={NETWORK}\ninternal=false\noracle_network=oo-dl01w1-04-p2-src\n"
        "reason=docker address pools are fully subnetted; reused the owned compile bridge\n"
    )
    if run(["docker", "network", "inspect", NETWORK]).returncode != 0:
        proc = docker([
            "network", "create", *LABELS, "--label", "oo.kind=network", NETWORK
        ], "pg-network")
        if proc.returncode != 0:
            raise SystemExit("network")
    if run(["docker", "volume", "inspect", VOLUME]).returncode != 0:
        proc = docker([
            "volume", "create", *LABELS, "--label", "oo.kind=pg-data", VOLUME
        ], "pg-volume")
        if proc.returncode != 0:
            raise SystemExit("volume")
    exists = run(["docker", "container", "inspect", PG_NAME])
    if exists.returncode == 0:
        if rewritten:
            proc = docker(["restart", "-t", "20", PG_NAME], "pg-restart", timeout=60)
            if proc.returncode != 0:
                raise SystemExit("pg restart")
        else:
            run(["docker", "start", PG_NAME])
    else:
        proc = docker([
            "run", "-d", "--name", PG_NAME,
            "--platform", "linux/arm64", "--pull", "never",
            "--memory", "1g", "--memory-swap", "1g", "--cpus", "1",
            "--pids-limit", "256",
            "--network", NETWORK,
            *LABELS, "--label", "oo.kind=postgres",
            "-e", "POSTGRES_PASSWORD_FILE=/run/secrets/admin",
            "-e", f"POSTGRES_DB={DATABASE}",
            "-e", "POSTGRES_INITDB_ARGS=--encoding=UTF8 --locale=C",
            "--mount", f"type=volume,source={VOLUME},target=/var/lib/postgresql/data",
            "--mount", f"type=bind,source={SECRET_DIR / 'pg-admin'},target=/run/secrets/admin,readonly",
            "--mount", f"type=bind,source={SECRET_DIR / 'pg-tls'},target=/certs,readonly",
            PG_IMAGE,
            "bash", "-c",
            "cp /certs/server.crt /var/lib/postgresql/server.crt && "
            "cp /certs/server.key /var/lib/postgresql/server.key && "
            "chown postgres:postgres /var/lib/postgresql/server.crt /var/lib/postgresql/server.key && "
            "chmod 600 /var/lib/postgresql/server.key && "
            "exec docker-entrypoint.sh postgres -c ssl=on "
            "-c ssl_cert_file=/var/lib/postgresql/server.crt "
            "-c ssl_key_file=/var/lib/postgresql/server.key",
        ], "pg-run", timeout=60)
        if proc.returncode != 0:
            raise SystemExit("pg start")


def wait_pg() -> None:
    deadline = time.time() + 90
    while time.time() < deadline:
        probe = run(["docker", "exec", "-u", "postgres", PG_NAME, "pg_isready", "-d", DATABASE])
        if probe.returncode == 0:
            (EVIDENCE / "pg-ready.txt").write_text("ready\n")
            return
        time.sleep(2)
    raise SystemExit("pg not ready")


def migration_text(name: str) -> str:
    return (MIGRATIONS / name).read_text()


def migration_sha(name: str) -> str:
    return catalog.sha256_hex((MIGRATIONS / name).read_bytes())


def reset_database() -> None:
    sql = f"""
SELECT pg_terminate_backend(pid)
  FROM pg_stat_activity
 WHERE datname = '{DATABASE}' AND pid <> pg_backend_pid();
DROP DATABASE IF EXISTS "{DATABASE}";
DO $reset$
DECLARE name text;
BEGIN
  FOREACH name IN ARRAY ARRAY[
    'oo_preview_login', 'oo_r_reader', 'oo_pipeline_runtime', 'oo_target_owner'
  ]
  LOOP
    IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = name) THEN
      EXECUTE format('DROP OWNED BY %I', name);
      EXECUTE format('DROP ROLE %I', name);
    END IF;
  END LOOP;
END
$reset$;
CREATE DATABASE "{DATABASE}" TEMPLATE template0 ENCODING 'UTF8' LOCALE 'C';
"""
    proc = psql(sql, "reset-database", database="postgres")
    if proc.returncode != 0:
        raise SystemExit("reset database failed")


def apply_schema() -> None:
    marker = EVIDENCE / "schema-applied.txt"
    if marker.exists():
        return
    reset_database()
    fence = (
        "0000_bootstrap_roles.sql",
        "0001_target_fence_expand.sql",
        "0003_target_segment_preflight_expand.sql",
        "0004_physical_mapping.sql",
    )
    markers = {
        "0000_bootstrap_roles.sql": "target-bootstrap/1",
        "0001_target_fence_expand.sql": "target-fence-receipts/1",
        "0003_target_segment_preflight_expand.sql": "target-fence-receipts/3",
        "0004_physical_mapping.sql": "physical-mapping/v1",
    }
    values = ",\n".join(
        f"  ('{version}', '{migration_sha(name)}', 'expand', '{name}')"
        for name, version in ((item, markers[item]) for item in fence)
    )
    script = "\n".join([
        "BEGIN;",
        "SET LOCAL lock_timeout = '15s';",
        "SET LOCAL statement_timeout = '120s';",
        "SELECT pg_catalog.pg_advisory_xact_lock(6404212121);",
        migration_text("0000_bootstrap_roles.sql"),
        "SET LOCAL ROLE oo_target_owner;",
        migration_text("0001_target_fence_expand.sql"),
        "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;",
        migration_text("0003_target_segment_preflight_expand.sql"),
        migration_text("0004_physical_mapping.sql"),
        "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES",
        values + ";",
        "COMMIT;",
        "BEGIN;",
        "SET LOCAL lock_timeout = '15s';",
        "SET LOCAL statement_timeout = '120s';",
        "SELECT pg_catalog.pg_advisory_xact_lock(6404212121);",
        "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;",
        "GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE;",
        "SET LOCAL ROLE oo_target_owner;",
        migration_text("0005_full_import_expand.sql"),
        "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES",
        f"  ('full-import/v1', '{migration_sha('0005_full_import_expand.sql')}', 'expand', '0005_full_import_expand.sql');",
        "COMMIT;",
    ])
    proc = psql(script, "migrate-schema")
    if proc.returncode != 0:
        raise SystemExit("migrate-schema failed")
    marker.write_text("applied\n")


def config_bytes(price: str, deadline_ms: int, bearer: str) -> bytes:
    body = catalog.config_document(bearer, price=price, deadline_ms=deadline_ms)
    return (json.dumps(body, separators=(",", ":"), sort_keys=True) + "\n").encode()


def write_inputs() -> str:
    work = EVIDENCE
    work.mkdir(parents=True, exist_ok=True)
    (work / "proxy.py").write_text(proxy_script())
    (work / "proxy.c").write_bytes((Path(__file__).resolve().parent / "proxy.c").read_bytes())
    live = config_bytes("declared", 120000, "/run/llm-token")
    (work / "config-live.json").write_bytes(live)
    (work / "config-cost.json").write_bytes(config_bytes("unknown", 120000, "/run/llm-token"))
    (work / "config-timeout.json").write_bytes(config_bytes("declared", 1, "/run/llm-token"))
    bad = SECRET_DIR / "bad-token"
    if not bad.exists():
        bad.write_text("badtok")
        os.chmod(bad, 0o600)
    (work / "config-401.json").write_bytes(config_bytes("declared", 120000, "/secrets/bad-token"))
    config_digest = catalog.sha256_hex(live)
    (work / "config-digest.txt").write_text(config_digest + "\n")
    (work / "catalog.json").write_text(catalog.dumps(catalog.catalog_document(config_digest)))
    (work / "catalog-lossy.json").write_text(catalog.dumps(catalog.catalog_document(config_digest, lossy_key=True)))
    (work / "settings.json").write_text(json.dumps(catalog.settings_document(), separators=(",", ":"), sort_keys=True))
    (work / "visibility.json").write_text(json.dumps(catalog.visibility_document(), separators=(",", ":"), sort_keys=True))
    (work / "gold.json").write_text(json.dumps(list(catalog.GOLD), indent=2) + "\n")
    (work / "cache").mkdir(exist_ok=True)
    (work / "query.json").write_text(json.dumps({
        "protocol": "oo.operator-preview.query/v1",
        "sql": "SELECT id, n16, n1, d, c6, v2, r FROM batch.t_batch ORDER BY id",
        "max_rows": 10,
        "max_result_bytes": 8192,
        "wall_clock_ms": 5000,
    }, separators=(",", ":")))
    (work / "query-cancel.json").write_text(json.dumps({
        "protocol": "oo.operator-preview.query/v1",
        "sql": "SELECT id, n16, n1, d, c6, v2, r FROM batch.t_batch ORDER BY id",
        "max_rows": 10,
        "max_result_bytes": 8192,
        "wall_clock_ms": 20000,
    }, separators=(",", ":")))
    (work / "query-hidden.json").write_text(json.dumps({
        "protocol": "oo.operator-preview.query/v1",
        "sql": "SELECT secret FROM batch.t_batch",
        "max_rows": 10,
        "max_result_bytes": 8192,
        "wall_clock_ms": 5000,
    }, separators=(",", ":")))
    (work / "query-write.json").write_text(json.dumps({
        "protocol": "oo.operator-preview.query/v1",
        "sql": "UPDATE batch.t_batch SET c6 = c6",
        "max_rows": 10,
        "max_result_bytes": 8192,
        "wall_clock_ms": 5000,
    }, separators=(",", ":")))
    return config_digest


def start_product() -> None:
    # The proxy process does not reload the bind-mounted script.
    run(["docker", "rm", "-f", PRODUCT_NAME])
    ready = EVIDENCE / "proxy-ready"
    if ready.exists():
        ready.unlink()
    proc = docker([
        "run", "-d", "--name", PRODUCT_NAME,
        "--platform", "linux/arm64", "--pull", "never",
        "--init", "--user", "0:0",
        "--security-opt", "no-new-privileges", "--cap-drop", "ALL",
        "--memory", "1g", "--memory-swap", "1g", "--cpus", "1",
        "--pids-limit", "256",
        "--network", NETWORK,
        "--add-host", "host.docker.internal:host-gateway",
        *LABELS, "--label", "oo.kind=oracle-product",
        "-e", "HOME=/tmp",
        "--mount", f"type=bind,source={EVIDENCE},target=/work",
        "--mount", f"type=bind,source={BINS},target=/bins,readonly",
        "--mount", f"type=bind,source={SECRET_DIR},target=/secrets,readonly",
        "--mount", f"type=bind,source={TOKEN},target=/run/llm-token,readonly",
        RUST_IMAGE,
        "bash", "-c",
        "if command -v python3 >/dev/null 2>&1; then exec python3 /work/proxy.py; fi; "
        "gcc -O2 -pthread -o /tmp/oo-proxy /work/proxy.c && exec /tmp/oo-proxy",
    ], "product-run", timeout=60)
    if proc.returncode != 0:
        raise SystemExit("product container")


def wait_proxy() -> None:
    deadline = time.time() + 30
    while time.time() < deadline:
        if (EVIDENCE / "proxy-ready").exists():
            return
        time.sleep(0.5)
    raise SystemExit("proxy not ready")


def exec_bin(args: list[str], name: str, timeout: int = 180) -> subprocess.CompletedProcess:
    proc = run(["docker", "exec", "-i", "-e", "HOME=/tmp", PRODUCT_NAME, *args], timeout=timeout)
    save(name, proc)
    return proc


def builder(config: str, catalog_name: str, out_name: str, step: str, *, cache: str = "/work/cache") -> dict:
    out = f"/work/{out_name}"
    proc = exec_bin([
        "/bins/pg-builder", "sealed",
        "--config", f"/work/{config}",
        "--catalog", f"/work/{catalog_name}",
        "--settings", "/work/settings.json",
        "--visibility", "/work/visibility.json",
        "--cache", cache,
        "--out", out,
        "--reviewer", "w04.owner",
        "--reviewed-at", "2026-10-10",
        "--dataset", catalog.DATASET,
        "--target-host", PG_HOST,
        "--target-port", "5432",
        "--target-database", DATABASE,
        "--target-user", LOGIN,
        "--login", LOGIN,
        "--target-ca", "/secrets/pg-tls/ca.crt",
        "--target-password", "/secrets/pg-login",
    ], step, timeout=180)
    result_path = EVIDENCE / out_name / "result.json"
    result = json.loads(result_path.read_text()) if result_path.exists() else {}
    result["_exit"] = proc.returncode
    return result


def import_seal(catalog_name: str, step: str) -> dict:
    proc = exec_bin([
        "/bins/pg-builder", "import-seal",
        "--catalog", f"/work/{catalog_name}",
        "--out", f"/work/{step}.json",
    ], step, timeout=60)
    body = json.loads((EVIDENCE / f"{step}.json").read_text()) if (EVIDENCE / f"{step}.json").exists() else {}
    body["_exit"] = proc.returncode
    return body


def prompt_is_clean(out_name: str) -> bool:
    path = EVIDENCE / out_name / "model-input.json"
    if not path.exists():
        return False
    text = path.read_text()
    return all(token not in text for token in catalog.FORBIDDEN_PROMPT)


def prepare_arrow_input() -> dict:
    """Record the real Arrow batch. This does not open or seal an import run."""
    if not ARROW_BATCH.is_file():
        raise SystemExit("native arrow batch is absent")
    raw = ARROW_BATCH.read_bytes()
    document = {
        "protocol": "oo.oracle-preview.native-arrow-input/v1",
        "status": "awaiting_loader_api",
        "admits_import": False,
        "sealed_at": None,
        "full_import_run": "not_opened",
        "gold_insert": "refused",
        "source_object": "OO04_FIX.T_BATCH",
        "source_owner": "OO04_FIX",
        "source_service": "FREEPDB1",
        "reader": "OO04_READER",
        "scn": catalog.SCN,
        "rows": 5,
        "arrow_path": str(ARROW_BATCH.relative_to(ROOT)),
        "arrow_bytes": len(raw),
        "input_digest": catalog.sha256_hex(raw),
        "input_digest_is_file_sha256": True,
        "rejected_literal_preimage": f"oracle-asof-scn-{catalog.SCN}-rows-5",
        "rejected_literal_digest": REJECTED_LITERAL_DIGEST,
        "decoder_gold_fingerprint": DECODER_GOLD_FINGERPRINT,
        "decoder_gold_fingerprint_is_not_the_file_hash": True,
        "schema": [
            {"name": "ID", "arrow": "Int16", "oracle": "NUMBER(4,0)"},
            {"name": "N16", "arrow": "Decimal128(16,6)", "oracle": "NUMBER(16,6)"},
            {"name": "N1", "arrow": "Int8", "oracle": "NUMBER(1,0)"},
            {"name": "D", "arrow": "Timestamp(Microsecond, None)", "oracle": "DATE"},
            {"name": "C6", "arrow": "Utf8", "oracle": "CHAR(6)"},
            {"name": "V2", "arrow": "Utf8", "oracle": "VARCHAR2"},
            {"name": "R", "arrow": "Binary", "oracle": "RAW"},
        ],
        "missing_api": MISSING_LOADER_API,
    }
    REPAIR_EVIDENCE.mkdir(parents=True, exist_ok=True)
    (REPAIR_EVIDENCE / "native-arrow-input.json").write_text(json.dumps(document, indent=2) + "\n")
    return document


def load_rows(config_digest: str) -> str:
    """Refuse the historical GOLD insert. The preserved previewdb run stays untouched."""
    prepare_arrow_input()
    REPAIR_EVIDENCE.mkdir(parents=True, exist_ok=True)
    (REPAIR_EVIDENCE / "gold-insert-refused.txt").write_text(
        "load_rows refused catalog.GOLD and did not call _oo.full_import_open_run or psql.\n"
        f"config_digest_not_used={config_digest}\n"
        "The open previewdb run is the preserved non-genuine seeded loop.\n"
    )
    raise SystemExit("missing public native Arrow physical import API")


def install_acl() -> dict:
    written = json.loads((EVIDENCE / "out-fresh" / "policy.json").read_text())
    policy = {
        "project": written["project"],
        "config_digest": written["config_digest"],
        "generation": written["generation"],
        "role": written["role"],
        "login": written["login"],
        "table": written["table"],
        "row_filter": None,
    }
    body = {
        "protocol": "oo.gateway.install-column-acl-cli/v1",
        "policies": [policy],
        "endpoint": {
            "host": PG_HOST,
            "port": 5432,
            "database": DATABASE,
            "user": "postgres",
        },
        "password_file": "/secrets/pg-admin",
        "ca_file": "/secrets/pg-tls/ca.crt",
    }
    payload = json.dumps(body, separators=(",", ":")).encode()
    proc = run(["docker", "exec", "-i", PRODUCT_NAME, "/bins/install-column-acl"], stdin=payload, timeout=60)
    save("install-acl", proc)
    if proc.returncode != 0:
        raise SystemExit(f"install acl exit {proc.returncode}")
    return json.loads(proc.stdout.decode())


def operator(args: list[str], name: str, timeout: int = 60) -> subprocess.CompletedProcess:
    return exec_bin(["/bins/operator-preview", *args], name, timeout=timeout)


def _batch_projection() -> str:
    return (
        "SELECT id || '|' || NVL(TO_CHAR(n16), 'NULL') || '|' || NVL(TO_CHAR(n1), 'NULL') || '|' || "
        "TO_CHAR(d, 'YYYY-MM-DD HH24:MI:SS') || '|' || NVL(c6, 'NULL') || '|' || "
        "NVL(v2, 'NULL') || '|' || NVL(RAWTOHEX(r), 'NULL')\n"
    )


def _asof_select() -> str:
    return _batch_projection() + f"FROM oo04_fix.t_batch AS OF SCN {catalog.SCN} ORDER BY id;\n"


def _sqlplus(sql: str, name: str) -> subprocess.CompletedProcess:
    """Run sqlplus as sysdba. Writes only under the repair evidence directory."""
    REPAIR_EVIDENCE.mkdir(parents=True, exist_ok=True)
    (REPAIR_EVIDENCE / f"{name}.sql").write_text(sql)
    proc = run(
        ["docker", "exec", "-i", ORACLE_NAME, "sqlplus", "-s", "/", "as", "sysdba"],
        stdin=sql.encode(),
        timeout=90,
    )
    (REPAIR_EVIDENCE / f"{name}.stdout").write_bytes(proc.stdout)
    (REPAIR_EVIDENCE / f"{name}.stderr").write_bytes(proc.stderr)
    (REPAIR_EVIDENCE / f"{name}.exit").write_text(f"{proc.returncode}\n")
    return proc


def confirm_root_ora_fails() -> None:
    """The historical root session must now exit nonzero on ORA-00942."""
    sql = (
        "SET PAGESIZE 0 FEEDBACK OFF VERIFY OFF HEADING OFF LINESIZE 200\n"
        "WHENEVER SQLERROR EXIT FAILURE\n"
        "SELECT SYS_CONTEXT('USERENV','CON_NAME') FROM dual;\n"
        + _asof_select()
        + "EXIT\n"
    )
    proc = _sqlplus(sql, "oracle-root-asof")
    text = proc.stdout.decode(errors="replace")
    if proc.returncode == 0 or "ORA-00942" not in text:
        raise SystemExit(f"root ORA-00942 did not fail closed exit={proc.returncode}")


def confirm_pdb_ora_fails() -> None:
    """A missing object inside FREEPDB1 must exit nonzero."""
    sql = (
        "SET PAGESIZE 0 FEEDBACK OFF VERIFY OFF HEADING OFF LINESIZE 200\n"
        "WHENEVER SQLERROR EXIT FAILURE\n"
        "ALTER SESSION SET CONTAINER = FREEPDB1;\n"
        "SELECT SYS_CONTEXT('USERENV','CON_NAME') FROM dual;\n"
        "SELECT COUNT(*) FROM oo04_fix.table_absent_oo04;\n"
        "EXIT\n"
    )
    proc = _sqlplus(sql, "oracle-freepdb1-missing")
    text = proc.stdout.decode(errors="replace")
    if proc.returncode == 0 or "ORA-" not in text or "FREEPDB1" not in text:
        raise SystemExit(f"FREEPDB1 ORA error did not fail closed exit={proc.returncode}")


def confirm_freepdb1_current() -> None:
    """Show the object is visible in FREEPDB1 now. These rows are not the SCN batch."""
    sql = (
        "SET PAGESIZE 0 FEEDBACK OFF VERIFY OFF HEADING OFF LINESIZE 200\n"
        "WHENEVER SQLERROR EXIT FAILURE\n"
        "ALTER SESSION SET CONTAINER = FREEPDB1;\n"
        "SELECT SYS_CONTEXT('USERENV','CON_NAME') FROM dual;\n"
        "SELECT DBMS_FLASHBACK.GET_SYSTEM_CHANGE_NUMBER FROM dual;\n"
        + _batch_projection()
        + "FROM oo04_fix.t_batch ORDER BY id;\n"
        "EXIT\n"
    )
    proc = _sqlplus(sql, "oracle-freepdb1-current")
    text = proc.stdout.decode(errors="replace")
    if proc.returncode != 0 or "ORA-" in text or "FREEPDB1" not in text:
        raise SystemExit(f"FREEPDB1 current read failed exit={proc.returncode}")


def confirm_oracle() -> None:
    """Read the real batch in FREEPDB1. Any ORA- or nonzero status fails the caller."""
    sql = (
        "SET PAGESIZE 0 FEEDBACK OFF VERIFY OFF HEADING OFF LINESIZE 200\n"
        "WHENEVER SQLERROR EXIT FAILURE\n"
        "ALTER SESSION SET CONTAINER = FREEPDB1;\n"
        "SELECT SYS_CONTEXT('USERENV','CON_NAME') FROM dual;\n"
        + _asof_select()
        + "EXIT\n"
    )
    proc = _sqlplus(sql, "oracle-freepdb1-asof")
    text = proc.stdout.decode(errors="replace")
    if proc.returncode != 0 or "ORA-" in text or "FREEPDB1" not in text:
        raise SystemExit(f"FREEPDB1 read failed exit={proc.returncode}")


def fence_oracle() -> str:
    # Docker Desktop answers unknown names from 198.18.0.0/15 and accepts the
    # TCP handshake. That address is not the Oracle container. The fence is the
    # container's own bridge address on the internal source network.
    inspected = run([
        "docker", "inspect", "-f",
        '{{with index .NetworkSettings.Networks "oo-dl01w1-04-p2-src"}}{{.IPAddress}}{{end}}',
        ORACLE_NAME,
    ])
    oracle_ip = inspected.stdout.decode().strip()
    if not oracle_ip:
        raise SystemExit("oracle bridge address is absent")
    script = (
        "import socket\n"
        f"oracle_ip={oracle_ip!r}\n"
        "try:\n"
        "    resolved=socket.getaddrinfo('oo-dl01w1-04-p2-oracle', 1521)[0][4][0]\n"
        "except Exception as exc:\n"
        "    resolved=type(exc).__name__\n"
        "print('resolved=' + resolved)\n"
        "print('oracle_bridge=' + oracle_ip)\n"
        "sock=socket.socket(); sock.settimeout(2)\n"
        "try:\n"
        "    sock.connect((oracle_ip, 1521)); print('oracle_ip=open')\n"
        "except Exception as exc:\n"
        "    print('oracle_ip=' + type(exc).__name__)\n"
        "finally:\n"
        "    sock.close()\n"
    )
    proc = run(["docker", "exec", PRODUCT_NAME, "python3", "-c", script], timeout=20)
    save("oracle-fence", proc)
    text = proc.stdout.decode(errors="replace")
    (EVIDENCE / "oracle-fence.txt").write_text(text)
    if proc.returncode != 0 or "oracle_ip=open" in text or f"resolved={oracle_ip}" in text:
        raise SystemExit("product reached oracle")
    return text


def stop_oracle() -> None:
    proc = docker(["stop", "-t", "20", ORACLE_NAME], "oracle-stop", timeout=40)
    if proc.returncode != 0:
        raise SystemExit("oracle stop")
    state = run(["docker", "inspect", "-f", "{{.State.Running}}", ORACLE_NAME])
    (EVIDENCE / "oracle-running-after-stop.txt").write_bytes(state.stdout)


def compare_query(name: str) -> None:
    proc = operator(["query", "--authority", "/work/out-fresh/authority.json", "--query", "/work/query.json"], name, timeout=60)
    if proc.returncode != 0:
        raise SystemExit(f"{name} exit {proc.returncode}")
    body = json.loads(proc.stdout.decode())
    ok, detail = catalog.cells_match(body.get("rows") or [])
    (EVIDENCE / f"{name}.compare").write_text(detail + "\n")
    if not ok:
        raise SystemExit(f"{name} compare {detail}")


def backend_state(pid: int) -> str:
    proc = run([
        "docker", "exec", "-u", "postgres", PG_NAME, "psql", "-d", DATABASE, "-tA", "-c",
        f"SELECT COALESCE(state, '') FROM pg_stat_activity WHERE pid = {int(pid)}",
    ], timeout=20)
    save("cancel-backend", proc)
    return proc.stdout.decode(errors="replace").strip()


def release_lock_holder(holder: subprocess.Popen) -> None:
    holder.kill()
    try:
        holder.wait(timeout=10)
    except subprocess.TimeoutExpired:
        pass
    run([
        "docker", "exec", "-u", "postgres", PG_NAME, "psql", "-d", DATABASE, "-c",
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity "
        "WHERE query LIKE '%pg_sleep%' AND pid <> pg_backend_pid()",
    ], timeout=20)


def cancel_locked(table: str) -> None:
    holder = subprocess.Popen(
        ["docker", "exec", "-i", "-u", "postgres", PG_NAME, "psql", "-d", DATABASE, "-v", "ON_ERROR_STOP=1"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    script = (
        "BEGIN;\n"
        f'LOCK TABLE "_oo"."{table}" IN ACCESS EXCLUSIVE MODE;\n'
        "\\echo locked\n"
        "SELECT pg_sleep(40);\n"
        "COMMIT;\n"
    ).encode()
    assert holder.stdin is not None
    holder.stdin.write(script)
    holder.stdin.close()
    assert holder.stdout is not None
    deadline = time.time() + 20
    seen = b""
    while time.time() < deadline and b"locked" not in seen:
        chunk = holder.stdout.readline()
        if not chunk:
            break
        seen += chunk
    (EVIDENCE / "cancel-lock.stdout").write_bytes(seen)
    if b"locked" not in seen:
        release_lock_holder(holder)
        raise SystemExit("lock was not acquired")
    query = subprocess.Popen(
        ["docker", "exec", "-i", PRODUCT_NAME, "/bins/operator-preview", "query",
         "--authority", "/work/out-fresh/authority.json", "--query", "/work/query-cancel.json"],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    )
    execution = ""
    backend_pid = 0
    state = EVIDENCE / "out-fresh" / "state" / "executions"
    deadline = time.time() + 15
    while time.time() < deadline and not execution:
        if state.exists():
            files = sorted(state.glob("*.json"), key=lambda path: path.stat().st_mtime)
            for path in reversed(files):
                body = json.loads(path.read_text())
                if body.get("status") == "running" and body.get("execution_id"):
                    execution = str(body["execution_id"])
                    backend_pid = int(body.get("backend_pid") or 0)
                    break
        time.sleep(0.05)
    if not execution:
        query.kill()
        release_lock_holder(holder)
        raise SystemExit("cancel target did not reach running")
    active = ""
    deadline = time.time() + 8
    while time.time() < deadline:
        active = backend_state(backend_pid)
        if active == "active":
            break
        time.sleep(0.05)
    (EVIDENCE / "cancel-backend-state.txt").write_text(active + "\n")
    if active != "active":
        query.kill()
        release_lock_holder(holder)
        raise SystemExit(f"cancel backend was {active or 'absent'}")
    cancelled = operator(
        ["cancel", "--authority", "/work/out-fresh/authority.json", "--execution", execution],
        "preview-cancel",
        timeout=30,
    )
    try:
        query.wait(timeout=30)
    except subprocess.TimeoutExpired:
        query.kill()
        query.wait(timeout=10)
    (EVIDENCE / "preview-cancel-query.stdout").write_bytes(query.stdout.read() if query.stdout else b"")
    (EVIDENCE / "preview-cancel-query.stderr").write_bytes(query.stderr.read() if query.stderr else b"")
    (EVIDENCE / "preview-cancel-query.exit").write_text(f"{query.returncode}\n")
    release_lock_holder(holder)
    cancel_body = json.loads(cancelled.stdout.decode() or "{}")
    query_body = json.loads((EVIDENCE / "preview-cancel-query.stdout").read_text() or "{}")
    (EVIDENCE / "cancel-proof.txt").write_text(
        f"backend={active}\ncancel_exit={cancelled.returncode}\n"
        f"cancel_outcome={cancel_body.get('outcome')}\n"
        f"query_exit={query.returncode}\nquery_outcome={query_body.get('outcome')}\n"
        f"sqlstate={query_body.get('sqlstate')}\n"
        f"rollback_confirmed={query_body.get('rollback_confirmed')}\n"
    )
    if cancelled.returncode != 0 or cancel_body.get("outcome") != "sent":
        raise SystemExit(f"cancel exit {cancelled.returncode} outcome {cancel_body.get('outcome')}")
    if query.returncode != 0 or query_body.get("outcome") != "canceled" or query_body.get("sqlstate") != "57014":
        raise SystemExit(f"canceled query outcome {query_body.get('outcome')} sqlstate {query_body.get('sqlstate')}")


def docker_ids(args: list[str]) -> list[str]:
    proc = run(["docker", *args])
    return [line for line in proc.stdout.decode().splitlines() if line.strip()]


def capacity_ok(need: int) -> str:
    own = docker_ids(["ps", "-q", "--filter", "label=oo.owner=dl01w1-04", "--filter", "label=oo.kind=rust-builder"])
    builders = docker_ids(["ps", "-q", "--filter", "label=oo.kind=rust-builder"])
    host = run(["ps", "-axo", "comm="])
    host_cargo = 0
    for line in host.stdout.decode().splitlines():
        if line.strip() in {"cargo", "rustc", "rustdoc", "rustup"}:
            host_cargo += 1
    info = run(["docker", "info", "--format", "{{.NCPU}} {{.MemTotal}}"])
    parts = info.stdout.decode().split()
    ncpu = int(parts[0]) if info.returncode == 0 and len(parts) >= 2 and parts[0].isdigit() else 0
    docker_mem = int(parts[1]) if info.returncode == 0 and len(parts) >= 2 and parts[1].isdigit() else 0
    reserved = 0
    for cid in docker_ids(["ps", "-q"]):
        one = run(["docker", "inspect", "-f", "{{.HostConfig.Memory}}", cid])
        text = one.stdout.decode().strip()
        if one.returncode == 0 and text.isdigit():
            reserved += int(text)
    headroom = docker_mem - reserved
    line = (
        f"own_rust_builder={len(own)} global_rust_builder={len(builders)} host_cargo={host_cargo} "
        f"docker_ncpu={ncpu} docker_mem_bytes={docker_mem} reserved_bytes={reserved} "
        f"headroom_bytes={headroom} need_bytes={need}\n"
    )
    (EVIDENCE / "capacity.txt").write_text(line)
    ten = 10 * 1024 * 1024 * 1024
    if (
        own
        or len(builders) >= 10
        or host_cargo >= 1
        or ncpu < 16
        or docker_mem < 8 * 1024 * 1024 * 1024
        or headroom - need <= ten
    ):
        (EVIDENCE / "pending-capacity.txt").write_text("status=pending-capacity\n" + line)
        return ""
    return line


def running_container(name: str) -> bool:
    proc = run(["docker", "inspect", "-f", "{{.State.Running}}", name])
    return proc.returncode == 0 and proc.stdout.decode().strip() == "true"


def reserve_runtime() -> None:
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    lock_path = "/tmp/oo-dl01-wave1-resources.lock"
    for _ in range(30):
        fd = os.open(lock_path, os.O_CREAT | os.O_RDWR, 0o644)
        fcntl.lockf(fd, fcntl.LOCK_EX)
        try:
            os.ftruncate(fd, 0)
            os.write(fd, f"DL01-W1-04 product-runtime pid={os.getpid()}\n".encode())
            need = 0
            if not running_container(PG_NAME):
                need += 1024 * 1024 * 1024
            if not running_container(PRODUCT_NAME):
                need += 1024 * 1024 * 1024
            if capacity_ok(need):
                start_pg()
                start_product()
                return
        finally:
            fcntl.lockf(fd, fcntl.LOCK_UN)
            os.close(fd)
        time.sleep(20)
    raise SystemExit("capacity")


def assert_clean_prompts() -> None:
    for name in ("out-fresh", "out-cache", "out-cost", "out-401", "out-timeout", "out-lossy"):
        path = EVIDENCE / name / "model-input.json"
        if not path.exists():
            continue
        text = path.read_text()
        for token in catalog.FORBIDDEN_PROMPT:
            if token in text:
                raise SystemExit(f"gold reached {name}")


def resume_cancel() -> int:
    compare = EVIDENCE / "preview-query.compare"
    if not compare.exists() or compare.read_text().strip() != "match":
        raise SystemExit("resume requires a matched preview query")
    config_digest = (EVIDENCE / "config-digest.txt").read_text().strip()
    table = catalog.physical_name(config_digest, "t", catalog.SCHEMA, catalog.TABLE, "")
    (EVIDENCE / "phase.txt").write_text("preview\n")
    cancel_locked(table)
    (EVIDENCE / "phase.txt").write_text("detach\n")
    stop_oracle()
    compare_query("preview-detached")
    (EVIDENCE / "phase.txt").write_text("done\n")
    print("DONE product")
    return 0


def main() -> int:
    prepare_arrow_input()
    raise SystemExit("missing public native Arrow physical import API")


def _seeded_loop_refused() -> int:
    """Historical seeded loop. main does not call it."""
    if os.environ.get("OO_PRODUCT_RESUME") == "cancel":
        return resume_cancel()
    catalog.self_check()
    for binary in ("pg-builder", "operator-preview", "install-column-acl"):
        if not (BINS / binary).exists():
            raise SystemExit(f"product binary absent: {binary}")
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    (EVIDENCE / "phase.txt").write_text("inputs\n")
    config_digest = write_inputs()
    (EVIDENCE / "phase.txt").write_text("capacity\n")
    reserve_runtime()
    (EVIDENCE / "phase.txt").write_text("pg\n")
    wait_pg()
    apply_schema()
    (EVIDENCE / "phase.txt").write_text("product\n")
    wait_proxy()
    fence_oracle()
    confirm_oracle()
    (EVIDENCE / "phase.txt").write_text("builder\n")
    sealed_import = import_seal("catalog.json", "import-seal")
    if sealed_import.get("_exit") != 0 or sealed_import.get("outcome") != "draft_compiled":
        raise SystemExit(f"import-seal {sealed_import.get('outcome')}")
    lossy_import = import_seal("catalog-lossy.json", "import-seal-lossy")
    if lossy_import.get("_exit") != 1 or lossy_import.get("model") != "not_called":
        raise SystemExit("lossy import-seal was not refused")
    lossy = builder("config-live.json", "catalog-lossy.json", "out-lossy", "sealed-lossy")
    if lossy.get("_exit") != 1 or lossy.get("model") != "not_called" or lossy.get("outcome") != "compiler_refused":
        raise SystemExit(f"lossy sealed {lossy.get('outcome')} model={lossy.get('model')}")
    fresh = builder("config-live.json", "catalog.json", "out-fresh", "sealed-fresh")
    if fresh.get("_exit") != 0 or fresh.get("pack") != "produced" or fresh.get("cache") != "miss":
        raise SystemExit(f"fresh {fresh.get('outcome')} cache={fresh.get('cache')} pack={fresh.get('pack')}")
    if not prompt_is_clean("out-fresh"):
        raise SystemExit("fresh prompt contains a gold token")
    cached = builder("config-live.json", "catalog.json", "out-cache", "sealed-cache")
    if cached.get("_exit") != 0 or cached.get("cache") != "hit" or cached.get("socket") != "not_opened":
        raise SystemExit(f"cache {cached.get('cache')} socket={cached.get('socket')}")
    if cached.get("identity_hash") != fresh.get("identity_hash") or cached.get("pack_digest") != fresh.get("pack_digest"):
        raise SystemExit("cache identity changed")
    cost = builder("config-cost.json", "catalog.json", "out-cost", "sealed-cost")
    if cost.get("outcome") != "cost_unknown" or cost.get("pack") != "not_produced":
        raise SystemExit(f"cost {cost.get('outcome')}")
    denied = builder("config-401.json", "catalog.json", "out-401", "sealed-401", cache="/work/cache-401")
    auth = "refused" if denied.get("_exit") != 0 and denied.get("pack") != "produced" else "not_enforced"
    (EVIDENCE / "auth-enforcement.txt").write_text(f"{auth}\nexit={denied.get('_exit')}\npack={denied.get('pack')}\noutcome={denied.get('outcome')}\n")
    timed = builder("config-timeout.json", "catalog.json", "out-timeout", "sealed-timeout", cache="/work/cache-timeout")
    deadline = "refused" if timed.get("pack") != "produced" else "not_enforced"
    (EVIDENCE / "deadline-enforcement.txt").write_text(
        f"{deadline}\nexit={timed.get('_exit')}\npack={timed.get('pack')}\noutcome={timed.get('outcome')}\nreason={timed.get('reason')}\n"
    )
    assert_clean_prompts()
    (EVIDENCE / "phase.txt").write_text("load\n")
    table = load_rows(config_digest)
    (EVIDENCE / "phase.txt").write_text("preview\n")
    installed = install_acl()
    (EVIDENCE / "install-acl.json").write_text(json.dumps(installed) + "\n")
    described = operator(["describe", "--authority", "/work/out-fresh/authority.json"], "preview-describe")
    if described.returncode != 0:
        raise SystemExit("describe")
    compare_query("preview-query")
    hidden = operator(["query", "--authority", "/work/out-fresh/authority.json", "--query", "/work/query-hidden.json"], "preview-hidden")
    written = operator(["query", "--authority", "/work/out-fresh/authority.json", "--query", "/work/query-write.json"], "preview-write")
    if hidden.returncode != 6 or written.returncode != 6:
        raise SystemExit(f"guard exits hidden={hidden.returncode} write={written.returncode}")
    cancel_locked(table)
    (EVIDENCE / "phase.txt").write_text("detach\n")
    stop_oracle()
    compare_query("preview-detached")
    (EVIDENCE / "phase.txt").write_text("done\n")
    print("DONE product")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except SystemExit as exc:
        if exc.code not in (0, None):
            EVIDENCE.mkdir(parents=True, exist_ok=True)
            (EVIDENCE / "product-error.txt").write_text(f"SystemExit: {exc.code}\n")
        raise
    except Exception as exc:
        EVIDENCE.mkdir(parents=True, exist_ok=True)
        (EVIDENCE / "product-error.txt").write_text(f"{type(exc).__name__}: {exc}\n")
        raise
