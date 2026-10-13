#!/usr/bin/env python3
"""Classify raw probe, observer, and audit exits. Does not invent missing files."""

from __future__ import annotations

import json
import sys
from pathlib import Path

ALLOW = {
    "SELECT",
    "LOGON",
    "LOGOFF",
    "LOGOFF BY CLEANUP",
    "SESSION REC",
    "EXECUTE",
    "CALL",
    "PL/SQL EXECUTE",
    "ALTER SESSION",
    "COMMIT",
}
FORBID_SUCCESS = {
    "INSERT",
    "UPDATE",
    "DELETE",
    "CREATE TABLE",
    "ALTER DATABASE",
    "ALTER TABLE",
    "DROP TABLE",
    "GRANT",
    "LOGMINING",
}


def load_json(path: Path):
    if not path.is_file():
        return None
    return json.loads(path.read_text())


def parse_audit(path: Path) -> list[dict]:
    rows = []
    if not path.is_file():
        return rows
    for line in path.read_text(errors="replace").splitlines():
        if not line.startswith("audit|"):
            continue
        parts = line.split("|", 7)
        if len(parts) < 7:
            continue
        _, user, action, code, schema, obj, sql = parts[:7]
        try:
            return_code = int(code)
        except ValueError:
            return_code = None
        rows.append(
            {
                "user": user,
                "action": action,
                "return_code": return_code,
                "object_schema": schema,
                "object_name": obj,
                "sql": sql,
            }
        )
    return rows


def parse_obs(path: Path) -> list[dict]:
    rows = []
    if not path.is_file():
        return rows
    for line in path.read_text(errors="replace").splitlines():
        if not line.startswith("obs|"):
            continue
        parts = line.split("|", 8)
        if len(parts) < 9:
            continue
        _, id_text, sample, user, sid, serial, status, module, sql = parts
        rows.append(
            {
                "id": int(id_text),
                "sample": int(sample) if sample.isdigit() else None,
                "user": user,
                "sid": sid,
                "serial": serial,
                "status": status,
                "module": module,
                "sql": sql,
            }
        )
    return rows


def main() -> int:
    raw = Path(sys.argv[1])
    cutoff_path = raw / "oracle" / "obs-business-max.txt"
    cutoff = 10**12
    if cutoff_path.is_file():
        text = cutoff_path.read_text().strip().splitlines()
        nums = [line.strip() for line in text if line.strip().isdigit()]
        if nums:
            cutoff = int(nums[-1])
    audit_rows = parse_audit(raw / "oracle" / "dba-audit.out")
    obs_rows = parse_obs(raw / "oracle" / "dba-obs.out")
    business = [row for row in obs_rows if row["id"] <= cutoff and row["user"] == "OO34_READER"]
    peak = 0
    by_sample: dict[int, int] = {}
    for row in business:
        if row["sample"] is None:
            continue
        by_sample[row["sample"]] = by_sample.get(row["sample"], 0) + 1
    if by_sample:
        peak = max(by_sample.values())
    marker = "oo-dl01w1-34-cancel"
    active_marker = [
        row
        for row in obs_rows
        if row["user"] == "OO34_READER" and row["status"] == "ACTIVE" and marker in row["sql"]
    ]
    cancel = load_json(raw / "probe" / "cancel-client.json") or {}
    gates = load_json(raw / "probe" / "gates.json") or {}
    negative = load_json(raw / "probe" / "negative.json") or {}
    identity = load_json(raw / "probe" / "identity.json") or {}
    platform_dml = []
    unknown = []
    allowed = []
    negative_failures = []
    for row in audit_rows:
        action = row["action"]
        success = row["return_code"] == 0
        secret = "T_SECRET" in row["sql"].upper() or row["object_name"] == "T_SECRET"
        labeled_negative = row["user"] == "OO34_NOFLASH" or secret
        if action in FORBID_SUCCESS and success and not labeled_negative:
            platform_dml.append(row)
        elif action in FORBID_SUCCESS and not success:
            negative_failures.append(row)
        elif action in ALLOW:
            allowed.append({"user": row["user"], "action": action, "return_code": row["return_code"]})
        else:
            unknown.append(row)
    status_text = ""
    status_path = raw / "oracle" / "dba-session-status.out"
    if status_path.is_file():
        status_text = status_path.read_text(errors="replace")
    still_active = "status|ACTIVE|" in status_text
    absent = "status|done" in status_text and "status|" not in status_text.replace("status|done", "")
    dba_kill = (raw / "oracle" / "dba-cancel.out").is_file()
    body = {
        "driver_server_version": identity.get("driver_server_version"),
        "certifies_19c": identity.get("certifies_19c"),
        "certifies_marketing_alias": identity.get("certifies_marketing_alias"),
        "tls": identity.get("tls"),
        "business_reader_peak": peak,
        "business_reader_samples": len(by_sample),
        "cancel_client_timeout": cancel.get("client_timeout"),
        "cancel_elapsed_ms": cancel.get("elapsed_ms"),
        "cancel_error": cancel.get("error"),
        "cancel_server_seen_active": bool(active_marker),
        "cancel_server_still_active": still_active,
        "cancel_server_status_absent": absent,
        "dba_kill_used": dba_kill,
        "gates": gates,
        "negative_denied": bool(
            negative.get("insert_denied")
            and negative.get("create_denied")
            and negative.get("asof_denied")
            and negative.get("secret_denied")
        ),
        "platform_dml_success": platform_dml,
        "audit_unknown": unknown,
        "audit_allowed_count": len(allowed),
        "audit_negative_or_failed_count": len(negative_failures),
        "audit_rows": len(audit_rows),
        "obs_cutoff_id": cutoff,
    }
    out = raw / "classification.json"
    out.write_text(json.dumps(body, indent=2) + "\n")
    print(json.dumps({k: body[k] for k in body if k != "gates"}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
