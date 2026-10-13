"""Oracle C5 full-diff shadow.

This is a periodic full read compared by key and row hash. It is not log CDC,
not LogMiner, and not the Agent native loader. schema_digest stays unset
until a sealer catalog object exists.
"""

from __future__ import annotations

import hashlib
import json
import os
import time
from pathlib import Path

DOCS_DAILY_SECONDS = 86400
METHOD = "C5"
INITIAL_LOAD = "shadow_diff"
DELETE_DETECTION = "reconcile"
PERIOD_KEYS = (
    "detect_period_seconds",
    "pull_period_seconds",
    "reconcile_period_seconds",
    "delete_visibility_sla_seconds",
)
TEXT_TYPES = {"CHAR", "VARCHAR2", "NCHAR", "NVARCHAR2", "CLOB", "NCLOB"}
PLATFORM_DML = {
    "INSERT",
    "UPDATE",
    "DELETE",
    "MERGE",
    "ALTER",
    "DROP",
    "TRUNCATE",
    "GRANT",
    "CREATE",
}


class CaptureError(ValueError):
    pass


def resolve_periods(raw):
    if raw is None:
        raw = {}
    if not isinstance(raw, dict):
        raise CaptureError("periods")
    unknown = set(raw) - set(PERIOD_KEYS)
    if unknown:
        raise CaptureError("unknown_period")
    values = {}
    defaults = 0
    for key in PERIOD_KEYS:
        if key not in raw or raw[key] is None:
            values[key] = DOCS_DAILY_SECONDS
            defaults += 1
            continue
        value = raw[key]
        if type(value) is not int or value < 1:
            raise CaptureError(key)
        values[key] = value
    if values["reconcile_period_seconds"] > values["delete_visibility_sla_seconds"]:
        raise CaptureError("delete_sla_unmet")
    if defaults == len(PERIOD_KEYS):
        origin = "docs_default"
    elif defaults == 0:
        origin = "configured"
    else:
        origin = "mixed"
    values["origin"] = origin
    return values


def wait_periods(periods, sleeper, clock):
    steps = []
    for name, key in (
        ("detect", "detect_period_seconds"),
        ("pull", "pull_period_seconds"),
        ("reconcile", "reconcile_period_seconds"),
    ):
        configured = periods[key]
        before = clock()
        sleeper(configured)
        elapsed = clock() - before
        if elapsed < configured - 0.02:
            raise CaptureError("short_wait")
        steps.append({
            "name": name,
            "configured_seconds": configured,
            "elapsed_seconds": elapsed,
        })
    return {
        "origin": periods["origin"],
        "steps": steps,
        "elapsed_seconds": sum(step["elapsed_seconds"] for step in steps),
    }


def wait_realtime(periods):
    return wait_periods(periods, time.sleep, time.monotonic)


def canonical_json(cells):
    if not isinstance(cells, dict):
        raise CaptureError("cells")
    clean = {}
    for key in sorted(cells):
        if not isinstance(key, str) or not key.replace("_", "").isalnum() or not key.isascii():
            raise CaptureError("cell_key")
        value = cells[key]
        if value is None or isinstance(value, str):
            clean[key] = value
        elif type(value) is int:
            clean[key] = value
        else:
            raise CaptureError("cell_value")
    return json.dumps(clean, sort_keys=True, separators=(",", ":"), ensure_ascii=True)


def row_hash(cells):
    return hashlib.sha256(canonical_json(cells).encode("ascii")).hexdigest()


def column_flags(data_type, precision, scale):
    flags = []
    if data_type in TEXT_TYPES:
        flags.append("empty_string_is_null")
    if data_type == "NUMBER" and precision is None:
        flags.append("unconstrained_number")
    if str(data_type).startswith("TIMESTAMP") and scale is not None and scale > 6:
        flags.append("precision_loss")
    return flags


def normalize_text_cell(data_type, value):
    if data_type in TEXT_TYPES and value == "":
        return None
    return value


def consistency_grade(snapshot_token, source_changed_without_token):
    if source_changed_without_token or not snapshot_token:
        return "B"
    return "S0"


def diff_hashes(previous, nxt):
    events = []
    for key in sorted(set(previous) | set(nxt)):
        old = previous.get(key)
        new = nxt.get(key)
        if old is None and new is not None:
            events.append({"op": "insert", "key": key})
        elif old is not None and new is None:
            events.append({"op": "delete", "key": key})
        elif old != new:
            events.append({"op": "update", "key": key})
    return events


def position_state(age_seconds, retention_seconds):
    if retention_seconds == 0 or age_seconds > retention_seconds:
        return "POSITION_EXPIRED"
    if age_seconds * 10 > retention_seconds * 8:
        return "retention_pause"
    if age_seconds * 2 > retention_seconds:
        return "STALE_RISK"
    return "ready"


def resume_expired():
    raise CaptureError("resume_forbidden")


def generation_path(root, generation):
    return Path(root) / f"gen-{generation}.json"


def staging_path(root, generation):
    return Path(root) / f"gen-{generation}.staging.json"


def load_generation(root, generation):
    return json.loads(generation_path(root, generation).read_text())


def _tombstone_hit(tombstones, key, snapshot_generation):
    for item in tombstones:
        name, _, fence = item.rpartition(":")
        if name == key and snapshot_generation <= int(fence):
            return True
    return False


def apply_snapshot(
    root,
    previous,
    generation,
    position,
    columns,
    approved,
    rows,
    volume_limit,
    interrupt=False,
    stale_upsert_generation=None,
    snapshot_token=True,
    source_changed_without_token=False,
    quality=None,
):
    root = Path(root)
    if len(rows) > volume_limit:
        raise CaptureError("volume_exceeds_limit")
    if list(columns) != list(approved):
        return {
            "committed": False,
            "state": "paused_schema_change",
            "generation": None if previous is None else previous["generation"],
            "events": [],
            "resumed": False,
            "schema_digest": None,
            "admitted": False,
        }
    hashes = {}
    cells = {}
    for key, row in rows.items():
        cells[key] = {name: row[name] for name in columns}
        hashes[key] = row_hash(cells[key])
    prior = {} if previous is None else previous["hashes"]
    events = diff_hashes(prior, hashes)
    tombstones = [] if previous is None else list(previous["tombstones"])
    if stale_upsert_generation is not None:
        for event in events:
            if event["op"] == "insert" and _tombstone_hit(tombstones, event["key"], stale_upsert_generation):
                raise CaptureError("stale_upsert")
    for event in events:
        if event["op"] == "delete":
            tombstones.append(f"{event['key']}:{generation}")
    body = {
        "generation": generation,
        "position": position,
        "columns": list(columns),
        "hashes": hashes,
        "cells": cells,
        "tombstones": tombstones,
        "resumed": False,
        "method": METHOD,
        "initial_load": INITIAL_LOAD,
        "delete_detection": DELETE_DETECTION,
        "consistency_grade": consistency_grade(snapshot_token, source_changed_without_token),
        "quality": list(quality or ["DELETES_LAGGED"]),
        "schema_digest": None,
        "admitted": False,
        "native_loader": False,
        "log_cdc": False,
    }
    root.mkdir(parents=True, exist_ok=True)
    staging = staging_path(root, generation)
    text = json.dumps(body, indent=2, sort_keys=True) + "\n"
    with staging.open("w", encoding="utf-8") as handle:
        handle.write(text)
        handle.flush()
        os.fsync(handle.fileno())
    if interrupt:
        staging.unlink()
        return {
            "committed": False,
            "state": "interrupted",
            "generation": None if previous is None else previous["generation"],
            "events": events,
            "resumed": False,
            "schema_digest": None,
            "admitted": False,
        }
    os.replace(staging, generation_path(root, generation))
    return {
        "committed": True,
        "state": "committed",
        "generation": generation,
        "events": events,
        "resumed": False,
        "schema_digest": None,
        "admitted": False,
        "path": str(generation_path(root, generation)),
    }


def classify_audit(rows):
    platform = []
    kernel = []
    other = []
    for row in rows:
        schema = row.get("object_schema") or "-"
        name = row.get("object_name") or "-"
        action = row.get("action") or ""
        if schema == "AUDSYS" or name in {"AUD$UNIFIED", "AUD$"}:
            kernel.append(row)
        elif action in PLATFORM_DML:
            platform.append(row)
        else:
            other.append(row)
    return {
        "platform_dml": len(platform),
        "kernel_audit": len(kernel),
        "other": len(other),
        "platform_rows": platform,
    }


def parse_dump(text):
    # This Oracle build returns DUMP(NULL) as the varchar NULL, so NVL does not substitute.
    if text in {"<null>", "NULL"}:
        return None
    marker = "Len="
    if marker not in text or ":" not in text:
        raise CaptureError("dump")
    hexpart = text.split(":", 1)[1].strip()
    if not hexpart:
        return ""
    raw = bytes(int(part, 16) for part in hexpart.split(","))
    return raw.decode("utf-8")


def refuse_forbidden_sql(sql):
    upper = sql.upper()
    for token in ("DBMS_FLASHBACK", "DBMS_LOGMNR", "LOGMINER", " AS SYSDBA", "GRANT ", "SUPPLEMENTAL LOG", "KILL "):
        if token in upper:
            raise CaptureError("forbidden_sql")
    return sql
