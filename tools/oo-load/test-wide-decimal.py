#!/usr/bin/env python3
"""Owned MySQL 8.4 -> Arrow -> chunk COPY -> PostgreSQL byte-exact regression."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
os.environ["OO_TEST_LANE"] = "v03"
sys.path.insert(0, str(ROOT / "tools/oo-test"))
from t1 import AgentDB, LaneRun, configure_mysql, configure_pg, private


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--engine", choices=["mysql", "postgres"], default="mysql")
    args = parser.parse_args()
    if not args.run_id or any(c not in "abcdefghijklmnopqrstuvwxyz0123456789-" for c in args.run_id):
        parser.error("invalid run id")
    path = ROOT / "evidence" / args.run_id
    path.mkdir(mode=0o700)
    version = "8.4.7" if args.engine == "mysql" else "17"
    run = LaneRun(path, args.engine, version, argparse.Namespace(
        scale=100, tables=0, platform=None, health_timeout=300))
    target = AgentDB(run)
    store = path / ".wide-private"
    store.mkdir(mode=0o700)
    result = {"status": "failed", "source": args.engine + " " + version, "target": "PostgreSQL 17"}
    try:
        run.setup()
        if args.engine == "mysql":
            configure_mysql(run, store)
            table = "fixture.wide_decimal"
        else:
            run.sql("CREATE ROLE fixture_reader LOGIN;", admin=True)
            configure_pg(run, store)
            table = "public.wide_decimal"
        run.sql(f"CREATE TABLE {table} "
                "(id BIGINT PRIMARY KEY,a DECIMAL(40,12),b DECIMAL(65,30));", admin=True)
        if args.engine == "postgres":
            run.sql(f"GRANT SELECT ON {table} TO fixture_reader;", admin=True)
        values = [
            ("0." + "0" * 12, "0." + "0" * 30),
            ("9" * 28 + "." + "9" * 12, "9" * 35 + "." + "9" * 30),
            ("-" + "9" * 28 + "." + "9" * 12, "-" + "9" * 35 + "." + "9" * 30),
            ("-0." + "0" * 11 + "1", "-0." + "0" * 29 + "1"),
        ]
        rows = ",".join(f"({i},'{a}','{b}')" for i, (a, b) in enumerate(values))
        run.sql(f"INSERT INTO {table} VALUES " + rows + ",(4,NULL,NULL);", admin=True)
        cast = "CHAR" if args.engine == "mysql" else "TEXT"
        _, raw = run.sql(f"SELECT id,CAST(a AS {cast}),CAST(b AS {cast}) FROM {table} ORDER BY id;")
        source = []
        for line in raw.strip().splitlines():
            cells = line.split("\t" if args.engine == "mysql" else "|")
            source.append([int(cells[0]), *[None if v in ("NULL", "") else v for v in cells[1:]]])
        assert len(source) == 5
        private(store / "source-rows.json", json.dumps(source))
        private(store / "source-password", run.reader_secret)
        _, port = run.command(["docker", "port", run.name, "3306/tcp" if args.engine == "mysql" else "5432/tcp"])
        target.start()
        private(store / "target-password", target.password)
        private(store / "config.json", json.dumps({
            "source_port": int(port.strip().rsplit(":", 1)[1]), "target_port": target.port, "engine": args.engine,
        }))
        with (path / "test.log").open("wb") as log:
            proc = subprocess.run(
                ["cargo", "test", "--locked", "--manifest-path", "tools/oo-load/Cargo.toml",
                 "--test", "wide_decimal", "--", "--ignored", "--nocapture"],
                cwd=ROOT, env=dict(os.environ, CARGO_TARGET_DIR=str(ROOT / "target"),
                                  OO_WIDE_DECIMAL_CONFIG=str(store / "config.json")),
                stdout=log, stderr=subprocess.STDOUT, timeout=1200)
        result["exit"] = proc.returncode
        if proc.returncode:
            raise RuntimeError("wide_decimal_test_failed")
        result.update(status="passed", rows=5, comparison="byte-exact source CAST AS CHAR vs target numeric text")
    finally:
        target.close()
        result["cleanup"] = run.cleanup()
        shutil.rmtree(store)
        (path / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
