#!/usr/bin/env python3
"""Run the ignored trait-split test with an admin-only growth handshake."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import time

from t1 import LaneRun, ROOT, configure_mysql, configure_pg, private
from runner import checksums, write_json


def main():
    def interrupted(signum, frame):
        raise KeyboardInterrupt(f"signal {signum}")
    signal.signal(signal.SIGINT, interrupted)
    signal.signal(signal.SIGTERM, interrupted)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--engine", choices=["mysql", "postgres", "oracle"], required=True)
    parser.add_argument("--versions", required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--test-binary", type=Path, required=True)
    args = parser.parse_args()
    if not args.run_id or any(c not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_" for c in args.run_id):
        parser.error("invalid run ID")
    base = ROOT / "evidence" / args.run_id
    base.mkdir(mode=0o700, exist_ok=False)
    results = []
    for version in args.versions.split(","):
        path = base / (args.engine + "-" + version)
        path.mkdir(mode=0o700)
        run = LaneRun(path, args.engine, version, argparse.Namespace(
            scale=100, tables=0, platform=None, health_timeout=None))
        store = path / ".split-private"
        store.mkdir(mode=0o700)
        child = None
        result = {"version": version, "status": "failed",
                  "test_binary_sha256": hashlib.sha256(args.test_binary.read_bytes()).hexdigest()}
        try:
            run.setup()
            run.seed()
            if args.engine == "oracle":
                run.sql("GRANT SELECT ON SYS.V_$DATABASE TO fixture_reader;\n"
                        "GRANT SELECT ON SYS.V_$SESSION TO fixture_reader;\n"
                        "GRANT SELECT ON SYS.V_$SQL_PLAN TO fixture_reader;\n"
                        "GRANT SELECT ON SYS.DBA_EXTENTS TO fixture_reader;\n"
                        "GRANT EXECUTE ON SYS.DBMS_ROWID TO fixture_reader;\n"
                        "GRANT FLASHBACK ON growth_events TO fixture_reader;\n", admin=True)
            else:
                (configure_pg if args.engine == "postgres" else configure_mysql)(run, store)
            port = {"postgres": 5432, "mysql": 3306, "oracle": 1521}[args.engine]
            _, published = run.command(["docker", "port", run.name, f"{port}/tcp"])
            private(store / "source-password", run.reader_secret)
            private(store / "config.json", json.dumps({
                "engine": args.engine, "port": int(published.strip().rsplit(":", 1)[1])}))
            inserts = "".join(f"INSERT INTO growth_events VALUES({n*10},1);\n" for n in range(1, 9))
            run.sql("DELETE FROM growth_events;\n" + inserts + "COMMIT;\n", admin=True)
            with (path / "test.log").open("w") as output:
                child = subprocess.Popen([
                    str(args.test_binary.resolve()), "--exact", "real_growing_table_uses_trait_split",
                    "--ignored", "--nocapture",
                ], env=dict(os.environ, OO_LOAD_SPLIT_CONFIG=str(store / "config.json")),
                    stdout=output, stderr=subprocess.STDOUT)
                deadline = time.monotonic() + 120
                while not (store / "planned").exists():
                    if child.poll() is not None or time.monotonic() > deadline:
                        raise RuntimeError("split plan handshake failed")
                    time.sleep(0.05)
                inserts = "".join(f"INSERT INTO growth_events VALUES({n},1);\n" for n in range(81, 101))
                run.sql(inserts + "COMMIT;\n", admin=True)
                (store / "grown").write_text("ready")
                if child.wait(timeout=180) != 0:
                    raise RuntimeError("trait split test failed")
            result.update(json.loads((store / "split-result.json").read_text()), status="passed")
        except Exception as error:
            result["error"] = str(error) if isinstance(error, RuntimeError) else type(error).__name__
        finally:
            if child is not None and child.poll() is None:
                child.kill()
                child.wait()
            result["cleanup_failed"] = run.cleanup()
            shutil.rmtree(store)
            write_json(path / "result.json", result)
            checksums(path)
        results.append(result)
        write_json(base / "results.json", results)
    raise SystemExit(0 if all(r["status"] == "passed" and not r["cleanup_failed"] for r in results) else 1)


if __name__ == "__main__":
    main()
