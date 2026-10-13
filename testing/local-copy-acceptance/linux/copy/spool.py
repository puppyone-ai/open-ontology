#!/usr/bin/env python3
"""Seal the source typed spool from the fixture formula, and convert a target COPY into the same JSONL."""

import json
import sys


def cell_int(value: str) -> dict:
    return {"t": "int8", "v": value}


def cell_dec(scale: int, value: str) -> dict:
    return {"t": "decimal", "scale": scale, "v": value}


def cell_text(value: str) -> dict:
    return {"t": "text", "v": value}


def cell_time(value: str) -> dict:
    return {"t": "time", "v": value}


def cell_bin(value: str) -> dict:
    return {"t": "binary", "v": value}


def cell_null() -> dict:
    return {"t": "null"}


def emit(path: str, rows: list[dict]) -> None:
    with open(path, "w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row, separators=(",", ":"), ensure_ascii=True))
            handle.write("\n")


def batch_a() -> list[dict]:
    rows = []
    for g in range(1, 2001):
        if g == 1:
            amount = cell_null()
        elif g == 2:
            amount = cell_dec(4, "1234567890123456789012345678901234.1234")
        else:
            amount = cell_dec(4, "0.0001")
        label = cell_text("" if g == 3 else "n")
        rows.append({"amount": amount, "id": cell_int(str(g)), "label": label})
    return rows


def batch_b() -> list[dict]:
    rows = []
    for g in range(1, 8001):
        at_time = cell_null() if g == 2 else cell_time("13:14:15.123456")
        blob = cell_null() if g == 3 else cell_bin("0001ff")
        rows.append({"at_time": at_time, "blob": blob, "id": cell_int(str(g))})
    return rows


def batch_c() -> list[dict]:
    rows = []
    for g in range(1, 90001):
        if g == 1:
            qty = "9223372036854775807"
        elif g == 2:
            qty = "-9223372036854775808"
        else:
            qty = str(g)
        amount = cell_null() if g == 4 else cell_dec(4, "1.5000")
        note = cell_null() if g == 5 else cell_text("c")
        rows.append(
            {
                "amount": amount,
                "id": cell_int(str(g)),
                "note": note,
                "qty": cell_int(qty),
            }
        )
    return rows


def write_source(directory: str) -> None:
    emit(f"{directory}/batch_a.source.jsonl", batch_a())
    emit(f"{directory}/batch_b.source.jsonl", batch_b())
    emit(f"{directory}/batch_c.source.jsonl", batch_c())


def convert_target(kind: str, src: str, dst: str) -> None:
    rows = []
    with open(src, "r", encoding="utf-8") as handle:
        for line in handle:
            line = line.rstrip("\n")
            if line == "":
                continue
            parts = line.split("\t")
            if kind == "batch_a":
                ident, amount, label = parts
                rows.append(
                    {
                        "amount": cell_null() if amount == "NULLMARK" else cell_dec(4, amount),
                        "id": cell_int(ident),
                        "label": cell_text("" if label == "NULLMARK" else label),
                    }
                )
            elif kind == "batch_b":
                ident, at_time, blob = parts
                rows.append(
                    {
                        "at_time": cell_null() if at_time == "NULLMARK" else cell_time(at_time),
                        "blob": cell_null() if blob == "NULLMARK" else cell_bin(blob),
                        "id": cell_int(ident),
                    }
                )
            elif kind == "batch_c":
                ident, qty, amount, note = parts
                rows.append(
                    {
                        "amount": cell_null() if amount == "NULLMARK" else cell_dec(4, amount),
                        "id": cell_int(ident),
                        "note": cell_null() if note == "NULLMARK" else cell_text(note),
                        "qty": cell_int(qty),
                    }
                )
            else:
                raise SystemExit(f"unknown kind {kind}")
    emit(dst, rows)


def main() -> None:
    command = sys.argv[1]
    if command == "source":
        write_source(sys.argv[2])
    elif command == "target":
        convert_target(sys.argv[2], sys.argv[3], sys.argv[4])
    else:
        raise SystemExit("usage")


if __name__ == "__main__":
    main()
