"""Explicit suite verdicts, bound to raw results rather than process exits."""
from datetime import datetime, timezone
import hashlib
import json
import re

from matrix import target_id


def write_rows(run, raw, verdicts, *, cleanup_failed=False):
    """verdicts contains (suite, variant, pass|fail|skip) from the suite owner."""
    receipt = getattr(run, "receipt", {})
    if any(s not in {f"T{i}" for i in range(9)} or v not in ("snapshot", "c1", "c5")
           or r not in ("pass", "fail", "skip") for s, v, r in verdicts):
        raise ValueError("invalid suite verdict")
    version = receipt.get("version_actual")
    if not version and run.engine in ("mongodb", "opensearch", "elasticsearch"):
        version = run.version
    if not version:
        import runner
        version = re.search(r"\d+(?:\.\d+)*", runner.resolve(run.engine, run.version)["id"]).group()
    digest = hashlib.sha256(raw.read_bytes()).hexdigest()
    rows = [{
        "target": target_id(run.engine, run.version), "version": version,
        "suite": suite, "variant": variant,
        "result": "fail" if cleanup_failed or (result == "pass" and not receipt.get("version_actual")) else result,
        "run_id": run.id, "sha256": digest, "evidence": raw.name,
        "emulated": receipt.get("execution") == "emulated",
        "version_measured": bool(receipt.get("version_actual")),
        "observed_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    } for suite, variant, result in verdicts]
    path = raw.parent / "index-rows.json"
    old = json.loads(path.read_text()) if path.exists() else []
    keys = {(r["suite"], r["variant"]) for r in rows}
    old = [r for r in old if (r["suite"], r["variant"]) not in keys]
    path.write_text(json.dumps(old + rows, indent=2) + "\n")
    return rows


def status(value):
    return "pass" if value == "passed" else "fail"


def incremental(report):
    """A failed overall exit (including negative checks) invalidates both paths."""
    rows = []
    for mode, cycles in (("C1", report.get("cycles", [])),
                         ("C5", report.get("c5_cycles", report.get("cycles", [])))):
        outcome = status(report.get(mode)) if report.get("T2") == "passed" else "fail"
        rows.append(("T2", mode.lower(), outcome))
        samples = [c["T8"] for c in cycles if "T8" in c and c.get("mode", mode) == mode]
        rows.append(("T8", mode.lower(), "pass" if outcome == "pass" and len(samples) >= 3
                     and all(m.get("status") == "passed" for m in samples) else "fail"))
    return rows


def merge(root):
    """Validate raw hashes before admitting rows. Missing rows never become passes."""
    rows = []
    for path in sorted(root.rglob("index-rows.json")):
        for row in json.loads(path.read_text()):
            if any(not row.get(k) for k in ("target", "version", "suite", "run_id", "sha256", "evidence")):
                raise ValueError("missing evidence identity")
            if row["suite"] not in {f"T{i}" for i in range(9)}:
                raise ValueError("invalid suite")
            if row.get("result") not in ("pass", "fail", "skip"):
                raise ValueError("invalid explicit verdict")
            raw = (path.parent / row["evidence"]).resolve()
            if not raw.is_relative_to(root.resolve()) or not raw.is_file():
                raise ValueError("raw evidence outside run or missing")
            if hashlib.sha256(raw.read_bytes()).hexdigest() != row["sha256"]:
                raise ValueError("raw evidence digest mismatch")
            if not row.get("observed_at") or row.get("variant") not in ("snapshot", "c1", "c5"):
                raise ValueError("missing UTC observation or capability variant")
            observed = datetime.fromisoformat(row["observed_at"].replace("Z", "+00:00"))
            if observed.utcoffset() != timezone.utc.utcoffset(observed):
                raise ValueError("observation must be UTC")
            if row["result"] == "pass" and row.get("version_measured") is not True:
                raise ValueError("pass requires a measured version")
            rows.append(dict(row, evidence=str(raw.relative_to(root.resolve()))))
    return rows
