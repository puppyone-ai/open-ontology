//! Deterministic exact comparison.

use std::collections::BTreeMap;

use oo_core::decimal::ExactDecimal;
use serde_json::{Map, Value};

use crate::parse::{
    Cell, Column, ColumnType, Comparator, Completion, ConfigError, Kind, TypedResult, Warning,
    parse_input,
};
use crate::render_report;

#[derive(Debug)]
pub struct Comparison {
    pub report: String,
    pub matched: bool,
}

pub(crate) struct Diff {
    pub actual: String,
    pub expected: String,
    pub path: String,
    pub reason: &'static str,
    pub row: Option<String>,
}

pub fn compare_document(input: &str) -> Result<Comparison, ConfigError> {
    let parsed = parse_input(input)?;
    let differences = compare_input(&parsed);
    let matched = differences.is_empty();
    Ok(Comparison {
        report: render_report(&differences),
        matched,
    })
}

fn compare_input(input: &crate::parse::Input) -> Vec<Diff> {
    let mut differences = Vec::new();
    if input.expected.kind != input.actual.kind {
        differences.push(text_diff(
            "kind",
            "kind",
            kind_name(input.expected.kind),
            kind_name(input.actual.kind),
        ));
    }
    if input.expected.completion != input.actual.completion {
        differences.push(text_diff(
            "completion",
            "completion",
            completion_name(input.expected.completion),
            completion_name(input.actual.completion),
        ));
    }
    if input.expected.warnings != input.actual.warnings {
        differences.push(text_diff(
            "warnings",
            "warnings",
            &warnings_json(&input.expected.warnings),
            &warnings_json(&input.actual.warnings),
        ));
    }
    if input.expected.kind == Kind::NotFound || input.actual.kind == Kind::NotFound {
        return differences;
    }
    if input.expected.columns != input.actual.columns {
        differences.push(text_diff(
            "columns",
            "schema",
            &columns_json(&input.expected.columns),
            &columns_json(&input.actual.columns),
        ));
        return differences;
    }
    match input.comparator {
        Comparator::Ordered => compare_ordered(&input.expected, &input.actual, &mut differences),
        Comparator::Unordered => {
            compare_unordered(&input.expected, &input.actual, &mut differences);
        }
    }
    differences
}

fn compare_ordered(expected: &TypedResult, actual: &TypedResult, differences: &mut Vec<Diff>) {
    if expected.rows.len() != actual.rows.len() {
        differences.push(text_diff(
            "rows",
            "length",
            &expected.rows.len().to_string(),
            &actual.rows.len().to_string(),
        ));
    }
    let shared = expected.rows.len().min(actual.rows.len());
    for row_index in 0..shared {
        for column_index in 0..expected.columns.len() {
            let left = &expected.rows[row_index][column_index];
            let right = &actual.rows[row_index][column_index];
            if let Some(reason) = cell_difference(left, right) {
                differences.push(text_diff(
                    &format!("rows/{row_index}/{column_index}"),
                    reason,
                    &cell_json(left),
                    &cell_json(right),
                ));
            }
        }
    }
}

fn compare_unordered(expected: &TypedResult, actual: &TypedResult, differences: &mut Vec<Diff>) {
    let expected_counts = row_counts(&expected.rows);
    let actual_counts = row_counts(&actual.rows);
    let mut keys: Vec<&str> = expected_counts
        .keys()
        .map(String::as_str)
        .chain(actual_counts.keys().map(String::as_str))
        .collect();
    keys.sort_unstable();
    keys.dedup();
    for key in keys {
        let left = expected_counts.get(key).copied().unwrap_or(0);
        let right = actual_counts.get(key).copied().unwrap_or(0);
        if left != right {
            differences.push(Diff {
                actual: right.to_string(),
                expected: left.to_string(),
                path: "rows".to_owned(),
                reason: "multiplicity",
                row: Some(key.to_owned()),
            });
        }
    }
}

fn row_counts(rows: &[Vec<Cell>]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for row in rows {
        *counts.entry(row_json(row)).or_insert(0) += 1;
    }
    counts
}

fn cell_difference(expected: &Cell, actual: &Cell) -> Option<&'static str> {
    match (expected, actual) {
        (Cell::Null, Cell::Null)
        | (Cell::Missing, Cell::Missing)
        | (Cell::Unavailable, Cell::Unavailable) => None,
        (Cell::Bool(left), Cell::Bool(right)) if left == right => None,
        (Cell::Int(left), Cell::Int(right)) if left == right => None,
        (Cell::Text(left), Cell::Text(right)) if left == right => None,
        (Cell::Decimal(left), Cell::Decimal(right)) if decimals_match(left, right) => None,
        (Cell::Decimal(_), Cell::Decimal(_)) => Some("decimal"),
        (left, right) if presence_name(left) != presence_name(right) => Some("presence"),
        _ => Some("value"),
    }
}

fn decimals_match(left: &ExactDecimal, right: &ExactDecimal) -> bool {
    if left.coefficient() == "0" && right.coefficient() == "0" {
        return !left.is_negative() && !right.is_negative();
    }
    match left.rescale_exact(right.scale()) {
        Ok(scaled) => scaled == *right,
        Err(_) => false,
    }
}

fn text_diff(path: &str, reason: &'static str, expected: &str, actual: &str) -> Diff {
    Diff {
        actual: actual.to_owned(),
        expected: expected.to_owned(),
        path: path.to_owned(),
        reason,
        row: None,
    }
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Rows => "rows",
        Kind::NotFound => "not_found",
    }
}

fn completion_name(completion: Completion) -> &'static str {
    match completion {
        Completion::Complete => "complete",
        Completion::Partial => "partial",
        Completion::Failed => "failed",
    }
}

fn presence_name(cell: &Cell) -> &'static str {
    match cell {
        Cell::Null => "null",
        Cell::Missing => "missing",
        Cell::Unavailable => "unavailable",
        Cell::Bool(_) | Cell::Int(_) | Cell::Text(_) | Cell::Decimal(_) => "value",
    }
}

pub(crate) fn cell_json(cell: &Cell) -> String {
    render(&cell_value(cell))
}

fn cell_value(cell: &Cell) -> Value {
    let mut map = Map::new();
    match cell {
        Cell::Null => {
            map.insert("presence".to_owned(), Value::String("null".to_owned()));
        }
        Cell::Missing => {
            map.insert("presence".to_owned(), Value::String("missing".to_owned()));
        }
        Cell::Unavailable => {
            map.insert(
                "presence".to_owned(),
                Value::String("unavailable".to_owned()),
            );
        }
        Cell::Bool(value) => {
            map.insert("presence".to_owned(), Value::String("value".to_owned()));
            map.insert("value".to_owned(), Value::Bool(*value));
        }
        Cell::Int(value) | Cell::Text(value) => {
            map.insert("presence".to_owned(), Value::String("value".to_owned()));
            map.insert("value".to_owned(), Value::String(value.clone()));
        }
        Cell::Decimal(decimal) => {
            map.insert(
                "coefficient".to_owned(),
                Value::String(decimal.coefficient().to_owned()),
            );
            map.insert("negative".to_owned(), Value::Bool(decimal.is_negative()));
            map.insert("presence".to_owned(), Value::String("value".to_owned()));
            map.insert(
                "scale".to_owned(),
                Value::Number(serde_json::Number::from(i64::from(decimal.scale()))),
            );
        }
    }
    Value::Object(map)
}

fn row_json(row: &[Cell]) -> String {
    render(&Value::Array(row.iter().map(cell_value).collect()))
}

fn warnings_json(warnings: &[Warning]) -> String {
    let items = warnings
        .iter()
        .map(|warning| {
            let mut map = Map::new();
            map.insert("code".to_owned(), Value::String(warning.code.clone()));
            map.insert("message".to_owned(), Value::String(warning.message.clone()));
            Value::Object(map)
        })
        .collect();
    render(&Value::Array(items))
}

fn columns_json(columns: &[Column]) -> String {
    let items = columns
        .iter()
        .map(|column| {
            let mut map = Map::new();
            map.insert("name".to_owned(), Value::String(column.name.clone()));
            map.insert(
                "type".to_owned(),
                Value::String(column_type_name(column.ty).to_owned()),
            );
            Value::Object(map)
        })
        .collect();
    render(&Value::Array(items))
}

fn column_type_name(ty: ColumnType) -> &'static str {
    match ty {
        ColumnType::Bool => "bool",
        ColumnType::Int64 => "int64",
        ColumnType::Decimal => "decimal",
        ColumnType::Text => "text",
    }
}

fn render(value: &Value) -> String {
    serde_json::to_string(value).expect("canonical json")
}
