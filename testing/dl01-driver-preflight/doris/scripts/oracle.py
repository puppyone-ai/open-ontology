#!/usr/bin/env python3
"""Independent typed oracle. Days use Howard Hinnant's days_from_civil."""

import json
from pathlib import Path


def unix_days(year, month, day):
    if month <= 2:
        year -= 1
    era = (year if year >= 0 else year - 399) // 400
    yoe = year - era * 400
    month_part = month - 3 if month > 2 else month + 9
    day_of_year = (153 * month_part + 2) // 5 + day - 1
    day_of_era = yoe * 365 + yoe // 4 - yoe // 100 + day_of_year
    return era * 146097 + day_of_era - 719468


def main():
    checks = {
        (1970, 1, 1): 0,
        (2026, 3, 1): 20513,
        (2026, 3, 11): 20523,
        (2026, 4, 1): 20544,
        (2026, 4, 4): 20547,
        (2026, 4, 18): 20561,
        (2026, 5, 1): 20574,
    }
    for date, expected in checks.items():
        got = unix_days(*date)
        if got != expected:
            raise SystemExit(f"{date} {got} != {expected}")
    wide = "12345678901234567890.123456789012345678"
    digits = wide.replace(".", "")
    if len(digits) != 38 or len(wide.split(".")[1]) != 18:
        raise SystemExit("decimal38 width")
    document = {
        "protocol": "oo.doris.oracle/v0",
        "engine": "apache-doris",
        "select_version_is_product_identity": False,
        "final_keyword_required": False,
        "empty_string_is_null": False,
        "constraint_trust": "absent",
        "saturation_lossless": False,
        "scale_note": "representative typed fixture, not a 100k logistics load",
        "tables": {
            "dup_events": {
                "model": "DUPLICATE",
                "columns": [
                    {"name": "event_id", "kind": "int64"},
                    {"name": "kind", "kind": "utf8"},
                    {"name": "qty", "kind": "decimal", "precision": 10, "scale": 2},
                ],
                "rows": [
                    [11, "p", "4.25"],
                    [11, "q", "8.75"],
                    [12, "twin", "6.50"],
                    [12, "twin", "6.50"],
                    [13, "gap", None],
                ],
            },
            "agg_qty": {
                "model": "AGGREGATE",
                "columns": [
                    {"name": "sku", "kind": "utf8"},
                    {"name": "qty", "kind": "decimal", "precision": 10, "scale": 2},
                    {"name": "note", "kind": "utf8"},
                ],
                "rows": [
                    ["M", "9.75", "later"],
                    ["N", "2.50", "n2"],
                    ["P", None, "p2"],
                    ["Q", "0.00", "q"],
                    ["R", "7.00", None],
                ],
            },
            "uniq_acct": {
                "model": "UNIQUE",
                "columns": [
                    {"name": "id", "kind": "int64"},
                    {"name": "balance", "kind": "decimal", "precision": 18, "scale": 4},
                    {"name": "label", "kind": "utf8"},
                ],
                "rows": [
                    [1, "12.5000", "second"],
                    [2, "3.0000", "keep"],
                    [3, "9.2500", "upd2"],
                ],
            },
            "dec_exact": {
                "model": "DUPLICATE",
                "columns": [
                    {"name": "id", "kind": "int64"},
                    {"name": "a18", "kind": "decimal", "precision": 18, "scale": 4},
                    {"name": "b18", "kind": "decimal", "precision": 18, "scale": 4},
                    {"name": "d38", "kind": "decimal", "precision": 38, "scale": 18},
                ],
                "rows": [
                    [1, "0.3000", None, None],
                    [2, "0.4000", None, None],
                    [3, "0.7000", "-1.2500", wide],
                ],
            },
            "part_day": {
                "model": "DUPLICATE",
                "columns": [
                    {"name": "day", "kind": "date"},
                    {"name": "region", "kind": "utf8"},
                    {"name": "amount", "kind": "decimal", "precision": 12, "scale": 2},
                ],
                "rows": [
                    ["2026-03-11", "west", "33.25"],
                    ["2026-04-04", "north", "44.10"],
                    ["2026-04-18", "south", None],
                ],
            },
            "str_key": {
                "model": "DUPLICATE",
                "columns": [
                    {"name": "k", "kind": "utf8"},
                    {"name": "note", "kind": "utf8"},
                ],
                "rows": [
                    ["", "empty"],
                    ["M", "upper"],
                    ["M ", "trail"],
                    ["N", "en"],
                    ["m", "low"],
                    ["n", "en2"],
                    ["日", "cjk"],
                ],
            },
            "nz_obs": {
                "model": "DUPLICATE",
                "columns": [
                    {"name": "id", "kind": "int64"},
                    {"name": "qty", "kind": "decimal", "precision": 10, "scale": 2},
                ],
                "rows": [
                    [1, None],
                    [2, None],
                    [3, "0.00"],
                ],
            },
        },
        "sum_check": {"table": "dec_exact", "column": "a18", "rows": [0, 1], "into": 2},
        "string_order": ["", "M", "M ", "N", "m", "n", "日"],
        "null_vs_zero": {"nulls": 2, "zeros": 1},
        "partition_bounds": ["2026-03-01", "2026-04-01", "2026-05-01"],
        "half_open": True,
    }
    out = Path(__file__).resolve().parents[1] / "fixtures" / "oracle.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")
    print(out)


if __name__ == "__main__":
    main()
