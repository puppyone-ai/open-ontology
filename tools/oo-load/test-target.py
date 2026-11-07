#!/usr/bin/env python3
"""Execute the ignored fake-SourceEngine test against one owned fresh Agent DB."""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools/oo-test"))
from t1 import AgentDB, LaneRun
from runner import checksums, write_json


def main():
    def interrupted(signum, frame):
        raise KeyboardInterrupt(f"signal {signum}")
    signal.signal(signal.SIGINT, interrupted)
    signal.signal(signal.SIGTERM, interrupted)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-id", required=True)
    args = parser.parse_args()
    if not args.run_id or not all(c.isalnum() or c in "-_" for c in args.run_id):
        parser.error("invalid run ID")
    path = ROOT / "evidence" / args.run_id
    path.mkdir(mode=0o700, exist_ok=False)
    run = LaneRun(path, "postgres", "17", argparse.Namespace())
    target = AgentDB(run)
    code = 1
    try:
        target.start()
        config = run.private / "target.json"
        config.write_text(json.dumps({"port": target.port,
                                     "password_file": str(target.directory / "password")}))
        config.chmod(0o600)
        os.environ["OO_LOAD_TEST_TARGET"] = str(config)
        os.environ["CARGO_TARGET_DIR"] = str(ROOT / "target")
        code, _ = run.command([
            "cargo", "test", "--manifest-path", "tools/oo-load/Cargo.toml", "--locked",
            "fake_source_engine_to_real_target", "--", "--ignored", "--nocapture",
        ], timeout=1200, required=False)
        if code:
            raise RuntimeError("fake target regression failed")
        code = 1
        _, artifacts = run.command([
            "cargo", "test", "--manifest-path", "tools/oo-load/Cargo.toml", "--locked",
            "--no-run", "--message-format=json",
        ], timeout=1200)
        binaries = [
            item["executable"] for line in artifacts.splitlines()
            if line.startswith("{")
            for item in [json.loads(line)]
            if item.get("reason") == "compiler-artifact" and item.get("executable")
            and item["target"]["kind"] == ["lib"] and item["target"]["name"] == "oo_load"
        ]
        if len(binaries) != 1:
            raise RuntimeError("test binary identity")
        results = []
        for phase in ("stage", "published", "complete"):
            marker = run.private / (phase + "-marker")
            env = dict(os.environ, OO_LOAD_TEST_PHASE=phase, OO_LOAD_TEST_MARKER=str(marker))
            with (path / (phase + ".log")).open("w") as output:
                child = subprocess.Popen([
                    binaries[0], "--exact", "tests::sigkill_target_fixture", "--ignored", "--nocapture",
                ], env=env, stdout=output, stderr=subprocess.STDOUT)
                try:
                    if phase != "complete":
                        deadline = time.monotonic() + 60
                        while not marker.exists() and child.poll() is None and time.monotonic() < deadline:
                            time.sleep(0.05)
                        if not marker.exists():
                            raise RuntimeError("commit checkpoint missing")
                        child.send_signal(signal.SIGKILL)
                    exit_code = child.wait(timeout=60)
                    if exit_code != (-signal.SIGKILL if phase != "complete" else 0):
                        raise RuntimeError("unexpected child exit")
                finally:
                    if child.poll() is None:
                        child.kill()
                        child.wait()
            raw = target.sql(
                "SELECT count(*),sum((c.receipt->>'rows')::bigint),sum(c.write_bytes) "
                "FROM _oo_load.chunks c JOIN _oo_load.schemas s ON s.stage=c.stage WHERE s.run='kill-1';")
            count, rows, size = map(int, raw.strip().split("|"))
            results.append({"phase": phase, "exit": exit_code, "receipts": count, "rows": rows, "write_bytes": size})
            write_json(path / "sigkill-results.json", results)
            if count != 1 or rows != 3 or size <= 0 or size != results[0]["write_bytes"]:
                raise RuntimeError("duplicate apply or write metering changed")
        code = 0
    except Exception:
        code = 1
        raise
    finally:
        target.close()
        run.cleanup()
        write_json(path / "result.json", {"exit": code, "test": "fake_source_engine_to_real_target"})
        checksums(path)
    raise SystemExit(code)


if __name__ == "__main__":
    main()
