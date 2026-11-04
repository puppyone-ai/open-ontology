"""Nightly evidence contract shared with the coordinator's matrix generator."""
import hashlib
import json

P0 = [("mysql", v) for v in ("5.7", "8.0", "8.4", "9.x")]
P0 += [("postgres", str(v)) for v in range(13, 19)]
P0 += [("oracle", v) for v in ("19c", "23ai", "11g", "21c")]


def target_id(engine, version):
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
        engine, version = result["engine"], result["version"]
        path = root / f"{engine}-{version}"
        digest = hashlib.sha256((path / "SHA256SUMS").read_bytes()).hexdigest()
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
            rows.append({
                "target": target_id(engine, version),
                "version": receipt.get("version_actual", result.get("version_locked", version)),
                "version_measured": "version_actual" in receipt,
                "capability": {"T0": "fixture", "T1": "snapshot", "T2": "incremental",
                               "T3": "recovery", "T4": "schema_change", "T5": "selfbuild",
                               "T6": "dialogue", "T7": "read_only", "T8": "budget"}[suite],
                "suite": suite, "variant": result.get("variant") if suite in ("T2", "T3", "T7") else None,
                "result": outcome,
                "run_id": report["run_id"], "sha256": digest,
                "evidence": f"{engine}-{version}/SHA256SUMS",
                "emulated": receipt.get("execution") == "emulated",
                "detail": status, "error": result.get("error"),
            })
            # Missing hooks must invalidate both paths, not leave old CDC/digest
            # passes authoritative merely because no variant was supplied.
            if suite in ("T2", "T3", "T7") and outcome != "pass" and not result.get("variant"):
                row = rows.pop()
                rows.extend(dict(row, variant=variant) for variant in ("c1", "c5"))
    return rows


def downgrade_report(rows):
    return {
        "schema": "oo-nightly-downgrade/v1",
        "authority": "coordinator must ingest evidence; no shared matrix edited",
        "owner_notification": "local report and CI failure summary; no external recipient configured",
        "failures": [row for row in rows if row["result"] != "pass"],
        "stable": bool(rows) and all(row["result"] == "pass" for row in rows),
    }
