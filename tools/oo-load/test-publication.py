#!/usr/bin/env python3
"""Owned real-PG candidate publication tests; not Gateway/source acceptance."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

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
    version = None
    try:
        target.start()
        version = target.sql("SELECT version();").strip()
        stack = run.private / "stack.json"
        stack.write_text(json.dumps({
            "version": "oo.stack/v1",
            "sources": [{"id": "owned", "engine": "postgres", "host": "127.0.0.1",
                         "port": 1, "database": "unused", "user": "reader",
                         "password": {"secret_ref": "absent-source-password"},
                         "scope": {"include": ["app.items"]}}],
            "model": {"provider": "openai-compatible", "base_url": "http://127.0.0.1:1",
                      "model": "unused", "key": {"secret_ref": "absent-model-key"}}}))
        cli_target = run.private / "cli-target.json"
        cli_target.write_text(json.dumps({
            "host": "127.0.0.1", "port": target.port, "database": "postgres",
            "user": "postgres", "password": {"secret_ref": "password"},
            "local_fixture": True, "run_id": "unused"}))
        env = dict(os.environ, CARGO_TARGET_DIR=str(ROOT / "target"))
        with (path / "cli-build.log").open("w") as output:
            subprocess.run(["cargo", "build", "--manifest-path", "tools/oo-load/Cargo.toml",
                            "--locked"], cwd=ROOT, env=env, stdout=output,
                           stderr=subprocess.STDOUT, check=True)
        config = run.private / "target.json"
        config.write_text(json.dumps({
            "port": target.port, "password_file": str(target.directory / "password"),
            "cli": str(ROOT / "target/debug/oo-load"), "stack": str(stack),
            "cli_target": str(cli_target), "secrets": str(target.directory)}))
        config.chmod(0o600)
        env["OO_LOAD_TEST_TARGET"] = str(config)
        command = ["cargo", "test", "-p", "oo-pipeline", "--locked",
                   "--test", "native_library_publication",
                   "retained_engine_publication_candidate", "--", "--ignored", "--exact", "--nocapture"]
        with (path / "publication.log").open("w") as output:
            subprocess.run(command, cwd=ROOT, env=env, stdout=output,
                           stderr=subprocess.STDOUT, check=True)
        status = "passed"
    finally:
        try:
            target.close()
        finally:
            cleanup = run.cleanup()
        write_json(path / "result.json", {
            "status": status, "version": version, "cleanup_failures": cleanup,
            "scope": "synthetic-source/real-target candidate only",
            "gateway_acceptance": False})
        checksums(path)


if __name__ == "__main__":
    main()
