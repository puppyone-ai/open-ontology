#!/usr/bin/env python3
"""ClickHouse C4 partition refresh, with explicit C5 and a refused-by-default C3.

This is the source adapter. It does not write a Knowledge Pack, an OOS file,
or a sealed catalog. A refused configuration does not call the source.
Partition replacement is one shadow transaction per changed partition.
"""

from __future__ import annotations

import hashlib
import re
from datetime import datetime, timezone

import customer_case as cc

PROTOCOL = "oo.clickhouse.capture/v1"
DURATION = re.compile(r"^(\d+)(ms|s|m|h)$")
WINDOW = re.compile(r"^([01]\d|2[0-3]):[0-5]\d-([01]\d|2[0-3]):[0-5]\d$")
PARTITION_ID = re.compile(r"^[0-9]{6}$")
WRITE_PREFIXES = (
    "insert",
    "alter",
    "create",
    "drop",
    "rename",
    "optimize",
    "truncate",
    "kill",
    "grant",
    "revoke",
    "system",
    "delete",
    "update",
    "attach",
    "detach",
)

DISCLOSURE = {
    "capture_family": "partition_refresh",
    "not_log_cdc": True,
    "consistency_ceiling": "S0",
    "cross_table": "B",
    "token_order": "PerPartition",
    "write_class": "R0",
    "final_required": True,
    "async_delete": (
        "A lightweight DELETE is a mutation. is_done=0 is DELETES_LAGGED "
        "and that cycle does not publish. Partition metadata that did not "
        "change can hide a delete outside the refreshed partition."
    ),
}


def parse_duration(text: object) -> float:
    if not isinstance(text, str):
        raise ValueError("duration is missing")
    match = DURATION.fullmatch(text)
    if match is None:
        raise ValueError("duration is not a closed unit")
    count = int(match.group(1))
    unit = match.group(2)
    scale = {"ms": 0.001, "s": 1.0, "m": 60.0, "h": 3600.0}[unit]
    return count * scale


def fingerprint(columns: list[dict]) -> str:
    lines = [f"{item['name']}\t{item['type']}" for item in columns]
    return "\n".join(lines)


def refuse(reason: str, detail: str) -> dict:
    return {
        "protocol": PROTOCOL,
        "admitted": False,
        "reason": reason,
        "detail": detail,
        "source_read": False,
        "sql": None,
        "disclosure": DISCLOSURE,
    }


def _window_open(window: str, moment: datetime) -> bool:
    if not WINDOW.fullmatch(window):
        raise ValueError("window is not HH:MM-HH:MM")
    start, end = window.split("-")
    now = moment.astimezone(timezone.utc).strftime("%H:%M")
    if start <= end:
        return start <= now < end
    return now >= start or now < end


def gate(customer: dict, capture: dict, secret_path: str | None, secret_is_file: bool) -> dict:
    if not isinstance(capture, dict):
        return refuse("capture_config", "capture config is not an object")
    sync = capture.get("sync")
    if sync == "cdc" or sync == "change_feed":
        return refuse("sync_cdc", "ClickHouse has no log CDC and this adapter will not call it one")
    if sync == "auto":
        return refuse("sync_auto", "auto is not a verified choice for this source")
    if sync not in {"partition_refresh", "full", "incremental"}:
        return refuse("sync_unknown", "sync method is not an explicit C4, C5, or C3 setting")
    if capture.get("consistency") == "transactional":
        return refuse("consistency_transactional", "a non-T source cannot take transactional consistency")
    if capture.get("consistency") != "per_table":
        return refuse("consistency_unknown", "consistency must be per_table for this S0 source")
    if capture.get("cdc") not in (None, {}):
        return refuse("cdc_settings", "CDC settings are present and this source has no CDC capability")
    license_state = capture.get("license")
    if license_state != "not_required":
        return refuse("license_unknown", "instance license is unknown, so the source is not read")
    capabilities = capture.get("capabilities")
    if not isinstance(capabilities, list):
        return refuse("capability_unknown", "capabilities were not declared")
    needed = {"partition_refresh": "C4", "full": "C5", "incremental": "C3"}[sync]
    if needed not in capabilities:
        return refuse("capability_unknown", f"{needed} is not a declared capability")
    if not secret_is_file or not isinstance(secret_path, str) or not secret_path:
        return refuse("secret_path_missing", "the secret path is missing or is not a file")
    locale = capture.get("locale")
    if not isinstance(locale, str) or not locale:
        return refuse("locale_missing", "locale is required")
    budget = capture.get("budget")
    if not isinstance(budget, dict) or budget.get("unit") != "Rows":
        return refuse("budget_unit", "budget unit must be Rows")
    limit = budget.get("hard_limit")
    if not isinstance(limit, int) or isinstance(limit, bool) or limit < 1:
        return refuse("budget_limit", "row hard limit is missing")
    try:
        every = parse_duration(capture.get("every"))
    except ValueError as exc:
        return refuse("period", str(exc))
    window = capture.get("window")
    if window is not None and not (isinstance(window, str) and WINDOW.fullmatch(window)):
        return refuse("window", "window is not HH:MM-HH:MM")
    columns = capture.get("schema_columns")
    if not isinstance(columns, list) or not columns:
        return refuse("schema_unknown", "the registered column list is missing")
    for item in columns:
        if not isinstance(item, dict) or not isinstance(item.get("name"), str) or not isinstance(item.get("type"), str):
            return refuse("schema_unknown", "a registered column has no name or type")
    delete_mode = (capture.get("delete_detection") or {}).get("mode") if isinstance(capture.get("delete_detection"), dict) else None
    if sync == "partition_refresh" and delete_mode != "partition_replace":
        return refuse("delete_detection", "C4 requires partition_replace")
    if sync == "full" and delete_mode not in {"reconcile", "partition_replace"}:
        return refuse("delete_detection", "C5 requires reconcile or partition_replace")
    overlap_seconds = 0.0
    if sync == "incremental":
        if capture.get("admin_confirmed") is not True:
            return refuse("cursor_unsafe", "C3 needs an explicit admin confirmation")
        if capture.get("cursor") != "ts" or capture.get("cursor_safety") != "statement_time":
            return refuse("cursor_unsafe", "this table has no commit-ordered cursor")
        if delete_mode != "reconcile":
            return refuse("cursor_unsafe", "C3 has no native deletes without reconcile")
        try:
            overlap_seconds = parse_duration(capture.get("overlap"))
            parse_duration((capture.get("delete_detection") or {}).get("every"))
        except ValueError as exc:
            return refuse("cursor_unsafe", str(exc))
    requested = {
        "object": capture.get("object"),
        "fields": capture.get("fields"),
    }
    try:
        selected = cc.select_fields(customer, requested)
    except SystemExit as exc:
        return refuse("customer_input", str(exc))
    if isinstance(selected, dict):
        return refuse(str(selected.get("reason")), str(selected.get("detail")))
    _chosen, fields = selected
    names = [field["name"] for field in fields]
    database = capture["object"]["database"]
    table = capture["object"]["table"]
    sql = (
        f"SELECT {', '.join(names)} FROM {database}.{table} FINAL "
        "WHERE _partition_id = {partition:String} ORDER BY reading_id FORMAT ArrowStream"
    )
    return {
        "protocol": PROTOCOL,
        "admitted": True,
        "reason": None,
        "detail": None,
        "source_read": False,
        "sql": sql,
        "sync": sync,
        "method": {"partition_refresh": "C4", "full": "C5", "incremental": "C3"}[sync],
        "every_seconds": every,
        "hard_limit": limit,
        "locale": locale,
        "fields": names,
        "fingerprint": fingerprint(columns),
        "window": window,
        "untrusted_partition_metadata": capture.get("untrusted_partition_metadata") is True,
        "overlap_seconds": overlap_seconds,
        "disclosure": DISCLOSURE,
        "object": {"database": database, "table": table},
    }


def observe_period(every: str, sleep, now) -> float:
    seconds = parse_duration(every)
    start = float(now())
    sleep(seconds)
    return float(now()) - start


def _outside(plan: dict, moment: datetime) -> bool:
    window = plan.get("window")
    if window is None:
        return False
    return not _window_open(window, moment)


def _quality_base(method: str) -> list[str]:
    if method == "C4":
        return ["S0", "PARTITION_REPLACE"]
    if method == "C5":
        return ["B", "FULL_DIFF"]
    return ["B", "DELETES_LAGGED"]


def _project(checkpoint: dict | None, replacements: list[tuple[str, list[dict]]], full: list[dict] | None) -> list[dict]:
    if full is not None:
        return list(full)
    published = {}
    if checkpoint is not None:
        for row in checkpoint.get("rows") or []:
            published[row["reading_id"]] = row
    for partition, rows in replacements:
        published = {key: value for key, value in published.items() if value.get("partition_id") != partition}
        for row in rows:
            published[row["reading_id"]] = row
    return list(published.values())


def cycle(plan: dict, source, shadow, checkpoint: dict | None, moment: datetime, before_commit=None) -> dict:
    if not plan.get("admitted"):
        return {
            "published": False,
            "source_read": False,
            "reason": plan.get("reason"),
            "rolled_back": False,
        }
    if _outside(plan, moment):
        return {
            "published": False,
            "source_read": False,
            "reason": "outside_window",
            "rolled_back": False,
            "disclosure": plan["disclosure"],
        }
    meta = source.inspect()
    if getattr(source, "statements", None):
        for statement in source.statements:
            head = statement.lstrip().split(" ", 1)[0].lower()
            if head in WRITE_PREFIXES:
                raise RuntimeError("product statement is not a read")
    if meta.get("mutation_pending"):
        return {
            "published": False,
            "source_read": True,
            "reason": "mutation_pending",
            "quality": ["DELETES_LAGGED", "S0"],
            "mutation_is_done": meta.get("mutation_is_done"),
            "rolled_back": False,
            "disclosure": plan["disclosure"],
        }
    if meta.get("fingerprint") != plan["fingerprint"]:
        return {
            "published": False,
            "source_read": True,
            "reason": "schema_changed",
            "quality": ["SCHEMA_CHANGED"],
            "rolled_back": False,
            "observed_fingerprint": meta.get("fingerprint"),
            "disclosure": plan["disclosure"],
        }
    method = plan["method"]
    expired = bool(checkpoint) and meta.get("uuid") != checkpoint.get("uuid")
    untrusted = plan["untrusted_partition_metadata"] or method == "C5" or expired
    if method == "C3" and not untrusted:
        return _cycle_c3(plan, source, shadow, checkpoint, meta, before_commit)
    if untrusted and method != "C4":
        method = "C5"
    elif untrusted and (plan["untrusted_partition_metadata"] or expired):
        method = "C5"
    quality = _quality_base(method)
    if expired:
        quality = ["POSITION_EXPIRED", "B"]
    partitions = list(meta.get("partitions") or [])
    for part in partitions:
        if not isinstance(part.get("id"), str) or PARTITION_ID.fullmatch(part["id"]) is None:
            return {
                "published": False,
                "source_read": True,
                "reason": "partition_id",
                "rolled_back": False,
                "disclosure": plan["disclosure"],
            }
    known = {} if checkpoint is None else dict(checkpoint.get("partitions") or {})
    if method == "C5":
        loaded = source.read_all()
        replacements = [("__all__", loaded)]
        projected = loaded
    else:
        changed = []
        seen = set()
        for part in partitions:
            seen.add(part["id"])
            if known.get(part["id"]) != part.get("signature"):
                changed.append(part["id"])
        for old in known:
            if old not in seen:
                changed.append(old)
        if not changed and checkpoint is not None:
            return {
                "published": False,
                "source_read": True,
                "applied": False,
                "reason": "unchanged",
                "quality": quality,
                "generation": checkpoint.get("generation"),
                "rows": list(checkpoint.get("rows") or []),
                "rolled_back": False,
                "replaces": 0,
                "disclosure": plan["disclosure"],
            }
        replacements = []
        for part_id in changed:
            if part_id not in seen:
                replacements.append((part_id, []))
            else:
                replacements.append((part_id, source.read_partition(part_id)))
        projected = _project(checkpoint, replacements, None)
    if len(projected) > plan["hard_limit"]:
        return {
            "published": False,
            "source_read": True,
            "reason": "SOURCE_BUDGET_EXHAUSTED",
            "quality": ["SOURCE_BUDGET_EXHAUSTED"],
            "rolled_back": False,
            "disclosure": plan["disclosure"],
        }
    counted = source.count()
    if counted != len(projected):
        return {
            "published": False,
            "source_read": True,
            "reason": "count_mismatch",
            "source_count": counted,
            "projected": len(projected),
            "rolled_back": False,
            "disclosure": plan["disclosure"],
        }
    shadow.begin()
    try:
        if method == "C5":
            shadow.replace_all(projected)
        else:
            for part_id, rows in replacements:
                shadow.replace_partition(part_id, rows)
        if before_commit is not None:
            before_commit()
        generation = shadow.commit()
    except Cancelled:
        shadow.rollback()
        return {
            "published": False,
            "source_read": True,
            "reason": "interrupted",
            "rolled_back": True,
            "generation": None if checkpoint is None else checkpoint.get("generation"),
            "disclosure": plan["disclosure"],
        }
    next_checkpoint = {
        "generation": generation,
        "uuid": meta.get("uuid"),
        "fingerprint": meta.get("fingerprint"),
        "partitions": {part["id"]: part.get("signature") for part in partitions},
        "rows": projected,
        "method": method,
        "locale": plan["locale"],
    }
    return {
        "published": True,
        "source_read": True,
        "applied": True,
        "reason": None,
        "method": method,
        "quality": quality,
        "generation": generation,
        "rows": projected,
        "checkpoint": next_checkpoint,
        "rolled_back": False,
        "replaces": len(replacements),
        "disclosure": plan["disclosure"],
    }


def _cycle_c3(plan, source, shadow, checkpoint, meta, before_commit) -> dict:
    quality = ["B", "DELETES_LAGGED"]
    if checkpoint is None or checkpoint.get("reconcile_due") is True:
        loaded = source.read_all()
        mode = "reconcile" if checkpoint is not None else "initial"
    else:
        loaded = source.read_since(checkpoint.get("cursor"), plan.get("overlap_seconds") or 0)
        mode = "incremental"
    if len(loaded) > plan["hard_limit"]:
        return {
            "published": False,
            "source_read": True,
            "reason": "SOURCE_BUDGET_EXHAUSTED",
            "rolled_back": False,
            "disclosure": plan["disclosure"],
        }
    shadow.begin()
    try:
        if mode == "incremental":
            shadow.upsert(loaded)
        else:
            shadow.replace_all(loaded)
        if before_commit is not None:
            before_commit()
        generation = shadow.commit()
    except Cancelled:
        shadow.rollback()
        return {
            "published": False,
            "source_read": True,
            "reason": "interrupted",
            "rolled_back": True,
            "disclosure": plan["disclosure"],
        }
    rows = shadow.rows()
    cursor = max((int(row["ts"]) for row in rows), default=None)
    next_checkpoint = {
        "generation": generation,
        "uuid": meta.get("uuid"),
        "fingerprint": meta.get("fingerprint"),
        "rows": rows,
        "cursor": cursor,
        "method": "C3",
        "reconcile_due": False,
        "locale": plan["locale"],
    }
    return {
        "published": True,
        "source_read": True,
        "applied": True,
        "method": "C3",
        "mode": mode,
        "quality": quality,
        "generation": generation,
        "rows": rows,
        "checkpoint": next_checkpoint,
        "rolled_back": False,
        "disclosure": plan["disclosure"],
    }


class Cancelled(Exception):
    pass


def canonical(rows: list[dict]) -> list[tuple]:
    body = []
    for row in rows:
        note = row.get("note")
        if isinstance(note, bytes):
            note_text = note.hex()
        elif note is None:
            note_text = None
        else:
            note_text = str(note)
        body.append(
            (
                str(row["reading_id"]),
                str(row["ts"]),
                str(row["amount"]),
                note_text,
                str(row["qty"]),
            )
        )
    return sorted(body)


def content_sha256(rows: list[dict]) -> str:
    text = "\n".join("\t".join("" if item is None else item for item in line) for line in canonical(rows))
    return hashlib.sha256(text.encode("utf-8")).hexdigest()
