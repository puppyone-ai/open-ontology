#!/usr/bin/env python3
"""Labeled unit check: expected key order is computed here, not copied from a server."""

import pathlib
import sys
from decimal import Decimal


def load(path: pathlib.Path, prefix: str) -> list[str]:
    values = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith(prefix):
            values.append(line.split("|", 2)[2])
    return values


def main() -> int:
    root = pathlib.Path(__file__).resolve().parents[1]
    coverage = root / "expected" / "coverage.tsv"
    failed = 0

    unsigned = [int(value) for value in load(coverage, "order|unsigned|")]
    if unsigned != sorted(unsigned):
        failed += 1
        print("unit_fail unsigned order is not numeric")

    decimal = [Decimal(value) for value in load(coverage, "order|decimal|")]
    if decimal != sorted(decimal):
        failed += 1
        print("unit_fail decimal order is not numeric")

    temporal = load(coverage, "order|temporal|")
    if temporal != sorted(temporal):
        failed += 1
        print("unit_fail temporal text order is not chronological ISO order")

    binary = []
    for value in load(coverage, "order|binary|"):
        binary.append(b"" if value == "<EMPTY>" else bytes.fromhex(value))
    if binary != sorted(binary):
        failed += 1
        print("unit_fail binary order is not raw byte order")

    print(f"unit=expect unsigned={len(unsigned)} decimal={len(decimal)} temporal={len(temporal)} binary={len(binary)} failed={failed}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
