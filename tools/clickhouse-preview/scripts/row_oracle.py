#!/usr/bin/env python3
"""Compare operator rows with a ClickHouse source extract.

Expected numbers are parsed from the source text. This module does not call
the product encoder, does not rewrite gold, and refuses to replace a
preserved verdict named independent-verdict.txt.
"""

from __future__ import annotations

import argparse
import json
from decimal import Context, Decimal, localcontext
from pathlib import Path

COLUMNS = (
    "reading_id",
    "version",
    "amount",
    "note",
    "code",
    "code_or_null",
    "qty",
    "label",
    "wide_u64",
)
NUMERIC = frozenset({"reading_id", "version", "amount", "wide_u64"})
BYTEA = frozenset({"note", "code", "code_or_null", "label"})
QTY_KIND = "int8"
PRESERVED_VERDICT_NAME = "independent-verdict.txt"


def parse_source_decimal(raw: object) -> tuple[int, int]:
    text = str(raw).strip()
    if text.startswith("+"):
        text = text[1:]
    negative = text.startswith("-")
    body = text[1:] if negative else text
    if body.count(".") > 1 or body in {"", "."}:
        raise ValueError("decimal")
    whole, frac = body.split(".", 1) if "." in body else (body, "")
    if whole == "":
        whole = "0"
    if not whole.isdigit() or (frac and not frac.isdigit()):
        raise ValueError("decimal")
    coefficient = int(whole + frac) if whole or frac else 0
    if negative and coefficient != 0:
        coefficient = -coefficient
    return coefficient, len(frac)


def parse_cell_decimal(cell: dict) -> tuple[int, int]:
    if cell.get("kind") not in {"numeric", "decimal"}:
        raise ValueError("kind")
    digits = cell.get("coefficient")
    if not isinstance(digits, str) or not digits.isdigit() or digits == "":
        raise ValueError("coefficient")
    scale = cell.get("scale")
    if isinstance(scale, bool) or not isinstance(scale, int) or scale < 0:
        raise ValueError("scale")
    if cell.get("negative") is True:
        sign = -1
    elif cell.get("negative") is False:
        sign = 1
    else:
        raise ValueError("negative")
    coefficient = int(digits)
    if coefficient == 0:
        sign = 1
    return sign * coefficient, scale


def same_scaled(left: tuple[int, int], right: tuple[int, int]) -> bool:
    left_coefficient, left_scale = left
    right_coefficient, right_scale = right
    if left_scale >= right_scale:
        return left_coefficient == right_coefficient * (10 ** (left_scale - right_scale))
    return left_coefficient * (10 ** (right_scale - left_scale)) == right_coefficient


def high_precision_equal(cell: dict, raw: object) -> bool:
    scale = int(cell["scale"])
    precision = max(76 + scale, len(str(cell["coefficient"])) + scale + 2)
    with localcontext(Context(prec=precision)):
        coefficient = Decimal(cell["coefficient"])
        if cell.get("negative") is True:
            coefficient = -coefficient
        got = coefficient.scaleb(-scale)
    expected = Decimal(str(raw))
    return got == expected


CUSTOMER_COLUMNS = ("reading_id", "ts", "amount", "note", "qty")
CUSTOMER_NUMERIC = frozenset({"reading_id", "ts", "amount"})
CUSTOMER_BYTEA = frozenset({"note"})
PROFILES = {
    "faithful": (COLUMNS, NUMERIC, BYTEA),
    "customer": (CUSTOMER_COLUMNS, CUSTOMER_NUMERIC, CUSTOMER_BYTEA),
}


def resolve_profile(profile: str) -> tuple[tuple[str, ...], frozenset[str], frozenset[str]]:
    try:
        return PROFILES[profile]
    except KeyError as error:
        raise ValueError(profile) from error


def compare(operator: dict, source_rows: list[dict], profile: str = "faithful") -> tuple[list[str], int]:
    columns, numeric, bytea = resolve_profile(profile)
    problems: list[str] = []
    rows = operator.get("rows")
    if operator.get("outcome") != "succeeded":
        problems.append("outcome")
    if not isinstance(rows, list) or len(rows) != len(source_rows) or len(source_rows) != 3:
        shown = len(rows) if isinstance(rows, list) else "none"
        problems.append(f"row-count operator={shown} source={len(source_rows)}")
        return problems, 0
    nulls = 0
    for index, (got, expect) in enumerate(zip(rows, source_rows)):
        if not isinstance(got, list) or len(got) != len(columns):
            problems.append(f"width-{index}")
            continue
        if not isinstance(expect, dict):
            problems.append(f"source-{index}")
            continue
        for name, cell in zip(columns, got):
            if not isinstance(cell, dict) or name not in expect:
                problems.append(f"cell-{name}-{index}")
                continue
            raw = expect[name]
            if name in bytea:
                if raw is None:
                    nulls += 1
                    if cell.get("kind") != "null":
                        problems.append(f"null-{name}-{index}")
                elif cell.get("kind") != "bytea" or str(cell.get("value", "")).lower() != str(raw).lower():
                    problems.append(f"bytea-{name}-{index}")
            elif name == "qty":
                value = cell.get("value")
                if cell.get("kind") != QTY_KIND or not isinstance(value, str) or value != str(raw):
                    problems.append(f"qty-{index}")
            elif name in numeric:
                try:
                    exact = same_scaled(parse_cell_decimal(cell), parse_source_decimal(raw))
                    precise = high_precision_equal(cell, raw)
                except (KeyError, TypeError, ValueError, ArithmeticError):
                    problems.append(f"numeric-shape-{name}-{index}")
                    continue
                if exact != precise:
                    problems.append(f"oracle-internal-{name}-{index}")
                elif not exact:
                    problems.append(f"numeric-{name}-{index}")
    return problems, nulls


def verdict_text(problems: list[str], nulls: int) -> str:
    if problems:
        head = "MISMATCHES " + " ".join(problems) + "\n"
    else:
        head = "MATCH\n"
    return head + f"null_cells {nulls}\n"


def load_source(path: Path) -> list[dict]:
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.strip():
            rows.append(json.loads(line))
    return rows


def write_verdict(path: Path, text: str) -> None:
    if path.name == PRESERVED_VERDICT_NAME:
        raise SystemExit("refusing to overwrite the preserved verdict")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--operator", required=True, type=Path)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--verdict", required=True, type=Path)
    parser.add_argument("--profile", default="faithful", choices=sorted(PROFILES))
    args = parser.parse_args()
    operator = json.loads(args.operator.read_text(encoding="utf-8"))
    problems, nulls = compare(operator, load_source(args.source), args.profile)
    text = verdict_text(problems, nulls)
    write_verdict(args.verdict, text)
    print(text, end="")
    return 1 if problems else 0


if __name__ == "__main__":
    raise SystemExit(main())
