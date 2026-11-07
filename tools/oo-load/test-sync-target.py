#!/usr/bin/env python3
"""Real target transaction and process-death regression; not an engine T2 pass."""
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
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-id", required=True)
    args = parser.parse_args()
    if not args.run_id or not all(c.isalnum() or c in "-_" for c in args.run_id):
        parser.error("invalid run ID")
    path = ROOT / "evidence" / args.run_id
    path.mkdir(mode=0o700, exist_ok=False)
    run = LaneRun(path, "postgres", "17", argparse.Namespace())
    target = AgentDB(run)
    status = "failed"
    child = None
    try:
        target.start()
        config = run.private / "target.json"
        config.write_text(json.dumps({"port": target.port,
                                     "password_file": str(target.directory / "password")}))
        config.chmod(0o600)
        env = dict(os.environ, OO_LOAD_TEST_TARGET=str(config), CARGO_TARGET_DIR=str(ROOT / "target"))
        command = ["cargo", "test", "--manifest-path", "tools/oo-load/Cargo.toml", "--locked",
                   "--test", "sync_target"]
        with (path / "build.jsonl").open("w") as out, (path / "build.log").open("w") as err:
            subprocess.run(command + ["--no-run", "--message-format=json"], cwd=ROOT,
                           env=env, stdout=out, stderr=err, check=True)
        artifacts = [json.loads(line) for line in (path / "build.jsonl").read_text().splitlines()]
        binaries = [item["executable"] for item in artifacts
                    if item.get("reason") == "compiler-artifact" and item.get("executable")
                    and item["target"]["name"] == "sync_target"]
        if len(binaries) != 1:
            raise RuntimeError("test binary identity")
        binary = binaries[0]
        with (path / "atomic.log").open("w") as out:
            subprocess.run([binary, "--exact", "sync_target_atomic_overlay_and_resume",
                            "--ignored", "--nocapture"], env=env, stdout=out,
                           stderr=subprocess.STDOUT, timeout=90, check=True)
        with (path / "c5.log").open("w") as out:
            subprocess.run([binary, "--exact", "c5_target_reuses_parent_and_no_change_writes_nothing",
                            "--ignored", "--nocapture"], env=env, stdout=out,
                           stderr=subprocess.STDOUT, timeout=90, check=True)
        with (path / "publication-snapshots.log").open("w") as out:
            subprocess.run([binary, "--exact", "three_publications_preserve_inflight_repeatable_read",
                            "--ignored", "--nocapture"], env=env, stdout=out,
                           stderr=subprocess.STDOUT, timeout=90, check=True)
        marker = run.private / "committed"
        env.update(OO_SYNC_MARKER=str(marker), OO_SYNC_PHASE="commit")
        with (path / "kill.log").open("w") as out:
            child = subprocess.Popen([binary, "--exact", "sync_target_sigkill", "--ignored",
                                      "--nocapture"], env=env, stdout=out, stderr=subprocess.STDOUT)
            deadline = time.monotonic() + 90
            while not marker.exists() and child.poll() is None and time.monotonic() < deadline:
                time.sleep(0.05)
            if not marker.exists():
                raise RuntimeError("target commit marker missing")
            child.send_signal(signal.SIGKILL)
            if child.wait(timeout=10) != -signal.SIGKILL:
                raise RuntimeError("unexpected process exit")
        before = target.sql("SELECT source,position,generation,write_bytes FROM _oo_load.sync_state ORDER BY source;")
        env["OO_SYNC_PHASE"] = "resume"
        with (path / "resume.log").open("w") as out:
            subprocess.run([binary, "--exact", "sync_target_sigkill", "--ignored", "--nocapture"],
                           env=env, stdout=out, stderr=subprocess.STDOUT, timeout=90, check=True)
        after = target.sql("SELECT source,position,generation,write_bytes FROM _oo_load.sync_state ORDER BY source;")
        if before != after:
            raise RuntimeError("resume rewrote target state")
        status = "passed"
    finally:
        if child is not None and child.poll() is None:
            child.kill()
            child.wait()
        try:
            target.close()
        finally:
            cleanup = run.cleanup()
        write_json(path / "result.json", {"status": status, "cleanup_failures": cleanup,
                                         "scope": "synthetic-source/real-target; not T2"})
        checksums(path)


if __name__ == "__main__":
    main()
