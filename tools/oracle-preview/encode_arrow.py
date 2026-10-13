#!/usr/bin/env python3
"""Encode the reader AS OF rows into an Arrow IPC stream.

The row text is the sqlplus capture. Values are not replaced with the
historical gold cells. input_digest is the sha256 of these bytes.
"""

from __future__ import annotations

import hashlib
import json
from datetime import datetime
from decimal import Decimal
from pathlib import Path

import pyarrow as pa
import pyarrow.ipc as ipc

OUT = Path(__file__).resolve().parents[2] / (
    "evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/policy-2"
)
OLD_ARROW = "fc547b45aa6743e243de59b16719cb9efffcdd403dfab47d5e9eaec4f65b523b"
OLD_LITERAL = "a6236974c0d71bfc372587e84e39d4dcf93aa994005700157854294abed47aa3"


def parse_dump(text: str) -> str | None:
    if text == "NULL":
        return None
    marker = "Len="
    if marker not in text or ":" not in text:
        raise SystemExit(f"dump is not oracle DUMP output: {text}")
    hexpart = text.split(":", 1)[1].strip()
    if not hexpart:
        return ""
    raw = bytes(int(part, 16) for part in hexpart.split(","))
    return raw.decode("utf-8")


def parse_raw(text: str) -> bytes | None:
    if text == "NULL":
        return None
    if text == "" or any(ch not in "0123456789abcdefABCDEF" for ch in text) or len(text) % 2:
        raise SystemExit("raw hex is not an even hex string")
    return bytes.fromhex(text)


def parse_decimal(text: str) -> Decimal | None:
    if text == "NULL":
        return None
    if text.startswith("-."):
        text = "-0." + text[2:]
    elif text.startswith("."):
        text = "0" + text
    if "#" in text or text.count(".") != 1:
        raise SystemExit(f"decimal text {text}")
    value = Decimal(text)
    if value != value.quantize(Decimal("0.000001")):
        raise SystemExit("decimal needs more than 6 fractional digits")
    return value


def parse_int8(text: str) -> int | None:
    if text == "NULL":
        return None
    value = int(text, 10)
    if value < -128 or value > 127:
        raise SystemExit("n1 does not fit int8")
    return value


def parse_time(text: str) -> datetime | None:
    if text == "NULL":
        return None
    return datetime.strptime(text, "%Y-%m-%d %H:%M:%S")


def load_rows() -> list[dict]:
    lines = (OUT / "rows.txt").read_text().splitlines()
    if len(lines) != 5:
        raise SystemExit("rows.txt is not five lines")
    parsed = []
    for line in lines:
        parts = line.split("|")
        if len(parts) != 7:
            raise SystemExit("row field count")
        ident, n16, n1, stamp, c6, v2, raw = parts
        parsed.append(
            {
                "ID": int(ident, 10),
                "N16": parse_decimal(n16),
                "N1": parse_int8(n1),
                "D": parse_time(stamp),
                "C6": parse_dump(c6),
                "V2": parse_dump(v2),
                "R": parse_raw(raw),
            }
        )
    if any(row["ID"] < -32768 or row["ID"] > 32767 for row in parsed):
        raise SystemExit("id does not fit int16")
    return parsed


def main() -> None:
    rows = load_rows()
    schema = pa.schema(
        [
            pa.field("ID", pa.int16(), nullable=False),
            pa.field("N16", pa.decimal128(16, 6), nullable=True),
            pa.field("N1", pa.int8(), nullable=True),
            pa.field("D", pa.timestamp("us"), nullable=True),
            pa.field("C6", pa.utf8(), nullable=True),
            pa.field("V2", pa.utf8(), nullable=True),
            pa.field("R", pa.binary(), nullable=True),
        ]
    )
    batch = pa.RecordBatch.from_pylist(rows, schema=schema)
    sink = pa.BufferOutputStream()
    with ipc.new_stream(sink, schema) as writer:
        writer.write_batch(batch)
    payload = sink.getvalue().to_pybytes()
    digest = hashlib.sha256(payload).hexdigest()
    if digest == OLD_ARROW or digest == OLD_LITERAL:
        raise SystemExit("arrow bytes reused an old digest")
    if b"2175138" in payload:
        raise SystemExit("arrow payload contains the expired scn text")
    (OUT / "batch.arrow").write_bytes(payload)
    reader = ipc.open_stream(pa.BufferReader(payload))
    got = reader.read_all()
    if got.num_rows != 5 or got.schema != schema:
        raise SystemExit("reread schema mismatch")
    kinds = {
        "null_n16": any(row["N16"] is None for row in rows),
        "null_n1": any(row["N1"] is None for row in rows),
        "null_v2": any(row["V2"] is None for row in rows),
        "null_r": any(row["R"] is None for row in rows),
        "decimal": any(row["N16"] is not None for row in rows),
        "date": any(row["D"] is not None for row in rows),
        "raw": any(row["R"] is not None for row in rows),
    }
    if not all(kinds.values()):
        raise SystemExit(f"capture is missing a required value class {kinds}")
    scn = (OUT / "published-scn.txt").read_text().strip()
    receipt = {
        "protocol": "oo.oracle-preview.capture-receipt/v1",
        "source_id": "oo04_t_batch",
        "published_scn": int(scn),
        "reader_called_package": False,
        "rows": 5,
        "arrow_bytes": len(payload),
        "arrow_sha256": digest,
        "input_digest": digest,
        "input_digest_preimage": "sha256 of the arrow ipc stream bytes",
        "not_scn": 2175138,
        "not_arrow_sha256": OLD_ARROW,
        "not_literal_input_digest": OLD_LITERAL,
        "schema": [f"{field.name}:{field.type}:nullable={field.nullable}" for field in schema],
        "value_classes": kinds,
        "int8_target": {
            "source_arrow": "int8",
            "postgresql_oid": 21,
            "postgresql_type": "int2",
            "lossless_for_int8_range": True,
            "provenance": "int8",
        },
    }
    (OUT / "capture-receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    (OUT / "batch.arrow.sha256").write_text(digest + "\n")


if __name__ == "__main__":
    main()
