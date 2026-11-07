#!/usr/bin/env python3
"""DM8 product T0: the real `oo doctor` judged by lane H's t0.validate.

Runs auto, C5 and C1 modes against the run's fixture through its DPI worker
wrapper, then probes a separate ungranted table for actionable SELECT advice.
The model endpoint is a local synthetic replay, not live model acceptance.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import subprocess
import sys
from types import SimpleNamespace

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from admin import ROOT, Admin, run_dir  # noqa: E402

sys.path.insert(0, str(ROOT / "tools/oo-test"))
from t0 import Replay, private, validate  # noqa: E402
from verdicts import product_sha, write_rows  # noqa: E402

TABLES = ["FIXTURE.EVENTS", "FIXTURE.TYPES", "FIXTURE.COMPOSITE", "FIXTURE.NO_KEY"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--attempt", default="product-t0")
    args = parser.parse_args()
    if not re.fullmatch(r"[a-z0-9-]{1,64}", args.attempt):
        parser.error("attempt")
    base = run_dir(args.run_id)
    admin = Admin(args.run_id)
    out = base / args.attempt
    out.mkdir()
    home = out / ".private"
    store = home / ".config/open-ontology/secrets"
    store.mkdir(parents=True, mode=0o700)
    password = (base / ".private/reader").read_text()
    model_key = secrets.token_hex(24)
    private(store / "reader", password)
    private(store / "model", model_key)
    redact = lambda text: admin.redact(text).replace(password, "[redacted]").replace(
        model_key, "[redacted]")
    report = {"T0": "fail", "modes": {}, "version_actual": None,
              "validator_sha256": hashlib.sha256((ROOT / "tools/oo-test/t0.py").read_bytes()).hexdigest(),
              "binary_sha256": hashlib.sha256(args.binary.read_bytes()).hexdigest(),
              "model": "local synthetic replay, not live model acceptance"}
    replay = Replay()
    env = dict(os.environ, HOME=str(home), OO_DM8_WORKER=str(base / "worker"))
    source = {"id": "fixture", "engine": "dm8", "host": "127.0.0.1", "port": 5236,
              "database": "DAMENG", "user": "OOREADER",
              "password": {"secret_ref": "reader"}, "scope": {"include": list(TABLES)}}
    stack = {"version": "oo.stack/v1", "sources": [source],
             "model": {"provider": "openai-compatible", "model": "fixture-model",
                       "base_url": f"http://127.0.0.1:{replay.httpd.server_port}/v1",
                       "key": {"secret_ref": "model"}}}
    config = home / "stack.json"

    def doctor(label):
        if config.exists():
            config.unlink()
        private(config, json.dumps(stack))
        process = subprocess.run(
            [str(args.binary.resolve()), "doctor", "--config", str(config), "--json"],
            env=env, capture_output=True, text=True, timeout=300)
        for suffix, text in (("stdout", process.stdout), ("stderr", process.stderr)):
            (out / f"{label}.{suffix}").write_text(redact(text))
        (out / f"{label}.exit").write_text(str(process.returncode))
        return json.loads(process.stdout), process.returncode

    try:
        for mode in ("auto", "C5", "C1"):
            source["sync"] = {"mode": mode}
            data, code = doctor(mode)
            report["modes"][mode] = validate(data, code, "8", "dameng", mode, TABLES)
            probe = data["sources"][0]["probe"]
            report["version_actual"] = str(probe["version"]["major"])
            report["version_raw"] = probe["version"]["raw"]
            report["settings"] = probe["settings"]
            report["chunk_plans"] = data["sources"][0]["chunk_plans"]
        if not re.fullmatch(r"DM Database Server \S+ V8 \d{11}-\d{8}-\d+-\d+", report["version_raw"]):
            raise ValueError("server build not measured")
        # A separate, ungranted table; a running reader's grant is never revoked.
        admin.sql("CREATE TABLE FIXTURE.T0_DENIED (ID INT);")
        source["scope"]["include"] = ["FIXTURE.T0_DENIED"]
        source["sync"] = {"mode": "auto"}
        data, code = doctor("denied")
        requirements = data["sources"][0]["dba_requirements"]
        if code != 1 or not any(
                r["kind"] == "privilege" and "GRANT SELECT ON \"FIXTURE\".\"T0_DENIED\" TO \"OOREADER\""
                == r.get("dba_statement") for r in requirements):
            raise ValueError("missing actionable SELECT advice")
        report.update(T0="pass", missing_select_advice=True)
    except Exception as error:
        report["error"] = type(error).__name__ + ":" + redact(str(error))
    finally:
        replay.close()
        try:
            admin.sql("DROP TABLE IF EXISTS FIXTURE.T0_DENIED;")
        except Exception:
            report["T0"] = "fail"
            report["fixture_restore"] = "failed"
        raw = out / "doctor-report.json"
        raw.write_text(json.dumps(report, indent=2))
        run = SimpleNamespace(engine="dm8", version="8", id=args.run_id,
                              receipt={"version_actual": report["version_actual"],
                                       "execution": "native"})
        if report["version_actual"]:
            write_rows(run, raw, [("T0", "snapshot", report["T0"])])
        else:
            (out / "index-rows.json").write_text(json.dumps([{
                "target": "dm8", "version": "8", "suite": "T0", "variant": "snapshot",
                "result": "fail", "version_measured": False, "emulated": False,
                "run_id": args.run_id, "sha256": hashlib.sha256(raw.read_bytes()).hexdigest(),
                "evidence": raw.name, "product_sha": product_sha(),
                "observed_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
            }], indent=2) + "\n")
    return 0 if report["T0"] == "pass" else 1


if __name__ == "__main__":
    raise SystemExit(main())
