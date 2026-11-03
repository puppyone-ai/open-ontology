#!/usr/bin/env python3
"""Disposable lane-C fixtures; never connects to an existing database."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import secrets
import subprocess
import sys
import time


def run(argv, **kw):
    return subprocess.run(argv, check=True, capture_output=True, **kw).stdout


def private(path, text):
    path.write_text(text)
    path.chmod(0o600)


def seed_sql():
    digits = " UNION ALL ".join(f"SELECT {i} AS n" for i in range(10))
    return f"""
CREATE DATABASE fixture CHARACTER SET utf8mb4 COLLATE utf8mb4_general_ci;
USE fixture;
CREATE TABLE digits (n INT NOT NULL PRIMARY KEY);
INSERT INTO digits {digits};
CREATE TABLE scale_rows (id INT NOT NULL PRIMARY KEY, payload VARCHAR(64) NOT NULL);
INSERT INTO scale_rows
SELECT n, CONCAT('row-',n) FROM
(SELECT 1+a.n+10*b.n+100*c.n+1000*d.n+10000*e.n+100000*f.n+1000000*g.n AS n
FROM digits a CROSS JOIN digits b CROSS JOIN digits c CROSS JOIN digits d
CROSS JOIN digits e CROSS JOIN digits f CROSS JOIN digits g WHERE g.n<2) x;
CREATE TABLE int_rows LIKE scale_rows;
INSERT INTO int_rows SELECT * FROM scale_rows WHERE id<=2000;
CREATE TABLE growing LIKE scale_rows;
INSERT INTO growing SELECT * FROM scale_rows WHERE id<=40;
CREATE TABLE text_rows (k VARCHAR(40) COLLATE utf8mb4_general_ci PRIMARY KEY, payload VARCHAR(64));
INSERT INTO text_rows VALUES ('a','lower'),('B','upper'),(_utf8mb4 0xC3A9636C616972,'accent'),('trail ','space'),('Z','last');
INSERT IGNORE INTO text_rows VALUES ('A','case collision'),('ECLAIR','accent collision'),('trail','space collision');
CREATE TABLE composite_rows (tenant VARCHAR(40) COLLATE utf8mb4_general_ci, seq INT, payload VARCHAR(64), PRIMARY KEY(tenant,seq));
INSERT INTO composite_rows SELECT k,n,CONCAT(payload,n) FROM text_rows CROSS JOIN digits;
CREATE TABLE unique_rows (code VARCHAR(40) COLLATE utf8mb4_general_ci NOT NULL, payload VARCHAR(64), UNIQUE KEY code_unique(code));
INSERT INTO unique_rows SELECT * FROM text_rows;
CREATE TABLE no_key (id INT, payload VARCHAR(64));
INSERT INTO no_key VALUES (1,NULL),(1,''),(2,'duplicate'),(2,'duplicate');
CREATE TABLE empty_rows LIKE scale_rows;
CREATE TABLE digest_edges (id INT PRIMARY KEY, a VARBINARY(50), b VARBINARY(50));
INSERT INTO digest_edges VALUES (1,0x611f62,0x63);
ANALYZE TABLE scale_rows,int_rows,growing,text_rows,composite_rows,unique_rows,no_key;
"""


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--versions", nargs="+", default=["5.7.44", "8.0.44", "8.4.7", "9.5.0"])
    p.add_argument("--run-id", default="c-s0s1-" + time.strftime("%Y%m%d-%H%M%S"))
    p.add_argument("--suite", choices=["s0s1", "s2-digest", "s2-watermark", "s2-c5", "s2-retention"], default="s0s1")
    args = p.parse_args()
    root = Path(__file__).resolve().parents[3]
    os.chdir(root)
    evidence = root / "evidence" / args.run_id
    evidence.mkdir(parents=True, exist_ok=False)
    results = {}
    for version in args.versions:
        if version not in ["5.7.44", "8.0.44", "8.4.7", "9.5.0"]:
            raise ValueError("version must be an exact lane-C pin")
        directory = evidence / version
        directory.mkdir(mode=0o700)
        name = "oo-c-" + secrets.token_hex(6)
        volume = name + "-data"
        created = False
        root_pw, reader_pw, writer_pw = [secrets.token_hex(24) for _ in range(3)]
        private(directory / "root-password", root_pw)
        private(directory / "reader-password", reader_pw)
        private(directory / "writer-password", writer_pw)
        private(directory / "admin.cnf", f"[client]\nuser=root\npassword={root_pw}\n")
        # Generated secrets stay in the ignored, private run directory.
        run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes",
             "-keyout", str(directory / "ca-key.pem"), "-out", str(directory / "ca.pem"),
             "-days", "2", "-subj", "/CN=lane-c-fixture-ca"])
        run(["openssl", "req", "-newkey", "rsa:2048", "-nodes",
             "-keyout", str(directory / "server-key.pem"), "-out", str(directory / "server.csr"),
             "-subj", "/CN=localhost"])
        (directory / "extensions").write_text(
            "subjectAltName=DNS:localhost,IP:127.0.0.1\n"
            "basicConstraints=critical,CA:FALSE\n"
            "keyUsage=critical,digitalSignature,keyEncipherment\n"
            "extendedKeyUsage=serverAuth\n")
        run(["openssl", "x509", "-req", "-in", str(directory / "server.csr"),
             "-CA", str(directory / "ca.pem"), "-CAkey", str(directory / "ca-key.pem"),
             "-CAcreateserial", "-out", str(directory / "server-cert.pem"), "-days", "2",
             "-extfile", str(directory / "extensions")])
        try:
            run(["docker", "volume", "create", "--label", "oo.lane=c",
                 "--label", f"oo.run_id={args.run_id}", volume])
            script = (
                "cp /fixture/server-key.pem /fixture/server-cert.pem /fixture/ca.pem /tmp/; "
                "chown mysql:mysql /tmp/server-key.pem /tmp/server-cert.pem /tmp/ca.pem; "
                "exec docker-entrypoint.sh mysqld --server-id=731 --log-bin=binlog "
                "--binlog-format=ROW --binlog-row-image=FULL --binlog-checksum=CRC32 --gtid-mode=ON "
                "--enforce-gtid-consistency=ON --ssl-ca=/tmp/ca.pem "
                "--ssl-cert=/tmp/server-cert.pem --ssl-key=/tmp/server-key.pem"
            )
            if args.suite == "s2-watermark":
                script += (" --log-slave-updates=ON" if version.startswith("5.7")
                           else " --log-replica-updates=ON")
            cmd = ["docker", "run", "-d", "--name", name, "--label", "oo.lane=c",
                   "--label", f"oo.run_id={args.run_id}"]
            if version.startswith("5.7"):
                cmd += ["--platform", "linux/amd64"]
            cmd += ["-p", "127.0.0.1::3306", "--mount",
                    f"type=volume,source={volume},target=/var/lib/mysql",
                    "--mount", f"type=bind,source={directory},target=/fixture,readonly",
                    "-e", "MYSQL_ROOT_PASSWORD_FILE=/fixture/root-password",
                    "--entrypoint", "/bin/sh", f"mysql:{version}", "-c", script]
            run(cmd)
            created = True
            for _ in range(180):
                ready = subprocess.run(["docker", "exec", name, "mysql",
                    "--defaults-extra-file=/fixture/admin.cnf", "--protocol=TCP",
                    "-h127.0.0.1", "-Nse", "SELECT 1"],
                    capture_output=True)
                if ready.returncode == 0:
                    break
                time.sleep(1)
            else:
                raise RuntimeError("fixture did not become ready")
            if args.suite == "s0s1":
                seed = seed_sql()
            else:
                seed = "CREATE DATABASE fixture CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci; USE fixture;\n" + run([
                    sys.executable, "testing/engines/seed-common/generate.py",
                    "--engine", "mysql", "--version", version, "--scale", "2000",
                    "--tables", "3",
                ]).decode()
            setup = seed + f"""
CREATE USER 'reader'@'%' IDENTIFIED BY '{reader_pw}';
GRANT SELECT ON fixture.* TO 'reader'@'%';
GRANT REPLICATION SLAVE, REPLICATION CLIENT ON *.* TO 'reader'@'%';
CREATE USER 'writer'@'%' IDENTIFIED BY '{writer_pw}';
GRANT SELECT, INSERT, UPDATE, DELETE ON fixture.* TO 'writer'@'%';
GRANT PROCESS ON *.* TO 'writer'@'%';
"""
            run(["docker", "exec", "-i", name, "mysql",
                 "--defaults-extra-file=/fixture/admin.cnf"], input=setup.encode())
            port = run(["docker", "port", name, "3306/tcp"]).decode().strip().rsplit(":", 1)[1]
            config = dict(port=int(port), ca=str(directory / "ca.pem"),
                          reader_password=str(directory / "reader-password"),
                          writer_password=str(directory / "writer-password"),
                          version=version, container=name, out=str(directory))
            private(directory / "config.json", json.dumps(config))
            sources = list((root / "crates/oo-connectors/src/mysql").glob("*.rs"))
            sources += list((root / "crates/oo-connectors/tests").glob("mysql_*.rs"))
            sources += [Path(__file__).resolve()]
            if args.suite == "s2-retention":
                sources += list((root / "tools/mysql-preview/src").glob("*.rs"))
                sources += [
                    root / "tools/mysql-preview/tests/retention_owned.rs",
                    root / "tools/mysql-preview/Cargo.toml",
                    root / "tools/mysql-preview/Cargo.lock",
                ]
            (directory / "source-sha256.json").write_text(json.dumps({
                str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest()
                for path in sources}, indent=2))
            env = os.environ.copy()
            env["OO_MYSQL_ENGINE_FIXTURE"] = str(directory / "config.json")
            if args.suite == "s2-retention":
                test_command = [
                    "cargo", "test", "--manifest-path", "tools/mysql-preview/Cargo.toml",
                    "--locked", "--test", "retention_owned",
                    "--", "--ignored", "--nocapture",
                ]
            else:
                test_command = [
                    "cargo", "test", "-p", "oo-connectors", "--locked", "--features", "mysql-binlog",
                    "--test", "mysql_engine_live",
                    {"s0s1": "engine_matrix_contract", "s2-digest": "digest_primitives",
                     "s2-watermark": "watermark_primitives", "s2-c5": "c5_cycles"}[args.suite],
                    "--", "--ignored", "--nocapture",
                ]
            with (directory / "test.log").open("wb") as log:
                result = subprocess.run(
                    test_command,
                    env=env, stdout=log, stderr=subprocess.STDOUT)
            results[version] = {"exit": result.returncode}
            if args.suite == "s2-digest" and result.returncode == 0:
                sys.path.insert(0, str(root / "tools/oo-test"))
                from comparison import compare
                rows = json.loads((directory / "digest-oracle.json").read_text())
                report = compare(rows["source"], rows["adapter"], key_columns=[0],
                                 chunk_size=97, consistency="B")
                (directory / "comparison-report.json").write_text(json.dumps(report, indent=2))
                if not report["equal"]:
                    results[version]["exit"] = 1
            print(version, "PASS" if results[version]["exit"] == 0 else "FAIL", flush=True)
            (directory / "image.json").write_bytes(run(
                ["docker", "image", "inspect", f"mysql:{version}", "--format",
                 '{"id":"{{.Id}}","digests":{{json .RepoDigests}},"arch":"{{.Architecture}}"}']))
        except Exception as exc:
            # Never print subprocess arguments or SQL containing setup passwords.
            results[version] = {"error": type(exc).__name__}
            if isinstance(exc, subprocess.CalledProcessError):
                text = ((exc.stdout or b"") + (exc.stderr or b"")).decode(errors="replace")
                for secret in [root_pw, reader_pw, writer_pw]:
                    text = text.replace(secret, "[redacted]")
                (directory / "setup-error.log").write_text(text)
            print(version, type(exc).__name__, flush=True)
        finally:
            if created:
                logs = subprocess.run(["docker", "logs", name], capture_output=True)
                text = (logs.stdout + logs.stderr).decode(errors="replace")
                for secret in [root_pw, reader_pw, writer_pw]:
                    text = text.replace(secret, "[redacted]")
                (directory / "container.log").write_text(text)
                run(["docker", "rm", "-f", name])
            run(["docker", "volume", "rm", volume])
        (evidence / "results.json").write_text(json.dumps(results, indent=2))
    checksums = []
    for path in evidence.rglob("*"):
        if path.is_file() and path.suffix in [".log", ".json"]:
            checksums.append(f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.relative_to(evidence)}")
    (evidence / "SHA256SUMS").write_text("\n".join(checksums) + "\n")
    return int(any(result.get("exit") != 0 for result in results.values()))


if __name__ == "__main__":
    raise SystemExit(main())
