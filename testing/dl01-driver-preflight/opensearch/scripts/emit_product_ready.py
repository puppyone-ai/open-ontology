#!/usr/bin/env python3
"""Write the measured OOS dataset and the public query cases.

Values come from the fast-8 oracle and the field dictionary. No control POST,
no pack compile, and no gateway call. oos_data_type stays null. c5_extra and
where_point stay out of the dataset columns.
"""

import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
PRE = ROOT / "evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight"
OUT = PRE / "product-ready-20261012"
ORACLE = PRE / "native-20261012/oracle/pg-fast-8.json"
DICTIONARY = PRE / "builder-input-20261012/dictionary.json"

# column_storage in crates/oo-pipeline/src/native_update.rs. list_binary has no arm.
STORAGE = {
    "utf8": ("text", 1043),
    "binary": ("bytea", 17),
    "float64": ("float8", 701),
    "timestamp_micros_utc": ("timestamptz", 1184),
    "bool": ("bool", 16),
    "int64": ("int8", 20),
}
FLOAT8 = {
    "0000000000000000": "0",
    "3ff8000000000000": "1.5",
    "c004000000000000": "-2.5",
    "4008000000000000": "3",
}


def civil_from_days(unix_days: int) -> tuple[int, int, int]:
    z = unix_days + 719_468
    era = z // 146_097 if z >= 0 else (z - 146_096) // 146_097
    doe = z - era * 146_097
    yoe = (doe - doe // 1460 + doe // 36524 - doe // 146096) // 365
    year = yoe + era * 400
    doy = doe - (365 * yoe + yoe // 4 - yoe // 100)
    mp = (5 * doy + 2) // 153
    day = doy - (153 * mp + 2) // 5 + 1
    month = mp + 3 if mp < 10 else mp - 9
    if month <= 2:
        year += 1
    return year, month, day


def timestamptz_text(unix_us: int) -> str:
    secs = unix_us // 1_000_000
    frac = unix_us % 1_000_000
    days = secs // 86_400
    sod = secs % 86_400
    year, month, day = civil_from_days(days)
    hour, minute, second = sod // 3600, (sod % 3600) // 60, sod % 60
    return f"{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{frac:06}Z"


def yaml_quote(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)


def cell(column: str, row: dict, arrow_type: str):
    raw = row[column]
    if raw is None:
        return None
    if arrow_type == "float64":
        return FLOAT8[raw]
    if arrow_type == "timestamp_micros_utc":
        return timestamptz_text(int(raw))
    if arrow_type == "bool":
        if raw == "true":
            return True
        if raw == "false":
            return False
        raise SystemExit(f"bad bool {raw}")
    return raw


def main() -> None:
    dictionary = json.loads(DICTIONARY.read_text())
    rows = json.loads(ORACLE.read_text())
    rows.sort(key=lambda row: row["source_doc_id"])
    ids = [row["source_doc_id"] for row in rows]
    if ids != ["led-01", "led-02", "led-03", "led-04", "led-rm"]:
        raise SystemExit(f"unexpected live ids {ids}")
    fields = [field for field in dictionary["fields"] if field["role"] == "approved_value"]
    names = [field["name"] for field in fields]
    if names != dictionary["approved_fields"]:
        raise SystemExit("approved field order drifted")
    withheld = [field["name"] for field in dictionary["fields"] if field["approval"] == "not_approved"]
    if withheld != ["c5_extra"]:
        raise SystemExit(f"unexpected withheld {withheld}")
    if dictionary["unclassified"][0]["name"] != "where_point":
        raise SystemExit("where_point is not the unclassified field")
    if dictionary["unclassified"][0]["source_type"] != "geo_point":
        raise SystemExit("where_point is not geo_point")
    lines = [
        "dataset: ledger.ledger_rows",
        'source_name: "oo-dl01w1-44-ledger"',
        "label: OpenSearch ledger documents",
        "description: >",
        "  One row is one live document from the fast-8 PIT of index",
        "  oo-dl01w1-44-ledger. Five documents are live. led-c5 is absent.",
        "  The captured count is not a planner estimate.",
        "tier: raw",
        "recommended: false",
        "grain: one OpenSearch document",
        "primary_key: [source_doc_id]",
        "columns:",
        "  - name: source_doc_id",
        "    label: document id",
        "    data_type: text",
        "    semantic_type: identifier",
    ]
    for field in fields:
        arrow = field["loader_arrow_type"]
        lines.append(f"  - name: {field['name']}")
        lines.append(f"    label: {field['name']}")
        storage = STORAGE.get(arrow)
        if storage is None:
            lines.append("    description: >")
            lines.append(f"      Approved source type {field['source_type']}. Loader arrow {arrow}.")
            lines.append("      Current sealer column_storage has no arm for this arrow type,")
            lines.append("      so this dataset assigns no data_type. oos_data_type is null.")
        else:
            public, oid = storage
            lines.append(f"    data_type: {public}")
            lines.append("    description: >")
            lines.append(
                f"      Source type {field['source_type']}. Loader arrow {arrow}, storage oid {oid}."
            )
            lines.append("      Dictionary oos_data_type is null. No classification is confirmed.")
    lines.extend(
        [
            "caveats:",
            "  - c5_extra was captured as keyword stored utf8. Approval is not_approved and oos_data_type is null. It is not a column here.",
            "  - where_point is geo_point on the source document. It is unclassified, absent from the Arrow, and not represented as a map.",
            "  - Delete detection is reconcile. The fast-8 snapshot expresses the led-c5 delete by absence. A snapshot request cannot also list that id as a tombstone.",
            "  - Consistency grade is B. The source has no change stream and no transaction.",
            "  - DELETES_LAGGED applies between reconcile cycles. POSITION_EXPIRED is the state of the generation-4 child whose applied_pos is 0; recovery is a new full snapshot, not a continued position.",
            "source_semantics:",
            "  family: opensearch",
            '  product: OpenSearch',
            '  version: "3.9.0"',
            "  capture: c5_diff",
            "  grade: B",
            "  caveats: [no_native_change_stream, delete_reconcile, consistency_ceiling_b]",
        ]
    )
    dataset_dir = OUT / "ontology/curated/datasets/ledger"
    dataset_dir.mkdir(parents=True, exist_ok=True)
    (dataset_dir / "ledger_rows.yaml").write_text("\n".join(lines) + "\n", encoding="utf-8")
    manifest = "\n".join(
        [
            "spec_version: oos/v1",
            "name: oo-dl01w1-44",
            "locale: zh-CN",
            "include_schemas: [ledger]",
            "default_tier_preference: [raw]",
            "publish:",
            "  include_unreviewed: []",
            "compile:",
            "  per_role: true",
            "  budgets: { readme_tokens: 2000, table_file_tokens: 3000 }",
            "  sample_values: { default: masked, max_per_column: 8 }",
        ]
    )
    (OUT / "ontology/ontology.yaml").write_text(manifest + "\n", encoding="utf-8")
    selected = [
        "source_doc_id",
        "doc_key",
        "double_value",
        "event_ms",
        "flag",
        "long_value",
        "note",
        "payload_json",
        "quantities",
        "scaled_value",
        "seq_key",
        "bin_value",
        "tags",
    ]
    by_name = {field["name"]: field for field in dictionary["fields"]}
    by_name["source_doc_id"] = {"loader_arrow_type": "utf8"}
    columns = []
    for name in selected:
        arrow = by_name[name]["loader_arrow_type"]
        storage = STORAGE.get(arrow)
        columns.append(
            {
                "name": name,
                "loader_arrow_type": arrow,
                "public_type": None if storage is None else storage[0],
                "storage_oid": None if storage is None else storage[1],
                "type_claimed": storage is not None,
            }
        )
    typed_rows = []
    for row in rows:
        values = []
        for name in selected:
            arrow = by_name[name]["loader_arrow_type"]
            values.append(cell(name, row, arrow))
        typed_rows.append(values)
    cases = {
        "protocol": "oo.opensearch.public-query-cases/v1",
        "gateway_delivery": "def424680e2066e997c7496d0bf62d36332fb211",
        "route": "POST /v1/query",
        "request_type": "oo_core::api::QueryRequest",
        "cell_type": "oo_core::api::QueryCell",
        "executed": False,
        "pack_compiled": False,
        "body_sent": False,
        "live_ids_order_by_source_doc_id": ids,
        "absent_ids": ["led-c5"],
        "float8_text": "Rust Display of the IEEE-754 big-endian bits in native-20261012/oracle/pg-fast-8.json. 1.5 is the gateway unit value.",
        "timestamptz_text": "ISO-8601 from the Arrow unix microseconds. The sealer stores timestamptz by subtracting the PostgreSQL epoch and the public decoder adds it back.",
        "tags": "Captured canonical JSON text. storage_oid is null because column_storage has no list_binary arm.",
        "cases": [
            {
                "name": "five-live-rows",
                "request": {
                    "sql": "SELECT "
                    + ", ".join(selected)
                    + " FROM ledger.ledger_rows ORDER BY source_doc_id",
                    "max_rows": 40,
                    "format": "json",
                    "client_context": {"purpose": "answer", "turn": 1},
                },
                "expected_columns": columns,
                "expected_rows": typed_rows,
            },
            {
                "name": "flag-true",
                "request": {
                    "sql": "SELECT source_doc_id FROM ledger.ledger_rows WHERE flag IS TRUE ORDER BY source_doc_id",
                    "max_rows": 40,
                    "format": "json",
                    "client_context": {"purpose": "answer", "turn": 2},
                },
                "expected_columns": [{"name": "source_doc_id", "public_type": "text", "type_claimed": True}],
                "expected_rows": [["led-02"], ["led-04"]],
            },
            {
                "name": "led-c5-absent",
                "request": {
                    "sql": "SELECT source_doc_id FROM ledger.ledger_rows WHERE source_doc_id = 'led-c5'",
                    "max_rows": 40,
                    "format": "json",
                    "client_context": {"purpose": "answer", "turn": 3},
                },
                "expected_columns": [{"name": "source_doc_id", "public_type": "text", "type_claimed": True}],
                "expected_rows": [],
            },
            {
                "name": "event-null",
                "request": {
                    "sql": "SELECT source_doc_id FROM ledger.ledger_rows WHERE event_ms IS NULL ORDER BY source_doc_id",
                    "max_rows": 40,
                    "format": "json",
                    "client_context": {"purpose": "answer", "turn": 4},
                },
                "expected_columns": [{"name": "source_doc_id", "public_type": "text", "type_claimed": True}],
                "expected_rows": [["led-03"]],
            },
        ],
    }
    (OUT / "query-cases.json").write_text(json.dumps(cases, indent=2) + "\n", encoding="utf-8")
    maintenance = {
        "protocol": "oo.opensearch.builder-maintenance/v1",
        "body_sent": False,
        "pack_compiled": False,
        "path": "/control/v1/builds/{build_id}/maintenance",
        "ontology": "product-ready-20261012/ontology/ontology.yaml",
        "dataset": "product-ready-20261012/ontology/curated/datasets/ledger/ledger_rows.yaml",
        "query_cases": "product-ready-20261012/query-cases.json",
        "triggers": json.loads((PRE / "builder-input-20261012/maintenance.json").read_text())["triggers"],
    }
    (OUT / "maintenance.json").write_text(json.dumps(maintenance, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"wrote": str(OUT), "rows": len(typed_rows), "event_led_rm": typed_rows[4][3]}))


if __name__ == "__main__":
    main()
