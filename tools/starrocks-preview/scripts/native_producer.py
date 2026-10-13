#!/usr/bin/env python3
"""Build trusted StarRocks capture receipts from the sealed capture.

The receipt file and the native-update document share one generation and the
same digests. Digests are hashes of captured bytes. This script does not call
the loader and does not invent a model token.
"""

from __future__ import annotations

import hashlib
import json
import re
import sys
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
CAPTURE = (
    ROOT
    / "evidence/2c023948-0862-4dae-ae8f-6fc30241db62/DL01/starrocks-product/raw/capture-schedule"
)
PROBE = (
    ROOT
    / "evidence/2c023948-0862-4dae-ae8f-6fc30241db62/DL01/starrocks-driver-preflight/build/bin/starrocks-probe"
)
REGISTRY = (
    ROOT
    / "evidence/2c023948-0862-4dae-ae8f-6fc30241db62/DL01/starrocks-product/raw/native-bind/registry.json"
)
GEN1_ARROW = (
    ROOT
    / "evidence/b5ac4bc8-1872-4fe4-a56a-04a70f7103a2/DL01/native-physical-import/source-bound-loader/raw/arrow/gen1.arrow"
)
DEFAULT_OUT = (
    ROOT
    / "evidence/2c023948-0862-4dae-ae8f-6fc30241db62/DL01/starrocks-product/raw/native-chain"
)

DRIVER_SHA = "21bbdc81d436bbffb2951b2245068c1667dccd02ed32a6667b3770b4b7bd1aee"
PK_BASELINE_ARROW_SHA = "4463f9c889619fcd4e84428cb818b4d36c0534b5435b8df42d4ea82bc493ca6a"
GEN1_SCHEMA_SHA = "f4ebfa2fc6765828fee6e8db1d021002c39d0e1896678c12248b04bb838d0439"
RECEIPT_SCHEMA = "oo.source-admission.capture-receipt/v1"
REQUEST_PROTOCOL = "oo.pipeline.native-update/v1"
SOURCE_ID = "sr_w45"
SOURCE_ENGINE = "starrocks"
LOGICAL_SCHEMA = "oo_w45_sr"
DATABASE = "oo_w45_native"
SHARD = "sr_w45"
OWNER = str(uuid.UUID(bytes=hashlib.sha256(b"DL01-W1-45/sr_w45/fixture-owner").digest()[:16]))
PASSWORD_FILE = "/run/secrets/pg.pw"
CLOSED_DELETE = {"native", "partition_replace", "reconcile", "none"}
CLOSED_GRADE = {"T", "S1", "S0", "B"}
METHOD_TOKEN = {"C4": "c4_partition", "C5": "c5_diff"}
IDENTITY = {
    "PRIMARY": "unique",
    "UNIQUE": "unique",
    "DUPLICATE": "duplicate",
    "AGGREGATE": "aggregate",
}
TABLES = ("dup_events", "pk_accounts", "agg_metrics", "uniq_dim", "dup_partition_sums")
PHASES = (
    {"name": "baseline", "generation": 1, "parent": None, "config": "initial-decisions.json", "tick": None},
    {"name": "catchup", "generation": 2, "parent": 1, "config": "config-fast.json", "tick": "ticks/catchup.json"},
    {"name": "slow", "generation": 3, "parent": 2, "config": "config-slow.json", "tick": "ticks/slow.json"},
    {"name": "expired", "generation": 4, "parent": 3, "config": "config-slow.json", "tick": "ticks/expired.json"},
)


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


def canon(value) -> bytes:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()


def align8(value: int) -> int:
    return (value + 7) & ~7


def _need(data: bytes, at: int, size: int) -> None:
    if at < 0 or size < 0 or at + size > len(data):
        raise ValueError("truncated")


def _u8(data: bytes, at: int) -> int:
    _need(data, at, 1)
    return data[at]


def _u16(data: bytes, at: int) -> int:
    _need(data, at, 2)
    return int.from_bytes(data[at : at + 2], "little")


def _u32(data: bytes, at: int) -> int:
    _need(data, at, 4)
    return int.from_bytes(data[at : at + 4], "little")


def _as_i32(value: int) -> int:
    return value - 0x100000000 if value >= 0x80000000 else value


def _i64(data: bytes, at: int) -> int:
    _need(data, at, 8)
    return int.from_bytes(data[at : at + 8], "little", signed=True)


class _Table:
    def __init__(self, data: bytes, start: int):
        soff = _as_i32(_u32(data, start))
        vtable = start - soff
        vsize = _u16(data, vtable)
        if vsize < 4 or vsize % 2 != 0:
            raise ValueError("schema")
        _need(data, vtable, vsize)
        self.data = data
        self.start = start
        self.vtable = vtable
        self.vsize = vsize

    def field(self, index: int):
        slot = 4 + index * 2
        if slot + 2 > self.vsize:
            return None
        rel = _u16(self.data, self.vtable + slot)
        if rel == 0:
            return None
        return self.start + rel

    def u8_field(self, index: int):
        at = self.field(index)
        if at is None:
            return None
        return _u8(self.data, at)

    def i64_field(self, index: int):
        at = self.field(index)
        if at is None:
            return None
        return _i64(self.data, at)


def _root(meta: bytes) -> _Table:
    return _Table(meta, _u32(meta, 0))


def _frames(data: bytes):
    if data.startswith(b"ARROW1"):
        body = _file_interior(data)
        require_eos = False
    else:
        body = data
        require_eos = True
    offset = 0
    saw_eos = False
    frames = []
    while offset + 8 <= len(body):
        continuation = _as_i32(_u32(body, offset))
        meta_len = _as_i32(_u32(body, offset + 4))
        if continuation != -1 or meta_len < 0:
            raise ValueError("truncated")
        if meta_len == 0:
            if offset + 8 != len(body):
                raise ValueError("truncated")
            return frames
        meta_end = offset + 8 + meta_len
        if meta_end > len(body):
            raise ValueError("truncated")
        meta = body[offset + 8 : meta_end]
        body_len = _root(meta).i64_field(3) or 0
        if body_len < 0:
            raise ValueError("truncated")
        body_start = align8(meta_end)
        body_end = body_start + body_len
        if body_end > len(body):
            raise ValueError("truncated")
        frames.append(meta)
        nxt = body_end
        if nxt % 8 != 0:
            aligned = align8(nxt)
            if aligned > len(body) or any(byte != 0 for byte in body[nxt:aligned]):
                raise ValueError("truncated")
            nxt = aligned
        offset = nxt
    if offset != len(body) or (require_eos and not saw_eos):
        raise ValueError("truncated")
    return frames


def _file_interior(data: bytes) -> bytes:
    if len(data) < 16 or data[-6:] != b"ARROW1":
        raise ValueError("truncated")
    size_at = len(data) - 10
    footer_len = _as_i32(_u32(data, size_at))
    if footer_len <= 0:
        raise ValueError("truncated")
    footer_start = size_at - footer_len
    if footer_start < 8:
        raise ValueError("truncated")
    at = 6
    if at % 8 != 0:
        aligned = align8(at)
        if aligned > footer_start or any(byte != 0 for byte in data[at:aligned]):
            raise ValueError("truncated")
        at = aligned
    while at + 8 <= footer_start:
        if _as_i32(_u32(data, at)) == -1:
            break
        if any(byte != 0 for byte in data[at : at + 8]):
            raise ValueError("truncated")
        at += 8
    if at + 8 > footer_start:
        raise ValueError("truncated")
    return data[at:footer_start]


def arrow_schema_sha256(data: bytes) -> str:
    """SHA-256 of the Arrow schema message meta. Matches native_arrow.rs."""
    for meta in _frames(data):
        if not meta:
            continue
        if _root(meta).u8_field(1) == 1:
            return sha256_bytes(meta)
    raise ValueError("schema")


def assert_schema_oracle() -> None:
    got = arrow_schema_sha256(GEN1_ARROW.read_bytes())
    if got != GEN1_SCHEMA_SHA:
        raise SystemExit(f"schema oracle mismatch {got}")


def loader_column(field: dict) -> dict:
    spell = field["type"]
    column = {
        "name": field["name"],
        "logical_name": field["name"],
        "nullable": bool(field["nullable"]),
    }
    decimal = re.fullmatch(r"decimal128\((\d+),(\d+)\)", spell)
    if decimal:
        column["arrow_type"] = "decimal128"
        column["precision"] = int(decimal.group(1))
        column["scale"] = int(decimal.group(2))
    elif spell == "int64":
        column["arrow_type"] = "int64"
        column["precision"] = 64
    elif spell in {"date32", "timestamp_second_naive", "utf8"}:
        column["arrow_type"] = spell
    else:
        raise ValueError(f"unmeasured arrow type {spell}")
    # The loader treats char_length as bpchar(n), and rejects n above 63.
    # StarRocks varchar(n) stays unbounded varchar here. native_type remains on the schema preimage.
    return column


def cell_text(value):
    if value is None:
        return None
    if isinstance(value, bool) or isinstance(value, float):
        raise ValueError("inexact cell")
    if isinstance(value, int):
        return str(value)
    if isinstance(value, str):
        return value
    raise ValueError("inexact cell")


def truth_rows(decoded: dict) -> list:
    names = [field["name"] for field in decoded["fields"]]
    rows = []
    for row in decoded["rows"]:
        rows.append([cell_text(row.get(name)) for name in names])
    return rows


def parse_tsv(text: str, width: int) -> list:
    lines = text.splitlines()
    rows = []
    for line in lines:
        parts = line.split("\t")
        if len(parts) != width:
            raise ValueError(f"tsv width {len(parts)} != {width}")
        rows.append([None if part == "NULL" else part for part in parts])
    return rows


def bag(rows: list) -> list:
    return sorted(json.dumps(row, ensure_ascii=False) for row in rows)


def backtick_names(text: str) -> list:
    return re.findall(r"`([^`]+)`", text or "")


def load_json(path: Path):
    return json.loads(path.read_text())


def decision_for(phase: dict, table: str) -> dict:
    if phase["tick"] is None:
        document = load_json(CAPTURE / "initial-decisions.json")
        item = document[table]
        if "token" not in item or "method" not in item or "grade" not in item:
            raise ValueError(f"baseline decision incomplete for {table}")
        return item
    document = load_json(CAPTURE / phase["tick"])
    if document.get("outcome") != "committed":
        raise ValueError(f"{phase['name']} is not a committed tick")
    return document["tables"][table]


def delete_token(decision: dict) -> str | None:
    raw = decision.get("delete_detection")
    if raw in CLOSED_DELETE:
        return raw
    quality = decision.get("quality") or []
    if "DELETES_UNDETECTED" in quality:
        return "none"
    method = decision.get("method")
    if method == "C5":
        return "reconcile"
    if method == "C4":
        return "partition_replace"
    return None


def key_columns(table: str, catalog_obj: dict | None, fields: list) -> tuple[str, list]:
    if table == "dup_partition_sums":
        return "aggregate", ["part"]
    if catalog_obj is None:
        raise ValueError(f"no catalog object for {table}")
    identity = IDENTITY.get(catalog_obj["key_model"])
    if identity is None:
        raise ValueError(f"unmeasured key model {catalog_obj['key_model']}")
    names = backtick_names((catalog_obj.get("physical_hints") or {}).get("primary_key") or "")
    if not names and identity == "aggregate":
        names = [field["name"] for field in fields if not field["nullable"]]
    if not names and identity == "unique":
        names = [
            field["name"]
            for field in fields
            if field["type"] == "int64" and not field["nullable"]
        ]
    if identity == "duplicate":
        names = []
    return identity, names


def catalog_object(catalog: dict, table: str):
    for item in catalog["objects"]:
        if item["name"] == table:
            return item
    return None


def build_one(phase: dict, table: str, baseline_columns: dict) -> dict:
    decoded_path = CAPTURE / "decoded" / phase["name"] / f"{table}.json"
    arrow_path = CAPTURE / "reads" / phase["name"] / f"{table}.arrow"
    tsv_path = CAPTURE / "native" / phase["name"] / f"{table}.tsv"
    decoded = load_json(decoded_path)
    if decoded.get("protocol") != "oo.source.starrocks.ipc-decode/v1":
        raise ValueError(f"unexpected decode protocol for {table}")
    columns = [loader_column(field) for field in decoded["fields"]]
    signature = [(column["name"], column["arrow_type"], column["nullable"], column.get("precision"), column.get("scale")) for column in columns]
    if phase["generation"] == 1:
        baseline_columns[table] = signature
    elif baseline_columns.get(table) != signature:
        return {"table": table, "generation": phase["generation"], "emitted": False, "reason": "schema_differs_from_baseline"}
    truth = truth_rows(decoded)
    tsv = parse_tsv(tsv_path.read_text(), len(columns))
    if bag(truth) != bag(tsv):
        raise ValueError(f"decoded cells differ from native tsv for {phase['name']} {table}")
    decision = decision_for(phase, table)
    grade = decision.get("grade")
    method = METHOD_TOKEN.get(decision.get("method"))
    delete = delete_token(decision)
    token = decision.get("token")
    if grade not in CLOSED_GRADE or method is None or delete is None or not isinstance(token, str) or len(token) != 64:
        return {
            "table": table,
            "generation": phase["generation"],
            "emitted": False,
            "reason": "grade_or_delete_unmeasured",
            "grade": grade,
            "method": decision.get("method"),
            "delete_detection": decision.get("delete_detection"),
        }
    catalog = load_json(CAPTURE / "reads" / phase["name"] / "catalog.json")
    identity, keys = key_columns(table, catalog_object(catalog, table), decoded["fields"])
    arrow = arrow_path.read_bytes()
    arrow_sha = sha256_bytes(arrow)
    if phase["name"] == "baseline" and table == "pk_accounts" and arrow_sha != PK_BASELINE_ARROW_SHA:
        raise SystemExit(f"pk baseline arrow changed {arrow_sha}")
    schema_hash = arrow_schema_sha256(arrow)
    config_path = CAPTURE / phase["config"]
    audit_path = CAPTURE / "reads" / phase["name"] / "audit-counts.json"
    catalog_path = CAPTURE / "reads" / phase["name"] / "catalog.json"
    preimage = catalog_object(catalog, table)
    if preimage is None:
        preimage = {
            "base_create": False,
            "name": table,
            "probe_aggregate": True,
            "fields": decoded["fields"],
        }
    else:
        preimage = {"base_create": True, "columns": preimage["columns"], "key_model": preimage["key_model"], "name": table}
    quality = list(decision.get("quality") or [])
    native_note = (
        "full-image snapshot of a finished read; upsert_supported is false; "
        "quality tokens stay on quality and are not storage-caveat tokens"
    )
    if table == "dup_partition_sums":
        native_note += "; probe aggregate over dup_events, not a base CREATE"
    if table == "uniq_dim":
        native_note += "; engine UPDATE on this unique-key table failed in the capture and is not rewritten as an upsert"
    run_id = f"{phase['name']}.{table}.{token}"
    if len(run_id) > 128:
        raise ValueError("run_id too long")
    receipt = {
        "schema": RECEIPT_SCHEMA,
        "source_engine": SOURCE_ENGINE,
        "source_id": SOURCE_ID,
        "source_generation": phase["generation"],
        "snapshot": {"kind": "token", "value": token},
        "run_id": run_id,
        "schema_digest": sha256_bytes(canon(preimage)),
        "type_digest": sha256_bytes(canon(columns)),
        "config_digest": sha256_file(config_path),
        "input_digest": arrow_sha,
        "audit_digest": sha256_file(audit_path),
        "catalog_sha256": sha256_file(catalog_path),
        "registry_sha256": sha256_file(REGISTRY),
        "driver_digest": sha256_file(PROBE),
        "arrow_sha256": [arrow_sha],
        "arrow_schema_sha256": schema_hash,
        "config_path": str(config_path.relative_to(CAPTURE)),
        "audit_path": str(audit_path.relative_to(CAPTURE)),
        "catalog_path": str(catalog_path.relative_to(CAPTURE)),
        "quality": quality,
        "native_note": native_note,
        "storage_caveats": [],
        "full_snapshot": True,
        "upsert_supported": False,
        "window_closed": True,
    }
    if receipt["driver_digest"] != DRIVER_SHA:
        raise SystemExit(f"driver digest {receipt['driver_digest']}")
    document = {
        "protocol": REQUEST_PROTOCOL,
        "source_engine": SOURCE_ENGINE,
        "source_id": SOURCE_ID,
        "source_generation": phase["generation"],
        "logical_schema": LOGICAL_SCHEMA,
        "logical_table": table,
        "batch_id": f"g{phase['generation']}-{table}",
        "capture_method": method,
        "delete_detection": delete,
        "consistency_grade": grade,
        "cross_table_consistent": False,
        "position_codec": "snapshot-zero/v1",
        "position_lineage": "native-update.batch",
        "applied_pos": "0",
        "schema_digest": receipt["schema_digest"],
        "type_digest": receipt["type_digest"],
        "config_digest": receipt["config_digest"],
        "input_digest": receipt["input_digest"],
        "audit_digest": receipt["audit_digest"],
        "snapshot": {"kind": "token", "value": token},
        "columns": columns,
        "arrow_path": str(arrow_path),
        "tombstones": [],
        "truth": truth,
        "row_identity": identity,
        "mirror": False,
        "full_snapshot": True,
        "upsert_supported": False,
        "window_closed": True,
        "target": {
            "host": "127.0.0.1",
            "port": 5432,
            "database": DATABASE,
            "runtime_principal": "postgres",
            "password_file": PASSWORD_FILE,
            "ssl": "disable",
            "shard": SHARD,
            "owner": OWNER,
        },
        "quality": quality,
        "native_note": native_note,
    }
    if keys:
        document["key_columns"] = keys
    if phase["parent"] is not None:
        document["parent_source_generation"] = phase["parent"]
    return {
        "table": table,
        "generation": phase["generation"],
        "phase": phase["name"],
        "emitted": True,
        "receipt": receipt,
        "document": document,
        "rows": len(truth),
        "arrow_sha256": arrow_sha,
        "arrow_schema_sha256": schema_hash,
    }


def emit(out_dir: Path) -> dict:
    assert_schema_oracle()
    if not PROBE.is_file():
        raise SystemExit("probe elf missing")
    out_dir.mkdir(parents=True, exist_ok=True)
    baseline_columns: dict = {}
    loads = []
    skipped = []
    for phase in PHASES:
        for table in TABLES:
            built = build_one(phase, table, baseline_columns)
            if not built.get("emitted"):
                skipped.append(built)
                continue
            slot = out_dir / f"g{phase['generation']}" / table
            slot.mkdir(parents=True, exist_ok=True)
            receipt_path = slot / "receipt.json"
            receipt_bytes = json.dumps(built["receipt"], ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode() + b"\n"
            receipt_path.write_bytes(receipt_bytes)
            receipt_sha = sha256_bytes(receipt_bytes)
            document = built["document"]
            document["admission_path"] = str(receipt_path)
            document["admission_expect"] = {
                "source_engine": SOURCE_ENGINE,
                "source_id": SOURCE_ID,
                "source_generation": phase["generation"],
                "declared_receipt_sha256": receipt_sha,
            }
            request_path = slot / "request.json"
            request_path.write_bytes(json.dumps(document, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode() + b"\n")
            blob = json.dumps(document)
            if "secret://" in blob or "secret_ref" in blob:
                raise SystemExit("secret marker in request")
            loads.append(
                {
                    "phase": phase["name"],
                    "generation": phase["generation"],
                    "parent": phase["parent"],
                    "table": table,
                    "rows": built["rows"],
                    "request": str(request_path),
                    "receipt": str(receipt_path),
                    "receipt_sha256": receipt_sha,
                    "arrow_sha256": built["arrow_sha256"],
                    "arrow_schema_sha256": built["arrow_schema_sha256"],
                    "capture_method": document["capture_method"],
                    "delete_detection": document["delete_detection"],
                    "consistency_grade": document["consistency_grade"],
                    "batch_id": document["batch_id"],
                    "snapshot": document["snapshot"]["value"],
                }
            )
    manifest = {
        "protocol": "oo.starrocks.native-producer/v1",
        "source_id": SOURCE_ID,
        "source_engine": SOURCE_ENGINE,
        "owner": OWNER,
        "driver_sha256": DRIVER_SHA,
        "schema_oracle": GEN1_SCHEMA_SHA,
        "loads": loads,
        "skipped": skipped,
        "model_called": False,
        "upsert_supported": False,
        "full_snapshot": True,
    }
    (out_dir / "manifest.json").write_bytes(json.dumps(manifest, ensure_ascii=False, indent=2).encode() + b"\n")
    return manifest


def main(argv: list[str]) -> int:
    if len(argv) < 2 or argv[1] != "emit":
        print("usage: native_producer.py emit [--out DIR]", file=sys.stderr)
        return 2
    out = DEFAULT_OUT
    if "--out" in argv:
        out = Path(argv[argv.index("--out") + 1])
    manifest = emit(out)
    print(json.dumps({"loads": len(manifest["loads"]), "skipped": manifest["skipped"], "owner": OWNER}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
