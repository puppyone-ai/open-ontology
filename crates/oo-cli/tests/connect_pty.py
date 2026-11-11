#!/usr/bin/env python3
"""Drive `oo init` screen 1 through a real PTY for each P0 engine.

PostgreSQL: a throwaway postgres:17.11 container on loopback with a
self-signed TLS certificate and a non-superuser read-only role; real
sign-in and version detection. MySQL: a stub that speaks the protocol-10
greeting (version 8.4.7) and upgrades to TLS with the same certificate.
Oracle: a stub TCP listener (sign-in is deferred to `oo doctor`). Each run
ends on screen 2 with a stubbed table catalog, then quits.

Counts the keys pressed after the splash and checks that no password
reaches the terminal, the secret store holds the password and CA, and the
recent-connections file holds no secret. Run from the repository root:

    python3 -I crates/oo-cli/tests/connect_pty.py --binary target/debug/oo [--out FILE]
"""
import argparse
import fcntl
import json
import os
from pathlib import Path
import pty
import re
import secrets
import select
import socket
import ssl
import struct
import subprocess
import sys
import tempfile
import termios
import threading
import time

ESC = "\x1b"
DOWN, UP, ENTER = ESC + "[B", ESC + "[A", "\r"
ANSI = re.compile(rb"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07]*\x07|\x1b[()][0-9A-B]")


def certificate(directory: Path):
    """A CA and a `localhost` server certificate it issued; `cert.pem` holds
    the chain (leaf, CA) as servers usually send it."""
    def openssl(*args):
        subprocess.run(["openssl", *args], check=True, capture_output=True)

    (directory / "ca.cnf").write_text(
        "[req]\ndistinguished_name=dn\nx509_extensions=v3\nprompt=no\n"
        "[dn]\nCN=oo pty CA\n[v3]\nbasicConstraints=critical,CA:TRUE\n"
        "keyUsage=critical,keyCertSign,cRLSign\n")
    (directory / "leaf.cnf").write_text(
        "[req]\ndistinguished_name=dn\nprompt=no\n[dn]\nCN=localhost\n"
        "[v3]\nbasicConstraints=critical,CA:FALSE\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n"
        "keyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n")
    d = str(directory)
    openssl("req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2", "-keyout", f"{d}/ca.key",
            "-out", f"{d}/ca.pem", "-config", f"{d}/ca.cnf")
    openssl("req", "-newkey", "rsa:2048", "-nodes", "-keyout", f"{d}/key.pem", "-out", f"{d}/leaf.csr",
            "-config", f"{d}/leaf.cnf")
    openssl("x509", "-req", "-in", f"{d}/leaf.csr", "-CA", f"{d}/ca.pem", "-CAkey", f"{d}/ca.key",
            "-CAcreateserial", "-out", f"{d}/leaf.pem", "-days", "2", "-extfile", f"{d}/leaf.cnf",
            "-extensions", "v3")
    (directory / "cert.pem").write_text((directory / "leaf.pem").read_text() + (directory / "ca.pem").read_text())
    os.chmod(directory / "key.pem", 0o644)  # read by the container's copy step


class Postgres:
    def __init__(self, certs: Path):
        self.password = secrets.token_hex(12)
        admin = secrets.token_hex(12)
        script = (
            "cp /certs/cert.pem /certs/key.pem /tmp/ && chown postgres /tmp/cert.pem /tmp/key.pem "
            "&& chmod 600 /tmp/key.pem && exec docker-entrypoint.sh postgres -c ssl=on "
            "-c ssl_cert_file=/tmp/cert.pem -c ssl_key_file=/tmp/key.pem"
        )
        self.id = subprocess.run(
            ["docker", "run", "-d", "--rm", "--label", "oo.lane=a-tui-pty",
             "-e", "POSTGRES_USER=admin", "-e", f"POSTGRES_PASSWORD={admin}",
             "-e", "POSTGRES_DB=logistics", "-p", "127.0.0.1::5432",
             "-v", f"{certs}:/certs:ro", "--entrypoint", "sh",
             "postgres:17.11-bookworm", "-c", script],
            check=True, capture_output=True, text=True,
        ).stdout.strip()
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            ok = subprocess.run(
                ["docker", "exec", self.id, "pg_isready", "-U", "admin", "-d", "logistics", "-h", "127.0.0.1"],
                capture_output=True,
            ).returncode == 0
            if ok:
                break
            time.sleep(1)
        sql = (
            f"CREATE ROLE oo_reader LOGIN PASSWORD '{self.password}' NOSUPERUSER NOCREATEDB NOCREATEROLE;"
            "GRANT pg_read_all_data TO oo_reader;"
        )
        subprocess.run(
            ["docker", "exec", self.id, "psql", "-U", "admin", "-d", "logistics", "-v", "ON_ERROR_STOP=1", "-c", sql],
            check=True, capture_output=True,
        )
        out = subprocess.run(["docker", "port", self.id, "5432/tcp"], check=True,
                             capture_output=True, text=True).stdout
        self.port = int(out.strip().splitlines()[0].rsplit(":", 1)[1])

    def close(self):
        subprocess.run(["docker", "rm", "-f", self.id], capture_output=True)


class MysqlStub:
    """Protocol-10 greeting with CLIENT_SSL, then a TLS server handshake."""

    def __init__(self, certs: Path):
        self.context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        self.context.load_cert_chain(certs / "cert.pem", certs / "key.pem")
        self.sock = socket.socket()
        self.sock.bind(("127.0.0.1", 0))
        self.sock.listen(8)
        self.port = self.sock.getsockname()[1]
        threading.Thread(target=self.serve, daemon=True).start()

    def serve(self):
        while True:
            try:
                conn, _ = self.sock.accept()
            except OSError:
                return
            threading.Thread(target=self.one, args=(conn,), daemon=True).start()

    def one(self, conn):
        try:
            conn.settimeout(5)
            caps = 0x0200 | 0x0800 | 0x8000
            body = (b"\x0a" + b"8.4.7\x00" + struct.pack("<I", 7) + b"abcdefgh" + b"\x00"
                    + struct.pack("<H", caps & 0xFFFF) + b"\xff" + struct.pack("<H", 2)
                    + struct.pack("<H", 0x0008) + b"\x15" + b"\x00" * 10 + b"ijklmnopqrst\x00"
                    + b"caching_sha2_password\x00")
            conn.sendall(struct.pack("<I", len(body))[:3] + b"\x00" + body)
            request = b""
            while len(request) < 36:
                chunk = conn.recv(36 - len(request))
                if not chunk:
                    return
                request += chunk
            tls = self.context.wrap_socket(conn, server_side=True)
            tls.recv(1)
        except (OSError, ssl.SSLError):
            pass
        finally:
            conn.close()

    def close(self):
        self.sock.close()


class Listener:
    """Accepts and closes: an Oracle listener stand-in for the port check."""

    def __init__(self):
        self.sock = socket.socket()
        self.sock.bind(("127.0.0.1", 0))
        self.sock.listen(8)
        self.port = self.sock.getsockname()[1]
        threading.Thread(target=self.serve, daemon=True).start()

    def serve(self):
        while True:
            try:
                conn, _ = self.sock.accept()
                conn.close()
            except OSError:
                return

    def close(self):
        self.sock.close()


class Session:
    def __init__(self, binary: Path, home: Path, cwd: Path, args, extra_env=None):
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
        env = {k: v for k, v in os.environ.items()
               if not k.startswith(("PG", "MYSQL_", "ORACLE_", "TNS_", "OO_LANG", "LC_"))}
        env.update(HOME=str(home), TERM="xterm-256color", LANG="en_US.UTF-8")
        env.update(extra_env or {})
        self.process = subprocess.Popen([str(binary), "init", *args], cwd=cwd, env=env,
                                        stdin=slave, stdout=slave, stderr=slave, close_fds=True)
        os.close(slave)
        self.raw = bytearray()
        self.keys = 0

    def drain(self, seconds=0.2):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if select.select([self.master], [], [], 0.03)[0]:
                try:
                    data = os.read(self.master, 65536)
                except OSError:
                    return
                if not data:
                    return
                self.raw.extend(data)

    def text(self):
        return ANSI.sub(b"", bytes(self.raw)).decode("utf-8", "replace")

    def wait(self, needle, seconds=20):
        start = len(self.text())
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            self.drain(0.15)
            if needle in self.text()[max(0, start - 4000):]:
                return
        raise AssertionError(f"timed out waiting for {needle!r}; tail: {self.text()[-1500:]!r}")

    def press(self, *keys, count=True):
        for key in keys:
            os.write(self.master, key.encode())
            if count:
                self.keys += 1 if key.startswith(ESC) or len(key) == 1 else len(key)
            self.drain(0.12)

    def type(self, text):
        for ch in text:
            self.press(ch)

    def close(self):
        if self.process.poll() is None:
            self.process.kill()
        self.process.wait(timeout=10)
        os.close(self.master)


def catalog(path: Path, schema: str):
    path.write_text(json.dumps({"protocol": "oo.table-catalog/v1", "tables": [
        {"schema": schema, "name": n, "rows": r, "has_primary_key": True, "has_unique_key": False,
         "partition_child": False, "references": refs}
        for n, r, refs in [("customers", 4800, []), ("orders", 120000, [f"{schema}.customers"]),
                           ("audit_log", 900000, [])]
    ]}))


def to_screen_two(s: Session):
    s.wait("SSH jump host")
    s.press(ENTER)  # direct connection
    s.wait("recommend")
    s.press("q", count=False)
    s.process.wait(timeout=10)


def run_postgres(binary, root, certs, report):
    pg = Postgres(certs)
    try:
        home = root / "home-pg"
        home.mkdir()
        work = root / "work-pg"
        work.mkdir()
        catalog(root / "pg-catalog.json", "public")
        args = ["--catalog", str(root / "pg-catalog.json")]
        s = Session(binary, home, work, args)
        try:
            s.wait("PostgreSQL")
            s.press(" ", count=False)  # skip the splash (not counted)
            s.wait("engines")
            s.press(ENTER)                       # PostgreSQL
            s.wait("host")
            s.press(DOWN)                        # host: keep localhost
            s.type(str(pg.port))                 # port
            s.press(DOWN)
            s.type("logistics")                  # database
            s.press(ENTER)                       # ⏎ jumps to the password
            s.type(pg.password)
            s.wait("trust this CA")
            s.press(ENTER)                       # ⏎ jumps to TLS
            s.press("t")                         # trust the presented CA (loopback)
            s.press(ENTER)                       # sign in
            s.wait("signed in as oo_reader", 30)
            s.wait("detected 17.11")
            s.press(ENTER)                       # continue
            first = s.keys
            to_screen_two(s)
            assert pg.password not in s.text(), "password echoed"
        finally:
            s.close()
        secrets_dir = home / ".config/open-ontology/secrets"
        stored = sorted(p.name for p in secrets_dir.iterdir())
        assert "source-password" in stored and "source-ca" in stored, stored
        recent = (home / ".config/open-ontology/recent-connections.json").read_text()
        assert pg.password not in recent and "BEGIN CERTIFICATE" not in recent
        # Second run: the recent connection is one key away.
        s = Session(binary, home, work, args)
        try:
            s.wait("PostgreSQL")
            s.press(" ", count=False)
            s.wait("recent")
            s.press(ENTER)                       # the recent connection
            s.wait("saved earlier")
            s.press(ENTER)                       # sign in with saved secrets
            s.wait("signed in as oo_reader", 30)
            s.press(ENTER)
            again = s.keys
            to_screen_two(s)
        finally:
            s.close()
        report["postgres"] = {"server": "postgres:17.11 (TLS, scram, read-only role)",
                              "keys_first_run": first, "keys_recent_rerun": again,
                              "password_chars": len(pg.password), "detected": "17.11 → 17",
                              "stored_secrets": stored}
    finally:
        pg.close()


def run_mysql(binary, root, certs, report):
    stub = MysqlStub(certs)
    try:
        home = root / "home-my"
        home.mkdir()
        work = root / "work-my"
        work.mkdir()
        catalog(root / "my-catalog.json", "shop")
        password = secrets.token_hex(12)
        s = Session(binary, home, work, ["--catalog", str(root / "my-catalog.json")])
        try:
            s.wait("MySQL")
            s.press(" ", count=False)
            s.wait("engines")
            s.press(DOWN, ENTER)                 # MySQL
            s.wait("host")
            s.press(DOWN)
            s.type(str(stub.port))
            s.press(DOWN)
            s.type("shop")
            s.press(ENTER)
            s.type(password)
            s.wait("trust this CA")
            s.press(ENTER, "t", ENTER)
            s.wait("checked by oo doctor next")
            s.wait("8.4.7")
            s.press(ENTER)
            keys = s.keys
            to_screen_two(s)
            assert password not in s.text()
        finally:
            s.close()
        report["mysql"] = {"server": "stub greeting 8.4.7 + TLS upgrade", "keys_first_run": keys,
                           "password_chars": len(password), "detected": "8.4.7 → 8.4"}
    finally:
        stub.close()


def run_oracle(binary, root, report):
    stub = Listener()
    try:
        home = root / "home-ora"
        home.mkdir()
        work = root / "work-ora"
        work.mkdir()
        catalog(root / "ora-catalog.json", "APP")
        password = secrets.token_hex(12)
        env = {"OO_ORACLE_WORKER_IMAGE": "sha256:" + "0" * 64}
        s = Session(binary, home, work, ["--catalog", str(root / "ora-catalog.json")], env)
        try:
            s.wait("Oracle")
            s.press(" ", count=False)
            s.wait("engines")
            s.press(DOWN, DOWN, ENTER)           # Oracle
            s.wait("service")
            s.press(DOWN)
            s.type(str(stub.port))
            s.press(ENTER)                       # service ORCLPDB1 and user kept
            s.type(password)
            s.press(ENTER)                       # sign in (deferred to the worker)
            s.wait("Oracle worker")
            s.press(ENTER)
            keys = s.keys
            to_screen_two(s)
            assert password not in s.text()
        finally:
            s.close()
        report["oracle"] = {"server": "stub listener; sign-in deferred to oo doctor (worker)",
                            "keys_first_run": keys, "password_chars": len(password)}
    finally:
        stub.close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--out")
    args = parser.parse_args()
    binary = Path(args.binary).resolve()
    report = {"protocol": "oo.connect-pty/v1", "terminal": "80x24"}
    with tempfile.TemporaryDirectory(prefix="oo-connect-pty-") as tmp:
        root = Path(tmp)
        certs = root / "certs"
        certs.mkdir()
        certificate(certs)
        run_postgres(binary, root, certs, report)
        run_mysql(binary, root, certs, report)
        run_oracle(binary, root, report)
    text = json.dumps(report, indent=1, ensure_ascii=False)
    print(text)
    if args.out:
        Path(args.out).write_text(text + "\n")


if __name__ == "__main__":
    sys.exit(main())
