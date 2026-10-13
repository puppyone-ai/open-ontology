use serde_json::{json, Value};

use crate::audit::parse_json_each_row;
use crate::fail::Gate;

pub fn judge(columns_body: &str, table_body: &str, parts_body: &str, nofinal_gold: &Value) -> Gate<Value> {
    let columns = parse_json_each_row(columns_body)?;
    let tables = parse_json_each_row(table_body)?;
    let parts = parse_json_each_row(parts_body)?;
    let mut failures = Vec::new();
    let table = match tables.first() {
        Some(row) => row,
        None => {
            failures.push("system.tables returned no readings row".into());
            return Ok(failed(failures));
        }
    };
    expect_text(table, "database", "oo_dl01w40", &mut failures);
    expect_text(table, "name", "readings", &mut failures);
    expect_text(table, "engine", "ReplacingMergeTree", &mut failures);
    expect_text(table, "partition_key", "toYYYYMM(ts)", &mut failures);
    expect_text(table, "sorting_key", "reading_id", &mut failures);
    expect_text(table, "primary_key", "reading_id", &mut failures);
    let engine_full = text(table, "engine_full");
    if !engine_full.contains("ReplacingMergeTree") || !engine_full.contains("version") {
        failures.push(format!("engine_full does not name ReplacingMergeTree(version): {engine_full}"));
    }
    let expected = [
        ("reading_id", "UInt64", "", ""),
        ("version", "UInt64", "", ""),
        ("amount", "Decimal(76, 10)", "", ""),
        ("note", "Nullable(String)", "", ""),
        ("code", "LowCardinality(String)", "", ""),
        ("code_or_null", "LowCardinality(Nullable(String))", "", ""),
        ("ts", "DateTime64(9, 'UTC')", "", ""),
        ("tags", "Array(Int64)", "", ""),
        ("labels", "Array(Nullable(String))", "", ""),
        ("qty", "Int64", "DEFAULT", "0"),
        ("label", "String", "DEFAULT", "''"),
        ("wide_u64", "UInt64", "", ""),
    ];
    if columns.len() != expected.len() {
        let got = columns.len();
        let want = expected.len();
        failures.push(format!("column count {got} != {want}"));
    }
    for (index, want) in expected.iter().enumerate() {
        let Some(row) = columns.get(index) else {
            continue;
        };
        expect_text(row, "database", "oo_dl01w40", &mut failures);
        expect_text(row, "table", "readings", &mut failures);
        expect_text(row, "name", want.0, &mut failures);
        expect_text(row, "type", want.1, &mut failures);
        if number(row, "position") != (index as u64) + 1 {
            let name = want.0;
            let position = index + 1;
            failures.push(format!("{name} position is not {position}"));
        }
        expect_text(row, "default_kind", want.2, &mut failures);
        expect_text(row, "default_expression", want.3, &mut failures);
    }
    let mut seen_parts = Vec::new();
    for row in &parts {
        seen_parts.push(json!({
            "partition": text(row, "partition"),
            "rows": number(row, "rows").to_string(),
            "active_parts": number(row, "active_parts").to_string(),
        }));
    }
    let expected_parts = [
        ("202401", "2", "2"),
        ("202403", "2", "2"),
    ];
    if seen_parts.len() != expected_parts.len() {
        let got = seen_parts.len();
        failures.push(format!("active part groups {got} != 2"));
    }
    for (index, want) in expected_parts.iter().enumerate() {
        let Some(row) = seen_parts.get(index) else {
            continue;
        };
        if row["partition"] != want.0 || row["rows"] != want.1 || row["active_parts"] != want.2 {
            failures.push(format!("part group {index} is {row}"));
        }
    }
    let duplicate_key = nofinal_has_two_versions(nofinal_gold);
    if !duplicate_key {
        failures.push("nofinal gold does not show two versions of one reading_id".into());
    }
    let metadata_time = text(table, "metadata_modification_time");
    Ok(json!({
        "passed": failures.is_empty(),
        "failures": failures,
        "engine": text(table, "engine"),
        "engine_full": engine_full,
        "partition_key": text(table, "partition_key"),
        "sorting_key": text(table, "sorting_key"),
        "primary_key": text(table, "primary_key"),
        "primary_key_enforced": false,
        "primary_key_evidence": "nofinal retains two versions of reading_id 101 while merges are stopped",
        "duplicate_reading_id_observed": duplicate_key,
        "metadata_modification_time": metadata_time,
        "parts": seen_parts,
        "shared_source_catalog_protocol": "absent",
        "frozen_protocol_not_emitted": "oo.source.pgsql/v1",
    }))
}

fn failed(failures: Vec<String>) -> Value {
    json!({"passed": false, "failures": failures})
}

fn nofinal_has_two_versions(gold: &Value) -> bool {
    let Some(rows) = gold.as_array() else {
        return false;
    };
    let mut versions = Vec::new();
    for row in rows {
        if row.get("reading_id").and_then(Value::as_str) == Some("101") {
            versions.push(row.get("version").and_then(Value::as_str).unwrap_or("").to_string());
        }
    }
    versions.iter().any(|item| item == "1") && versions.iter().any(|item| item == "2")
}

fn expect_text(row: &Value, key: &str, want: &str, failures: &mut Vec<String>) {
    let got = text(row, key);
    if got != want {
        failures.push(format!("{key} is {got:?}, want {want:?}"));
    }
}

fn text(row: &Value, key: &str) -> String {
    match row.get(key) {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

fn number(row: &Value, key: &str) -> u64 {
    match row.get(key) {
        Some(Value::Number(value)) => value.as_u64().unwrap_or(u64::MAX),
        Some(Value::String(value)) => value.parse().unwrap_or(u64::MAX),
        _ => u64::MAX,
    }
}

#[cfg(test)]
mod tests {
    use super::judge;
    use serde_json::json;

    #[test]
    fn native_catalog_matches_the_fixture_contract() {
        let mut columns = String::new();
        let rows = [
            ("reading_id", "UInt64", "", ""),
            ("version", "UInt64", "", ""),
            ("amount", "Decimal(76, 10)", "", ""),
            ("note", "Nullable(String)", "", ""),
            ("code", "LowCardinality(String)", "", ""),
            ("code_or_null", "LowCardinality(Nullable(String))", "", ""),
            ("ts", "DateTime64(9, 'UTC')", "", ""),
            ("tags", "Array(Int64)", "", ""),
            ("labels", "Array(Nullable(String))", "", ""),
            ("qty", "Int64", "DEFAULT", "0"),
            ("label", "String", "DEFAULT", "''"),
            ("wide_u64", "UInt64", "", ""),
        ];
        for (index, row) in rows.iter().enumerate() {
            columns.push_str(&format!(
                "{{\"database\":\"oo_dl01w40\",\"table\":\"readings\",\"name\":\"{}\",\"type\":\"{}\",\"position\":{},\"default_kind\":\"{}\",\"default_expression\":\"{}\"}}\n",
                row.0,
                row.1,
                index + 1,
                row.2,
                row.3.replace('\'', "\\u0027")
            ));
        }
        let table = r#"{"database":"oo_dl01w40","name":"readings","engine":"ReplacingMergeTree","engine_full":"ReplacingMergeTree(version) PARTITION BY toYYYYMM(ts) ORDER BY reading_id","partition_key":"toYYYYMM(ts)","sorting_key":"reading_id","primary_key":"reading_id","metadata_modification_time":"2026-10-10 01:02:03"}"#;
        let parts = "{\"partition\":\"202401\",\"rows\":\"2\",\"active_parts\":2}\n{\"partition\":\"202403\",\"rows\":2,\"active_parts\":\"2\"}\n";
        let gold = json!([
            {"reading_id":"101","version":"1"},
            {"reading_id":"101","version":"2"}
        ]);
        let verdict = judge(&columns, table, parts, &gold).unwrap();
        assert_eq!(verdict["passed"], true, "{verdict}");
        assert_eq!(verdict["primary_key_enforced"], false);
        assert_eq!(verdict["shared_source_catalog_protocol"], "absent");
    }
}
