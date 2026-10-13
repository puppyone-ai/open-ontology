"""Oracle sealed-import catalog for the five-row typed batch.

The cell values live here as the independent expected result. They are not
copied into the builder prompt. Schema text matches
``oo_core::sealed_import`` preimage lines. Physical names match
``_oo.pm_payload`` / ``_oo.pm_physical``.
"""

from __future__ import annotations

import hashlib
import json
from decimal import Decimal

SOURCE_ID = "oraclebatch"
SCHEMA = "batch"
TABLE = "t_batch"
DATASET = "batch.t_batch"
RUN_ID = "a04a11e0-2026-4010-8004-0000000005a1"
SOURCE_GENERATION = 1
TARGET_GENERATION = 1
SCN = 2175138
NUMERIC_TYPMOD = ((16 << 16) | 6) + 4  # PostgreSQL numeric typmod, VARHDRSZ is 4

COLUMNS = (
    ("id", 21, -1, "int2", True),
    ("n16", 1700, NUMERIC_TYPMOD, "numeric", False),
    ("n1", 21, -1, "int2", False),
    # Operator preview admits bool, int2, int4, int8, numeric, and text.
    # Oracle DATE and RAW stay exact text here. The native Arrow batch keeps
    # Timestamp(us) and Binary. This is not a whole-row JSON value.
    ("d", 25, -1, "text", False),
    ("c6", 25, -1, "text", False),
    ("v2", 25, -1, "text", False),
    ("r", 25, -1, "text", False),
)

# AS OF SCN 2175138 gold. id 41's later current row is not this snapshot.
GOLD = (
    {"id": 41, "n16": "3141592653.589793", "n1": 0, "d": "2026-10-10 07:11:42", "c6": "ZQ    ", "v2": "alpha", "r": "0001ff00"},
    {"id": 42, "n16": "-0.000001", "n1": 1, "d": "2026-10-10 00:00:00", "c6": "      ", "v2": None, "r": None},
    {"id": 43, "n16": None, "n1": None, "d": "2026-10-10 23:59:59", "c6": "Z     ", "v2": None, "r": "00"},
    {"id": 44, "n16": "9999999999.999999", "n1": 0, "d": "2026-10-10 12:00:01", "c6": "AB C  ", "v2": " ", "r": "dead00be"},
    {"id": 45, "n16": "0.000000", "n1": 1, "d": "2028-02-29 18:45:07", "c6": "n     ", "v2": "x", "r": "00000000"},
)

FORBIDDEN_PROMPT = (
    "3141592653",
    "9999999999",
    "07:11:42",
    "dead00be",
    "18:45:07",
    "23:59:59",
)


def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def schema_preimage(columns=COLUMNS) -> str:
    lines = [f"{SCHEMA}.{TABLE}\n"]
    for name, oid, typmod, _source, _key in columns:
        lines.append(f"{name}\t{oid}\t{typmod}\t\n")
    return "".join(lines)


def pm_payload(config_digest: str, level: str, namespace: str, table: str, column: str) -> bytes:
    return (
        b"physical-mapping/v1\x00"
        + SOURCE_ID.encode()
        + b"\x00"
        + config_digest.encode()
        + b"\x00"
        + str(SOURCE_GENERATION).encode()
        + b"\x00"
        + level.encode()
        + namespace.encode()
        + b"\x00"
        + table.encode()
        + b"\x00"
        + column.encode()
        + b"\x00"
    )


def physical_name(config_digest: str, level: str, namespace: str, table: str, column: str) -> str:
    digest = hashlib.sha256(pm_payload(config_digest, level, namespace, table, column)).digest()
    prefix = {"n": "pn_", "t": "pt_", "c": "pc_"}[level]
    return prefix + digest.hex()[:60]


def identity_digest(config_digest: str, level: str, namespace: str, table: str, column: str) -> bytes:
    return hashlib.sha256(pm_payload(config_digest, level, namespace, table, column)).digest()


def type_text() -> str:
    rows = [f"{name}:{source}:{oid}:{typmod}" for name, oid, typmod, source, _key in COLUMNS]
    return (
        "operator-preview-types=bool,int2,int4,int8,numeric,text\n"
        "oracle-date=exact-text\n"
        "oracle-raw=exact-hex-text\n"
        + "\n".join(rows)
        + f"\nscn={SCN}\nrows=5\n"
    )


def canonical_text() -> str:
    lines = []
    for row in GOLD:
        lines.append(
            "|".join(
                [
                    str(row["id"]),
                    "" if row["n16"] is None else row["n16"],
                    "" if row["n1"] is None else str(row["n1"]),
                    row["d"],
                    row["c6"],
                    "" if row["v2"] is None else row["v2"],
                    "" if row["r"] is None else row["r"],
                ]
            )
        )
    return "\n".join(lines) + "\n"


def catalog_document(config_digest: str, *, lossy_key: bool = False) -> dict:
    columns = []
    for name, oid, typmod, source, key in COLUMNS:
        if lossy_key and name == "id":
            oid, typmod, source, key = 700, -1, "float4", True
        columns.append((name, oid, typmod, source, key))
    preimage = schema_preimage(columns) if not lossy_key else _lossy_preimage()
    namespace = physical_name(config_digest, "n", SCHEMA, "", "")
    table = physical_name(config_digest, "t", SCHEMA, TABLE, "")
    stored = []
    for name, oid, typmod, source, key in columns:
        stored.append(
            {
                "caveat": None,
                "key": key,
                "logical_name": name,
                "oid": oid,
                "source_type": source,
                "stored_physical": physical_name(config_digest, "c", SCHEMA, TABLE, name),
                "typmod": typmod,
            }
        )
    digest = sha256_hex(preimage.encode())
    blank = sha256_hex(b"oracle-preview-hook-not-used")
    return {
        "capture": "sealed_import",
        "config_digest": config_digest,
        "engine": "oracle",
        "hook_checked": False,
        "input_digest": sha256_hex(f"oracle-asof-scn-{SCN}-rows-5".encode()),
        "protocol": "oo.pipeline.sealed-import-catalog/v1",
        "quality": {
            "loss_concealed": False,
            "manifest_reject_count": 0,
            "payload_includes_reject_lines": False,
            "stored_omits_manifest_rejects": False,
            "stored_rejected_rows": 0,
            "stored_rows": [
                {
                    "accepted_rows": 5,
                    "attnum": None,
                    "batch_id": "t0.b0",
                    "key_digest": None,
                    "kind": "batch",
                    "physical_name": table,
                    "reason": None,
                    "rejected_rows": 0,
                    "row_index": None,
                    "value_len": None,
                }
            ],
        },
        "run_id": RUN_ID,
        "schema_digest": sha256_hex(preimage.encode()) if not lossy_key else digest,
        "shadows_queried": False,
        "source_generation": SOURCE_GENERATION,
        "source_id": SOURCE_ID,
        "source_introspection": "not_used",
        "state": "awaiting_publish",
        "tables": [
            {
                "accepted_rows": 5,
                "canonical_digest": sha256_hex(canonical_text().encode()),
                "columns": stored,
                "logical_schema": SCHEMA,
                "logical_table": TABLE,
                "rejects": [],
                "stored_namespace": namespace,
                "stored_physical": table,
            }
        ],
        "target_generation": TARGET_GENERATION,
        "tool_hook_digest": blank,
        "type_digest": sha256_hex(type_text().encode()),
    }


def _lossy_preimage() -> str:
    lines = [f"{SCHEMA}.{TABLE}\n"]
    for name, oid, typmod, _source, _key in COLUMNS:
        if name == "id":
            oid, typmod = 700, -1
        lines.append(f"{name}\t{oid}\t{typmod}\t\n")
    return "".join(lines)


def dumps(document: dict) -> str:
    return json.dumps(document, separators=(",", ":"), sort_keys=True)


def settings_document() -> dict:
    return {
        "catalog_version": "pending",
        "knowledge": {"id": "know-oraclebatch", "version": "1"},
        "ontology": {
            "classification": {"auto_accept": "rule_high_confidence"},
            "compile": {
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 4000},
                "per_role": True,
                "sample_values": {"default": "masked", "max_per_column": 0},
            },
            "default_tier_preference": ["raw"],
            "include_schemas": [SCHEMA],
            "lifecycle": {"deprecation_min_releases": 2},
            "locale": "en-US",
            "name": "batch",
            "publish": {"include_unreviewed": []},
            "spec_version": "oos/v1",
        },
        "ontology_version": "ont-1",
        "policy": {"id": "pol-oraclebatch", "version": "1"},
        "project": SOURCE_ID,
    }


def visibility_document() -> dict:
    return {
        "catalog_version": "pending",
        "datasets": [
            {
                "columns": [name for name, *_rest in COLUMNS],
                "dataset": DATASET,
                "global_stats": "denied",
                "row_security": "none",
            }
        ],
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "knowledge": {"id": "know-oraclebatch", "version": "1"},
        "limits": {"max_entries": 128, "max_file_bytes": 262144, "max_total_bytes": 1048576},
        "ontology_version": "ont-1",
        "policy": {"id": "pol-oraclebatch", "version": "1"},
        "principal": "operator",
        "project": SOURCE_ID,
        "protocol": "oo.builder.role-visibility/v1",
        "role": "oo_r_reader",
    }


def config_document(bearer_file: str, *, price: str, deadline_ms: int) -> dict:
    if price == "unknown":
        declared = {"status": "unknown"}
    else:
        declared = {"currency": "USD", "per_total_token": "0.000001", "status": "declared_non_billing"}
    return {
        "adapter": "loopback_http_local_dev",
        "budget": {
            "deadline_ms": deadline_ms,
            "max_completion_tokens": 16000,
            "max_in_flight": 1,
            "max_prompt_tokens": 100000,
            "max_request_bytes": 262144,
            "max_response_bytes": 262144,
            "max_tables": 16,
            "max_total_tokens": 200000,
        },
        "declared_price": declared,
        "endpoint": {
            "model_id": "grok-4.7",
            "origin": "http://127.0.0.1:8317",
            "path": "/v1/chat/completions",
        },
        "namespace": "pg-builder",
        "protocol": "oo.pg-builder.config/v1",
        "purpose": "generate",
        "sample_policy": "none",
        "secret": {"bearer_file": bearer_file},
        "temperature": 0,
    }


def decimal_of(cell: dict) -> Decimal:
    coefficient = int(cell["coefficient"])
    scale = int(cell["scale"])
    sign = -1 if cell["negative"] else 1
    return sign * Decimal(coefficient).scaleb(-scale)


def cells_match(rows: list) -> tuple[bool, str]:
    if len(rows) != len(GOLD):
        return False, f"row_count={len(rows)}"
    names = [name for name, *_rest in COLUMNS]
    for index, (got, expected) in enumerate(zip(rows, GOLD)):
        if len(got) != len(names):
            return False, f"row {index} width"
        for name, cell in zip(names, got):
            want = expected[name]
            kind = cell.get("kind")
            if want is None:
                if kind != "null":
                    return False, f"row {index} {name} not null"
                continue
            if name in {"id", "n1"}:
                if kind != "int" or int(cell["value"]) != int(want):
                    return False, f"row {index} {name}"
            elif name == "n16":
                if kind != "decimal" or decimal_of(cell) != Decimal(want):
                    return False, f"row {index} {name}"
            elif name == "d":
                if kind != "text" or cell.get("value") != want:
                    return False, f"row {index} {name} {cell.get('value')!r}"
            elif name == "r":
                text = str(cell.get("value", "")).lower().removeprefix("\\x")
                if kind != "text" or text != want:
                    return False, f"row {index} {name} {text!r}"
            else:
                if kind != "text" or cell.get("value") != want:
                    return False, f"row {index} {name}"
    return True, "match"


def self_check() -> None:
    preimage = schema_preimage()
    assert preimage.startswith("batch.t_batch\n")
    assert "n16\t1700\t" in preimage
    assert NUMERIC_TYPMOD == 1048586
    assert COLUMNS[3] == ("d", 25, -1, "text", False)
    assert COLUMNS[6] == ("r", 25, -1, "text", False)
    for row in GOLD:
        assert len(row["c6"]) == 6
    sample = sha256_hex(b"config")
    name = physical_name(sample, "t", SCHEMA, TABLE, "")
    assert name.startswith("pt_") and len(name) == 63
    document = catalog_document(sample)
    assert document["schema_digest"] == sha256_hex(preimage.encode())
    lossy = catalog_document(sample, lossy_key=True)
    assert lossy["schema_digest"] != document["schema_digest"]
    assert lossy["tables"][0]["columns"][0]["oid"] == 700
    encoded = dumps(document)
    assert "3141592653" not in encoded
    assert "password" not in encoded
    assert "://" not in encoded


if __name__ == "__main__":
    self_check()
    print(f"schema_preimage_sha256={sha256_hex(schema_preimage().encode())}")
    print(f"numeric_typmod={NUMERIC_TYPMOD}")
    print("catalog_self_check=ok")
