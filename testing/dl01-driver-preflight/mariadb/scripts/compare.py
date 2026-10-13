#!/usr/bin/env python3
"""Compare the independent oracle, the decoded Arrow rows, and the frozen types.tsv."""

import json
import pathlib
import sys

PHYSICAL = {
    "int8": "int8",
    "int16": "int16",
    "int32": "int32",
    "int64": "int64",
    "uint8": "uint8",
    "uint16": "uint16",
    "uint32": "uint32",
    "uint64": "uint64",
    "utf8": "utf8",
    "binary": "binary",
    "date32": "date32",
    "timestamp_us_naive": "timestamp_us_naive",
    "timestamp_us_+00:00": "timestamp_us_+00:00",
    "int64_interval_us": "int64",
}


def load(path: pathlib.Path):
    return json.loads(path.read_text(encoding="utf-8"))


def cell(value, binary=False):
    if value is None:
        return "<NULL>"
    if binary and value == "":
        return "<EMPTY>"
    return value


def tsv_from_decoded(decoded: dict, kinds: list) -> list[str]:
    tables = {item["name"]: item for item in decoded["tables"]}
    lines = []
    for row in tables["k_unsigned"]["rows"]:
        lines.append(f"unsigned|{row[0]}|{row[1]}")
    for row in tables["k_decimal"]["rows"]:
        lines.append(f"decimal|{row[0]}|{row[1]}")
    for row in tables["k_null"]["rows"]:
        lines.append("|".join([
            "null",
            cell(row[0]),
            cell(row[1]),
            cell(row[2]),
            cell(row[3]),
            cell(row[4]),
            cell(row[5]),
            cell(row[6], binary=True),
        ]))
    for row in tables["k_null_unique"]["rows"]:
        lines.append(f"null_unique|{cell(row[0])}|{cell(row[1])}")
    kind_by_id = {item[0]: item for item in kinds}
    for row in tables["k_json"]["rows"]:
        kind = kind_by_id[row[0]]
        doc = "<SQLNULL>" if row[1] is None else row[1]
        kind_text = "<SQLNULL>" if kind[1] is None else kind[1]
        valid = "<NULL>" if kind[2] is None else kind[2]
        lines.append(f"json|{row[0]}|{doc}|{kind_text}|{valid}")
    for row in tables["k_temporal"]["rows"]:
        lines.append("|".join(["temporal", cell(row[0]), cell(row[1]), cell(row[2]), cell(row[3]), cell(row[4])]))
    for row in tables["k_binary"]["rows"]:
        lines.append(f"binary|{cell(row[0], binary=True)}|{row[1]}")
    return lines


def main() -> int:
    if len(sys.argv) != 5:
        print("usage: compare.py DECODED ORACLE CHECKS TYPES_TSV", file=sys.stderr)
        return 2
    decoded = load(pathlib.Path(sys.argv[1]))
    oracle = load(pathlib.Path(sys.argv[2]))
    checks = load(pathlib.Path(sys.argv[3]))
    expected = [line for line in pathlib.Path(sys.argv[4]).read_text(encoding="utf-8").splitlines() if line]
    failed = 0
    if decoded.get("protocol") != oracle.get("protocol"):
        failed += 1
        print("protocol mismatch")
    got = {item["name"]: item for item in decoded["tables"]}
    want = {item["name"]: item for item in oracle["tables"]}
    if set(got) != set(want):
        failed += 1
        print(f"table set decoded={sorted(got)} oracle={sorted(want)}")
    for name, spec in sorted(want.items()):
        table = got.get(name)
        if table is None:
            continue
        if table["rows"] != spec["rows"]:
            failed += 1
            print(f"row mismatch {name} decoded={table['rows']!r} oracle={spec['rows']!r}")
        if table["arrow"] != spec["arrow"]:
            failed += 1
            print(f"arrow mismatch {name} decoded={table['arrow']} oracle={spec['arrow']}")
        for tag, physical in zip(table["arrow"], table["physical"]):
            if tag.startswith("decimal128") or tag.startswith("decimal256"):
                if physical != tag:
                    failed += 1
                    print(f"physical decimal {name} {tag} != {physical}")
                continue
            if PHYSICAL.get(tag) != physical:
                failed += 1
                print(f"physical mismatch {name} {tag} != {physical}")
    produced = tsv_from_decoded(decoded, checks["json_kinds"])
    if produced != expected:
        failed += 1
        print("types.tsv mismatch")
        for index, (left, right) in enumerate(zip(produced, expected)):
            if left != right:
                print(f"line {index + 1}\n got {left}\n tsv {right}")
        if len(produced) != len(expected):
            print(f"line count got={len(produced)} tsv={len(expected)}")
    identity = checks["identity"]
    if not identity.startswith("11.4.13-MariaDB"):
        failed += 1
        print(f"service version {identity}")
    print(f"compare_failures={failed}")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
