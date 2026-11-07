"""Synthetic Db2 fixtures; admin-only, never called by the product connector."""
import argparse
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[3]


def statements(full=False):
    scale = 2_000_000 if full else 2000
    sql = [
        "CONNECT TO FIXTURE",
        "REVOKE CREATETAB, BINDADD, IMPLICIT_SCHEMA ON DATABASE FROM PUBLIC",
        "GRANT CONNECT ON DATABASE TO USER OOREADER",
        "CREATE SCHEMA FIXTURE",
        "CREATE TABLE FIXTURE.EVENTS (ID BIGINT NOT NULL PRIMARY KEY, VALUE VARCHAR(100), AMOUNT DECIMAL(31,8), INSTANT TIMESTAMP(12))",
    ]
    # Separate autocommit batches fit the unmodified fixture's transaction log.
    for offset in range(0, scale, 100_000):
        sql.append(f"""INSERT INTO FIXTURE.EVENTS
WITH D(N) AS (VALUES 0,1,2,3,4,5,6,7,8,9),
IDS(ID) AS (SELECT {offset + 1}+A.N+10*B.N+100*C.N+1000*E.N+10000*F.N
FROM D A,D B,D C,D E,D F)
SELECT ID,'row-'||VARCHAR(ID),DECIMAL(ID,31,8)/7,
CAST('2026-11-05-01.02.03.123456789012' AS TIMESTAMP(12))
FROM IDS WHERE ID<={scale}""")
    sql += [
        "CREATE TABLE FIXTURE.COMPOSITE (K VARCHAR(20) NOT NULL, ID INTEGER NOT NULL, VALUE VARCHAR(30), PRIMARY KEY(K,ID))",
        "INSERT INTO FIXTURE.COMPOSITE VALUES ('a',1,'first'),('a',2,'second'),('A',1,'upper'),('b',1,NULL),('z',9,'last')",
        "CREATE TABLE FIXTURE.EMPTY (ID INTEGER NOT NULL PRIMARY KEY, VALUE VARCHAR(20))",
        "CREATE TABLE FIXTURE.NO_KEY (VALUE VARCHAR(20))",
        "INSERT INTO FIXTURE.NO_KEY VALUES ('same'),('same'),(NULL)",
    ]
    tables = ["EVENTS", "COMPOSITE", "EMPTY", "NO_KEY"]
    if full:
        sql.append("CREATE TABLE FIXTURE.WIDE_ROWS (ID INTEGER NOT NULL PRIMARY KEY," +
                   ",".join(f"C{i:03} VARCHAR(8)" for i in range(1, 300)) + ")")
        sql.append("INSERT INTO FIXTURE.WIDE_ROWS VALUES (1," +
                   ",".join("NULL" if i % 7 == 0 else f"'c{i}'" for i in range(1, 300)) + ")")
        tables.append("WIDE_ROWS")
        for i in range(226):
            name = f"TABLE_{i:03}"
            tables.append(name)
            sql += [f"CREATE TABLE FIXTURE.{name} (ID INTEGER NOT NULL PRIMARY KEY, VALUE VARCHAR(30))",
                    f"INSERT INTO FIXTURE.{name} VALUES (1,'table-{i}'),(2,NULL)"]
    sql += [f"GRANT SELECT ON FIXTURE.{t} TO USER OOREADER" for t in tables]
    return ";\n".join(sql) + ";\n", tables, scale


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--full", action="store_true")
    args = parser.parse_args()
    if not re.fullmatch(r"[a-z0-9-]{1,64}", args.run_id):
        parser.error("run-id")
    out = ROOT / "evidence" / args.run_id
    owned = json.loads((out / "fixture-owned.json").read_text())["container"]
    sql, tables, scale = statements(args.full)
    (out / "seed-generated.sql").write_text(sql)
    with (out / "seed-generated.log").open("w") as log:
        p = subprocess.run(["docker", "exec", "-i", owned, "su", "-", "db2inst1", "-c",
                            "db2 +p -st"], input=sql, text=True,
                           stdout=log, stderr=subprocess.STDOUT, timeout=900)
    (out / "seed-receipt.json").write_text(json.dumps({
        "exit": p.returncode, "tables": tables, "event_rows": scale, "wide_columns": 300 if args.full else 0,
    }, indent=2))
    return p.returncode


if __name__ == "__main__":
    raise SystemExit(main())
