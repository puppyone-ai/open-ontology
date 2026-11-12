import copy
import unittest

import live
import run as ab


class NumericComparisonTests(unittest.TestCase):
    def result(self, kind, values):
        return live.typed_result({
            "columns": [{"name": "n", "type": kind}],
            "rows": [[v] for v in values], "returned_rows": len(values),
            "truncated": False, "warnings": [],
        })

    def compare(self, left, right, ordered=False):
        return ab.compare_rows({"gold": left, "order_required": ordered},
                               right, "exact-decimal-v1")["pass"]

    def test_exact_equality_is_symmetric_and_opt_in(self):
        for integer, decimal in [
            ("60", "60.0000"), ("-60", "-60.000"),
            ("0", "0.0000"), ("9223372036854775807", "9223372036854775807.00"),
            ("-9223372036854775808", "-9223372036854775808.000"),
        ]:
            a, b = self.result("int64", [integer]), self.result("numeric", [decimal])
            before = copy.deepcopy((a, b))
            self.assertTrue(self.compare(a, b))
            self.assertTrue(self.compare(b, a))
            self.assertFalse(ab.compare_rows({"gold": a}, b)["pass"])
            self.assertEqual((a, b), before)

    def test_no_rounding_text_coercion_or_presence_collapse(self):
        integer = self.result("int64", ["60"])
        for kind, values in [
            ("numeric", ["60.0000000000000000000000000001"]),
            ("text", ["60"]), ("numeric", [None]), ("bool", ["true"]),
        ]:
            self.assertFalse(self.compare(integer, self.result(kind, values)))
        a = self.result("numeric", ["123456789012345678901234567890.01"])
        b = self.result("numeric", ["123456789012345678901234567890.02"])
        self.assertFalse(self.compare(a, b))
        null = self.result("int64", [None])
        for presence in ("missing", "unavailable"):
            other = self.result("numeric", [None])
            other["rows"][0][0]["presence"] = presence
            self.assertFalse(self.compare(null, other))

    def test_multiplicity_order_and_completion_stay_strict(self):
        a = self.result("int64", ["1", "2", "2", None])
        b = self.result("numeric", [None, "2.0", "2.00", "1.0"])
        self.assertTrue(self.compare(a, b))
        self.assertFalse(self.compare(a, b, ordered=True))
        b["rows"].pop()
        self.assertFalse(self.compare(a, b))
        b = self.result("numeric", ["1", "2", "2", None])
        b["completion"] = "partial"
        self.assertFalse(self.compare(a, b))

    def test_invalid_integer_is_not_laundered_by_conversion(self):
        for value in ("9223372036854775808", "-0", "01"):
            a = self.result("int64", ["1"])
            a["rows"][0][0]["value"] = value
            with self.assertRaises(ab.Invalid):
                self.compare(a, self.result("numeric", ["1"]))

    def test_unknown_policy_fails_closed(self):
        dataset = ab.load(ab.ROOT / "benchmarks/ab/fixtures/demo.json")
        dataset["numeric_comparison"] = "float-tolerance"
        with self.assertRaisesRegex(ab.Invalid, "numeric_comparison"):
            ab.validate_dataset(dataset)
