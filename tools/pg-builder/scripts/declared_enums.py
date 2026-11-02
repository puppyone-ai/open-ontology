"""Read declared dictionary metadata from a configured local Agent DB adapter."""
import hashlib
import json

from library_observation import quote, literal

ROLES = {
    "controls": {"id", "entity_key", "table_schema", "table_name", "table_column"},
    "headers": {"ctl_id", "entity_key", "dictionary_type"},
    "dictionary": {"type_code", "code", "label", "active"},
}


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, ensure_ascii=False,
                                    separators=(",", ":")).encode()).hexdigest()

def inventory_digest(tables):
    # S1 adds classifications and sorts columns; neither changes the observed inventory.
    return digest(sorted([
        {k: v for k, v in table.items() if k != "columns"} |
        {"columns": sorted([
            {k: v for k, v in column.items() if k != "classification_candidate"}
            for column in table["columns"]], key=lambda c: c["name"])}
        for table in tables], key=lambda t: t["dataset"]))


def resolve_column(table, source_name):
    found = [c for c in table["columns"] if c.get("source_name", c["name"]) == source_name]
    if len(found) != 1:
        raise ValueError("declared_enum_column_binding")
    return found[0]


def discover(db, tables, config):
    inventory = {t["dataset"]: t for t in tables}
    if len(inventory) != len(tables):
        raise ValueError("declared_enum_inventory_duplicate")
    limit = config.get("max_rows", 500)
    if type(limit) is not int or not 1 <= limit <= 5000:
        raise ValueError("declared_enum_row_budget")
    aliases = config.get("schema_aliases")
    if not isinstance(aliases, dict) or not aliases or any(
            not isinstance(k, str) or not k or not isinstance(v, str) or not v
            for k, v in aliases.items()):
        raise ValueError("declared_enum_schema_mapping_required")
    bound = {}
    for role, fields in ROLES.items():
        spec = config[role]
        if set(spec["columns"]) != fields or spec["dataset"] not in inventory:
            raise ValueError("declared_enum_adapter_binding")
        table = inventory[spec["dataset"]]
        bound[role] = table
        for name in spec["columns"].values():
            resolve_column(table, name)
    scope = config["dictionary"].get("scope")
    if not isinstance(scope, dict) or not scope or any(
            not isinstance(k, str) or not k or not isinstance(v, str) or not v
            for k, v in scope.items()):
        raise ValueError("declared_enum_scope_required")

    def col(role, name, alias):
        source = config[role]["columns"][name]
        return alias + "." + quote(resolve_column(bound[role], source)["physical_name"])

    c = lambda name: col("controls", name, "c")
    h = lambda name: col("headers", name, "h")
    d = lambda name: col("dictionary", name, "d")
    predicates = [
        "d." + quote(resolve_column(bound["dictionary"], name)["physical_name"]) + "=" + literal(value)
        for name, value in scope.items()
    ]
    sql = (
        "select coalesce(json_agg(t),'[]'::json) from (select distinct "
        + ",".join(c(k) + " as " + quote(k) for k in ["table_schema", "table_name", "table_column"])
        + "," + h("dictionary_type") + " as dictionary_type from "
        + bound["controls"]["physical"] + " c join " + bound["headers"]["physical"] + " h on "
        + h("ctl_id") + "=" + c("id") + " and " + h("entity_key") + "=" + c("entity_key")
        + " where " + c("table_schema") + " in (" + ",".join(map(literal, sorted(aliases))) + ")"
        + " and " + h("dictionary_type") + " is not null and " + h("dictionary_type") + "<>''"
        + f" limit {limit + 1}) t"
    )
    declarations = db.query(sql)
    if not isinstance(declarations, list) or len(declarations) > limit:
        raise ValueError("declared_enum_bindings_truncated")
    types = sorted({r["dictionary_type"] for r in declarations})
    values = []
    if types:
        sql = (
            "select coalesce(json_agg(t),'[]'::json) from (select distinct "
            + ",".join(d(k) + " as " + quote(k) for k in ["type_code", "code", "label", "active"])
            + " from " + bound["dictionary"]["physical"] + " d where " + " and ".join(predicates)
            + " and " + d("type_code") + " in (" + ",".join(map(literal, types)) + ")"
            + f" limit {limit + 1}) t"
        )
        values = db.query(sql)
        if not isinstance(values, list) or len(values) > limit:
            raise ValueError("declared_enum_values_truncated")
        if any(not isinstance(r.get(k), str) or not r[k]
               for r in values for k in ["type_code", "code", "label"]):
            raise ValueError("declared_enum_value_invalid")
    targets, unbound = {}, []
    for row in declarations:
        schema = aliases.get(row["table_schema"])
        dataset = str(schema) + "." + row["table_name"]
        table = inventory.get(dataset)
        if table is None:
            unbound.append(dict(row, reason="target_dataset_missing"))
            continue
        try:
            column = resolve_column(table, row["table_column"])
        except ValueError:
            unbound.append(dict(row, reason="target_column_missing"))
            continue
        key = dataset + "." + column["name"]
        targets.setdefault(key, []).append(row)
    bindings = []
    for target, rows in sorted(targets.items()):
        kinds = sorted({r["dictionary_type"] for r in rows})
        selected = [r for r in values if r["type_code"] in kinds]
        labels = {}
        for row in selected:
            labels.setdefault((row["type_code"], row["code"]), set()).add((row["label"], row["active"]))
        conflict = len(kinds) != 1 or any(len(v) != 1 for v in labels.values())
        binding = {
            "enum": target, "dictionary_types": kinds,
            "state": "conflict" if conflict else "declared_candidate" if selected else "dictionary_missing",
            "schema_alias_review": "pending" if any(aliases[r["table_schema"]] != r["table_schema"]
                                                      for r in rows) else "not_needed",
            "method": "ui_control_id_and_entity_key", "review": "pending",
            "dictionary_dataset": config["dictionary"]["dataset"], "scope": dict(scope),
            "values": sorted(selected, key=lambda r: (r["type_code"], r["code"], str(r["label"]), str(r["active"]))),
            "declarations": sorted(rows, key=lambda r: json.dumps(r, sort_keys=True)),
        }
        binding["binding_sha256"] = digest(binding)
        bindings.append(binding)
    evidence = {
        "protocol": "oo.builder.declared-enums/v1", "config_sha256": digest(config),
        "inventory_sha256": inventory_digest(tables),
        "binding_vector": [{k: t[k] for k in ["dataset", "source_id", "source_generation", "target_generation"]}
                           for t in sorted(tables, key=lambda t: t["dataset"])],
        "source": "local_agent_db", "source_time": "unmeasured",
        "published": False, "business_owner_signed": False,
        "bindings": bindings, "unbound": unbound,
    }
    evidence["evidence_sha256"] = digest(evidence)
    return evidence
