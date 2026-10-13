"""Oracle C5 witness for ELF e465241f.

Reads the committed 450db dictionary, shadow cells, site-proof grants, and
Arrow IPC. Writes report, config, capturer, witness, and preimages. Does not
set admitted or trusted, and does not invent a schema digest.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CAPTURE = (
    ROOT
    / "evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/c5-capture"
)
ARROW_DIR = CAPTURE / "run" / "arrow"
PROFILE_PATH = CAPTURE / "run" / "producer-profile.json"
SITE_PATH = CAPTURE / "native-join" / "site-proof" / "result.json"

SCHEMA = "OO04_FIX"
SECRET_REF = "secret://oo04p2/driver/source/reader"
TARGET_ID = "oo04_fix"
BUDGET_LIMIT = 8
PRODUCT = "Oracle AI Database 26ai Free"
PRODUCT_VERSION = "23.26.3.0.0"

TABLES = (
    {
        "table": "T_WIDE",
        "generation": 1,
        "arrow": "t_wide-gen-1.arrow",
        "source_id": "oo04_c5_wide",
    },
    {
        "table": "T_TS",
        "generation": 1,
        "arrow": "t_ts-gen-1.arrow",
        "source_id": "oo04_c5_ts",
    },
    {
        "table": "T_BATCH",
        "generation": 8,
        "arrow": "t_batch-gen-8.arrow",
        "source_id": "oo04_c5_batch",
    },
)

SENT_PRIVILEGES = frozenset({"CREATE SESSION", "SELECT", "FLASHBACK"})
SCHEMA_MESSAGE = {
    "t_batch-gen-1.arrow": "c8976240f10e8de32d9afa1f577e1c14881f16996ffe3eeead6fa909b47a0a09",
    "t_batch-gen-3.arrow": "c8976240f10e8de32d9afa1f577e1c14881f16996ffe3eeead6fa909b47a0a09",
    "t_batch-gen-8.arrow": "61fc7a7afa8825d3f66f039a0e8659c2a4cdc0213d1fd25aab79f6948010392e",
    "t_ts-gen-1.arrow": "ddb113133d899319262fc924d1845c222dfdb6cff2c436ee41e3ec8a8571ece6",
    "t_wide-gen-1.arrow": "a5f91543c47e3f31fdb3fe0707c9fb9629410d36d1c8697d2bf3cf08dd3ccea4",
}
STREAM_SHA = {
    "t_batch-gen-1.arrow": "99a3e8fb6db06ede3386bc3f48f5acbefec32d71cf768bae9db6d83674aed2b7",
    "t_batch-gen-3.arrow": "afb890ab496b91f746bcc48a1fd92ca79b54e1e4bf18fa7ffd45ad98dbc2285a",
    "t_batch-gen-8.arrow": "d12c786b0426d113152f3879cc57dde50094616c7309882755b2b0a66291c415",
    "t_ts-gen-1.arrow": "25896f7c56f8bdb9033c98c48980d63a1fc310a506107072582f8f19d9cb02cf",
    "t_wide-gen-1.arrow": "e0ff61f78e8e10bb37e45baf7932e85c26ecfa3aaadbc514879fd34920132f68",
}
ROW_COUNT = {
    "t_batch-gen-1.arrow": 5,
    "t_batch-gen-3.arrow": 6,
    "t_batch-gen-8.arrow": 5,
    "t_ts-gen-1.arrow": 2,
    "t_wide-gen-1.arrow": 4,
}


def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _u16(buf: bytes, at: int) -> int:
    return int.from_bytes(buf[at : at + 2], "little")


def _u32(buf: bytes, at: int) -> int:
    return int.from_bytes(buf[at : at + 4], "little")


def _i16(buf: bytes, at: int) -> int:
    return int.from_bytes(buf[at : at + 2], "little", signed=True)


def _i32(buf: bytes, at: int) -> int:
    return int.from_bytes(buf[at : at + 4], "little", signed=True)


def _i64(buf: bytes, at: int) -> int:
    return int.from_bytes(buf[at : at + 8], "little", signed=True)


def _align8(value: int) -> int:
    return (value + 7) & ~7


def _field_slot(buf: bytes, table: int, index: int) -> int | None:
    soffset = _i32(buf, table)
    if soffset == 0:
        return None
    vtable = table - soffset if soffset > 0 else table + (-soffset)
    if vtable < 0 or vtable + 2 > len(buf):
        return None
    size = _u16(buf, vtable)
    if size < 4 or vtable + size > len(buf):
        return None
    slot = 4 + index * 2
    if slot + 2 > size:
        return None
    rel = _u16(buf, vtable + slot)
    if rel == 0:
        return None
    return table + rel


def _uoffset_at(buf: bytes, pos: int) -> int:
    return pos + _u32(buf, pos)


def _slot_uoffset(buf: bytes, table: int, index: int) -> int | None:
    pos = _field_slot(buf, table, index)
    if pos is None:
        return None
    return _uoffset_at(buf, pos)


def _offset_vector(buf: bytes, slot: int) -> list[int]:
    start = _uoffset_at(buf, slot)
    count = _u32(buf, start)
    if count > 64:
        raise ValueError("schema")
    tables = []
    for index in range(count):
        tables.append(_uoffset_at(buf, start + 4 + index * 4))
    return tables


def _string_at(buf: bytes, slot: int) -> str:
    start = _uoffset_at(buf, slot)
    length = _u32(buf, start)
    return buf[start + 4 : start + 4 + length].decode("utf-8")


def _bool_at(buf: bytes, pos: int) -> bool:
    value = buf[pos]
    if value not in (0, 1):
        raise ValueError("schema")
    return value == 1


def _name_type(buf: bytes, type_byte: int, table: int | None) -> dict:
    if type_byte == 2:
        if table is None:
            raise ValueError("schema")
        width = _i32(buf, _field_slot(buf, table, 0))
        signed_slot = _field_slot(buf, table, 1)
        signed = False if signed_slot is None else _bool_at(buf, signed_slot)
        names = {
            (True, 8): "int8",
            (True, 16): "int16",
            (True, 32): "int32",
            (True, 64): "int64",
            (False, 8): "uint8",
            (False, 16): "uint16",
            (False, 32): "uint32",
            (False, 64): "uint64",
        }
        name = names.get((signed, width))
        if name is None:
            raise ValueError("unsupported")
        return {"arrow_type": name, "precision": width, "scale": None, "supported": True}
    if type_byte == 4:
        return {"arrow_type": "binary", "precision": None, "scale": None, "supported": True}
    if type_byte == 5:
        return {"arrow_type": "utf8", "precision": None, "scale": None, "supported": True}
    if type_byte == 6:
        return {"arrow_type": "bool", "precision": None, "scale": None, "supported": True}
    if type_byte == 7:
        if table is None:
            raise ValueError("schema")
        precision = _i32(buf, _field_slot(buf, table, 0))
        scale = _i32(buf, _field_slot(buf, table, 1))
        bits_slot = _field_slot(buf, table, 2)
        bits = 128 if bits_slot is None else _i32(buf, bits_slot)
        if bits == 128:
            name = "decimal128"
            limit = 38
        elif bits == 256:
            name = "decimal256"
            limit = 76
        else:
            raise ValueError("unsupported")
        if not (1 <= precision <= limit) or not (0 <= scale <= precision):
            raise ValueError("unsupported")
        return {
            "arrow_type": name,
            "precision": precision,
            "scale": scale,
            "supported": True,
        }
    if type_byte == 8:
        if table is None:
            raise ValueError("schema")
        unit_slot = _field_slot(buf, table, 0)
        if unit_slot is None:
            raise ValueError("unsupported")
        unit = _i16(buf, unit_slot)
        name = {0: "date32", 1: "date64"}.get(unit)
        if name is None:
            raise ValueError("unsupported")
        return {"arrow_type": name, "precision": None, "scale": None, "supported": True}
    if type_byte == 10:
        if table is None:
            raise ValueError("schema")
        # Flatbuffers omit scalar defaults; Arrow TimeUnit defaults to SECOND.
        unit_slot = _field_slot(buf, table, 0)
        unit = 0 if unit_slot is None else _i16(buf, unit_slot)
        zone_slot = _field_slot(buf, table, 1)
        zone = None if zone_slot is None else _string_at(buf, zone_slot)
        zone = zone or None
        names = {(0, None): "timestamp_second_naive", (0, "UTC"): "timestamp_second_utc",
                 (1, None): "timestamp_millis_naive", (1, "UTC"): "timestamp(ms,utc)",
                 (2, None): "timestamp_micros", (3, "UTC"): "timestamp_nanos_utc"}
        name = names.get((unit, zone))
        if unit == 2 and zone is not None:
            name = "timestamp_micros_utc"
        if name is None:
            raise ValueError("unsupported")
        result = {"arrow_type": name, "precision": None, "scale": None, "supported": True}
        if zone is not None and unit != 1:
            result["timezone"] = zone
        return result
    return {"arrow_type": None, "precision": None, "scale": None, "supported": False, "type_byte": type_byte}


def _read_field(buf: bytes, table: int) -> dict:
    name_slot = _field_slot(buf, table, 0)
    if name_slot is None:
        raise ValueError("schema")
    name = _string_at(buf, name_slot)
    nullable_slot = _field_slot(buf, table, 1)
    nullable = False if nullable_slot is None else _bool_at(buf, nullable_slot)
    if _field_slot(buf, table, 4) is not None:
        raise ValueError("unsupported")
    children_slot = _field_slot(buf, table, 5)
    if children_slot is not None and _offset_vector(buf, children_slot):
        raise ValueError("unsupported")
    type_slot = _field_slot(buf, table, 2)
    if type_slot is None:
        raise ValueError("unsupported")
    type_byte = buf[type_slot]
    type_table_slot = _field_slot(buf, table, 3)
    type_table = None if type_table_slot is None else _uoffset_at(buf, type_table_slot)
    typed = _name_type(buf, type_byte, type_table)
    return {
        "name": name,
        "nullable": nullable,
        "type_byte": type_byte,
        **typed,
    }


def parse_arrow(data: bytes) -> dict:
    """Walk one IPC stream the way seal-native does, and keep a diagnostic.

    A timestamp field makes `supported` false. The schema-message hash is still
    the metadata slice, so a refused stream stays identifiable.
    """
    if data.startswith(b"ARROW1") or len(data) < 8:
        raise ValueError("truncated")
    offset = 0
    schema = None
    schema_sha = None
    rows = 0
    saw_batch = False
    while offset + 8 <= len(data):
        continuation = _i32(data, offset)
        meta_len = _i32(data, offset + 4)
        if meta_len == 0 and continuation in (-1, 0):
            offset += 8
            break
        if continuation != -1 or meta_len <= 0:
            raise ValueError("continuation")
        meta_end = offset + 8 + meta_len
        if meta_end > len(data):
            raise ValueError("truncated")
        meta = data[offset + 8 : meta_end]
        root = _u32(meta, 0)
        header_type = meta[_field_slot(meta, root, 1)]
        body_slot = _field_slot(meta, root, 3)
        body_length = 0 if body_slot is None else _i64(meta, body_slot)
        if body_length < 0:
            raise ValueError("truncated")
        next_offset = _align8(_align8(meta_end) + body_length)
        if next_offset > len(data):
            raise ValueError("truncated")
        if header_type == 1 and schema is None and not saw_batch:
            table = _slot_uoffset(meta, root, 2)
            if table is None:
                raise ValueError("schema")
            endian_slot = _field_slot(meta, table, 0)
            if endian_slot is not None and _i16(meta, endian_slot) != 0:
                raise ValueError("unsupported")
            fields_slot = _field_slot(meta, table, 1)
            if fields_slot is None:
                raise ValueError("schema")
            schema = [_read_field(meta, item) for item in _offset_vector(meta, fields_slot)]
            schema_sha = sha256_hex(meta)
        elif header_type == 3 and schema is not None:
            table = _slot_uoffset(meta, root, 2)
            if table is None:
                raise ValueError("schema")
            length = _i64(meta, _field_slot(meta, table, 0))
            if length < 0:
                raise ValueError("schema")
            rows += length
            saw_batch = True
        else:
            raise ValueError("schema")
        offset = next_offset
    if offset != len(data) or schema is None or not saw_batch or not schema:
        raise ValueError("schema")
    supported = all(field["supported"] for field in schema)
    return {
        "fields": schema,
        "rows": rows,
        "schema_sha256": schema_sha,
        "supported": supported,
    }


def schema_message_sha256(data: bytes) -> str:
    if len(data) < 8 or _i32(data, 0) != -1:
        raise ValueError("arrow framing")
    length = _i32(data, 4)
    return sha256_hex(data[8 : 8 + length])


def load_json(path: Path) -> dict:
    return json.loads(path.read_text())


def shadow_path(table: str, generation: int) -> Path:
    return CAPTURE / "run" / "shadow" / table / f"gen-{generation}.json"


def cell_rows(shadow: dict) -> list[str]:
    columns = shadow["columns"]
    lines = []
    for key in sorted(shadow["cells"], key=lambda item: int(item)):
        cell = shadow["cells"][key]
        parts = []
        for name in columns:
            value = cell[name]
            rendered = r"\N" if value is None else str(value)
            parts.append(f"{name}={rendered}")
        lines.append("\x1f".join(parts))
    return lines


def grants_from_site(site: dict) -> tuple[list[dict], list[dict]]:
    sent: list[dict] = []
    skipped: list[dict] = []
    for name in site["privileges"]["system_privileges"]:
        item = {"name": name, "status": "known", "granted": True}
        if name == "CREATE SESSION":
            sent.append(item)
        else:
            skipped.append({"name": name, "reason": "not in the closed read set"})
    for row in site["privileges"]["object_privileges"]:
        privilege = row["privilege"]
        table = row["table"]
        if privilege in {"SELECT", "FLASHBACK"} and table in {"T_BATCH", "T_TS", "T_WIDE"}:
            sent.append(
                {
                    "name": privilege,
                    "status": "known",
                    "granted": True,
                    "scope": {"database": SCHEMA, "table": table},
                }
            )
        else:
            skipped.append(
                {
                    "name": privilege,
                    "table": table,
                    "reason": "observed on the reader and not sent",
                }
            )
    if site["privileges"]["flashback_execute"]:
        raise ValueError("flashback execute is present")
    return sent, skipped


def _column_preimage(field: dict) -> dict:
    body = {
        "name": field["name"],
        "logical_name": field["name"],
        "arrow_type": field["arrow_type"],
        "nullable": field["nullable"],
    }
    if field["arrow_type"] in {"decimal128", "decimal256"}:
        body["precision"] = field["precision"]
        body["scale"] = field["scale"]
    elif field["arrow_type"] in {
        "int8",
        "int16",
        "int32",
        "int64",
        "uint8",
        "uint16",
        "uint32",
        "uint64",
    }:
        body["precision"] = field["precision"]
    if "timezone" in field:
        body["timezone"] = field["timezone"]
    if "char_length" in field:
        body["char_length"] = field["char_length"]
    return body


def _type_row(table: str, field: dict) -> dict:
    precision = field["precision"] if field["arrow_type"] not in {"utf8", "binary", "bool"} else None
    scale = field["scale"] if field["arrow_type"] in {"decimal128", "decimal256"} else None
    if field["arrow_type"] in {
        "int8",
        "int16",
        "int32",
        "int64",
        "uint8",
        "uint16",
        "uint32",
        "uint64",
    }:
        precision = field["precision"]
        scale = None
    body = {
        "table": table,
        "name": field["name"],
        "arrow_type": field["arrow_type"],
        "precision": precision,
        "scale": scale,
    }
    if "timezone" in field:
        body["timezone"] = field["timezone"]
    return body


def build_inputs(spec: dict, driver: dict, permissions: list[dict]) -> dict:
    shadow = load_json(shadow_path(spec["table"], spec["generation"]))
    if shadow["generation"] != spec["generation"]:
        raise ValueError("generation")
    if shadow["method"] != "C5" or shadow["consistency_grade"] != "S0":
        raise ValueError("grade")
    if "DELETES_LAGGED" not in shadow["quality"] or shadow["delete_detection"] != "reconcile":
        raise ValueError("delete")
    arrow_path = (ARROW_DIR / spec["arrow"]).resolve()
    parsed = parse_arrow(arrow_path.read_bytes())
    if parsed["rows"] != len(shadow["cells"]):
        raise ValueError("row count")
    if not parsed["supported"]:
        unsupported = [field["name"] for field in parsed["fields"] if not field["supported"]]
    else:
        unsupported = []
    rows = cell_rows(shadow)
    if len(rows) != parsed["rows"]:
        raise ValueError("rows")
    snapshot = shadow["position"]
    table_body = {
        "name": spec["table"],
        "arrow": spec["arrow"],
        "route": "C5",
        "consistency": "S0",
        "delete_mode": "reconcile",
        "cursor_safety": "no_cursor",
        "quality": "DELETES_LAGGED",
        "rows": rows,
    }
    tombstones = list(shadow["tombstones"])
    if tombstones:
        table_body["tombstones"] = tombstones
    capturer = {
        "protocol": "oo.oracle.capture-tick/v1",
        "database": SCHEMA,
        "label": snapshot,
        "snapshot": {"kind": "scn", "value": snapshot},
        "source_id": spec["source_id"],
        "source_generation": spec["generation"],
        "product_reader_write_attempts": 0,
        "snapshot_token": False,
        "cross_table_consistency": "B",
        "tables": [table_body],
    }
    fields = []
    types = []
    source_columns = {column["name"]: column for column in load_json(PROFILE_PATH)["columns"][spec["table"]]}
    for field in parsed["fields"]:
        original = source_columns[field["name"]]
        if original["data_type"] in {"CHAR", "NCHAR"}:
            field["char_length"] = original["char_length"]
        if not field["supported"]:
            continue
        fields.append(_column_preimage(field))
        types.append(_type_row(spec["table"], field))
    if unsupported:
        # The sealer rejects the stream before it compares columns. Keep the
        # readable prefix so a later supported field is not invented as utf8.
        schema_preimage = [{"table": spec["table"], "columns": fields}] if fields else [
            {"table": spec["table"], "columns": []}
        ]
    else:
        schema_preimage = [{"table": spec["table"], "columns": fields}]
    report = {
        "protocol": "oo.source-admission.report/v1",
        "target_id": TARGET_ID,
        "source_id": spec["source_id"],
        "secret_ref": SECRET_REF,
        "driver": {
            "kind": driver["kind"],
            "name": driver["name"],
            "version": driver["version"],
            "digest": driver["digest"],
        },
        "compat_mode": "oracle",
        "license_flags": ["UPL-1.0 OR Apache-2.0"],
        "permissions": permissions,
        "capabilities": [
            {
                "name": "BatchFull",
                "cursor": None,
                "feed": None,
                "cdc": None,
                "token_order": None,
                "budget_unit": None,
                "level": "experimental",
                "write_class": "R0",
                "consistency": "S0",
            },
            {
                "name": "DeleteReconcile",
                "cursor": None,
                "feed": None,
                "cdc": None,
                "token_order": None,
                "budget_unit": None,
                "level": "experimental",
                "write_class": "R0",
                "consistency": "S0",
            },
        ],
        "capture_methods": [],
        "cost_model": {
            "unit": "Rows",
            "confidence": "unknown",
            "estimate": "customer hard_limit 8 Rows",
        },
        "r1w": None,
    }
    config = {
        "protocol": "oo.source-admission.config/v1",
        "target_id": TARGET_ID,
        "source_id": spec["source_id"],
        "secret_ref": SECRET_REF,
        "compat_mode": "oracle",
        "permissions": permissions,
        "budget": {"unit": "Rows", "hard_limit": BUDGET_LIMIT, "confidence": "unknown"},
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
    return {
        "spec": spec,
        "shadow": shadow,
        "parsed": parsed,
        "unsupported": unsupported,
        "capturer": capturer,
        "schema_preimage": schema_preimage,
        "type_preimage": types,
        "report": report,
        "config": config,
        "arrow_path": arrow_path,
        "snapshot": snapshot,
    }


def dump_json(path: Path, value) -> None:
    path.write_text(json.dumps(value, indent=2, ensure_ascii=True) + "\n")


def write_table(directory: Path, built: dict, driver_path: Path) -> None:
    directory.mkdir(parents=True)
    dump_json(directory / "tick.json", built["capturer"])
    dump_json(directory / "schema.json", built["schema_preimage"])
    dump_json(directory / "types.json", built["type_preimage"])
    witness = {
        "protocol": "oo.source-admission.capture-witness/v1",
        "source_engine": "oracle",
        "source_id": built["spec"]["source_id"],
        "source_generation": built["spec"]["generation"],
        "snapshot": {"kind": "scn", "value": built["snapshot"]},
        "run_id": f"{built['spec']['source_id']}-{built['snapshot'].rsplit('-', 1)[-1]}",
        "logical_schema": SCHEMA,
        "logical_table": built["spec"]["table"],
        "capturer_path": str((directory / "tick.json").resolve()),
        "arrow_path": str(built["arrow_path"]),
        "driver_path": str(driver_path.resolve()),
        "schema_preimage_path": str((directory / "schema.json").resolve()),
        "type_preimage_path": str((directory / "types.json").resolve()),
    }
    dump_json(directory / "witness.json", witness)
    report = dict(built["report"])
    report["capture_witness"] = str((directory / "witness.json").resolve())
    dump_json(directory / "report.json", report)
    dump_json(directory / "config.json", built["config"])
    dump_json(
        directory / "arrow-parse.json",
        {
            "protocol": "oo.oracle.c5-arrow-parse/v1",
            "supported": built["parsed"]["supported"],
            "unsupported_columns": built["unsupported"],
            "rows": built["parsed"]["rows"],
            "schema_message_sha256": built["parsed"]["schema_sha256"],
            "stream_sha256": sha256_hex(built["arrow_path"].read_bytes()),
            "fields": [
                {
                    "name": field["name"],
                    "nullable": field["nullable"],
                    "arrow_type": field["arrow_type"],
                    "precision": field["precision"],
                    "scale": field["scale"],
                    "type_byte": field["type_byte"],
                    "supported": field["supported"],
                }
                for field in built["parsed"]["fields"]
            ],
            "tombstones": built["shadow"]["tombstones"],
            "cell_keys": sorted(built["shadow"]["cells"], key=lambda item: int(item)),
            "schema_digest": None,
            "admitted": False,
            "trusted": False,
        },
    )


def _yaml_quote(value: str) -> str:
    return json.dumps(value, ensure_ascii=True)


def standard_yaml(profile: dict, shadows: dict[str, dict]) -> str:
    lines = [
        "# Producer YAML for OO04_FIX. This is not a compiler role pack and not a published OOS document.",
        "# source_acceptance: not_claimed",
        "# publication: not_published",
        "# consumable_by_current_pin: false",
        "# sealed_import_claimed: false",
        "locale: zh-CN",
        "datasets:",
    ]
    order = ("T_BATCH", "T_WIDE", "T_TS")
    for table in order:
        shadow = shadows[table]
        columns = profile["columns"][table]
        dataset = f"oo04_fix.{table.lower()}"
        lines.append(f"  - dataset: {dataset}")
        lines.append(f"    source_name: {SCHEMA}.{table}")
        lines.append(f"    label: {SCHEMA}.{table}")
        lines.append("    tier: raw")
        lines.append("    grain: one source row")
        lines.append("    primary_key: [ID]")
        lines.append(f"    generation: {shadow['generation']}")
        lines.append(f"    scn: {_yaml_quote(shadow['position'])}")
        lines.append("    columns:")
        for column in columns:
            flags = column["flags"]
            lines.append(f"      - name: {column['name']}")
            lines.append(f"        data_type: {_yaml_quote(column['data_type'])}")
            lines.append(f"        nullable: {'true' if column['nullable'] == 'Y' else 'false'}")
            if column["precision"] is not None:
                lines.append(f"        precision: {column['precision']}")
            if column["scale"] is not None:
                lines.append(f"        scale: {column['scale']}")
            if column["char_length"]:
                lines.append(f"        char_length: {column['char_length']}")
            if flags:
                rendered = ", ".join(flags)
                lines.append(f"        description: {rendered}")
            else:
                lines.append(f"        description: {column['data_type']}")
        caveats = sorted({flag for column in columns for flag in column["flags"]})
        caveats.append("DELETES_LAGGED")
        lines.append("    source_semantics:")
        lines.append("      family: oracle")
        lines.append(f"      product: {_yaml_quote(PRODUCT)}")
        lines.append(f"      version: {_yaml_quote(PRODUCT_VERSION)}")
        lines.append("      capture: C5")
        lines.append("      grade: S0")
        lines.append("      caveats: [" + ", ".join(caveats) + "]")
        lines.append(f"    delete_detection: {shadow['delete_detection']}")
        lines.append(f"    tombstones: [{', '.join(_yaml_quote(item) for item in shadow['tombstones'])}]")
        lines.append("    cell_keys: [" + ", ".join(sorted(shadow["cells"], key=lambda item: int(item))) + "]")
    batch = shadows["T_BATCH"]
    wide = shadows["T_WIDE"]
    ts = shadows["T_TS"]
    lines.append("samples:")
    def _cell(value) -> str:
        return "null" if value is None else str(value)

    lines.append(
        "  t_wide_n_free: ["
        + ", ".join(_yaml_quote(_cell(wide["cells"][key]["N_FREE"])) for key in ("71", "72", "73", "74"))
        + "]"
    )
    lines.append(f"  t_ts_ts: [{_yaml_quote(ts['cells']['81']['TS'])}, {_yaml_quote(ts['cells']['82']['TS'])}]")
    lines.append(f"  t_ts_tstz: [{_yaml_quote(ts['cells']['81']['TSTZ'])}, {_yaml_quote(ts['cells']['82']['TSTZ'])}]")
    lines.append(f"  t_batch_generation_8_has_91: {'true' if '91' in batch['cells'] else 'false'}")
    lines.append(f"  t_batch_tombstone: {_yaml_quote(batch['tombstones'][0] if batch['tombstones'] else None)}")
    lines.append(f"  t_batch_c5_note_key_41: {_yaml_quote(_cell(batch['cells']['41'].get('C5_NOTE')))}")
    lines.append(f"  t_batch_c6_key_41: {_yaml_quote(batch['cells']['41']['C6'])}")
    return "\n".join(lines) + "\n"


def freshness_yaml(profile: dict, shadows: dict[str, dict]) -> str:
    lines = [
        "# Producer freshness and role view. Gateway get_freshness was not called.",
        "# The model token directory is empty, so there is no Agent multi-turn session.",
        "# This file is not a compiler role pack.",
        "protocol: oo.oracle.c5-producer-freshness/v1",
        "gateway_get_freshness: not_called",
        "role_pack: not_compiled",
        "source_acceptance: not_claimed",
        "publication: not_published",
        "consumable_by_current_pin: false",
        "delete_does_not_resurrect:",
        "  table: OO04_FIX.T_BATCH",
        "  generation: 8",
        "  tombstone: \"91:5\"",
        "  key_91_in_cells: false",
        "  source_recovery_exit: historical_unchanged",
        "loss_flags:",
    ]
    for name, columns in profile["loss_flags"].items():
        lines.append(f"  {name}: [{', '.join(_yaml_quote(item) for item in columns)}]")
    wide = shadows["T_WIDE"]
    ts = shadows["T_TS"]

    def _cell(value) -> str:
        return "null" if value is None else str(value)

    lines.append("samples:")
    lines.append(
        "  n_free: ["
        + ", ".join(_yaml_quote(_cell(wide["cells"][key]["N_FREE"])) for key in ("71", "72", "73", "74"))
        + "]"
    )
    lines.append(
        "  ts: ["
        + ", ".join(_yaml_quote(ts["cells"][key]["TS"]) for key in ("81", "82"))
        + "]"
    )
    lines.append(
        "  tstz: ["
        + ", ".join(_yaml_quote(ts["cells"][key]["TSTZ"]) for key in ("81", "82"))
        + "]"
    )
    lines.append("tables:")
    for table, shadow in shadows.items():
        lines.append(f"  - dataset: oo04_fix.{table.lower()}")
        lines.append(f"    source_name: {SCHEMA}.{table}")
        lines.append(f"    consistency_grade: {shadow['consistency_grade']}")
        lines.append(f"    delete_detection: {shadow['delete_detection']}")
        lines.append("    capture_method: C5")
        lines.append("    loader_capture_method: c5_diff")
        lines.append("    quality: DELETES_LAGGED")
        lines.append(f"    scn: {_yaml_quote(shadow['position'])}")
        lines.append(f"    generation: {shadow['generation']}")
        lines.append("    columns:")
        for column in profile["columns"][table]:
            flags = column["flags"] or ["none"]
            lines.append(f"      - name: {column['name']}")
            lines.append(f"        oracle_type: {_yaml_quote(column['data_type'])}")
            lines.append("        caveats: [" + ", ".join(flags) + "]")
    return "\n".join(lines) + "\n"


def write_tree(out: Path, driver_path: Path, driver: dict) -> None:
    if out.exists():
        raise SystemExit(f"output exists: {out}")
    site = load_json(SITE_PATH)
    permissions, skipped = grants_from_site(site)
    out.mkdir(parents=True)
    profile = load_json(PROFILE_PATH)
    shadows = {}
    for spec in TABLES:
        built = build_inputs(spec, driver, permissions)
        if built["unsupported"] and spec["table"] != "T_BATCH":
            raise SystemExit(f"unexpected unsupported column in {spec['table']}")
        write_table(out / spec["source_id"], built, driver_path)
        shadows[spec["table"]] = built["shadow"]
    dump_json(out / "driver.json", driver)
    dump_json(out / "permissions-sent.json", permissions)
    dump_json(out / "permissions-not-sent.json", skipped)
    (out / "standard.yaml").write_text(standard_yaml(profile, shadows))
    (out / "freshness.yaml").write_text(freshness_yaml(profile, shadows))


def main() -> None:
    import argparse

    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--driver", type=Path, required=True)
    parser.add_argument("--driver-kind", default="native-rust")
    parser.add_argument("--driver-name", default="oracledb")
    parser.add_argument("--driver-version", default="26.0.0-beta.4")
    args = parser.parse_args()
    blob = args.driver.read_bytes()
    driver = {
        "kind": args.driver_kind,
        "name": args.driver_name,
        "version": args.driver_version,
        "digest": sha256_hex(blob),
        "bytes": len(blob),
        "path": str(args.driver.resolve()),
        "digest_is_file": True,
    }
    if args.driver_name == "oracledb":
        driver["crate_checksum"] = "d729181747bd69ba53cde2956ea9ddc06cf09d0badaf4a9e89944b7c0b7b4397"
    write_tree(args.out, args.driver, driver)


if __name__ == "__main__":
    main()
