#!/usr/bin/env python3
"""Owned, serial Oracle fixtures. Never connects to an existing database."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import secrets
import shutil
import subprocess
import time

ROOT = Path(__file__).resolve().parents[3]
IMAGES = {
    "23": ("gvenzl/oracle-free:23-slim", "linux/arm64", "FREEPDB1"),
    "21": ("gvenzl/oracle-xe:21-slim", "linux/amd64", "XEPDB1"),
    "11": ("gvenzl/oracle-xe:11", "linux/amd64", "XE"),
}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", choices=IMAGES, required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--worker-image", help="local thick-worker image; required for 11g")
    args = parser.parse_args()
    if not all(c.isalnum() or c in "-_" for c in args.run_id):
        parser.error("invalid run-id")
    out = ROOT / "evidence" / args.run_id / args.version
    out.mkdir(parents=True, exist_ok=False)
    passwords = [secrets.token_hex(12) for _ in range(3)]

    def private(path, text):
        path.touch(mode=0o600, exist_ok=False)
        path.write_text(text)

    def run(argv, name, input=None, timeout=120):
        p = subprocess.run(argv, input=input, text=True, capture_output=True,
                           timeout=timeout, cwd=ROOT)
        raw = p.stdout + p.stderr
        for password in passwords:
            raw = raw.replace(password, "[redacted]")
        (out / (name + ".log")).write_text(raw)
        (out / (name + ".exit")).write_text(str(p.returncode) + "\n")
        if p.returncode:
            raise RuntimeError(name + " failed (see evidence)")
        return p.stdout

    tag, platform, service = IMAGES[args.version]
    image = json.loads(run(["docker", "image", "inspect", tag], "image"))[0]
    pinned = image["RepoDigests"][0]
    (out / "image.json").write_text(json.dumps({
        "tag": tag, "digest": pinned, "platform": platform
    }, indent=2) + "\n")
    # Full docker inspect contains no credentials before the container exists.
    env_file = out / "container.env"
    private(env_file, "ORACLE_PASSWORD=" + passwords[0] + "\n")
    name = "oo-e-" + args.run_id + "-" + args.version
    created = False
    worker_id = None
    data_dirs = []
    try:
        mounts = []
        for i, target in enumerate((image["Config"].get("Volumes") or {}).keys()):
            data = out / f"data-{i}"
            data.mkdir(mode=0o777)
            data.chmod(0o777)
            data_dirs.append(data)
            mounts += ["--mount", f"type=bind,src={data},dst={target}"]
        container_id = run(["docker", "create", "--name", name, "--label", "oo.lane=e",
             "--label", "oo.run_id=" + args.run_id, "--platform", platform,
             "--env-file", str(env_file), "-p", "127.0.0.1::1521",
             *mounts, pinned], "create").strip()
        name = container_id
        created = True
        run(["docker", "start", name], "start")
        deadline = time.monotonic() + 900
        while True:
            logs = run(["docker", "logs", name], "startup")
            if "DATABASE IS READY TO USE!" in logs:
                break
            running = run(["docker", "inspect", "--format", "{{.State.Running}}", name], "running")
            if running.strip() != "true":
                raise RuntimeError("database exited during startup")
            if time.monotonic() > deadline:
                raise RuntimeError("database startup timed out")
            time.sleep(5)
        port = int(run(["docker", "port", name, "1521/tcp"], "port").strip().split(":")[-1])
        config = {
            "port": port, "service": service, "version": int(args.version),
            "reader_password": passwords[1], "writer_password": passwords[2],
            "out": str(out),
        }
        if args.worker_image:
            worker_image = json.loads(run(["docker", "image", "inspect", args.worker_image], "worker-image"))[0]
            worker_digest = worker_image["Id"]
            (out / "worker-image.json").write_text(json.dumps({
                "id": worker_digest, "platform": "linux/amd64",
                "instant_client": "19.30.0.0.0", "license": "Oracle Free Use Terms and Conditions",
            }, indent=2) + "\n")
            admin_script = Path(__file__).resolve().with_name("fixture_admin.py")
            worker_id = run(["docker", "create", "--platform", "linux/amd64",
                "--label", "oo.lane=e", "--label", "oo.run_id=" + args.run_id,
                "--mount", f"type=bind,src={admin_script},dst=/fixture_admin.py,readonly",
                "--entrypoint", "sleep", worker_digest, "infinity"], "worker-create").strip()
            run(["docker", "start", worker_id], "worker-start")
            run(["docker", "exec", worker_id, "python", "-c",
                 "import oracledb,json,platform; oracledb.init_oracle_client(); "
                 "print(json.dumps({'instant_client':oracledb.clientversion(),"
                 "'driver':oracledb.__version__,'machine':platform.machine()}))"],
                "worker-client-version")
            config["worker_container"] = worker_id
        private(out / "config.json", json.dumps(config))
        switch = "" if args.version == "11" else f"ALTER SESSION SET CONTAINER={service};"
        ddl = f"""
SET ECHO OFF VERIFY OFF FEEDBACK OFF
WHENEVER SQLERROR EXIT SQL.SQLCODE
{switch}
CREATE USER OO_FIXTURE IDENTIFIED BY "{passwords[2]}" DEFAULT TABLESPACE USERS QUOTA UNLIMITED ON USERS;
CREATE USER OO_READER IDENTIFIED BY "{passwords[1]}";
GRANT CREATE SESSION, CREATE TABLE TO OO_FIXTURE;
GRANT EXECUTE ON SYS.DBMS_LOCK TO OO_FIXTURE;
GRANT CREATE SESSION TO OO_READER;
GRANT SELECT ON SYS.V_$DATABASE TO OO_READER;
GRANT SELECT ON SYS.V_$SESSION TO OO_READER;
GRANT SELECT ON SYS.V_$SQL_PLAN TO OO_READER;
GRANT SELECT ON SYS.DBA_EXTENTS TO OO_READER;
GRANT EXECUTE ON SYS.DBMS_ROWID TO OO_READER;
CREATE TABLE OO_FIXTURE.NUMERIC_PK (ID NUMBER(12) PRIMARY KEY, PAYLOAD VARCHAR2(80), EMPTY_VALUE VARCHAR2(20));
INSERT INTO OO_FIXTURE.NUMERIC_PK SELECT n, 'value-'||TO_CHAR(n), '' FROM (SELECT (a.n-1)*2000+b.n n FROM (SELECT LEVEL n FROM DUAL CONNECT BY LEVEL<=1000) a CROSS JOIN (SELECT LEVEL n FROM DUAL CONNECT BY LEVEL<=2000) b);
CREATE TABLE OO_FIXTURE.TEXT_PK (ID VARCHAR2(80) PRIMARY KEY, PAYLOAD VARCHAR2(80));
INSERT INTO OO_FIXTURE.TEXT_PK VALUES ('A','upper');
INSERT INTO OO_FIXTURE.TEXT_PK VALUES ('a','lower');
INSERT INTO OO_FIXTURE.TEXT_PK VALUES (UNISTR('\\00E1'),'accent');
INSERT INTO OO_FIXTURE.TEXT_PK VALUES ('a ','trailing');
INSERT INTO OO_FIXTURE.TEXT_PK VALUES ('a  ','two spaces');
CREATE TABLE OO_FIXTURE.COMPOSITE_PK (ID NUMBER(12), CODE VARCHAR2(40), PAYLOAD VARCHAR2(80), PRIMARY KEY (ID,CODE));
INSERT INTO OO_FIXTURE.COMPOSITE_PK SELECT MOD(LEVEL,100), TO_CHAR(LEVEL), 'value' FROM DUAL CONNECT BY LEVEL <= 3000;
CREATE TABLE OO_FIXTURE.UNIQUE_ONLY (ID VARCHAR2(40) NOT NULL, PAYLOAD VARCHAR2(80));
CREATE UNIQUE INDEX OO_FIXTURE.UX_ONLY ON OO_FIXTURE.UNIQUE_ONLY(ID);
INSERT INTO OO_FIXTURE.UNIQUE_ONLY SELECT TO_CHAR(LEVEL), 'value' FROM DUAL CONNECT BY LEVEL <= 3000;
CREATE TABLE OO_FIXTURE.HEAP (ID NUMBER(12), PAYLOAD VARCHAR2(80));
INSERT INTO OO_FIXTURE.HEAP SELECT LEVEL, 'value' FROM DUAL CONNECT BY LEVEL <= 3000;
CREATE TABLE OO_FIXTURE.GROWING (ID NUMBER(12) PRIMARY KEY, PAYLOAD VARCHAR2(80));
INSERT INTO OO_FIXTURE.GROWING SELECT LEVEL, 'value' FROM DUAL CONNECT BY LEVEL <= 10000;
CREATE TABLE OO_FIXTURE.DIGEST_EDGE (A VARCHAR2(80), B VARCHAR2(80));
INSERT INTO OO_FIXTURE.DIGEST_EDGE VALUES ('x'||CHR(31)||'vy','z');
COMMIT;
"""
        for table in ["NUMERIC_PK", "TEXT_PK", "COMPOSITE_PK", "UNIQUE_ONLY", "HEAP", "GROWING", "DIGEST_EDGE"]:
            ddl += f"""
GRANT SELECT, FLASHBACK ON OO_FIXTURE.{table} TO OO_READER;
BEGIN DBMS_STATS.GATHER_TABLE_STATS('OO_FIXTURE','{table}'); END;
/
"""
        ddl += "EXIT\n"
        run(["docker", "exec", "-i", name, "sqlplus", "-s", "/", "as", "sysdba"],
            "fixture", input=ddl, timeout=600)
        if worker_id:
            probe = """
import json,sys,oracledb
from worker import error_status
cfg=json.load(sys.stdin)
oracledb.init_oracle_client()
results=[]
for _ in range(3):
    conn=oracledb.connect(user='OO_READER',password=cfg['password'],dsn=cfg['dsn'])
    conn.call_timeout=1
    try:
        with conn.cursor() as cursor:
            cursor.execute('SELECT COUNT(*) FROM OO_FIXTURE.NUMERIC_PK')
            cursor.fetchone()
        results.append({'completed':True})
    except oracledb.Error as error:
        results.append(error_status(error.args[0]))
    try:
        conn.close()
    except oracledb.Error:
        pass
print(json.dumps(results))
"""
            run(["docker", "exec", "-i", worker_id, "python", "-c", probe],
                "worker-timeout-codes", input=json.dumps({
                    "password": passwords[1], "dsn": f"host.docker.internal:{port}/{service}",
                }))
        env = os.environ.copy()
        env["OO_ORACLE_FIXTURE_CONFIG"] = str(out / "config.json")
        source_files = sorted((ROOT / "crates/oo-connectors/src/oracle").glob("*.rs"))
        source_files.extend(sorted((ROOT / "crates/oo-connectors/tests").glob("oracle_*real.rs")))
        def source_hashes():
            return {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest()
                    for p in source_files}
        sources = source_hashes()
        (out / "source-hashes.json").write_text(json.dumps(sources, indent=2) + "\n")
        p = subprocess.run(
            ["cargo", "test", "-p", "oo-connectors", "--features", "oracle",
             "--locked", "--test", "oracle_engine_real",
             "--", "--ignored", "--nocapture"],
            cwd=ROOT, env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            timeout=1800,
        )
        raw = p.stdout
        for password in passwords:
            raw = raw.replace(password, "[redacted]")
        (out / "test.log").write_text(raw)
        (out / "test.exit").write_text(str(p.returncode) + "\n")
        if sources != source_hashes():
            raise RuntimeError("source changed during test; rerun for fixed-source evidence")
        if p.returncode:
            raise RuntimeError("engine test failed (see evidence)")
    finally:
        if worker_id:
            run(["docker", "rm", "-f", "-v", worker_id], "worker-cleanup")
        if created:
            run(["docker", "rm", "-f", "-v", name], "cleanup")
        for data in data_dirs:
            shutil.rmtree(data)
        for path in [env_file, out / "config.json"]:
            path.unlink(missing_ok=True)
        sums = {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                for p in out.iterdir() if p.is_file()}
        (out / "SHA256SUMS.json").write_text(json.dumps(sums, indent=2) + "\n")


if __name__ == "__main__":
    main()
