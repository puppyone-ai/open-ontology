#!/usr/bin/env python3
"""Emit an OpenSearch capture witness from the committed fast-8 artifacts.

The witness names files. It does not carry a hash, an admitted bit, or a
trusted bit. Row count and Arrow type bytes are read from the IPC stream.
Document ids, PIT, and search_after are read from the capture pages.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path

ROOT = Path("/Users/redux/puppy-one/open-ontology")
EVIDENCE = (
    ROOT
    / "evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight"
)
OUT = EVIDENCE / "witness-20261012"
ARROW = EVIDENCE / "c5-loop-20261012/proof/fast-8/batch.arrow"
AUDIT = EVIDENCE / "c5-loop-20261012/proof/fast-8/audit.jsonl"
PROOF = EVIDENCE / "c5-loop-20261012/proof/fast-8"
DRIVER = EVIDENCE / "c5-loop-20261012/bin/os-reader"
LOAD = EVIDENCE / "native-20261012/phases/load-fast-8/request.json"
SCHEDULE = (
    ROOT / "testing/dl01-driver-preflight/opensearch/fixtures/c5-schedule.json"
)
READ_CLASSES = {"catalog", "cursor_open", "read", "cursor_close", "count"}


def u16(buf: bytes, at: int) -> int:
    return int.from_bytes(buf[at : at + 2], "little")


def u32(buf: bytes, at: int) -> int:
    return int.from_bytes(buf[at : at + 4], "little")


def i32(buf: bytes, at: int) -> int:
    return int.from_bytes(buf[at : at + 4], "little", signed=True)


def i64(buf: bytes, at: int) -> int:
    return int.from_bytes(buf[at : at + 8], "little", signed=True)


def field_slot(buf: bytes, table: int, index: int) -> int | None:
    soffset = i32(buf, table)
    if soffset == 0:
        return None
    vtable = table - soffset if soffset > 0 else table + abs(soffset)
    size = u16(buf, vtable)
    slot = 4 + index * 2
    if slot + 2 > size:
        return None
    rel = u16(buf, vtable + slot)
    if rel == 0:
        return None
    return table + rel


def uoffset_at(buf: bytes, pos: int) -> int:
    return pos + u32(buf, pos)


def read_ipc(data: bytes) -> dict:
    offset = 0
    fields = []
    rows = 0
    schema_sha = None
    saw_batch = False
    while offset + 8 <= len(data):
        continuation = i32(data, offset)
        meta_len = i32(data, offset + 4)
        if meta_len == 0 and continuation in (-1, 0):
            offset += 8
            break
        if continuation != -1 or meta_len <= 0:
            raise SystemExit(f"continuation at {offset}")
        meta = data[offset + 8 : offset + 8 + meta_len]
        root = u32(meta, 0)
        header_type = meta[field_slot(meta, root, 1)]
        body_slot = field_slot(meta, root, 3)
        body_length = 0 if body_slot is None else i64(meta, body_slot)
        if header_type == 1 and not saw_batch and not fields:
            schema_table = uoffset_at(meta, field_slot(meta, root, 2))
            slot = field_slot(meta, schema_table, 1)
            start = uoffset_at(meta, slot)
            count = u32(meta, start)
            for index in range(count):
                table = uoffset_at(meta, start + 4 + index * 4)
                name_slot = field_slot(meta, table, 0)
                name_at = uoffset_at(meta, name_slot)
                name_len = u32(meta, name_at)
                name = meta[name_at + 4 : name_at + 4 + name_len].decode()
                null_slot = field_slot(meta, table, 1)
                nullable = False if null_slot is None else meta[null_slot] == 1
                type_byte = meta[field_slot(meta, table, 2)]
                child_slot = field_slot(meta, table, 5)
                children = 0
                if child_slot is not None:
                    child_start = uoffset_at(meta, child_slot)
                    children = u32(meta, child_start)
                fields.append(
                    {
                        "name": name,
                        "nullable": nullable,
                        "type_byte": type_byte,
                        "children": children,
                    }
                )
            schema_sha = hashlib.sha256(meta).hexdigest()
        elif header_type == 3 and fields:
            batch = uoffset_at(meta, field_slot(meta, root, 2))
            length_slot = field_slot(meta, batch, 0)
            rows += i64(meta, length_slot)
            saw_batch = True
        else:
            raise SystemExit(f"header {header_type} at {offset}")
        meta_end = offset + 8 + meta_len
        body_end = (meta_end + 7) & ~7
        body_end = (body_end + body_length + 7) & ~7
        offset = body_end
    if offset != len(data) or not saw_batch:
        raise SystemExit(f"arrow tail offset={offset} len={len(data)} rows={rows}")
    return {"fields": fields, "rows": rows, "schema_sha256": schema_sha}


def page_ids() -> list[str]:
    found = []
    for path in sorted(PROOF.glob("page-*.json"), key=lambda path: int(path.stem.split("-")[1])):
        page = json.loads(path.read_text())
        for hit in page["hits"]["hits"]:
            found.append(hit["_id"])
    return found


def audit_rows() -> list[dict]:
    rows = []
    for line in AUDIT.read_text().splitlines():
        if line.strip():
            item = json.loads(line)
            rows.append(
                {
                    "n": item["n"],
                    "method": item["method"],
                    "class": item["class"],
                    "status": item["status"],
                    "path": item["path"],
                }
            )
    return rows


def main(generation: int = 4, run_id: str = "os44-g4-c9f4d243ecc0eb6b", snapshot_identity: dict | None = None) -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    arrow = ARROW.read_bytes()
    parsed = read_ipc(arrow)
    ids = page_ids()
    if len(ids) != parsed["rows"]:
        raise SystemExit(f"page ids {len(ids)} != arrow rows {parsed['rows']}")
    audit = audit_rows()
    writes = [row for row in audit if row["class"] not in READ_CLASSES]
    if writes:
        raise SystemExit(f"reader class outside the read set: {writes}")
    load = json.loads(LOAD.read_text())
    declared = [column["name"] for column in load["columns"]]
    observed = [field["name"] for field in parsed["fields"]]
    if declared != observed:
        raise SystemExit("arrow field names differ from the load request")
    schedule = json.loads(SCHEDULE.read_text())
    search_after = []
    for path in sorted(PROOF.glob("search-*.json"), key=lambda path: int(path.stem.split("-")[1])):
        body = json.loads(path.read_text())
        if "search_after" in body:
            search_after.append(body["search_after"])
    pit = json.loads((PROOF / "search-1.json").read_text())["pit"]["id"]
    tick = {
        "protocol": "oo.opensearch.capture-tick/v1",
        "source_engine": "opensearch",
        "source_id": "oo_dl01w1_44",
        "source_generation": generation,
        "label": PROOF.name,
        "snapshot": snapshot_identity or {"kind": "c5_cycle", "value": PROOF.name},
        "logical_schema": "ledger",
        "database": "ledger",
        "product_reader_write_attempts": 0,
        "snapshot_token": False,
        "pit_keepalive": "2m",
        "pit_closed": True,
        "search_after": search_after,
        "cross_table_consistency": "B",
        "reader_audit": audit,
        "unclassified": [{"name": "where_point", "source_type": "geo_point"}],
        "not_approved": [{"name": "c5_extra", "source_type": "keyword", "oos_data_type": None}],
        "tables": [
            {
                "name": "ledger_rows",
                "arrow": ARROW.name,
                "route": "C5",
                "consistency": "B",
                "delete_mode": "reconcile",
                "cursor_safety": "no_cursor",
                "tombstones": load["tombstones"],
                "rows": ids,
            }
        ],
    }
    text = json.dumps(tick)
    if "secret://" in text or "secret_ref" in text:
        raise SystemExit("tick picked up a secret reference")
    capturer_path = OUT / f"{PROOF.name}.tick.json"
    capturer_path.write_text(text + "\n")
    schema = [
        {
            "table": "ledger_rows",
            "columns": [
                {
                    "name": column["name"],
                    "logical_name": column["name"],
                    "arrow_type": column["arrow_type"],
                    "nullable": column["nullable"],
                    **(
                        {"key": True}
                        if column["name"] == load["key_column"]
                        else {}
                    ),
                }
                for column in load["columns"]
            ],
        }
    ]
    schema_path = OUT / "schema-preimage.json"
    schema_path.write_text(json.dumps(schema) + "\n")
    types = [
        {
            "table": "ledger_rows",
            "name": column["name"],
            "arrow_type": column["arrow_type"],
            "approval": (
                "not_approved"
                if column["name"] == "c5_extra"
                else "approved"
                if column["name"] in schedule["approved_fields"]
                else "loader"
            ),
            "oos_data_type": None,
        }
        for column in load["columns"]
    ]
    type_path = OUT / "type-preimage.json"
    type_path.write_text(json.dumps(types) + "\n")
    (OUT / "ipc-fields.json").write_text(json.dumps(parsed, indent=2) + "\n")
    witness = {
        "protocol": "oo.source-admission.capture-witness/v1",
        "source_engine": "opensearch",
        "source_id": "oo_dl01w1_44",
        "source_generation": generation,
        "snapshot": snapshot_identity or {"kind": "c5_cycle", "value": PROOF.name},
        "run_id": run_id,
        "logical_schema": "ledger",
        "logical_table": "ledger_rows",
        "capturer_path": str(capturer_path),
        "arrow_path": str(ARROW),
        "driver_path": str(DRIVER),
        "schema_preimage_path": str(schema_path),
        "type_preimage_path": str(type_path),
    }
    witness_path = OUT / "witness.json"
    witness_path.write_text(json.dumps(witness, indent=2) + "\n")
    driver_digest = hashlib.sha256(DRIVER.read_bytes()).hexdigest()
    report = {
        "protocol": "oo.source-admission.report/v1",
        "target_id": schedule["target"],
        "source_id": "oo_dl01w1_44",
        "secret_ref": schedule["secret_ref"],
        "driver": {
            "kind": "native-http",
            "name": "os-reader",
            "version": "3.9.0",
            "digest": driver_digest,
        },
        "compat_mode": "opensearch-native",
        "license_flags": ["Apache-2.0"],
        "permissions": [
            {
                "name": "indices:data/read/search",
                "status": "known",
                "granted": True,
                "scope": {"index": schedule["approved_index"], "observed": True},
            },
            {
                "name": "indices:data/read/mget",
                "status": "known",
                "granted": False,
                "scope": {"index": schedule["approved_index"], "observed": False},
            },
        ],
        "capabilities": [
            {
                "name": "BatchFull",
                "cursor": None,
                "feed": None,
                "cdc": None,
                "token_order": "None",
                "budget_unit": "Rows",
                "level": "experimental",
                "write_class": "R0",
                "consistency": "B",
            }
        ],
        "capture_methods": [
            {
                "method": "C5",
                "write_class": "R0",
                "consistency": "B",
                "token_order": "None",
                "retention_window": None,
                "cursor": None,
                "feed": None,
                "cdc": None,
                "delete_detection": "reconcile",
            }
        ],
        "cost_model": {"unit": "Rows", "confidence": "unknown", "estimate": str(parsed["rows"])},
        "r1w": None,
        "capture_witness": str(witness_path),
    }
    config = {
        "protocol": "oo.source-admission.config/v1",
        "target_id": schedule["target"],
        "source_id": "oo_dl01w1_44",
        "secret_ref": schedule["secret_ref"],
        "compat_mode": "opensearch-native",
        "permissions": report["permissions"],
        "budget": {
            "unit": schedule["budget_unit"],
            "hard_limit": schedule["budget_rows"],
            "confidence": "unknown",
        },
        "write_options": {
            "signal_table": False,
            "heartbeat_write": False,
            "create_stream": False,
            "create_trigger": False,
        },
        "debezium": {
            "signal_data_collection": None,
            "heartbeat_action_query": None,
            "snapshot_mode": "never",
            "create_stream": False,
            "create_trigger": False,
        },
        "requested_write_class": "R0",
        "r1w": None,
    }
    (OUT / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    (OUT / "config.json").write_text(json.dumps(config, indent=2) + "\n")
    request = dict(load)
    request.pop("admission_path", None)
    request.pop("admission_expect", None)
    request.pop("lease_id", None)
    request.pop("source_bound", None)
    request.pop("sealed_catalog", None)
    request["arrow_path"] = str(ARROW)
    (OUT / "import-request.json").write_text(json.dumps(request) + "\n")
    print(json.dumps({"rows": parsed["rows"], "fields": len(parsed["fields"]), "driver": driver_digest, "ids": ids}, indent=2))


if __name__ == "__main__":
    main()
