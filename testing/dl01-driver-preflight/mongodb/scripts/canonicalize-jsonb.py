#!/usr/bin/env python3
"""Ask the task Postgres for canonical jsonb text. No password is printed."""

from __future__ import annotations

import hashlib
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path("/Users/redux/puppy-one/open-ontology")
DECODE = ROOT / "evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-66-decode"
CAPTURE = ROOT / "evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-driver-preflight/raw/repair-64-fulldoc/probe"
OUT = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-67-canonical"


def psql(sql: str) -> str:
    proc = subprocess.run(
        [
            "docker",
            "exec",
            "-i",
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
            sql,
        ],
        text=True,
        capture_output=True,
        check=False,
    )
    if proc.returncode != 0:
        raise SystemExit(proc.stderr.strip() or f"psql {proc.returncode}")
    return proc.stdout.strip()


def canonical(payload: str) -> tuple[str, str]:
    hx = payload.encode().hex()
    sql = (
        "SELECT encode(((convert_from(decode('"
        + hx
        + "','hex'),'UTF8')::jsonb)::text)::bytea, 'hex') || ' ' || "
        "encode(jsonb_send(convert_from(decode('"
        + hx
        + "','hex'),'UTF8')::jsonb), 'hex')"
    )
    text_hex, send_hex = psql(sql).split(" ", 1)
    text = bytes.fromhex(text_hex).decode()
    if send_hex != "01" + text.encode().hex():
        raise SystemExit("jsonb_send is not version byte plus text")
    return text, send_hex


def envelope(dataset: str, column: str, raw_hex: str, text: str, send_hex: str) -> dict:
    raw = bytes.fromhex(raw_hex)
    return {
        "protocol": "oo.document.bson-jsonb/v1",
        "dataset": dataset,
        "column": column,
        "oid": 3802,
        "typmod": -1,
        "source_type": "jsonb",
        "raw_bson_dropped": False,
        "raw_bson_hex": raw_hex,
        "raw_bson_sha256": hashlib.sha256(raw).hexdigest(),
        "pg_jsonb_hex": send_hex,
        "pg_jsonb_text": text,
        "loss_concealed": False,
        "missing_is_null": False,
    }


def main() -> int:
    if OUT.exists():
        raise SystemExit("output dir exists")
    seal_lines = [line for line in (CAPTURE / "seal-scope.bson.hex").read_text().splitlines() if line]
    full_lines = [line for line in (CAPTURE / "full-approved.bson.hex").read_text().splitlines() if line]
    decoded = [json.loads(line) for line in (DECODE / "decode-seal.jsonl").read_text().splitlines() if line]
    if len(seal_lines) != 16 or len(full_lines) != 16 or len(decoded) != 16:
        raise SystemExit("row count is not 16")
    OUT.mkdir(parents=True)
    (OUT / "envelopes").mkdir()
    rows = []
    for item in decoded:
        index = item["index"]
        payload = json.dumps(item["json"], separators=(",", ":"), ensure_ascii=False)
        if "weight" in item["json"]:
            raise SystemExit(f"weight present in seal json {index}")
        text, send_hex = canonical(payload)
        if "weight" in json.loads(text):
            raise SystemExit(f"weight present after postgres {index}")
        body = envelope("oo_w141.shipments", "doc", seal_lines[index], text, send_hex)
        path = OUT / "envelopes" / f"seal-{index:02d}.json"
        path.write_text(json.dumps(body, ensure_ascii=False) + "\n")
        if index == 0:
            side = envelope("oo_w141.shipment_doc", "doc", seal_lines[index], text, send_hex)
            (OUT / "envelopes" / "side-00.json").write_text(json.dumps(side, ensure_ascii=False) + "\n")
        rows.append({"index": index, "pg_jsonb_text": text, "pg_jsonb_sha256": hashlib.sha256(bytes.fromhex(send_hex)).hexdigest()})
    # Public seal of the full BSON must refuse. The pg bytes are a closed empty object,
    # not a claim that the full document equals that object.
    empty_text, empty_send = canonical("{}")
    for index, raw_hex in enumerate(full_lines):
        body = envelope("oo_w141.shipments", "doc", raw_hex, empty_text, empty_send)
        (OUT / "envelopes" / f"full-{index:02d}.json").write_text(json.dumps(body, ensure_ascii=False) + "\n")
    (OUT / "canonical.json").write_text(json.dumps({"rows": rows, "empty_send": empty_send}, indent=2) + "\n")
    (OUT / "status.txt").write_text("DONE\n")
    print(f"rows {len(rows)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
