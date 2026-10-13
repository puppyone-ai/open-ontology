#!/usr/bin/env python3
"""Compare one operator-preview query result with the frozen decoded shipment rows."""

from __future__ import annotations

import json
import sys
from decimal import Decimal
from pathlib import Path


COLUMNS = [
    "_id",
    "code",
    "flag",
    "units",
    "price",
    "created",
    "uid",
    "blob",
    "blob_presence",
    "weight",
    "warehouse",
    "lane",
    "memo",
    "memo_presence",
    "marks",
    "marks_presence",
    "pack",
    "pack_presence",
]


def expect_lane(row: dict) -> str:
    if row["lane_tag"] == "utf8":
        value = json.dumps(row["lane_text"], ensure_ascii=False)
        return f'{{"type_id":0,"type":"utf8","value":{value}}}'
    return f'{{"type_id":1,"type":"int64","value":{row["lane_num"]}}}'


def expect_marks(row: dict) -> str | None:
    if row["marks"] is None:
        return None
    return json.dumps(row["marks"], ensure_ascii=False, separators=(",", ":"))


def expect_pack(row: dict) -> str | None:
    if row["pack_presence"] < 2:
        return None
    kind = "null" if row["pack_kind"] is None else json.dumps(row["pack_kind"], ensure_ascii=False)
    number = "null" if row["pack_n"] is None else str(row["pack_n"])
    return f'{{"kind":{kind},"n":{number}}}'


def cell_text(cell: dict) -> str:
    kind = cell.get("kind")
    if kind == "null":
        return ""
    if kind == "numeric":
        return str(cell.get("coefficient"))
    return str(cell.get("value"))


def main() -> int:
    result_path = Path(sys.argv[1])
    decoded_path = Path(sys.argv[2])
    result = json.loads(result_path.read_text())
    decoded = json.loads(decoded_path.read_text())
    mismatches = []
    if result.get("outcome") != "succeeded":
        mismatches.append(f"outcome {result.get('outcome')}")
    if result.get("truncated") is not False:
        mismatches.append("truncated")
    rows = result.get("rows")
    records = decoded.get("records")
    if not isinstance(rows, list) or not isinstance(records, list):
        mismatches.append("rows missing")
        rows = []
        records = []
    if len(rows) != 16 or len(records) != 16:
        mismatches.append(f"row count {len(rows)} decoded {len(records)}")
    by_id = {}
    for record in records:
        by_id[str(record["_id"])] = record
    seen = set()
    for row in rows:
        if not isinstance(row, list) or len(row) != len(COLUMNS):
            mismatches.append(f"width {len(row) if isinstance(row, list) else 'na'}")
            continue
        values = {name: cell_text(cell) for name, cell in zip(COLUMNS, row)}
        ident = values["_id"]
        seen.add(ident)
        record = by_id.get(ident)
        if record is None:
            mismatches.append(f"unknown _id {ident}")
            continue
        if len(ident) != 24:
            mismatches.append(f"_id width {ident}")
        if values["uid"] != str(record["uid_hex"]) or len(values["uid"]) != 32:
            mismatches.append(f"uid {ident}")
        if values["blob"] != str(record["blob_hex"]):
            mismatches.append(f"blob {ident}")
        if values["code"] != str(record["code"]):
            mismatches.append(f"code {ident}")
        if values["flag"] != str(record["flag"]) or values["units"] != str(record["units"]):
            mismatches.append(f"ints {ident}")
        if values["warehouse"] != str(record["warehouse"]):
            mismatches.append(f"warehouse {ident}")
        for name in ("blob_presence", "memo_presence", "marks_presence", "pack_presence"):
            if values[name] != str(record[name]):
                mismatches.append(f"{name} {ident}")
        if record["memo"] is None:
            if row[COLUMNS.index("memo")].get("kind") != "null":
                mismatches.append(f"memo {ident}")
        elif values["memo"] != str(record["memo"]):
            mismatches.append(f"memo {ident}")
        created = row[COLUMNS.index("created")]
        # Native timestamptz micros count from 2000-01-01, the PostgreSQL epoch.
        micros = int(created.get("micros"))
        unix_micros = micros + 946_684_800_000_000
        expect = int(record["created_ms"]) * 1000
        if created.get("kind") != "timestamptz" or unix_micros != expect:
            mismatches.append(f"created {ident} {micros} {expect}")
        price = row[COLUMNS.index("price")]
        coefficient = str(price.get("coefficient"))
        if coefficient != str(record["price"]):
            mismatches.append(f"price {ident} {coefficient}")
        if int(price.get("scale", -1)) != 7:
            mismatches.append(f"price scale {ident}")
        weight = row[COLUMNS.index("weight")]
        if weight.get("kind") != "float8" or Decimal(str(weight.get("value"))) != Decimal(str(record["weight"])):
            mismatches.append(f"weight {ident} {weight.get('value')}")
        expected_json = {
            "lane": expect_lane(record),
            "marks": expect_marks(record),
            "pack": expect_pack(record),
        }
        for name, expected in expected_json.items():
            cell = row[COLUMNS.index(name)]
            if expected is None:
                if cell.get("kind") != "null":
                    mismatches.append(f"json null {name} {ident}")
            elif cell.get("kind") != "json" or cell.get("value") != expected:
                mismatches.append(f"json {name} {ident}")
        for name in ("_id", "uid", "blob"):
            got = row[COLUMNS.index(name)].get("kind")
            if got != "bytea":
                mismatches.append(f"kind {name} {got}")
    if seen != set(by_id):
        mismatches.append("id set")
    print(f"MISMATCHES {len(mismatches)}")
    for item in mismatches:
        print(item)
    return 0 if not mismatches else 1


if __name__ == "__main__":
    raise SystemExit(main())
