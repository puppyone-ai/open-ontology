#!/usr/bin/env python3
"""Customer files choose the object and the fields. readings is one input."""

import json
import unittest
from pathlib import Path

import customer_case as cc

RUN = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"


def customer(schema: str, table: str, fields: list[dict], database: str = "oo_dl01w40") -> dict:
    return {
        "source_id": "clickhouse_268",
        "secret_ref": "secret://oo-dl01w1-40/reader",
        "budget": {"unit": "Rows", "hard_limit": 100000, "confidence": "unknown"},
        "role": "oo_r_reader",
        "model": {"model_id": "grok-4.7", "origin": "http://127.0.0.1:8317"},
        "approved_objects": [
            {
                "database": database,
                "table": table,
                "logical_schema": schema,
                "logical_table": "final",
                "fields": fields,
            }
        ],
    }


def fields_named() -> list[dict]:
    return [
        {"name": "reading_id", "source_type": "uint64", "key": True},
        {"name": "ts", "source_type": "timestamp_nanos_utc"},
        {"name": "amount", "source_type": "decimal76_10"},
        {"name": "note", "source_type": "nullable_string"},
        {"name": "qty", "source_type": "int64"},
    ]


class CustomerCaseTest(unittest.TestCase):
    def test_logical_names_come_from_the_customer_file(self) -> None:
        body = cc.plan(
            customer("meter_window", "batches", fields_named(), database="oo_caller"),
            {"object": {"database": "oo_caller", "table": "batches"}, "fields": [item["name"] for item in fields_named()]},
            3,
            RUN,
            "/run/invalid-bearer",
        )
        self.assertTrue(body["admitted"])
        self.assertEqual(body["dataset"], "meter_window.final")
        self.assertNotIn("readings", body["sql"])
        self.assertIn("oo_caller.batches", body["sql"])
        self.assertEqual(body["catalog"]["tables"][0]["logical_schema"], "meter_window")
        self.assertNotIn("secret://", json.dumps(body))

    def test_unapproved_field_is_refused_before_sql(self) -> None:
        body = cc.plan(
            customer("readings", "readings", fields_named()),
            {"object": {"database": "oo_dl01w40", "table": "readings"}, "fields": ["reading_id", "labels"]},
            3,
            RUN,
            "/run/invalid-bearer",
        )
        self.assertFalse(body["admitted"])
        self.assertEqual(body["reason"], "unapproved_field")
        self.assertFalse(body["source_read"])
        self.assertIsNone(body["sql"])

    def test_unapproved_object_is_refused_before_sql(self) -> None:
        body = cc.plan(
            customer("readings", "readings", fields_named()),
            {"object": {"database": "oo_dl01w40", "table": "other"}, "fields": ["reading_id"]},
            3,
            RUN,
            "/run/invalid-bearer",
        )
        self.assertEqual(body["reason"], "unapproved_object")
        self.assertIsNone(body["sql"])

    def test_fixed_string_is_undeclared(self) -> None:
        approved = fields_named() + [{"name": "code_fixed", "source_type": "fixed_string"}]
        body = cc.plan(
            customer("readings", "readings", approved),
            {
                "object": {"database": "oo_dl01w40", "table": "readings"},
                "fields": ["reading_id", "code_fixed"],
            },
            3,
            RUN,
            "/run/invalid-bearer",
        )
        self.assertEqual(body["reason"], "capability_undeclared")
        self.assertFalse(body["source_read"])
        self.assertIsNone(body["sql"])

    def test_datetime64_and_array_are_not_success_columns(self) -> None:
        approved = fields_named() + [
            {"name": "ts_raw", "source_type": "datetime64_9_utc"},
            {"name": "tags", "source_type": "array_int64"},
        ]
        for name in ("ts_raw", "tags"):
            body = cc.plan(
                customer("readings", "readings", approved),
                {"object": {"database": "oo_dl01w40", "table": "readings"}, "fields": ["reading_id", name]},
                3,
                RUN,
                "/run/invalid-bearer",
            )
            self.assertEqual(body["reason"], "type_unsupported")
            self.assertIsNone(body["sql"])

    def test_nanos_column_is_numeric_20_0_without_timestamptz(self) -> None:
        body = cc.plan(
            customer("readings", "readings", fields_named()),
            {
                "object": {"database": "oo_dl01w40", "table": "readings"},
                "fields": [item["name"] for item in fields_named()],
            },
            3,
            RUN,
            "/run/invalid-bearer",
        )
        columns = {item["logical_name"]: item for item in body["catalog"]["tables"][0]["columns"]}
        ts = columns["ts"]
        self.assertEqual(ts["source_type"], "timestamp_nanos_utc")
        self.assertEqual(ts["oid"], 1700)
        self.assertEqual(ts["typmod"], cc.rp.numeric_typmod(20, 0))
        self.assertIsNone(ts["caveat"])
        self.assertNotEqual(ts["oid"], 1184)
        self.assertEqual(body["role"], "oo_r_reader")
        self.assertEqual(body["project"], "clickhouse_268")
        self.assertEqual(body["builder_config"]["endpoint"]["model_id"], "grok-4.7")
        self.assertNotIn("secret_ref", body["builder_config"])
        for sql in body["nanos_sql"]:
            self.assertNotIn(" AS ts", sql)
            self.assertIn("toUnixTimestamp64Nano(ts)", sql)
            self.assertIn(" AS nanos", sql)
        text = Path(__file__).with_name("customer_case.py").read_text(encoding="utf-8")
        self.assertNotIn("meter_window", text)


if __name__ == "__main__":
    unittest.main()
