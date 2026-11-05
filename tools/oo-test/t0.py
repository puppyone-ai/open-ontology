#!/usr/bin/env python3
"""Product doctor hook for owned P0 fixtures; no live model or owner secrets."""
import hashlib
import json
import os
from pathlib import Path
import secrets
import shutil
import subprocess
import sys
import time

import runner
from t1 import LaneRun, configure_mysql, configure_pg, private
from verdicts import write_rows

ROOT = runner.ROOT
sys.path.insert(0, str(ROOT / "tools/model-egress/tests"))
from test_egress import Replay


def validate(report, code, version, engine, mode, tables):
    """A successful process alone is not evidence of usable diagnostics."""
    if report.get("protocol") != "oo.doctor/v1" or report.get("read_only") is not True:
        raise ValueError("doctor protocol/read-only missing")
    if report.get("model", {}).get("status") != "ok":
        raise ValueError("local model replay failed")
    sources = report.get("sources", [])
    if len(sources) != 1:
        raise ValueError("expected one fixture source")
    source = sources[0]
    if source.get("error"):
        raise ValueError("source probe failed: " + str(source["error"]))
    probe = source["probe"]
    actual = probe["version"]
    expected = ([int(v) for v in version.split(".")] + [0, 0])[:3]
    if [actual[k] for k in ("major", "minor", "patch")] != expected:
        raise ValueError("doctor version differs from measured fixture")
    if probe["family"] != engine or not isinstance(probe["settings"], dict):
        raise ValueError("wrong engine or missing log settings")
    caps = probe["capabilities"]
    for name in ("catalog", "chunked_snapshot", "digest_diff", "log_cdc"):
        if caps[name]["state"] not in ("available", "needs_setup", "unsupported"):
            raise ValueError("invalid capability state")
    requirements = source["dba_requirements"]
    if not isinstance(requirements, list):
        raise ValueError("missing DBA requirements")
    for requirement in requirements:
        if requirement["kind"] == "privilege" and not requirement.get("dba_statement"):
            raise ValueError("missing executable privilege advice")
    plans = source["chunk_plans"]
    if sorted(f'{p["table"]["schema"]}.{p["table"]["name"]}' for p in plans) != sorted(tables):
        raise ValueError("incomplete chunk plans")
    for plan in plans:
        if "error" in plan or "estimate_error" in plan:
            raise ValueError("chunk planning failed")
        if not {"strategy", "chunks", "estimated_rows", "estimated_bytes", "bytes_basis"} <= plan.keys():
            raise ValueError("incomplete budget estimates")
    needed = ["chunked_snapshot"]
    if mode == "C1":
        needed.append("log_cdc")
    if mode == "C5":
        needed.append("digest_diff")
    ready = all(caps[c]["state"] == "available" for c in needed)
    expected_status = "ok" if ready else "blocked"
    if (source.get("status"), report.get("status"), code) != (
            expected_status, expected_status, 0 if ready else 1):
        raise ValueError("sync readiness disagrees with doctor status/exit")
    if mode != "C1" and not ready:
        raise ValueError("snapshot/C5 fixture not ready")
    return {"ready": ready, "status": expected_status, "capabilities": caps,
            "dba_requirements": requirements}


def worker_environment(run, home, image):
    """Relabel only doctor's exact worker invocation, without altering its IO."""
    if not image.startswith("sha256:") or len(image) != 71:
        raise ValueError("Oracle worker content ID required")
    docker = shutil.which("docker")
    name = "oo-h-doctor-" + secrets.token_hex(8)
    expected = ["run", "--rm", "-i", "--platform", "linux/amd64", "--label",
                "oo.lane=a", "--cap-drop", "ALL", "--security-opt",
                "no-new-privileges", image]
    owned = expected[:-1] + ["--name", name, "--label", "oo.run_id=" + run.id, image]
    owned[6] = "oo.lane=h"
    shim = home / "bin"
    shim.mkdir(exist_ok=True)
    script = shim / "docker"
    private(script, f"#!{sys.executable}\nimport os, sys\n"
            f"if sys.argv[1:] != {expected!r}:\n    sys.exit(125)\n"
            f"os.execv({docker!r}, [{docker!r}] + {owned!r})\n")
    script.chmod(0o700)
    return name, {"PATH": str(shim) + os.pathsep + os.environ["PATH"],
                  "OO_ORACLE_WORKER_IMAGE": image}


def main():
    context = json.loads(Path(os.environ["OO_TEST_CONTEXT"]).read_text())
    labels = json.loads(subprocess.check_output(
        ["docker", "inspect", "--format", "{{json .Config.Labels}}", context["container"]],
        text=True))
    lane = "E19" if context["engine"] == "oracle" and context["version"].startswith("19") else "h"
    if labels.get("oo.lane") != lane or labels.get("oo.run_id") != context["run_id"]:
        raise ValueError("T0 fixture ownership")
    run = LaneRun.__new__(LaneRun)
    run.name, run.id = context["container"], context["run_id"]
    run.engine, run.version = context["engine"], context["version"]
    run.path = Path(context["evidence"]) / "product-t0"
    run.path.mkdir(mode=0o700)
    run.seq, run.secret = 0, secrets.token_hex(24)
    run.reader_secret = Path(context["reader_password_file"]).read_text().strip()
    run.receipt = json.loads((Path(context["evidence"]) / "resource-receipt.json").read_text())
    home = run.path / ".doctor-private"
    store = home / ".config/open-ontology/secrets"
    store.mkdir(parents=True, mode=0o700)
    replay, workers = None, []
    report = {"T0": "failed", "model": "local synthetic replay; no live model acceptance",
              "scope": "events/order_lines planning on the seeded fixture", "modes": {},
              "hook_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
    try:
        binary = ROOT / "target/debug/oo"
        report["binary_sha256"] = hashlib.sha256(binary.read_bytes()).hexdigest()
        if run.engine == "mysql":
            helper = binary.with_name("oo-mysql-doctor")
            report["helper_sha256"] = hashlib.sha256(helper.read_bytes()).hexdigest()
            configure_mysql(run, store)
        elif run.engine == "postgres":
            configure_pg(run, store)
        elif run.engine == "oracle":
            run.sql("GRANT FLASHBACK ON fixture.events TO fixture_reader;\n"
                    "GRANT FLASHBACK ON fixture.order_lines TO fixture_reader;\n", admin=True)
        else:
            raise ValueError("T0 hook engine unavailable")
        port = {"postgres": 5432, "mysql": 3306, "oracle": 1521}[run.engine]
        _, address = run.command(["docker", "port", run.name, f"{port}/tcp"])
        schema = {"postgres": "public", "mysql": "fixture", "oracle": "FIXTURE"}[run.engine]
        tables = [schema + "." + (t.upper() if run.engine == "oracle" else t)
                  for t in ("events", "order_lines")]
        service = "fixture" if run.engine != "oracle" else (
            "XE" if run.version.startswith("11") else
            "XEPDB1" if run.version.startswith("21") else "FREEPDB1")
        source = {"id": "fixture", "engine": run.engine, "host": "127.0.0.1",
                  "port": int(address.strip().rsplit(":", 1)[1]), "database": service,
                  "user": context["reader"], "password": {"secret_ref": "reader"},
                  "scope": {"include": tables}}
        if run.engine != "oracle":
            source["tls_ca"] = {"secret_ref": "source-ca"}
        env = dict(os.environ, HOME=str(home))
        env.pop("OO_ORACLE_WORKER_IMAGE", None)
        if run.engine == "oracle" and run.version.startswith("11"):
            source["host"] = "host.docker.internal"
            report["worker_image"] = os.environ["OO_TEST_ORACLE_WORKER_IMAGE"]
        private(store / "reader", run.reader_secret)
        model_key = secrets.token_hex(24)
        private(store / "model", model_key)
        replay = Replay()
        stack = {"version": "oo.stack/v1", "sources": [source],
                 "model": {"provider": "openai-compatible", "model": "fixture-model",
                           "base_url": f"http://127.0.0.1:{replay.httpd.server_port}/v1",
                           "key": {"secret_ref": "model"}}}
        config = home / "stack.json"
        for mode in ("auto", "C5", "C1"):
            if run.engine == "oracle" and run.version.startswith("11"):
                worker, extra = worker_environment(run, home, os.environ["OO_TEST_ORACLE_WORKER_IMAGE"])
                workers.append(worker)
                env.update(extra)
            source["sync"] = {"mode": mode}
            private(config, json.dumps(stack))
            started = time.monotonic()
            process = subprocess.run(
                [str(binary), "doctor", "--config", str(config), "--json"],
                env=env, capture_output=True, text=True, timeout=300)
            for suffix, text in (("stdout", process.stdout), ("stderr", process.stderr)):
                (run.path / f"{mode}.{suffix}").write_text(
                    runner.redact(text, run.secret, run.reader_secret, model_key))
            runner.write_json(run.path / f"{mode}.exit.json", {
                "exit": process.returncode, "elapsed_seconds": time.monotonic() - started})
            try:
                report["modes"][mode] = validate(json.loads(process.stdout), process.returncode,
                                                run.receipt["version_actual"], run.engine, mode, tables)
            except (ValueError, KeyError, TypeError) as error:
                report["modes"][mode] = {"error": str(error)}
        if any("error" in value for value in report["modes"].values()):
            raise ValueError("one or more doctor modes failed; see raw reports")
        if not any(r["kind"] == "privilege" for r in report["modes"]["C1"]["dba_requirements"]):
            raise ValueError("fixture did not exercise missing privilege diagnostics")
        report["T0"] = "passed"
    except Exception as error:
        report["error"] = type(error).__name__ + ": " + runner.redact(
            str(error), run.secret, run.reader_secret)
    finally:
        if replay:
            replay.close()
        for worker in workers:
            code, names = run.command(["docker", "ps", "-a", "--filter", "name=^/" + worker + "$",
                                       "--format", "{{.Names}}"], required=False)
            if not code and names.strip():
                code, raw = run.command(["docker", "inspect", "--format",
                                         "{{json .Config.Labels}}", worker], required=False)
                if code:
                    report.update(T0="failed", cleanup_failed=True)
                    continue
                labels = json.loads(raw)
                if labels.get("oo.lane") == "h" and labels.get("oo.run_id") == run.id:
                    code, _ = run.command(["docker", "rm", "-f", worker], required=False)
                else:
                    code = 1
            if code:
                report.update(T0="failed", cleanup_failed=True)
        shutil.rmtree(home)
        raw = run.path / "doctor-report.json"
        runner.write_json(raw, report)
        write_rows(run, raw, [("T0", "snapshot", "pass" if report["T0"] == "passed" else "fail")])
        runner.checksums(run.path)
    return 0 if report["T0"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
