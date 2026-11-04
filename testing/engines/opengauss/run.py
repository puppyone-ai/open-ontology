#!/usr/bin/env python3
"""Owned openGauss fixture; passwords stay in ignored 0600 files/stdin."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import secrets
import subprocess
import time

ROOT = Path(__file__).resolve().parents[3]


def run(args, *, data=None, out=None, check=True, env=None):
    p = subprocess.run(args, input=data, stdout=subprocess.PIPE,
                       stderr=subprocess.STDOUT, env=env, cwd=ROOT)
    if out:
        out.write_bytes(p.stdout)
    if check and p.returncode:
        raise RuntimeError(f"command failed ({p.returncode}); see {out.name if out else args[0]}")
    return p


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--version", choices=["5.0.3", "6.0.0"], required=True)
    parser.add_argument("--uniform-vm", action="store_true",
                        help="EE fixture only: emulate one NUMA node on Docker's uniform-memory ARM VM")
    args = parser.parse_args()
    if not args.run_id or any(c not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_" for c in args.run_id):
        raise ValueError("invalid run-id")
    pins = json.loads((ROOT / "testing/engines/opengauss/compatible-images.json").read_text())
    pin = pins[args.version]
    d = ROOT / "evidence" / args.run_id / args.version
    d.mkdir(parents=True, exist_ok=False)
    private = d / "private"
    private.mkdir(mode=0o700)
    for name in ["admin", "reader", "shaonly", "initial"]:
        p = private / name
        p.write_text("Aa9!" + secrets.token_hex(10))
        p.chmod(0o600)
    envfile = private / "container.env"
    envfile.write_text(f"GS_PASSWORD={(private/'initial').read_text()}\nGS_DB=postgres\n")
    envfile.chmod(0o600)
    run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", str(private/"ca.key"),
         "-out", str(private/"ca.crt"), "-days", "2", "-subj", "/CN=OO Fixture CA"], out=d/"tls-ca.log")
    run(["openssl", "req", "-newkey", "rsa:2048", "-nodes", "-keyout", str(private/"server.key"),
         "-out", str(private/"server.csr"), "-subj", "/CN=localhost"], out=d/"tls-request.log")
    (private/"extensions").write_text("basicConstraints=critical,CA:FALSE\nsubjectAltName=DNS:localhost,IP:127.0.0.1\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n")
    run(["openssl", "x509", "-req", "-in", str(private/"server.csr"), "-CA", str(private/"ca.crt"),
         "-CAkey", str(private/"ca.key"), "-CAcreateserial", "-out", str(private/"server.crt"),
         "-days", "2", "-extfile", str(private/"extensions")], out=d/"tls-sign.log")
    for p in private.iterdir():
        p.chmod(0o600)
    name = f"oo-d-og-{secrets.token_hex(5)}"
    extra = []
    entry = []
    if args.uniform_vm:
        compiler = "rust@sha256:652612f07bfbbdfa3af34761c1e435094c00dde4a98036132fca28c7bb2b165c"
        run(["docker", "run", "--rm", "--platform", "linux/arm64",
             "--security-opt", "seccomp=unconfined", "--label", "oo.lane=d",
             "-v", f"{ROOT / 'testing/engines/opengauss'}:/src:ro", "-v", f"{private}:/out",
             compiler, "bash", "-ec",
             "gcc -shared -fPIC -O2 -o /out/uniform-vm.so /src/uniform-vm.c -ldl; "
             "gcc -O2 -o /out/uniform-vm-test /src/uniform-vm-test.c -ldl; "
             "LD_PRELOAD=/out/uniform-vm.so /out/uniform-vm-test"], out=d/"uniform-vm-build.log")
        extra = ["-v", f"{private/'uniform-vm.so'}:/tmp/oo-uniform-vm.so:ro",
                 "-v", f"{ROOT/'testing/engines/opengauss/uniform-vm.sh'}:/tmp/oo-start:ro",
                 "--entrypoint", "bash"]
        entry = ["/tmp/oo-start"]
    created = False
    result = {"version": args.version, "image": pin, "uniform_vm": args.uniform_vm, "status": "failed"}
    try:
        run(["docker", "run", "-d", "--privileged", "--platform", pin["platform"],
             "--label", "oo.lane=d", "--label", f"oo.run_id={args.run_id}",
             "--name", name, "--env-file", str(envfile), "-p", "127.0.0.1::5432",
             *extra, pin["image"], *entry], out=d/"container.id")
        created = True
        gsql = ["docker", "exec", "-i", "-u", "omm", name, "bash", "-c",
                "export LD_LIBRARY_PATH=/usr/local/opengauss/lib; /usr/local/opengauss/bin/gsql -d postgres -v ON_ERROR_STOP=1"]
        for _ in range(90):
            if run(gsql, data=b"SELECT 1;\n", check=False).returncode == 0:
                break
            time.sleep(1)
        else:
            raise RuntimeError("startup failed")
        for f in ["server.key", "server.crt"]:
            run(["docker", "cp", str(private/f), f"{name}:/tmp/{f}"])
        run(["docker", "exec", name, "bash", "-c",
             "chown omm:omm /tmp/server.*; chmod 600 /tmp/server.*"])
        config = ("ssl=on\nssl_cert_file='/tmp/server.crt'\nssl_key_file='/tmp/server.key'\n"
                  "password_encryption_type=1\nlog_statement='none'\n")
        run(["docker", "exec", "-i", name, "bash", "-c",
             'cat >> "$PGDATA/postgresql.conf"'], data=config.encode())
        hba = "local all all trust\nhostssl all all 0.0.0.0/0 md5\n"
        run(["docker", "exec", "-i", name, "bash", "-c",
             'cat > "$PGDATA/pg_hba.conf"; chown omm:omm "$PGDATA/pg_hba.conf"'], data=hba.encode())
        run(["docker", "restart", name], out=d/"restart.log")
        for _ in range(90):
            if run(gsql, data=b"SELECT 1;\n", check=False).returncode == 0:
                break
            time.sleep(1)
        else:
            raise RuntimeError("TLS restart failed")
        setup = ""
        for user, secret in [("oo_admin", "admin"), ("oo_reader", "reader")]:
            verifier = "md5" + hashlib.md5(((private/secret).read_text()+user).encode()).hexdigest()
            setup += f"CREATE USER {user} {'SYSADMIN' if secret=='admin' else ''} PASSWORD '{verifier}';\n"
        setup += (ROOT/"testing/engines/opengauss/fixture.sql").read_text()
        setup += "CREATE TABLE fixture.wide (id bigint PRIMARY KEY," + ",".join(f"c{i} bigint" for i in range(299)) + ");\n"
        setup += "INSERT INTO fixture.wide VALUES (" + ",".join(str(i) for i in range(300)) + ");\n"
        setup += "GRANT SELECT ON fixture.wide TO oo_reader;\n"
        run(gsql, data=setup.encode(), out=d/"fixture.log")
        # A real SHA256-only account, created by the separate admin connection.
        run(gsql, data=b"ALTER SYSTEM SET password_encryption_type=2; SELECT pg_reload_conf();\n", out=d/"sha-config.log")
        time.sleep(1)
        run(gsql, data=f"CREATE USER shaonly PASSWORD '{(private/'shaonly').read_text()}';\n".encode(), out=d/"sha-user.log")
        run(gsql, data=b"ALTER SYSTEM SET password_encryption_type=1; SELECT pg_reload_conf();\n", out=d/"compatible-config.log")
        port = run(["docker", "port", name, "5432/tcp"]).stdout.decode().strip().rsplit(":", 1)[1]
        env = dict(os.environ, OO_OG_TEST_DIR=str(d), OO_OG_TEST_PORT=port, OO_OG_TEST_VERSION=args.version)
        test = run(["cargo", "test", "--config", "profile.test.package.oo-connectors.opt-level=2",
                    "--config", "profile.test.package.sha2.opt-level=3",
                    "-p", "oo-connectors", "--locked", "--test", "opengauss_engine",
                    "--", "--ignored", "--nocapture", "--test-threads=1"], env=env, out=d/"test.log", check=False)
        result.update(status="passed" if test.returncode == 0 else "failed", test_exit=test.returncode)
        if test.returncode:
            raise RuntimeError("engine tests failed")
    finally:
        if created:
            run(["docker", "logs", name], out=d/"startup.log", check=False)
            run(["docker", "rm", "-f", "-v", name], out=d/"cleanup.log")
        (d/"result.json").write_text(json.dumps(result, indent=2))
        sources = [*sorted((ROOT/"testing/engines/opengauss").glob("*.py")),
                   *sorted((ROOT/"testing/engines/opengauss").glob("*.sql")),
                   *sorted((ROOT/"testing/engines/opengauss").glob("*.c")),
                   *sorted((ROOT/"testing/engines/opengauss").glob("*.sh")),
                   ROOT/"crates/oo-connectors/tests/opengauss_engine.rs",
                   *sorted((ROOT/"crates/oo-connectors/src/postgres").glob("*.rs"))]
        (d/"source-sha256.json").write_text(json.dumps({
            str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in sources if p.exists()
        }, indent=2))
    print(f"{args.version}: {result['status']}")


if __name__ == "__main__":
    main()
