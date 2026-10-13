#!/usr/bin/env python3
"""Seal the five sr_w45 Arrow files on this session's PostgreSQL and read them
back with import-native-catalog. The pipeline and builder are the a6825165
archive binaries. The live workspace is not compiled and no other task's
database is opened.
"""

from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import re
import shutil
import socket
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import closure_gate

REPO = Path("/Users/redux/puppy-one/open-ontology")
SESSION = "2c023948-0862-4dae-ae8f-6fc30241db62"
TASK = "DL01-W1-45"
PREFIX = "oo-dl01w1-45"
EVIDENCE = REPO / "evidence" / SESSION / "DL01" / "starrocks-product"
OUT = EVIDENCE / "raw" / "native-catalog"
PREFLIGHT = REPO / "evidence" / SESSION / "DL01" / "starrocks-driver-preflight"
PLAN_SRC = EVIDENCE / "raw" / "native-bind" / "plan"
REGISTRY = EVIDENCE / "raw" / "native-bind" / "registry.json"
ARROW = PREFLIGHT / "native" / "read"
DECODED = PREFLIGHT / "native" / "decoded-initial"
ADMISSION_BIN = (
    REPO
    / "evidence"
    / "992aa5d3-6848-4dd9-aff1-d4d31a099983"
    / "DL01"
    / "oracle-product"
    / "admission-1"
    / "compile-5df"
    / "bin"
    / "oo-source-admission"
)
RECEIPT = (
    REPO
    / "evidence"
    / "2ecaf08e-eb23-4b7a-a720-e07459e6f2a6"
    / "DL01"
    / "budget-report"
    / "native-readonly-completion"
    / "receipt.json"
)
BINS = Path("/tmp/oo-dl01w1-45-catalog-bins")
SECRET_DIR = Path("/tmp/oo-dl01w1-45-pg-secrets")
TOKEN_SRC = Path("/tmp/oo-customer-demo-20261007/llm-token")
ADMIT = REPO / "testing" / "dl01-driver-preflight" / "starrocks" / "scripts" / "admit.py"
PROXY = Path(__file__).with_name("loopback_proxy.pl")
PG_IMAGE = "docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb"
RUST_IMAGE = "docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
PG_NAME = f"{PREFIX}-postgres"
LIBSRC = f"{PREFIX}-libsrc"
PG_PORT = 15445
OWNER = SESSION
SHARD = "oo45_sr_std"
SLUG = "w45"
ROLE = "oo_r_w45"
LOGIN = "oo_w45_login"
RUNTIME = "oo_runtime"
CUSTOMER_INPUT = EVIDENCE / "customer-input.json"
PROBE = PREFLIGHT.parent / "starrocks-driver-preflight" / "build" / "bin" / "starrocks-probe"
DECODE_BIN = PREFLIGHT.parent / "starrocks-driver-preflight" / "build" / "bin" / "starrocks-decode"
GATE_PY = REPO / "testing" / "dl01-driver-preflight" / "starrocks" / "scripts" / "run_gate.py"
SOURCE_PIN = "a6825165dd83ffaa30e17fbb2da1a481a05f6559"
ADMITTED: list[dict] = []
SOURCE_REPORT: dict = {}
ARROW_DIR = ARROW
DECODED_DIR = DECODED
PACK_DOCS: dict[str, dict] = {}
LOG = None
OPERATOR_SHA = "da44feccb813b70bf4d53b0d49ef5d2d3ca0c2228cff82a0a379bb78559e70c6"
PROBE_SHA = "21bbdc81d436bbffb2951b2245068c1667dccd02ed32a6667b3770b4b7bd1aee"
DECODE_SHA = "016be98241f4e4214bbf4d9e5826e80edaf881ceab4dcb2d62eb42a64ec50b9f"
EXPECTED_COUNTS = {
    "pk_accounts": 3,
    "agg_metrics": 2,
    "dup_events": 6,
    "uniq_dim": 2,
    "dup_partition_sums": 2,
}
REGISTRY_SHA = "6833baeb559ef004fcf510cdc260b8a9818ee223389f3d3fe560b2c92011eca2"
NATIVE_MIGRATION = "5c25559bc4d1b24471f19ae9eae8a45f40007c955938ea51edb03f1f5dbf2e26"
OID_NAME = {20: "int8", 1700: "numeric", 1043: "varchar", 1082: "date", 1114: "timestamp", 1042: "bpchar"}
SECRETS: list[bytes] = []


def now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def say(text: str) -> None:
    line = f"{now()} {text}"
    print(line, flush=True)
    if LOG is not None:
        LOG.write(line + "\n")
        LOG.flush()


def run(cmd: list[str], timeout: int = 180, stdin: bytes | None = None) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(cmd, input=stdin, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout, check=False)


def scrub(data: bytes) -> bytes:
    return closure_gate.scrub_bytes(data, SECRETS)


def save(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(scrub(data))


def write_json(path: Path, value: object) -> None:
    body = json.dumps(value, indent=2, sort_keys=True).encode() + b"\n"
    # Request documents name a password file. Keep those bodies out of evidence.
    if "password_file" in body.decode() and str(path).startswith(str(EVIDENCE)):
        digest = hashlib.sha256(body).hexdigest()
        path.with_suffix(path.suffix + ".sha").write_text(digest + "\n", encoding="utf-8")
        private = Path("/tmp/oo-dl01w1-45-catalog-requests")
        private.mkdir(mode=0o700, exist_ok=True)
        hidden = private / path.name
        hidden.write_bytes(body)
        os.chmod(hidden, 0o600)
        return
    save(path, body)


def remember(text: str) -> None:
    raw = text.encode()
    SECRETS.append(raw)
    if not raw.endswith(b"\n"):
        SECRETS.append(raw + b"\n")


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    digest.update(path.read_bytes())
    return digest.hexdigest()


def dexec(args: list[str], stdin: bytes | None = None, timeout: int = 180) -> subprocess.CompletedProcess[bytes]:
    return run(["docker", "exec", "-i", PG_NAME, *args], timeout=timeout, stdin=stdin)


def copy_in(src: Path, dest: str) -> None:
    proc = run(["docker", "cp", str(src), f"{PG_NAME}:{dest}"], timeout=180)
    if proc.returncode != 0:
        save(OUT / "docker-cp.stderr", proc.stderr)
        raise RuntimeError(f"docker cp {src.name} failed")


def port_free(port: int) -> bool:
    sock = socket.socket()
    try:
        sock.bind(("127.0.0.1", port))
        return True
    except OSError:
        return False
    finally:
        sock.close()


def make_certs() -> None:
    SECRET_DIR.mkdir(mode=0o700, exist_ok=True)
    os.chmod(SECRET_DIR, 0o700)
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
            save(OUT / "openssl.stderr", proc.stderr)
            raise RuntimeError("openssl failed")


def start_postgres() -> None:
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
        "/secrets/admin.password /secrets/runtime.password /secrets/login.password "
        "/secrets/llm-token /secrets/bearer-bad; "
        "chmod 600 /certs/server.key /secrets/admin.password /secrets/runtime.password "
        "/secrets/login.password /secrets/llm-token /secrets/bearer-bad; "
        "chmod 644 /certs/server.crt /certs/ca.crt; "
        "export PATH=\"$(echo /usr/lib/postgresql/*/bin):$PATH\"; "
        "exec docker-entrypoint.sh postgres -c ssl=on -c ssl_cert_file=/certs/server.crt "
        "-c ssl_key_file=/certs/server.key -c port=15445 -c listen_addresses='*' -c shared_buffers=64MB",
    ]
    for attempt in range(1, 31):
        if not port_free(PG_PORT):
            raise RuntimeError(f"port {PG_PORT} is in use")
        say(f"postgres capacity attempt {attempt}")
        proc = run([os.sys.executable, str(ADMIT), "acquire-start", "postgres", str(OUT / "pg-admission.json"), "--", *command], timeout=120)
        save(OUT / "pg-admission.stdout", proc.stdout)
        save(OUT / "pg-admission.stderr", proc.stderr)
        (OUT / "pg-admission.exit").write_text(f"{proc.returncode}\n", encoding="utf-8")
        if proc.returncode == 0:
            return
        if proc.returncode != 2:
            raise RuntimeError(f"postgres was not admitted: {proc.returncode}")
        say("postgres capacity denied")
        time.sleep(900)
    raise RuntimeError("postgres capacity denied 30 times")


def install_glibc() -> None:
    run(["docker", "rm", "-f", LIBSRC], timeout=30)
    created = run(["docker", "create", "--name", LIBSRC, "--platform", "linux/arm64", "--pull", "never", RUST_IMAGE], timeout=60)
    if created.returncode != 0:
        save(OUT / "glibc-create.stderr", created.stderr)
        raise RuntimeError("glibc source create failed")
    host = Path("/tmp/oo-dl01w1-45-glibc")
    host.mkdir(parents=True, exist_ok=True)
    names = {
        "ld-linux-aarch64.so.1": "/usr/lib/aarch64-linux-gnu/ld-linux-aarch64.so.1",
        "libc.so.6": "/usr/lib/aarch64-linux-gnu/libc.so.6",
        "libgcc_s.so.1": "/usr/lib/aarch64-linux-gnu/libgcc_s.so.1",
    }
    try:
        for local, remote in names.items():
            proc = run(["docker", "cp", f"{LIBSRC}:{remote}", str(host / local)], timeout=60)
            if proc.returncode != 0 or (host / local).is_symlink():
                raise RuntimeError(f"glibc copy {local} failed")
    finally:
        run(["docker", "rm", "-f", LIBSRC], timeout=30)
    dexec(["mkdir", "-p", "/opt/glibc"])
    for local in names:
        copy_in(host / local, f"/opt/glibc/{local}")
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
    proc = dexec(["bash", "-c", script], timeout=30)
    save(OUT / "glibc-install.stderr", proc.stderr)
    if proc.returncode != 0:
        raise RuntimeError("glibc install failed")


def wait_postgres() -> None:
    deadline = time.time() + 180
    last = b""
    while time.time() < deadline:
        state = run(["docker", "inspect", "-f", "{{.State.Status}} {{.State.OOMKilled}}", PG_NAME])
        if not state.stdout.decode().startswith("running"):
            save(OUT / "pg-state.txt", state.stdout + state.stderr)
            raise RuntimeError("postgres exited")
        probe = dexec(["pg_isready", "-h", "127.0.0.1", "-p", str(PG_PORT), "-d", "oo_w45"])
        last = probe.stdout + probe.stderr
        if probe.returncode == 0:
            save(OUT / "pg-ready.txt", last)
            return
        time.sleep(2)
    save(OUT / "pg-ready.txt", last)
    raise RuntimeError("postgres did not become ready")


def psql(sql: str, user: str = "postgres") -> subprocess.CompletedProcess[bytes]:
    return dexec(
        [
            "bash", "-c",
            f"PGPASSFILE=/secrets/{user}.pgpass PGUSER={user} psql "
            "\"host=127.0.0.1 port=15445 dbname=oo_w45 sslmode=verify-full sslrootcert=/certs/ca.crt\" "
            "-v ON_ERROR_STOP=1 -tA",
        ],
        stdin=sql.encode(),
        timeout=60,
    )


def record_step(directory: Path, name: str, proc: subprocess.CompletedProcess[bytes]) -> int:
    directory.mkdir(parents=True, exist_ok=True)
    save(directory / f"{name}.stdout", proc.stdout)
    save(directory / f"{name}.stderr", proc.stderr)
    code = proc.returncode
    (directory / f"{name}.exit").write_text(f"{code}\n", encoding="utf-8")
    return code


def pipeline(args: list[str], body: dict, directory: Path, name: str, env: list[str] | None = None) -> int:
    raw = json.dumps(body).encode()
    write_json(directory / f"{name}.request.json", body)
    command = ["docker", "exec", "-i", PG_NAME]
    if env:
        command.append("env")
        command.extend(env)
    command.extend(args)
    proc = run(command, stdin=raw, timeout=180)
    return record_step(directory, name, proc)


def dba_roles() -> None:
    # init-pg revokes CONNECT from PUBLIC. The frozen operator logs in as
    # oo_w45_login and reads _oo.physical_mappings before SET ROLE. The column
    # role is not created here: a plain NOLOGIN role inherits by default, and
    # the installer refuses that shape instead of granting columns onto it.
    sql = (
        f"CREATE ROLE {RUNTIME} LOGIN INHERIT PASSWORD '{(SECRET_DIR / 'runtime.password').read_text().strip()}';\n"
        f"GRANT oo_pipeline_runtime TO {RUNTIME};\n"
        f"CREATE ROLE {LOGIN} LOGIN PASSWORD '{(SECRET_DIR / 'login.password').read_text().strip()}';\n"
        f"GRANT CONNECT ON DATABASE oo_w45 TO {LOGIN};\n"
        f"GRANT USAGE ON SCHEMA _oo TO {LOGIN};\n"
        f"GRANT SELECT ON TABLE _oo.physical_mappings TO {LOGIN};\n"
        f"GRANT EXECUTE ON FUNCTION _oo.lookup_physical_mapping(text, text, bigint, text, text, text, text) TO {LOGIN};\n"
        f"GRANT pg_read_all_stats TO {LOGIN} WITH INHERIT TRUE, SET FALSE;\n"
    )
    # The SQL contains passwords. Run it and keep only the exit code.
    proc = psql(sql)
    code = proc.returncode
    (OUT / "dba-roles.exit").write_text(f"{code}\n", encoding="utf-8")
    save(OUT / "dba-roles.stderr", proc.stderr)
    if code != 0:
        raise RuntimeError("dba roles failed")
    # prepare_shadow grants INSERT on the unpublished shadow and not SELECT.
    # The loader's own readback is SELECT count(*). PostgreSQL 17 returns
    # 42501. This DBA fixture grants SELECT to oo_pipeline_runtime only for
    # _oo.pt_* created in this database. It does not grant the operator role.
    grant_sql = """
CREATE OR REPLACE FUNCTION public.w45_shadow_select_grant()
RETURNS event_trigger
LANGUAGE plpgsql
AS $fn$
DECLARE
  obj record;
BEGIN
  FOR obj IN SELECT * FROM pg_event_trigger_ddl_commands() LOOP
    IF obj.command_tag = 'CREATE TABLE'
       AND obj.schema_name = '_oo'
       AND obj.object_identity ~ '^_oo\\.pt_[0-9a-f]+$'
    THEN
      EXECUTE format('GRANT SELECT ON TABLE %s TO oo_pipeline_runtime', obj.object_identity);
    END IF;
  END LOOP;
END;
$fn$;
DROP EVENT TRIGGER IF EXISTS w45_shadow_select;
CREATE EVENT TRIGGER w45_shadow_select
  ON ddl_command_end
  WHEN TAG IN ('CREATE TABLE')
  EXECUTE FUNCTION public.w45_shadow_select_grant();
"""
    granted = psql(grant_sql)
    (OUT / "shadow-select-grant.exit").write_text(f"{granted.returncode}\n", encoding="utf-8")
    save(OUT / "shadow-select-grant.stderr", granted.stderr)
    if granted.returncode != 0:
        raise RuntimeError("shadow select grant failed")
    probe = psql("SELECT current_user;\n", user=LOGIN)
    (OUT / "login-connect.exit").write_text(f"{probe.returncode}\n", encoding="utf-8")
    save(OUT / "login-connect.stderr", probe.stderr)
    if probe.returncode != 0:
        raise RuntimeError("login connect failed")


def target_block(principal: str, password_file: str) -> dict:
    return {
        "host": "127.0.0.1",
        "port": PG_PORT,
        "database": "oo_w45",
        "runtime_principal": principal,
        "password_file": password_file,
        "ca_file": "/certs/ca.crt",
    }


def import_tables(registry_sha: str, names: list[str]) -> dict[str, dict]:
    sealed: dict[str, dict] = {}
    env = [
        "OO_NATIVE_ADMISSION_BIN=/admission/oo-source-admission",
        "OO_NATIVE_BINDING_REGISTRY=/evidence/plan/registry.json",
        f"OO_NATIVE_BINDING_REGISTRY_SHA256={registry_sha}",
    ]
    for name in names:
        manifest = json.loads((PLAN_SRC / name / "manifest.json").read_text())
        manifest["target"] = {
            **{key: value for key, value in target_block(RUNTIME, "/secrets/runtime.password").items() if key != "runtime_principal"},
            "runtime_principal": RUNTIME,
            "shard": SHARD,
            "owner": OWNER,
        }
        directory = OUT / "import" / name
        directory.mkdir(parents=True, exist_ok=True)
        raw = json.dumps(manifest).encode()
        write_json(directory / "manifest.json", manifest)
        hidden = Path("/tmp/oo-dl01w1-45-catalog-requests") / f"{name}-manifest.json"
        hidden.write_bytes(raw)
        copy_in(hidden, f"/work/in/{name}-manifest.json")
        proc = dexec(
            ["env", *env, "/opt/oo/bin/pg-builder", "import-native",
             "--manifest", f"/work/in/{name}-manifest.json",
             "--out", f"/work/out/import-{name}.json",
             "--pipeline", "/opt/oo/bin/oo-pipeline",
             "--stderr", f"/work/out/import-{name}.stderr"],
            timeout=180,
        )
        record_step(directory, "import", proc)
        envelope_proc = dexec(["cat", f"/work/out/import-{name}.json"])
        save(directory / "envelope.json", envelope_proc.stdout)
        stderr_proc = dexec(["cat", f"/work/out/import-{name}.stderr"])
        save(directory / "import.child.stderr", stderr_proc.stdout)
        if proc.returncode != 0:
            say(f"import {name} exit {proc.returncode}")
            continue
        envelope = json.loads(envelope_proc.stdout.decode() or "{}")
        run_id = (envelope.get("import") or {}).get("run_id")
        if not run_id:
            say(f"import {name} has no run_id")
            continue
        gen = psql(f"SELECT target_generation FROM _oo.full_import_run WHERE run_id = '{run_id}';\n")
        save(directory / "generation.stdout", gen.stdout)
        save(directory / "generation.stderr", gen.stderr)
        sealed[name] = {
            "run_id": run_id,
            "target_generation": int(gen.stdout.decode().strip()),
            "identity": manifest["catalog_identity"],
        }
        say(f"sealed {name} {run_id} generation {sealed[name]['target_generation']}")
    return sealed


def catalog_tables(sealed: dict[str, dict]) -> dict[str, dict]:
    catalogs: dict[str, dict] = {}
    env = [
        "OO_NATIVE_ADMISSION_BIN=/admission/oo-source-admission",
        "OO_NATIVE_BINDING_REGISTRY=/evidence/plan/registry.json",
        f"OO_NATIVE_BINDING_REGISTRY_SHA256={REGISTRY_SHA}",
    ]
    for name, item in sealed.items():
        identity = item["identity"]
        body = {
            "protocol": "oo.pipeline.native-sealed-catalog/v1",
            "run_id": item["run_id"],
            "source_id": "sr_w45",
            "owner": OWNER,
            "source_generation": 1,
            "target_generation": item["target_generation"],
            "config_digest": identity["config_digest"],
            "schema_digest": identity["schema_digest"],
            "type_digest": identity["type_digest"],
            "input_digest": identity["input_digest"],
            "snapshot": {"kind": "token", "value": "sr-w45-c4-native-read"},
            "target": target_block(RUNTIME, "/secrets/runtime.password"),
        }
        directory = OUT / "catalog" / name
        directory.mkdir(parents=True, exist_ok=True)
        raw = json.dumps(body).encode()
        write_json(directory / "request.json", body)
        hidden = Path("/tmp/oo-dl01w1-45-catalog-requests") / f"{name}-catalog-request.json"
        hidden.write_bytes(raw)
        copy_in(hidden, f"/work/in/{name}-catalog-request.json")
        proc = dexec(
            ["env", *env, "/opt/oo/bin/pg-builder", "import-native-catalog",
             "--request", f"/work/in/{name}-catalog-request.json",
             "--out", f"/work/out/catalog-{name}.json",
             "--pipeline", "/opt/oo/bin/oo-pipeline",
             "--stderr", f"/work/out/catalog-{name}.stderr"],
            timeout=120,
        )
        record_step(directory, "catalog", proc)
        document = dexec(["cat", f"/work/out/catalog-{name}.json"])
        save(directory / "catalog.json", document.stdout)
        child = dexec(["cat", f"/work/out/catalog-{name}.stderr"])
        save(directory / "catalog.child.stderr", child.stdout)
        if proc.returncode != 0 or not document.stdout.strip():
            say(f"catalog {name} exit {proc.returncode}")
            continue
        parsed = json.loads(document.stdout.decode())
        catalogs[name] = parsed
        say(f"catalog {name} engine {parsed.get('engine')} rows {parsed.get('tables', [{}])[0].get('accepted_rows')}")
    return catalogs


def settings_json() -> dict:
    return {
        "project": "sr_w45",
        "ontology_version": "ont-w45",
        "catalog_version": "pending",
        "policy": {"id": "pol-w45", "version": "1"},
        "knowledge": {"id": "know-w45", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "sr_w45",
            "locale": "en-US",
            "include_schemas": ["oo_w45_sr"],
            "default_tier_preference": ["raw"],
            "publish": {"include_unreviewed": ["dataset_descriptions"]},
            "classification": {"auto_accept": "rule_high_confidence"},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": True,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                "sample_values": {"default": "masked", "max_per_column": 0},
            },
        },
    }


def visibility_json(catalog: dict) -> dict:
    table = catalog["tables"][0]
    return {
        "protocol": "oo.builder.role-visibility/v1",
        "project": "sr_w45",
        "principal": "w45-operator",
        "role": ROLE,
        "ontology_version": "ont-w45",
        "catalog_version": "pending",
        "policy": {"id": "pol-w45", "version": "1"},
        "knowledge": {"id": "know-w45", "version": "1"},
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "datasets": [{
            "dataset": f"oo_w45_sr.{table['logical_table']}",
            "row_security": "none",
            "global_stats": "denied",
            "columns": [column["logical_name"] for column in table["columns"]],
        }],
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128},
    }


def builder_config(price: str, deadline_ms: int, bearer: str) -> dict:
    return {
        "protocol": "oo.pg-builder.config/v1",
        "adapter": "loopback_http_local_dev",
        "namespace": "pg-builder",
        "purpose": "generate",
        "sample_policy": "none",
        "temperature": 0,
        "endpoint": {"origin": "http://127.0.0.1:8317", "path": "/v1/chat/completions", "model_id": "grok-4.7"},
        "secret": {"bearer_file": bearer},
        "declared_price": json.loads(price),
        "budget": {
            "deadline_ms": deadline_ms,
            "max_request_bytes": 262144,
            "max_response_bytes": 262144,
            "max_prompt_tokens": 100000,
            "max_completion_tokens": 100000,
            "max_total_tokens": 200000,
            "max_tables": 16,
            "max_in_flight": 1,
        },
    }


def run_builder(name: str, label: str, catalog_host: Path, price: str, deadline_ms: int, bearer: str, cache: str) -> dict:
    remote = f"/work/out/builder/{name}/{label}"
    dexec(["mkdir", "-p", remote, f"/work/out/cache/{name}/{cache}"])
    config = builder_config(price, deadline_ms, bearer)
    config_path = Path("/tmp/oo-dl01w1-45-catalog-requests") / f"{name}-{label}-builder.json"
    config_path.write_text(json.dumps(config), encoding="utf-8")
    copy_in(config_path, f"{remote}/builder-config.json")
    settings = Path("/tmp/oo-dl01w1-45-catalog-requests") / f"{name}-settings.json"
    settings.write_text(json.dumps(settings_json()), encoding="utf-8")
    copy_in(settings, f"{remote}/settings.json")
    catalog = json.loads(catalog_host.read_text())
    vis = Path("/tmp/oo-dl01w1-45-catalog-requests") / f"{name}-visibility.json"
    vis.write_text(json.dumps(visibility_json(catalog)), encoding="utf-8")
    copy_in(vis, f"{remote}/visibility.json")
    copy_in(catalog_host, f"{remote}/catalog.json")
    proc = dexec(
        [
            "/opt/oo/bin/pg-builder", "sealed",
            "--config", f"{remote}/builder-config.json",
            "--catalog", f"{remote}/catalog.json",
            "--settings", f"{remote}/settings.json",
            "--visibility", f"{remote}/visibility.json",
            "--cache", f"/work/out/cache/{name}/{cache}",
            "--out", remote,
            "--reviewer", "w45-source-owner",
            "--reviewed-at", "2026-10-10",
            "--dataset", f"oo_w45_sr.{name}",
            "--target-host", "127.0.0.1",
            "--target-port", str(PG_PORT),
            "--target-database", "oo_w45",
            "--target-user", LOGIN,
            "--login", LOGIN,
            "--target-ca", "/certs/ca.crt",
            "--target-password", "/secrets/login.password",
        ],
        timeout=max(60, deadline_ms // 1000 + 45),
    )
    directory = OUT / "builder" / name / label
    record_step(directory, "builder", proc)
    result = dexec(["cat", f"{remote}/result.json"])
    save(directory / "result.json", result.stdout or b"")
    parsed = {}
    if result.stdout.strip():
        try:
            parsed = json.loads(result.stdout.decode())
        except json.JSONDecodeError:
            parsed = {}
    parsed["_exit"] = proc.returncode
    say(f"builder {name} {label} exit {proc.returncode} outcome {parsed.get('outcome')} reason {parsed.get('reason')}")
    return parsed


def install_acl(catalogs: dict[str, dict]) -> int:
    policies = []
    for name, catalog in catalogs.items():
        table = catalog["tables"][0]
        columns = []
        for column in table["columns"]:
            semantic = OID_NAME.get(column["oid"])
            if semantic is None:
                say(f"acl skip unknown oid {column['oid']} on {name}.{column['logical_name']}")
                continue
            columns.append({
                "name": column["logical_name"],
                "status": "confirmed",
                "semantic_type": semantic,
                "pii": {"class": "none"},
            })
        policies.append({
            "project": "sr_w45",
            "config_digest": catalog["config_digest"],
            "generation": catalog["source_generation"],
            "role": SLUG,
            "login": LOGIN,
            "table": {"namespace": "oo_w45_sr", "name": name, "columns": columns},
        })
    body = {
        "protocol": "oo.gateway.install-column-acl-cli/v1",
        "policies": policies,
        "endpoint": {
            "host": "127.0.0.1",
            "port": PG_PORT,
            "database": "oo_w45",
            "user": "postgres",
        },
        "password_file": "/secrets/admin.password",
        "ca_file": "/certs/ca.crt",
    }
    raw = json.dumps(body).encode()
    write_json(OUT / "acl-request.json", body)
    proc = dexec(["/opt/oo/bin/install-column-acl"], stdin=raw, timeout=120)
    return record_step(OUT, "install-column-acl", proc)


def operator_query(name: str, label: str, sql: str, authority: str, wall_clock_ms: int = 10000) -> subprocess.CompletedProcess[bytes]:
    body = {
        "protocol": "oo.operator-preview.query/v1",
        "sql": sql,
        "max_rows": 100,
        "max_result_bytes": 65536,
        "wall_clock_ms": wall_clock_ms,
    }
    path = Path("/tmp/oo-dl01w1-45-catalog-requests") / f"{name}-{label}.query.json"
    path.write_text(json.dumps(body), encoding="utf-8")
    copy_in(path, f"/work/in/{name}-{label}.query.json")
    return dexec(
        ["/opt/oo/bin/operator-preview", "query", "--authority", authority, "--query", f"/work/in/{name}-{label}.query.json"],
        timeout=max(60, wall_clock_ms // 1000 + 30),
    )


IDENT = re.compile(r"^[A-Za-z_][A-Za-z0-9_]{0,62}$")


def parse_json_stdout(proc: subprocess.CompletedProcess[bytes]) -> dict:
    text = proc.stdout.decode(errors="replace").strip()
    if not text:
        return {}
    try:
        value = json.loads(text)
    except json.JSONDecodeError:
        return {}
    return value if isinstance(value, dict) else {}


def compare_rows(name: str, decoded_rows: list, fields: list, got_rows: list) -> dict:
    return closure_gate.compare_rows(name, decoded_rows, fields, got_rows)


def type_and_role(catalog: dict) -> dict:
    table = catalog["tables"][0]
    physical = str(table.get("stored_physical", ""))
    proof: dict = {"namespace": "_oo", "physical": physical, "stored_namespace": table.get("stored_namespace", "")}
    if not IDENT.fullmatch(physical) or not physical.startswith("pt_"):
        proof["type_sql"] = "refused_ident"
        return proof
    sql = (
        "SELECT a.attname, a.atttypid::int, a.atttypmod "
        "FROM pg_catalog.pg_attribute a "
        "JOIN pg_catalog.pg_class c ON c.oid = a.attrelid "
        "JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace "
        f"WHERE n.nspname = '_oo' AND c.relname = '{physical}' "
        "AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum;\n"
    )
    proc = psql(sql)
    proof["type_exit"] = proc.returncode
    proof["type_stdout"] = proc.stdout.decode(errors="replace")
    expected = {
        column["stored_physical"]: (column["oid"], column["typmod"])
        for column in table["columns"]
    }
    got = {}
    for line in proof["type_stdout"].splitlines():
        parts = line.split("|")
        if len(parts) != 3:
            continue
        got[parts[0]] = (int(parts[1]), int(parts[2]))
    proof["type_got"] = {name: [oid, typmod] for name, (oid, typmod) in got.items()}
    proof["type_equal"] = all(got.get(name) == pair for name, pair in expected.items())
    role_sql = (
        "SELECT member.rolname, m.inherit_option, m.set_option, m.admin_option "
        "FROM pg_catalog.pg_auth_members m "
        "JOIN pg_catalog.pg_roles role ON role.oid = m.roleid "
        "JOIN pg_catalog.pg_roles member ON member.oid = m.member "
        f"WHERE role.rolname = '{ROLE}' ORDER BY member.rolname;\n"
    )
    role = psql(role_sql)
    proof["role_exit"] = role.returncode
    proof["role_stdout"] = role.stdout.decode(errors="replace")
    proof["role_stderr"] = role.stderr.decode(errors="replace")
    return proof


def our_source_names() -> list[str]:
    proc = run(["docker", "ps", "-a", "--filter", f"name={PREFIX}", "--format", "{{.Names}}"])
    names = []
    for line in proc.stdout.decode().splitlines():
        name = line.strip()
        if name.startswith(PREFIX) and "starrocks" in name:
            names.append(name)
    return names


def stop_our_source() -> dict:
    names = our_source_names()
    stopped = []
    for name in names:
        proc = run(["docker", "stop", name], timeout=120)
        stopped.append({"name": name, "exit": proc.returncode})
    remaining = our_source_names()
    running = run(["docker", "ps", "--filter", f"name={PREFIX}", "--format", "{{.Names}} {{.Status}}"])
    lines = running.stdout.decode(errors="replace").splitlines()
    return {
        "stopped": stopped,
        "still_present": remaining,
        "source_running": remaining,
        "prefix_running": lines,
    }


def hold_lock(name: str, physical: str) -> str:
    if not IDENT.fullmatch(physical) or not physical.startswith("pt_"):
        raise RuntimeError(f"physical name refused for {name}")
    remote = f"/work/out/cancel/{name}"
    dexec(["mkdir", "-p", remote])
    sql = (
        "BEGIN;\n"
        "SET LOCAL lock_timeout = '15s';\n"
        f'LOCK TABLE "_oo"."{physical}" IN ACCESS EXCLUSIVE MODE;\n'
        "SELECT pg_sleep(180);\n"
        "ROLLBACK;\n"
    )
    hidden = Path("/tmp/oo-dl01w1-45-catalog-requests") / f"{name}-lock.sql"
    hidden.write_text(sql, encoding="utf-8")
    copy_in(hidden, f"{remote}/lock.sql")
    script = (
        f"PGPASSFILE=/secrets/postgres.pgpass PGUSER=postgres psql "
        "\"host=127.0.0.1 port=15445 dbname=oo_w45 sslmode=verify-full sslrootcert=/certs/ca.crt\" "
        f"-v ON_ERROR_STOP=1 -f {remote}/lock.sql > {remote}/lock.stdout 2> {remote}/lock.stderr"
    )
    proc = dexec(["bash", "-c", f"{script} & echo $! > {remote}/lock.pid"])
    if proc.returncode != 0:
        raise RuntimeError(f"lock start failed for {name}")
    deadline = time.time() + 20
    while time.time() < deadline:
        probe = psql(
            "SELECT count(*) FROM pg_catalog.pg_locks l "
            "JOIN pg_catalog.pg_class c ON c.oid = l.relation "
            f"WHERE c.relname = '{physical}' AND l.mode = 'AccessExclusiveLock' AND l.granted;\n"
        )
        if probe.stdout.decode().strip() == "1":
            return remote
        time.sleep(0.2)
    raise RuntimeError(f"access exclusive lock was not granted for {physical}")


def rotate_native_audit(name: str, label: str) -> None:
    # native_wal::begin admits one execution into seg-00000001.wal and refuses
    # the segment once it contains frames. The approved query uses that
    # segment. Cancel and the source-off query need their own header.
    remote = f"/work/out/builder/{name}/pack/audit"
    destination = OUT / "audit" / name / label
    if destination.exists():
        shutil.rmtree(destination)
    destination.mkdir(parents=True, exist_ok=True)
    run(["docker", "cp", f"{PG_NAME}:{remote}/.", str(destination)], timeout=60)
    cleared = dexec(["bash", "-c", f"mkdir -p {remote} && rm -f {remote}/seg-*.wal"])
    if cleared.returncode != 0:
        raise RuntimeError(f"audit rotate failed for {name}")


def release_lock(remote: str) -> None:
    dexec(["bash", "-c", f"if [ -f {remote}/lock.pid ]; then kill $(cat {remote}/lock.pid) || true; fi"])


def running_execution(name: str) -> dict | None:
    proc = dexec([
        "bash", "-c",
        f"find /work/out/builder/{name}/pack/state/executions -type f -print 2>/dev/null",
    ])
    for path in proc.stdout.decode().splitlines():
        path = path.strip()
        if not path:
            continue
        body = dexec(["cat", path])
        try:
            value = json.loads(body.stdout.decode())
        except json.JSONDecodeError:
            continue
        if value.get("status") == "running" and value.get("execution_id") and value.get("backend_pid"):
            return value
    return None


def inflight_cancel(name: str, catalog: dict, authority: str) -> dict:
    table = catalog["tables"][0]
    physical = str(table.get("stored_physical", ""))
    report: dict = {"table": name, "physical": physical, "lock_schema": "_oo"}
    if not IDENT.fullmatch(physical) or not physical.startswith("pt_"):
        report["error"] = "physical name refused"
        return report
    remote = ""
    query: subprocess.Popen[bytes] | None = None
    try:
        rotate_native_audit(name, "typed-query")
        remote = hold_lock(name, physical)
        query_path = f"/work/in/{name}-cancel.query.json"
        body = {
            "protocol": "oo.operator-preview.query/v1",
            "sql": approved_sql(catalog),
            "max_rows": 100,
            "max_result_bytes": 65536,
            "wall_clock_ms": 20000,
        }
        hidden = Path("/tmp/oo-dl01w1-45-catalog-requests") / f"{name}-cancel.query.json"
        hidden.write_text(json.dumps(body), encoding="utf-8")
        copy_in(hidden, query_path)
        query = subprocess.Popen(
            ["docker", "exec", "-i", PG_NAME, "/opt/oo/bin/operator-preview", "query",
             "--authority", authority, "--query", query_path],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        running = None
        deadline = time.time() + 15
        while time.time() < deadline:
            running = running_execution(name)
            if running is not None:
                break
            time.sleep(0.2)
        if running is None:
            report["error"] = "query did not stay running"
        else:
            pid = int(running["backend_pid"])
            report["execution_id"] = running["execution_id"]
            active = False
            for _ in range(40):
                state = psql(f"SELECT state FROM pg_catalog.pg_stat_activity WHERE pid = {pid};\n")
                if state.stdout.decode().strip() == "active":
                    active = True
                    break
                time.sleep(0.1)
            report["backend_active_before_cancel"] = active
            if not active:
                report["error"] = "backend was not active"
            else:
                cancel = dexec(
                    ["/opt/oo/bin/operator-preview", "cancel", "--authority", authority,
                     "--execution", str(running["execution_id"])],
                    timeout=30,
                )
                record_step(OUT / "cancel" / name, "cancel", cancel)
                report["cancel_exit"] = cancel.returncode
                report["cancel_result"] = parse_json_stdout(cancel)
    finally:
        if query is not None:
            try:
                stdout, stderr = query.communicate(timeout=40)
            except subprocess.TimeoutExpired:
                query.kill()
                stdout, stderr = query.communicate()
            finished = subprocess.CompletedProcess(query.args, query.returncode if query.returncode is not None else 1, stdout, stderr)
            record_step(OUT / "cancel" / name, "query", finished)
            report["query_exit"] = finished.returncode
            report["query_result"] = parse_json_stdout(finished)
        if remote:
            release_lock(remote)
            for leaf in ("lock.stdout", "lock.stderr"):
                body = dexec(["cat", f"{remote}/{leaf}"])
                save(OUT / "cancel" / name / leaf, body.stdout)
    queried = report.get("query_result") or {}
    cancel_doc = report.get("cancel_result") or {}
    report["proven"] = bool(
        report.get("backend_active_before_cancel")
        and cancel_doc.get("outcome") == "sent"
        and report.get("cancel_exit") == 0
        and queried.get("outcome") == "canceled"
        and queried.get("sqlstate") == "57014"
        and queried.get("rollback_confirmed") is True
    )
    return report


def scan_tree_for_ddl(root: Path) -> list[str]:
    hits = []
    if not root.exists():
        return hits
    for path in root.rglob("*"):
        if not path.is_file() or path.stat().st_size > 2_000_000:
            continue
        if path.suffix not in {".json", ".jsonl", ".log", ".txt", ".stdout", ".stderr"}:
            continue
        for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
            stripped = line.strip()
            verb = stripped.lstrip("{").upper()
            if re.search(r"\b(DROP|ALTER|GRANT)\b", verb) and not verb.startswith("SHOW"):
                hits.append(f"{path.name}: {stripped[:180]}")
    return hits


def reader_ddl_count() -> int:
    root = SOURCE_REPORT.get("evidence")
    if not root:
        return -1
    base = Path(str(root))
    found = False
    count = 0
    for label in ("read", "cancel"):
        path = base / label / "audit.jsonl"
        if not path.is_file():
            continue
        found = True
        for line in path.read_text(encoding="utf-8").splitlines():
            try:
                item = json.loads(line)
            except json.JSONDecodeError:
                count += 1
                continue
            sql = item.get("sql")
            if item.get("class") == "write" or (isinstance(sql, str) and closure_gate.WRITE_VERB.match(sql.strip())):
                count += 1
    return count if found else -1


def copy_audit(name: str, label: str) -> None:
    destination = OUT / "audit" / name / label
    if destination.exists():
        shutil.rmtree(destination)
    destination.mkdir(parents=True, exist_ok=True)
    proc = run(["docker", "cp", f"{PG_NAME}:/work/out/builder/{name}/pack/audit/.", str(destination)], timeout=60)
    if proc.returncode != 0:
        save(OUT / "audit" / f"{name}-{label}-copy.stderr", proc.stderr)


def cleanup_runtime() -> None:
    run(["docker", "rm", "-f", PG_NAME], timeout=60)
    run(["docker", "rm", "-f", LIBSRC], timeout=30)
    run([os.sys.executable, str(ADMIT), "release", "postgres"], timeout=30)
    say("postgres container removed and slot released")


def approved_sql(catalog: dict) -> str:
    table = catalog["tables"][0]
    columns = ", ".join(column["logical_name"] for column in table["columns"])
    first = table["columns"][0]["logical_name"]
    return f"SELECT {columns} FROM oo_w45_sr.{table['logical_table']} ORDER BY {first}"


def widen_sql(catalog: dict) -> str:
    table = catalog["tables"][0]
    numeric = next(column["logical_name"] for column in table["columns"] if column["oid"] == 1700)
    return f"SELECT {numeric}::float8 AS wide FROM oo_w45_sr.{table['logical_table']}"


def preserve_builder(name: str, label: str, dest: str) -> None:
    source = OUT / "builder" / name / label
    target = OUT / "builder" / name / dest
    if not source.exists():
        return
    if target.exists():
        shutil.rmtree(target)
    shutil.copytree(source, target)


def file_size(path: str) -> int | None:
    if not path.startswith("/"):
        return None
    stat = dexec(["stat", "-c", "%s", path])
    if stat.returncode != 0:
        return None
    try:
        return int(stat.stdout.decode().strip())
    except ValueError:
        return None


def save_authority(name: str) -> None:
    remote = f"/work/out/builder/{name}/pack/authority.json"
    proc = dexec(["cat", remote])
    if proc.returncode != 0 or not proc.stdout.strip():
        return
    try:
        doc = json.loads(proc.stdout.decode())
    except json.JSONDecodeError:
        return
    target = doc.get("target") if isinstance(doc.get("target"), dict) else {}
    write_json(OUT / "builder" / name / "pack" / "authority-scrubbed.json", {
        "protocol": doc.get("protocol"),
        "project": doc.get("project"),
        "role": doc.get("role"),
        "host": target.get("host"),
        "port": target.get("port"),
        "database": target.get("database"),
        "user": target.get("user"),
        "ca_path": target.get("ca_path"),
        "password_path": target.get("password_path"),
        "ca_bytes": file_size(str(target.get("ca_path") or "")),
        "password_bytes": file_size(str(target.get("password_path") or "")),
    })


def retry_timeout(name: str, label: str, first: dict, rerun, attempts: int = 2) -> dict:
    current = first
    for index in range(1, attempts):
        if not (current.get("reason") == "timeout" and current.get("_exit") != 0):
            return current
        preserve_builder(name, label, f"{label}-timeout-{index}")
        current = rerun()
    return current


def one_table(name: str, catalog: dict) -> dict:
    host_catalog = OUT / "catalog" / name / "catalog.json"
    price_known = '{"status":"declared_non_billing","currency":"USD","per_total_token":"0"}'
    unknown = retry_timeout(
        name, "cost-unknown",
        run_builder(name, "cost-unknown", host_catalog, '{"status":"unknown"}', 120000, "/secrets/llm-token", "fresh-unknown"),
        lambda: run_builder(name, "cost-unknown", host_catalog, '{"status":"unknown"}', 120000, "/secrets/llm-token", "fresh-unknown"),
        attempts=3,
    )
    # The separate timeout probe stays at 1 ms. Pack itself waits long enough
    # for the live model to finish; a 30 s budget returned provider timeouts
    # for calls that later completed.
    pack = retry_timeout(
        name, "pack",
        run_builder(name, "pack", host_catalog, price_known, 120000, "/secrets/llm-token", "pack"),
        lambda: run_builder(name, "pack", host_catalog, price_known, 120000, "/secrets/llm-token", "pack"),
    )
    cached = run_builder(
        name, "cache", host_catalog,
        '{"status":"declared_non_billing","currency":"USD","per_total_token":"0"}',
        30000, "/secrets/llm-token", "pack",
    )
    denied = run_builder(
        name, "unauthorized", host_catalog,
        '{"status":"declared_non_billing","currency":"USD","per_total_token":"0"}',
        30000, "/secrets/bearer-bad", "unauthorized",
    )
    timed = run_builder(
        name, "timeout", host_catalog,
        '{"status":"declared_non_billing","currency":"USD","per_total_token":"0"}',
        1, "/secrets/llm-token", "timeout",
    )
    authority = f"/work/out/builder/{name}/pack/authority.json"
    present = dexec(["test", "-f", authority])
    item: dict = {
        "engine": catalog.get("engine"),
        "state": catalog.get("state"),
        "cost_unknown": unknown.get("outcome"),
        "pack_exit": pack.get("_exit"),
        "pack_outcome": pack.get("outcome"),
        "cache": cached.get("cache"),
        "cache_model": cached.get("model"),
        "cache_model_matches_pack": bool(pack.get("model_input_sha256")) and cached.get("model_input_sha256") == pack.get("model_input_sha256"),
        "unauthorized_http": denied.get("http_status"),
        "unauthorized_reason": denied.get("reason"),
        "timeout_reason": timed.get("reason"),
        "authority_present": present.returncode == 0,
    }
    if present.returncode != 0:
        item["error"] = "pack authority is absent"
        return item
    save_authority(name)
    try:
        decoded = json.loads((DECODED_DIR / f"{name}.json").read_text())
        queried = operator_query(name, "approved", approved_sql(catalog), authority)
        record_step(OUT / "query" / name, "approved", queried)
        approved = parse_json_stdout(queried)
        compared = compare_rows(name, decoded["rows"], decoded["fields"], approved.get("rows") or [])
        saved = persisted_document(OUT / "query" / name / "approved.stdout")
        saved_compared = compare_rows(name, decoded["rows"], decoded["fields"], saved.get("rows") or [])
        compared["persisted_equal"] = bool(
            compared["equal"]
            and saved_compared["equal"]
            and saved_compared["got"] == compared["got"]
            and saved_compared["temporal_equal"]
        )
        compared["temporal_equal"] = bool(compared["temporal_equal"] and saved_compared["temporal_equal"])
        identity = closure_gate.pack_identity(pack, approved)
        PACK_DOCS[name] = pack
        compared["consumer"] = consumer_provenance(approved, identity)
        write_json(OUT / "query" / name / "compare.json", compared)
        hidden_column = operator_query(name, "hidden", f"SELECT secret FROM oo_w45_sr.{name}", authority)
        record_step(OUT / "query" / name, "hidden", hidden_column)
        hidden_doc = parse_json_stdout(hidden_column)
        widen = operator_query(name, "widen", widen_sql(catalog), authority)
        record_step(OUT / "query" / name, "widen", widen)
        widen_doc = parse_json_stdout(widen)
        proof = type_and_role(catalog)
        write_json(OUT / "query" / name / "type-role.json", proof)
        cancel = inflight_cancel(name, catalog, authority)
        write_json(OUT / "cancel" / name / "report.json", cancel)
        copy_audit(name, "cancel")
        item.update({
            "query_exit": queried.returncode,
            "query_outcome": approved.get("outcome"),
            "rows_equal": compared["equal"],
            "hidden_exit": hidden_column.returncode,
            "hidden_class": hidden_doc.get("class"),
            "widen_exit": widen.returncode,
            "widen_class": widen_doc.get("class"),
            "type_equal": proof.get("type_equal"),
            "role_stdout": proof.get("role_stdout"),
            "cancel_proven": cancel.get("proven"),
            "cancel_sqlstate": (cancel.get("query_result") or {}).get("sqlstate"),
            "rollback_confirmed": (cancel.get("query_result") or {}).get("rollback_confirmed"),
            "persisted_equal": compared["persisted_equal"],
            "temporal_equal": compared["temporal_equal"],
            "pack_identity": identity,
            "returned_rows": approved.get("returned_rows"),
            "import_rows": (catalog.get("tables") or [{}])[0].get("accepted_rows"),
            "registry_rows": next((row["row_count"] for row in ADMITTED if row["name"] == name), None),
        })
    except Exception as exc:
        item["error"] = str(exc)
        say(f"table {name} kept builder fields after {exc}")
        return item
    say(
        f"table {name} query {item['query_exit']} rows {item['rows_equal']} "
        f"hidden {item['hidden_exit']} widen {item['widen_exit']} cancel {item['cancel_proven']}"
    )
    return item


def consume(catalogs: dict[str, dict]) -> dict:
    proxy = dexec(["bash", "-c", "perl /opt/oo/loopback_proxy.pl >/work/out/proxy.log 2>&1 & echo $!"])
    save(OUT / "proxy.stdout", proxy.stdout)
    time.sleep(0.4)
    report: dict[str, object] = {"tables": {}}
    acl_code = install_acl(catalogs)
    report["acl_exit"] = acl_code
    say(f"install-column-acl {acl_code}")
    tables: dict[str, dict] = {}
    for name, catalog in catalogs.items():
        try:
            tables[name] = one_table(name, catalog)
        except Exception as exc:
            tables[name] = {"error": str(exc)}
            say(f"table {name} stopped: {exc}")
    report["tables"] = tables
    report["source_off"] = stop_our_source()
    source_queries = {}
    for name, catalog in catalogs.items():
        authority = f"/work/out/builder/{name}/pack/authority.json"
        if tables.get(name, {}).get("authority_present") is not True:
            source_queries[name] = {"skipped": "authority absent"}
            continue
        rotate_native_audit(name, "before-source-off")
        queried = operator_query(name, "source-off", approved_sql(catalog), authority)
        record_step(OUT / "query" / name, "source-off", queried)
        parsed = parse_json_stdout(queried)
        decoded = json.loads((DECODED_DIR / f"{name}.json").read_text())
        compared = compare_rows(name, decoded["rows"], decoded["fields"], parsed.get("rows") or [])
        saved = persisted_document(OUT / "query" / name / "source-off.stdout")
        saved_compared = compare_rows(name, decoded["rows"], decoded["fields"], saved.get("rows") or [])
        off_identity = closure_gate.pack_identity(PACK_DOCS.get(name) or {}, parsed)
        typed_identity = tables.get(name, {}).get("pack_identity") or {}
        compared["persisted_equal"] = bool(
            compared["equal"]
            and saved_compared["equal"]
            and saved_compared["got"] == compared["got"]
            and saved_compared["temporal_equal"]
        )
        compared["temporal_equal"] = bool(compared["temporal_equal"] and saved_compared["temporal_equal"])
        compared["same_pack"] = closure_gate.same_pack(typed_identity, off_identity)
        compared["consumer"] = consumer_provenance(parsed, off_identity)
        write_json(OUT / "query" / name / "source-off-compare.json", compared)
        source_queries[name] = {
            "exit": queried.returncode,
            "outcome": parsed.get("outcome"),
            "rows_equal": compared["equal"],
            "persisted_equal": compared["persisted_equal"],
            "temporal_equal": compared["temporal_equal"],
            "same_pack": compared["same_pack"],
            "returned_rows": parsed.get("returned_rows"),
            "pack_identity": off_identity,
        }
        copy_audit(name, "source-off")
        say(f"source-off {name} exit {queried.returncode} rows {compared['equal']}")
    report["source_off_queries"] = source_queries
    audit_hits = []
    for name in catalogs:
        audit_hits.extend(scan_tree_for_ddl(OUT / "audit" / name))
    report["operator_audit_ddl"] = audit_hits
    report["reader_ddl_attempt_count"] = reader_ddl_count()
    write_json(OUT / "audit-scan.json", {
        "operator_audit_ddl": audit_hits,
        "reader_ddl_attempt_count": report["reader_ddl_attempt_count"],
    })
    return report


def preserve_prior() -> None:
    dest = EVIDENCE / "attempts" / "20261010T222833Z-rowcount-compare"
    if dest.exists() or not OUT.exists():
        return
    if not (OUT / "gates.json").is_file() and not (OUT / "closure-run.log").is_file():
        return
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.move(str(OUT), str(dest))


def persisted_document(path: Path) -> dict:
    if not path.is_file():
        return {}
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return {}
    return value if isinstance(value, dict) else {}


def consumer_provenance(document: dict, identity: dict) -> dict:
    imported = document.get("import") if isinstance(document.get("import"), dict) else {}
    return {
        "protocol": document.get("protocol"),
        "type_path": document.get("type_path"),
        "source_dialed": imported.get("source_dialed"),
        "operator_preview_sha256": OPERATOR_SHA,
        "source_pin": SOURCE_PIN,
        "pack_digest": identity.get("pack_digest"),
        "model_input_sha256": identity.get("model_input_sha256"),
        "identity_hash": identity.get("identity_hash"),
        "returned_rows": document.get("returned_rows"),
    }


def product_secret_hits() -> list[str]:
    secrets: list[str] = []
    for raw in SECRETS:
        try:
            text = raw.decode("utf-8")
        except UnicodeDecodeError:
            continue
        if text and text not in secrets:
            secrets.append(text)
    hits = []
    if not secrets:
        return hits
    for path in OUT.rglob("*"):
        if not path.is_file() or path.suffix == ".arrow" or path.stat().st_size > 2_000_000:
            continue
        try:
            body = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        if closure_gate.bounded_secret_hits(body, secrets):
            hits.append(str(path.relative_to(OUT)))
    return hits


def elf_problem() -> str | None:
    digests = {
        "oo-pipeline": "4919cbe68dfcfaf703d9756d304ffadfd1a23b8f701049e54d11ff6efc11b3d7",
        "init-pg": "1598de5396d1a2744aad0ad7b5153bb10c30f1c07f0abb369465ff4f3d6acf34",
        "install-column-acl": "6064a99b834c58de6f695c8f7e2e93fc0bec8cd66b1c84c8e1f08948eeb393a4",
        "pg-builder": "4f6b138f9de94c03b521e49a14a3cd1de3e33a1a4ee818aa9c077f6f4f529023",
        "operator-preview": OPERATOR_SHA,
    }
    if not PROBE.is_file() or sha256_file(PROBE) != PROBE_SHA:
        return "probe elf sha"
    if not DECODE_BIN.is_file() or sha256_file(DECODE_BIN) != DECODE_SHA:
        return "decode elf sha"
    for name, digest in digests.items():
        path = BINS / name
        if not path.is_file() or sha256_file(path) != digest:
            return f"binary {name}"
    return None


def prepare_customer() -> dict:
    if not CUSTOMER_INPUT.is_file():
        decision = {
            "ok": False,
            "errors": ["customer_input_absent"],
            "admitted": [],
            "refused": [],
            "read_attempted_names": [],
        }
        write_json(OUT / "customer-admission.json", decision)
        return decision
    request = json.loads(CUSTOMER_INPUT.read_text(encoding="utf-8"))
    registry = json.loads(REGISTRY.read_text(encoding="utf-8"))
    decision = closure_gate.admit_customer(request, registry)
    counts = {item["name"]: item["row_count"] for item in decision["admitted"]}
    reasons = {item["reason"] for item in decision["refused"]}
    decision["registry_sha_ok"] = request.get("registry_sha256") == REGISTRY_SHA
    decision["registry_counts_ok"] = counts == EXPECTED_COUNTS
    decision["refusal_ok"] = {"not_approved", "field_not_approved"} <= reasons
    decision["read_attempted_names"] = []
    write_json(OUT / "customer-admission.json", decision)
    return decision


def customer_ready(decision: dict) -> bool:
    return bool(
        decision.get("ok")
        and decision.get("registry_sha_ok")
        and decision.get("registry_counts_ok")
        and decision.get("refusal_ok")
        and not decision.get("errors")
        and [item["name"] for item in decision.get("admitted") or []] == list(EXPECTED_COUNTS)
    )


def load_source_gate():
    spec = importlib.util.spec_from_file_location("starrocks_run_gate", GATE_PY)
    if spec is None or spec.loader is None:
        raise RuntimeError("run_gate could not be loaded")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    module.EVIDENCE = OUT / "source-session"
    module.EVIDENCE.mkdir(parents=True, exist_ok=True)
    return module


def start_source_with_capacity(gate) -> bool:
    for attempt in range(1, 4):
        ok = gate.start_container()
        reason = ""
        admission_path = gate.EVIDENCE / "admission.json"
        if admission_path.is_file():
            try:
                reason = str(json.loads(admission_path.read_text(encoding="utf-8")).get("reason") or "")
            except json.JSONDecodeError:
                reason = "admission json unreadable"
        if ok:
            say(f"starrocks admitted on attempt {attempt}")
            return True
        retryable = any(word in reason.lower() for word in ("headroom", "cpu", "cap"))
        say(f"starrocks admit attempt {attempt} refused: {reason or 'no reason'}")
        if not retryable or attempt == 3:
            return False
        say("starrocks capacity wait 900s")
        time.sleep(900)
    return False


def stop_source_container() -> None:
    run(["docker", "rm", "-f", f"{PREFIX}-starrocks"], timeout=60)
    run([sys.executable, str(ADMIT), "release", "starrocks"], timeout=30)
    say("starrocks container removed and slot released")


def tsv_count(path: Path) -> int:
    if not path.is_file():
        return -1
    return sum(1 for line in path.read_text(encoding="utf-8", errors="replace").splitlines() if line.strip())


def file_mode(path: str) -> int:
    return os.stat(path).st_mode


def run_source_session(decision: dict) -> bool:
    global ARROW_DIR, DECODED_DIR, SOURCE_REPORT, ADMITTED
    report: dict = {"ok": False, "read_attempted_names": [], "removed_before_target": False}
    SOURCE_REPORT = report
    gate = None
    try:
        if not customer_ready(decision):
            report["error"] = "customer admission refused the read"
            return False
        ADMITTED = list(decision["admitted"])
        problem = elf_problem()
        if problem:
            report["error"] = problem
            return False
        token = TOKEN_SRC.read_text(encoding="utf-8")
        remember(token.strip())
        gate = load_source_gate()
        report["evidence"] = str(gate.EVIDENCE)
        if token.strip():
            gate.SECRETS.append(token.strip())
        unknown = closure_gate.resolve_secret(
            "secret://oo-dl01w1-45/not-issued",
            {},
            os.path.exists,
            file_mode,
        )
        report["unknown_secret"] = {
            "ok": unknown.get("ok"),
            "reason": unknown.get("reason"),
            "read_attempted": unknown.get("read_attempted"),
        }
        if unknown.get("ok") is not False or unknown.get("read_attempted") is not False:
            report["error"] = "unknown secret_ref was not refused"
            return False
        if not start_source_with_capacity(gate):
            report["error"] = "starrocks was not admitted"
            return False
        if not gate.wait_ready() or not gate.identity():
            report["error"] = "starrocks identity failed"
            return False
        accounts = gate.create_accounts()
        for secret in list(gate.SECRETS):
            remember(secret)
        resolved = closure_gate.resolve_secret(
            str(decision.get("secret_ref") or ""),
            {"secret://oo-dl01w1-45/reader": "/tmp/oo-dl01w1-45-secrets/reader.password"},
            os.path.exists,
            file_mode,
        )
        report["secret"] = {
            "ok": resolved.get("ok"),
            "reason": resolved.get("reason"),
            "read_attempted": resolved.get("read_attempted"),
        }
        if not resolved.get("ok") or resolved.get("read_attempted") is not False:
            report["error"] = "approved secret_ref did not resolve before the read"
            return False
        if not gate.load_fixture(accounts["dba"]):
            report["error"] = "dba load failed"
            report["dba_window"] = "separate"
            return False
        report["dba_window"] = "separate"
        gate.install(PROBE, "/tmp/starrocks-probe", "755")
        gate.install(DECODE_BIN, "/tmp/starrocks-decode", "755")
        linked = gate.run(["docker", "exec", gate.CONTAINER, "/tmp/starrocks-probe", "--help"])
        if linked.returncode not in {0, 1} or b"GLIBC" in linked.stderr or b"No such file" in linked.stderr:
            report["error"] = "probe did not start"
            return False
        names = [item["name"] for item in ADMITTED]
        report["read_attempted_names"] = names
        decision["read_attempted_names"] = names
        write_json(OUT / "customer-admission.json", decision)
        read_code = gate.run_probe(
            "/tmp/starrocks-probe",
            ["read", "--host", "127.0.0.1", "--port", "9030", "--password-file", "/tmp/oo-secrets/reader.password", "--image-digest", gate.PIN],
            "read",
        )
        native = gate.EVIDENCE / "native-initial"
        gate.native_dump(accounts["reader"], native)
        decoded_dir = gate.EVIDENCE / "decoded-initial"
        gate.decode("/tmp/starrocks-decode", gate.EVIDENCE / "read", decoded_dir)
        oracle_ok = gate.run_oracle(decoded_dir, native, "initial")
        cancel_code = gate.run_probe(
            "/tmp/starrocks-probe",
            ["cancel", "--host", "127.0.0.1", "--port", "9030", "--password-file", "/tmp/oo-secrets/reader.password", "--image-digest", gate.PIN],
            "cancel",
            timeout=120,
        )
        gate.fe_audit()
        cancel_path = gate.EVIDENCE / "cancel" / "cancel.json"
        cancel_doc = json.loads(cancel_path.read_text(encoding="utf-8")) if cancel_path.is_file() else {}
        old_cancel = PREFLIGHT / "native" / "cancel" / "cancel.json"
        fresh_cancel = cancel_path.is_file() and old_cancel.is_file() and sha256_file(cancel_path) != sha256_file(old_cancel)
        report["cancel"] = {
            "exit": cancel_code,
            "state": cancel_doc.get("state"),
            "dba_kill_used": cancel_doc.get("dba_kill_used"),
            "kill": cancel_doc.get("kill"),
            "fresh_file": fresh_cancel,
        }
        report["cancel_proven"] = bool(
            cancel_code == 0
            and cancel_doc.get("state") == "cancel_proven"
            and cancel_doc.get("dba_kill_used") is False
            and cancel_doc.get("kill") == "ok"
            and fresh_cancel
        )
        events = []
        for label in ("read", "cancel"):
            path = gate.EVIDENCE / label / "audit.jsonl"
            if path.is_file():
                events.extend(closure_gate.load_jsonl(path))
        audit = closure_gate.audit_events(events)
        report["audit"] = audit
        report["write_attempts"] = audit.get("write_attempts")
        fe_path = gate.EVIDENCE / "fe-audit-reader.log"
        fe_text = fe_path.read_text(encoding="utf-8", errors="replace") if fe_path.is_file() else ""
        report["fe_write_attempts"] = closure_gate.fe_write_attempts(fe_text)
        objects = []
        identity_ok = True
        for item in ADMITTED:
            name = item["name"]
            arrow = gate.EVIDENCE / "read" / f"{name}.arrow"
            decoded = decoded_dir / f"{name}.json"
            arrow_sha = sha256_file(arrow) if arrow.is_file() else ""
            decoded_sha = sha256_file(decoded) if decoded.is_file() else ""
            decoded_rows = -1
            if decoded.is_file():
                decoded_rows = len(json.loads(decoded.read_text(encoding="utf-8")).get("rows") or [])
            native_rows = tsv_count(native / f"{name}.tsv")
            row = {
                "name": name,
                "registry_rows": item["row_count"],
                "decoded_rows": decoded_rows,
                "native_rows": native_rows,
                "arrow_sha256": arrow_sha,
                "arrow_ok": arrow_sha == item.get("arrow_sha256"),
                "receipt_sha256": decoded_sha,
                "receipt_ok": decoded_sha == item.get("capture_receipt_sha256"),
                "count_ok": decoded_rows == item["row_count"] == native_rows == EXPECTED_COUNTS[name],
            }
            identity_ok = identity_ok and bool(row["arrow_ok"] and row["receipt_ok"] and row["count_ok"])
            objects.append(row)
        report["objects"] = objects
        report["oracle_ok"] = bool(oracle_ok)
        report["probe_read_exit"] = read_code
        failed_checks = [item["name"] for item in gate.CHECKS if not item.get("ok")]
        write_json(gate.EVIDENCE / "checks.json", {"failed": failed_checks})
        report["gate_checks_failed"] = failed_checks
        report["ok"] = bool(
            read_code == 0
            and oracle_ok
            and report["cancel_proven"]
            and audit.get("ok")
            and audit.get("write_attempts") == 0
            and audit.get("failed_write_attempts") == 0
            and audit.get("cancel", 0) >= 1
            and report["fe_write_attempts"] == 0
            and identity_ok
        )
        if report["ok"]:
            ARROW_DIR = gate.EVIDENCE / "read"
            DECODED_DIR = decoded_dir
            report["arrow_dir"] = str(ARROW_DIR.relative_to(EVIDENCE))
            report["decoded_dir"] = str(DECODED_DIR.relative_to(EVIDENCE))
        else:
            report["error"] = "source identity, oracle, cancel, or write audit failed"
        return bool(report["ok"])
    except Exception as exc:
        if gate is not None:
            for secret in list(gate.SECRETS):
                remember(secret)
        report["error"] = type(exc).__name__
        say(f"source session stopped: {type(exc).__name__}")
        return False
    finally:
        write_json(OUT / "source-report.json", report)


def execute_closure() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    say("catalog closure start")
    if not TOKEN_SRC.is_file() or TOKEN_SRC.stat().st_size == 0:
        write_json(OUT / "blocked.json", {"outcome": "blocked_external", "reason": "llm token file is absent"})
        return 4
    if sha256_file(REGISTRY) != REGISTRY_SHA:
        say("registry sha is not the sealed sr_w45 binding")
        return 2
    if not ADMISSION_BIN.is_file():
        say("admission binary is absent")
        return 2
    for name in ("oo-pipeline", "init-pg", "pg-builder", "install-column-acl", "operator-preview"):
        if not (BINS / name).is_file():
            say(f"missing binary {name}")
            return 2
    problem = elf_problem()
    if problem:
        say(problem)
        return 2
    decision = prepare_customer()
    if not customer_ready(decision):
        say("customer input was refused before the source read")
        write_json(OUT / "gates.json", {"failed": ["customer"]})
        return 2
    source_ok = False
    try:
        source_ok = run_source_session(decision)
    finally:
        stop_source_container()
        SOURCE_REPORT["removed_before_target"] = not our_source_names()
        SOURCE_REPORT["source_running_after_stop"] = our_source_names()
        write_json(OUT / "source-report.json", SOURCE_REPORT)
    if not source_ok or SOURCE_REPORT.get("source_running_after_stop"):
        failed_source = ["source"]
        if SOURCE_REPORT.get("cancel_proven") is not True:
            failed_source.append("source-cancel")
        if SOURCE_REPORT.get("write_attempts") not in (0,) or SOURCE_REPORT.get("fe_write_attempts") not in (0,):
            failed_source.append("source-writes")
        write_json(OUT / "gates.json", {"failed": failed_source, "source_error": SOURCE_REPORT.get("error")})
        say(f"source refused the catalog: {SOURCE_REPORT.get('error')}")
        return 1
    admin = os.urandom(24).hex()
    runtime = os.urandom(24).hex()
    login = os.urandom(24).hex()
    remember(admin)
    remember(runtime)
    remember(login)
    SECRET_DIR.mkdir(mode=0o700, exist_ok=True)
    for leaf, text in (("admin.password", admin), ("runtime.password", runtime), ("login.password", login), ("bearer-bad", "denied")):
        path = SECRET_DIR / leaf
        path.write_text(text, encoding="utf-8")
        os.chmod(path, 0o600)
    shutil.copyfile(TOKEN_SRC, SECRET_DIR / "llm-token")
    os.chmod(SECRET_DIR / "llm-token", 0o600)
    remember((SECRET_DIR / "llm-token").read_text(encoding="utf-8"))
    for user, secret in (("postgres", admin), (RUNTIME, runtime), (LOGIN, login)):
        pgpass = SECRET_DIR / f"{user}.pgpass"
        pgpass.write_text(f"127.0.0.1:{PG_PORT}:oo_w45:{user}:{secret}\n", encoding="utf-8")
        os.chmod(pgpass, 0o600)
    make_certs()
    (SECRET_DIR / "ready").write_text("ready\n", encoding="utf-8")
    start_postgres()
    try:
        dexec(["mkdir", "-p", "/certs", "/secrets", "/opt/oo/bin", "/work/in", "/work/out", "/frozen/read", "/frozen/decoded", "/evidence/plan", "/admission"])
        for name in ("ca.crt", "server.crt", "server.key"):
            copy_in(SECRET_DIR / name, f"/certs/{name}")
        for name in ("admin.password", "runtime.password", "login.password", "llm-token", "bearer-bad"):
            copy_in(SECRET_DIR / name, f"/secrets/{name}")
        for user in ("postgres", RUNTIME, LOGIN):
            copy_in(SECRET_DIR / f"{user}.pgpass", f"/secrets/{user}.pgpass")
            dexec(["chmod", "600", f"/secrets/{user}.pgpass"])
        install_glibc()
        copy_in(SECRET_DIR / "ready", "/certs/ready")
        wait_postgres()
        for name in ("oo-pipeline", "init-pg", "pg-builder", "install-column-acl", "operator-preview"):
            copy_in(BINS / name, f"/opt/oo/bin/{name}")
            dexec(["chmod", "755", f"/opt/oo/bin/{name}"])
        copy_in(ADMISSION_BIN, "/admission/oo-source-admission")
        dexec(["chmod", "755", "/admission/oo-source-admission"])
        copy_in(REGISTRY, "/evidence/plan/registry.json")
        copy_in(PLAN_SRC / "report.json", "/evidence/plan/report.json")
        copy_in(PLAN_SRC / "config.json", "/evidence/plan/config.json")
        copy_in(RECEIPT, "/frozen/native-readonly-receipt.json")
        for item in ADMITTED:
            name = item["name"]
            copy_in(ARROW_DIR / f"{name}.arrow", f"/frozen/read/{name}.arrow")
            copy_in(DECODED_DIR / f"{name}.json", f"/frozen/decoded/{name}.json")
            dexec(["mkdir", "-p", f"/evidence/plan/{name}"])
            copy_in(PLAN_SRC / name / "catalog.json", f"/evidence/plan/{name}/catalog.json")
        copy_in(PROXY, "/opt/oo/loopback_proxy.pl")
        init_body = {
            "protocol": "oo.pipeline.init-pg/v1",
            "target": {
                "host": "127.0.0.1",
                "port": PG_PORT,
                "database": "oo_w45",
                "administrator": "postgres",
                "password_file": "/secrets/admin.password",
                "ca_file": "/certs/ca.crt",
            },
        }
        init_code = pipeline(["/opt/oo/bin/init-pg"], init_body, OUT, "init-pg")
        say(f"init-pg {init_code}")
        if init_code != 0:
            return 1
        sealed_body = {
            "protocol": "oo.pipeline.install-sealed-catalog/v1",
            "target": init_body["target"],
        }
        sealed_code = pipeline(["/opt/oo/bin/oo-pipeline", "install-sealed-catalog"], sealed_body, OUT, "install-sealed-catalog")
        say(f"install-sealed-catalog {sealed_code}")
        if sealed_code != 0:
            return 1
        dba_roles()
        native_body = {
            "protocol": "oo.pipeline.install-native-sealed-catalog/v1",
            "target": init_body["target"],
        }
        # 0008 is installed after the seals exist only if the installer allows
        # that order. The readiness check used by the read command requires it
        # first. Install it before the loads.
        native_code = pipeline(["/opt/oo/bin/oo-pipeline", "install-native-sealed-catalog"], native_body, OUT, "install-native-sealed-catalog")
        say(f"install-native {native_code}")
        stdout = (OUT / "install-native-sealed-catalog.stdout").read_text(encoding="utf-8") if (OUT / "install-native-sealed-catalog.stdout").is_file() else ""
        if NATIVE_MIGRATION not in stdout:
            say("native migration digest was not in the installer stdout")
        sealed = import_tables(REGISTRY_SHA, [row["name"] for row in ADMITTED])
        catalogs = catalog_tables(sealed)
        summary = {
            "sealed": sorted(sealed),
            "catalogs": {
                name: {
                    "engine": doc.get("engine"),
                    "run_id": doc.get("run_id"),
                    "state": doc.get("state"),
                    "rows": (doc.get("tables") or [{}])[0].get("accepted_rows"),
                    "columns": [
                        {"name": column["logical_name"], "oid": column["oid"], "typmod": column["typmod"], "source_type": column["source_type"]}
                        for column in (doc.get("tables") or [{}])[0].get("columns") or []
                    ],
                }
                for name, doc in catalogs.items()
            },
        }
        write_json(OUT / "summary.json", summary)
        say(f"closure sealed {len(sealed)} catalogs {len(catalogs)}")
        if len(catalogs) != 5:
            return 1
        report = consume(catalogs)
        write_json(OUT / "consumer-report.json", report)
        failed = []
        for name, item in report["tables"].items():
            if item.get("engine") != "starrocks" or item.get("state") != "awaiting_publish":
                failed.append(f"{name} catalog")
            if item.get("cost_unknown") != "cost_unknown":
                failed.append(f"{name} cost_unknown")
            if item.get("pack_outcome") != "packed" or item.get("pack_exit") != 0:
                failed.append(f"{name} pack")
            if item.get("cache") != "hit":
                failed.append(f"{name} cache")
            if item.get("unauthorized_http") != 401:
                failed.append(f"{name} unauthorized")
            if item.get("timeout_reason") != "timeout":
                failed.append(f"{name} timeout")
            if item.get("query_exit") != 0 or item.get("rows_equal") is not True:
                failed.append(f"{name} query")
            if item.get("persisted_equal") is not True or item.get("temporal_equal") is not True:
                failed.append(f"{name} cells")
            if item.get("cache_model_matches_pack") is not True:
                failed.append(f"{name} cache-model")
            expected = EXPECTED_COUNTS.get(name)
            if item.get("returned_rows") != expected or item.get("import_rows") != expected or item.get("registry_rows") != expected:
                failed.append(f"{name} rows")
            identity = item.get("pack_identity") or {}
            if identity.get("source_id") != "sr_w45" or identity.get("source_engine") != "starrocks" or identity.get("identity_hash") != identity.get("query_import_hash"):
                failed.append(f"{name} pack-identity")
            if item.get("hidden_exit") != 6 or item.get("hidden_class") != "guard":
                failed.append(f"{name} hidden")
            if item.get("widen_exit") != 6:
                failed.append(f"{name} widen")
            if item.get("type_equal") is not True:
                failed.append(f"{name} type")
            if f"{LOGIN}|f|t|f" not in (item.get("role_stdout") or ""):
                failed.append(f"{name} role")
            if item.get("cancel_proven") is not True:
                failed.append(f"{name} cancel")
            off = report["source_off_queries"].get(name, {})
            if (
                off.get("exit") != 0
                or off.get("rows_equal") is not True
                or off.get("persisted_equal") is not True
                or off.get("temporal_equal") is not True
                or off.get("same_pack") is not True
                or off.get("returned_rows") != expected
            ):
                failed.append(f"{name} source-off")
        if report.get("operator_audit_ddl") or report.get("reader_ddl_attempt_count") not in (0,):
            failed.append("audit")
        if report.get("acl_exit") != 0:
            failed.append("acl")
        if SOURCE_REPORT.get("ok") is not True or SOURCE_REPORT.get("removed_before_target") is not True or SOURCE_REPORT.get("cancel_proven") is not True:
            failed.append("source")
        if SOURCE_REPORT.get("write_attempts") not in (0,) or SOURCE_REPORT.get("fe_write_attempts") not in (0,):
            failed.append("source-writes")
        secret_hits = product_secret_hits()
        write_json(OUT / "secret-scan.json", {"hits": secret_hits})
        if secret_hits:
            failed.append("secrets")
        customer = json.loads((OUT / "customer-admission.json").read_text(encoding="utf-8"))
        if customer.get("refusal_ok") is not True or customer.get("read_attempted_names") != list(EXPECTED_COUNTS):
            failed.append("customer")
        write_json(OUT / "gates.json", {"failed": failed})
        say(f"gates failed {len(failed)}")
        return 0 if not failed else 2
    finally:
        pass


def main() -> int:
    global LOG
    code = 1
    handle = None
    try:
        preserve_prior()
        OUT.mkdir(parents=True, exist_ok=True)
        handle = (OUT / "closure-run.log").open("w", encoding="utf-8")
        LOG = handle
        code = execute_closure()
    finally:
        cleanup_runtime()
        if handle is not None:
            handle.write(f"EXIT:{code}\n")
            handle.flush()
            handle.close()
        LOG = None
    return code


if __name__ == "__main__":
    raise SystemExit(main())
