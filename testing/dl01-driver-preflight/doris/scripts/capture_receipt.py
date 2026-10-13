#!/usr/bin/env python3
"""Emit oo.source-admission.capture-receipt/v1 from a real Doris capture tick.

The receipt bytes and the import-native-update document share one generation
and the digests of the same files. This script does not connect to Doris and
does not write a password.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

PROTOCOL = "oo.source-admission.capture-receipt/v1"
LOAD_PROTOCOL = "oo.pipeline.native-update/v1"
SOURCE_ENGINE = "doris"
SOURCE_ID = "dorispreview"
LOGICAL_SCHEMA = "oo_dl01w1_43"
OWNER = "5106fed0-5bcd-4aaf-8df7-838c95a7d7e2"
SHARD = "dorispreview"
DRIVER_SHA = "d5dc8555ad76ecc9d4550ac5d2abd6b00bb2fee6ca5a17c7c206d294b206010c"
SEPARATOR = "\u001f"

# Column order and Arrow types are the capture reader's TABLES. The tick does
# not repeat precision. A tick whose name, key model, route, or delete mode
# disagrees with this list is refused.
TABLES = {
    "dup_events": {
        "identity": "duplicate",
        "keys": [],
        "columns": [
            ("event_id", "int64", False, None, None),
            ("kind", "utf8", False, None, None),
            ("qty", "decimal128", True, 10, 2),
        ],
    },
    "agg_qty": {
        "identity": "aggregate",
        "keys": [],
        "columns": [
            ("sku", "utf8", False, None, None),
            ("qty", "decimal128", True, 10, 2),
            ("note", "utf8", True, None, None),
        ],
    },
    "uniq_acct": {
        "identity": "unique",
        "keys": ["id"],
        "columns": [
            ("id", "int64", False, None, None),
            ("balance", "decimal128", True, 18, 4),
            ("label", "utf8", True, None, None),
        ],
    },
    "dec_exact": {
        "identity": "duplicate",
        "keys": [],
        "columns": [
            ("id", "int64", False, None, None),
            ("a18", "decimal128", True, 18, 4),
            ("b18", "decimal128", True, 18, 4),
            ("d38", "decimal128", True, 38, 18),
        ],
    },
    "part_day": {
        "identity": "duplicate",
        "keys": ["day", "region"],
        "columns": [
            ("day", "date32", False, None, None),
            ("region", "utf8", False, None, None),
            ("amount", "decimal128", True, 12, 2),
        ],
    },
    "str_key": {
        "identity": "duplicate",
        "keys": ["k"],
        "columns": [
            ("k", "utf8", False, None, None),
            ("note", "utf8", True, None, None),
        ],
    },
    "nz_obs": {
        "identity": "duplicate",
        "keys": ["id"],
        "columns": [
            ("id", "int64", False, None, None),
            ("qty", "decimal128", True, 10, 2),
        ],
    },
}

ROUTE_METHOD = {"C4": "c4_partition", "C5": "c5_diff"}
ROUTE_DELETE = {"C4": "partition_replace", "C5": "reconcile"}
KEY_MODEL = {"DUPLICATE": "duplicate", "AGGREGATE": "aggregate", "UNIQUE": "unique"}


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


def canonical(value) -> bytes:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()


def write_json(path: Path, value) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(canonical(value) + b"\n")


def u16(data: bytes, at: int) -> int:
    return int.from_bytes(data[at : at + 2], "little")


def i32(data: bytes, at: int) -> int:
    return int.from_bytes(data[at : at + 4], "little", signed=True)


def u32(data: bytes, at: int) -> int:
    return int.from_bytes(data[at : at + 4], "little")


def header_type(meta: bytes) -> int | None:
    if len(meta) < 4:
        return None
    root = u32(meta, 0)
    if root + 4 > len(meta):
        return None
    soffset = i32(meta, root)
    vtable = root - soffset
    if vtable < 0 or vtable + 2 > len(meta):
        return None
    vsize = u16(meta, vtable)
    slot = 6
    if slot + 2 > vsize or vtable + slot + 2 > len(meta):
        return None
    rel = u16(meta, vtable + slot)
    if rel == 0:
        return None
    at = root + rel
    if at >= len(meta):
        return None
    return meta[at]


def arrow_schema_sha256(data: bytes) -> str:
    if data.startswith(b"ARROW1"):
        raise SystemExit("arrow file format is refused; the capture writes a stream")
    if len(data) < 8:
        raise SystemExit("arrow stream is truncated")
    if i32(data, 0) != -1:
        raise SystemExit("arrow continuation is absent")
    meta_len = i32(data, 4)
    if meta_len <= 0 or 8 + meta_len > len(data):
        raise SystemExit("arrow schema message is truncated")
    meta = data[8 : 8 + meta_len]
    if header_type(meta) != 1:
        raise SystemExit("first arrow message is not the schema")
    return sha256_bytes(meta)


def column_decl(spec) -> dict:
    name, arrow_type, nullable, precision, scale = spec
    body = {"name": name, "logical_name": name, "arrow_type": arrow_type, "nullable": nullable}
    if precision is not None:
        body["precision"] = precision
        body["scale"] = scale
    return body


def schema_preimage() -> list:
    return [
        {
            "table": name,
            "columns": [column_decl(spec) for spec in table["columns"]],
        }
        for name, table in TABLES.items()
    ]


def type_preimage() -> list:
    rows = []
    for name, table in TABLES.items():
        for spec in table["columns"]:
            col_name, arrow_type, _nullable, precision, scale = spec
            rows.append(
                {
                    "table": name,
                    "name": col_name,
                    "arrow_type": arrow_type,
                    "precision": precision,
                    "scale": scale,
                }
            )
    return rows


def cells(row: str) -> list:
    out = []
    for part in row.split(SEPARATOR):
        out.append(None if part == r"\N" else part)
    return out


def key_text(table: str, row: str) -> str | None:
    keys = TABLES[table]["keys"]
    if not keys:
        return None
    values = cells(row)
    indexes = [index for index, spec in enumerate(TABLES[table]["columns"]) if spec[0] in keys]
    picked = [values[index] for index in indexes]
    if any(value is None for value in picked):
        raise SystemExit(f"{table} key cell is null")
    if len(picked) == 1:
        return picked[0]
    return SEPARATOR.join(f"{len(value)}:{value}" for value in picked)


def require_tick_table(table: dict, name: str) -> None:
    expect = TABLES[name]
    identity = KEY_MODEL.get(table.get("key_model"))
    if identity != expect["identity"]:
        raise SystemExit(f"{name} key_model {table.get('key_model')} is not {expect['identity']}")
    route = table.get("route")
    if route not in ROUTE_METHOD:
        raise SystemExit(f"{name} route {route} is not C4 or C5")
    if table.get("delete_mode") != ROUTE_DELETE[route]:
        raise SystemExit(f"{name} delete_mode {table.get('delete_mode')} disagrees with {route}")
    if route == "C4" and not expect["keys"]:
        raise SystemExit(f"{name} C4 has no key columns in the capture schema")
    if route == "C5" and identity == "unique" and not expect["keys"]:
        raise SystemExit(f"{name} unique C5 has no key columns")
    if table.get("consistency") != "S0":
        raise SystemExit(f"{name} consistency {table.get('consistency')} is not S0")
    quality = table.get("quality")
    if route == "C5" and quality != "DELETES_LAGGED":
        raise SystemExit(f"{name} C5 quality {quality} is not DELETES_LAGGED")
    if route == "C4" and quality != "partition_metadata_recorded":
        raise SystemExit(f"{name} C4 quality {quality} is not partition_metadata_recorded")


def run_id(tick: dict) -> str:
    label = tick["label"]
    finished = tick["finished_unix_ms"]
    text = f"dorispreview-{label}-{finished}"
    if not text or len(text) > 128 or any(ord(char) < 32 for char in text):
        raise SystemExit("run_id is not a capture label")
    return text


def emit(args: argparse.Namespace) -> None:
    tick_path = Path(args.tick)
    tick = json.loads(tick_path.read_text(encoding="utf-8"))
    if tick.get("protocol") != "oo.doris.capture-tick/v1":
        raise SystemExit("tick protocol is not oo.doris.capture-tick/v1")
    if tick.get("database") != LOGICAL_SCHEMA:
        raise SystemExit("tick database is not oo_dl01w1_43")
    if tick.get("snapshot_token") is not False:
        raise SystemExit("tick claims a snapshot token")
    if tick.get("binlog") != "refused_unpublished":
        raise SystemExit("tick does not refuse unpublished binlog")
    if tick.get("product_reader_write_attempts") != 0:
        raise SystemExit("tick records product reader writes")
    tables = {item["name"]: item for item in tick["tables"]}
    if set(tables) != set(TABLES):
        raise SystemExit("tick tables are not the seven approved objects")
    for name, table in tables.items():
        require_tick_table(table, name)
    driver = Path(args.driver)
    driver_digest = sha256_file(driver)
    if driver_digest != DRIVER_SHA:
        raise SystemExit("driver ELF sha does not match the pinned capture reader")
    config_digest = sha256_file(Path(args.config))
    registry_digest = sha256_file(Path(args.registry))
    audit_digest = sha256_file(Path(args.audit))
    schema_digest = sha256_bytes(canonical(schema_preimage()))
    type_digest = sha256_bytes(canonical(type_preimage()))
    parent_rows = {}
    if args.parent_tick:
        parent = json.loads(Path(args.parent_tick).read_text(encoding="utf-8"))
        parent_rows = {item["name"]: item["rows"] for item in parent["tables"]}
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    index = []
    for name, table in tables.items():
        arrow_name = table["arrow"]
        arrow_path = Path(args.arrow_dir) / arrow_name
        arrow = arrow_path.read_bytes()
        arrow_hash = sha256_bytes(arrow)
        schema_hash = arrow_schema_sha256(arrow)
        route = table["route"]
        identity = TABLES[name]["identity"]
        tombstones = []
        absent = []
        if args.parent_tick and identity == "unique":
            child_keys = {key_text(name, row) for row in table["rows"]}
            for row in parent_rows[name]:
                key = key_text(name, row)
                if key not in child_keys:
                    tombstones.append(key)
        if args.parent_tick and route == "C4":
            child_keys = {key_text(name, row) for row in table["rows"]}
            for row in parent_rows[name]:
                key = key_text(name, row)
                if key not in child_keys:
                    absent.append(key)
        receipt = {
            "arrow_schema_sha256": schema_hash,
            "arrow_sha256": [arrow_hash],
            "audit_digest": audit_digest,
            "catalog_sha256": sha256_bytes(canonical(table)),
            "config_digest": config_digest,
            "cross_table_consistency": tick["cross_table_consistency"],
            "delete_observation": "unknown" if not tombstones and not absent else "key_absent_from_child_snapshot",
            "driver_digest": driver_digest,
            "input_digest": arrow_hash,
            "config_file": str(Path(args.config)),
            "quality": table["quality"],
            "registry_file": str(Path(args.registry)),
            "registry_sha256": registry_digest,
            "run_id": run_id(tick),
            "schema": PROTOCOL,
            "schema_digest": schema_digest,
            "snapshot": {"kind": "capture-tick", "value": tick["label"]},
            "source_engine": SOURCE_ENGINE,
            "source_generation": args.generation,
            "source_id": SOURCE_ID,
            "type_digest": type_digest,
        }
        encoded = canonical(receipt) + b"\n"
        if b"secret://" in encoded or b'"secret_ref"' in encoded:
            raise SystemExit("receipt body names a secret")
        receipt_path = out / f"g{args.generation}-{name}.receipt.json"
        receipt_path.write_bytes(encoded)
        receipt_sha = sha256_bytes(encoded)
        request = {
            "protocol": LOAD_PROTOCOL,
            "source_engine": SOURCE_ENGINE,
            "source_id": SOURCE_ID,
            "source_generation": args.generation,
            "logical_schema": LOGICAL_SCHEMA,
            "logical_table": name,
            "batch_id": f"{tick['label']}-{name}",
            "capture_method": ROUTE_METHOD[route],
            "delete_detection": ROUTE_DELETE[route],
            "consistency_grade": "S0",
            "cross_table_consistent": False,
            "position_codec": "snapshot-zero/v1",
            "position_lineage": "native-update.batch",
            "applied_pos": "0",
            "config_digest": config_digest,
            "schema_digest": schema_digest,
            "type_digest": type_digest,
            "input_digest": arrow_hash,
            "audit_digest": audit_digest,
            "snapshot": {"kind": "capture-tick", "value": tick["label"]},
            "columns": [column_decl(spec) for spec in TABLES[name]["columns"]],
            "arrow_path": str(arrow_path.resolve()),
            "tombstones": tombstones,
            "truth": [cells(row) for row in table["rows"]],
            "row_identity": identity,
            "mirror": False,
            "target": {
                "host": "127.0.0.1",
                "port": 5432,
                "database": args.database,
                "runtime_principal": args.principal,
                "password_file": args.password_file,
                "ssl": "disable",
                "shard": SHARD,
                "owner": OWNER,
            },
            "admission_path": str(receipt_path.resolve()),
            "admission_expect": {"declared_receipt_sha256": receipt_sha},
        }
        if args.generation > 1:
            request["parent_source_generation"] = args.generation - 1
        if TABLES[name]["keys"] and (identity == "unique" or route == "C4"):
            request["key_columns"] = TABLES[name]["keys"]
        request_path = out / f"g{args.generation}-{name}.request.json"
        write_json(request_path, request)
        index.append(
            {
                "table": name,
                "route": route,
                "quality": table["quality"],
                "rows": len(table["rows"]),
                "tombstones": tombstones,
                "c4_keys_absent_not_tombstoned": absent,
                "receipt": receipt_path.name,
                "receipt_sha256": receipt_sha,
                "arrow_sha256": arrow_hash,
                "arrow_schema_sha256": schema_hash,
            }
        )
    write_json(
        out / f"g{args.generation}.index.json",
        {
            "generation": args.generation,
            "label": tick["label"],
            "run_id": run_id(tick),
            "config_digest": config_digest,
            "registry_sha256": registry_digest,
            "audit_digest": audit_digest,
            "schema_digest": schema_digest,
            "type_digest": type_digest,
            "driver_digest": driver_digest,
            "tables": index,
        },
    )
    print(f"emitted generation {args.generation} label {tick['label']} tables {len(index)}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--tick", required=True)
    parser.add_argument("--arrow-dir", required=True)
    parser.add_argument("--audit", required=True)
    parser.add_argument("--config", required=True)
    parser.add_argument("--registry", required=True)
    parser.add_argument("--driver", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--generation", type=int, required=True)
    parser.add_argument("--parent-tick")
    parser.add_argument("--database", default="dorisnative")
    parser.add_argument("--principal", default="oo_admin")
    parser.add_argument("--password-file", default="/secrets/pg_admin_password")
    args = parser.parse_args()
    if args.generation < 1:
        raise SystemExit("generation starts at 1")
    if args.generation > 1 and not args.parent_tick:
        raise SystemExit("a child generation needs the parent tick")
    emit(args)


if __name__ == "__main__":
    try:
        main()
    except BrokenPipeError:
        sys.exit(1)
