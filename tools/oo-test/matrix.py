"""Nightly evidence contract shared with the coordinator's matrix generator."""
import hashlib
import json
from datetime import datetime, timezone

P0 = [("mysql", v) for v in ("5.7", "8.0", "8.4", "9.x")]
P0 += [("postgres", str(v)) for v in range(13, 19)]
P0 += [("oracle", v) for v in ("19c", "23ai", "11g", "21c")]


def target_id(engine, version):
    if engine == "db2":
        return "db2-luw-11_5"
    if engine == "mongodb":
        return "mongodb"
    if engine in ("opensearch", "elasticsearch"):
        return "elasticsearch-opensearch"
    if engine == "sqlserver":
        return "sqlserver-selfmanaged"
    if engine == "postgres":
        return "postgres-selfmanaged"
    if engine == "mysql":
        return ("mysql57" if version.startswith("5.") else
                "mysql9" if version.startswith("9") else "mysql8") + "-selfmanaged"
    family = {"19": "19c-selfmanaged", "23": "23ai-selfmanaged",
              "11": "11g-xe", "21": "21c-xe"}
    return "oracle-" + family[version[:2]]


def index_rows(root, report, requested):
    rows = []
    for result in report["results"]:
        first = len(rows)
        engine, version = result["engine"], result["version"]
        path = root / f"{engine}-{version}"
        raw = path / "comparison-report.json"
        digest = hashlib.sha256(raw.read_bytes() if raw.exists() else (path / "SHA256SUMS").read_bytes()).hexdigest()
        receipt_path = path / "resource-receipt.json"
        receipt = json.loads(receipt_path.read_text()) if receipt_path.exists() else {}
        for suite in requested:
            status = result["suites"].get(suite, "not_run")
            # Fixture precheck is NOT product doctor. Never promote full from it.
            product = suite != "T0" or result.get("product_t0", False)
            outcome = ("pass" if status == "passed" and product else
                       "skip" if status == "passed" else "fail")
            if receipt.get("cleanup_failed"):
                outcome = "fail"
            if outcome == "pass" and "version_actual" not in receipt:
                outcome = "fail"
            rows.append({
                "target": target_id(engine, version),
                "version": receipt.get("version_actual", result.get("version_locked", version)),
                "version_measured": "version_actual" in receipt,
                "capability": {"T0": "fixture", "T1": "snapshot", "T2": "incremental",
                               "T3": "recovery", "T4": "schema_change", "T5": "selfbuild",
                               "T6": "dialogue", "T7": "read_only", "T8": "budget"}[suite],
                "suite": suite, "variant": (result.get("variant") or "snapshot") if suite in ("T2", "T3", "T7") else "snapshot",
                "result": outcome,
                "run_id": report["run_id"], "sha256": digest,
                "evidence": raw.name if raw.exists() else "SHA256SUMS",
                "observed_at": result.get("observed_at") or datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
                "emulated": receipt.get("execution") == "emulated",
                "detail": status, "error": result.get("error"),
            })
            if engine == "sqlserver" and outcome == "pass" and suite in ("T2", "T7"):
                row = rows.pop()
                # The built-in SQL Server suite explicitly runs both T2 paths;
                # its separate T7 audit covers CDC, not C5.
                variants = ("c1", "c5") if suite == "T2" else ("c1",)
                rows.extend(dict(row, variant=v) for v in variants)
            # Missing hooks must invalidate both paths, not leave old CDC/digest
            # passes authoritative merely because no variant was supplied.
            if suite in ("T2", "T3", "T7") and outcome != "pass" and not result.get("variant"):
                row = rows.pop()
                rows.extend(dict(row, variant=variant) for variant in ("c1", "c5"))
        (path / "index-rows.json").write_text(json.dumps(rows[first:], indent=2) + "\n")
    return rows


def downgrade_report(rows):
    return {
        "schema": "oo-nightly-downgrade/v1",
        "authority": "coordinator must ingest evidence; no shared matrix edited",
        "owner_notification": "local report and CI failure summary; no external recipient configured",
        "failures": [row for row in rows if row["result"] != "pass"],
        "stable": bool(rows) and all(row["result"] == "pass" for row in rows),
    }
