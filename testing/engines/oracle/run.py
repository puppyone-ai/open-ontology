#!/usr/bin/env python3
"""Owned, serial Oracle fixtures. Never connects to an existing database."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import secrets
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
    parser.add_argument("--lane", choices=("c", "e"), default="e")
    parser.add_argument("--worker-image", help="local thick-worker image; required for 11g")
    parser.add_argument("--lob-only", action="store_true", help="bounded locator/Arrow regression")
    parser.add_argument("--undo-stress", action="store_true",
                        help="shrink undo in this owned 23ai fixture; test lost-undo behavior")
    parser.add_argument("--incremental", action="store_true",
                        help="C5 cycles and owned LogMiner/audit fixture")
    parser.add_argument("--c5-only", action="store_true",
                        help="run incremental C5 and strict T7 with a non-LogMiner reader")
    args = parser.parse_args()
    if args.incremental and args.undo_stress:
        parser.error("run incremental and undo stress separately")
    if args.lob_only and (args.incremental or args.undo_stress):
        parser.error("run LOB regression separately")
    if args.c5_only and not args.incremental:
        parser.error("--c5-only requires --incremental")
    if args.undo_stress and (args.version != "23" or args.worker_image):
        parser.error("undo stress currently requires the native 23ai fixture")
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
    try:
        mounts = []
        for target in (image["Config"].get("Volumes") or {}):
            # Disposable fixture data must not fill the host's small free disk.
            mounts += ["--tmpfs", f"{target}:rw,size=12g,mode=0777"]
        container_id = run(["docker", "create", "--name", name, "--label", "oo.lane=" + args.lane,
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
        if args.incremental and not args.c5_only:
            config["miner_password"] = passwords[1]
            config["root_service"] = "FREE" if args.version == "23" else "XE"
        if args.worker_image:
            worker_image = json.loads(run(["docker", "image", "inspect", args.worker_image], "worker-image"))[0]
            worker_digest = worker_image["Id"]
            (out / "worker-image.json").write_text(json.dumps({
                "id": worker_digest, "platform": "linux/amd64",
                "instant_client": "19.30.0.0.0", "license": "Oracle Free Use Terms and Conditions",
            }, indent=2) + "\n")
            admin_script = Path(__file__).resolve().with_name("fixture_admin.py")
            worker_id = run(["docker", "create", "--platform", "linux/amd64",
                "--label", "oo.lane=" + args.lane, "--label", "oo.run_id=" + args.run_id,
                "--mount", f"type=bind,src={admin_script},dst=/fixture_admin.py,readonly",
                "--mount", f"type=bind,src={admin_script.parent / 'worker/worker.py'},dst=/worker.py,readonly",
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
CREATE TABLE OO_FIXTURE.C5_HINT (ID NUMBER(12) PRIMARY KEY, PAYLOAD VARCHAR2(80), UPDATED_AT DATE);
INSERT INTO OO_FIXTURE.C5_HINT SELECT LEVEL, 'value', CASE WHEN MOD(LEVEL,2)=0 THEN DATE '2020-01-01' ELSE NULL END FROM DUAL CONNECT BY LEVEL<=100;
CREATE TABLE OO_FIXTURE.DIGEST_EDGE (A VARCHAR2(80), B VARCHAR2(80));
INSERT INTO OO_FIXTURE.DIGEST_EDGE VALUES ('x'||CHR(31)||'vy','z');
COMMIT;
"""
        for table in ["NUMERIC_PK", "TEXT_PK", "COMPOSITE_PK", "UNIQUE_ONLY", "HEAP", "GROWING", "DIGEST_EDGE", "C5_HINT"]:
            ddl += f"""
GRANT SELECT, FLASHBACK ON OO_FIXTURE.{table} TO OO_READER;
BEGIN DBMS_STATS.GATHER_TABLE_STATS('OO_FIXTURE','{table}'); END;
/
"""
        ddl += "EXIT\n"
        if args.incremental or args.lob_only:
            # T2 uses GROWING; do not generate the unrelated two-million-row
            # T1 fixture's redo for every LogMiner test.
            ddl = ddl.replace("CONNECT BY LEVEL<=1000) a CROSS", "CONNECT BY LEVEL<=1) a CROSS")
        run(["docker", "exec", "-i", name, "sqlplus", "-s", "/", "as", "sysdba"],
            "fixture", input=ddl, timeout=600)
        if args.lob_only:
            run(["docker", "exec", "-i", name, "sqlplus", "-s", "/", "as", "sysdba"],
                "lob-fixture", input=f"""
WHENEVER SQLERROR EXIT SQL.SQLCODE
{switch}
CREATE TABLE OO_FIXTURE.LOB_EDGES (ID NUMBER(9) PRIMARY KEY, C CLOB, N NCLOB, B BLOB);
DECLARE
 c CLOB; n NCLOB; b BLOB;
 text VARCHAR2(4000) := RPAD('x',2000,'x');
 national NVARCHAR2(2000);
 raw_value RAW(2000) := HEXTORAW(RPAD('00FF',4000,'00FF'));
BEGIN
 FOR j IN 1..500 LOOP
  national := national || UNISTR('\\4E2D\\D83D\\DE00');
 END LOOP;
 INSERT INTO OO_FIXTURE.LOB_EDGES VALUES (1,EMPTY_CLOB(),EMPTY_CLOB(),EMPTY_BLOB())
 RETURNING C,N,B INTO c,n,b;
 FOR i IN 1..12 LOOP
  DBMS_LOB.WRITEAPPEND(c,LENGTH(text),text);
  DBMS_LOB.WRITEAPPEND(n,LENGTH2(national),national);
  DBMS_LOB.WRITEAPPEND(b,UTL_RAW.LENGTH(raw_value),raw_value);
 END LOOP;
 INSERT INTO OO_FIXTURE.LOB_EDGES VALUES (2,EMPTY_CLOB(),EMPTY_CLOB(),EMPTY_BLOB());
 INSERT INTO OO_FIXTURE.LOB_EDGES VALUES (3,NULL,NULL,NULL);
 COMMIT;
END;
/
GRANT SELECT, FLASHBACK ON OO_FIXTURE.LOB_EDGES TO OO_READER;
EXIT
""")
        if args.incremental and not args.c5_only:
            miner_user = "OO_MINER" if args.version == "11" else "C##OO_MINER"
            mining = f"""
SET ECHO OFF VERIFY OFF FEEDBACK OFF
WHENEVER SQLERROR EXIT SQL.SQLCODE
ALTER SYSTEM SET AUDIT_TRAIL='DB','EXTENDED' SCOPE=SPFILE;
SHUTDOWN IMMEDIATE;
STARTUP MOUNT;
ALTER DATABASE ARCHIVELOG;
ALTER DATABASE OPEN;
ALTER PLUGGABLE DATABASE ALL OPEN;
ALTER DATABASE ADD SUPPLEMENTAL LOG DATA;
CREATE USER C##OO_MINER IDENTIFIED BY "{passwords[1]}" CONTAINER=ALL;
GRANT CREATE SESSION, LOGMINING, SELECT ANY TRANSACTION TO C##OO_MINER CONTAINER=ALL;
GRANT EXECUTE ON SYS.DBMS_LOGMNR TO C##OO_MINER CONTAINER=ALL;
"""
            for view in ["V_$DATABASE", "V_$ARCHIVED_LOG", "V_$LOG", "V_$LOGFILE",
                         "V_$LOGMNR_CONTENTS", "V_$LOGMNR_LOGS", "V_$SESSION", "V_$TRANSACTION"]:
                mining += f"GRANT SELECT ON SYS.{view} TO C##OO_MINER CONTAINER=ALL;\n"
            mining += f"""
AUDIT ALL BY C##OO_MINER BY ACCESS;
AUDIT SELECT TABLE, INSERT TABLE, UPDATE TABLE, DELETE TABLE, EXECUTE PROCEDURE BY C##OO_MINER BY ACCESS;
{switch}
ALTER TABLE OO_FIXTURE.GROWING ADD SUPPLEMENTAL LOG DATA (ALL) COLUMNS;
EXIT
"""
            if args.version != "11":
                mining = mining.replace("AUDIT ALL BY C##OO_MINER BY ACCESS;", "CREATE AUDIT POLICY OO_MINER_AUDIT ACTIONS ALL;\nAUDIT POLICY OO_MINER_AUDIT BY C##OO_MINER;")
                mining = mining.replace("AUDIT SELECT TABLE, INSERT TABLE, UPDATE TABLE, DELETE TABLE, EXECUTE PROCEDURE BY C##OO_MINER BY ACCESS;", "")
            if args.version == "23":
                mining = mining.replace("ALTER SYSTEM SET AUDIT_TRAIL='DB','EXTENDED' SCOPE=SPFILE;", "")
            if args.version == "11":
                mining = mining.replace("ALTER PLUGGABLE DATABASE ALL OPEN;", "")
                mining = mining.replace("C##OO_MINER", "OO_MINER").replace(" CONTAINER=ALL", "")
                mining = mining.replace("CREATE SESSION, LOGMINING, SELECT ANY TRANSACTION", "CREATE SESSION, SELECT ANY TRANSACTION")
            # Capture the active audit mode and trace recursion, rather than
            # treating an empty UNIFIED_AUDIT_TRAIL as proof of zero writes.
            mining = mining.replace("\nEXIT", """
SELECT parameter,value FROM v$option WHERE parameter='Unified Auditing';
SELECT name,value FROM v$parameter WHERE name='audit_trail';
EXIT""")
            run(["docker", "exec", "-i", name, "sqlplus", "-s", "/", "as", "sysdba"],
                "logminer-setup", input=mining, timeout=300)
        if args.c5_only:
            audit_setup = """
SET ECHO OFF VERIFY OFF FEEDBACK OFF
WHENEVER SQLERROR EXIT SQL.SQLCODE
"""
            if args.version != "23":
                audit_setup += """
ALTER SYSTEM SET AUDIT_TRAIL='DB','EXTENDED' SCOPE=SPFILE;
SHUTDOWN IMMEDIATE;
STARTUP;
"""
                if args.version != "11":
                    audit_setup += "ALTER PLUGGABLE DATABASE ALL OPEN;\n"
            audit_setup += switch + "\n"
            if args.version == "11":
                audit_setup += """
AUDIT ALL BY OO_READER BY ACCESS;
AUDIT SELECT TABLE, INSERT TABLE, UPDATE TABLE, DELETE TABLE, EXECUTE PROCEDURE BY OO_READER BY ACCESS;
"""
            else:
                audit_setup += """
CREATE AUDIT POLICY OO_C5_AUDIT ACTIONS ALL;
AUDIT POLICY OO_C5_AUDIT BY OO_READER;
"""
            audit_setup += """
SELECT parameter,value FROM v$option WHERE parameter='Unified Auditing';
SELECT name,value FROM v$parameter WHERE name='audit_trail';
SELECT privilege FROM dba_sys_privs WHERE grantee='OO_READER';
SELECT owner,table_name,privilege FROM dba_tab_privs WHERE grantee='OO_READER';
EXIT
"""
            run(["docker", "exec", "-i", name, "sqlplus", "-s", "/", "as", "sysdba"],
                "c5-audit-setup", input=audit_setup, timeout=300)
        if args.incremental:
            tracing = """
WHENEVER SQLERROR EXIT SQL.SQLCODE
EXEC DBMS_MONITOR.DATABASE_TRACE_ENABLE(waits=>FALSE,binds=>FALSE);
EXIT
"""
            if args.c5_only:
                tracing = tracing.replace("EXEC DBMS_MONITOR", switch + "\nEXEC DBMS_MONITOR")
            run(["docker", "exec", "-i", name, "sqlplus", "-s", "/", "as", "sysdba"],
                "trace-enable", input=tracing)

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
        if args.undo_stress:
            undo_sql = f"""
SET ECHO OFF VERIFY OFF FEEDBACK ON
WHENEVER SQLERROR EXIT SQL.SQLCODE
{switch}
CREATE TABLE OO_FIXTURE.UNDO_PROBE (ID NUMBER(12) PRIMARY KEY, PAYLOAD VARCHAR2(1000));
INSERT INTO OO_FIXTURE.UNDO_PROBE SELECT LEVEL, RPAD('a',1000,'a') FROM DUAL CONNECT BY LEVEL<=12000;
COMMIT;
GRANT SELECT, FLASHBACK ON OO_FIXTURE.UNDO_PROBE TO OO_READER;
BEGIN DBMS_STATS.GATHER_TABLE_STATS('OO_FIXTURE','UNDO_PROBE'); END;
/
DECLARE
  directory VARCHAR2(4000);
BEGIN
  SELECT SUBSTR(file_name,1,INSTR(file_name,'/',-1)) INTO directory
  FROM dba_data_files WHERE tablespace_name='USERS' AND ROWNUM=1;
  EXECUTE IMMEDIATE 'CREATE UNDO TABLESPACE OO_UNDO DATAFILE '''||directory||'oo_undo.dbf'' SIZE 8M AUTOEXTEND OFF';
END;
/
ALTER TABLESPACE OO_UNDO RETENTION NOGUARANTEE;
ALTER SYSTEM SET UNDO_TABLESPACE=OO_UNDO;
ALTER SYSTEM SET UNDO_RETENTION=1;
BEGIN DBMS_SESSION.SLEEP(5); END;
/
SELECT name,value FROM v$parameter WHERE name IN ('undo_tablespace','undo_retention');
SELECT tablespace_name,bytes,autoextensible FROM dba_data_files WHERE tablespace_name='OO_UNDO';
EXIT
"""
            run(["docker", "exec", "-i", name, "sqlplus", "-s", "/", "as", "sysdba"],
                "undo-setup", input=undo_sql, timeout=300)
        env = os.environ.copy()
        env["OO_ORACLE_FIXTURE_CONFIG"] = str(out / "config.json")
        source_files = sorted((ROOT / "crates/oo-connectors/src/oracle").glob("*.rs"))
        source_files.extend(sorted((ROOT / "crates/oo-connectors/tests").glob("oracle_*.rs")))
        source_files.extend(sorted((ROOT / "crates/oo-connectors/tests/oracle_incremental_cases").glob("*.rs")))
        source_files.extend(sorted(Path(__file__).parent.glob("*.rs")))
        source_files.extend(sorted(Path(__file__).parent.glob("*.py")))
        source_files.extend(sorted(Path(__file__).parent.joinpath("worker").glob("*.py")))
        def source_hashes():
            return {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest()
                    for p in source_files}
        sources = source_hashes()
        (out / "source-hashes.json").write_text(json.dumps(sources, indent=2) + "\n")
        command = ["cargo", "test", "-p", "oo-connectors", "--features", "oracle",
             "--locked", "--test", "oracle_undo_real" if args.undo_stress else "oracle_engine_real",
             "--"]
        if args.incremental:
            command = ["cargo", "test", "-p", "oo-connectors", "--features", "oracle", "--locked", "--lib", "--", "s2::real_incremental_cycles"]
        elif not args.undo_stress:
            command.append("oracle_lob_budget_and_hash" if args.lob_only else "oracle_engine_t0_t1_t8")
        command += ["--ignored", "--nocapture"]
        p = subprocess.run(
            command,
            cwd=ROOT, env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            timeout=1800,
        )
        raw = p.stdout
        for password in passwords:
            raw = raw.replace(password, "[redacted]")
        (out / "test.log").write_text(raw)
        (out / "test.exit").write_text(str(p.returncode) + "\n")
        after = source_hashes()
        (out / "source-hashes-after.json").write_text(json.dumps(after, indent=2) + "\n")
        (out / "source-hashes-match.json").write_text(json.dumps({"match": sources == after}) + "\n")
        if sources != after:
            raise RuntimeError("source changed during test; rerun for fixed-source evidence")
        if args.incremental:
            from audit import collect
            collect(run, name, args.version, out,
                    "OO_READER" if args.c5_only else miner_user,
                    mode="c5" if args.c5_only else "c1",
                    service=service if args.c5_only else None)
        if args.undo_stress:
            run(["docker", "exec", "-i", name, "sqlplus", "-s", "/", "as", "sysdba"],
                "undo-observation", input=f"""
WHENEVER SQLERROR EXIT SQL.SQLCODE
{switch}
SELECT SUM(ssolderrcnt) snapshot_too_old, SUM(unxpstealcnt) unexpired_steals,
       SUM(nospaceerrcnt) no_space FROM v$undostat;
SELECT tablespace_name,bytes,autoextensible FROM dba_data_files WHERE tablespace_name='OO_UNDO';
EXIT
""")
        if sources != source_hashes():
            raise RuntimeError("source changed during test; rerun for fixed-source evidence")
        if p.returncode:
            raise RuntimeError("engine test failed (see evidence)")
    finally:
        if worker_id:
            run(["docker", "rm", "-f", "-v", worker_id], "worker-cleanup")
        if created:
            run(["docker", "rm", "-f", "-v", name], "cleanup")
        for path in [env_file, out / "config.json"]:
            path.unlink(missing_ok=True)
        sums = {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                for p in out.iterdir() if p.is_file()}
        (out / "SHA256SUMS.json").write_text(json.dumps(sums, indent=2) + "\n")


if __name__ == "__main__":
    main()
