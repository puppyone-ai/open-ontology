#!/usr/bin/env python3
"""Static oo-load/Agent DB comparison; not concurrent-writer T1 or T8."""
import json
from pathlib import Path
import re
import secrets
import subprocess
import sys
import time

from run import ROOT, preflight, private, run


def expected():
    return {
        "rows": [{"id": i, "payload": p} for i, p in
                 [(1, None), (2, ""), (3, "three"), (4, "four"), (5, "five"), (6, "six")]],
        "text_rows": [{"k": k, "payload": p} for k, p in
                      [("a", "lower"), ("B", "upper"), ("trail ", "space")]],
        "composite_rows": [{"tenant": t, "seq": s, "payload": p} for t, s, p in
                           [("a", 1, "one"), ("a", 2, None), ("b", 1, ""), ("b", 2, "two")]],
        "no_key": [{"id": 1, "payload": None}, {"id": 1, "payload": None},
                   {"id": 2, "payload": ""}],
        "empty_rows": [],
    }


def canonical(rows):
    return sorted(json.dumps(r, sort_keys=True) for r in rows)


def main():
    config = json.loads(Path(sys.argv[1]).read_text())
    directory = Path(config["out"]) / "loader"
    directory.mkdir(mode=0o700)
    store = directory / "secrets"
    store.mkdir(mode=0o700)
    private(store / "source-password", Path(config["reader_password"]).read_text())
    private(store / "source-ca", Path(config["ca"]).read_text())
    private(store / "target-password", secrets.token_hex(24))
    name = "oo-c-target-" + secrets.token_hex(6)
    label = Path(config["out"]).parent.name
    result = {"scope": "static snapshot only; no concurrent writer or T8",
              "tables": {}, "typed_snapshot": "not_run", "cleanup": "not_created"}
    created = False

    def sql(statement):
        return run(["docker", "exec", "-i", name, "psql", "-X", "-U", "postgres",
                    "-v", "ON_ERROR_STOP=1", "-A", "-t"],
                   input=statement.encode()).decode().strip()

    def load(table):
        command = [str(ROOT / "target/debug/oo-load"), "--stack", str(directory / "stack.json"),
                   "--source", "fixture", "--target", str(directory / "target.json"),
                   "--secrets-dir", str(store), "--tables", "fixture." + table,
                   "--chunk-rows", "10"]
        process = subprocess.run(command, capture_output=True, timeout=180)
        (directory / (table + ".stdout")).write_bytes(process.stdout)
        (directory / (table + ".stderr")).write_bytes(process.stderr)
        return process

    try:
        preflight("mariadb")
        pins = json.loads((ROOT / "testing/infra/images.lock.json").read_text())
        pin = next(p for p in pins["images"] if p["id"] == "postgres-17.11-bookworm")
        image = pin["ref"] + "@" + pin["platforms"]["linux/arm64"]["digest"]
        pulled = subprocess.run(["docker", "pull", "--platform", "linux/arm64", image],
                                capture_output=True, timeout=300)
        (directory / "pull.log").write_bytes(pulled.stdout + pulled.stderr)
        if pulled.returncode:
            raise RuntimeError("target image pull failed")
        preflight("mariadb")
        created = True
        run(["docker", "run", "-d", "--name", name, "--label", "oo.lane=c",
             "--label", "oo.run_id=" + label, "--platform", "linux/arm64",
             "-p", "127.0.0.1::5432", "--tmpfs", "/var/lib/postgresql/data:rw,size=512m",
             "--mount", f"type=bind,source={store},target=/fixture,readonly",
             "-e", "POSTGRES_PASSWORD_FILE=/fixture/target-password", image])
        for _ in range(90):
            ready = subprocess.run(["docker", "exec", name, "pg_isready", "-U", "postgres"],
                                   capture_output=True, timeout=15)
            if ready.returncode == 0:
                break
            time.sleep(1)
        else:
            raise RuntimeError("target readiness timeout")
        port = int(run(["docker", "port", name, "5432/tcp"]).decode().strip().rsplit(":", 1)[1])
        stack = {
            "version": "oo.stack/v1",
            "sources": [{"id": "fixture", "engine": config["family"], "host": "127.0.0.1",
                         "port": config["port"], "database": "fixture",
                         "user": "reader@test" if config["family"] == "oceanbase" else "reader",
                         "password": {"secret_ref": "source-password"},
                         "tls_ca": {"secret_ref": "source-ca"},
                         "scope": {"include": ["fixture.*"]}}],
            "model": {"provider": "openai-compatible", "base_url": "https://example.invalid/v1",
                      "model": "unused", "key": {"secret_ref": "unused"}, "context_window": "auto"},
        }
        (directory / "stack.json").write_text(json.dumps(stack))
        (directory / "target.json").write_text(json.dumps({
            "host": "127.0.0.1", "port": port, "database": "postgres", "user": "postgres",
            "password": {"secret_ref": "target-password"}, "local_fixture": True,
            "run_id": "family-static-snapshot",
        }))
        for table, gold in expected().items():
            process = load(table)
            if process.returncode:
                raise RuntimeError("snapshot failed: " + table)
            receipt = json.loads(process.stdout)
            relation = receipt["target"]
            if not re.fullmatch(r"oo_load_data\.[a-z0-9_]+", relation):
                raise RuntimeError("invalid target identifier")
            actual = json.loads(sql(
                f"SELECT COALESCE(jsonb_agg(to_jsonb(t)), '[]'::jsonb) FROM {relation} t;"))
            if canonical(actual) != canonical(gold):
                raise RuntimeError("target differs: " + table)
            result["tables"][table] = {"rows": len(gold), "differences": 0,
                                       "target_write_bytes": receipt["target_write_bytes"]}
        # Do not exclude the duration column to turn unsupported typing into a pass.
        process = load("typed_rows")
        result["typed_snapshot"] = {
            "exit": process.returncode,
            "error": json.loads(process.stderr) if process.returncode else None,
        }
        if process.returncode:
            raise RuntimeError("typed snapshot failed; inspect typed_rows.stderr")
        relation = json.loads(process.stdout)["target"]
        if not re.fullmatch(r"oo_load_data\.[a-z0-9_]+", relation):
            raise RuntimeError("invalid target identifier")
        actual = json.loads(sql(
            "SELECT jsonb_agg(to_jsonb(t)) FROM (SELECT id,amount::text,big::text,"
            "doc::jsonb,encode(raw,'hex') AS raw,"
            f"(extract(epoch FROM elapsed)*1000000)::bigint AS elapsed FROM {relation}) t;"))
        gold = [
            {"id": 1, "amount": "12345678901234567890.12345678",
             "big": "18446744073709551615", "doc": {"a": 1}, "raw": "00ff",
             "elapsed": -97323123456},
            {"id": 2, "amount": None, "big": "0", "doc": None, "raw": "", "elapsed": 0},
        ]
        if canonical(actual) != canonical(gold):
            raise RuntimeError("target differs: typed_rows")
        result["tables"]["typed_rows"] = {"rows": 2, "differences": 0}
        result["status"] = "passed"
    except Exception as error:
        result["status"] = "failed"
        result["error"] = str(error) if isinstance(error, RuntimeError) else type(error).__name__
    finally:
        if created:
            try:
                cleanup = subprocess.run(["docker", "rm", "-fv", name], capture_output=True, timeout=30)
                (directory / "cleanup.log").write_bytes(cleanup.stdout + cleanup.stderr)
                result["cleanup"] = "removed" if cleanup.returncode == 0 else name
            except subprocess.TimeoutExpired:
                result["cleanup"] = name
        (directory / "result.json").write_text(json.dumps(result, indent=2))
    return int(result["status"] != "passed" or result["cleanup"] not in ("removed", "not_created"))


if __name__ == "__main__":
    raise SystemExit(main())
