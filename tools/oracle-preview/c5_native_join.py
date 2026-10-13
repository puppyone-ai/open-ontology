"""Bind the committed C5 capture to the fixed native-update loader.

The receipt, sealed catalog, and load document are built from the capture
files. schema_digest is the sealer preimage (push_schema_table / push_schema_column,
empty caveat, SHA-256). It is not the Arrow schema-message hash. The fixed
ELF is the check: a mismatched preimage comes back as digest_mismatch.

This module does not edit the loader, does not start Oracle or PostgreSQL,
and does not claim publication or source acceptance.
"""

from __future__ import annotations

import hashlib
import json
import uuid
from pathlib import Path

import c5_capture as c5

ROOT = Path("/Users/redux/puppy-one/open-ontology")
EVIDENCE = ROOT / "evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/c5-capture"
RUN = EVIDENCE / "run"
JOIN = EVIDENCE / "native-join"

CAPTURE_RECEIPT_PROTOCOL = "oo.source-admission.capture-receipt/v1"
SEALED_CATALOG_PROTOCOL = "oo.pipeline.sealed-import-catalog/v1"
SOURCE_BOUND_PROTOCOL = "oo.source-semantics.source-bound/v1"
TYPE_DICTIONARY_PROTOCOL = "oo.source-semantics.type-dictionary/v1"
LOAD_PROTOCOL = "oo.pipeline.native-update/v1"
INSTALL_PROTOCOL = "oo.pipeline.install-native-update/v1"
HOOK_PREIMAGE = b"full-import/v1 hook none"
PHYSICAL_PREFIX = b"physical-mapping/v1\x00"
FIXED_PIPELINE_ELF = "4673c8b6e58526c46cac9697b13fd015601992179ac6c38f0e21cf3566acabfe"
FIXED_PG_BUILDER_ELF = "af5e79560d50a1c29cee465a95361b7e3a1632e5a51156c46e154e2363172c54"
FIXED_ARCHIVE = "2e0bd098363ace48bda9b9127f5fabe6d3a1647d"
ENCODER_ELF = "9b06acb3d873057efd0d2f2b431f129ab53061e7b19978f336930d1aad935d75"
BANNER_EXPECTED_RELEASE = "23.26.3.0.0"
SHARD = "oo04_c5_native"
OWNER = str(uuid.uuid5(uuid.NAMESPACE_URL, "oo-dl01w1-04-c5-native-owner"))
LOGICAL_SCHEMA = "oo04_fix"
SOURCE = {
    "T_BATCH": ("oo04_t_batch", "t_batch"),
    "T_WIDE": ("oo04_t_wide_text", "t_wide"),
    "T_TS": ("oo04_t_ts_text", "t_ts"),
}

# Arrow files already committed by fc70. Later generations are encoded from
# the committed shadow and are not a new Oracle capture.
COMMITTED_ARROW = {
    ("T_BATCH", 1): "t_batch-gen-1.arrow",
    ("T_BATCH", 3): "t_batch-gen-3.arrow",
    ("T_BATCH", 8): "t_batch-gen-8.arrow",
    ("T_WIDE", 1): "t_wide-gen-1.arrow",
    ("T_TS", 1): "t_ts-gen-1.arrow",
}
COMMITTED_SCHEMA_MESSAGE = {
    "t_batch-gen-1.arrow": "c8976240f10e8de32d9afa1f577e1c14881f16996ffe3eeead6fa909b47a0a09",
    "t_batch-gen-3.arrow": "c8976240f10e8de32d9afa1f577e1c14881f16996ffe3eeead6fa909b47a0a09",
    "t_batch-gen-8.arrow": "61fc7a7afa8825d3f66f039a0e8659c2a4cdc0213d1fd25aab79f6948010392e",
    "t_ts-gen-1.arrow": "ddb113133d899319262fc924d1845c222dfdb6cff2c436ee41e3ec8a8571ece6",
    "t_wide-gen-1.arrow": "a5f91543c47e3f31fdb3fe0707c9fb9629410d36d1c8697d2bf3cf08dd3ccea4",
}


def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def stable_ident(original: str) -> str:
    """pgident/v1 for one Oracle name. Uppercase names become q_ plus hex."""
    if not original or "\0" in original:
        raise ValueError("empty_name")
    raw = original.encode()
    if _safe_ident(original):
        return original
    return "q_" + raw.hex()


def _safe_ident(name: str) -> bool:
    if not name or len(name) > 63 or name.startswith("q_"):
        return False
    first, rest = name[0], name[1:]
    if first != "_" and not ("a" <= first <= "z"):
        return False
    return all(ch == "_" or ("a" <= ch <= "z") or ch.isdigit() for ch in rest)


def schema_preimage(schema: str, table: str, columns: list[tuple[str, int, int]]) -> str:
    """Sealer preimage. Caveat is empty. Not the Arrow schema message."""
    lines = [f"{schema}.{table}\n"]
    for name, oid, typmod in columns:
        lines.append(f"{name}\t{oid}\t{typmod}\t\n")
    return "".join(lines)


def numeric_typmod(precision: int, scale: int) -> int:
    return ((precision << 16) | (scale & 0x7FF)) + 4


def physical_name(level: str, source: str, config_digest: str, generation: int, schema: str, table: str, column: str) -> str:
    prefix = {"namespace": "pn_", "table": "pt_", "column": "pc_"}[level]
    level_byte = {"namespace": b"n", "table": b"t", "column": b"c"}[level]
    blob = bytearray(PHYSICAL_PREFIX)
    for part in (source, config_digest, str(generation)):
        blob.extend(part.encode())
        blob.append(0)
    blob.extend(level_byte)
    for part in (schema, table, column):
        blob.extend(part.encode())
        blob.append(0)
    return prefix + sha256_hex(bytes(blob))[:60]


def payload_digest(
    source: str,
    schema: str,
    table: str,
    generation: int,
    parent: int | None,
    batch: str,
    arrow_sha: str,
    tombstones: list[str],
    columns: list[str],
    config_digest: str,
    schema_digest: str,
    type_digest: str,
    input_digest: str,
) -> str:
    """Same byte string as native_update::payload_digest. Tombstones stay empty."""
    parts = bytearray(b"native-update/v1\n")
    parts.extend(source.encode())
    parts.extend(schema.encode())
    parts.extend(table.encode())
    parts.extend(str(generation).encode())
    parts.extend(str(0 if parent is None else parent).encode())
    parts.extend(batch.encode())
    parts.extend(arrow_sha.encode())
    for key in tombstones:
        parts.extend(key.encode())
        parts.append(0)
    for column in columns:
        parts.extend(column.encode())
        parts.append(0)
    parts.extend(config_digest.encode())
    parts.extend(schema_digest.encode())
    parts.extend(type_digest.encode())
    parts.extend(input_digest.encode())
    return sha256_hex(bytes(parts))


def canonical_digest(payload: str, rows: int) -> str:
    raw = b"native-update/v1 canonical\n" + payload.encode() + str(rows).encode()
    return sha256_hex(raw)


def arrow_schema_message_sha256(blob: bytes) -> str:
    """SHA-256 of the IPC schema-message metadata slice. Not the stream hash."""
    if len(blob) < 8 or blob[:4] != b"\xff\xff\xff\xff":
        raise ValueError("arrow framing")
    length = int.from_bytes(blob[4:8], "little")
    end = 8 + length
    if length <= 0 or end > len(blob):
        raise ValueError("arrow metadata length")
    return sha256_hex(blob[8:end])


def dictionary_line(column: dict) -> str:
    native = column["data_type"]
    precision = column["precision"]
    scale = column["scale"]
    char_length = column["char_length"]
    if native == "NUMBER" and precision is None:
        left, right, chars = "-", "-", 0
    elif native == "NUMBER":
        left, right, chars = str(precision), str(scale), 0
    elif native in {"DATE", "RAW"}:
        left, right, chars = "-", "-", 0
    elif native in {"CHAR", "VARCHAR2"}:
        left, right, chars = "-", "-", char_length
    elif native in {"TIMESTAMP(9)", "TIMESTAMP(9) WITH TIME ZONE"}:
        left, right, chars = "-", "9", 0
    else:
        raise ValueError(f"native type {native}")
    return f"{column['name']}|{native}|{left}|{right}|{chars}"


def sealed_type(column: dict) -> tuple[int, int, str, int | None]:
    """OID, typmod, source_type, arrow char_length for one dictionary column.

    source_type is what derive_source_bound requires. int8 is not used as a
    source_type token because the sealer maps that word to PostgreSQL oid 20.
    NUMBER(p<=4, scale 0) is sealed as int2. The Arrow field stays int16 or int8.
    """
    native = column["data_type"]
    if native == "CHAR":
        length = int(column["char_length"])
        return 1042, length + 4, "utf8", length
    if native == "VARCHAR2":
        return 1043, -1, "utf8", None
    if native == "NUMBER" and column["precision"] is None:
        return 1043, -1, "utf8", None
    if native == "NUMBER":
        precision = int(column["precision"])
        scale = int(column["scale"])
        if scale == 0 and precision <= 4:
            return 21, -1, "int2", None
        return 1700, numeric_typmod(precision, scale), "numeric", None
    if native == "DATE":
        return 1114, -1, "timestamp_micros", None
    if native == "RAW":
        return 17, -1, "binary", None
    if native in {"TIMESTAMP(9)", "TIMESTAMP(9) WITH TIME ZONE"}:
        return 1043, -1, "utf8", None
    raise ValueError(f"native type {native}")


def arrow_nullable(name: str) -> bool:
    """Nullable bit stored by the C5 encoder. Only ID is not null.

    Oracle NOT NULL on TS and TSTZ is not the Arrow bit. The declaration
    follows the committed file so check_arrow_declaration can match it.
    """
    return name != "ID"


def arrow_declaration(column: dict) -> dict:
    native = column["data_type"]
    nullable = arrow_nullable(column["name"])
    name = column["name"]
    if name == "ID":
        body = {"name": name, "logical_name": name, "arrow_type": "int16", "nullable": False}
    elif name == "N1":
        body = {"name": name, "logical_name": name, "arrow_type": "int8", "nullable": nullable}
    elif native == "NUMBER" and column["precision"] is not None:
        body = {
            "name": name,
            "logical_name": name,
            "arrow_type": "decimal128",
            "nullable": nullable,
            "precision": int(column["precision"]),
            "scale": int(column["scale"]),
        }
    elif native == "DATE":
        body = {"name": name, "logical_name": name, "arrow_type": "timestamp_micros", "nullable": nullable}
    elif native == "RAW":
        body = {"name": name, "logical_name": name, "arrow_type": "binary", "nullable": nullable}
    else:
        body = {"name": name, "logical_name": name, "arrow_type": "utf8", "nullable": nullable}
        char_length = sealed_type(column)[3]
        if char_length is not None:
            body["char_length"] = char_length
    return body


def load_shadow(table: str, generation: int) -> dict:
    path = RUN / "shadow" / table / f"gen-{generation}.json"
    return json.loads(path.read_text())


def dictionary_for(table: str, generation: int) -> list[dict]:
    """Columns present in that generation's shadow, in Arrow order."""
    shadow = load_shadow(table, generation)
    names = list(shadow["columns"])
    catalog = json.loads((RUN / "producer-catalog.json").read_text())
    by_name = {}
    for item in catalog["tables"]:
        if item["table"] != table:
            continue
        for column in item["columns"]:
            by_name[column["name"]] = column
    before = _dictionary_before()
    for column in before.get(table, []):
        by_name.setdefault(column["name"], column)
    missing = [name for name in names if name not in by_name]
    if missing:
        raise ValueError(f"{table} gen {generation} missing dictionary {missing}")
    return [by_name[name] for name in names]


def _dictionary_before() -> dict[str, list[dict]]:
    text = (RUN / "sql" / "dictionary-before.stdout").read_text()
    grouped: dict[str, list[dict]] = {}
    for line in text.splitlines():
        if not line.startswith("COL|"):
            continue
        _tag, table, position, name, native, precision, scale, char_length, nullable = line.split("|")
        grouped.setdefault(table, []).append({
            "name": name,
            "data_type": native,
            "precision": None if precision == "-" else int(precision),
            "scale": None if scale == "-" else int(scale),
            "char_length": int(char_length),
            "nullable": nullable,
            "position": int(position),
            "table": table,
            "flags": [],
        })
    return grouped


def json_bytes(body: dict) -> bytes:
    return json.dumps(body, indent=2, sort_keys=True).encode() + b"\n"


def write_json(path: Path, body: dict) -> str:
    path.parent.mkdir(parents=True, exist_ok=True)
    raw = json_bytes(body)
    path.write_bytes(raw)
    return sha256_hex(raw)


def store_json(path: Path, body: dict, persist: bool) -> str:
    raw = json_bytes(body)
    if persist:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(raw)
    return sha256_hex(raw)


def build_load(
    table: str,
    generation: int,
    arrow_path: Path,
    target_generation: int,
    config_digest: str,
    audit_digest: str,
    driver_digest: str,
    banner: str,
    password_file: str,
    persist: bool = True,
) -> dict:
    shadow = load_shadow(table, generation)
    columns = dictionary_for(table, generation)
    source_id, logical_table = SOURCE[table]
    arrow = arrow_path.read_bytes()
    arrow_sha = sha256_hex(arrow)
    schema_message = arrow_schema_message_sha256(arrow)
    if arrow_path.name in COMMITTED_SCHEMA_MESSAGE and schema_message != COMMITTED_SCHEMA_MESSAGE[arrow_path.name]:
        raise ValueError(f"schema message drift {arrow_path.name}")
    position = shadow["position"]
    if not position.startswith("oo-dl01w1-04-scn-"):
        raise ValueError("snapshot token")
    scn = position.removeprefix("oo-dl01w1-04-scn-")
    if not scn.isdigit():
        raise ValueError("scn")
    mapped = [stable_ident(column["name"]) for column in columns]
    sealed_columns = []
    preimage_columns = []
    declarations = []
    for column, mapped_name in zip(columns, mapped):
        oid, typmod, source_type, _char_length = sealed_type(column)
        stored = physical_name("column", source_id, config_digest, generation, LOGICAL_SCHEMA, logical_table, mapped_name)
        sealed_columns.append({
            "caveat": None,
            "key": column["name"] == "ID",
            "logical_name": mapped_name,
            "oid": oid,
            "source_type": source_type,
            "stored_physical": stored,
            "typmod": typmod,
        })
        preimage_columns.append((mapped_name, oid, typmod))
        declarations.append(arrow_declaration(column))
    schema_digest = sha256_hex(schema_preimage(LOGICAL_SCHEMA, logical_table, preimage_columns).encode())
    if schema_digest == schema_message:
        raise ValueError("schema digest collapsed onto the arrow schema message")
    dictionary = [dictionary_line(column) for column in columns]
    type_digest = sha256_hex(("\n".join(dictionary) + "\n").encode())
    input_digest = arrow_sha
    rows = len(shadow["cells"])
    batch_id = f"oo04-{logical_table}-gen-{generation}"
    parent = None if generation == 1 else generation - 1
    payload = payload_digest(
        source_id, LOGICAL_SCHEMA, logical_table, generation, parent, batch_id, arrow_sha,
        [], mapped, config_digest, schema_digest, type_digest, input_digest,
    )
    canonical = canonical_digest(payload, rows)
    run_id = str(uuid.uuid5(uuid.NAMESPACE_URL, f"oo04-c5:{source_id}:{generation}:{arrow_sha}"))
    table_physical = physical_name("table", source_id, config_digest, generation, LOGICAL_SCHEMA, logical_table, "")
    namespace_physical = physical_name("namespace", source_id, config_digest, generation, LOGICAL_SCHEMA, "", "")
    catalog = {
        "protocol": SEALED_CATALOG_PROTOCOL,
        "capture": "sealed_import",
        "config_digest": config_digest,
        "engine": "oracle",
        "hook_checked": False,
        "input_digest": input_digest,
        "protocol_note_absent": None,
        "quality": {
            "loss_concealed": False,
            "manifest_reject_count": 0,
            "payload_includes_reject_lines": False,
            "stored_omits_manifest_rejects": False,
            "stored_rejected_rows": 0,
            "stored_rows": [{
                "accepted_rows": rows,
                "attnum": None,
                "batch_id": f"t{generation}.b0",
                "key_digest": None,
                "kind": "batch",
                "physical_name": table_physical,
                "reason": None,
                "rejected_rows": 0,
                "row_index": None,
                "value_len": None,
            }],
        },
        "run_id": run_id,
        "schema_digest": schema_digest,
        "shadows_queried": False,
        "source_generation": generation,
        "source_id": source_id,
        "source_introspection": "not_used",
        "state": "awaiting_publish",
        "tables": [{
            "accepted_rows": rows,
            "canonical_digest": canonical,
            "columns": sealed_columns,
            "logical_schema": LOGICAL_SCHEMA,
            "logical_table": logical_table,
            "rejects": [],
            "stored_namespace": namespace_physical,
            "stored_physical": table_physical,
        }],
        "target_generation": target_generation,
        "tool_hook_digest": sha256_hex(HOOK_PREIMAGE),
        "type_digest": type_digest,
    }
    # The placeholder key is not part of the sealed protocol. Drop it.
    catalog.pop("protocol_note_absent")
    stem = f"{logical_table}-gen-{generation}"
    catalog_path = JOIN / "catalog" / f"{stem}.json"
    catalog_sha = store_json(catalog_path, catalog, persist)
    registry = {
        "protocol": "oo.oracle.c5-registry-snapshot/v1",
        "source_engine": "oracle",
        "source_id": source_id,
        "source_generation": generation,
        "owner": "OO04_FIX",
        "table": table,
        "snapshot": {"kind": "token", "value": position},
        "scn": scn,
        "arrow_sha256": arrow_sha,
        "arrow_schema_sha256": schema_message,
        "schema_digest": schema_digest,
        "publication": "not_published",
        "source_acceptance": "not_claimed",
    }
    registry_path = JOIN / "registry" / f"{stem}.json"
    registry_sha = store_json(registry_path, registry, persist)
    receipt = {
        "schema": CAPTURE_RECEIPT_PROTOCOL,
        "source_engine": "oracle",
        "source_id": source_id,
        "source_generation": generation,
        "snapshot": {"kind": "token", "value": position},
        "run_id": run_id,
        "schema_digest": schema_digest,
        "type_digest": type_digest,
        "config_digest": config_digest,
        "input_digest": input_digest,
        "audit_digest": audit_digest,
        "catalog_sha256": catalog_sha,
        "registry_sha256": registry_sha,
        "driver_digest": driver_digest,
        "arrow_sha256": [arrow_sha],
        "arrow_schema_sha256": schema_message,
    }
    receipt_path = JOIN / "receipt" / f"{stem}.json"
    receipt_sha = store_json(receipt_path, receipt, persist)
    bound = {
        "protocol": SOURCE_BOUND_PROTOCOL,
        "engine": "oracle",
        "source_id": source_id,
        "source_generation": generation,
        "target_generation": target_generation,
        "run_id": run_id,
        "schema_digest": schema_digest,
        "type_digest": type_digest,
        "config_digest": config_digest,
        "input_digest": input_digest,
        "banner": banner,
        "scn": scn,
        "registry_snapshot": position,
        "published_scn": scn,
        "consistency_grade": shadow["consistency_grade"],
        "delete_detection": shadow["delete_detection"],
        "dictionary": {"protocol": TYPE_DICTIONARY_PROTOCOL, "columns": dictionary},
        "bindings": [
            {"sealed_column": mapped_name, "dictionary_column": column["name"]}
            for column, mapped_name in zip(columns, mapped)
        ],
    }
    load = {
        "protocol": LOAD_PROTOCOL,
        "source_engine": "oracle",
        "source_id": source_id,
        "source_generation": generation,
        "logical_schema": LOGICAL_SCHEMA,
        "logical_table": logical_table,
        "batch_id": batch_id,
        "capture_method": "c5_diff",
        "delete_detection": "reconcile",
        "consistency_grade": shadow["consistency_grade"],
        "cross_table_consistent": False,
        "position_codec": "snapshot-zero/v1",
        "position_lineage": position,
        "applied_pos": "0",
        "config_digest": config_digest,
        "schema_digest": schema_digest,
        "type_digest": type_digest,
        "input_digest": input_digest,
        "audit_digest": audit_digest,
        "snapshot": {"kind": "token", "value": position},
        "columns": declarations,
        "arrow_path": str(arrow_path),
        "tombstones": [],
        "row_identity": "none",
        "full_snapshot": True,
        "mirror": False,
        "target": _target(password_file),
        "admission_path": str(receipt_path),
        "admission_expect": {"declared_receipt_sha256": receipt_sha},
        "sealed_catalog": catalog,
        "source_bound": bound,
    }
    if parent is not None:
        load["parent_source_generation"] = parent
    probe = json.loads(json.dumps(load))
    probe["row_identity"] = "unique"
    probe["key_column"] = "ID"
    probe["full_snapshot"] = False
    return {
        "stem": stem,
        "table": table,
        "generation": generation,
        "source_id": source_id,
        "rows": rows,
        "schema_digest": schema_digest,
        "type_digest": type_digest,
        "arrow_schema_sha256": schema_message,
        "arrow_sha256": arrow_sha,
        "receipt_sha256": receipt_sha,
        "catalog_sha256": catalog_sha,
        "payload_digest": payload,
        "physical_name": table_physical,
        "scn": scn,
        "tombstones_in_capture": shadow.get("tombstones", []),
        "arrow_nullable_vs_oracle": [
            {
                "name": column["name"],
                "oracle_nullable": column["nullable"],
                "arrow_nullable": arrow_nullable(column["name"]),
            }
            for column in columns
        ],
        "load": load,
        "probe": probe,
        "receipt_path": str(receipt_path),
        "catalog_path": str(catalog_path),
        "persisted": persist,
        "catalog": catalog,
        "registry": registry,
        "receipt": receipt,
    }


def _target(password_file: str) -> dict:
    return {
        "host": "127.0.0.1",
        "port": 5432,
        "database": "previewdb",
        "runtime_principal": "postgres",
        "password_file": password_file,
        "ssl": "disable",
        "shard": SHARD,
        "owner": OWNER,
    }


def config_body() -> dict:
    return {
        "protocol": "oo.oracle.c5-load-config/v1",
        "method": "C5",
        "initial_load": "shadow_diff",
        "delete_detection": "reconcile",
        "cdc_default": "off",
        "logminer_used": False,
        "xstream": "not_used",
        "docs_daily_seconds": 86400,
        "docs_daily_waited": False,
        "proof_periods_seconds": {"detect": 2, "pull": 2, "reconcile": 2},
        "changed_periods_seconds": {"detect": 3, "pull": 4, "reconcile": 5},
        "reader": "OO04_READER",
        "service": "FREEPDB1",
        "oracle_container": "oo-dl01w1-04-p2-oracle",
        "oracle_container_id": "ffc81586acf3cbf4f333b86aed9b48bca1fb5e90b4bf26d7294c67b98a23a732",
        "fixed_importer_archive": FIXED_ARCHIVE,
        "fixed_pipeline_elf": FIXED_PIPELINE_ELF,
        "position_codec_closed_set_has_no_oracle_scn": True,
        "position_codec_sent": "snapshot-zero/v1",
        "scn_carried_by": ["snapshot.value", "position_lineage", "source_bound.scn"],
    }


# DL01-W1-71 adapt.rs pins read from the workspace. This producer does not
# relabel Oracle bytes with those PostgreSQL protocol strings.
PIN_71_CATALOG = "oo.postgres-preview.capture-check/v1"
PIN_71_PROFILE = "oo.postgres-preview.capture-compare/v1"
PIN_71_SEMANTICS = "oo.builder.source-semantics-profile/v1"
PIN_71_BASIS = "oo.pipeline.pg-sealed-manifest/v1"
PIN_71_OUTER = "oo.pipeline.catalog-import-pg/v1"
PIN_71_CORE = "983577c9b89453b08ab4e1a02cab88df78c3e005"
PIN_71_SOURCE_COMMIT = "4c8c10cf121eaa2390c98d5ee012b0421aab6e80"
ORACLE_CATALOG_PROTOCOL = "oo.oracle.c5-producer-catalog/v1"
ORACLE_PROFILE_PROTOCOL = "oo.oracle.c5-producer-profile/v1"
ORACLE_SEMANTICS_PROTOCOL = "oo.oracle.c5-source-semantics/v1"
RECORDED_WIDE = {
    "schema_digest": "d09b1025274e8831d7cb6e38af2d379d59c71996f414e93038467060694834f0",
    "receipt_sha256": "638555c12427d25fbcf9948ef66d6b1bd6843d8160e9c4bdfc4d73ec036cda70",
    "type_digest": "f84fba400564f9d04e206db027bb3859fd332f0a0ce4414815a28eca49d654ea",
    "arrow_schema_sha256": "a5f91543c47e3f31fdb3fe0707c9fb9629410d36d1c8697d2bf3cf08dd3ccea4",
    "arrow_sha256": "e0ff61f78e8e10bb37e45baf7932e85c26ecfa3aaadbc514879fd34920132f68",
}
OOS_FROM_TARGET = {
    "bool": "boolean",
    "int8": "bigint",
    "int4": "integer",
    "text": "text",
    "date": "date",
    "timestamp": "timestamp",
    "timestamptz": "timestamptz",
    "numeric": "numeric",
    "uuid": "uuid",
    "jsonb": "jsonb",
}
PG_TARGET = {
    16: "bool",
    17: "bytea",
    20: "int8",
    21: "int2",
    23: "int4",
    25: "text",
    114: "json",
    700: "float4",
    701: "float8",
    1042: "bpchar",
    1043: "varchar",
    1082: "date",
    1083: "time",
    1114: "timestamp",
    1184: "timestamptz",
    1700: "numeric",
    2950: "uuid",
    3802: "jsonb",
}


def ident_ok_71(value: str) -> bool:
    """Same rule as tools/selfbuild-control-api/src/ids.rs ident_ok."""
    if not value or len(value) > 63 or not ("a" <= value[0] <= "z"):
        return False
    return all(("a" <= char <= "z") or char.isdigit() or char == "_" for char in value)


def typmod_ok_71(oid: int, typmod: int) -> bool:
    if oid == 1042:
        return typmod >= 5
    if oid == 1043:
        return typmod == -1 or typmod >= 4
    if oid == 1700:
        if typmod < 0:
            return True
        adjusted = typmod - 4
        precision = (adjusted >> 16) & 0xFFFF
        scale = adjusted & 0xFFFF
        return precision > 0 and 0 <= scale <= precision
    if oid in {1083, 1114, 1184}:
        return typmod == -1 or 0 <= typmod <= 6
    return typmod == -1


def pg_target_name_71(oid: int, typmod: int) -> str | None:
    name = PG_TARGET.get(oid)
    if name is None or not typmod_ok_71(oid, typmod):
        return None
    return name


def shadow_generations(table: str) -> list[int]:
    gens = []
    for path in (RUN / "shadow" / table).glob("gen-*.json"):
        gens.append(int(path.name.removeprefix("gen-").removesuffix(".json")))
    return sorted(gens)


def dictionary_rows(table: str, generation: int) -> list[dict]:
    rows = []
    for column in dictionary_for(table, generation):
        oid, typmod, source_type, _char_length = sealed_type(column)
        flags = c5.column_flags(column["data_type"], column["precision"], column["scale"])
        rows.append({
            "char_length": column["char_length"],
            "flags": flags,
            "line": dictionary_line(column),
            "lowercase_alias": False,
            "name": column["name"],
            "nullable": column["nullable"],
            "oid": oid,
            "oracle_type": column["data_type"],
            "precision": column["precision"],
            "scale": column["scale"],
            "sealed_logical_name": stable_ident(column["name"]),
            "source_type": source_type,
            "typmod": typmod,
        })
    return rows


def dictionary_document() -> dict:
    tables = []
    for table in ("T_BATCH", "T_WIDE", "T_TS"):
        generations = []
        for generation in shadow_generations(table):
            shadow = load_shadow(table, generation)
            arrow_name = COMMITTED_ARROW.get((table, generation))
            generations.append({
                "arrow_file": arrow_name,
                "arrow_present": arrow_name is not None,
                "columns": dictionary_rows(table, generation),
                "consistency_grade": shadow["consistency_grade"],
                "delete_detection": shadow["delete_detection"],
                "generation": generation,
                "position": shadow["position"],
                "row_count": len(shadow["cells"]),
            })
        tables.append({"generations": generations, "table": table})
    return {
        "protocol": TYPE_DICTIONARY_PROTOCOL,
        "publication": "not_published",
        "source_acceptance": "not_claimed",
        "tables": tables,
    }


def maintenance_changes() -> list[dict]:
    changes = []
    for table in ("T_BATCH", "T_WIDE", "T_TS"):
        previous = None
        previous_generation = None
        for generation in shadow_generations(table):
            shadow = load_shadow(table, generation)
            columns = list(shadow["columns"])
            if previous is None:
                changes.append({
                    "columns": columns,
                    "consistency_grade": shadow["consistency_grade"],
                    "delete_detection": shadow["delete_detection"],
                    "generation": generation,
                    "kind": "baseline",
                    "position": shadow["position"],
                    "row_count": len(shadow["cells"]),
                    "table": table,
                })
            else:
                added = [name for name in columns if name not in previous["columns"]]
                removed = [name for name in previous["columns"] if name not in columns]
                events = c5.diff_hashes(previous["hashes"], shadow["hashes"])
                if added or removed or events:
                    changes.append({
                        "columns_added": added,
                        "columns_removed": removed,
                        "consistency_grade": shadow["consistency_grade"],
                        "delete_detection": shadow["delete_detection"],
                        "from_generation": previous_generation,
                        "generation": generation,
                        "kind": "change",
                        "position": shadow["position"],
                        "row_events": events,
                        "table": table,
                        "tombstones": shadow.get("tombstones", []),
                    })
            previous = shadow
            previous_generation = generation
    return changes


def maintenance_requests(changes: list[dict]) -> list[dict]:
    """71 POST bodies. Not sent. Uppercase Oracle names fail ident_ok."""
    requests = []
    for trigger, key in (
        ("table_ready", "oo04-table-ready-0001"),
        ("scheduled", "oo04-scheduled-0001"),
        ("manual", "oo04-manual-0001"),
    ):
        requests.append({
            "body": {"identity_revision": 1, "idempotency_key": key, "trigger": trigger},
            "posted": False,
            "reason": "no build; 71 pin refuses the oracle catalog before create",
        })
    for change in changes:
        if change["kind"] != "change" or not change["columns_added"]:
            continue
        for name in change["columns_added"]:
            requests.append({
                "body": {
                    "fact_edit": {
                        "column": name,
                        "data_type": "utf8",
                        "dataset": f"{LOGICAL_SCHEMA}.{SOURCE[change['table']][1]}",
                    },
                    "identity_revision": 1,
                    "idempotency_key": f"oo04-schema-{change['table'].lower()}-g{change['generation']}",
                    "trigger": "schema_changed",
                },
                "column_ident_ok": ident_ok_71(name),
                "posted": False,
                "reason": "schema_changed requires an existing generated column and a lowercase ident; this column was added and keeps its Oracle name",
            })
    return requests


def latest_shadows() -> list[dict]:
    chosen = []
    for table in ("T_BATCH", "T_WIDE", "T_TS"):
        generation = shadow_generations(table)[-1]
        shadow = load_shadow(table, generation)
        source_id, logical = SOURCE[table]
        chosen.append({
            "columns": list(shadow["columns"]),
            "consistency_grade": shadow["consistency_grade"],
            "delete_detection": shadow["delete_detection"],
            "generation": generation,
            "logical_table": logical,
            "position": shadow["position"],
            "row_count": len(shadow["cells"]),
            "source_id": source_id,
            "table": table,
        })
    return chosen


def intake_documents() -> dict:
    chosen = latest_shadows()
    grades = {item["consistency_grade"] for item in chosen}
    deletes = {item["delete_detection"] for item in chosen}
    grade = grades.pop() if len(grades) == 1 else None
    delete = deletes.pop() if len(deletes) == 1 else None
    catalog = {
        "engine": "oracle",
        "protocol": ORACLE_CATALOG_PROTOCOL,
        "tables": [
            {
                "columns": item["columns"],
                "generation": item["generation"],
                "schema": LOGICAL_SCHEMA,
                "source_table": item["table"],
                "table": item["logical_table"],
            }
            for item in chosen
        ],
    }
    profile = {
        "cross_table_consistent": False,
        "full_scan": True,
        "per_table": [
            {"rows_compared": item["row_count"], "table": item["table"], "generation": item["generation"]}
            for item in chosen
        ],
        "protocol": ORACLE_PROFILE_PROTOCOL,
        "rows_compared": sum(item["row_count"] for item in chosen),
        "sampled": False,
    }
    semantics = {
        "consistency_grade": grade,
        "delete_detection": delete,
        "protocol": ORACLE_SEMANTICS_PROTOCOL,
        "requested_sealed_grade": "S1",
        "sealed_import_claimed": False,
    }
    bases = []
    for item in chosen:
        rows = dictionary_rows(item["table"], item["generation"])
        bases.append({
            "outer_protocol": SEALED_CATALOG_PROTOCOL,
            "protocol": SEALED_CATALOG_PROTOCOL,
            "tables": [{
                "columns": [
                    {
                        "logical_name": row["sealed_logical_name"],
                        "oid": row["oid"],
                        "typmod": row["typmod"],
                    }
                    for row in rows
                ],
                "logical_schema": LOGICAL_SCHEMA,
                "logical_table": item["logical_table"],
            }],
        })
    return {"bases": bases, "catalog": catalog, "profile": profile, "semantics": semantics}


def intake_verdict(documents: dict) -> dict:
    catalog = documents["catalog"]
    profile = documents["profile"]
    semantics = documents["semantics"]
    refused = []
    if catalog["protocol"] != PIN_71_CATALOG:
        refused.append("catalog_protocol")
    if profile["protocol"] != PIN_71_PROFILE:
        refused.append("profile_protocol")
    if semantics["protocol"] != PIN_71_SEMANTICS:
        refused.append("semantics_protocol")
    bad_idents = []
    for table in catalog["tables"]:
        for name in (table["schema"], table["table"], *table["columns"]):
            if not ident_ok_71(name):
                bad_idents.append(name)
    if bad_idents:
        refused.append("ident_ok")
    for basis in documents["bases"]:
        if basis["protocol"] != PIN_71_BASIS or basis["outer_protocol"] != PIN_71_OUTER:
            refused.append("basis_protocol")
            break
    counterfactual = []
    for basis in documents["bases"]:
        for table in basis["tables"]:
            for column in table["columns"]:
                target = pg_target_name_71(column["oid"], column["typmod"])
                mapped = None if target is None else OOS_FROM_TARGET.get(target)
                if target is None:
                    support = "unknown_oid"
                elif mapped is None:
                    support = "oos_unsupported" if target == "bytea" else "unknown"
                else:
                    support = "mapped"
                counterfactual.append({
                    "logical_name": column["logical_name"],
                    "oid": column["oid"],
                    "oos_data_type": mapped,
                    "support": support,
                    "target_type": target,
                    "typmod": column["typmod"],
                })
    return {
        "auto_approved": False,
        "consumable_by_current_pin": False,
        "counterfactual_if_pin_extended_without_weakening_oos": counterfactual,
        "derive_source_bound_called": False,
        "derive_skip_recorded_by_71": "engine_not_oracle",
        "ident_rejected": bad_idents,
        "pin": {
            "basis": PIN_71_BASIS,
            "catalog": PIN_71_CATALOG,
            "core": PIN_71_CORE,
            "outer": PIN_71_OUTER,
            "profile": PIN_71_PROFILE,
            "semantics": PIN_71_SEMANTICS,
            "source_commit_on_pin": PIN_71_SOURCE_COMMIT,
        },
        "posted": False,
        "refused": refused,
        "relabelled_as_postgres": False,
        "source_acceptance": "not_claimed",
    }


def load_blockers(generation: int) -> list[str]:
    blockers = [
        "DL01-W1-06 prepare_table maps each logical_name a second time",
        "DL01-W1-77 F1 a self-written receipt file is not trusted",
        "DL01-W1-77 F6 a null jobs lease must not seal",
    ]
    if generation != 1:
        blockers.append("DL01-W1-77 F4 applied_pos 0 on a generation with a parent must be refused")
    return blockers


def capture_context() -> dict:
    preflight = json.loads((JOIN / "phases" / "preflight.json").read_text())
    banner = json.loads((JOIN / "phases" / "banner.json").read_text())
    if banner["user"] != ["OO04_READER"] or banner["con"] != ["FREEPDB1"] or len(banner["banners"]) != 1:
        raise ValueError("recorded banner")
    return {
        "audit_digest": preflight["audit_digest"],
        "banner": banner["banners"][0],
        "config_digest": preflight["config_digest"],
        "driver_digest": preflight["encoder_elf"],
    }


def mapping_pairs(built: dict) -> list[dict]:
    load = built["load"]
    pairs = []
    for column in load["columns"]:
        once = stable_ident(column["name"])
        twice = stable_ident(once)
        pairs.append({
            "once_ident": once,
            "once_physical": physical_name(
                "column", built["source_id"], load["config_digest"], built["generation"],
                LOGICAL_SCHEMA, load["logical_table"], once,
            ),
            "oracle_name": column["name"],
            "twice_ident": twice,
            "twice_physical": physical_name(
                "column", built["source_id"], load["config_digest"], built["generation"],
                LOGICAL_SCHEMA, load["logical_table"], twice,
            ),
        })
    return pairs


def held_entry(built: dict, persisted: bool) -> dict:
    generation = built["generation"]
    load = built["load"]
    return {
        "arrow_schema_sha256": built["arrow_schema_sha256"],
        "arrow_sha256": built["arrow_sha256"],
        "blockers": load_blockers(generation),
        "catalog_sha256": built["catalog_sha256"],
        "generation": generation,
        "mapping": mapping_pairs(built),
        "parent_source_generation": load.get("parent_source_generation"),
        "persisted": persisted,
        "physical_name": built["physical_name"],
        "receipt_sha256": built["receipt_sha256"],
        "rows": built["rows"],
        "schema_digest": built["schema_digest"],
        "scn": built["scn"],
        "sealed": False,
        "source_id": built["source_id"],
        "stem": built["stem"],
        "submitted": False,
        "table": built["table"],
        "trusted": False,
    }


def emit_multi_table() -> dict:
    """Write held multi-table inputs. Does not call the loader."""
    ctx = capture_context()
    password_file = "/run/oo04-trust"
    wide = build_load(
        "T_WIDE", 1, RUN / "arrow" / "t_wide-gen-1.arrow", 1,
        ctx["config_digest"], ctx["audit_digest"], ctx["driver_digest"], ctx["banner"],
        password_file, persist=False,
    )
    for key, expected in RECORDED_WIDE.items():
        if wide[key] != expected:
            raise ValueError(f"wide recompute drifted {key}")
    if wide["load"]["columns"][0]["name"] != "ID":
        raise ValueError("wide logical name")
    wide_entry = held_entry(wide, False)
    wide_entry["historical_submission"] = {
        "commit": "4fa46c07fe59fb1fda05e41130da9b64ee767978",
        "container": "oo-dl01w1-04-native-import-wide-1",
        "exit_code": 1,
        "reason": "copy",
        "resubmitted": False,
        "sealed": False,
    }
    entries = [wide_entry]
    for table, generation in (("T_TS", 1), ("T_BATCH", 1), ("T_BATCH", 3), ("T_BATCH", 8)):
        arrow_name = COMMITTED_ARROW[(table, generation)]
        built = build_load(
            table, generation, RUN / "arrow" / arrow_name, 1,
            ctx["config_digest"], ctx["audit_digest"], ctx["driver_digest"], ctx["banner"],
            password_file, persist=True,
        )
        if built["schema_digest"] == built["arrow_schema_sha256"]:
            raise ValueError("schema digest collapsed")
        names = [column["name"] for column in built["load"]["columns"]]
        if any(name != name.upper() for name in names):
            raise ValueError("lowercase alias")
        envelope = {
            "document": built["load"],
            "protocol": "oo.oracle.c5-held-load/v1",
            "seal": "not_attempted",
            "submitted": False,
            "trusted": False,
        }
        write_json(JOIN / "load" / f"{built['stem']}.json", envelope)
        entries.append(held_entry(built, True))
    dictionary = dictionary_document()
    changes = maintenance_changes()
    requests = maintenance_requests(changes)
    documents = intake_documents()
    verdict = intake_verdict(documents)
    write_json(JOIN / "dictionary" / "tables.json", dictionary)
    write_json(JOIN / "maintenance" / "changes.json", {
        "changes": changes,
        "protocol": "oo.oracle.c5-maintenance/v1",
        "source_acceptance": "not_claimed",
    })
    write_json(JOIN / "maintenance" / "requests.json", {
        "posted": False,
        "protocol": "oo.oracle.c5-maintenance-request/v1",
        "requests": requests,
    })
    write_json(JOIN / "intake-71" / "catalog.json", documents["catalog"])
    write_json(JOIN / "intake-71" / "profile.json", documents["profile"])
    write_json(JOIN / "intake-71" / "source_semantics.json", documents["semantics"])
    for basis, item in zip(documents["bases"], latest_shadows()):
        write_json(JOIN / "intake-71" / "basis" / f"{item['logical_table']}.json", basis)
    write_json(JOIN / "intake-71" / "verdict.json", verdict)
    bindings = []
    for table in dictionary["tables"]:
        latest = table["generations"][-1]
        for column in latest["columns"]:
            bindings.append({
                "oracle_name": column["name"],
                "sealed_logical_name": column["sealed_logical_name"],
                "table": table["table"],
            })
    write_json(JOIN / "intake-71" / "original-names.json", {
        "lowercase_alias": False,
        "protocol": "oo.oracle.c5-original-names/v1",
        "rows": bindings,
    })
    lease = {
        "jobs_lease_receipt": None,
        "protocol": "oo.oracle.c5-lease-hold/v1",
        "rust_claim_api": "not_invoked",
        "seal": "not_attempted",
        "target_fence_certified": False,
    }
    write_json(JOIN / "lease" / "not-sealed.json", lease)
    index = {
        "complete_signoff": "0/10",
        "elf_called": False,
        "entries": entries,
        "fixed_pipeline_elf": FIXED_PIPELINE_ELF,
        "intake_71_consumable": False,
        "model_calls": 0,
        "preview_counter": "6/10",
        "protocol": "oo.oracle.c5-multi-input/v1",
        "publication": "not_published",
        "source_acceptance": "not_claimed",
        "wide_recompute_matches_4fa46": True,
    }
    write_json(JOIN / "multi" / "index.json", index)
    return index
