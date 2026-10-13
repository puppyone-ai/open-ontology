#!/usr/bin/env python3
"""Independent compare of this source's sealed shadow. Does not open another owner's database."""

from __future__ import annotations

import json
import subprocess
import sys
from decimal import Decimal
from pathlib import Path

CONTAINER = "oo-dl01w1-41-pg"
DATABASE = "oo_w141"
USER = "oo_w141_admin"
DECODED = Path(
    "/Users/redux/puppy-one/open-ontology/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-driver-preflight/raw/probe/decoded.json"
)


def psql(sql: str) -> str:
    result = subprocess.run(
        [
            "docker",
            "exec",
            "-i",
            CONTAINER,
            "psql",
            "-U",
            USER,
            "-d",
            DATABASE,
            "-v",
            "ON_ERROR_STOP=1",
            "-At",
        ],
        input=sql,
        text=True,
        capture_output=True,
    )
    if result.returncode != 0:
        sys.stderr.write(result.stderr)
        raise SystemExit(result.returncode)
    return result.stdout


def runs() -> str:
    return psql(
        "SELECT shard_id||'|'||state||'|'||source_id||'|'||source_generation||'|'||target_generation "
        "FROM _oo.full_import_run ORDER BY 1"
    )


def negative(out: Path) -> int:
    current = runs()
    (out / "pg-after-negative.txt").write_text(current)
    before = (out / "pg-before.txt").read_text()
    extra = psql(
        "SELECT COUNT(*) FROM _oo.full_import_run WHERE shard_id='oo_w141' OR source_id='oo_w141'"
    ).strip()
    open_left = psql(
        "SELECT COUNT(*) FROM _oo.full_import_run WHERE state='open' AND shard_id='oo_w141'"
    ).strip()
    text = f"runs_unchanged={current == before}\noo_w141_runs={extra}\noo_w141_open={open_left}\n"
    (out / "pg-negative-verdict.txt").write_text(text)
    print(text, end="")
    return 0 if current == before and extra == "0" and open_left == "0" else 1


def expect_lane(row: dict) -> str:
    if row["lane_tag"] == "utf8":
        value = json.dumps(row["lane_text"], ensure_ascii=False)
        return f'{{"type_id":0,"type":"utf8","value":{value}}}'
    return f'{{"type_id":1,"type":"int64","value":{row["lane_num"]}}}'


def expect_marks(row: dict):
    if row["marks"] is None:
        return None
    return json.dumps(row["marks"], ensure_ascii=False, separators=(",", ":"))


def expect_pack(row: dict):
    if row["pack_presence"] < 2:
        return None
    kind = "null" if row["pack_kind"] is None else json.dumps(row["pack_kind"], ensure_ascii=False)
    number = "null" if row["pack_n"] is None else str(row["pack_n"])
    return f'{{"kind":{kind},"n":{number}}}'


def expect_price(text: str) -> Decimal:
    return Decimal(text).scaleb(-7)


def positive(out: Path) -> int:
    envelope = json.loads((out / "positive.json").read_text())
    table = envelope["import"]["tables"][0]
    physical = table["physical_name"]
    if not (
        len(physical) == 63
        and physical.startswith("pt_")
        and all(char in "0123456789abcdef" for char in physical[3:])
    ):
        print("physical name refused")
        return 1
    columns = table["columns"]
    names = [column["logical_name"] for column in columns]
    wanted = [
        "_id", "code", "flag", "units", "price", "created", "uid", "blob",
        "blob_presence", "weight", "warehouse", "whCode", "lane", "memo",
        "memo_presence", "marks", "marks_presence", "pack", "pack_presence",
    ]
    if names != wanted:
        print("column order", names)
        return 1
    attrs = psql(
        "SELECT a.attnum::int || '|' || a.attname || '|' || a.atttypid::int || '|' || a.atttypmod::int "
        "|| '|' || format_type(a.atttypid, a.atttypmod) "
        "FROM pg_attribute a "
        "JOIN pg_class c ON c.oid = a.attrelid "
        "JOIN pg_namespace n ON n.oid = c.relnamespace "
        f"WHERE n.nspname = '_oo' AND c.relname = '{physical}' AND a.attnum > 0 AND NOT a.attisdropped "
        "ORDER BY a.attnum::int"
    )
    (out / "pg-after-attrs.txt").write_text(attrs)
    attr_rows = [line.split("|") for line in attrs.splitlines() if line]
    if len(attr_rows) != 20:
        print("attr count", len(attr_rows))
        return 1
    expected_oid = {
        "_id": 17, "code": 1043, "flag": 23, "units": 20, "price": 1700,
        "created": 1184, "uid": 17, "blob": 17, "blob_presence": 21, "weight": 701,
        "warehouse": 1043, "whCode": 1043, "lane": 114, "memo": 1043,
        "memo_presence": 21, "marks": 114, "marks_presence": 21, "pack": 114,
        "pack_presence": 21,
    }
    for column, attr in zip(columns, attr_rows[1:]):
        if int(attr[2]) != column["oid"] or int(attr[2]) != expected_oid[column["logical_name"]]:
            print("oid", column["logical_name"], attr, column["oid"])
            return 1
        if int(attr[2]) == 3802:
            print("jsonb", column["logical_name"])
            return 1
    select = ", ".join(
        [
            '"_oo_pos"::text',
            f'encode("{attr_rows[1][1]}", \'hex\')',
            f'CASE WHEN "{attr_rows[2][1]}" IS NULL THEN \'<NULL>\' ELSE "{attr_rows[2][1]}"::text END',
            f'"{attr_rows[3][1]}"::text',
            f'"{attr_rows[4][1]}"::text',
            f'"{attr_rows[5][1]}"::text',
            f'(EXTRACT(EPOCH FROM "{attr_rows[6][1]}") * 1000)::numeric(30,0)::text',
            f'encode("{attr_rows[7][1]}", \'hex\')',
            f'encode("{attr_rows[8][1]}", \'hex\')',
            f'"{attr_rows[9][1]}"::text',
            f'"{attr_rows[10][1]}"::text',
            f'CASE WHEN "{attr_rows[11][1]}" IS NULL THEN \'<NULL>\' ELSE "{attr_rows[11][1]}"::text END',
            f'CASE WHEN "{attr_rows[12][1]}" IS NULL THEN \'<NULL>\' ELSE "{attr_rows[12][1]}"::text END',
            f'"{attr_rows[13][1]}"::text',
            f'CASE WHEN "{attr_rows[14][1]}" IS NULL THEN \'<NULL>\' ELSE "{attr_rows[14][1]}"::text END',
            f'"{attr_rows[15][1]}"::text',
            f'CASE WHEN "{attr_rows[16][1]}" IS NULL THEN \'<NULL>\' ELSE "{attr_rows[16][1]}"::text END',
            f'"{attr_rows[17][1]}"::text',
            f'CASE WHEN "{attr_rows[18][1]}" IS NULL THEN \'<NULL>\' ELSE "{attr_rows[18][1]}"::text END',
            f'"{attr_rows[19][1]}"::text',
        ]
    )
    body = psql(f'SELECT {select} FROM _oo."{physical}" ORDER BY "_oo_pos"::numeric')
    (out / "pg-independent.txt").write_text(body)
    decoded = json.loads(DECODED.read_text())["records"]
    got_rows = [line.split("|") for line in body.splitlines() if line]
    mismatches = []
    if len(got_rows) != len(decoded):
        mismatches.append(f"row_count {len(got_rows)} {len(decoded)}")
    for index, (got, row) in enumerate(zip(got_rows, decoded)):
        expect = [
            str(index),
            row["_id"],
            row["code"],
            str(row["flag"]),
            str(row["units"]),
            format(expect_price(row["price"]), "f"),
            str(row["created_ms"]),
            row["uid_hex"],
            row["blob_hex"],
            str(row["blob_presence"]),
            format(Decimal(str(row["weight"])), "f"),
            "<NULL>" if row["warehouse"] is None else row["warehouse"],
            "<NULL>" if row["whCode"] is None else row["whCode"],
            expect_lane(row),
            "<NULL>" if row["memo"] is None else row["memo"],
            str(row["memo_presence"]),
            "<NULL>" if expect_marks(row) is None else expect_marks(row),
            str(row["marks_presence"]),
            "<NULL>" if expect_pack(row) is None else expect_pack(row),
            str(row["pack_presence"]),
        ]
        for name, actual, wanted_value in zip(["pos", *wanted], got, expect):
            if name == "price":
                if Decimal(actual) != expect_price(row["price"]):
                    mismatches.append(f"{index} price {actual} {expect_price(row['price'])}")
                continue
            if name == "weight":
                if Decimal(actual) != Decimal(str(row["weight"])):
                    mismatches.append(f"{index} weight {actual} {row['weight']}")
                continue
            if actual != wanted_value:
                mismatches.append(f"{index} {name} {actual!r} {wanted_value!r}")
    after = runs()
    (out / "pg-after-runs.txt").write_text(after)
    before_lines = [line for line in (out / "pg-before.txt").read_text().splitlines() if line]
    after_lines = [line for line in after.splitlines() if line]
    added = [line for line in after_lines if line not in before_lines]
    removed = [line for line in before_lines if line not in after_lines]
    verdict = (
        f"MISMATCHES {len(mismatches)}\n"
        + "\n".join(mismatches[:40])
        + f"\nadded={added}\nremoved={removed}\nphysical={physical}\n"
    )
    (out / "independent-verdict.txt").write_text(verdict)
    print(verdict)
    ok = not mismatches and removed == [] and added == ["oo_w141|awaiting_publish|oo_w141|1|1"]
    return 0 if ok else 1


def main() -> int:
    mode = sys.argv[1]
    out = Path(sys.argv[2])
    if mode == "negative":
        return negative(out)
    if mode == "positive":
        return positive(out)
    raise SystemExit("mode")


if __name__ == "__main__":
    raise SystemExit(main())
