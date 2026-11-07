#!/usr/bin/env python3
"""Own a PostgreSQL 17.11 fixture under evidence; never use an existing server."""
import argparse
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--cli", action="store_true", help="also SIGKILL the real mTLS first-token client")
    args = parser.parse_args()
    assert args.run_id and all(c.isalnum() or c == "-" for c in args.run_id)
    os.umask(0o077)
    out = ROOT / "evidence" / args.run_id
    out.mkdir(mode=0o700)
    work = out / "private"
    work.mkdir(mode=0o700)
    pg = Path(os.environ.get("OO_CI_PG_BIN", "/opt/homebrew/opt/postgresql@17/bin"))
    version = subprocess.check_output([str(pg / "postgres"), "--version"], text=True).strip()
    assert " 17.11 " in version
    secret = secrets.token_hex(32)
    (work / "admin.pass").write_text(secret)
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    (work / "endpoint.json").write_text(json.dumps(dict(host="localhost", port=port, database="postgres")))
    started = False
    gateway = None
    result = dict(status="failed", engine=version, containers_left=0)
    with (out / "fixture.log").open("w") as log:
        def run(argv, **kwargs):
            return subprocess.run(list(map(str, argv)), check=True, stdout=log, stderr=log, **kwargs)
        run([pg / "initdb", "-D", work / "pg", "-U", "postgres",
             "--auth-local=trust", "--auth-host=scram-sha-256", "--pwfile", work / "admin.pass"])
        run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes",
             "-keyout", work / "ca.key", "-out", work / "ca.crt", "-days", "1",
             "-subj", "/CN=Owned retry fixture", "-addext", "basicConstraints=critical,CA:TRUE",
             "-addext", "keyUsage=critical,keyCertSign,cRLSign"])
        run(["openssl", "req", "-newkey", "rsa:2048", "-nodes", "-keyout", work / "server.key",
             "-out", work / "server.csr", "-subj", "/CN=localhost"])
        (work / "server.ext").write_text(
            "basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\n"
            "extendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n"
            "authorityKeyIdentifier=keyid,issuer\nsubjectKeyIdentifier=hash\n")
        run(["openssl", "x509", "-req", "-in", work / "server.csr", "-CA", work / "ca.crt",
             "-CAkey", work / "ca.key", "-CAcreateserial", "-out", work / "server.crt",
             "-days", "1", "-extfile", work / "server.ext"])
        with (work / "pg/postgresql.conf").open("a") as config:
            config.write(f"\nlisten_addresses='127.0.0.1'\nport={port}\nunix_socket_directories=''\n"
                         f"ssl=on\nssl_cert_file='{work / 'server.crt'}'\nssl_key_file='{work / 'server.key'}'\n"
                         "log_statement='none'\nlog_min_error_statement=panic\n")
        try:
            run([pg / "pg_ctl", "-D", work / "pg", "-l", work / "postgres.log", "-w", "start"])
            started = True
            env = dict(os.environ, PGPASSWORD=secret)
            # Password stays in stdin/environment, never argv or a command log.
            setup = subprocess.run([str(pg / "psql"), "-Xq", "-v", "ON_ERROR_STOP=1",
                "-h", "localhost", "-p", str(port), "-U", "postgres", "-d", "postgres"],
                input=f"CREATE ROLE oo_token_admin LOGIN PASSWORD '{secret}';"
                      "CREATE ROLE oo_token_runtime NOLOGIN;"
                      "GRANT CREATE ON DATABASE postgres TO oo_token_admin;",
                text=True, capture_output=True, env=env)
            assert setup.returncode == 0, "fixture SQL failed (details withheld)"
            with (out / "test.log").open("w") as test_log:
                test = subprocess.run(["cargo", "test", "--locked", "-p", "oo-gateway", "--lib",
                    "owned_pg_issue_retry", "--", "--ignored", "--exact",
                    "http_gateway::token_lifecycle::retry_tests::owned_pg_issue_retry", "--nocapture"],
                    cwd=ROOT, env=dict(os.environ, OO_RETRY_FIXTURE=str(work)),
                    stdout=test_log, stderr=test_log)
            assert test.returncode == 0, "retry test failed; see test.log"
            assert "1 passed" in (out / "test.log").read_text()
            if args.cli:
                run(["openssl", "req", "-newkey", "rsa:2048", "-nodes", "-keyout", work / "client.key",
                     "-out", work / "client.csr", "-subj", "/CN=Owned retry client"])
                (work / "client.ext").write_text(
                    "basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\n"
                    "extendedKeyUsage=clientAuth\nsubjectAltName=URI:urn:oo:workload:retry-admin\n")
                run(["openssl", "x509", "-req", "-in", work / "client.csr", "-CA", work / "ca.crt",
                     "-CAkey", work / "ca.key", "-CAcreateserial", "-out", work / "client.crt",
                     "-days", "1", "-extfile", work / "client.ext"])
                (work / "client.pem").write_bytes((work / "client.crt").read_bytes() + (work / "client.key").read_bytes())
                (work / "packs").mkdir()
                with socket.socket() as sock:
                    sock.bind(("127.0.0.1", 0))
                    gateway_port = sock.getsockname()[1]
                audience = f"https://localhost:{gateway_port}/"
                (work / "config.json").write_text(json.dumps(dict(
                    listen=f"127.0.0.1:{gateway_port}", audience=audience, transport="tls",
                    runtime_user="oo_token_runtime", password_file="admin.pass", ca_file="ca.crt",
                    endpoint_file="endpoint.json", pack_dir="packs", body_limit=65536,
                    request_timeout_ms=10000, global_concurrency=4, tls_key_file="server.key",
                    tls_chain_file="server.crt", client_ca_file="ca.crt", handshake_timeout_ms=5000,
                    token_admin_password_file="admin.pass", token_admin_workload="urn:oo:workload:retry-admin")))
                publication = dict(gateway_url=audience, ca_file=str(work / "ca.crt"),
                    client_pem=str(work / "client.pem"), first_user_grant=dict(
                        project_id="cli-retry-fixture", principal_id="owner", role="reader",
                        claims={}, policy_version="v1", knowledge_version="v1",
                        audience_url=audience, ttl_ms=600000, purpose="build"))
                publication_file = work / "publication.json"
                publication_file.write_text(json.dumps(publication))
                with (out / "gateway.log").open("w") as gateway_log:
                    gateway = subprocess.Popen([str(ROOT / "target/debug/oo-gateway"), str(work)],
                        stdout=gateway_log, stderr=gateway_log)
                deadline = time.monotonic() + 15
                while not (work / "listen.addr").exists() and time.monotonic() < deadline:
                    assert gateway.poll() is None, "owned Gateway startup failed"
                    time.sleep(.05)
                assert (work / "listen.addr").exists()
                with (out / "cli.log").open("w") as cli_log:
                    test = subprocess.run(["cargo", "test", "--locked", "-p", "oo-cli", "--lib",
                        "owned_gateway_first_token", "--", "--ignored", "--exact",
                        "args::admin::build_token::tests::owned_gateway_first_token", "--nocapture"],
                        cwd=ROOT, env=dict(os.environ, OO_BUILD_TOKEN_PUBLICATION=str(publication_file)),
                        stdout=cli_log, stderr=cli_log)
                assert test.returncode == 0 and "1 passed" in (out / "cli.log").read_text(), "CLI retry failed"
                observed = subprocess.run([str(pg / "psql"), "-XqAt", "-h", "localhost",
                    "-p", str(port), "-U", "postgres", "-d", "postgres"],
                    input="SELECT count(*) FROM oo_token_control.token_record WHERE project_id='cli-retry-fixture';",
                    text=True, capture_output=True, env=env, check=True)
                assert observed.stdout.strip() == "1", "interrupted CLI issued a duplicate"
                result.update(cli_sigkill_after_remote_commit="passed", cli_credentials=1,
                              gateway="real_mtls_lifecycle_only", published_query=False)
            result["status"] = "passed"
        finally:
            if gateway:
                gateway.terminate()
                try:
                    gateway.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    gateway.kill()
                    gateway.wait()
            if started:
                run([pg / "pg_ctl", "-D", work / "pg", "-m", "fast", "-w", "stop"])
            (out / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
