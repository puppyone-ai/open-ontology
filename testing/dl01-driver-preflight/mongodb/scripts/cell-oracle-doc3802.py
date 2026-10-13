#!/usr/bin/env python3
"""Compare the generation-2 PostgreSQL rows with the frozen typed decode and the seal-scope jsonb."""

from __future__ import annotations

import json
import subprocess
from pathlib import Path

ROOT = Path("/Users/redux/puppy-one/open-ontology")
DECODED = ROOT / "evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-driver-preflight/raw/repair-64-fulldoc/probe/decoded.json"
CANONICAL = ROOT / "evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-67-canonical/canonical.json"
OUT = ROOT / "evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-71-native-import/cell-oracle.json"
TABLE = "pt_4d11431451ccacc3837c1cbd40cc4f0e28844c70b465650f63aef51b9898"
COLUMNS = {
    "_id": "pc_e1c247ebe28a5cc5c41153fb3eeaefd365bbc199815beee6fb773dd8bd04",
    "code": "pc_35a081b51387859077161194a8dd4a46108aef6499b46565593755528b14",
    "flag": "pc_10dad191362c7a9e7607b9c92cac47bd759510fd4d87c08fc94e22430ce7",
    "units": "pc_5520b9823123f618783d45a4aa20ce87e5a2c0f8b00936779e35ec786457",
    "price": "pc_f1fecc911392a60042fea45680955042e9e3c5e4418e1fe3a839b708a030",
    "created": "pc_f341b4223ed911cdfc17a56a9abf9033227bcb436045940deb2c4ee214e3",
    "uid": "pc_80e5c668691b66ab01a1b4359804676eef5f26174aae7069774c319b06f4",
    "blob": "pc_06ac47b644ca5c0ba74da798f18def3e092369e621f39acc52006a8beb2e",
    "blob_presence": "pc_f8bce95f528b7e1282dce17985b65adba7fafc806e0d406d6be604bbfbc0",
    "weight": "pc_e0697e2bc20cc559b4357131abd0e227b6973f95fd4cb57928e8da39b507",
    "warehouse": "pc_93203681c808f8f290288087497907b5b0fb725adc31139e0512950fe54f",
    "whCode": "pc_ece1b78ea2f5ffc2c5ad9ffe15b8169190aa0503b4e438487efba859a63f",
    "lane": "pc_f917a95c19e905453d50f1e6b3fc27529b11dec667cd7da83e32a150fa5d",
    "memo": "pc_5c588db468ed001ed8250fa651e4a58599ccd167c422a8129e1dbc31594b",
    "memo_presence": "pc_631c74245ee0cd1ab3196358f30a503c0dea87ec36fddd836bc700386d86",
    "marks": "pc_0862418aeec757688afe178747ca772d23e23237e965ec5cf828983403fb",
    "marks_presence": "pc_6d1afdd121fc6725046399c3345420cfec0e4ae3a0373cea9eb17462db17",
    "pack": "pc_a1d5ec125cd55c0cfa04099f4299a5c8eced7c34d551e91cfc2bf298b6f6",
    "pack_presence": "pc_e03a4dd309dd217dfe4e31bc80da6adb650dcfdd3f3953187506db31f0f0",
    "doc": "pc_ef107119f2cd14e89c719da1d0f8d86dcb2483e797325259750ab16b7d1f",
}


def q(name: str) -> str:
    return '"' + name + '"'


def sql_text() -> str:
    parts = [
        f"'pos', {q('_oo_pos')}::text",
        f"'_id', encode({q(COLUMNS['_id'])}, 'hex')",
        f"'code', {q(COLUMNS['code'])}",
        f"'flag', {q(COLUMNS['flag'])}",
        f"'units', {q(COLUMNS['units'])}::text",
        f"'price', {q(COLUMNS['price'])}::text",
        f"'created_ms', ((EXTRACT(EPOCH FROM {q(COLUMNS['created'])}) * 1000)::numeric)::bigint",
        f"'uid', encode({q(COLUMNS['uid'])}, 'hex')",
        f"'blob', encode({q(COLUMNS['blob'])}, 'hex')",
        f"'blob_presence', {q(COLUMNS['blob_presence'])}",
        f"'weight', {q(COLUMNS['weight'])}::text",
        f"'warehouse', {q(COLUMNS['warehouse'])}",
        f"'whCode', {q(COLUMNS['whCode'])}",
        f"'lane', {q(COLUMNS['lane'])}::text",
        f"'memo', {q(COLUMNS['memo'])}",
        f"'memo_presence', {q(COLUMNS['memo_presence'])}",
        f"'marks', {q(COLUMNS['marks'])}::text",
        f"'marks_presence', {q(COLUMNS['marks_presence'])}",
        f"'pack', {q(COLUMNS['pack'])}::text",
        f"'pack_presence', {q(COLUMNS['pack_presence'])}",
        f"'doc', {q(COLUMNS['doc'])}::text",
    ]
    body = ", ".join(parts)
    return (
        "SELECT json_build_object("
        + body
        + f")::text FROM _oo.{q(TABLE)} ORDER BY {q('_oo_pos')}"
    )


def price_text(unscaled: str) -> str:
    scale = 7
    digits = unscaled
    if len(digits) <= scale:
        return "0." + digits.zfill(scale)
    return digits[:-scale] + "." + digits[-scale:]


def hidden(value, path="$"):
    found = []
    if isinstance(value, dict):
        for key, child in value.items():
            if key in {"weight", "notApproved", "price_odd", "mixed_marks"}:
                found.append(f"{path}.{key}")
            found.extend(hidden(child, f"{path}.{key}"))
    elif isinstance(value, list):
        for index, child in enumerate(value):
            found.extend(hidden(child, f"{path}[{index}]"))
    return found


def main() -> int:
    query = sql_text()
    proc = subprocess.run(
        [
            "docker",
            "exec",
            "oo-dl01w1-41-pg",
            "psql",
            "-v",
            "ON_ERROR_STOP=1",
            "-U",
            "oo_w141_admin",
            "-d",
            "oo_w141",
            "-At",
            "-c",
            query,
        ],
        text=True,
        capture_output=True,
        check=False,
    )
    if proc.returncode != 0:
        raise SystemExit(proc.stderr.strip() or "psql failed")
    stored = [json.loads(line) for line in proc.stdout.splitlines() if line]
    expected = json.loads(DECODED.read_text())["records"]
    canonical = json.loads(CANONICAL.read_text())["rows"]
    diffs = []
    if len(stored) != 16 or len(expected) != 16 or len(canonical) != 16:
        diffs.append(f"counts stored={len(stored)} expected={len(expected)} canonical={len(canonical)}")
    for index, (got, want, doc) in enumerate(zip(stored, expected, canonical)):
        checks = {
            "_id": (got["_id"], want["_id"]),
            "code": (got["code"], want["code"]),
            "flag": (got["flag"], want["flag"]),
            "units": (got["units"], want["units"]),
            "price": (got["price"], price_text(want["price"])),
            "created_ms": (got["created_ms"], want["created_ms"]),
            "uid": (got["uid"], want["uid_hex"]),
            "blob": (got["blob"], want["blob_hex"]),
            "blob_presence": (got["blob_presence"], want["blob_presence"]),
            "weight": (got["weight"], format(want["weight"], ".2f") if isinstance(want["weight"], float) else str(want["weight"])),
            "warehouse": (got["warehouse"], want["warehouse"]),
            "whCode": (got["whCode"], want["whCode"]),
            "memo": (got["memo"], want["memo"]),
            "memo_presence": (got["memo_presence"], want["memo_presence"]),
            "marks_presence": (got["marks_presence"], want["marks_presence"]),
            "pack_presence": (got["pack_presence"], want["pack_presence"]),
            "doc": (got["doc"], doc["pg_jsonb_text"]),
        }
        for name, (actual, target) in checks.items():
            if actual != target:
                diffs.append(f"row {index} {name}: {actual!r} != {target!r}")
        lane = json.loads(got["lane"]) if got["lane"] is not None else None
        lane_value = lane.get("value") if isinstance(lane, dict) else lane
        lane_type = lane.get("type") if isinstance(lane, dict) else want["lane_tag"]
        if lane_type != want["lane_tag"]:
            diffs.append(f"row {index} lane type: {lane_type!r} != {want['lane_tag']!r}")
        elif want["lane_tag"] == "utf8":
            if lane_value != want["lane_text"]:
                diffs.append(f"row {index} lane: {lane_value!r} != {want['lane_text']!r}")
        elif want["lane_tag"] == "int64":
            if lane_value != int(want["lane_num"]):
                diffs.append(f"row {index} lane: {lane_value!r} != {want['lane_num']!r}")
        else:
            diffs.append(f"row {index} lane tag {want['lane_tag']}")
        marks = json.loads(got["marks"]) if got["marks"] is not None else None
        if marks != want["marks"]:
            diffs.append(f"row {index} marks: {marks!r} != {want['marks']!r}")
        pack = json.loads(got["pack"]) if got["pack"] is not None else None
        if want["pack_presence"] == 2 and want["pack_kind"] is None:
            if pack != {"kind": None, "n": None}:
                diffs.append(f"row {index} empty pack: {pack!r}")
        elif want["pack_kind"] is None:
            if pack is not None:
                diffs.append(f"row {index} pack: {pack!r}")
        else:
            if pack != {"kind": want["pack_kind"], "n": want["pack_n"]} and pack != {"n": want["pack_n"], "kind": want["pack_kind"]}:
                diffs.append(f"row {index} pack: {pack!r}")
        parsed = json.loads(got["doc"])
        leaked = hidden(parsed)
        if leaked:
            diffs.append(f"row {index} hidden {leaked}")
        if "weight" in parsed:
            diffs.append(f"row {index} doc contains weight")
    report = {
        "protocol": "oo.w141.doc3802-cell-oracle/v1",
        "rows": len(stored),
        "cells_differ": len(diffs),
        "diffs": diffs[:40],
        "doc_oid": 3802,
        "doc_typmod": -1,
        "frozen_arrow_replaced": False,
    }
    OUT.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"rows": len(stored), "cells_differ": len(diffs)}))
    if diffs:
        print("\n".join(diffs[:20]))
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
