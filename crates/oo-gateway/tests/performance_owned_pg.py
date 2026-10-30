#!/usr/bin/env python3
"""New synthetic performance PG; writes only under gateway-performance, never old F22 target."""
import json
import hashlib
import os
from pathlib import Path
import secrets
import socket
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[3]
BIN = Path("/opt/homebrew/opt/postgresql@17/bin")
EVIDENCE = Path(sys.argv[1]).resolve()
if EVIDENCE.name != "gateway-performance":
    raise SystemExit("evidence directory must be gateway-performance")
phase = sys.argv[2] if len(sys.argv) > 2 else "optimized"
if phase not in ("baseline", "session", "optimized", "planning"):
    raise SystemExit("baseline|session|optimized|planning required")
delay = int(sys.argv[3]) if len(sys.argv) > 3 else 0
if delay not in (0, 1, 2):
    raise SystemExit("only 0/1/2ms local response-chunk delay supported")
EVIDENCE.mkdir(parents=True, exist_ok=True)
work = Path(tempfile.mkdtemp(prefix="perf-owned-", dir=EVIDENCE))
os.chmod(work, 0o700)
pg = work / "pg"
password = secrets.token_hex(24)
nonce = secrets.token_hex(16)
(work / "password").write_text(password)
os.chmod(work / "password", 0o600)

def run(args, **kwargs):
    return subprocess.run([str(a) for a in args], check=True, **kwargs)

with socket.socket() as sock:
    sock.bind(("127.0.0.1", 0))
    port = sock.getsockname()[1]
with (work / "setup.log").open("w") as log:
    run([BIN / "initdb", "-D", pg, "-U", "postgres", "--auth-local=trust",
         "--auth-host=scram-sha-256", "--pwfile", work / "password"], stdout=log, stderr=log)
    run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes",
         "-keyout", work / "ca.key", "-out", work / "ca.crt", "-days", "2",
         "-subj", "/CN=Gateway performance owned fixture CA", "-addext", "basicConstraints=critical,CA:TRUE"],
        stdout=log, stderr=log)
    run(["openssl", "req", "-newkey", "rsa:2048", "-nodes", "-keyout", work / "server.key",
         "-out", work / "server.csr", "-subj", "/CN=localhost"], stdout=log, stderr=log)
    (work / "server.ext").write_text("basicConstraints=critical,CA:FALSE\n"
        "keyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n"
        "subjectAltName=DNS:localhost,IP:127.0.0.1\n")
    run(["openssl", "x509", "-req", "-in", work / "server.csr", "-CA", work / "ca.crt",
         "-CAkey", work / "ca.key", "-CAcreateserial", "-out", work / "server.crt",
         "-days", "2", "-extfile", work / "server.ext"], stdout=log, stderr=log)
os.chmod(work / "server.key", 0o600)
with (pg / "postgresql.conf").open("a") as f:
    f.write(f"\nlisten_addresses='127.0.0.1'\nport={port}\nunix_socket_directories=''\n"
            f"ssl=on\nssl_cert_file='{work / 'server.crt'}'\nssl_key_file='{work / 'server.key'}'\n"
            "max_connections=40\nlog_min_duration_statement=0\nlog_parameter_max_length=0\n"
            "log_line_prefix='%m [%p] %a ' \n")
started = False
try:
    run([BIN / "pg_ctl", "-D", pg, "-l", work / "postgres.log", "-w", "start"])
    started = True
    env = {k:v for k,v in os.environ.items() if not k.startswith("OO_") and not k.startswith("PG")}
    env["PGPASSWORD"] = password
    sql = f"CREATE DATABASE gateway_perf; CREATE ROLE oo_perf_login LOGIN NOINHERIT PASSWORD '{password}';"
    for name in ("a", "b", "c"):
        sql += f"CREATE ROLE oo_r_perf_{name} NOLOGIN NOINHERIT; GRANT oo_r_perf_{name} TO oo_perf_login WITH INHERIT FALSE, SET TRUE;"
    run([BIN / "psql", "-X", "-qAt", "-v", "ON_ERROR_STOP=1", "-h", "127.0.0.1", "-p", port,
         "-U", "postgres", "-d", "postgres"], input=sql, text=True, capture_output=True, env=env)
    fixture = {"protocol":"oo.gateway-performance.owned/v1", "host":"127.0.0.1", "port":port,
        "database":"gateway_perf", "work":str(work), "password_file":str(work / "password"),
        "ca_file":str(work / "ca.crt"), "response_chunk_delay_ms":delay,
        "postmaster_pid":int((pg / "postmaster.pid").read_text().splitlines()[0])}
    (work / "fixture.json").write_text(json.dumps(fixture,indent=2)+"\n")
    env.pop("PGPASSWORD")
    env.update(OO_PERF_OWNED_FIXTURE=str(work / "fixture.json"), OO_PERF_PHASE=phase,
               CARGO_TARGET_DIR=os.environ.get("CARGO_TARGET_DIR", str(EVIDENCE / "target")))
    test = ("planning_catalog_is_not_authorization" if phase == "planning"
            else "performance_owned_pg_catalog_and_roles")
    command=["cargo","test","--offline","--locked","-p","oo-gateway","--lib",
        "public_surface::performance_owned_pg::" + test,
        "--","--exact","--ignored","--nocapture","--test-threads=1"]
    with (work / "test.log").open("w") as log:
        result=subprocess.run(command,cwd=ROOT,env=env,stdout=log,stderr=subprocess.STDOUT)
    receipt={"command":command,"exit":result.returncode,"phase":phase,"response_chunk_delay_ms":delay,
        "work":str(work),"log_sha256":hashlib.sha256((work / "test.log").read_bytes()).hexdigest()}
    (work / "result.json").write_text(json.dumps(receipt,indent=2)+"\n")
    if result.returncode==0:
        (EVIDENCE / f"{phase}-delay{delay}-latest.json").write_text(json.dumps(receipt,indent=2)+"\n")
    print((work / "test.log").read_text())
    print("PERF_EVIDENCE="+str(work),flush=True)
    raise SystemExit(result.returncode)
finally:
    if started:
        run([BIN / "pg_ctl", "-D", pg, "-m", "fast", "-w", "stop"])
