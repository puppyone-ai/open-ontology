#!/usr/bin/env python3
"""Offline checks of the OLAP runner's pins and normalization (no containers)."""
from decimal import Decimal
import unittest

import run


class RunnerTest(unittest.TestCase):
    def test_every_family_resolves_one_arm64_pin(self):
        for family, ids in run.PINS.items():
            for pin_id in ids:
                ref = run.pin(pin_id)
                self.assertRegex(ref, r"^[a-z0-9/-]+@sha256:[0-9a-f]{64}$", family)
        self.assertTrue(run.pin("postgres-17.11-bookworm").startswith("postgres@sha256:"))
        with self.assertRaises(RuntimeError):
            run.pin("doris-9.9.9")

    def test_normalization_is_exact(self):
        self.assertEqual(run.norm("dec", "-9999999999999999999999999999.9999999999", "source"),
                         run.norm("dec", Decimal("-9999999999999999999999999999.99999999990"), "target"))
        self.assertNotEqual(run.norm("dec", "1.0000000001", "source"), run.norm("dec", "1", "target"))
        self.assertEqual(run.norm("f32", "3.4028235e38", "source"), run.norm("f32", 3.4028234663852886e38, "target"))
        self.assertEqual(run.norm("float", "-0", "source"), run.norm("float", Decimal("-0"), "target"))
        self.assertEqual(run.norm("float", "nan", "source"), run.norm("float", "NaN", "target"))
        self.assertEqual(run.norm("float", "inf", "source"), run.norm("float", "Infinity", "target"))
        self.assertEqual(run.norm("bytes", "FF00", "source"), run.norm("bytes", "\\xff00", "target"))
        self.assertEqual(run.norm("ts64", "1500000", "source"), run.norm("ts64", "1970-01-01T00:00:01.5+00:00", "target"))
        self.assertEqual(run.norm("naive", "2024-01-05 01:02:03.123456", "source"),
                         run.norm("naive", "2024-01-05T01:02:03.123456", "target"))
        self.assertEqual(run.norm("bool", "1", "source"), run.norm("bool", True, "target"))
        self.assertIsNone(run.norm("int", None, "source"))

    def test_source_expressions_and_final(self):
        self.assertEqual(run.source_expr("clickhouse", "raw", "bytes"), "hex(`raw`)")
        self.assertEqual(run.source_expr("clickhouse", "ts", "ts64"), "toString(toUnixTimestamp64Micro(`ts`))")
        self.assertEqual(run.source_expr("doris", "j", "text"), "`j`")
        self.assertEqual(run.final("clickhouse", "events"), " FINAL")
        self.assertEqual(run.final("clickhouse", "dup_log"), "")
        self.assertEqual(run.final("doris", "events"), "")
        for family in run.VERSION:
            self.assertIn("events", run.COLUMNS[family])


if __name__ == "__main__":
    unittest.main()
