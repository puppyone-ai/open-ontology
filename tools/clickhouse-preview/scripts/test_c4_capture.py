#!/usr/bin/env python3
"""Refusal and capture behavior for the ClickHouse source adapter."""

import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path

import c4_capture as cap

MOMENT = datetime(2026, 10, 11, 12, 0, tzinfo=timezone.utc)
COLUMNS = [
    {"name": "reading_id", "type": "UInt64"},
    {"name": "version", "type": "UInt64"},
    {"name": "amount", "type": "Decimal(76, 10)"},
    {"name": "note", "type": "Nullable(String)"},
    {"name": "code", "type": "LowCardinality(String)"},
    {"name": "code_or_null", "type": "LowCardinality(Nullable(String))"},
    {"name": "ts", "type": "DateTime64(9, 'UTC')"},
    {"name": "tags", "type": "Array(Int64)"},
    {"name": "labels", "type": "Array(Nullable(String))"},
    {"name": "qty", "type": "Int64"},
    {"name": "label", "type": "String"},
    {"name": "wide_u64", "type": "UInt64"},
]


def customer() -> dict:
    return {
        "source_id": "clickhouse_268",
        "secret_ref": "secret://oo-dl01w1-40/reader",
        "budget": {"unit": "Rows", "hard_limit": 100000, "confidence": "unknown"},
        "role": "oo_r_reader",
        "model": {"model_id": "grok-4.7", "origin": "http://127.0.0.1:8317"},
        "approved_objects": [
            {
                "database": "oo_dl01w40",
                "table": "readings",
                "logical_schema": "readings",
                "logical_table": "final",
                "fields": [
                    {"name": "reading_id", "source_type": "uint64", "key": True},
                    {"name": "ts", "source_type": "timestamp_nanos_utc"},
                    {"name": "amount", "source_type": "decimal76_10"},
                    {"name": "note", "source_type": "nullable_string"},
                    {"name": "qty", "source_type": "int64"},
                    {"name": "code_fixed", "source_type": "fixed_string"},
                    {"name": "ts_raw", "source_type": "datetime64_9_utc"},
                    {"name": "tags", "source_type": "array_int64"},
                ],
            }
        ],
    }


def capture(**overrides) -> dict:
    body = {
        "connector": "clickhouse",
        "sync": "partition_refresh",
        "every": "3s",
        "consistency": "per_table",
        "credential": "secret://oo-dl01w1-40/reader",
        "locale": "en-US",
        "budget": {"unit": "Rows", "hard_limit": 100000},
        "window": None,
        "capabilities": ["C4", "C5", "C3", "Catalog", "BatchFull"],
        "license": "not_required",
        "cdc": None,
        "delete_detection": {"mode": "partition_replace"},
        "schema_columns": COLUMNS,
        "untrusted_partition_metadata": False,
        "object": {"database": "oo_dl01w40", "table": "readings"},
        "fields": ["reading_id", "ts", "amount", "note", "qty"],
    }
    body.update(overrides)
    return body


def row(reading_id: int, partition: str, amount: str = "1.0000000000", ts: int = 10) -> dict:
    return {
        "reading_id": reading_id,
        "partition_id": partition,
        "ts": ts,
        "amount": amount,
        "note": None,
        "qty": 1,
    }


class MemorySource:
    def __init__(self) -> None:
        self.partitions = {
            "202401": [row(102, "202401", "0.0000000000", 1), row(103, "202401", "-42.5000000000", 2)],
            "202403": [row(101, "202403", "9.0000000000", 3)],
        }
        self.signatures = {"202401": "a", "202403": "b"}
        self.uuid = "table-1"
        self.fingerprint = cap.fingerprint(COLUMNS)
        self.mutation_pending = False
        self.mutation_is_done = 1
        self.statements = ["SELECT partition FROM system.parts"]
        self.reads = []

    def inspect(self) -> dict:
        self.reads.append("inspect")
        return {
            "uuid": self.uuid,
            "fingerprint": self.fingerprint,
            "mutation_pending": self.mutation_pending,
            "mutation_is_done": self.mutation_is_done,
            "partitions": [
                {"id": key, "signature": self.signatures[key]} for key in sorted(self.partitions)
            ],
        }

    def read_partition(self, partition: str) -> list[dict]:
        self.reads.append(partition)
        return list(self.partitions.get(partition, []))

    def read_all(self) -> list[dict]:
        self.reads.append("all")
        rows = []
        for key in sorted(self.partitions):
            rows.extend(self.partitions[key])
        return rows

    def read_since(self, cursor, overlap) -> list[dict]:
        self.reads.append(("since", cursor, overlap))
        floor = -1 if cursor is None else int(cursor) - int(float(overlap) * 1_000_000_000)
        return [item for item in self.read_all() if int(item["ts"]) >= floor]

    def count(self) -> int:
        self.reads.append("count")
        return sum(len(items) for items in self.partitions.values())


class MemoryShadow:
    def __init__(self) -> None:
        self.published: dict[int, dict] = {}
        self.generation = 0
        self.open = False
        self.rolled_back = 0
        self.replaces = 0

    def begin(self) -> None:
        if self.open:
            raise RuntimeError("transaction already open")
        self.open = True
        self.staged = dict(self.published)

    def replace_partition(self, partition: str, rows: list[dict]) -> None:
        if not self.open:
            raise RuntimeError("no transaction")
        self.staged = {key: value for key, value in self.staged.items() if value.get("partition_id") != partition}
        for item in rows:
            self.staged[item["reading_id"]] = item
        self.replaces += 1

    def replace_all(self, rows: list[dict]) -> None:
        if not self.open:
            raise RuntimeError("no transaction")
        self.staged = {item["reading_id"]: item for item in rows}
        self.replaces += 1

    def upsert(self, rows: list[dict]) -> None:
        if not self.open:
            raise RuntimeError("no transaction")
        for item in rows:
            self.staged[item["reading_id"]] = item
        self.replaces += 1

    def commit(self) -> int:
        if not self.open:
            raise RuntimeError("no transaction")
        self.published = self.staged
        self.open = False
        self.generation += 1
        return self.generation

    def rollback(self) -> None:
        self.open = False
        self.staged = None
        self.rolled_back += 1

    def rows(self) -> list[dict]:
        return list(self.published.values())


class CaptureTest(unittest.TestCase):
    def setUp(self) -> None:
        self.secret = tempfile.NamedTemporaryFile()
        self.secret.write(b"x")
        self.secret.flush()

    def tearDown(self) -> None:
        self.secret.close()

    def plan(self, **overrides):
        return cap.gate(customer(), capture(**overrides), self.secret.name, True)

    def test_cdc_auto_and_missing_secret_do_not_read(self) -> None:
        for sync, reason in (("cdc", "sync_cdc"), ("auto", "sync_auto"), ("change_feed", "sync_cdc")):
            body = self.plan(sync=sync)
            self.assertFalse(body["admitted"])
            self.assertEqual(body["reason"], reason)
            self.assertFalse(body["source_read"])
            self.assertIsNone(body["sql"])
        missing = cap.gate(customer(), capture(), "/tmp", False)
        self.assertEqual(missing["reason"], "secret_path_missing")
        self.assertFalse(missing["source_read"])
        directory = tempfile.TemporaryDirectory()
        as_dir = cap.gate(customer(), capture(), directory.name, False)
        self.assertEqual(as_dir["reason"], "secret_path_missing")
        directory.cleanup()

    def test_transactional_license_capability_and_unapproved_field(self) -> None:
        self.assertEqual(self.plan(consistency="transactional")["reason"], "consistency_transactional")
        self.assertEqual(self.plan(license="unknown")["reason"], "license_unknown")
        self.assertEqual(self.plan(capabilities=["Catalog"])["reason"], "capability_unknown")
        self.assertEqual(self.plan(cdc={"slot": "x"})["reason"], "cdc_settings")
        refused = self.plan(fields=["reading_id", "code_fixed"])
        self.assertEqual(refused["reason"], "capability_undeclared")
        self.assertIsNone(refused["sql"])
        self.assertFalse(refused["source_read"])

    def test_periods_differ_and_window_blocks_before_read(self) -> None:
        slept = []

        def sleep(seconds: float) -> None:
            slept.append(seconds)

        clock = {"now": 100.0}

        def now() -> float:
            clock["now"] += 0
            value = clock["now"]
            return value

        def sleep_advance(seconds: float) -> None:
            slept.append(seconds)
            clock["now"] += seconds

        first = cap.observe_period("3s", sleep_advance, now)
        second = cap.observe_period("7s", sleep_advance, now)
        self.assertEqual(slept, [3.0, 7.0])
        self.assertEqual(first, 3.0)
        self.assertEqual(second, 7.0)
        source = MemorySource()
        blocked = cap.cycle(self.plan(window="20:00-08:00"), source, MemoryShadow(), None, MOMENT)
        self.assertEqual(blocked["reason"], "outside_window")
        self.assertFalse(blocked["source_read"])
        self.assertEqual(source.reads, [])

    def test_partition_refresh_catches_insert_update_and_delete(self) -> None:
        source = MemorySource()
        shadow = MemoryShadow()
        plan = self.plan()
        first = cap.cycle(plan, source, shadow, None, MOMENT)
        self.assertTrue(first["published"])
        self.assertEqual(first["method"], "C4")
        self.assertEqual(len(first["rows"]), 3)
        self.assertIn("not_log_cdc", first["disclosure"])
        second = cap.cycle(plan, source, shadow, first["checkpoint"], MOMENT)
        self.assertEqual(second["reason"], "unchanged")
        self.assertEqual(second["replaces"], 0)
        self.assertEqual(shadow.generation, 1)
        source.partitions["202402"] = [row(104, "202402", "4.0000000000", 4)]
        source.signatures["202402"] = "c"
        inserted = cap.cycle(plan, source, shadow, first["checkpoint"], MOMENT)
        self.assertEqual(sorted(row["reading_id"] for row in inserted["rows"]), [101, 102, 103, 104])
        source.partitions["202403"] = [row(101, "202403", "8.0000000000", 5)]
        source.signatures["202403"] = "d"
        updated = cap.cycle(plan, source, shadow, inserted["checkpoint"], MOMENT)
        amounts = {row["reading_id"]: row["amount"] for row in updated["rows"]}
        self.assertEqual(amounts[101], "8.0000000000")
        self.assertNotIn(row(101, "202403", "9.0000000000", 3)["amount"], [amounts[101]])
        del source.partitions["202401"][0]
        source.signatures["202401"] = "e"
        deleted = cap.cycle(plan, source, shadow, updated["checkpoint"], MOMENT)
        identities = sorted(row["reading_id"] for row in deleted["rows"])
        self.assertEqual(identities, [101, 103, 104])
        again = cap.cycle(plan, source, shadow, deleted["checkpoint"], MOMENT)
        self.assertEqual(again["reason"], "unchanged")
        self.assertEqual(sorted(row["reading_id"] for row in again["rows"]), [101, 103, 104])

    def test_pending_mutation_schema_expiry_and_interrupt(self) -> None:
        source = MemorySource()
        shadow = MemoryShadow()
        plan = self.plan()
        source.mutation_pending = True
        source.mutation_is_done = 0
        pending = cap.cycle(plan, source, shadow, None, MOMENT)
        self.assertEqual(pending["reason"], "mutation_pending")
        self.assertFalse(pending["published"])
        self.assertEqual(shadow.generation, 0)
        self.assertIn("DELETES_LAGGED", pending["quality"])
        source.mutation_pending = False
        source.fingerprint = "reading_id\tInt32"
        changed = cap.cycle(plan, source, shadow, None, MOMENT)
        self.assertEqual(changed["reason"], "schema_changed")
        self.assertEqual(shadow.generation, 0)
        source.fingerprint = cap.fingerprint(COLUMNS + [{"name": "extra_c4", "type": "String"}])
        added = cap.cycle(plan, source, shadow, None, MOMENT)
        self.assertEqual(added["reason"], "schema_changed")
        source.fingerprint = cap.fingerprint(COLUMNS)
        first = cap.cycle(plan, source, shadow, None, MOMENT)
        source.uuid = "table-2"
        expired = cap.cycle(plan, source, shadow, first["checkpoint"], MOMENT)
        self.assertTrue(expired["published"])
        self.assertEqual(expired["method"], "C5")
        self.assertIn("POSITION_EXPIRED", expired["quality"])
        source.uuid = "table-2"

        def cancel() -> None:
            raise cap.Cancelled()

        source.signatures["202403"] = "z"
        stopped = cap.cycle(plan, source, shadow, expired["checkpoint"], MOMENT, before_commit=cancel)
        self.assertTrue(stopped["rolled_back"])
        self.assertFalse(stopped["published"])
        self.assertEqual(shadow.rolled_back, 1)
        self.assertEqual(shadow.generation, expired["generation"])

    def test_c5_budget_and_explicit_c3(self) -> None:
        source = MemorySource()
        shadow = MemoryShadow()
        plan = self.plan(sync="full", delete_detection={"mode": "reconcile"})
        full = cap.cycle(plan, source, shadow, None, MOMENT)
        self.assertEqual(full["method"], "C5")
        self.assertEqual(len(full["rows"]), 3)
        untrusted = self.plan(untrusted_partition_metadata=True)
        fallback = cap.cycle(untrusted, source, MemoryShadow(), None, MOMENT)
        self.assertEqual(fallback["method"], "C5")
        tight = self.plan(budget={"unit": "Rows", "hard_limit": 1})
        exhausted = cap.cycle(tight, source, MemoryShadow(), None, MOMENT)
        self.assertEqual(exhausted["reason"], "SOURCE_BUDGET_EXHAUSTED")
        self.assertFalse(exhausted["published"])
        refused = self.plan(sync="incremental")
        self.assertEqual(refused["reason"], "cursor_unsafe")
        self.assertFalse(refused["source_read"])
        explicit = self.plan(
            sync="incremental",
            admin_confirmed=True,
            cursor="ts",
            cursor_safety="statement_time",
            overlap="5m",
            delete_detection={"mode": "reconcile", "every": "24h"},
        )
        self.assertTrue(explicit["admitted"])
        shadow = MemoryShadow()
        c3 = cap.cycle(explicit, source, shadow, None, MOMENT)
        self.assertEqual(c3["method"], "C3")
        self.assertEqual(c3["mode"], "initial")
        self.assertIn("DELETES_LAGGED", c3["quality"])
        source.partitions["202403"] = [row(101, "202403", "9.0000000000", 9)]
        follow = cap.cycle(explicit, source, shadow, c3["checkpoint"], MOMENT)
        self.assertEqual(follow["mode"], "incremental")
        self.assertEqual(sorted(item["reading_id"] for item in follow["rows"]), [101, 102, 103])
        self.assertTrue(any(item["reading_id"] == 101 and item["ts"] == 9 for item in follow["rows"]))

    def test_known_arrow_matches_the_fixture_rows(self) -> None:
        path = Path(
            "/Users/redux/puppy-one/open-ontology/evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/"
            "DL01/clickhouse-product/raw/run/product-run-12-customer-chain/out/raw/customer-five.arrow"
        )
        import arrow_stream

        decoded = arrow_stream.rows_from_stream(path.read_bytes())
        self.assertEqual([item["reading_id"] for item in decoded], [101, 102, 103])
        self.assertIsNone(decoded[0]["note"])
        self.assertEqual(decoded[1]["note"], b"")
        self.assertEqual(decoded[2]["note"], b"present")
        self.assertEqual(decoded[0]["amount"], "1234567890123456789012345678901234567890.1234567890")
        self.assertEqual(decoded[2]["qty"], 0)


if __name__ == "__main__":
    unittest.main()
