#!/usr/bin/env python3
"""Oracle tests. Expected values are source strings, not product encoder output."""

from __future__ import annotations

import json
import tempfile
import unittest
from decimal import Context, Decimal, localcontext
from pathlib import Path

from row_oracle import compare, verdict_text, write_verdict


def numeric(coefficient: str, scale: int, negative: bool = False) -> dict:
    return {"kind": "numeric", "coefficient": coefficient, "scale": scale, "negative": negative}


def matching_pair() -> tuple[list[dict], dict]:
    source = {
        "reading_id": "1",
        "version": "1",
        "amount": "0",
        "note": None,
        "code": "41",
        "code_or_null": None,
        "qty": "0",
        "label": "61",
        "wide_u64": "1",
    }
    row = [
        numeric("1", 0),
        numeric("1", 0),
        numeric("0", 0),
        {"kind": "null"},
        {"kind": "bytea", "value": "41"},
        {"kind": "null"},
        {"kind": "int8", "value": "0"},
        {"kind": "bytea", "value": "61"},
        numeric("1", 0),
    ]
    return row, source


def operator_of(rows: list[list[dict]]) -> dict:
    return {"outcome": "succeeded", "rows": rows}


WIDE_COEFFICIENT = "1" + ("0" * 48) + "1"
WIDE_RAW = "1" + ("0" * 39) + "." + ("0" * 9) + "1"


class RowOracleTest(unittest.TestCase):
    def test_wide_decimal_matches_source_text(self) -> None:
        self.assertGreater(len(WIDE_COEFFICIENT), 28)
        row, source = matching_pair()
        row[2] = numeric(WIDE_COEFFICIENT, 10)
        source["amount"] = WIDE_RAW
        rows = [row, *self._copies(row)]
        sources = [source, *self._source_copies(source)]
        problems, _nulls = compare(operator_of(rows), sources)
        self.assertEqual(problems, [])

    def test_default_decimal_precision_would_round_this_value(self) -> None:
        cell = numeric(WIDE_COEFFICIENT, 10)
        with localcontext(Context(prec=28)):
            coefficient = Decimal(cell["coefficient"])
            rounded = coefficient.scaleb(-cell["scale"])
        exact = Decimal(WIDE_RAW)
        self.assertNotEqual(rounded, exact)

    def test_last_digit_damage_is_unequal(self) -> None:
        damaged = WIDE_COEFFICIENT[:-1] + "2"
        row, source = matching_pair()
        row[2] = numeric(damaged, 10)
        source["amount"] = WIDE_RAW
        problems, _nulls = compare(operator_of([row, *self._copies(row)]), [source, *self._source_copies(source)])
        self.assertIn("numeric-amount-0", problems)

    def test_outdated_int_kind_is_rejected(self) -> None:
        row, source = matching_pair()
        row[6] = {"kind": "int", "value": "0"}
        problems, _nulls = compare(operator_of(self._three(row)), self._three_source(source))
        self.assertEqual(problems, ["qty-0", "qty-1", "qty-2"])

    def test_wrong_qty_value_is_rejected(self) -> None:
        row, source = matching_pair()
        row[6] = {"kind": "int8", "value": "9"}
        problems, _nulls = compare(operator_of(self._three(row)), self._three_source(source))
        self.assertEqual([item for item in problems if item.startswith("qty-")], ["qty-0", "qty-1", "qty-2"])

    def test_null_and_wrong_type_are_rejected(self) -> None:
        row, source = matching_pair()
        row[3] = {"kind": "bytea", "value": "00"}
        row[2] = {"kind": "text", "value": "0"}
        problems, _nulls = compare(operator_of(self._three(row)), self._three_source(source))
        self.assertIn("null-note-0", problems)
        self.assertIn("numeric-shape-amount-0", problems)
        present, present_source = matching_pair()
        present[3] = {"kind": "null"}
        present_source["note"] = "41"
        present_problems, _nulls = compare(
            operator_of(self._three(present)),
            self._three_source(present_source),
        )
        self.assertIn("bytea-note-0", present_problems)

    def test_negative_scale_alignment_matches(self) -> None:
        row, source = matching_pair()
        row[2] = numeric("425000000000", 10, negative=True)
        source["amount"] = "-42.5"
        problems, _nulls = compare(operator_of(self._three(row)), self._three_source(source))
        self.assertEqual(problems, [])

    def test_preserved_verdict_name_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "independent-verdict.txt"
            with self.assertRaises(SystemExit):
                write_verdict(path, verdict_text([], 0))
            self.assertFalse(path.exists())

    def test_customer_profile_keeps_wide_amount_and_null_note(self) -> None:
        source = {
            "reading_id": "101",
            "ts": "1710504000000000001",
            "amount": WIDE_RAW,
            "note": None,
            "qty": "7",
        }
        row = [
            numeric("101", 0),
            numeric("1710504000000000001", 0),
            numeric(WIDE_COEFFICIENT, 10),
            {"kind": "null"},
            {"kind": "int8", "value": "7"},
        ]
        problems, nulls = compare(operator_of(self._three(row)), self._three_source(source), "customer")
        self.assertEqual(problems, [])
        self.assertEqual(nulls, 3)
        damaged = json.loads(json.dumps(row))
        damaged[2] = numeric(WIDE_COEFFICIENT[:-1] + "2", 10)
        bad, _nulls = compare(
            operator_of(self._three(damaged)),
            self._three_source(source),
            "customer",
        )
        self.assertIn("numeric-amount-0", bad)

    def test_verdict_text_keeps_mismatch_tokens(self) -> None:
        text = verdict_text(["numeric-amount-0", "qty-0"], 2)
        self.assertEqual(text, "MISMATCHES numeric-amount-0 qty-0\nnull_cells 2\n")
        parsed = json.loads('{"outcome":"succeeded","rows":[]}')
        problems, nulls = compare(parsed, [])
        self.assertIn("row-count operator=0 source=0", problems)
        self.assertEqual(nulls, 0)

    def _three(self, row: list[dict]) -> list[list[dict]]:
        return [row, json.loads(json.dumps(row)), json.loads(json.dumps(row))]

    def _three_source(self, source: dict) -> list[dict]:
        return [source, json.loads(json.dumps(source)), json.loads(json.dumps(source))]

    def _copies(self, row: list[dict]) -> list[list[dict]]:
        return self._three(row)[1:]

    def _source_copies(self, source: dict) -> list[dict]:
        return self._three_source(source)[1:]


if __name__ == "__main__":
    unittest.main()
