#!/usr/bin/env python3
"""Build one ClickHouse sealed case from a customer file.

Object names, approved fields, the role, the source budget, and the model id
come from the customer file. This module does not treat ``readings`` as the
only object. A field that the file does not approve is refused before a source
statement is built. ``fixed_string`` is not a declared ClickHouse mapping.
``datetime64_9_utc`` and the array tokens are not put on a success catalog:
oid 1184 with typmod 9 and oid 2277 have no normalized target. A
``timestamp_nanos_utc`` field is numeric(20,0) with an empty caveat, which is
nanoseconds since the epoch and not timestamptz.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path

import run_product as rp

PROTOCOL = "oo.clickhouse.customer-case/v1"
SOURCE_TOKEN = re.compile(r"^[a-z][a-z0-9_]{0,62}$")
TABLE_TOKEN = re.compile(r"^[A-Za-z_][A-Za-z0-9_]{0,127}$")
ROLE_TOKEN = re.compile(r"^oo_r_[a-z0-9_]{1,57}$")
LOOPBACK = "http://127.0.0.1:8317"

DECLARED = {
    "uint64": (1700, rp.numeric_typmod(20, 0), "bigint_unsigned_numeric", "toString({name})"),
    "decimal76_10": (1700, rp.numeric_typmod(76, 10), None, "toString({name})"),
    "int64": (20, -1, None, "toString({name})"),
    "nullable_string": (17, -1, None, "if(isNull({name}), NULL, hex({name}))"),
    "low_cardinality_string": (17, -1, None, "hex({name})"),
    "low_cardinality_nullable_string": (17, -1, None, "if(isNull({name}), NULL, hex({name}))"),
    "string": (17, -1, None, "hex({name})"),
    "timestamp_nanos_utc": (1700, rp.numeric_typmod(20, 0), None, "toString(toUnixTimestamp64Nano({name}))"),
}
UNDECLARED = {"fixed_string", "fixedstring"}
UNSUPPORTED = {
    "datetime64_9_utc": (1184, 9, None),
    "array_int64": (2277, -1, None),
    "array_nullable_string": (2277, -1, None),
}


def sha256_text(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def require_token(value: object, pattern: re.Pattern[str], label: str) -> str:
    if not isinstance(value, str) or pattern.fullmatch(value) is None:
        raise SystemExit(f"{label} is not a closed token")
    return value


def load_json(path: Path) -> dict:
    body = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(body, dict):
        raise SystemExit("customer input is not an object")
    return body


def approved_index(customer: dict) -> dict[tuple[str, str], dict]:
    objects = customer.get("approved_objects")
    if not isinstance(objects, list) or not objects:
        raise SystemExit("customer file has no approved objects")
    index: dict[tuple[str, str], dict] = {}
    for item in objects:
        if not isinstance(item, dict):
            raise SystemExit("approved object is not an object")
        database = require_token(item.get("database"), SOURCE_TOKEN, "database")
        table = require_token(item.get("table"), TABLE_TOKEN, "table")
        require_token(item.get("logical_schema"), SOURCE_TOKEN, "logical_schema")
        require_token(item.get("logical_table"), TABLE_TOKEN, "logical_table")
        fields = item.get("fields")
        if not isinstance(fields, list) or not fields:
            raise SystemExit("approved object has no fields")
        seen = set()
        for field in fields:
            if not isinstance(field, dict):
                raise SystemExit("approved field is not an object")
            name = require_token(field.get("name"), TABLE_TOKEN, "field")
            source_type = field.get("source_type")
            if not isinstance(source_type, str):
                raise SystemExit("approved field has no source type")
            if name in seen:
                raise SystemExit("approved field is repeated")
            seen.add(name)
        index[(database, table)] = item
    return index


def refuse(reason: str, detail: str) -> dict:
    return {
        "protocol": PROTOCOL,
        "admitted": False,
        "reason": reason,
        "detail": detail,
        "source_read": False,
        "sql": None,
    }


def select_fields(customer: dict, request: dict) -> tuple[dict, list[dict]] | dict:
    source_id = require_token(customer.get("source_id"), SOURCE_TOKEN, "source_id")
    secret = customer.get("secret_ref")
    if not isinstance(secret, str) or not secret.startswith("secret://") or len(secret) > 200:
        raise SystemExit("secret_ref is not a secret name")
    role = require_token(customer.get("role"), ROLE_TOKEN, "role")
    budget = customer.get("budget")
    if not isinstance(budget, dict) or budget.get("unit") != "Rows":
        raise SystemExit("source budget is not a row budget")
    model = customer.get("model")
    if not isinstance(model, dict) or model.get("origin") != LOOPBACK:
        raise SystemExit("model origin is not the loopback builder endpoint")
    if model.get("model_id") != "grok-4.7":
        raise SystemExit("model id is not the approved product model")
    requested = request.get("object")
    if not isinstance(requested, dict):
        raise SystemExit("request has no object")
    database = require_token(requested.get("database"), SOURCE_TOKEN, "database")
    table = require_token(requested.get("table"), TABLE_TOKEN, "table")
    names = request.get("fields")
    if not isinstance(names, list) or not names or len(names) > 64:
        raise SystemExit("request fields are outside 1..=64")
    index = approved_index(customer)
    chosen = index.get((database, table))
    if chosen is None:
        return refuse("unapproved_object", "the requested object is not in the customer file")
    by_name = {field["name"]: field for field in chosen["fields"]}
    selected = []
    for name in names:
        token = require_token(name, TABLE_TOKEN, "field")
        field = by_name.get(token)
        if field is None:
            return refuse("unapproved_field", "the requested field is not approved for this object")
        source_type = field["source_type"]
        if source_type in UNDECLARED:
            return refuse("capability_undeclared", "FixedString is not a declared ClickHouse mapping")
        if source_type in UNSUPPORTED:
            return refuse("type_unsupported", "DateTime64 and arrays have no normalized sealed target")
        if source_type not in DECLARED:
            return refuse("type_unsupported", "the approved source type has no declared mapping")
        selected.append(field)
    if not any(field.get("key") is True for field in selected):
        return refuse("missing_key", "the requested field set has no key")
    _ = (source_id, role, secret)
    return chosen, selected


def schema_preimage(schema: str, table: str, columns: list[dict]) -> str:
    lines = [f"{schema}.{table}\n"]
    for item in columns:
        caveat = item["caveat"] or ""
        lines.append(f"{item['logical_name']}\t{item['oid']}\t{item['typmod']}\t{caveat}\n")
    return "".join(lines)


def catalog(customer: dict, chosen: dict, selected: list[dict], rows: int, run_id: str) -> dict:
    schema = chosen["logical_schema"]
    table = chosen["logical_table"]
    dataset = f"{schema}.{table}"
    columns = []
    for field in selected:
        oid, typmod, caveat, _sql = DECLARED[field["source_type"]]
        columns.append(rp.column(field["name"], oid, typmod, field["source_type"], bool(field.get("key")), caveat))
    preimage = schema_preimage(schema, table, columns)
    field_preimage = "\n".join(
        f"{field['name']}\t{field['source_type']}" for field in selected
    )
    type_preimage = "\n".join(
        f"{field['source_type']}\t{DECLARED[field['source_type']][0]}\t{DECLARED[field['source_type']][1]}"
        for field in selected
    )
    config_preimage = json.dumps(
        {
            "budget": customer["budget"],
            "model_id": customer["model"]["model_id"],
            "role": customer["role"],
            "source_id": customer["source_id"],
        },
        sort_keys=True,
    )
    table_shadow = rp.shadow("pt_", f"table:{dataset}")
    return {
        "capture": "sealed_import",
        "config_digest": sha256_text(config_preimage),
        "engine": "clickhouse",
        "hook_checked": False,
        "input_digest": sha256_text(field_preimage),
        "protocol": rp.PROTOCOL,
        "quality": {
            "loss_concealed": False,
            "manifest_reject_count": 0,
            "payload_includes_reject_lines": False,
            "stored_omits_manifest_rejects": False,
            "stored_rejected_rows": 0,
            "stored_rows": [
                {
                    "accepted_rows": rows,
                    "attnum": None,
                    "batch_id": "t0.b0",
                    "key_digest": None,
                    "kind": "batch",
                    "physical_name": table_shadow,
                    "reason": None,
                    "rejected_rows": 0,
                    "row_index": None,
                    "value_len": None,
                }
            ],
        },
        "run_id": run_id,
        "schema_digest": sha256_text(preimage),
        "shadows_queried": False,
        "source_generation": 1,
        "source_id": customer["source_id"],
        "source_introspection": "not_used",
        "state": "awaiting_publish",
        "tables": [
            {
                "accepted_rows": rows,
                "canonical_digest": sha256_text(f"oo-dl01w1-40:table:{dataset}"),
                "columns": columns,
                "logical_schema": schema,
                "logical_table": table,
                "rejects": [],
                "stored_namespace": rp.shadow("pn_", f"ns:{schema}"),
                "stored_physical": table_shadow,
            }
        ],
        "target_generation": 1,
        "tool_hook_digest": sha256_text("oo-dl01w1-40:hook"),
        "type_digest": sha256_text(type_preimage),
    }


def source_sql(chosen: dict, selected: list[dict]) -> str:
    parts = []
    for field in selected:
        _oid, _typmod, _caveat, template = DECLARED[field["source_type"]]
        expr = template.format(name=field["name"])
        parts.append(f"{expr} AS {field['name']}")
    key = next(field["name"] for field in selected if field.get("key") is True)
    select = ", ".join(parts)
    return (
        f"SELECT {select} FROM {chosen['database']}.{chosen['table']} FINAL "
        f"ORDER BY {key} FORMAT JSONEachRow"
    )


def nanos_checks(chosen: dict, selected: list[dict]) -> list[str]:
    if not any(field["source_type"] == "timestamp_nanos_utc" for field in selected):
        return []
    key = next(field["name"] for field in selected if field.get("key") is True)
    base = f"FROM {chosen['database']}.{chosen['table']} FINAL ORDER BY {key} FORMAT JSONEachRow"
    return [
        "SELECT toString(toUnixTimestamp64Nano(ts)) AS nanos, "
        "toString(toDecimal256(toUnixTimestamp64Nano(ts), 0)) AS nanos_decimal "
        f"{base}",
        "SELECT toString(toUnixTimestamp64Nano(ts)) AS nanos, "
        "toString(toUnixTimestamp64Nano(fromUnixTimestamp64Nano(toUnixTimestamp64Nano(ts)))) AS nanos_roundtrip "
        f"{base}",
    ]


def settings(customer: dict, chosen: dict) -> dict:
    return {
        "catalog_version": "pending",
        "knowledge": {"id": "know-clickhouse", "version": "1"},
        "ontology": {
            "classification": {"auto_accept": "rule_high_confidence"},
            "compile": {
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 4000},
                "per_role": True,
                "sample_values": {"default": "masked", "max_per_column": 0},
            },
            "default_tier_preference": ["raw"],
            "include_schemas": [chosen["logical_schema"]],
            "lifecycle": {"deprecation_min_releases": 2},
            "locale": "en-US",
            "name": "clickhouse",
            "publish": {"include_unreviewed": []},
            "spec_version": "oos/v1",
        },
        "ontology_version": "ont-1",
        "policy": {"id": "pol-clickhouse", "version": "1"},
        "project": customer["source_id"],
    }


def visibility(customer: dict, chosen: dict, selected: list[dict]) -> dict:
    dataset = f"{chosen['logical_schema']}.{chosen['logical_table']}"
    return {
        "catalog_version": "pending",
        "datasets": [
            {
                "columns": [field["name"] for field in selected],
                "dataset": dataset,
                "global_stats": "denied",
                "row_security": "none",
            }
        ],
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "knowledge": {"id": "know-clickhouse", "version": "1"},
        "limits": {"max_entries": 128, "max_file_bytes": 262144, "max_total_bytes": 1048576},
        "ontology_version": "ont-1",
        "policy": {"id": "pol-clickhouse", "version": "1"},
        "principal": "operator",
        "project": customer["source_id"],
        "protocol": "oo.builder.role-visibility/v1",
        "role": customer["role"],
    }


def builder_config(customer: dict, bearer_file: str) -> dict:
    return {
        "protocol": "oo.pg-builder.config/v1",
        "adapter": "loopback_http_local_dev",
        "namespace": "pg-builder",
        "purpose": "generate",
        "sample_policy": "none",
        "temperature": 0,
        "endpoint": {
            "origin": LOOPBACK,
            "path": "/v1/chat/completions",
            "model_id": customer["model"]["model_id"],
        },
        "secret": {"bearer_file": bearer_file},
        "declared_price": {
            "currency": "USD",
            "per_total_token": "0.000001",
            "status": "declared_non_billing",
        },
        "budget": {
            "deadline_ms": 120000,
            "max_request_bytes": 262144,
            "max_response_bytes": 262144,
            "max_prompt_tokens": 100000,
            "max_completion_tokens": 16000,
            "max_total_tokens": 200000,
            "max_tables": 16,
            "max_in_flight": 1,
        },
    }


def write_json(path: Path, body: dict) -> None:
    path.write_text(json.dumps(body, indent=2) + "\n", encoding="utf-8")


def plan(customer: dict, request: dict, rows: int, run_id: str, bearer_file: str) -> dict:
    if "secret://" in json.dumps(request):
        raise SystemExit("request copied a secret reference")
    selected = select_fields(customer, request)
    if isinstance(selected, dict):
        return selected
    chosen, fields = selected
    body = catalog(customer, chosen, fields, rows, run_id)
    rendered = {
        "protocol": PROTOCOL,
        "admitted": True,
        "reason": "approved_fields",
        "detail": "the requested fields are approved declared mappings",
        "source_read": True,
        "sql": source_sql(chosen, fields),
        "nanos_sql": nanos_checks(chosen, fields),
        "dataset": f"{chosen['logical_schema']}.{chosen['logical_table']}",
        "role": customer["role"],
        "project": customer["source_id"],
        "model_id": customer["model"]["model_id"],
        "source_budget": customer["budget"],
        "catalog": body,
        "settings": settings(customer, chosen),
        "visibility": visibility(customer, chosen, fields),
        "builder_config": builder_config(customer, bearer_file),
    }
    packed = json.dumps(rendered)
    if "secret://" in packed or customer["secret_ref"] in packed:
        raise SystemExit("plan copied a secret reference")
    return rendered


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--customer", required=True, type=Path)
    parser.add_argument("--request", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--rows", type=int, default=3)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--bearer-file", default="/run/invalid-bearer")
    args = parser.parse_args()
    if not re.fullmatch(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}", args.run_id):
        raise SystemExit("run id is not a lowercase uuid")
    if args.rows < 0 or args.rows > 100000:
        raise SystemExit("row count is outside the source budget")
    customer = load_json(args.customer)
    request = load_json(args.request)
    body = plan(customer, request, args.rows, args.run_id, args.bearer_file)
    args.out.mkdir(parents=True, exist_ok=True)
    decision = {key: body[key] for key in ("protocol", "admitted", "reason", "detail", "source_read", "sql")}
    if body["admitted"]:
        decision["dataset"] = body["dataset"]
        decision["role"] = body["role"]
        decision["project"] = body["project"]
        decision["model_id"] = body["model_id"]
        decision["nanos_sql"] = body["nanos_sql"]
        write_json(args.out / "catalog.json", body["catalog"])
        write_json(args.out / "settings.json", body["settings"])
        write_json(args.out / "visibility.json", body["visibility"])
        write_json(args.out / "builder-config.json", body["builder_config"])
    write_json(args.out / "decision.json", decision)
    print(decision["reason"])
    return 0 if body["admitted"] else 2


if __name__ == "__main__":
    raise SystemExit(main())
