"""Observe stored report definitions as review evidence; never execute their SQL."""
import json
from declared_enums import digest, inventory_digest, resolve_column
from library_observation import literal, quote


def discover(db, tables, config):
    table = next((t for t in tables if t["dataset"] == config["dataset"]), None)
    if table is None:
        raise ValueError("report_dataset_missing")
    columns = config["columns"]
    search = config["search_columns"]
    terms = config["terms"]
    scope = config["scope"]
    limit = config.get("max_rows", 200)
    byte_limit = config.get("max_bytes", 1_048_576)
    if (type(limit) is not int or not 1 <= limit <= 1000
            or type(byte_limit) is not int or not 1 <= byte_limit <= 4_194_304):
        raise ValueError("report_observation_budget")
    if (not isinstance(columns, list) or not columns or len(columns) > 32
            or any(not isinstance(c, str) or not c for c in columns)
            or len(set(columns)) != len(columns)
            or not isinstance(search, list) or not search or not set(search) <= set(columns)
            or not isinstance(terms, list) or not 1 <= len(terms) <= 32
            or any(not isinstance(t, str) or not 1 <= len(t) <= 128 for t in terms)
            or not isinstance(scope, dict) or not scope
            or any(not isinstance(k, str) or not k or not isinstance(v, str) or not v
                   for k, v in scope.items())):
        raise ValueError("report_adapter_config")

    def ref(name):
        return quote(resolve_column(table, name)["physical_name"])

    selected = [ref(c) + " as " + quote(c) for c in columns]
    where = [ref(k) + "=" + literal(v) for k, v in sorted(scope.items())]
    # Literal substring matching, not LIKE patterns or stored executable predicates.
    where.append("(" + " or ".join(
        "strpos(lower(coalesce(" + ref(c) + "::text,'')),lower(" + literal(term) + "))>0"
        for c in search for term in terms) + ")")
    selection = ("select " + ",".join(selected) + " from " + table["physical"] + " where "
                 + " and ".join(where) + f" limit {limit + 1}")
    # Check bounds in the same statement before constructing a possibly huge JSON response.
    rows = db.query(
        "select payload from (with candidates as materialized (" + selection
        + "), bounds as (select count(*) n,coalesce(sum(octet_length(row_to_json(c)::text)),0) bytes"
        + " from candidates c) select case"
        + f" when n>{limit} then json_build_object('error','report_definitions_truncated')"
        + f" when bytes>{byte_limit} then json_build_object('error','report_definition_bytes')"
        + " else (select coalesce(json_agg(c),'[]'::json) from candidates c) end as payload"
        + " from bounds) q")
    if isinstance(rows, dict) and rows.get("error") in {
            "report_definitions_truncated", "report_definition_bytes"}:
        raise ValueError(rows["error"])
    if not isinstance(rows, list) or len(rows) > limit:
        raise ValueError("report_definitions_truncated")
    if any(not isinstance(row, dict) or set(row) != set(columns) for row in rows):
        raise ValueError("report_definitions_invalid")
    payload = json.dumps(rows, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    if len(payload.encode()) > byte_limit:
        raise ValueError("report_definition_bytes")
    evidence = {
        "protocol": "oo.builder.report-definitions/v1",
        "config_sha256": digest(config), "inventory_sha256": inventory_digest(tables),
        "dataset": table["dataset"], "scope": scope,
        "definitions": sorted(rows, key=digest),
        "review": "pending", "executed": False, "business_accepted": False,
        "coverage": "local_imported_snapshot_only",
    }
    evidence["evidence_sha256"] = digest(evidence)
    return evidence
