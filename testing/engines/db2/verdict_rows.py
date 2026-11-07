"""Emit explicit Db2 rows without the shared mapper's Oracle fallback.

Product T0 remains skipped until the coordinator integrates oo doctor.
The SourceEngine probe test is useful evidence, not a substitute for T0.
"""
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path


def write(run_id, raw, *, cleanup_ok):
    raw = Path(raw)
    report = json.loads(raw.read_text())
    if report["version_actual"] != "11.5.9.0":
        raise ValueError("unmeasured Db2 version")
    full = report["scope"] == "231 tables / 2M events / 300 columns"
    outcome = lambda suite: "pass" if full and cleanup_ok and report[suite] == "pass" else "fail"
    digest = hashlib.sha256(raw.read_bytes()).hexdigest()
    rows = [{
        "target": "db2-luw-11_5", "version": report["version_actual"],
        "suite": suite, "variant": "snapshot", "result": result,
        "run_id": run_id, "sha256": digest, "evidence": raw.name,
        "emulated": True, "version_measured": True,
        "observed_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "scope": report["scope"],
    } for suite, result in [("T0", "skip"), ("T1", outcome("T1")), ("T8", outcome("T8"))]]
    (raw.parent / "index-rows.json").write_text(json.dumps(rows, indent=2) + "\n")
    return rows
