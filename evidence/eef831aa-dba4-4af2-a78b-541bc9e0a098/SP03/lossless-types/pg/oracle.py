#!/usr/bin/env python3
"""Historical send-hex parser from the first fixture pass.

The deciding oracle is the Rust module `pg_wire_oracle` used by
`lossless_live`. This file and `out/oracle.json` stay as that attempt.
They are not the acceptance check.
"""

import json
import struct
import sys


def parse_numeric(text):
    raw = bytes.fromhex(text)
    count, weight, sign, dscale = struct.unpack(">hhhh", raw[:8])
    sign &= 0xFFFF
    if count < 0 or 8 + 2 * count != len(raw):
        raise SystemExit(f"numeric length {len(raw)} count {count}")
    digits = list(struct.unpack(">" + "h" * count, raw[8:])) if count else []
    return {
        "ndigits": count,
        "weight": weight,
        "sign": sign,
        "dscale": dscale,
        "digits": digits,
    }


def parse_interval(text):
    micros, days, months = struct.unpack(">qii", bytes.fromhex(text))
    return {"micros": micros, "days": days, "months": months}


def parse_array(text):
    raw = bytes.fromhex(text)
    ndim, _flags, elem = struct.unpack(">iii", raw[:12])
    if not 0 <= ndim <= 6:
        raise SystemExit(f"ndim {ndim}")
    offset = 12
    lengths = []
    lowers = []
    for _ in range(ndim):
        length, lower = struct.unpack(">ii", raw[offset : offset + 8])
        offset += 8
        lengths.append(length)
        lowers.append(lower)
    null_at = []
    index = 0
    while offset < len(raw):
        (size,) = struct.unpack(">i", raw[offset : offset + 4])
        offset += 4
        if size < 0:
            null_at.append(index)
        else:
            offset += size
        index += 1
    if offset != len(raw):
        raise SystemExit("array tail")
    return {
        "ndim": ndim,
        "elem": elem,
        "lengths": lengths,
        "lowers": lowers,
        "null_at": null_at,
        "count": index,
    }


def main():
    rows = [json.loads(line) for line in sys.stdin if line.strip()]
    by_id = {row["id"]: row for row in rows}
    one = by_id[1]
    two = by_id[2]
    units = parse_numeric(one["units_hex"])
    wide = parse_numeric(one["wide_hex"])
    span = parse_interval(one["span_hex"])
    nums = parse_array(one["nums_hex"])
    grid = parse_array(one["grid_hex"])
    bad = parse_numeric(two["bad_num_hex"])
    jsonb = bytes.fromhex(one["jsonb_hex"])
    if jsonb[:1] != b"\x01":
        raise SystemExit(f"jsonb version {jsonb[:1]!r}")
    jsonb_text = jsonb[1:].decode("utf-8")
    json_text = bytes.fromhex(one["json_hex"]).decode("utf-8")
    if units["digits"] != [1200] or units["sign"] != 0 or units["dscale"] != 0:
        raise SystemExit(f"units {units}")
    if wide["digits"] != [9999] * 10 or wide["sign"] != 0x4000 or wide["weight"] != 9 or wide["dscale"] != 0:
        raise SystemExit(f"wide {wide}")
    if span != {"micros": 5, "days": 4, "months": 3}:
        raise SystemExit(f"span {span}")
    if nums["lowers"] != [2] or nums["lengths"] != [3] or nums["null_at"] != [1]:
        raise SystemExit(f"nums {nums}")
    if grid["ndim"] != 2 or grid["lowers"] != [0, 2] or grid["lengths"] != [2, 2]:
        raise SystemExit(f"grid {grid}")
    if json_text != '{"z":1,"a":2,"a":3}':
        raise SystemExit(f"json {json_text!r}")
    if jsonb_text == json_text:
        raise SystemExit("jsonb kept the duplicate-key text")
    if bad["sign"] != 0xC000:
        raise SystemExit(f"bad numeric {bad}")
    if one["float8_hex"] != "8000000000000000":
        raise SystemExit(f"neg zero {one['float8_hex']}")
    if two["float8_hex"] != "0000000000000000":
        raise SystemExit(f"pos zero {two['float8_hex']}")
    if one["bool_true_hex"] != "01":
        raise SystemExit("bool")
    if one["int8_hex"] != (-9223372036854775808).to_bytes(8, "big", signed=True).hex():
        raise SystemExit(f"int8 {one['int8_hex']}")
    if two["hi_hex"] != "7ff0000000000000":
        raise SystemExit(f"inf {two['hi_hex']}")
    document = {
        "bool_true_hex": one["bool_true_hex"],
        "int8_min_hex": one["int8_hex"],
        "float8_neg_zero_hex": one["float8_hex"],
        "float8_pos_zero_hex": two["float8_hex"],
        "float8_pos_inf_hex": two["hi_hex"],
        "units_digits": units["digits"],
        "units_sign": units["sign"],
        "units_dscale": units["dscale"],
        "units_weight": units["weight"],
        "wide_digits": wide["digits"],
        "wide_sign": wide["sign"],
        "wide_weight": wide["weight"],
        "wide_dscale": wide["dscale"],
        "fine_text": one["fine_text"],
        "huge_text": one["huge_text"],
        "open_text": one["open_text"],
        "span_months": span["months"],
        "span_days": span["days"],
        "span_micros": span["micros"],
        "nums_lower0": nums["lowers"][0],
        "nums_null_at": nums["null_at"],
        "grid_ndim": grid["ndim"],
        "grid_lower": grid["lowers"],
        "grid_lengths": grid["lengths"],
        "json_text": json_text,
        "jsonb_text": jsonb_text,
        "jsonb_send_hex": one["jsonb_hex"],
        "bad_num_sign": bad["sign"],
        "label": one["label"],
        "date_hex": one["date_hex"],
        "uuid_hex": one["uuid_hex"],
        "bytea_hex": one["bytea_hex"],
    }
    json.dump(document, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
