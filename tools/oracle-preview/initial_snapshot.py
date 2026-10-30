#!/usr/bin/env python3
"""Resume native Oracle AS OF chunks at one measured SCN, never rebaseline silently."""
import argparse
import json
from pathlib import Path
import sys
import time

import bounded_source as b
import buffer_cursor as c
import producer as p

PROTOCOL = "oo.oracle.initial-source-config/v1"
EXPIRED = {"ORA-01555": "undo_expired", "ORA-08180": "scn_unavailable",
           "ORA-08181": "scn_unavailable", "ORA-01466": "definition_boundary",
           "ORA-30052": "invalid_snapshot_boundary", "ORA-01291": "log_gap"}


def scn(value):
    if (not isinstance(value, str) or not value.isascii() or not value.isdigit()
            or str(int(value)) != value or int(value) < 1):
        raise ValueError("source_scn_required_not_ordinal")
    return int(value)


def log_coverage(catalog, lower, upper):
    """Catalog coverage is necessary, but only real LogMiner END closes the handoff."""
    start, end = scn(lower), scn(upper)
    if (catalog.get("protocol") != "oo.oracle.log-catalog/v1"
            or end < start or end > scn(catalog["current_scn"])
            or catalog.get("reader_mutations") != 0):
        raise ValueError("log_catalog_binding")
    ranges = catalog["ranges"]
    if {r["thread"] for r in ranges} != {1}:
        raise ValueError("multi_thread_log_handoff_not_supported")
    cursor, used = start, []
    for item in sorted(ranges, key=lambda r: scn(r["first_scn"])):
        first, last = scn(item["first_scn"]), scn(item["next_scn"])
        if first >= last:
            raise ValueError("log_catalog_range")
        if last <= cursor:
            continue
        if first > cursor:
            break
        used.append(item)
        cursor = last
        if cursor > end:
            return {"covered": True, "required_first_scn": lower, "required_last_scn": upper,
                    "ranges": used, "catalog_only": True, "actual_logminer_end_still_required": True}
    raise ValueError(f"source_log_gap:{cursor}:{end}:restore_logs_or_explicit_rebaseline")


def failure_class(error):
    text = str(error)
    expired = next((code for code in EXPIRED if code in text), None)
    rebaseline = expired is not None or any(token in text for token in (
        "source_log_gap", "resume_binding_drift", "source_identity_mismatch",
        "schema_drift", "source_log_identity", "snapshot_scn_invalid_or_changed",
        "sealed_part_digest_mismatch"))
    return {"error": expired or text, "rebaseline_required": rebaseline,
            "reason_kind": EXPIRED[expired] if expired else "log_gap" if "source_log_gap" in text else "other",
            "resume_same_boundary_allowed": not rebaseline, "fresh_snapshot_fallback": False,
            "operator_action": "restore valid history/logs or start an explicitly new baseline; preserve this attempt"
            if rebaseline else "retry only this same SCN/key/chunk journal when the failure clears"}


def capture(config_path, out, resume=False, stop_after=None, driver_factory=b.Driver):
    config_path, out = Path(config_path).resolve(), Path(out).resolve()
    cfg = json.loads(config_path.read_text())
    if cfg.get("protocol") != PROTOCOL:
        raise ValueError("initial_source_protocol")
    boundary = json.loads(p.pinned(cfg["boundary"]).read_text())
    reader_config = json.loads(p.pinned(cfg["source_config"]).read_text())
    if (boundary["protocol"] != "oo.oracle.snapshot-boundary/v1"
            or boundary["source_id"] != reader_config["source_id"]
            or boundary["identity"] != reader_config["identity"]
            or boundary["table"]["owner"] != reader_config["owner"]):
        raise ValueError("initial_source_identity")
    frozen_scn = boundary["snapshot_scn"]
    scn(frozen_scn)
    for key in ("barrier_evidence", "log_admission"):
        p.pinned(boundary[key])
    admission = json.loads(p.pinned(boundary["log_admission"]).read_text())
    if (admission["source_id"] != boundary["source_id"]
            or admission["table"] != p.table_name(boundary["table"])
            or scn(admission["log_start_scn"]) > scn(frozen_scn)):
        raise ValueError("initial_log_admission")
    for key in ("vendor_offset", "connector_history", "subscription_ready"):
        p.pinned(admission[key])
    out.mkdir(mode=0o700, parents=True, exist_ok=True)
    with b.locked(out):
        state_path = out/"state.json"
        state = json.loads(state_path.read_text()) if state_path.exists() else None
        if state is not None:
            if state["config"] != c.reference(config_path) or state["boundary"] != cfg["boundary"]:
                raise ValueError("initial_resume_config_changed")
            if state.get("rebaseline_required"):
                raise ValueError("explicit_rebaseline_required")
            if not resume:
                raise ValueError("initial_explicit_resume_required")
            if state["status"] == "snapshot_sealed_pending_log_handoff":
                manifest = p.pinned(state["snapshot_manifest"])
                b.load_sealed(manifest.parent, state["snapshot_manifest"]["sha256"])
                return state
        elif resume:
            raise ValueError("initial_resume_state_missing")
        attempt = (state or {}).get("attempt", 0)+1
        evidence = out/("attempt-%04d" % attempt)
        evidence.mkdir(mode=0o700, exist_ok=False)
        state = {"protocol": "oo.oracle.initial-source-state/v1", "config": c.reference(config_path),
                 "boundary": cfg["boundary"], "snapshot_scn": frozen_scn, "attempt": attempt,
                 "source_id": boundary["source_id"], "status": "capturing", "rebaseline_required": False,
                 "source_mutations": 0, "source_offset_from_target_ack": False}
        b.atomic(state_path, b.canonical(state)+b"\n")
        driver = driver_factory(p.pinned(cfg["reader"]), reader_config)
        try:
            inventory = driver.call("inventory")
            if (inventory["identity"] != boundary["identity"] or inventory["tables"] != [boundary["table"]]
                    or scn(inventory["scn"]) < scn(frozen_scn)):
                raise ValueError("source_identity_or_schema_drift")
            catalog = driver.call("log-catalog")
            if catalog["identity"] != boundary["identity"] or catalog["source_id"] != boundary["source_id"]:
                raise ValueError("source_log_identity")
            coverage = log_coverage(catalog, admission["log_start_scn"], inventory["scn"])
            b.atomic(evidence/"catalog.json", b.canonical(catalog)+b"\n")
            b.atomic(evidence/"coverage.json", b.canonical(coverage)+b"\n")
            budgets = cfg["budgets"]
            result = b.capture(driver, out/"snapshot", snapshot_scn=frozen_scn,
                               chunk_rows=budgets["chunk_rows"], chunk_bytes=budgets["chunk_bytes"],
                               budget_rows=budgets["rows"], budget_bytes=budgets["bytes"],
                               deadline_seconds=budgets["seconds"], stop_after=stop_after)
            state["status"] = "snapshot_sealed_pending_log_handoff" if result["complete"] else "interrupted"
            state["snapshot_result"] = result
            if result["complete"]:
                state["snapshot_manifest"] = c.reference(out/"snapshot/manifest.json")
            b.atomic(state_path, b.canonical(state)+b"\n")
            b.atomic(evidence/"result.json", b.canonical(state)+b"\n")
            return state
        except (Exception, KeyboardInterrupt) as error:
            state.update(failure_class(error), status="refused")
            b.atomic(state_path, b.canonical(state)+b"\n")
            b.atomic(evidence/"failure.json", b.canonical(state)+b"\n")
            raise


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--config", type=Path, required=True)
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--resume", action="store_true")
    ap.add_argument("--stop-after", type=int)
    a = ap.parse_args()
    try:
        print(json.dumps(capture(a.config, a.out, a.resume, a.stop_after)))
    except Exception as error:
        print(json.dumps(failure_class(error)), file=sys.stderr)
        sys.exit(2)
