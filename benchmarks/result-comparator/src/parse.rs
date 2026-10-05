//! Strict parse of one comparison document.

use oo_core::decimal::ExactDecimal;
use oo_core::limits::{
    CANONICAL_MAX_COEFFICIENT_DIGITS, CANONICAL_MAX_FIELD_NAME_BYTES, QUERY_CELL_MAX_CHARS,
};
use oo_core::parse_json_value;
use serde_json::{Map, Number, Value};

pub(crate) use oo_core::limits::DOCUMENT_MAX_BYTES;
pub(crate) const STDIN_CAP: u64 = DOCUMENT_MAX_BYTES as u64 + 1;

const MAX_ROWS: usize = 2_000;
const MAX_COLUMNS: usize = 64;
const MAX_WARNINGS: usize = 32;
const MAX_PAYLOAD_BYTES: usize = 262_144;
const MAX_WARNING_CODE_BYTES: usize = 64;
const MAX_WARNING_MESSAGE_CHARS: usize = 200;
const PROTOCOL: &str = "oo.benchmark.result-comparison/v1";

#[derive(Debug, PartialEq, Eq)]
pub struct ConfigError {
    token: &'static str,
    pointer: Option<String>,
}

impl ConfigError {
    pub(crate) fn bare(token: &'static str) -> Self {
        Self {
            token,
            pointer: None,
        }
    }

    pub(crate) fn at(token: &'static str, pointer: impl Into<String>) -> Self {
        Self {
            token,
            pointer: Some(pointer.into()),
        }
    }

    pub fn stderr_line(&self) -> String {
        let token = self.token;
        match &self.pointer {
            Some(pointer) => format!("result-comparator: {token} {pointer}\n"),
            None => format!("result-comparator: {token}\n"),
        }
    }

    pub fn token(&self) -> &'static str {
        self.token
    }

    pub fn pointer(&self) -> Option<&str> {
        self.pointer.as_deref()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Comparator {
    Ordered,
    Unordered,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Rows,
    NotFound,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Completion {
    Complete,
    Partial,
    Failed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ColumnType {
    Bool,
    Int64,
    Decimal,
    Text,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Warning {
    pub code: String,
    pub message: String,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Column {
    pub name: String,
    pub ty: ColumnType,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Cell {
    Null,
    Missing,
    Unavailable,
    Bool(bool),
    Int(String),
    Text(String),
    Decimal(ExactDecimal),
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct TypedResult {
    pub kind: Kind,
    pub completion: Completion,
    pub warnings: Vec<Warning>,
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Cell>>,
}

pub(crate) struct Input {
    pub comparator: Comparator,
    pub expected: TypedResult,
    pub actual: TypedResult,
}

pub(crate) fn parse_input(input: &str) -> Result<Input, ConfigError> {
    if input.len() > DOCUMENT_MAX_BYTES {
        return Err(ConfigError::at("budget", "/"));
    }
    let value = parse_json_value(input, PROTOCOL).map_err(|_| ConfigError::bare("json"))?;
    let root = object(&value, "/")?;
    check_keys(
        root,
        &["actual", "expected", "row_comparator", "version"],
        "/",
    )?;
    let version = req_str(root, "version", "/")?;
    if version != PROTOCOL {
        return Err(ConfigError::at("schema", "/version"));
    }
    let comparator = match req_str(root, "row_comparator", "/")? {
        "ordered" => Comparator::Ordered,
        "unordered" => Comparator::Unordered,
        _ => return Err(ConfigError::at("schema", "/row_comparator")),
    };
    let expected = parse_result(req(root, "expected", "/")?, "/expected")?;
    let actual = parse_result(req(root, "actual", "/")?, "/actual")?;
    Ok(Input {
        comparator,
        expected,
        actual,
    })
}

fn parse_result(value: &Value, pointer: &str) -> Result<TypedResult, ConfigError> {
    let map = object(value, pointer)?;
    let kind = match req_str(map, "kind", pointer)? {
        "rows" => Kind::Rows,
        "not_found" => Kind::NotFound,
        _ => return Err(ConfigError::at("schema", child(pointer, "kind"))),
    };
    let completion = match req_str(map, "completion", pointer)? {
        "complete" => Completion::Complete,
        "partial" => Completion::Partial,
        "failed" => Completion::Failed,
        _ => return Err(ConfigError::at("schema", child(pointer, "completion"))),
    };
    let warnings = parse_warnings(req(map, "warnings", pointer)?, pointer)?;
    let (columns, rows) = match kind {
        Kind::NotFound => {
            check_keys(map, &["completion", "kind", "warnings"], pointer)?;
            (Vec::new(), Vec::new())
        }
        Kind::Rows => {
            check_keys(
                map,
                &["columns", "completion", "kind", "rows", "warnings"],
                pointer,
            )?;
            let columns = parse_columns(req(map, "columns", pointer)?, pointer)?;
            let rows = parse_rows(req(map, "rows", pointer)?, &columns, pointer)?;
            (columns, rows)
        }
    };
    let result = TypedResult {
        kind,
        completion,
        warnings,
        columns,
        rows,
    };
    if payload_bytes(&result) > MAX_PAYLOAD_BYTES {
        return Err(ConfigError::at("budget", pointer));
    }
    Ok(result)
}

fn parse_warnings(value: &Value, pointer: &str) -> Result<Vec<Warning>, ConfigError> {
    let items = array(value, &child(pointer, "warnings"))?;
    if items.len() > MAX_WARNINGS {
        return Err(ConfigError::at("budget", child(pointer, "warnings")));
    }
    let mut warnings = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let at = format!("{}/warnings/{index}", pointer.trim_end_matches('/'));
        let map = object(item, &at)?;
        check_keys(map, &["code", "message"], &at)?;
        let code = req_str(map, "code", &at)?.to_owned();
        if !valid_code(&code) {
            let token = if code.len() > MAX_WARNING_CODE_BYTES || code.is_empty() {
                "budget"
            } else {
                "schema"
            };
            return Err(ConfigError::at(token, format!("{at}/code")));
        }
        let message = req_str(map, "message", &at)?;
        let chars = message.chars().count();
        if chars > MAX_WARNING_MESSAGE_CHARS {
            return Err(ConfigError::at("budget", format!("{at}/message")));
        }
        if chars == 0 || message.contains('\0') {
            return Err(ConfigError::at("schema", format!("{at}/message")));
        }
        warnings.push(Warning {
            code,
            message: message.to_owned(),
        });
    }
    Ok(warnings)
}

fn parse_columns(value: &Value, pointer: &str) -> Result<Vec<Column>, ConfigError> {
    let items = array(value, &child(pointer, "columns"))?;
    if items.len() > MAX_COLUMNS {
        return Err(ConfigError::at("budget", child(pointer, "columns")));
    }
    let mut columns: Vec<Column> = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let at = format!("{}/columns/{index}", pointer.trim_end_matches('/'));
        let map = object(item, &at)?;
        check_keys(map, &["name", "type"], &at)?;
        let name = req_str(map, "name", &at)?.to_owned();
        if !valid_name(&name) {
            let token = if name.len() > CANONICAL_MAX_FIELD_NAME_BYTES || name.is_empty() {
                "budget"
            } else {
                "schema"
            };
            return Err(ConfigError::at(token, format!("{at}/name")));
        }
        if columns.iter().any(|column| column.name == name) {
            return Err(ConfigError::at("schema", child(pointer, "columns")));
        }
        let ty = match req_str(map, "type", &at)? {
            "bool" => ColumnType::Bool,
            "int64" => ColumnType::Int64,
            "decimal" => ColumnType::Decimal,
            "text" => ColumnType::Text,
            _ => return Err(ConfigError::at("schema", format!("{at}/type"))),
        };
        columns.push(Column { name, ty });
    }
    Ok(columns)
}

fn parse_rows(
    value: &Value,
    columns: &[Column],
    pointer: &str,
) -> Result<Vec<Vec<Cell>>, ConfigError> {
    let items = array(value, &child(pointer, "rows"))?;
    if items.len() > MAX_ROWS {
        return Err(ConfigError::at("budget", child(pointer, "rows")));
    }
    let mut rows = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let at = format!("{}/rows/{index}", pointer.trim_end_matches('/'));
        let cells = array(item, &at)?;
        if cells.len() != columns.len() {
            return Err(ConfigError::at("schema", at));
        }
        let mut row = Vec::with_capacity(cells.len());
        for (column_index, (cell, column)) in cells.iter().zip(columns).enumerate() {
            row.push(parse_cell(
                cell,
                column.ty,
                &format!("{at}/{column_index}"),
            )?);
        }
        rows.push(row);
    }
    Ok(rows)
}

fn parse_cell(value: &Value, ty: ColumnType, pointer: &str) -> Result<Cell, ConfigError> {
    let map = object(value, pointer)?;
    match req_str(map, "presence", pointer)? {
        "null" => {
            check_keys(map, &["presence"], pointer)?;
            Ok(Cell::Null)
        }
        "missing" => {
            check_keys(map, &["presence"], pointer)?;
            Ok(Cell::Missing)
        }
        "unavailable" => {
            check_keys(map, &["presence"], pointer)?;
            Ok(Cell::Unavailable)
        }
        "value" => parse_value_cell(map, ty, pointer),
        _ => Err(ConfigError::at("schema", child(pointer, "presence"))),
    }
}

fn parse_value_cell(
    map: &Map<String, Value>,
    ty: ColumnType,
    pointer: &str,
) -> Result<Cell, ConfigError> {
    match ty {
        ColumnType::Bool => {
            check_keys(map, &["presence", "value"], pointer)?;
            Ok(Cell::Bool(req_bool(map, "value", pointer)?))
        }
        ColumnType::Int64 => {
            check_keys(map, &["presence", "value"], pointer)?;
            let text = req_str(map, "value", pointer)?;
            canonical_i64(text)
                .map(Cell::Int)
                .map_err(|_| ConfigError::at("int64", child(pointer, "value")))
        }
        ColumnType::Text => {
            check_keys(map, &["presence", "value"], pointer)?;
            let text = req_str(map, "value", pointer)?;
            if text.chars().count() > QUERY_CELL_MAX_CHARS {
                return Err(ConfigError::at("budget", child(pointer, "value")));
            }
            if text.contains('\0') {
                return Err(ConfigError::at("schema", child(pointer, "value")));
            }
            Ok(Cell::Text(text.to_owned()))
        }
        ColumnType::Decimal => {
            check_keys(
                map,
                &["coefficient", "negative", "presence", "scale"],
                pointer,
            )?;
            let negative = req_bool(map, "negative", pointer)?;
            let coefficient = req_str(map, "coefficient", pointer)?;
            if !valid_coefficient(coefficient) {
                let token = if coefficient.len() > CANONICAL_MAX_COEFFICIENT_DIGITS {
                    "budget"
                } else {
                    "decimal"
                };
                return Err(ConfigError::at(token, child(pointer, "coefficient")));
            }
            if negative && coefficient == "0" {
                return Err(ConfigError::at("decimal", child(pointer, "negative")));
            }
            let scale = req_scale(map, pointer)?;
            let decimal = ExactDecimal::new(negative, coefficient, scale)
                .map_err(|_| ConfigError::at("decimal", pointer))?;
            Ok(Cell::Decimal(decimal))
        }
    }
}

fn payload_bytes(result: &TypedResult) -> usize {
    let mut total = 0usize;
    for column in &result.columns {
        total = total.saturating_add(column.name.len());
    }
    for warning in &result.warnings {
        total = total.saturating_add(warning.code.len());
        total = total.saturating_add(warning.message.len());
    }
    for row in &result.rows {
        for cell in row {
            total = total.saturating_add(1);
            let extra = match cell {
                Cell::Text(text) | Cell::Int(text) => text.len(),
                Cell::Decimal(decimal) => decimal.coefficient().len(),
                Cell::Null | Cell::Missing | Cell::Unavailable | Cell::Bool(_) => 0,
            };
            total = total.saturating_add(extra);
        }
    }
    total
}

fn valid_code(code: &str) -> bool {
    let bytes = code.as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_WARNING_CODE_BYTES {
        return false;
    }
    bytes[0].is_ascii_uppercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= CANONICAL_MAX_FIELD_NAME_BYTES
        && bytes
            .iter()
            .all(|byte| (*byte >= 0x80) || ((0x20..0x7f).contains(byte) && *byte != b'/'))
}

fn valid_coefficient(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.len() > CANONICAL_MAX_COEFFICIENT_DIGITS {
        return false;
    }
    if !bytes.iter().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    bytes == b"0" || bytes[0] != b'0'
}

fn canonical_i64(text: &str) -> Result<String, ()> {
    if text == "-0" || text.is_empty() || text.starts_with('+') {
        return Err(());
    }
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(());
    }
    if digits.len() > 1 && digits.starts_with('0') {
        return Err(());
    }
    let value: i64 = text.parse().map_err(|_| ())?;
    if value.to_string() != text {
        return Err(());
    }
    Ok(text.to_owned())
}

fn req_scale(map: &Map<String, Value>, pointer: &str) -> Result<i32, ConfigError> {
    let value = req(map, "scale", pointer)?;
    let Value::Number(number) = value else {
        return Err(ConfigError::at("schema", child(pointer, "scale")));
    };
    let text = number_text(number);
    if text.contains(['.', 'e', 'E', '+']) || text == "-0" {
        return Err(ConfigError::at("decimal", child(pointer, "scale")));
    }
    let scale: i32 = text
        .parse()
        .map_err(|_| ConfigError::at("decimal", child(pointer, "scale")))?;
    if scale.unsigned_abs() as usize > CANONICAL_MAX_COEFFICIENT_DIGITS {
        return Err(ConfigError::at("decimal", child(pointer, "scale")));
    }
    Ok(scale)
}

fn number_text(number: &Number) -> String {
    number.to_string()
}

fn object<'a>(value: &'a Value, pointer: &str) -> Result<&'a Map<String, Value>, ConfigError> {
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(ConfigError::at("schema", pointer)),
    }
}

fn array<'a>(value: &'a Value, pointer: &str) -> Result<&'a Vec<Value>, ConfigError> {
    match value {
        Value::Array(items) => Ok(items),
        _ => Err(ConfigError::at("schema", pointer)),
    }
}

fn req<'a>(
    map: &'a Map<String, Value>,
    key: &str,
    pointer: &str,
) -> Result<&'a Value, ConfigError> {
    map.get(key)
        .ok_or_else(|| ConfigError::at("schema", child(pointer, key)))
}

fn req_str<'a>(
    map: &'a Map<String, Value>,
    key: &str,
    pointer: &str,
) -> Result<&'a str, ConfigError> {
    match req(map, key, pointer)? {
        Value::String(text) => Ok(text),
        _ => Err(ConfigError::at("schema", child(pointer, key))),
    }
}

fn req_bool(map: &Map<String, Value>, key: &str, pointer: &str) -> Result<bool, ConfigError> {
    match req(map, key, pointer)? {
        Value::Bool(value) => Ok(*value),
        _ => Err(ConfigError::at("schema", child(pointer, key))),
    }
}

fn check_keys(
    map: &Map<String, Value>,
    allowed: &[&str],
    pointer: &str,
) -> Result<(), ConfigError> {
    let mut unknown: Vec<&str> = map
        .keys()
        .filter(|key| !allowed.contains(&key.as_str()))
        .map(String::as_str)
        .collect();
    unknown.sort_unstable();
    if let Some(key) = unknown.first() {
        return Err(ConfigError::at("schema", child(pointer, key)));
    }
    Ok(())
}

fn child(pointer: &str, key: &str) -> String {
    if pointer == "/" {
        format!("/{key}")
    } else {
        format!("{pointer}/{key}")
    }
}
