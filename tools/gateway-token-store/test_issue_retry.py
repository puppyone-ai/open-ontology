#!/usr/bin/env python3
"""Own a PostgreSQL 17.11 fixture under evidence; never use an existing server."""
import argparse
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-id", required=True)
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
            result["status"] = "passed"
        finally:
            if started:
                run([pg / "pg_ctl", "-D", work / "pg", "-m", "fast", "-w", "stop"])
            (out / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
