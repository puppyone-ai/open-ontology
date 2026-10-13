#!/usr/bin/env python3
"""Scheduler decisions for the StarRocks C4/C5 cycle. No database."""

from __future__ import annotations

import capture_schedule as cap


APPROVED = {"dup_events", "pk_accounts", "agg_metrics", "uniq_dim", "dup_partition_sums"}


def parts(items: list[tuple[str, str]]) -> list[dict]:
    return [{"name": name, "visible_version": version, "partition_id": name} for name, version in items]


def test_refusals_before_read() -> None:
    missing = cap.refuse_request({"secret_ref": "secret://oo-dl01w1-45/reader", "sync": "partition_refresh", "objects": ["pk_accounts"]}, False, APPROVED)
    assert missing[0]["reason"] == "secret_path_missing" and missing[0]["read_attempted"] is False
    unknown = cap.refuse_request({"secret_ref": "secret://oo-dl01w1-45/not-issued", "sync": "full", "objects": []}, True, APPROVED)
    assert unknown[0]["reason"] == "secret_ref_unknown"
    request = {
        "secret_ref": "secret://oo-dl01w1-45/reader",
        "sync": "cdc",
        "consistency": "transactional",
        "license_required": True,
        "objects": ["sketch_hold"],
        "fields_unknown": ["password"],
    }
    reasons = {item["reason"] for item in cap.refuse_request(request, True, APPROVED)}
    assert {"cdc_absent", "transactional_refused_non_T", "license_path_missing", "not_approved", "field_not_approved"} <= reasons
    c3 = cap.refuse_request({"secret_ref": "secret://oo-dl01w1-45/reader", "sync": "incremental", "objects": ["pk_accounts"]}, True, APPROVED)
    assert any(item["reason"] == "c3_cursor_not_configured" for item in c3)
    clean = cap.refuse_request({"secret_ref": "secret://oo-dl01w1-45/reader", "sync": "partition_refresh", "objects": ["pk_accounts"]}, True, APPROVED)
    assert clean == []


def test_two_periods_do_not_overlap() -> None:
    clock = cap.Scheduler()
    first = clock.install({"config_id": "user-fast", "sync": "partition_refresh", "every_ms": 8000, "window_ms": 6000}, 0)
    assert first["next_due_ms"] == 8000
    early = clock.begin_tick(7999)
    assert early["admitted"] is False and early["reason"] == "not_due" and early["read_attempted"] is False
    opened = clock.begin_tick(8000)
    assert opened["admitted"] is True
    overlap = clock.begin_tick(8000)
    assert overlap["reason"] == "overlap"
    try:
        clock.install({"config_id": "user-slow", "sync": "full", "every_ms": 20000, "window_ms": 15000}, 8000)
        raise AssertionError("install overlapped an open tick")
    except cap.ScheduleError as exc:
        assert exc.reason == "overlap"
    aborted = clock.abort_tick(8100, "interrupted")
    assert aborted["advanced"] is False and aborted["watermark_unchanged"] is True
    assert clock.watermarks == {}
    second = clock.install({"config_id": "user-slow", "sync": "full", "every_ms": 20000, "window_ms": 15000}, 9000)
    assert second["config_id"] == "user-slow" and second["next_due_ms"] == 29000
    assert clock.begin_tick(28999)["reason"] == "not_due"
    assert clock.begin_tick(29000)["admitted"] is True


def test_window_and_stale_do_not_advance() -> None:
    clock = cap.Scheduler()
    clock.install({"config_id": "user-fast", "sync": "full", "every_ms": 8000, "window_ms": 6000}, 0)
    assert clock.begin_tick(8000)["admitted"] is True
    record = clock.commit_tick(14001, {"pk_accounts": {"advance": True, "action": "full_read", "token": "n", "partitions": [], "method": "C5", "grade": "B", "quality": [], "trusted_after": False, "continue_old": False}})
    assert record["outcome"] == "window_exceeded" and record["advanced"] is False
    assert "pk_accounts" not in clock.watermarks


def test_c4_replace_and_c5_delete_stays_gone() -> None:
    old_dup = [
        {"event_id": 21, "bucket": "2026-05-06"},
        {"event_id": 23, "bucket": "2026-06-03"},
    ]
    fresh_dup = [
        {"event_id": 21, "bucket": "2026-05-06"},
        {"event_id": 23, "bucket": "2026-06-03"},
        {"event_id": 26, "bucket": "2026-06-11"},
    ]

    def part_of(row: dict) -> str:
        return "p202605" if row["bucket"] < "2026-06-01" else "p202606"

    replaced, how = cap.apply_c4(old_dup, fresh_dup, ["p202606"], part_of)
    assert how == "partition_replace"
    assert cap.canonical(replaced) == cap.canonical(fresh_dup)
    lied, fallback = cap.apply_c4(old_dup, [{"event_id": 99, "bucket": "2026-05-07"}, {"event_id": 26, "bucket": "2026-06-11"}], ["p202606"], part_of)
    assert fallback == "partition_metadata_untrusted"
    assert lied[0]["event_id"] == 99

    old_pk = [{"account_id": 300, "nickname": "new"}, {"account_id": 302, "nickname": ""}]
    fresh_pk = [{"account_id": 300, "nickname": "edited"}]
    diff = cap.multiset_diff(old_pk, fresh_pk, lambda row: row["account_id"])
    assert diff["deletes"] == 1 and diff["updates"] == 1
    assert diff["deleted_rows"][0]["account_id"] == 302
    again = cap.multiset_diff(fresh_pk, fresh_pk, lambda row: row["account_id"])
    assert again["deletes"] == 0 and 302 not in {row["account_id"] for row in fresh_pk}


def test_equal_version_skip_and_expiry() -> None:
    current = parts([("p202605", "2"), ("p202606", "3")])
    watermark = {"partitions": parts([("p202605", "2"), ("p202606", "3")])}
    skip = cap.plan_table("dup_events", watermark, current, current, True, False)
    assert skip["action"] == "skip_read" and skip["method"] == "C4" and skip["advance"] is False
    untrusted = cap.plan_table("dup_events", watermark, current, current, False, False)
    assert untrusted["method"] == "C5" and untrusted["action"] == "full_read"
    moved = parts([("p202605", "2"), ("p202606", "4")])
    catch = cap.plan_table("dup_events", watermark, moved, moved, False, True)
    assert catch["method"] == "C4" and catch["changed_partitions"] == ["p202606"] and "OUT_OF_PARTITION_DELETE_NOT_EXTRAPOLATED" in catch["quality"]
    assert catch["grade"] == "S0"
    silent = cap.plan_table("pk_accounts", {"partitions": parts([("pk", "1")])}, parts([("pk", "1")]), parts([("pk", "1")]), False, True)
    assert silent["method"] == "C5" and silent["reason"] == "dml_without_version_change" and silent["grade"] == "B"
    assert "DELETES_LAGGED" in silent["quality"]
    expired = cap.plan_table("dup_events", watermark, parts([("p202606", "5")]), parts([("p202606", "5")]), True, True)
    assert expired["reason"] == "POSITION_EXPIRED" and expired["continue_old"] is False
    stale = cap.plan_table("pk_accounts", {"partitions": parts([("pk", "1")])}, parts([("pk", "2")]), parts([("pk", "3")]), True, True)
    assert stale["action"] == "abort" and stale["advance"] is False and "STALE_RISK" in stale["quality"]
    primary = cap.plan_table("pk_accounts", {"partitions": parts([("pk", "1")])}, parts([("pk", "2")]), parts([("pk", "2")]), False, True)
    assert primary["grade"] == "B" and primary["method"] == "C4"
    derived = cap.plan_derived(skip)
    assert derived["action"] == "skip_read" and derived["method"] == "C5"


def test_schema_guard_and_transactional_config() -> None:
    approved = ["account_id", "balance", "nickname", "opened"]
    live = [{"name": name, "column_type": "bigint"} for name in approved]
    assert cap.schema_guard(approved, live, {"account_id": "bigint"})["read"] is True
    extra = live + [{"name": "password", "column_type": "varchar(32)"}]
    blocked = cap.schema_guard(approved, extra, None)
    assert blocked["read"] is False and blocked["reason"] == "schema_changed" and blocked["advance"] is False
    typed = cap.schema_guard(approved, [{"name": "account_id", "column_type": "int"}, *live[1:]], {"account_id": "bigint"})
    assert typed["reason"] == "type_changed"
    clock = cap.Scheduler()
    try:
        clock.install({"config_id": "bad", "sync": "cdc", "every_ms": 1000, "window_ms": 1000, "consistency": "transactional"}, 0)
        raise AssertionError("cdc config installed")
    except cap.ScheduleError as exc:
        assert exc.reason == "sync_refused"


def test_commit_keeps_old_token_on_expiry() -> None:
    clock = cap.Scheduler()
    clock.install({"config_id": "user-fast", "sync": "partition_refresh", "every_ms": 8000, "window_ms": 6000}, 0)
    assert clock.begin_tick(8000)["admitted"]
    initial = parts([("p202605", "1"), ("p202606", "1")])
    clock.commit_tick(8100, {
        "dup_events": {
            "advance": True,
            "action": "initial_full",
            "token": cap.token_of(initial),
            "partitions": initial,
            "method": "C5",
            "grade": "S0",
            "quality": [],
            "trusted_after": False,
            "continue_old": False,
        }
    })
    old = clock.watermarks["dup_events"]["token"]
    assert clock.begin_tick(16100)["admitted"]
    nxt = parts([("p202606", "2")])
    done = clock.commit_tick(16200, {
        "dup_events": {
            "advance": True,
            "action": "resnapshot",
            "token": cap.token_of(nxt),
            "partitions": nxt,
            "method": "C5",
            "grade": "B",
            "quality": ["POSITION_EXPIRED"],
            "trusted_after": False,
            "continue_old": False,
        }
    })
    assert done["advanced"] is True
    mark = clock.watermarks["dup_events"]
    assert mark["previous_token"] == old and mark["continued_from_old"] is False and mark["token"] != old


def main() -> None:
    test_refusals_before_read()
    test_two_periods_do_not_overlap()
    test_window_and_stale_do_not_advance()
    test_c4_replace_and_c5_delete_stays_gone()
    test_equal_version_skip_and_expiry()
    test_schema_guard_and_transactional_config()
    test_commit_keeps_old_token_on_expiry()
    print("capture_schedule tests ok")


if __name__ == "__main__":
    main()
