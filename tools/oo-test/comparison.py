"""Ordered, typed comparison API. Not an Arrow ChunkReceipt/RangeDigest hash."""
import datetime as dt
from decimal import Decimal
import hashlib
import itertools
import json
import struct


def canonical(value, *, oracle_empty=False):
    if value is None or (oracle_empty and isinstance(value, str) and value == ""):
        return ["null"]
    if isinstance(value, bool):
        return ["bool", value]
    if isinstance(value, (int, Decimal)):
        number = Decimal(value)
        if not number.is_finite():
            raise ValueError("nonfinite decimal")
        text = format(number, "f")
        if "." in text:
            text = text.rstrip("0").rstrip(".")
        return ["decimal", "0" if number == 0 else text]
    if isinstance(value, float):
        return ["float64-bits", struct.pack(">d", value).hex()]
    if isinstance(value, dt.datetime):
        if value.tzinfo is None:
            return ["local-timestamp", value.isoformat(timespec="microseconds")]
        return ["timestamp", value.astimezone(dt.timezone.utc).isoformat(timespec="microseconds"),
                {"offset": value.strftime("%z"), "zone": getattr(value.tzinfo, "key", None)}]
    if isinstance(value, dt.date):
        return ["date", value.isoformat()]
    if isinstance(value, bytes):
        return ["bytes", value.hex()]
    if isinstance(value, str):
        return ["text", value]
    if isinstance(value, (list, tuple)):
        return ["array", [canonical(v) for v in value]]
    if isinstance(value, dict):
        if any(not isinstance(key, str) for key in value):
            raise TypeError("JSON object keys must be strings")
        return ["object", {k: canonical(v) for k, v in sorted(value.items())}]
    raise TypeError(f"unsupported value type: {type(value).__name__}")


def encoded(row, oracle_empty=False):
    # Oracle's SQL empty-string rule does not apply inside a JSON cell.
    value = ["array", [canonical(cell, oracle_empty=oracle_empty) for cell in row]]
    return json.dumps(value, ensure_ascii=False,
                      sort_keys=True, separators=(",", ":")).encode("utf-8") + b"\n"


def compare(left, right, *, key_columns, chunk_size=10000, consistency="B",
            oracle_empty=False):
    """Consume source-ordered rows once; keys are column indexes, never sorted locally."""
    if chunk_size <= 0 or consistency not in ("T", "S1", "S0", "B") or not key_columns:
        raise ValueError("positive chunk_size, explicit keys and T/S1/S0/B consistency required")
    missing = object()
    chunks = []
    first = None
    counts = [0, 0]
    hashes = [hashlib.sha256(), hashlib.sha256()]
    size = 0
    for index, pair in enumerate(itertools.zip_longest(left, right, fillvalue=missing)):
        payloads = []
        for side, row in enumerate(pair):
            payload = None if row is missing else encoded(row, oracle_empty)
            payloads.append(payload)
            if payload is not None:
                hashes[side].update(payload)
                counts[side] += 1
        if payloads[0] != payloads[1] and first is None:
            first = {"row_index": index, "keys": [
                None if row is missing else canonical([row[k] for k in key_columns])
                for row in pair]}
        size += 1
        if size == chunk_size:
            chunks.append({"chunk_id": len(chunks), "rows": counts,
                           "sha256": [h.hexdigest() for h in hashes]})
            counts, hashes, size = [0, 0], [hashlib.sha256(), hashlib.sha256()], 0
    if size:
        chunks.append({"chunk_id": len(chunks), "rows": counts,
                       "sha256": [h.hexdigest() for h in hashes]})
    return {"format": "oo-test-typed-json-v1", "consistency": consistency,
            "oracle_empty_is_null": oracle_empty, "equal": first is None,
            "first_difference": first, "chunks": chunks}
