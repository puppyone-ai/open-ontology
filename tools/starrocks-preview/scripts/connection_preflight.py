#!/usr/bin/env python3
"""Same-identity, read-only OO CLI preflight for the StarRocks whole run."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import traceback


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def save(path, value):
    path = Path(path)
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + "\n")
    os.chmod(path, 0o600)


def run(cli, argv, output):
    proc = subprocess.run([cli, *argv], capture_output=True, text=True, timeout=60)
    output = Path(output)
    output.mkdir(mode=0o700, parents=True, exist_ok=True)
    (output / "stdout").write_text(proc.stdout)
    (output / "stderr").write_text(proc.stderr)
    (output / "exit").write_text(str(proc.returncode) + "\n")
    for name in ("stdout", "stderr", "exit"):
        os.chmod(output / name, 0o600)
    body = None
    try:
        body = json.loads(proc.stdout)
    except json.JSONDecodeError:
        pass
    return proc, body


def main(config_path, run_dir):
    config = json.loads(Path(config_path).read_text())
    run_dir = Path(run_dir)
    dialogue_config_path = run_dir / "dialogue-config.json"
    config = json.loads(dialogue_config_path.read_text())
    identity = config["identity"]
    token = Path(config["gateway_token_file"])
    token_sha = sha(token)
    cli = config["oo_cli"]
    common = ["--json", "--gateway", config["gateway"], "--transport", config["transport"],
              "--auth", config["auth"], "--token-file", str(token)]
    commands = {
        "describe": ["describe", "oo_w45_sr.dup_partition_sums", *common],
        "freshness": ["freshness", *common],
        "query": ["query", "SELECT part,row_count,amount_sum FROM oo_w45_sr.dup_partition_sums ORDER BY part",
                  "--max-rows", "100", "--timeout-ms", "10000", *common],
    }
    out = run_dir / "preflight"
    results = {}
    for name, argv in commands.items():
        proc, body = run(cli, argv, out / name)
        results[name] = {"exit": proc.returncode, "body": body}
    pack = Path(config["pack_file"])
    checks = {}
    for name, result in results.items():
        body = result["body"]
        checks[name] = result["exit"] == 0 and isinstance(body, dict) and not body.get("error")
    checks["source_generation"] = identity.get("source_generation") == 19
    checks["role_pack"] = sha(pack) == identity.get("pack_sha256")
    checks["token"] = token_sha == config.get("token_sha256")
    passed = all(checks.values())
    save(out / "identity.json", {
        "protocol": "oo.source-onboarding.cli-preflight/v1",
        "passed": passed,
        "read_only": True,
        "model_calls": 0,
        "results_injected": False,
        "source_id": identity.get("source_id"),
        "source_generation": identity.get("source_generation"),
        "role": identity.get("role"),
        "pack_sha256": identity.get("pack_sha256"),
        "token_sha256": token_sha,
        "checks": checks,
        "query_ids": [] if not isinstance(results["query"]["body"], dict) else [results["query"]["body"].get("query_id")],
    })
    if not passed:
        raise RuntimeError("same-identity read-only OO CLI preflight failed")


if __name__ == "__main__":
    try:
        main(sys.argv[1], sys.argv[2])
    except Exception as exc:
        traceback.print_exc(file=sys.stderr)
        print("starrocks-cli-preflight: " + str(exc), file=sys.stderr)
        raise SystemExit(1)
