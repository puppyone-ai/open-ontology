"""StarRocks capture decisions for a configured C4 or explicit C5 schedule.

The probe catalog hardcodes capture_method C4 and consistency_grade S0. This
module does not treat that label as a schedule. C1 and C2 are refused. C3 is
refused unless a cursor and cursor_safety are both present. There is no
snapshot token on this engine, so a primary or unique image is grade B.
"""

from __future__ import annotations

import hashlib
import json
from typing import Any


PROTOCOL = "oo.starrocks.capture-schedule/v1"
SYNC_ALLOWED = {"partition_refresh", "full"}
KEY_MODELS = {
    "dup_events": "duplicate",
    "pk_accounts": "primary",
    "agg_metrics": "aggregate",
    "uniq_dim": "unique",
    "dup_partition_sums": "derived_aggregate",
}


class ScheduleError(Exception):
    def __init__(self, reason: str, **details: Any) -> None:
        super().__init__(reason)
        self.reason = reason
        self.details = details


def canonical(rows: list[dict]) -> list[str]:
    return sorted(json.dumps(row, ensure_ascii=False, sort_keys=True, default=str) for row in rows)


def token_of(parts: list[dict] | None) -> str | None:
    if parts is None:
        return None
    body = json.dumps(parts, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(body.encode("utf-8")).hexdigest()


def parse_partition_table(header: list[str], rows: list[list[str]]) -> list[dict]:
    index = {name.strip().lower(): pos for pos, name in enumerate(header)}
    name_at = index.get("partitionname", index.get("partition_name"))
    version_at = index.get("visibleversion", index.get("visible_version"))
    id_at = index.get("partitionid", index.get("partition_id"))
    if name_at is None or version_at is None:
        raise ScheduleError("partition_header", header=header)
    parsed = []
    for row in rows:
        if name_at >= len(row) or version_at >= len(row):
            raise ScheduleError("partition_width", header=header, width=len(row))
        item = {
            "name": row[name_at],
            "visible_version": row[version_at],
            "partition_id": row[id_at] if id_at is not None and id_at < len(row) else "",
        }
        parsed.append(item)
    parsed.sort(key=lambda item: item["name"])
    return parsed


def changed_partitions(previous: list[dict] | None, current: list[dict]) -> list[str]:
    if not previous:
        return [item["name"] for item in current]
    before = {item["name"]: item["visible_version"] for item in previous}
    changed = []
    for item in current:
        if before.get(item["name"]) != item["visible_version"]:
            changed.append(item["name"])
    return changed


def names_of(parts: list[dict] | None) -> set[str]:
    if not parts:
        return set()
    return {item["name"] for item in parts}


def grade_for(model: str, stable: bool) -> str:
    if not stable:
        return "B"
    if model == "duplicate":
        return "S0"
    return "B"


def quality_for(model: str, method: str, stable: bool, expired: bool) -> list[str]:
    flags: list[str] = []
    if expired:
        flags.append("POSITION_EXPIRED")
    if not stable:
        flags.append("STALE_RISK")
    if model == "aggregate" or model == "derived_aggregate":
        flags.append("DELETES_UNDETECTED")
    elif method == "C5":
        flags.append("DELETES_LAGGED")
    elif method == "C4":
        flags.append("OUT_OF_PARTITION_DELETE_NOT_EXTRAPOLATED")
    return flags


def refuse_request(request: dict, secret_exists: bool, approved: set[str]) -> list[dict]:
    """Refusals that must be decided before a source read."""
    refused = []
    secret_ref = request.get("secret_ref")
    if not isinstance(secret_ref, str) or not secret_ref.startswith("secret://") or secret_ref == "secret://":
        refused.append({"reason": "secret_ref", "read_attempted": False})
        return refused
    if secret_ref != "secret://oo-dl01w1-45/reader":
        refused.append({"reason": "secret_ref_unknown", "secret_ref": secret_ref, "read_attempted": False})
        return refused
    if not secret_exists:
        refused.append({"reason": "secret_path_missing", "secret_ref": secret_ref, "read_attempted": False})
        return refused
    sync = request.get("sync")
    if sync not in SYNC_ALLOWED | {"incremental", "cdc", "change_feed", "auto"}:
        refused.append({"reason": "capability_unknown", "sync": sync, "read_attempted": False})
    if sync in {"cdc", "change_feed"}:
        refused.append({"reason": "cdc_absent", "sync": sync, "read_attempted": False})
    if sync == "auto":
        refused.append({"reason": "auto_not_selected", "read_attempted": False})
    if sync == "incremental" and not (request.get("cursor") and request.get("cursor_safety")):
        refused.append({"reason": "c3_cursor_not_configured", "read_attempted": False})
    if request.get("consistency") == "transactional":
        refused.append({"reason": "transactional_refused_non_T", "read_attempted": False})
    if request.get("license_required") and not request.get("license_ref"):
        refused.append({"reason": "license_path_missing", "read_attempted": False})
    for name in request.get("objects") or []:
        if name not in approved:
            refused.append({"reason": "not_approved", "name": name, "read_attempted": False})
    for name in request.get("fields_unknown") or []:
        refused.append({"reason": "field_not_approved", "name": name, "read_attempted": False})
    return refused


def schema_guard(approved: list[str], live: list[dict], frozen_types: dict[str, str] | None) -> dict:
    live_names = [column["name"] for column in live]
    if live_names != approved:
        return {
            "read": False,
            "advance": False,
            "reason": "schema_changed",
            "approved": approved,
            "live": live_names,
        }
    if frozen_types is not None:
        for column in live:
            seen = frozen_types.get(column["name"])
            if seen is not None and seen != column.get("column_type"):
                return {
                    "read": False,
                    "advance": False,
                    "reason": "type_changed",
                    "column": column["name"],
                    "frozen": seen,
                    "live": column.get("column_type"),
                }
    return {"read": True, "advance": True, "reason": None}


def multiset_diff(old: list[dict], fresh: list[dict], key_of) -> dict:
    def grouped(rows: list[dict]) -> dict:
        out: dict = {}
        for row in rows:
            out.setdefault(json.dumps(key_of(row), sort_keys=True, default=str), []).append(row)
        return out

    old_groups = grouped(old)
    fresh_groups = grouped(fresh)
    inserts = []
    deletes = []
    updates = []
    for key, rows in fresh_groups.items():
        previous = old_groups.get(key, [])
        if canonical(previous) != canonical(rows):
            if previous and rows:
                updates.append(key)
            elif rows:
                inserts.extend(rows)
    for key, rows in old_groups.items():
        if key not in fresh_groups:
            deletes.extend(rows)
    return {"inserts": len(inserts), "deletes": len(deletes), "updates": len(updates), "deleted_rows": deletes}


def apply_c4(old: list[dict], fresh: list[dict], changed: list[str], partition_of) -> tuple[list[dict], str]:
    changed_set = set(changed)
    old_kept = [row for row in old if partition_of(row) not in changed_set]
    fresh_changed = [row for row in fresh if partition_of(row) in changed_set]
    unchanged = [name for name in {partition_of(row) for row in fresh} | {partition_of(row) for row in old} if name not in changed_set]
    for name in unchanged:
        old_part = [row for row in old if partition_of(row) == name]
        fresh_part = [row for row in fresh if partition_of(row) == name]
        if canonical(old_part) != canonical(fresh_part):
            return list(fresh), "partition_metadata_untrusted"
    return old_kept + fresh_changed, "partition_replace"


def plan_derived(parent: dict) -> dict:
    action = "skip_read" if parent.get("action") == "skip_read" else "full_read"
    if parent.get("action") == "abort":
        action = "abort"
    return {
        "table": "dup_partition_sums",
        "model": "derived_aggregate",
        "method": "C5",
        "action": action,
        "reason": "derived_from_dup_events",
        "delete_detection": "none",
        "grade": "B",
        "quality": ["DELETES_UNDETECTED"],
        "continue_old": action == "skip_read",
        "trusted_after": False,
        "advance": action == "full_read" and bool(parent.get("advance")),
    }


def plan_table(
    table: str,
    watermark: dict | None,
    current: list[dict] | None,
    after: list[dict] | None,
    trusted: bool,
    mutation_since_watermark: bool,
) -> dict:
    model = KEY_MODELS[table]
    if model == "derived_aggregate":
        raise ScheduleError("use_plan_derived", table=table)
    if current is None:
        return {
            "table": table,
            "model": model,
            "method": "C5",
            "action": "full_read",
            "reason": "partition_metadata_unavailable",
            "delete_detection": "reconcile" if model in {"primary", "unique", "duplicate"} else "none",
            "grade": "B",
            "quality": quality_for(model, "C5", True, False),
            "continue_old": False,
            "trusted_after": False,
            "advance": True,
        }
    previous_parts = None if watermark is None else watermark.get("partitions")
    expired = previous_parts is not None and names_of(previous_parts) != names_of(current)
    stable = after is not None and token_of(current) == token_of(after)
    if after is not None and not stable:
        return {
            "table": table,
            "model": model,
            "method": "C5",
            "action": "abort",
            "reason": "version_moved_during_read",
            "delete_detection": "reconcile",
            "grade": "B",
            "quality": quality_for(model, "C5", False, False),
            "continue_old": False,
            "trusted_after": False,
            "advance": False,
        }
    if expired:
        return {
            "table": table,
            "model": model,
            "method": "C5",
            "action": "resnapshot",
            "reason": "POSITION_EXPIRED",
            "delete_detection": "reconcile",
            "grade": "B",
            "quality": quality_for(model, "C5", stable, True),
            "continue_old": False,
            "trusted_after": False,
            "advance": True,
        }
    changed = changed_partitions(previous_parts, current)
    if previous_parts is None:
        return {
            "table": table,
            "model": model,
            "method": "C5",
            "action": "initial_full",
            "reason": "no_watermark",
            "delete_detection": "reconcile" if model in {"primary", "unique", "duplicate"} else "none",
            "grade": grade_for(model, True),
            "quality": quality_for(model, "C5", True, False),
            "continue_old": False,
            "trusted_after": False,
            "advance": True,
        }
    if not changed and not mutation_since_watermark and trusted and model != "aggregate":
        return {
            "table": table,
            "model": model,
            "method": "C4",
            "action": "skip_read",
            "reason": "equal_partition_version",
            "delete_detection": "partition_replace",
            "grade": grade_for(model, True),
            "quality": quality_for(model, "C4", True, False),
            "continue_old": True,
            "trusted_after": True,
            "advance": False,
            "changed_partitions": [],
        }
    if not changed and not mutation_since_watermark and not trusted:
        return {
            "table": table,
            "model": model,
            "method": "C5",
            "action": "full_read",
            "reason": "untrusted_metadata_reconcile",
            "delete_detection": "reconcile" if model != "aggregate" else "none",
            "grade": grade_for(model, True),
            "quality": quality_for(model, "C5", True, False),
            "continue_old": False,
            "trusted_after": False,
            "advance": True,
        }
    if changed and mutation_since_watermark:
        method = "C4"
        action = "partition_replace"
        detection = "partition_replace" if model != "aggregate" else "none"
        reason = "visible_version_advanced"
        trusted_after = model != "aggregate"
        if model == "aggregate":
            method = "C5"
            action = "full_read"
            reason = "aggregate_image_not_row_cdc"
            trusted_after = False
        return {
            "table": table,
            "model": model,
            "method": method,
            "action": action,
            "reason": reason,
            "delete_detection": detection,
            "grade": grade_for(model, True),
            "quality": quality_for(model, method, True, False),
            "continue_old": False,
            "trusted_after": trusted_after,
            "advance": True,
            "changed_partitions": changed,
        }
    if mutation_since_watermark and not changed:
        return {
            "table": table,
            "model": model,
            "method": "C5",
            "action": "full_read",
            "reason": "dml_without_version_change",
            "delete_detection": "reconcile" if model in {"primary", "unique", "duplicate"} else "none",
            "grade": "B",
            "quality": quality_for(model, "C5", True, False),
            "continue_old": False,
            "trusted_after": False,
            "advance": True,
        }
    if changed and not mutation_since_watermark:
        return {
            "table": table,
            "model": model,
            "method": "C4" if trusted else "C5",
            "action": "partition_replace" if trusted and model != "aggregate" else "full_read",
            "reason": "version_changed",
            "delete_detection": "partition_replace" if trusted else "reconcile",
            "grade": grade_for(model, True),
            "quality": quality_for(model, "C4" if trusted else "C5", True, False),
            "continue_old": False,
            "trusted_after": trusted and model != "aggregate",
            "advance": True,
            "changed_partitions": changed,
        }
    return {
        "table": table,
        "model": model,
        "method": "C5",
        "action": "full_read",
        "reason": "reconcile",
        "delete_detection": "reconcile",
        "grade": grade_for(model, True),
        "quality": quality_for(model, "C5", True, False),
        "continue_old": False,
        "trusted_after": False,
        "advance": True,
    }


class Scheduler:
    def __init__(self) -> None:
        self.config: dict | None = None
        self.inflight: dict | None = None
        self.next_due_ms: int | None = None
        self.watermarks: dict[str, dict] = {}
        self.trusted: dict[str, bool] = {}
        self.ticks: list[dict] = []
        self._seq = 0

    def seed(self, name: str, item: dict) -> None:
        if self.inflight is not None:
            raise ScheduleError("overlap")
        previous = self.watermarks.get(name, {}).get("token")
        self.watermarks[name] = {
            "token": item["token"],
            "partitions": item.get("partitions"),
            "previous_token": previous,
            "continued_from_old": False,
            "generation": 1,
            "method": item["method"],
            "grade": item["grade"],
            "quality": item["quality"],
        }
        self.trusted[name] = bool(item.get("trusted_after"))

    def install(self, config: dict, now_ms: int) -> dict:
        if self.inflight is not None:
            raise ScheduleError("overlap", config_id=config.get("config_id"))
        if config.get("sync") not in SYNC_ALLOWED:
            raise ScheduleError("sync_refused", sync=config.get("sync"))
        if config.get("consistency") == "transactional":
            raise ScheduleError("transactional_refused_non_T")
        every = int(config["every_ms"])
        window = int(config["window_ms"])
        if every < 1 or window < 1 or window > every:
            raise ScheduleError("period_invalid", every_ms=every, window_ms=window)
        self.config = dict(config)
        self.next_due_ms = now_ms + every
        return {"installed": True, "config_id": config["config_id"], "next_due_ms": self.next_due_ms, "every_ms": every}

    def begin_tick(self, now_ms: int) -> dict:
        if self.config is None or self.next_due_ms is None:
            return {"admitted": False, "reason": "no_config", "read_attempted": False}
        if self.inflight is not None:
            return {"admitted": False, "reason": "overlap", "read_attempted": False, "inflight": self.inflight["tick_id"]}
        if now_ms < self.next_due_ms:
            return {
                "admitted": False,
                "reason": "not_due",
                "read_attempted": False,
                "now_ms": now_ms,
                "next_due_ms": self.next_due_ms,
                "config_id": self.config["config_id"],
            }
        self._seq += 1
        self.inflight = {
            "tick_id": f"t{self._seq}",
            "config_id": self.config["config_id"],
            "started_ms": now_ms,
            "watermark": json.loads(json.dumps(self.watermarks)),
        }
        return {"admitted": True, "tick_id": self.inflight["tick_id"], "config_id": self.config["config_id"], "read_attempted": False}

    def abort_tick(self, now_ms: int, reason: str) -> dict:
        if self.inflight is None:
            raise ScheduleError("no_inflight")
        record = {
            "tick_id": self.inflight["tick_id"],
            "config_id": self.inflight["config_id"],
            "outcome": reason,
            "advanced": False,
            "finished_ms": now_ms,
            "watermark_unchanged": True,
        }
        self.ticks.append(record)
        self.inflight = None
        return record

    def commit_tick(self, now_ms: int, tables: dict[str, dict]) -> dict:
        if self.inflight is None or self.config is None:
            raise ScheduleError("no_inflight")
        started = int(self.inflight["started_ms"])
        window = int(self.config["window_ms"])
        if now_ms - started > window:
            return self.abort_tick(now_ms, "window_exceeded")
        advanced = False
        for name, item in tables.items():
            if item.get("advance") and item.get("action") != "skip_read":
                previous = self.watermarks.get(name, {}).get("token")
                self.watermarks[name] = {
                    "token": item["token"],
                    "partitions": item.get("partitions"),
                    "previous_token": previous,
                    "continued_from_old": bool(item.get("continue_old")),
                    "generation": int(self.watermarks.get(name, {}).get("generation", 0)) + 1,
                    "method": item["method"],
                    "grade": item["grade"],
                    "quality": item["quality"],
                }
                self.trusted[name] = bool(item.get("trusted_after"))
                advanced = True
            elif item.get("action") == "skip_read":
                self.trusted[name] = bool(item.get("trusted_after"))
        every = int(self.config["every_ms"])
        self.next_due_ms = now_ms + every
        record = {
            "tick_id": self.inflight["tick_id"],
            "config_id": self.inflight["config_id"],
            "every_ms": every,
            "outcome": "committed",
            "advanced": advanced,
            "finished_ms": now_ms,
            "tables": tables,
        }
        self.ticks.append(record)
        self.inflight = None
        return record
