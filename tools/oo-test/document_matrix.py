"""Document engine matrix entry; source checks never masquerade as T0-T2."""
import argparse
import json
from pathlib import Path
import subprocess
import sys
import uuid

ROOT = Path(__file__).resolve().parents[2]
VERSIONS = {"mongodb": "8.0.34", "opensearch": "3.9.0", "elasticsearch": "9.2.0"}


def result(engine, returncode, product=None):
    product = product or {}
    suites = {suite: product.get(suite, "not_accepted")
              for suite in ("T0", "T1", "T2", "T8")}
    if engine == "mongodb" and product.get("C1") != "passed":
        suites["T2"] = "not_accepted"
    return {"engine": engine, "version": VERSIONS[engine],
            "source_checks": "passed" if returncode == 0 else "failed",
            "product_t0": False,
            "suites": suites,
            "error": "document_agent_db_acceptance_missing"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["matrix"])
    parser.add_argument("--engine", required=True, choices=VERSIONS)
    parser.add_argument("--versions")
    parser.add_argument("--suites", default="T0-T2")
    parser.add_argument("--run-id", default="f-doc-" + uuid.uuid4().hex[:12])
    parser.add_argument("--platform", choices=["linux/arm64"], default="linux/arm64")
    parser.add_argument("--runner", choices=["local"], default="local")
    args = parser.parse_args()
    if args.versions and args.versions != VERSIONS[args.engine]:
        parser.error("this entry only runs the exact pinned version")
    if args.suites not in ("T0", "T0-T2", "T0,T1,T2,T8"):
        parser.error("only T0/T1/T2/T8 source checks are implemented; product acceptance is blocked")
    completed = subprocess.run([sys.executable, str(ROOT / "testing/engines/document/run.py"),
                                "--engine", args.engine, "--run-id", args.run_id], cwd=ROOT)
    raw = ROOT / "evidence" / args.run_id / args.engine / "summary.json"
    summary = json.loads(raw.read_text()) if raw.exists() else {}
    report = result(args.engine, completed.returncode, summary.get("product"))
    print(json.dumps(report))
    # Product doctor, transactional MongoDB C1 and connection measurement remain
    # missing. Explicit per-suite verdicts are written beside the raw summary.
    return 1
