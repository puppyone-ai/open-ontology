//! Fixture catalog sealer. The preimage matches `pg-catalog-hash/v1`.
//! The library does not hash. The builder accepts or rejects the sealed bytes.

use serde_json::{Value, json};

pub const OWNER: &str = "owner_role_do_not_leak";
const CAPTURED_AT: &str = "2026-10-04T12:00:00.000000Z";

pub fn column(attnum: i32, name: &str, type_oid: i64, type_name: &str) -> Value {
    json!({
        "attnum": attnum,
        "name": name,
        "name_ident": name,
        "type_oid": type_oid,
        "type_schema": "pg_catalog",
        "type_name": type_name,
        "typtype": "b",
        "typmod": -1,
        "base_type_oid": Value::Null,
        "numeric_precision": Value::Null,
        "numeric_scale": Value::Null,
        "not_null": true,
        "collation_oid": 0,
        "collation_name": Value::Null,
        "identity": "",
        "generated": "",
        "default_expr": Value::Null,
        "comment": Value::Null,
        "enum_labels": [],
        "domain_checks": [],
        "type_support": {"converted": false, "reason": if type_name == "numeric" { "unbounded_or_wide_numeric" } else { "known_builtin_not_converted" }},
        "current_privileges": {"select": true, "insert": false, "update": false}
    })
}

pub fn table(name: &str, columns: Vec<Value>, primary_key: Vec<Value>, unique_keys: Vec<Value>, foreign_keys: Vec<Value>) -> Value {
    json!({
        "schema": "public",
        "schema_ident": "public",
        "name": name,
        "name_ident": name,
        "relkind": "r",
        "replica_identity": "d",
        "rls_enabled": false,
        "rls_forced": false,
        "owner": OWNER,
        "comment": Value::Null,
        "columns": columns,
        "primary_key": primary_key,
        "primary_key_nulls_not_distinct": false,
        "unique_keys": unique_keys,
        "foreign_keys": foreign_keys,
        "policies": [],
        "privileges": [{"grantee": OWNER, "privilege": "SELECT", "grantable": false}]
    })
}

pub fn key_col(attnum: i32, name: &str) -> Value {
    json!({"attnum": attnum, "name": name})
}

pub fn fk(name: &str, attnum: i32, column: &str, ref_table: &str, ref_attnum: i32, ref_name: &str) -> Value {
    json!({
        "name": name,
        "columns": [{
            "attnum": attnum,
            "name": column,
            "ref_schema": "public",
            "ref_table": ref_table,
            "ref_attnum": ref_attnum,
            "ref_name": ref_name
        }]
    })
}

pub fn catalog(mut tables: Vec<Value>) -> Value {
    tables.sort_by(|left, right| {
        left["schema"].as_str().cmp(&right["schema"].as_str()).then(left["name"].as_str().cmp(&right["name"].as_str()))
    });
    let mut value = json!({
        "protocol": "oo.source.pgsql/v1",
        "core_contract": "oo.config/v1",
        "source_id": "shop_src",
        "database": "shop",
        "captured_at": CAPTURED_AT,
        "schema_hash": "0".repeat(64),
        "hash_algorithm": "pg-catalog-hash/v1",
        "ident_algorithm": "pgident/v1",
        "tables": tables,
        "type_conversion": "unsupported"
    });
    let digest = preimage_hash(&value["tables"]);
    value["schema_hash"] = json!(digest);
    value
}

pub fn settings() -> Value {
    json!({
        "project": "shop",
        "ontology_version": "ont-1",
        "catalog_version": "cat-1",
        "policy": {"id": "pol-shop", "version": "1"},
        "knowledge": {"id": "know-shop", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "shop",
            "locale": "en-US",
            "include_schemas": ["public"],
            "default_tier_preference": ["raw"],
            "publish": {"include_unreviewed": []},
            "classification": {"auto_accept": "rule_high_confidence"},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": true,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                "sample_values": {"default": "masked", "max_per_column": 0}
            }
        }
    })
}

pub fn binding(
    catalog: &Value,
    semantics: &[(&str, &str, &str)],
    units: &[(&str, &str, &str)],
    examples: Value,
) -> Value {
    let mut decisions = Vec::new();
    let mut visible = Vec::new();
    let tables = catalog["tables"].as_array().expect("tables");
    for table in tables {
        let dataset = format!("public.{}", table["name"].as_str().expect("name"));
        let mut columns = Vec::new();
        for column in table["columns"].as_array().expect("columns") {
            let name = column["name"].as_str().expect("column");
            let (pii, semantic) = semantics
                .iter()
                .find(|(table_name, column_name, _)| {
                    *table_name == table["name"].as_str().unwrap_or("") && *column_name == name
                })
                .map(|(_, _, semantic)| ("none", *semantic))
                .unwrap_or(("none", "identifier"));
            let mut decision = json!({
                "dataset": dataset,
                "column": name,
                "pii": pii,
                "semantic_type": semantic,
                "reviewer": {"by": "ada.owner", "at": "2026-10-06"}
            });
            if let Some((_, _, unit)) = units.iter().find(|(table_name, column_name, _)| {
                *table_name == table["name"].as_str().unwrap_or("") && *column_name == name
            }) {
                decision["unit"] = json!(unit);
            }
            decisions.push(decision);
            columns.push(name);
        }
        visible.push(json!({
            "dataset": dataset,
            "row_security": "none",
            "global_stats": "denied",
            "columns": columns
        }));
    }
    json!({
        "settings": settings(),
        "decisions": decisions,
        "visibility": {
            "protocol": "oo.builder.role-visibility/v1",
            "project": "shop",
            "principal": "ada",
            "role": "oo_r_clerk",
            "ontology_version": "ont-1",
            "catalog_version": "cat-1",
            "policy": {"id": "pol-shop", "version": "1"},
            "knowledge": {"id": "know-shop", "version": "1"},
            "issued_for_compiler": "oo-builder/role-pack/v1",
            "datasets": visible,
            "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
        },
        "examples": examples
    })
}

pub fn command(catalog: Value, target: Value, binding: Option<Value>) -> Value {
    let mut value = json!({
        "protocol": "oo.source-semantics.command/v1",
        "catalog": catalog,
        "target": target,
    });
    if let Some(binding) = binding {
        value["binding"] = binding;
    }
    value
}

fn preimage_hash(tables: &Value) -> String {
    let mut ordered = tables.as_array().expect("tables").clone();
    ordered.sort_by(|left, right| {
        left["schema"].as_str().cmp(&right["schema"].as_str()).then(left["name"].as_str().cmp(&right["name"].as_str()))
    });
    let mut buf = String::from("{\"tables\":[");
    for (index, table) in ordered.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        write_table(&mut buf, table);
    }
    buf.push_str("]}");
    oo_core::api::payload_sha256(buf.as_bytes())
}

fn write_table(buf: &mut String, table: &Value) {
    buf.push('{');
    write_str(buf, "schema", table["schema"].as_str().unwrap());
    buf.push(',');
    write_str(buf, "name", table["name"].as_str().unwrap());
    buf.push(',');
    write_str(buf, "relkind", table["relkind"].as_str().unwrap());
    buf.push(',');
    write_str(buf, "replica_identity", table["replica_identity"].as_str().unwrap());
    buf.push(',');
    write_bool(buf, "rls", table["rls_enabled"].as_bool().unwrap());
    buf.push(',');
    write_bool(buf, "force_rls", table["rls_forced"].as_bool().unwrap());
    buf.push_str(",\"columns\":[");
    let mut columns = table["columns"].as_array().unwrap().clone();
    columns.sort_by_key(|column| column["attnum"].as_i64().unwrap());
    for (index, column) in columns.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        write_column(buf, column);
    }
    buf.push_str("],\"primary_key\":[");
    write_key_columns(buf, table["primary_key"].as_array().unwrap());
    buf.push_str("],\"primary_key_nulls_not_distinct\":");
    write_opt_bool(buf, &table["primary_key_nulls_not_distinct"]);
    buf.push_str(",\"unique_keys\":[");
    let mut uniques = table["unique_keys"].as_array().unwrap().clone();
    uniques.sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
    for (index, key) in uniques.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        buf.push('{');
        write_str(buf, "name", key["name"].as_str().unwrap());
        buf.push_str(",\"columns\":[");
        write_key_columns(buf, key["columns"].as_array().unwrap());
        buf.push_str("],");
        write_bool(buf, "nulls_not_distinct", key["nulls_not_distinct"].as_bool().unwrap());
        buf.push('}');
    }
    buf.push_str("],\"foreign_keys\":[");
    let mut foreign = table["foreign_keys"].as_array().unwrap().clone();
    foreign.sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
    for (index, key) in foreign.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        buf.push('{');
        write_str(buf, "name", key["name"].as_str().unwrap());
        buf.push_str(",\"columns\":[");
        for (col_index, column) in key["columns"].as_array().unwrap().iter().enumerate() {
            if col_index > 0 {
                buf.push(',');
            }
            buf.push('{');
            write_i64(buf, "attnum", column["attnum"].as_i64().unwrap());
            buf.push(',');
            write_str(buf, "name", column["name"].as_str().unwrap());
            buf.push(',');
            write_str(buf, "ref_schema", column["ref_schema"].as_str().unwrap());
            buf.push(',');
            write_str(buf, "ref_table", column["ref_table"].as_str().unwrap());
            buf.push(',');
            write_i64(buf, "ref_attnum", column["ref_attnum"].as_i64().unwrap());
            buf.push(',');
            write_str(buf, "ref_name", column["ref_name"].as_str().unwrap());
            buf.push('}');
        }
        buf.push_str("]}");
    }
    buf.push_str("],\"policies\":[");
    buf.push_str("]}");
}

fn write_column(buf: &mut String, column: &Value) {
    buf.push('{');
    write_i64(buf, "attnum", column["attnum"].as_i64().unwrap());
    buf.push(',');
    write_str(buf, "name", column["name"].as_str().unwrap());
    buf.push(',');
    write_i64(buf, "type_oid", column["type_oid"].as_i64().unwrap());
    buf.push(',');
    write_i64(buf, "typmod", column["typmod"].as_i64().unwrap());
    buf.push(',');
    write_bool(buf, "not_null", column["not_null"].as_bool().unwrap());
    buf.push(',');
    write_i64(buf, "collation_oid", column["collation_oid"].as_i64().unwrap());
    buf.push(',');
    write_str(buf, "typtype", column["typtype"].as_str().unwrap());
    buf.push(',');
    write_opt_i64(buf, "base_type_oid", &column["base_type_oid"]);
    buf.push(',');
    write_str(buf, "identity", column["identity"].as_str().unwrap());
    buf.push(',');
    write_str(buf, "generated", column["generated"].as_str().unwrap());
    buf.push(',');
    match column.get("default_expr") {
        Some(Value::Null) | None => {
            push_string(buf, "default_sha256");
            buf.push_str(":null");
        }
        Some(value) => {
            write_str(buf, "default_sha256", value["sha256"].as_str().unwrap());
        }
    }
    buf.push_str(",\"enum_labels\":[");
    for (index, label) in column["enum_labels"].as_array().unwrap().iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        push_string(buf, label.as_str().unwrap());
    }
    buf.push_str("],\"domain_checks\":[");
    for (index, check) in column["domain_checks"].as_array().unwrap().iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        push_string(buf, check["sha256"].as_str().unwrap());
    }
    buf.push_str("]}");
}

fn write_key_columns(buf: &mut String, columns: &[Value]) {
    for (index, column) in columns.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        buf.push('{');
        write_i64(buf, "attnum", column["attnum"].as_i64().unwrap());
        buf.push(',');
        write_str(buf, "name", column["name"].as_str().unwrap());
        buf.push('}');
    }
}

fn write_str(buf: &mut String, name: &str, value: &str) {
    push_string(buf, name);
    buf.push(':');
    push_string(buf, value);
}

fn write_bool(buf: &mut String, name: &str, value: bool) {
    push_string(buf, name);
    buf.push(':');
    buf.push_str(if value { "true" } else { "false" });
}

fn write_i64(buf: &mut String, name: &str, value: i64) {
    push_string(buf, name);
    buf.push(':');
    buf.push_str(&value.to_string());
}

fn write_opt_i64(buf: &mut String, name: &str, value: &Value) {
    push_string(buf, name);
    buf.push(':');
    match value.as_i64() {
        Some(inner) => buf.push_str(&inner.to_string()),
        None => buf.push_str("null"),
    }
}

fn write_opt_bool(buf: &mut String, value: &Value) {
    match value.as_bool() {
        Some(true) => buf.push_str("true"),
        Some(false) => buf.push_str("false"),
        None => buf.push_str("null"),
    }
}

fn push_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", u32::from(ch)));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}
