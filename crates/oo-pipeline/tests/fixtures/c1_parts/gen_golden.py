#!/usr/bin/env python3
"""Golden corpus for oo-pipeline c1_parts (protocol oo.pipeline.c1-parts/v2).

This is a second, independent implementation of DESIGN.md 3.8 and of the
c1_parts.rs byte rules. It writes golden-v2.json and pg17-readback-v2.json. The Rust
tests read those files. A mismatch means one of the two implementations drifted.
Run: python3 gen_golden.py   (writes next to this file; needs only the standard library)
"""
import datetime as dt
import hashlib
import json
import os
import re
import struct
from decimal import Decimal

HERE = os.path.dirname(os.path.abspath(__file__))
U64_MAX = (1 << 64) - 1
KEY_MIN = -(1 << 63)
KEY_MAX = U64_MAX
F64_CANON_NAN = 0x7FF8000000000000
KIND_ORDER = {"fill": 0, "cdc": 1}  # matches PartKind derive(Ord): Fill < Cdc


def canonical_decimal(text, scale):
    neg = text.startswith("-")
    body = text[1:] if neg else text
    whole, _, frac = body.partition(".")
    assert whole and whole.isdigit() and (frac == "" or frac.isdigit()), text
    kept, dropped = (frac[:scale], frac[scale:]) if len(frac) > scale else (frac, "")
    assert all(c == "0" for c in dropped), ("nonzero beyond scale", text, scale)
    whole = whole.lstrip("0") or "0"
    nonzero = whole != "0" or any(c != "0" for c in kept)
    out = ("-" if neg and nonzero else "") + whole
    if scale > 0:
        out += "." + kept + "0" * (scale - len(kept))
    return out.encode()


def json_number(d):
    if d == 0:
        return "0"
    return format(d.normalize(), "f")


def json_escape(s):
    out = ['"']
    for ch in s:
        if ch == '"':
            out.append('\\"')
        elif ch == "\\":
            out.append("\\\\")
        elif ch == "\b":
            out.append("\\b")
        elif ch == "\f":
            out.append("\\f")
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\r":
            out.append("\\r")
        elif ch == "\t":
            out.append("\\t")
        elif ord(ch) < 0x20:
            out.append("\\u%04x" % ord(ch))
        else:
            out.append(ch)
    out.append('"')
    return "".join(out)


def canonical_json_value(v):
    if v is None:
        return "null"
    if v is True:
        return "true"
    if v is False:
        return "false"
    if isinstance(v, Decimal):
        return json_number(v)
    if isinstance(v, int):
        return str(v)
    if isinstance(v, str):
        return json_escape(v)
    if isinstance(v, list):
        return "[" + ",".join(canonical_json_value(x) for x in v) + "]"
    if isinstance(v, dict):
        keys = sorted(v.keys(), key=lambda k: k.encode("utf-8"))
        return "{" + ",".join(json_escape(k) + ":" + canonical_json_value(v[k]) for k in keys) + "}"
    raise TypeError(type(v))


def canonical_json(text):
    value = json.loads(text, parse_float=Decimal, parse_int=int)
    return canonical_json_value(value).encode()


def days_from_civil(y, m, d):
    # Independent route: Python's datetime, not the civil-day formula used in Rust.
    return (dt.date(y, m, d) - dt.date(1970, 1, 1)).days


def micros_of(text):
    # "YYYY-MM-DD HH:MM:SS[.ffffff]" read as UTC.
    fmt = "%Y-%m-%d %H:%M:%S.%f" if "." in text else "%Y-%m-%d %H:%M:%S"
    t = dt.datetime.strptime(text, fmt).replace(tzinfo=dt.timezone.utc)
    return (t - dt.datetime(1970, 1, 1, tzinfo=dt.timezone.utc)) // dt.timedelta(microseconds=1)


def payload(ty, cell):
    kind = ty["kind"]
    if kind == "bool":
        return b"1" if cell["bool"] else b"0"
    if kind == "int":
        return str(int(cell["int"])).encode()
    if kind == "decimal":
        return canonical_decimal(cell["decimal"], ty["scale"])
    if kind == "float64":
        bits = int(cell["float_bits"], 16)
        if (bits & 0x7FFFFFFFFFFFFFFF) > 0x7FF0000000000000:
            bits = F64_CANON_NAN
        return struct.pack(">Q", bits)
    if kind == "text":
        return cell["text"].encode("utf-8")
    if kind == "json":
        return canonical_json(cell["text"])
    if kind == "bytes":
        return bytes.fromhex(cell["bytes_hex"])
    if kind == "date":
        y, m, d = (int(x) for x in cell["date"].split("-"))
        return ("%04d-%02d-%02d" % (y, m, d)).encode()
    if kind == "micros":
        return str(int(cell["micros"])).encode()
    raise ValueError(kind)


def canonical_cell(ty, cell):
    if "null" in cell:
        return b"\x00"
    p = payload(ty, cell)
    return b"\x01" + struct.pack(">I", len(p)) + p


def bits_of(x):
    return "0x%016x" % struct.unpack(">Q", struct.pack(">d", x))[0]


def bucket_of(key, bits):
    offset = key - KEY_MIN
    span = 3 * (1 << 63)
    return offset * (1 << bits) // span


def add_mod(a, b):
    return ((int.from_bytes(a, "big") + int.from_bytes(b, "big")) % (1 << 256)).to_bytes(32, "big")


def composed(tables, bits):
    """tables: {name: [(key, [canonical row bytes])]} -> (composed hex, {name: table hex})."""
    per = {}
    for name, rows in tables.items():
        buckets = {}
        for key, row in rows:
            idx = bucket_of(key, bits)
            row_sha = hashlib.sha256(row).digest()
            count, total = buckets.get(idx, (0, b"\x00" * 32))
            buckets[idx] = (count + 1, add_mod(total, row_sha))
        pre = bytes([bits])
        for idx in sorted(buckets):
            count, total = buckets[idx]
            pre += struct.pack(">I", idx) + struct.pack(">Q", count) + total
        per[name] = hashlib.sha256(pre).hexdigest()
    pre = b"oo.c1.state/v2\n"
    for name in sorted(per):
        pre += b"T:" + name.encode() + b"\n" + bytes.fromhex(per[name])
    return hashlib.sha256(pre).hexdigest(), per


def parts_digest(parts):
    if len(parts) == 1:
        return parts[0]["sha256"]
    lines = sorted(parts, key=lambda p: (KIND_ORDER[p["kind"]], p["part_no"]))
    pre = b"oo.c1.parts/v2\n"
    for p in lines:
        lo = "" if p.get("key_lo") is None else str(p["key_lo"])
        hi = "" if p.get("key_hi") is None else str(p["key_hi"])
        pre += ("%s|%d|%s|%s|%d|%d|%s\n" % (p["kind"], p["part_no"], lo, hi, p["rows"], p["bytes"], p["sha256"])).encode()
    return hashlib.sha256(pre).hexdigest()


def canonical_vectors():
    T = lambda kind, **kw: dict(kind=kind, **kw)
    vectors = [
        ("null_any_type", T("int"), {"null": True}),
        ("bool_false", T("bool"), {"bool": False}),
        ("bool_true", T("bool"), {"bool": True}),
        ("int_zero", T("int"), {"int": "0"}),
        ("int_i64_min", T("int"), {"int": "-9223372036854775808"}),
        ("int_u64_max", T("int"), {"int": str(U64_MAX)}),
        ("decimal_s2_negative", T("decimal", scale=2), {"decimal": "-12.3"}),
        ("decimal_s2_negative_zero_is_unsigned", T("decimal", scale=2), {"decimal": "-0.00"}),
        ("decimal_s0_u64_max", T("decimal", scale=0), {"decimal": str(U64_MAX)}),
        ("decimal_s4_small", T("decimal", scale=4), {"decimal": "0.0001"}),
        ("decimal_s10_padded", T("decimal", scale=10), {"decimal": "1234.5"}),
        ("decimal_s2_trailing_zero_dropped", T("decimal", scale=2), {"decimal": "7.000"}),
        ("float_1_5", T("float64"), {"float_bits": bits_of(1.5)}),
        ("float_neg_zero_keeps_sign", T("float64"), {"float_bits": bits_of(-0.0)}),
        ("float_pos_zero", T("float64"), {"float_bits": bits_of(0.0)}),
        ("float4_0_1_widened", T("float64"), {"float_bits": bits_of(float(struct.unpack(">f", struct.pack(">f", 0.1))[0]))}),
        ("float_max", T("float64"), {"float_bits": "0x7fefffffffffffff"}),
        ("float_nan_payload_canonical", T("float64"), {"float_bits": "0x7ff0000000000001"}),
        ("text_utf8", T("text"), {"text": "héllo 世界 😀"}),
        ("text_empty", T("text"), {"text": ""}),
        ("text_trailing_space_kept", T("text"), {"text": "a  "}),
        ("bytes_with_nul", T("bytes"), {"bytes_hex": "00ff10"}),
        ("bytes_empty", T("bytes"), {"bytes_hex": ""}),
        ("date_leap_day", T("date"), {"date": "2024-02-29"}),
        ("date_min", T("date"), {"date": "0001-01-01"}),
        ("date_max", T("date"), {"date": "9999-12-31"}),
        ("datetime_pre_epoch_half_second", T("micros"), {"micros": str(micros_of("1969-12-31 23:59:59.5"))}),
        ("datetime_micro_precision", T("micros"), {"micros": str(micros_of("2024-01-02 03:04:05.000001"))}),
        ("datetime_epoch", T("micros"), {"micros": str(micros_of("1970-01-01 00:00:00"))}),
        ("time_minus_838h59m59s", T("micros"), {"micros": str(-(838 * 3600 + 59 * 60 + 59) * 1_000_000)}),
        ("time_zero", T("micros"), {"micros": "0"}),
        ("time_fraction", T("micros"), {"micros": str(12 * 3600_000_000 + 34 * 60_000_000 + 56 * 1_000_000 + 7)}),
        ("json_sorted_and_normalized", T("json"), {"text": '{"b": 1.0, "a": [true, null]}'}),
        ("json_number_forms", T("json"), {"text": "[1e3, -0.0, 2.50, 0.000, -0]"}),
        ("json_string_escapes", T("json"), {"text": '"a\\"b\\\\c\\n\\u0001é"'}),
        ("json_nested_key_order", T("json"), {"text": '{"z": {"b": 2, "a": 1}, "a": []}'}),
    ]
    out = []
    for name, ty, cell in vectors:
        out.append({"name": name, "type": ty, "cell": cell, "expect_hex": canonical_cell(ty, cell).hex()})
    return out


def sanity():
    # Spot checks against values worked out by hand, so the script itself is not circular.
    assert payload(dict(kind="decimal", scale=2), {"decimal": "-12.3"}) == b"-12.30"
    assert payload(dict(kind="micros"), {"micros": "-3020399000000"}) == b"-3020399000000"  # 838:59:59
    assert micros_of("1969-12-31 23:59:59.5") == -500000
    assert payload(dict(kind="json"), {"text": '{"b": 1.0, "a": [true, null]}'}) == b'{"a":[true,null],"b":1}'
    assert days_from_civil(1970, 1, 1) == 0


def composed_vectors():
    # Keys cover both ends of the key domain and several buckets.
    rows_a = [
        (KEY_MIN, [("int", "-9223372036854775808"), ("decimal", "-12.3"), ("text", "x")]),
        (-1, [("int", "-1"), ("decimal", "0"), ("text", None)]),
        (0, [("int", "0"), ("decimal", "1.50"), ("text", "")]),
        (1, [("int", "1"), ("decimal", None), ("text", "é")]),
        (KEY_MAX, [("int", str(U64_MAX)), ("decimal", "99999999.99"), ("text", "last")]),
    ]
    rows_b = [
        (7, [("int", "7"), ("decimal", "7"), ("text", "b7")]),
        (1 << 40, [("int", str(1 << 40)), ("decimal", "0.5"), ("text", None)]),
    ]
    types = [dict(kind="int"), dict(kind="decimal", scale=2), dict(kind="text")]

    def encode(rows):
        out = []
        for key, cells in rows:
            enc = []
            for (k, v), ty in zip(cells, types):
                cell = {"null": True} if v is None else {k: v}
                enc.append(canonical_cell(ty, cell))
            out.append((key, b"".join(enc), [{"null": True} if v is None else {k: v} for k, v in cells]))
        return out

    encoded_a, encoded_b = encode(rows_a), encode(rows_b)
    tables = {"bench.shipments": [(k, row) for k, row, _ in encoded_a], "bench.a": [(k, row) for k, row, _ in encoded_b]}
    bits = 4
    digest, per = composed(tables, bits)
    return {
        "bucket_bits": bits,
        "column_types": types,
        "tables": [
            {"name": name, "rows": [{"key": str(key), "cells": cells} for key, _, cells in rs]}
            for name, rs in [
                ("bench.shipments", encoded_a),
                ("bench.a", encoded_b),
            ]
        ],
        "expect_table_digests": per,
        "expect_composed_digest": digest,
    }


def parts_vectors():
    parts = [
        dict(part_no=1, kind="fill", key_lo="1", key_hi="50000", rows=50000, bytes=8388608, sha256="a" * 64),
        dict(part_no=2, kind="fill", key_lo="50001", key_hi="100000", rows=50000, bytes=8388607, sha256="b" * 64),
        dict(part_no=1, kind="cdc", key_lo="5", key_hi="9", rows=3, bytes=1200, sha256="c" * 64),
    ]
    for p in parts:
        p["key_lo"] = p["key_lo"]
    return {"parts": parts, "expect_parts_digest": parts_digest(parts)}


def copy_unescape(field):
    """Undo COPY text-format escaping. \\N is NULL."""
    if field == "\\N":
        return None
    out = []
    i = 0
    simple = {"b": "\b", "f": "\f", "n": "\n", "r": "\r", "t": "\t", "v": "\v", "\\": "\\"}
    while i < len(field):
        ch = field[i]
        if ch == "\\":
            nxt = field[i + 1]
            out.append(simple[nxt])
            i += 2
        else:
            out.append(ch)
            i += 1
    return "".join(out)


def pg_readback():
    """Parse the recorded COPY TO STDOUT row and the attribute list from pg17-readback.raw.txt."""
    raw = open(os.path.join(HERE, "pg17-readback.raw.txt"), encoding="utf-8").read().split("\n")
    attrs = json.loads([line for line in raw if line.startswith("[")][0])
    row = [line for line in raw if line.count("\t") >= len(attrs) - 1 and not line.startswith("[")][0]
    fields = row.split("\t")
    assert len(fields) == len(attrs), (len(fields), len(attrs))
    rows = []
    for col, field in zip(attrs, fields):
        rows.append({"name": col["name"], "oid": col["oid"], "typmod": col["typmod"], "text": copy_unescape(field)})
    return {
        "protocol": "oo.pipeline.c1-pg-readback-golden/v2",
        "source": "postgres:17 image; COPY TO STDOUT (text format) after INSERT; session TimeZone Asia/Shanghai; recipe in pg17-readback.sql",
        "columns": rows,
    }


def parity_vectors():
    """One entry per column of pg17-readback-v2.json. The value is the MySQL logical value.

    The expected bytes depend only on the logical value and the target type. Both the
    MySQL decoder path (mysql-preview integration test) and the PostgreSQL text path
    (oo-pipeline unit test) must produce them.
    """
    T = lambda kind, **kw: dict(kind=kind, **kw)
    f4 = float(struct.unpack(">f", struct.pack(">f", 0.1))[0])
    rows = [
        ("c_bool", T("bool"), {"bool": True}),
        ("c_i2", T("int"), {"int": "-32768"}),
        ("c_i4", T("int"), {"int": "2147483647"}),
        ("c_i8", T("int"), {"int": "-9223372036854775808"}),
        ("c_u64", T("decimal", scale=0), {"decimal": str(U64_MAX)}),
        ("c_dec", T("decimal", scale=2), {"decimal": "-12.3"}),
        ("c_dec0", T("decimal", scale=4), {"decimal": "0.0001"}),
        ("c_dec1", T("decimal", scale=10), {"decimal": "1234.5"}),
        ("c_dec_zero", T("decimal", scale=4), {"decimal": "0.0000"}),
        ("c_dec_wide", T("decimal", scale=30), {"decimal": "-12345678901234567890123456789012345.123456789012345678901234567890"}),
        ("c_f4", T("float64"), {"float_bits": bits_of(f4)}),
        ("c_f8neg0", T("float64"), {"float_bits": bits_of(-0.0)}),
        ("c_f8big", T("float64"), {"float_bits": bits_of(1.5e300)}),
        ("c_f8small", T("float64"), {"float_bits": bits_of(1e-7)}),
        ("c_varchar", T("text"), {"text": "héllo 世界 😀"}),
        ("c_varchar_sp", T("text"), {"text": "a  "}),
        ("c_bpchar", T("text"), {"text": "a"}),
        ("c_json", T("json"), {"text": '{"b": 1.0, "a": [true, null]}'}),
        ("c_bytea", T("bytes"), {"bytes_hex": "00ff10"}),
        ("c_date", T("date"), {"date": "2024-02-29"}),
        ("c_date_min", T("date"), {"date": "0001-01-01"}),
        ("c_dt", T("micros"), {"micros": str(micros_of("2024-01-02 03:04:05.000001"))}),
        ("c_dt_pre", T("micros"), {"micros": str(micros_of("1969-12-31 23:59:59.5"))}),
        ("c_tstz", T("micros"), {"micros": str(micros_of("2024-01-02 03:04:05.123456"))}),
        ("c_time_neg", T("micros"), {"micros": str(-(838 * 3600 + 59 * 60 + 59) * 1_000_000)}),
        ("c_time_pos", T("micros"), {"micros": str(12 * 3600_000_000 + 34 * 60_000_000 + 56 * 1_000_000 + 7)}),
        ("c_time_zero", T("micros"), {"micros": "0"}),
        ("c_null", T("int"), {"null": True}),
    ]
    out = []
    for column, ty, cell in rows:
        out.append({"column": column, "type": ty, "cell": cell, "expect_hex": canonical_cell(ty, cell).hex()})
    return out


def manifest_fixture():
    """manifest-v2.json for the .arrow fixtures. sha256 and bytes are read from the files."""
    spec = [
        ("fill", 1, 1, 3, 3, "fill-00000001.arrow"),
        ("fill", 2, 10, 11, 2, "fill-00000002.arrow"),
        ("fill", 3, None, None, 0, "fill-00000003.arrow"),
        ("cdc", 1, 1, 3, 3, "cdc-00000001.arrow"),
    ]
    parts = []
    for kind, part_no, lo, hi, rows, name in spec:
        data = open(os.path.join(HERE, name), "rb").read()
        parts.append(dict(
            part_no=part_no,
            kind=kind,
            key_lo=None if lo is None else str(lo),
            key_hi=None if hi is None else str(hi),
            rows=rows,
            bytes=len(data),
            sha256=hashlib.sha256(data).hexdigest(),
        ))
    digest = parts_digest([dict(p, key_lo=None if p["key_lo"] is None else int(p["key_lo"]),
                                key_hi=None if p["key_hi"] is None else int(p["key_hi"])) for p in parts])
    return {"protocol": "oo.pipeline.c1-parts/v2", "parts": parts, "parts_digest": digest}


def main():
    sanity()
    golden = {
        "protocol": "oo.pipeline.c1-parts-golden/v2",
        "generator": "gen_golden.py (independent of c1_parts.rs). Regenerate: python3 gen_golden.py",
        "canonical": canonical_vectors(),
        "parity": parity_vectors(),
        "composed": composed_vectors(),
        "parts": parts_vectors(),
    }
    with open(os.path.join(HERE, "manifest-v2.json"), "w", encoding="utf-8") as fh:
        json.dump(manifest_fixture(), fh, indent=1)
        fh.write("\n")
    with open(os.path.join(HERE, "golden-v2.json"), "w", encoding="utf-8") as fh:
        json.dump(golden, fh, ensure_ascii=False, indent=1)
        fh.write("\n")
    with open(os.path.join(HERE, "pg17-readback-v2.json"), "w", encoding="utf-8") as fh:
        json.dump(pg_readback(), fh, ensure_ascii=False, indent=1)
        fh.write("\n")
    print("canonical vectors:", len(golden["canonical"]))
    print("composed digest:", golden["composed"]["expect_composed_digest"])


if __name__ == "__main__":
    main()
