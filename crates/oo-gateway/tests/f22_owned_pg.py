#!/usr/bin/env python3
"""Create a new loopback-only PG cluster for F22, run the real Gateway tests, stop it."""
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
handoff = sys.argv[2:] == ["--handoff"]
lifecycle = sys.argv[2:] == ["--lifecycle"]
if sys.argv[2:] and not (handoff or lifecycle):
    raise SystemExit("only --handoff or --lifecycle is accepted")
EVIDENCE.mkdir(parents=True, exist_ok=True)
work = Path(tempfile.mkdtemp(prefix="f22-owned-", dir=EVIDENCE))
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
kept = False
gateway = None
try:
    run([BIN / "pg_ctl", "-D", pg, "-l", work / "postgres.log", "-w", "start"])
    started = True
    env = dict(os.environ, PGPASSWORD=password)
    def sql(text):
        return run([BIN / "psql", "-X", "-qAt", "-v", "ON_ERROR_STOP=1", "-h", "127.0.0.1",
                    "-p", port, "-U", "postgres", "-d", "postgres"], input=text, text=True,
                   capture_output=True, env=env).stdout
    sql(f"""CREATE DATABASE f22_owned;
    CREATE ROLE oo_token_admin LOGIN SUPERUSER PASSWORD '{password}';
    CREATE ROLE oo_token_runtime LOGIN NOINHERIT PASSWORD '{password}';
    CREATE ROLE oo_f22_login LOGIN NOINHERIT PASSWORD '{password}';
    CREATE ROLE oo_r_f22 NOLOGIN NOINHERIT;
    GRANT oo_r_f22 TO oo_f22_login WITH INHERIT FALSE, SET TRUE;
    """)
    fixture = {"owned_fixture": "f22-owned/v1", "host": "127.0.0.1", "port": port,
               "database": "f22_owned", "password_file": str(work / "password"),
               "ca_file": str(work / "ca.crt"), "nonce": nonce, "work": str(work),
               "postmaster_pid": int((pg / "postmaster.pid").read_text().splitlines()[0])}
    fixture_path = work / "fixture.json"
    fixture_path.write_text(json.dumps(fixture, indent=2) + "\n")
    env = dict(os.environ, OO_F22_OWNED_FIXTURE=str(fixture_path),
               CARGO_TARGET_DIR=os.environ.get("CARGO_TARGET_DIR", str(ROOT / "target")))
    build = ["cargo", "build", "--offline", "--locked", "-p", "oo-cli"]
    if handoff:
        build += ["-p", "oo-gateway", "--bin", "oo", "--bin", "oo-gateway"]
    with (work / "cli-build.log").open("w") as log:
        run(build, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
    cli = Path(env["CARGO_TARGET_DIR"]) / "debug/oo"
    env["OO_F22_CLI"] = str(cli)
    (work / "cli-binary.json").write_text(json.dumps({
        "command": build, "exit": 0, "path": str(cli),
        "sha256": hashlib.sha256(cli.read_bytes()).hexdigest(),
    }, indent=2) + "\n")
    command = ["cargo", "test", "--offline", "--locked", "-p", "oo-gateway", "--lib",
               "public_surface::f22_owned_pg::" + ("f22_owned_pg_prepare_runtime" if handoff else "lifecycle::query_lifecycle_owned_pg" if lifecycle else "f22_owned_pg_gateway_and_executor"),
               "--", "--exact", "--ignored", "--nocapture", "--test-threads=1"]
    with (work / "test.log").open("w") as log:
        result = subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
    (work / "result.json").write_text(json.dumps({"command": command, "exit": result.returncode,
        "fixture": str(fixture_path), "ownership": "new dedicated local cluster; no existing database touched"}) + "\n")
    print((work / "test.log").read_text())
    print(f"F22_EVIDENCE={work}\nexit={result.returncode}")
    if handoff and result.returncode == 0:
        gateway_bin = Path(env["CARGO_TARGET_DIR"]) / "debug/oo-gateway"
        with (work / "gateway.stdout").open("w") as out, (work / "gateway.stderr").open("w") as err:
            gateway = subprocess.Popen([str(gateway_bin), str(work)], cwd=ROOT, env=env,
                stdin=subprocess.DEVNULL, stdout=out, stderr=err, start_new_session=True)
        for _ in range(100):
            if gateway.poll() is not None:
                raise RuntimeError("owned Gateway failed; inspect gateway.stderr")
            if (work / "listen.addr").is_file():
                break
            time.sleep(0.1)
        address = (work / "listen.addr").read_text().strip()
        if not address.startswith("127.0.0.1:"):
            raise RuntimeError("not a loopback listener")
        connection_args = ["--gateway", "http://" + address, "--transport", "local-development",
                           "--auth", "external", "--token-file", str(work / "client.token")]
        smoke = run([cli, "query", "--json", *connection_args,
                     "--expected-publication-id", "current", "SELECT amount FROM app.items"],
                    capture_output=True, text=True, env=env)
        receipt = json.loads(smoke.stdout)
        if "publication_binding" not in receipt:
            raise RuntimeError("actual standalone Gateway did not acknowledge binding")
        (work / "standalone-acquire.json").write_text(json.dumps(receipt, indent=2) + "\n")
        runtime = {"protocol":"oo.f22.owned-runtime/v1","fixture":str(fixture_path),
            "config_dir":str(work),"origin":"http://"+address,"gateway_pid":gateway.pid,
            "gateway_binary":str(gateway_bin),"gateway_binary_sha256":hashlib.sha256(gateway_bin.read_bytes()).hexdigest(),
            "cli_binary":str(cli),"cli_binary_sha256":hashlib.sha256(cli.read_bytes()).hexdigest(),
            "connection_args":connection_args,"token_file":str(work / "client.token"),
            "owner":"gateway F22; handed to root for consumer integration",
            "initial_receipt":str(work / "standalone-acquire.json"),
            "controlled_hook":str(Path(__file__).with_name("f22_runtime.py")),
            "cleanup":"running until root invokes stop; no auto cleanup on handoff"}
        (work / "runtime.json").write_text(json.dumps(runtime, indent=2) + "\n")
        (EVIDENCE / "ACTIVE-RUNTIME.json").write_text(json.dumps(runtime, indent=2) + "\n")
        kept = True
        print("F22_RUNTIME=" + str(work / "runtime.json"), flush=True)
    sys.exit(result.returncode)
finally:
    if gateway is not None and not kept:
        gateway.terminate()
        gateway.wait(timeout=5)
    if started and not kept:
        run([BIN / "pg_ctl", "-D", pg, "-m", "fast", "-w", "stop"])
