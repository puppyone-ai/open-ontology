#!/usr/bin/env python3
"""UNIT ONLY. Not capability acceptance. No database and no driver.

Checks the ratio rules, tie-break, null versus missing, and that a bare JSON
number is not a preservation type. The live corpus is judged by scripts/oracle.py.
"""

from __future__ import annotations

import json
import pathlib
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

import oracle  # noqa: E402


class RatioTests(unittest.TestCase):
    def test_boundaries(self):
        self.assertTrue(oracle.presence_ok(90, 100))
        self.assertFalse(oracle.presence_ok(89, 100))
        self.assertTrue(oracle.consistency_ok(99, 100))
        self.assertFalse(oracle.consistency_ok(98, 100))

    def test_null_is_not_missing_and_mixed_is_not_stable(self):
        docs = []
        for i in range(100):
            doc = {"_id": {"$oid": f"{i:024x}"}, "keep": f"k{i}"}
            if i < 98:
                doc["drift"] = "s"
            else:
                doc["drift"] = None
            if i < 89:
                doc["rare"] = "r"
            if i < 50:
                doc["mixed"] = "m"
            else:
                doc["mixed"] = {"$numberLong": str(i)}
            doc["tags"] = ["a"]
            docs.append(doc)
        got = oracle.classify(docs)["fields"]
        self.assertEqual(got["keep"]["bucket"], "stable")
        self.assertEqual(got["drift"]["bucket"], "type_drift")
        self.assertEqual(got["drift"]["dominant_count"], 98)
        self.assertEqual(got["rare"]["bucket"], "unclassified")
        self.assertEqual(got["rare"]["present"], 89)
        self.assertNotIn("rare", docs[89])
        self.assertIsNone(docs[98]["drift"])
        self.assertEqual(got["mixed"]["dominant"], "int64")
        self.assertEqual(got["mixed"]["bucket"], "type_drift")
        self.assertEqual(got["tags"]["bucket"], "retained_in_doc")

    def test_relaxed_number_is_not_int64(self):
        self.assertEqual(oracle.bson_type(9007199254740992), "json_number")
        self.assertEqual(oracle.bson_type({"$numberLong": "9007199254740993"}), "int64")
        self.assertNotEqual(int(float(9007199254740993)), 9007199254740993)

    def test_handwritten_anchors_match_formula(self):
        gold = json.loads((ROOT / "fixtures" / "expected-decisions.json").read_text())
        self.assertEqual(gold["fields"]["note_null_boundary"]["dominant_count"], 99)
        self.assertEqual(gold["fields"]["note_drift"]["dominant_count"], 98)
        self.assertEqual(gold["fields"]["rare_code"]["present"], 89)
        self.assertEqual(gold["fields"]["mixed"]["dominant"], "int64")
        self.assertEqual(oracle.expected_price(3), gold["anchors"]["3"]["price"]["decimal"])
        self.assertEqual(oracle.expected_price(0), "NaN")
        self.assertEqual(str(9007199254740993 + 99), gold["anchors"]["99"]["units"])
        self.assertEqual(oracle.expected_id(0), "64b0b1c0e1d2f3a400000000")


if __name__ == "__main__":
    unittest.main()
