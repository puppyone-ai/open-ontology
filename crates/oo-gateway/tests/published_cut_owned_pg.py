#!/usr/bin/env python3
"""Bounded published-cut test using the existing F22 owned-PG harness setup.
Target fixture only: no source, cloud, customer database, or model calls.
"""
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
EVIDENCE.mkdir(parents=True, exist_ok=True)
work = Path(tempfile.mkdtemp(prefix="published-cut-owned-", dir=EVIDENCE))
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
         "-subj", "/CN=F22 owned fixture CA", "-addext", "basicConstraints=critical,CA:TRUE"],
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
            "max_connections=30\n")
started = False
try:
    run([BIN / "pg_ctl", "-D", pg, "-l", work / "postgres.log", "-w", "start"])
    started = True
    env = dict(os.environ, PGPASSWORD=password)
    def sql(text):
        return run([BIN / "psql", "-X", "-qAt", "-v", "ON_ERROR_STOP=1", "-h", "127.0.0.1",
                    "-p", port, "-U", "postgres", "-d", "postgres"], input=text, text=True,
                   capture_output=True, env=env).stdout
    sql(f"""CREATE DATABASE published_cut_owned;
    CREATE ROLE oo_token_admin LOGIN SUPERUSER PASSWORD '{password}';
    CREATE ROLE oo_token_runtime LOGIN NOINHERIT PASSWORD '{password}';
    CREATE ROLE oo_f22_login LOGIN NOINHERIT PASSWORD '{password}';
    CREATE ROLE oo_r_f22 NOLOGIN NOINHERIT;
    GRANT oo_r_f22 TO oo_f22_login WITH INHERIT FALSE, SET TRUE;
    """)
    fixture = {"owned_fixture": "published-cut-owned/v1", "host": "127.0.0.1", "port": port,
               "database": "published_cut_owned", "password_file": str(work / "password"),
               "ca_file": str(work / "ca.crt"), "nonce": nonce, "work": str(work),
               "postmaster_pid": int((pg / "postmaster.pid").read_text().splitlines()[0])}
    fixture_path = work / "fixture.json"
    fixture_path.write_text(json.dumps(fixture, indent=2) + "\n")
    env = dict(os.environ, OO_PUBLISHED_CUT_FIXTURE=str(fixture_path),
               CARGO_TARGET_DIR=os.environ.get("CARGO_TARGET_DIR", str(EVIDENCE / "target")))
    command = ["cargo", "test", "--offline", "--locked", "-p", "oo-gateway", "--lib",
               "public_surface::published_cut_owned_pg::published_cut_survives_pending_generation",
               "--", "--exact", "--ignored", "--nocapture", "--test-threads=1"]
    with (work / "test.log").open("w") as log:
        result = subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=600)
    (work / "result.json").write_text(json.dumps({"command": command, "exit": result.returncode,
        "fixture": str(fixture_path), "ownership": "new dedicated local cluster; no existing database touched"}) + "\n")
    print((work / "test.log").read_text())
    print(f"F22_EVIDENCE={work}\nexit={result.returncode}")
    sys.exit(result.returncode)
finally:
    if started:
        run([BIN / "pg_ctl", "-D", pg, "-m", "fast", "-w", "stop"])
