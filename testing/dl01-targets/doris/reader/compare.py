#!/usr/bin/env python3
"""Compare reader TSV files with fixtures/expected.json.

Labeled unit check: `--self-check` exercises this parser only. It does not
accept the Doris capability. Decimal equality uses decimal.Decimal. A SQL
NULL cell is either the two-character mysql token backslash-N or the Doris
batch word NULL. A stored string NULL is not distinguishable from SQL NULL
in this rendering. Empty string stays empty. Scientific notation is not an
exact decimal.
"""

from __future__ import annotations

import json
import sys
from decimal import Decimal, InvalidOperation
from pathlib import Path


def parse_tsv(text: str, width: int) -> list[list[str | None]]:
    rows: list[list[str | None]] = []
    seen_header = False
    for line in text.splitlines():
        if line.strip() == "":
            continue
        parts = line.split("\t")
        if len(parts) != width:
            continue
        if not seen_header:
            seen_header = True
            continue
        parsed: list[str | None] = []
        for cell in parts:
            parsed.append(None if cell in {"\\N", "NULL"} else cell)
        rows.append(parsed)
    if not seen_header:
        raise ValueError(f"no {width}-column result in output")
    return rows


def same(actual: str | None, expected: str | None, decimal: bool) -> bool:
    if expected is None or actual is None:
        return actual is None and expected is None
    if not decimal:
        return actual == expected
    if "e" in actual.lower():
        return False
    try:
        return Decimal(actual) == Decimal(str(expected))
    except InvalidOperation:
        return False


def check_case(name: str, spec: dict, text: str) -> dict:
    width = len(spec["rows"][0]) if spec["rows"] else 0
    decimal_at = set(spec.get("decimal_columns", []))
    try:
        got = parse_tsv(text, width)
    except ValueError as exc:
        return {"case": name, "ok": False, "error": str(exc), "actual": text[:500]}
    problems = []
    if len(got) != len(spec["rows"]):
        problems.append(f"row count {len(got)} != {len(spec['rows'])}")
    for index, (actual, expected) in enumerate(zip(got, spec["rows"])):
        for column, (cell, want) in enumerate(zip(actual, expected)):
            if not same(cell, want, column in decimal_at):
                problems.append(
                    f"row {index} col {column}: actual {cell!r} expected {want!r}"
                )
    if len(got) != len(spec["rows"]):
        problems.append({"actual_rows": got, "expected_rows": spec["rows"]})
    return {"case": name, "ok": not problems, "problems": problems}


def must_fail(exit_dir: Path, name: str) -> dict:
    path = exit_dir / f"{name}.exit"
    if not path.is_file():
        return {"case": name, "ok": False, "error": "missing exit file"}
    code = path.read_text(encoding="utf-8").strip()
    ok = code not in {"", "0"}
    return {"case": name, "ok": ok, "exit": code}


def identity(text: str, marker: str, rejected: list[str]) -> dict:
    folded = text.lower()
    bad = [item for item in rejected if item in folded]
    # The FE row must name this pinned Doris build. A MySQL-looking version()
    # string is not accepted as identity.
    ok = marker in text and "doris" in folded and not bad
    return {
        "case": "identity_frontends",
        "ok": ok,
        "marker": marker,
        "rejected_hits": bad,
    }


def partitions(text: str) -> dict:
    ok = "p202601" in text and "p202602" in text and "2026-01-01" in text and "2026-02-01" in text
    return {"case": "show_partitions", "ok": ok}


def run(expected_path: Path, case_dir: Path, exit_dir: Path) -> dict:
    spec = json.loads(expected_path.read_text(encoding="utf-8"))
    results = []
    for name, case in spec["cases"].items():
        path = case_dir / f"{name}.stdout"
        if not path.is_file():
            results.append({"case": name, "ok": False, "error": "missing stdout"})
            continue
        results.append(check_case(name, case, path.read_text(encoding="utf-8")))
    for name in spec["must_fail"]:
        results.append(must_fail(exit_dir, name))
    frontends = case_dir / "identity_frontends.stdout"
    if frontends.is_file():
        results.append(
            identity(
                frontends.read_text(encoding="utf-8"),
                spec["identity"]["version_marker"],
                spec["identity"]["reject_markers"],
            )
        )
    else:
        results.append({"case": "identity_frontends", "ok": False, "error": "missing stdout"})
    part = case_dir / "partitions.stdout"
    if part.is_file():
        results.append(partitions(part.read_text(encoding="utf-8")))
    else:
        results.append({"case": "show_partitions", "ok": False, "error": "missing stdout"})
    backends = case_dir / "identity_backends.stdout"
    if backends.is_file():
        text = backends.read_text(encoding="utf-8").lower()
        results.append(
            {
                "case": "identity_backends",
                "ok": "true" in text,
                "alive_word": "true" in text,
            }
        )
    else:
        results.append({"case": "identity_backends", "ok": False, "error": "missing stdout"})
    return {
        "ok": all(item["ok"] for item in results),
        "results": results,
    }


def self_check() -> int:
    text = "qty\n1.50\n\\N\nNULL\n0.30\n"
    rows = parse_tsv(text, 1)
    assert rows == [["1.50"], [None], [None], ["0.30"]], rows
    assert same("0.30", "0.3", True)
    assert not same("1e-1", "0.1", True)
    assert same("", "", False)
    assert not same("", None, False)
    print("self-check ok")
    return 0


def main(argv: list[str]) -> int:
    if len(argv) == 2 and argv[1] == "--self-check":
        return self_check()
    if len(argv) != 4:
        print("usage: compare.py expected.json case_dir exit_dir", file=sys.stderr)
        return 2
    report = run(Path(argv[1]), Path(argv[2]), Path(argv[3]))
    json.dump(report, sys.stdout, indent=2, ensure_ascii=False)
    sys.stdout.write("\n")
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
