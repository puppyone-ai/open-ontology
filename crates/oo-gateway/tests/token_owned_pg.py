#!/usr/bin/env python3
"""Owned PostgreSQL 17 fixture. Writes only under the requested evidence run."""
import hashlib
import json
import os
from pathlib import Path
import secrets
import shutil
import socket
import subprocess
import sys
import tempfile
import ssl
import time
import urllib.request
import urllib.error

ROOT = Path(__file__).resolve().parents[3]
os.umask(0o077)
RUN = Path(sys.argv[1]).resolve()
if not RUN.is_relative_to(ROOT / "evidence"):
    raise SystemExit("run must be within this worktree's evidence directory")
RUN.mkdir(parents=True, exist_ok=True)
WORK = Path(tempfile.mkdtemp(prefix="owned-pg-", dir=RUN))
os.chmod(WORK, 0o700)
BIN = Path(os.environ.get("OO_CI_PG_BIN", "/opt/homebrew/opt/postgresql@17/bin"))
version = subprocess.check_output([str(BIN / "postgres"), "--version"], text=True)
if " 17." not in version:
    raise SystemExit("PostgreSQL 17 required")
passwords = {k: secrets.token_hex(24) for k in ("super", "admin", "runtime")}
for name, secret in passwords.items():
    path = WORK / (name + ".pass")
    path.write_text(secret)
    path.chmod(0o600)

def run(args, **kw):
    return subprocess.run(list(map(str, args)), check=True, timeout=900, **kw)

with socket.socket() as sock:
    sock.bind(("127.0.0.1", 0))
    port = sock.getsockname()[1]
(WORK / "endpoint.json").write_text(json.dumps({"host": "localhost", "port": port, "database": "postgres"}))
with (WORK / "setup.log").open("w") as log:
    run([BIN / "initdb", "-D", WORK / "pg", "-U", "postgres", "--auth-local=trust", "--auth-host=scram-sha-256", "--pwfile", WORK / "super.pass"], stdout=log, stderr=log)
    run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", WORK / "ca.key", "-out", WORK / "ca.crt", "-days", "2", "-subj", "/CN=Owned token fixture CA", "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign"], stdout=log, stderr=log)
    for name, eku, san in (("server", "serverAuth", "DNS:localhost,IP:127.0.0.1"), ("client", "clientAuth", "URI:urn:oo:workload:token-admin")):
        run(["openssl", "req", "-newkey", "rsa:2048", "-nodes", "-keyout", WORK / f"{name}.key", "-out", WORK / f"{name}.csr", "-subj", f"/CN=Owned {name}"], stdout=log, stderr=log)
        ext = WORK / f"{name}.ext"
        ext.write_text(f"basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage={eku}\nsubjectAltName={san}\nauthorityKeyIdentifier=keyid,issuer\nsubjectKeyIdentifier=hash\n")
        run(["openssl", "x509", "-req", "-in", WORK / f"{name}.csr", "-CA", WORK / "ca.crt", "-CAkey", WORK / "ca.key", "-CAcreateserial", "-out", WORK / f"{name}.crt", "-days", "2", "-extfile", ext], stdout=log, stderr=log)
        (WORK / f"{name}.key").chmod(0o600)
(WORK / "ca.key").chmod(0o600)
with (WORK / "pg/postgresql.conf").open("a") as out:
    out.write(f"\nlisten_addresses='127.0.0.1'\nport={port}\nunix_socket_directories=''\nssl=on\nssl_cert_file='{WORK / 'server.crt'}'\nssl_key_file='{WORK / 'server.key'}'\nmax_connections=30\nlog_statement='none'\nlog_min_error_statement=panic\n")
started = False
gateway = None
try:
    run([BIN / "pg_ctl", "-D", WORK / "pg", "-l", WORK / "postgres.log", "-w", "start"], capture_output=True)
    started = True
    def sql(text, database="postgres", user="postgres"):
        env = dict(os.environ, PGPASSWORD=passwords["super" if user == "postgres" else "admin"])
        result = subprocess.run([str(BIN / "psql"), "-XqAt", "-v", "ON_ERROR_STOP=1", "-h", "localhost", "-p", str(port), "-U", user, "-d", database], input=text, text=True, capture_output=True, env=env)
        if result.returncode:
            raise RuntimeError("fixture SQL failed (statement and stderr withheld)")
        return result.stdout
    sql(f"CREATE ROLE oo_token_admin LOGIN PASSWORD '{passwords['admin']}'; CREATE ROLE oo_token_runtime LOGIN PASSWORD '{passwords['runtime']}'; GRANT CREATE ON DATABASE postgres TO oo_token_admin;")
    sql("CREATE DATABASE upgrade OWNER oo_token_admin;")
    sql("CREATE DATABASE meta OWNER oo_token_admin;")
    sql("CREATE ROLE oo_r_meta NOLOGIN NOINHERIT; GRANT oo_r_meta TO oo_token_admin WITH INHERIT FALSE, SET TRUE;")
    v1 = (ROOT / "tools/gateway-token-store/migrations/oo_token_control_v1.sql").read_text()
    checksum = hashlib.sha256(v1.encode()).hexdigest()
    sql(v1 + f"INSERT INTO oo_token_control.install_manifest VALUES (true,'{checksum}'); INSERT INTO oo_token_control.token_record (token_sha256,token_kind,project_id,principal_id,role_name,claims_canonical,policy_version,knowledge_version,audience_url,issued_at_ms,absolute_deadline_ms,last_used_ms,revocation_floor) VALUES (repeat('a',64),'client','legacy','alice','reader','{{}}','v1','v1','https://gateway.example/',1000,2000,1000,0);", "upgrade", "oo_token_admin")
    env = dict(os.environ, OO_TOKEN_FIXTURE=str(WORK))
    with (RUN / "real-pg.log").open("w") as log:
        result = subprocess.run(["cargo", "test", "--locked", "-p", "oo-gateway", "--test", "token_lifecycle", "--", "--ignored", "--nocapture", "--test-threads=1"], cwd=ROOT, env=env, stdout=log, stderr=log)
    if result.returncode:
        raise RuntimeError("tests failed; see real-pg.log")
    with (RUN / "meta-pg.log").open("w") as log:
        result = subprocess.run(["cargo", "test", "--locked", "-p", "oo-gateway", "--lib", "token_meta_pg", "--", "--ignored", "--nocapture"], cwd=ROOT, env=env, stdout=log, stderr=log)
    if result.returncode:
        raise RuntimeError("meta tests failed; see meta-pg.log")
    # Real HTTP/TLS and CLI checks use only this newly created fixture.
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        gateway_port = sock.getsockname()[1]
    audience = f"https://localhost:{gateway_port}/"
    shutil.copytree(WORK / "meta/packs", WORK / "packs")
    shutil.copytree(WORK / "meta/reviewed", WORK / "reviewed")
    (WORK / "wal").mkdir()
    (WORK / "data-endpoint.json").write_text(json.dumps({"host": "localhost", "port": port, "database": "meta"}))
    (WORK / "config.json").write_text(json.dumps({
        "listen": f"127.0.0.1:{gateway_port}", "audience": audience,
        "runtime_user": "oo_token_runtime", "password_file": "runtime.pass",
        "ca_file": "ca.crt", "endpoint_file": "endpoint.json", "pack_dir": "packs",
        "body_limit": 65536, "request_timeout_ms": 10000, "global_concurrency": 4,
        "transport": "tls", "tls_key_file": "server.key", "tls_chain_file": "server.crt",
        "client_ca_file": "ca.crt", "handshake_timeout_ms": 5000,
        "token_admin_password_file": "admin.pass",
        "token_admin_workload": "urn:oo:workload:token-admin",
        "data_endpoint_file": "data-endpoint.json", "data_password_file": "admin.pass",
        "data_ca_file": "ca.crt", "data_user": "oo_token_admin",
        "wal_dir": "wal", "reviewed_dir": "reviewed",
    }))
    with (WORK / "gateway.log").open("w") as log:
        gateway = subprocess.Popen([str(ROOT / "target/debug/oo-gateway"), str(WORK)],
                                   stdout=log, stderr=log, cwd=ROOT)
    for _ in range(100):
        if (WORK / "listen.addr").exists():
            break
        if gateway.poll() is not None:
            raise RuntimeError("owned Gateway startup failed")
        time.sleep(0.1)
    else:
        raise RuntimeError("owned Gateway startup timed out")
    public_tls = ssl.create_default_context(cafile=str(WORK / "ca.crt"))
    admin_tls = ssl.create_default_context(cafile=str(WORK / "ca.crt"))
    admin_tls.load_cert_chain(str(WORK / "client.crt"), str(WORK / "client.key"))
    def http(path, body=None, token=None, admin=False):
        headers = {"Content-Type": "application/json"}
        if token:
            headers["Authorization"] = "Bearer " + token
        req = urllib.request.Request(audience.rstrip("/") + path,
                                     data=None if body is None else json.dumps(body).encode(),
                                     headers=headers)
        try:
            response = urllib.request.urlopen(req, context=admin_tls if admin else public_tls, timeout=15)
        except urllib.error.HTTPError as error:
            response = error
        return response.status, json.loads(response.read()), response.headers
    def admin_op(operation, **kw):
        status, value, headers = http("/internal/v1/tokens", {"operation": operation, **kw}, admin=True)
        if status != 200:
            raise RuntimeError(f"admin {operation} failed: HTTP {status}; code={value.get('error', {}).get('code')}; reason={value.get('error', {}).get('message')}")
        return value, headers
    g = {"project_id": "http", "principal_id": "alice", "role": "reader", "claims": {},
         "policy_version": "v1", "knowledge_version": "v1", "audience_url": audience,
         "ttl_ms": 600000, "purpose": "owned-http-test"}
    assert http("/internal/v1/tokens", {"operation": "issue", "grant": g})[0] != 200
    meta_status, meta, _ = http("/v1/meta")
    assert meta_status == 200 and meta["minimum_cli_version"] == "0.2.0"
    assert meta["publication_id"] is None and meta["freshness"] is None
    issued, headers = admin_op("issue", grant=g)
    assert headers["Cache-Control"] == "no-store" and issued["token"].startswith("ooct_")
    status, health, _ = http("/v1/health", token=issued["token"])
    assert status == 200 and health["token_expiry"]["status"] == "warning"
    assert health["internal_mtls_certificate_expiry"]["status"] == "warning"
    assert http("/v1/health")[0] != 200
    expiring, _ = admin_op("issue", grant={**g, "ttl_ms": 1})
    time.sleep(0.01)
    assert http("/v1/health", token=issued["token"])[1]["token_expiry"]["status"] == "expired"
    admin_op("revoke", project="http", hash=expiring["token_sha256"])
    enrolled, headers = admin_op("enroll", grant=g)
    status, bound, headers = http("/v1/auth/enroll", {"code": enrolled["code"]})
    assert status == 200 and headers["Cache-Control"] == "no-store"
    assert bound["token"].startswith("ooct_")
    status, who, _ = http("/v1/auth/whoami", token=bound["token"])
    assert status == 200 and who["principal_id"] == "alice" and who["role"] == "reader"
    assert who["expires_at_ms"] == bound["expires_at_ms"] and "token" not in who
    assert http("/v1/auth/enroll", {"code": enrolled["code"]})[0] != 200
    assert http("/v1/auth/renew", {}, token=bound["token"])[1]["error"]["message"] == "self_renew_disabled"
    admin_op("policy", project="http", policy={
        "max_lifetime_ms": 90 * 86400000, "default_ttl_ms": 3600000,
        "self_renew_enabled": True, "self_renew_window_ms": 86400000,
        "rotate_overlap_ms": 604800000, "enroll_ttl_ms": 86400000,
    })
    assert http("/v1/auth/renew", {}, token=bound["token"])[0] == 200
    admin_op("renew", project="http", hash=issued["token_sha256"],
             deadline_ms=issued["expires_at_ms"] + 1000)
    rotated, _ = admin_op("rotate", project="http", hash=issued["token_sha256"])
    assert rotated["token"] != issued["token"]
    listed, _ = admin_op("list", project="http")
    assert "ooct_" not in json.dumps(listed)
    admin_op("revoke", project="http", hash=rotated["token_sha256"])
    assert http("/v1/health", token=rotated["token"])[0] != 200
    # CLI credentials are file arguments; outputs never print plaintext.
    binary = ROOT / "target/debug/issue-token"
    material = [WORK / "endpoint.json", WORK / "admin.pass", WORK / "ca.crt"]
    request = WORK / "cli-request.json"
    output = WORK / "cli-enrollment"
    request.write_text(json.dumps({"grant": g}))
    cli = run([binary, "enroll", *material, request, output], capture_output=True, text=True)
    assert "OO-" not in cli.stdout and output.stat().st_mode & 0o777 == 0o600
    status, cli_bound, _ = http("/v1/auth/enroll", {"code": output.read_text().strip()})
    assert status == 200
    request.write_text(json.dumps({"project": "http", "hash": cli_bound["token_sha256"]}))
    rotated_file = WORK / "cli-rotated"
    cli = run([binary, "rotate", *material, request, rotated_file], capture_output=True, text=True)
    assert "ooct_" not in cli.stdout and rotated_file.stat().st_mode & 0o777 == 0o600
    request.write_text(json.dumps({"project": "http"}))
    cli = run([binary, "list", *material, request], capture_output=True, text=True)
    assert "ooct_" not in cli.stdout
    run([binary, "install", *material], capture_output=True)
    request.write_text(json.dumps({**g, "kind": "client", "claims": [],
                                   "client_ttl_ms": 600000}))
    issue_file = WORK / "cli-issued"
    cli = run([binary, "issue", *material, request, issue_file], capture_output=True, text=True)
    assert "ooct_" not in cli.stdout and issue_file.stat().st_mode & 0o777 == 0o600
    assert cli.stdout.startswith("hash ")
    issue_hash = cli.stdout.strip().removeprefix("hash ")
    request.write_text(json.dumps({"project": "http", "hash": issue_hash,
                                   "deadline_ms": int(time.time() * 1000) + 3600000}))
    run([binary, "renew", *material, request], capture_output=True)
    run([binary, "revoke", *material, "http", issue_hash], capture_output=True)
    assert http("/v1/health", token=issue_file.read_text().strip())[0] != 200
    request.write_text(json.dumps({"project": "cli-policy", "policy": {
        "max_lifetime_ms": 90 * 86400000, "default_ttl_ms": 90 * 86400000,
        "self_renew_enabled": False, "self_renew_window_ms": 14 * 86400000,
        "rotate_overlap_ms": 7 * 86400000, "enroll_ttl_ms": 86400000,
    }}))
    run([binary, "policy", *material, request], capture_output=True)
    # No certificate bypass: an unknown CA must not connect.
    try:
        urllib.request.urlopen(audience + "v1/meta", context=ssl.create_default_context(), timeout=5)
    except urllib.error.URLError:
        pass
    else:
        raise AssertionError("TLS trust was bypassed")
    # Installed npm entrypoint, real TLS enrollment, role-scoped tables and native query.
    from token_cli_e2e import cli_e2e
    # Respect the production ten-attempts/minute enrollment limiter.
    time.sleep(61)
    cli_e2e(ROOT, RUN, WORK, audience, http, admin_op)
    for _ in range(10):
        last = http("/v1/auth/enroll", {"code": "OO-0000-0000"})[0]
    assert last == 429
    (RUN / "http-cli-summary.json").write_text(json.dumps({
        "https_without_client_certificate": True, "admin_workload_required": True,
        "issue_list_renew_rotate_revoke_enroll": "passed", "single_use": "passed",
        "self_renew_policy": "passed", "meta": "passed (unconfigured publication)",
        "health_token_and_certificate_warning": "passed", "enrollment_rate_limit": "passed",
        "tls_trust_failure": "passed", "cli_private_output": "passed"
    }, indent=2))
    (RUN / "fixture-summary.json").write_text(json.dumps({"engine": version.strip(), "v1_sha256": checksum, "test_exit": result.returncode, "fixture": WORK.name}, indent=2))
finally:
    try:
        if gateway is not None:
            gateway.terminate()
            try:
                gateway.wait(timeout=15)
            except subprocess.TimeoutExpired:
                gateway.kill()
                gateway.wait(timeout=15)
    finally:
        if started:
            run([BIN / "pg_ctl", "-D", WORK / "pg", "-m", "fast", "-w", "stop"], capture_output=True)
        (RUN / "cleanup.json").write_text(json.dumps({
            "owned_postgres_stopped": True, "owned_gateway_stopped": True, "containers_created": 0
        }))
