"""Lane-E fixture adapter to the lane-H product T0 validator."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import subprocess
import sys
from types import SimpleNamespace

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tools/oo-test"))
from t0 import Replay, private, validate
from verdicts import write_rows


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--attempt", default="product-t0")
    args = parser.parse_args()
    if not all(re.fullmatch(r"[a-z0-9-]{1,64}", v) for v in (args.run_id, args.attempt)):
        parser.error("run-id")
    base = ROOT / "evidence" / args.run_id
    owned = json.loads((base / "fixture-owned.json").read_text())["container"]
    labels = json.loads(subprocess.check_output(
        ["docker", "inspect", "--format", "{{json .Config.Labels}}", owned], text=True))
    if labels.get("oo.lane") != "e" or labels.get("oo.run_id") != args.run_id:
        raise ValueError("fixture ownership")
    out = base / args.attempt
    out.mkdir()
    home = out / ".private"
    store = home / ".config/open-ontology/secrets"
    store.mkdir(parents=True, mode=0o700)
    password = (base / ".private/reader").read_text()
    model_key = secrets.token_hex(24)
    private(store / "reader", password)
    private(store / "model", model_key)
    tables = ["FIXTURE.EVENTS", "FIXTURE.COMPOSITE"]
    report = {"T0": "fail", "modes": {}, "version_actual": None,
              "validator_sha256": hashlib.sha256((ROOT / "tools/oo-test/t0.py").read_bytes()).hexdigest(),
              "binary_sha256": hashlib.sha256(args.binary.read_bytes()).hexdigest(),
              "model": "local synthetic replay, not live model acceptance"}
    replay = Replay()
    try:
        source = {"id": "fixture", "engine": "db2", "host": "127.0.0.1",
                  "port": 50000, "database": "FIXTURE", "user": "ooreader",
                  "password": {"secret_ref": "reader"}, "scope": {"include": tables}}
        stack = {"version": "oo.stack/v1", "sources": [source],
                 "model": {"provider": "openai-compatible", "model": "fixture-model",
                           "base_url": f"http://127.0.0.1:{replay.httpd.server_port}/v1",
                           "key": {"secret_ref": "model"}}}
        for mode in ("auto", "C5", "C1"):
            source["sync"] = {"mode": mode}
            config = home / "stack.json"
            private(config, json.dumps(stack))
            process = subprocess.run(
                [str(args.binary.resolve()), "doctor", "--config", str(config), "--json"],
                env=dict(os.environ, HOME=str(home), OO_DB2_WORKER=str(base / "worker")),
                capture_output=True, text=True, timeout=300)
            for suffix, text in (("stdout", process.stdout), ("stderr", process.stderr)):
                (out / f"{mode}.{suffix}").write_text(
                    text.replace(password, "[redacted]").replace(model_key, "[redacted]"))
            (out / f"{mode}.exit").write_text(str(process.returncode))
            data = json.loads(process.stdout)
            report["modes"][mode] = validate(data, process.returncode, "11.5.9.0",
                                            "db2", mode, tables)
            report["version_actual"] = data["sources"][0]["probe"]["version"]["raw"]
        # Probe a separate, ungranted table; never revoke a running reader's grant.
        sql = "CONNECT TO FIXTURE;\nCREATE TABLE FIXTURE.T0_DENIED (ID INTEGER);\n"
        subprocess.run(["docker", "exec", "-i", owned, "su", "-", "db2inst1", "-c",
                        "db2 +p -st"], input=sql, text=True, capture_output=True, check=True)
        source["scope"]["include"] = ["FIXTURE.T0_DENIED"]
        source["sync"] = {"mode": "auto"}
        private(config, json.dumps(stack))
        denied = subprocess.run(
            [str(args.binary.resolve()), "doctor", "--config", str(config), "--json"],
            env=dict(os.environ, HOME=str(home), OO_DB2_WORKER=str(base / "worker")),
            capture_output=True, text=True, timeout=300)
        (out / "denied.stdout").write_text(denied.stdout.replace(password, "[redacted]").replace(model_key, "[redacted]"))
        data = json.loads(denied.stdout)
        requirements = data["sources"][0]["dba_requirements"]
        if denied.returncode != 1 or not any(
                r["kind"] == "privilege" and "GRANT SELECT" in r.get("dba_statement", "")
                for r in requirements):
            raise ValueError("missing actionable SELECT advice")
        report.update(T0="pass", missing_select_advice=True)
    except Exception as error:
        report["error"] = type(error).__name__ + ":" + str(error).replace(password, "[redacted]").replace(model_key, "[redacted]")
    finally:
        replay.close()
        raw = out / "doctor-report.json"
        raw.write_text(json.dumps(report, indent=2))
        run = SimpleNamespace(engine="db2", version="11.5", id=args.run_id,
                              receipt={"version_actual": report["version_actual"], "execution": "emulated"})
        if report["version_actual"]:
            write_rows(run, raw, [("T0", "snapshot", report["T0"])])
        else:
            from datetime import datetime, timezone
            from verdicts import product_sha
            (out / "index-rows.json").write_text(json.dumps([{
                "target": "db2-luw-11_5", "version": "11.5",
                "suite": "T0", "variant": "snapshot", "result": "fail",
                "version_measured": False, "emulated": True, "run_id": args.run_id,
                "sha256": hashlib.sha256(raw.read_bytes()).hexdigest(), "evidence": raw.name,
                "observed_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
                "product_sha": product_sha(),
            }], indent=2))
    return 0 if report["T0"] == "pass" else 1


if __name__ == "__main__":
    raise SystemExit(main())
