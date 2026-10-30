"""Own a temporary loopback PostgreSQL instance; never reuse an existing server."""
import argparse
import json
import os
from pathlib import Path
import secrets
import shutil
import socket
import subprocess
import tempfile


def run(evidence, positive=False, native=False):
    positive = positive or native
    os.umask(0o077)
    evidence.mkdir(mode=0o700, parents=True, exist_ok=False)
    crate = Path(__file__).resolve().parents[1]
    binaries = {name: shutil.which(name) for name in ("initdb", "pg_ctl", "psql", "openssl", "cargo")}
    if not all(binaries.values()):
        raise RuntimeError("local_postgres_openssl_and_cargo_required")
    exits = []
    password = secrets.token_urlsafe(32)
    bootstrap_password = secrets.token_urlsafe(32)
    actors = {kind: secrets.token_urlsafe(32) for kind in ("executor", "oracle", "evaluator", "reviewer")}
    with (evidence / "fixture.log").open("w") as log:
        def command(args, *, data=None, env=None):
            result = subprocess.run(args, input=data, text=True, capture_output=True, env=env, cwd=crate)
            output = result.stdout + result.stderr
            for secret in (password, bootstrap_password, *actors.values()):
                output = output.replace(secret, "[REDACTED]")
            log.write(output)
            log.flush()
            exits.append({"program": Path(args[0]).name, "exit_code": result.returncode})
            return result.returncode

        # PG sockets have a short platform path limit; keep these owned files in
        # a private temporary root, not any existing installation/data directory.
        with tempfile.TemporaryDirectory(prefix="oo-business-guard-", dir="/tmp") as temporary:
            root = Path(temporary)
            data = root / "data"
            password_file = root / "password"
            password_file.write_text(password)
            bootstrap_file = root / "bootstrap-password"
            bootstrap_file.write_text(bootstrap_password)
            database = "oo_business_guard_" + secrets.token_hex(6)
            with socket.socket() as listener:
                listener.bind(("127.0.0.1", 0))
                port = listener.getsockname()[1]
            assert command([binaries["initdb"], "-D", str(data), "-U", "postgres",
                            "--auth-local=trust", "--auth-host=scram-sha-256",
                            "--pwfile=" + str(bootstrap_file), "--no-locale", "-E", "UTF8"]) == 0
            assert command([binaries["openssl"], "req", "-x509", "-newkey", "rsa:2048", "-nodes",
                            "-keyout", str(root / "ca.key"), "-out", str(root / "ca.crt"),
                            "-days", "1", "-subj", "/CN=Owned business guard fixture CA",
                            "-addext", "basicConstraints=critical,CA:TRUE"]) == 0
            assert command([binaries["openssl"], "req", "-new", "-newkey", "rsa:2048", "-nodes",
                            "-keyout", str(data / "server.key"), "-out", str(root / "server.csr"),
                            "-subj", "/CN=localhost"]) == 0
            extensions = root / "leaf.ext"
            extensions.write_text("basicConstraints=critical,CA:FALSE\n"
                                  "keyUsage=critical,digitalSignature,keyEncipherment\n"
                                  "extendedKeyUsage=serverAuth\n"
                                  "subjectAltName=DNS:localhost,IP:127.0.0.1\n")
            assert command([binaries["openssl"], "x509", "-req", "-in", str(root / "server.csr"),
                            "-CA", str(root / "ca.crt"), "-CAkey", str(root / "ca.key"),
                            "-CAcreateserial", "-out", str(data / "server.crt"), "-days", "1",
                            "-extfile", str(extensions)]) == 0
            (data / "server.key").chmod(0o600)
            with (data / "postgresql.conf").open("a") as config:
                config.write(f"\nlisten_addresses='127.0.0.1'\nport={port}\n"
                             f"unix_socket_directories='{root}'\nssl=on\n"
                             "password_encryption='scram-sha-256'\n")
            started = False
            code = 1
            try:
                started = command([binaries["pg_ctl"], "-D", str(data), "-l",
                                   str(root / "postgres.log"), "-w", "start"]) == 0
                assert started
                # Credentials go only through private stdin/files, never logs/argv.
                setup = (f"CREATE ROLE oo_release_admin LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE "
                         f"PASSWORD '{password}';\nCREATE ROLE oo_release_reader NOLOGIN;\n"
                         f"CREATE DATABASE {database} OWNER oo_release_admin;\n")
                actor_files = {}
                if positive:
                    setup += "CREATE ROLE oo_r_clerk NOLOGIN;\n"
                    for kind, secret in actors.items():
                        setup += f"CREATE ROLE biz_{kind} LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE PASSWORD '{secret}';\n"
                        actor_file = root / (kind + "-password")
                        actor_file.write_text(secret)
                        actor_files[kind] = str(actor_file)
                    setup += ("GRANT oo_r_clerk TO biz_executor,biz_evaluator,oo_release_admin;\n"
                              "GRANT biz_oracle TO oo_release_admin;\n")
                assert command([binaries["psql"], "-X", "-v", "ON_ERROR_STOP=1", "-h", str(root),
                                "-p", str(port), "-U", "postgres", "-d", "postgres"], data=setup) == 0
                if positive and not native:
                    assert command([binaries["psql"], "-X", "-v", "ON_ERROR_STOP=1", "-h", str(root),
                                    "-p", str(port), "-U", "postgres", "-d", database], data=(
                        "CREATE TABLE public.customer(id bigint NOT NULL,contact text);\n"
                        "INSERT INTO public.customer VALUES(1,'fixture'),(2,'fixture'),(3,'fixture');\n"
                        "CREATE TABLE public.independent_counts(n bigint NOT NULL);\n"
                        "INSERT INTO public.independent_counts VALUES(1),(2);\n"
                        "GRANT SELECT(id) ON public.customer TO oo_r_clerk;\n"
                        "GRANT SELECT,UPDATE ON public.customer,public.independent_counts TO oo_release_admin;\n"
                        "GRANT SELECT ON public.independent_counts TO biz_oracle;\n"
                        "CREATE TABLE public.capture_generation(dataset text PRIMARY KEY,generation bigint NOT NULL);\n"
                        "INSERT INTO public.capture_generation VALUES('public.customer',1),('public.independent_counts',1);\n"
                        "GRANT SELECT,UPDATE ON public.capture_generation TO oo_release_admin;\n"
                        "CREATE FUNCTION public.bump_fixture_generation() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER "
                        "SET search_path=pg_catalog AS $$ BEGIN UPDATE public.capture_generation SET generation=generation+1 "
                        "WHERE dataset=TG_TABLE_SCHEMA||'.'||TG_TABLE_NAME; RETURN NULL; END $$;\n"
                        "CREATE TRIGGER capture_customer AFTER INSERT OR UPDATE OR DELETE OR TRUNCATE ON public.customer "
                        "FOR EACH STATEMENT EXECUTE FUNCTION public.bump_fixture_generation();\n"
                        "CREATE TRIGGER capture_oracle AFTER INSERT OR UPDATE OR DELETE OR TRUNCATE ON public.independent_counts "
                        "FOR EACH STATEMENT EXECUTE FUNCTION public.bump_fixture_generation();\n"
                    )) == 0
                if native:
                    import native_business_fixture
                    sql = native_business_fixture.setup_sql(database)
                    (evidence / "native-setup.sql").write_text(sql)
                    assert command([binaries["psql"], "-X", "-v", "ON_ERROR_STOP=1", "-h", str(root),
                                    "-p", str(port), "-U", "postgres", "-d", database], data=sql) == 0
                config = root / "test.json"
                config.write_text(json.dumps({
                    "host": "127.0.0.1", "port": port, "database": database,
                    "password_file": str(password_file), "ca_file": str(root / "ca.crt"),
                    "actor_password_files": actor_files, "bootstrap_password_file": str(bootstrap_file),
                    "native": native,
                }))
                env = dict(os.environ, CARGO_TARGET_DIR=str(evidence.parent / "cargo-target"),
                           OO_BUSINESS_GUARD_CONFIG=str(config), OO_RELEASE_GATE_EVIDENCE_DIR=str(evidence),
                           OO_BUSINESS_CLI_BIN=str(evidence.parent / "cargo-target/debug/release-coordinator"))
                if positive:
                    assert command([binaries["cargo"], "build", "--locked", "--offline", "-j", "2",
                                    "--bin", "release-coordinator"], env=env) == 0
                test = ("business_native_pg::native_publication_aggregate_chain" if native else
                        "business_positive_pg::actual_test_authority_executes_reviews_and_rechecks_cas" if positive else
                        "business_guard_pg::authenticated_legacy_test_claims_cannot_bypass_business_preconditions")
                code = command([binaries["cargo"], "test", "--locked", "--offline", "-j", "2", "--lib", test,
                                "--", "--ignored", "--exact", "--nocapture"], env=env)
            finally:
                if started:
                    stop = command([binaries["pg_ctl"], "-D", str(data), "-m", "fast", "-w", "stop"])
                    if stop:
                        raise RuntimeError("owned_postgres_stop_failed")
                (evidence / "raw-exits.json").write_text(json.dumps({
                    "commands": exits, "test_exit_code": code,
                    "dedicated_loopback_only": True, "existing_server_used": False,
                    "customer_source_access": False, "production_gate_passed": False,
                }, indent=2) + "\n")
            return code


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence", required=True, type=Path)
    parser.add_argument("--positive", action="store_true")
    parser.add_argument("--native", action="store_true")
    args = parser.parse_args()
    raise SystemExit(run(args.evidence.resolve(), args.positive, args.native))
