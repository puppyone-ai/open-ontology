#!/usr/bin/env python3
"""Create, seed, or remove an isolated lane-d8 DM8 fixture and its DPI worker.

The server is the owner-authorised (E340) native arm64 image. Data is
synthetic and written only through the separate SYSDBA admin connection.
Reader and admin passwords are generated per run into ignored 0600 files.
"""
import argparse
import json
import os
from pathlib import Path
import secrets
import subprocess
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parent))
from admin import LANE, ROOT, Admin, owned, private, run_dir  # noqa: E402

IMAGE = "sha256:8ddae8147e7a3a3f9bb20c0988a35857dfcaf1bbf2944a1babc5a7f513274f75"
IMAGE_TAG = "oo-e-dm8-arm:e-s5f-20261105"
READER = "OOREADER"
FOCUSED = ["EVENTS", "TYPES", "COMPOSITE", "DEC_KEY", "EMPTY", "NO_KEY"]

TYPES_DDL = (
    "CREATE TABLE FIXTURE.TYPES (ID BIGINT NOT NULL PRIMARY KEY, I INT, S SMALLINT, "
    "T TINYINT, B BYTE, BT BIT, D DECIMAL(31,8), N NUMBER(10,2), NN NUMBER, F FLOAT, "
    "DB DOUBLE, R REAL, DT DATE, TS TIMESTAMP(6), TS9 TIMESTAMP(9), "
    "TSZ TIMESTAMP(6) WITH TIME ZONE, TSL TIMESTAMP(6) WITH LOCAL TIME ZONE, TM TIME(3), "
    "V VARCHAR(50), V2 VARCHAR2(50), C CHAR(5), CL CLOB, TX TEXT, BL BLOB, BI BINARY(4), "
    "VB VARBINARY(8), IV INTERVAL DAY TO SECOND)"
)
TYPES_ROWS = [
    "1,-2147483648,-32768,-128,-128,1,CAST(1 AS DECIMAL(31,8))/7,-12345678.91,"
    "123456789012345678901234567890.123456789,1.5,0.1,3.25,DATE '2026-11-05',"
    "TIMESTAMP '2026-11-05 01:02:03.123456',"
    "TO_TIMESTAMP('2026-11-05 01:02:03.123456789','YYYY-MM-DD HH24:MI:SS.FF9'),"
    "CAST('2026-11-05 01:02:03.123456 +05:30' AS TIMESTAMP(6) WITH TIME ZONE),"
    "TIMESTAMP '2026-11-05 01:02:03.5',TIME '01:02:03.456','héllo 中文','x|y','ab',"
    "'clob 中','',HEXTORAW('00FF10'),HEXTORAW('DEADBEEF'),HEXTORAW('0102'),"
    "INTERVAL '1 02:03:04.5' DAY TO SECOND",
    "2,2147483647,32767,127,127,0,-99999999999999999999999.99999999,99999999.99,"
    "-0.000000001,-2.5,1E300,-0.5,DATE '1969-12-31',TIMESTAMP '1900-01-01 00:00:00',"
    "TO_TIMESTAMP('1969-12-31 23:59:59.999999999','YYYY-MM-DD HH24:MI:SS.FF9'),"
    "CAST('1969-12-31 23:00:00 -08:00' AS TIMESTAMP(6) WITH TIME ZONE),"
    "TIMESTAMP '1999-12-31 23:59:59.999999',TIME '23:59:59.999','',' lead','Z',"
    "'x','long text',HEXTORAW('00'),HEXTORAW('00000000'),HEXTORAW('FF'),"
    "INTERVAL '-3 00:00:00' DAY TO SECOND",
    "3" + ",NULL" * 26,
]


def statements(full=False):
    """Seed SQL (one statement per line) and the reader's table scope."""
    scale = 2_000_000 if full else 2000
    sql = [
        f'CREATE USER {READER} IDENTIFIED BY "{{reader}}"',
        # disql reads CREATE SCHEMA as a block that only "/" terminates.
        "CREATE SCHEMA FIXTURE;\n/",
        "CREATE TABLE FIXTURE.EVENTS (ID BIGINT NOT NULL PRIMARY KEY, VALUE VARCHAR(100), "
        "AMOUNT DECIMAL(31,8), INSTANT TIMESTAMP(6), ZONED TIMESTAMP(6) WITH TIME ZONE)",
    ]
    for offset in range(0, scale, 500_000):
        rows = min(500_000, scale - offset)
        sql += [
            "INSERT INTO FIXTURE.EVENTS SELECT ID, CASE WHEN MOD(ID,97)=0 THEN NULL ELSE "
            "'row-'||ID END, CAST(ID AS DECIMAL(31,8))/7, TIMESTAMP '2026-11-05 01:02:03.123456'"
            "+NUMTODSINTERVAL(ID,'SECOND'), CAST('2026-11-05 01:02:03.123456 +05:30' AS "
            "TIMESTAMP(6) WITH TIME ZONE) FROM (SELECT LEVEL+" + str(offset) +
            f" AS ID FROM DUAL CONNECT BY LEVEL <= {rows})",
            "COMMIT",
        ]
    sql += [TYPES_DDL] + [f"INSERT INTO FIXTURE.TYPES VALUES ({row})" for row in TYPES_ROWS]
    sql += [
        "CREATE TABLE FIXTURE.COMPOSITE (K VARCHAR(20) NOT NULL, ID INT NOT NULL, "
        "VALUE VARCHAR(30), PRIMARY KEY(K,ID))",
        "INSERT INTO FIXTURE.COMPOSITE VALUES ('a',1,'first'),('a',2,'second'),"
        "('A',1,'upper'),('b',1,NULL),('z',9,'last'),('é',1,'accent'),('中',1,'cjk')",
        "CREATE TABLE FIXTURE.DEC_KEY (ID DECIMAL(12,3) NOT NULL PRIMARY KEY, VALUE VARCHAR(20))",
        "INSERT INTO FIXTURE.DEC_KEY VALUES (-1.5,'neg'),(0,'zero'),(2.25,'two'),(1000.125,'big')",
        "CREATE TABLE FIXTURE.EMPTY (ID INT NOT NULL PRIMARY KEY, VALUE VARCHAR(20))",
        "CREATE TABLE FIXTURE.NO_KEY (VALUE VARCHAR(20))",
        "INSERT INTO FIXTURE.NO_KEY VALUES ('same'),('same'),(NULL)",
    ]
    tables = list(FOCUSED)
    if full:
        sql.append("CREATE TABLE FIXTURE.WIDE_ROWS (ID INT NOT NULL PRIMARY KEY," +
                   ",".join(f"C{i:03} VARCHAR(8)" for i in range(1, 300)) + ")")
        sql.append("INSERT INTO FIXTURE.WIDE_ROWS VALUES (1," +
                   ",".join("NULL" if i % 7 == 0 else f"'c{i}'" for i in range(1, 300)) + ")")
        tables.append("WIDE_ROWS")
        for i in range(231 - len(tables)):
            name = f"TABLE_{i:03}"
            tables.append(name)
            sql += [f"CREATE TABLE FIXTURE.{name} (ID INT NOT NULL PRIMARY KEY, VALUE VARCHAR(30))",
                    f"INSERT INTO FIXTURE.{name} VALUES (1,'table-{i}'),(2,NULL)"]
    sql.append("COMMIT")
    sql += [f"GRANT SELECT ON FIXTURE.{t} TO {READER}" for t in tables]
    return sql, tables, scale


def script(sql):
    return "".join(s + ("\n" if s.endswith("\n/") else ";\n") for s in sql)


def control(password):
    return ("[FIXTURE]\nsystem_path=/opt/dmdbms/data\nlog_size=256\n"
            "auto_adj_mem=1024\ncharset=1\n"
            f"sysdba_pwd={password}\nsysauditor_pwd={password}\n")


def create(run_id, full):
    base = run_dir(run_id)
    base.mkdir(parents=True, exist_ok=False)
    if subprocess.check_output(["docker", "image", "inspect", "--format", "{{.Id}}", IMAGE_TAG],
                               text=True).strip() != IMAGE:
        raise ValueError("DM8 image identity")
    admin_password = "Dm" + secrets.token_hex(20)
    reader_password = "Rd" + secrets.token_hex(20)
    private(base / ".private/admin", admin_password)
    private(base / ".private/reader", reader_password)
    name = f"oo-d8-dm8-{run_id}"
    subprocess.run(["docker", "run", "-d", "--platform", "linux/arm64",
                    "--label", "oo.lane=" + LANE, "--label", "oo.run_id=" + run_id,
                    "--name", name, "--memory", "3g", "--cpus", "2",
                    "--entrypoint", "sleep", IMAGE, "infinity"],
                   check=True, stdout=subprocess.DEVNULL, timeout=120)
    (base / "fixture-owned.json").write_text(json.dumps({"container": name, "image": IMAGE}))
    if subprocess.check_output(["docker", "exec", name, "uname", "-m"], text=True).strip() != "aarch64":
        raise ValueError("not native arm64")
    subprocess.run(["docker", "exec", "-i", name, "sh", "-c",
                    "umask 077; cat > /opt/dmdbms/dminit-private.ini"],
                   input=control(admin_password), text=True, check=True, timeout=60)
    with (base / "fixture-init.log").open("w") as log:
        subprocess.run(["docker", "exec", name, "/opt/dmdbms/bin/dminit",
                        "CONTROL=/opt/dmdbms/dminit-private.ini"],
                       stdout=log, stderr=subprocess.STDOUT, check=True, timeout=300)
    subprocess.run(["docker", "exec", name, "rm", "-f", "/opt/dmdbms/dminit-private.ini"], check=True)
    # Admin disql speaks UTF-8 to the UTF-8 server (its default is GB18030).
    subprocess.run(["docker", "exec", "-u", "0", "-i", name, "sh", "-c", "cat > /etc/dm_svc.conf"],
                   input="CHAR_CODE=(PG_UTF8)\n", text=True, check=True, timeout=60)
    subprocess.run(["docker", "exec", "-d", name, "sh", "-c",
                    "exec /opt/dmdbms/bin/dmserver /opt/dmdbms/data/DAMENG/dm.ini "
                    "> /opt/dmdbms/server.log 2>&1"], check=True, timeout=60)
    for _ in range(120):
        log = subprocess.run(["docker", "exec", name, "cat", "/opt/dmdbms/server.log"],
                             capture_output=True, text=True).stdout
        if "SYSTEM IS READY." in log:
            break
        time.sleep(1)
    else:
        raise RuntimeError("server_not_ready")
    admin = Admin(run_id)
    sql, tables, scale = statements(full)
    (base / "seed-generated.sql").write_text(script(sql))
    started = time.monotonic()
    seed = script(s.replace("{reader}", reader_password) for s in sql)
    (base / "seed.log").write_text(admin.sql(seed, timeout=1800).replace(reader_password, "[redacted]"))
    counts = admin.rows("SELECT 'R|'||COUNT(*) FROM FIXTURE.EVENTS")
    if counts != [str(scale)]:
        raise RuntimeError("seed_scale")
    (base / "seed-receipt.json").write_text(json.dumps({
        "tables": tables, "event_rows": scale, "wide_columns": 300 if full else 0,
        "full": full, "seconds": round(time.monotonic() - started, 3)}, indent=2))


def build_worker(run_id):
    """Build the DPI worker from the authorised image and write its wrapper."""
    base = run_dir(run_id)
    record = owned(run_id)
    tag = f"oo-d8-dm8-worker:{run_id}"
    with (base / "worker-build.log").open("w") as log:
        subprocess.run(["docker", "build", "--platform", "linux/arm64",
                        "--label", "oo.lane=" + LANE, "--label", "oo.run_id=" + run_id,
                        "--build-arg", "DM8_IMAGE=" + IMAGE_TAG, "-t", tag,
                        str(Path(__file__).resolve().parent / "worker")],
                       stdout=log, stderr=subprocess.STDOUT, check=True, timeout=1800)
    image = subprocess.check_output(["docker", "image", "inspect", "--format", "{{.Id}}", tag],
                                    text=True).strip()
    wrapper = base / "worker"
    wrapper.write_text(
        "#!/bin/sh\nexec docker run --rm -i --platform linux/arm64 "
        f"--label oo.lane={LANE} --label oo.run_id={run_id} --label oo.role=dm8-worker "
        f"--network container:{record['container']} --memory 256m --cpus 1 --read-only "
        f"--tmpfs /tmp --cap-drop ALL --security-opt no-new-privileges {image}\n")
    wrapper.chmod(0o700)
    (base / "worker-image.json").write_text(json.dumps({"image": image, "tag": tag}))
    return image


def remove(run_id):
    base = run_dir(run_id)
    result = {}
    record = owned(run_id)
    result["fixture_rm"] = subprocess.run(["docker", "rm", "-f", record["container"]],
                                          capture_output=True).returncode
    image = base / "worker-image.json"
    if image.exists():
        tag = json.loads(image.read_text())["tag"]
        labels = json.loads(subprocess.check_output(
            ["docker", "image", "inspect", "--format", "{{json .Config.Labels}}", tag], text=True))
        if labels.get("oo.lane") == LANE and labels.get("oo.run_id") == run_id:
            result["worker_rmi"] = subprocess.run(["docker", "rmi", tag],
                                                  capture_output=True).returncode
    (base / "cleanup.json").write_text(json.dumps(result, indent=2))
    return 0 if all(v == 0 for v in result.values()) else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["create", "worker", "remove"])
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--full", action="store_true")
    args = parser.parse_args()
    if args.action == "create":
        create(args.run_id, args.full)
        build_worker(args.run_id)
        return 0
    if args.action == "worker":
        build_worker(args.run_id)
        return 0
    return remove(args.run_id)


if __name__ == "__main__":
    raise SystemExit(main())
