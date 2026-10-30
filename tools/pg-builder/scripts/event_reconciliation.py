"""Aggregate-only event reconciliation on an operator-approved local Agent DB."""
import argparse
from datetime import date
import json
import os
from pathlib import Path
import re

import declared_enums
import library_selfbuild as lib
from library_observation import literal, quote


def statement(tables, config):
    inventory = {t["dataset"]: t for t in tables}
    if len(inventory) != len(tables):
        raise ValueError("event_inventory_duplicate")
    period = config["period"]
    start, end = (date.fromisoformat(period[k]) for k in ("start", "end"))
    if start >= end or start.isoformat() != period["start"] or end.isoformat() != period["end"]:
        raise ValueError("event_period_invalid")
    left, right = config["left"], config["right"]
    scope = left.get("scope")
    if (not isinstance(scope, dict) or not scope or scope != right.get("scope")
            or any(not isinstance(k, str) or not k or not isinstance(v, str) or not v
                   for k, v in scope.items())):
        raise ValueError("event_equal_explicit_scope_required")
    size = len(left["keys"])
    if not 1 <= size <= 8 or len(right["keys"]) != size:
        raise ValueError("event_key_arity")
    ctes = []
    for side, spec in (("l", left), ("r", right)):
        if len(set(spec["keys"])) != size or not set(scope) <= set(spec["keys"]):
            raise ValueError("event_keys_must_include_scope")
        table = inventory[spec["dataset"]]

        def column(name):
            return declared_enums.resolve_column(table, name)

        def ref(name):
            return quote(column(name)["physical_name"])

        if not re.fullmatch(r"(?:numeric|decimal)(?:\(\d+(?:,\s*\d+)?\))?|smallint|integer|bigint",
                            column(spec["amount"])["data_type"]):
            raise ValueError("event_exact_amount_type_required")
        clock_type = column(spec["time"])["data_type"]
        if clock_type not in {"date", "timestamp", "timestamp without time zone"}:
            raise ValueError("event_business_time_type_unconfirmed")
        time_kind = "DATE" if clock_type == "date" else "TIMESTAMP"
        where = [ref(k) + "=" + literal(v) for k, v in sorted(scope.items())]
        where.extend([
            ref(spec["time"]) + ">=" + time_kind + " " + literal(start.isoformat()),
            ref(spec["time"]) + "<" + time_kind + " " + literal(end.isoformat()),
        ])
        keys = [ref(k) for k in spec["keys"]]
        fields = [k + " as k" + str(i) for i, k in enumerate(keys)]
        fields.extend(["count(*) as n", "count(" + ref(spec["amount"]) + ") as measured",
                       "sum(" + ref(spec["amount"]) + "::numeric) as amount"])
        ctes.append(side + " as (select " + ",".join(fields) + " from " + table["physical"]
                    + " where " + " and ".join(where) + " group by " + ",".join(keys) + ")")
    fields = [
        "pg_current_snapshot()::text as snapshot",
        "statement_timestamp()::text as observed_at",
        "count(*) filter(where l.n is not null and r.n is not null) as matched_events",
        "count(*) filter(where l.n is not null and r.n is null) as left_only_events",
        "count(*) filter(where r.n is not null and l.n is null) as right_only_events",
        "count(*) filter(where l.n is not null and r.n is not null "
        "and l.amount is distinct from r.amount) as unequal_amount_events",
    ]
    for side in ("l", "r"):
        fields.extend([
            f"coalesce(sum({side}.n),0) as {side}_rows",
            f"count({side}.n) as {side}_events",
            f"count(*) filter(where {side}.n>1) as {side}_multirow_events",
            f"coalesce(sum({side}.n-{side}.measured),0) as {side}_null_amount_rows",
            f"coalesce(sum({side}.n) filter(where "
            + " or ".join(f"{side}.k{i} is null" for i in range(size))
            + f"),0) as {side}_null_key_rows",
            f"sum({side}.amount)::text as {side}_amount",
        ])
    # Pre-aggregate both sides so many-to-many joins cannot multiply amounts.
    joined = " and ".join(f"l.k{i}=r.k{i}" for i in range(size))
    return ("select row_to_json(q) from (with " + ",".join(ctes) + " select "
            + ",".join(fields) + " from l full join r on " + joined + ") q")


def reconcile(db, tables, config):
    sql = statement(tables, config)
    result = db.query(sql)
    if not isinstance(result, dict) or not result.get("snapshot") or not result.get("observed_at"):
        raise ValueError("event_observation_incomplete")
    for side in ("l", "r"):
        for key in ("rows", "events", "multirow_events", "null_amount_rows", "null_key_rows"):
            count = result.get(side + "_" + key)
            if type(count) is not int or count < 0:
                raise ValueError("event_observation_incomplete")
        amount = result.get(side + "_amount")
        if amount is not None and (not isinstance(amount, str)
                                   or not re.fullmatch(r"-?\d+(?:\.\d+)?", amount)):
            raise ValueError("event_exact_amount_required")
    for key in ("matched_events", "left_only_events", "right_only_events", "unequal_amount_events"):
        if type(result.get(key)) is not int or result[key] < 0:
            raise ValueError("event_observation_incomplete")
    if result["unequal_amount_events"] > result["matched_events"]:
        raise ValueError("event_observation_counts")
    for side, only in (("l", "left_only_events"), ("r", "right_only_events")):
        rows, count = result[side + "_rows"], result[side + "_events"]
        if (count != result["matched_events"] + result[only] or count > rows
                or result[side + "_multirow_events"] > min(count, rows - count)
                or result[side + "_null_amount_rows"] > rows
                or result[side + "_null_key_rows"] > rows):
            raise ValueError("event_observation_counts")
    evidence = {
        "protocol": "oo.builder.event-reconciliation/v1",
        "coverage": "local_imported_snapshot_only",
        "config": config, "config_sha256": lib.digest(config),
        "inventory_sha256": declared_enums.inventory_digest(tables),
        "query_sha256": lib.digest(sql), "observation": result,
        "review": "pending", "business_accepted": False, "source_completeness_claimed": False,
        "caveats": [
            "Both sides use their declared time column and the same half-open period.",
            "Missing-time rows are outside this interval; boundary differences need separate review.",
            "Matching amounts do not prove successful payment, customer attribution or channel completeness.",
        ],
    }
    evidence["evidence_sha256"] = lib.digest(evidence)
    return evidence


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[4]
    output = args.output.resolve()
    if output.is_relative_to(root) or output.exists():
        parser.error("output must be a new private directory outside the repository")
    config = json.loads(args.config.read_text())
    connection = config["psql_command"]
    if (not isinstance(connection, list) or not connection
            or any(not isinstance(arg, str) or not arg for arg in connection)):
        parser.error("psql_command must be a nonempty argv array")
    os.umask(0o077)
    output.mkdir(parents=True, mode=0o700)
    db = lib.ReadOnlyPostgres(connection, timeout_ms=8000)
    spec = config["reconciliation"]
    tables = lib.collect_tables(db, Path(config["ddl_directory"]),
                                expected=sorted({spec["left"]["dataset"], spec["right"]["dataset"]}),
                                report_path=output / "inventory.json")
    evidence = reconcile(db, tables, spec)
    lib.validate_retained_tables(db, tables, Path(config["ddl_directory"]))
    lib.write(output / "reconciliation.json", evidence)
    print(json.dumps({"protocol": evidence["protocol"], "business_accepted": False,
                      "evidence_sha256": evidence["evidence_sha256"]}))


if __name__ == "__main__":
    main()
