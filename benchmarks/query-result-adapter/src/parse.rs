//! Strict parse of one adapter JSONL stream.

use oo_core::decimal::ExactDecimal;
use oo_core::limits::{
    CANONICAL_MAX_COEFFICIENT_DIGITS, CANONICAL_MAX_FIELD_NAME_BYTES, QUERY_CELL_MAX_CHARS,
};
use oo_core::parse_json_value;
use serde::Serialize;
use serde_json::{Map, Number, Value};

pub(crate) use oo_core::limits::DOCUMENT_MAX_BYTES;
pub(crate) const STDIN_CAP: u64 = DOCUMENT_MAX_BYTES as u64 + 1;

const MAX_ROWS: usize = 2_000;
const MAX_COLUMNS: usize = 64;
const MAX_PAYLOAD_BYTES: usize = 262_144;
const MAX_RESULT_BYTES: u64 = 262_144;
const MAX_TURN: u64 = 7;
const PROTOCOL: &str = "oo.benchmark.query-result-adapter/v1";
const LINE_PROTOCOL: &str = "oo.benchmark.query-result-adapter-line/v1";

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
            Some(pointer) => format!("query-result-adapter: {token} {pointer}\n"),
            None => format!("query-result-adapter: {token}\n"),
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
enum ColumnType {
    Bool,
    Int64,
    Decimal,
    Text,
}

struct Binding {
    name: String,
    ty: ColumnType,
    ordinal: Option<usize>,
}

struct Header {
    turn: u64,
    max_rows: u64,
    max_result_bytes: u64,
    columns: Vec<Binding>,
    width: usize,
}

struct Summary {
    status: &'static str,
    returned_rows: u64,
    result_bytes: u64,
    end_audit_ok: bool,
}

pub(crate) fn adapt(input: &str) -> Result<String, ConfigError> {
    let lines = split_lines(input)?;
    let header = parse_header(lines[0], "/0")?;
    let mut rows: Vec<Vec<OutCell>> = Vec::new();
    let mut summary: Option<Summary> = None;
    for (index, line) in lines.iter().enumerate().skip(1) {
        let pointer = format!("/{index}");
        let value = parse_line(line, &pointer)?;
        let map = object(&value, &pointer)?;
        let record = req_str(map, "record", &pointer)?;
        match record {
            "header" => return Err(ConfigError::at("protocol", pointer)),
            "row" => {
                if summary.is_some() {
                    return Err(ConfigError::at("protocol", pointer));
                }
                if rows.len() == MAX_ROWS {
                    return Err(ConfigError::at("budget", "/rows"));
                }
                rows.push(parse_row(map, &header, &pointer)?);
            }
            "summary" => {
                if summary.is_some() {
                    return Err(ConfigError::at("protocol", pointer));
                }
                summary = Some(parse_summary(map, header.turn, &pointer)?);
            }
            _ => return Err(ConfigError::at("protocol", pointer)),
        }
    }
    let Some(summary) = summary else {
        return Err(ConfigError::at("protocol", "/"));
    };
    let row_count = u64::try_from(rows.len()).map_err(|_| ConfigError::at("budget", "/rows"))?;
    if row_count != summary.returned_rows {
        return Err(ConfigError::at("rowcount", "/summary/returned_rows"));
    }
    if row_count > header.max_rows || summary.result_bytes > header.max_result_bytes {
        return Err(ConfigError::at("budget", "/summary"));
    }
    let completion = match (summary.status, summary.end_audit_ok) {
        ("succeeded", true) => "complete",
        ("succeeded", false) => {
            return Err(ConfigError::at("completion", "/summary/end_audit_ok"));
        }
        ("partial", _) => "partial",
        ("failed", _) => "failed",
        _ => return Err(ConfigError::at("schema", "/summary/status")),
    };
    let columns: Vec<ColumnOut> = header
        .columns
        .iter()
        .map(|column| ColumnOut {
            name: column.name.clone(),
            ty: column.ty.name(),
        })
        .collect();
    let result = TypedOut {
        kind: "rows",
        completion,
        warnings: Vec::new(),
        columns,
        rows,
    };
    if payload_bytes(&result) > MAX_PAYLOAD_BYTES {
        return Err(ConfigError::at("budget", "/"));
    }
    serde_json::to_string(&result).map_err(|_| ConfigError::bare("io"))
}

fn split_lines(input: &str) -> Result<Vec<&str>, ConfigError> {
    if input.is_empty() || !input.ends_with('\n') {
        return Err(ConfigError::at("protocol", "/"));
    }
    let mut lines: Vec<&str> = input.split('\n').collect();
    lines.pop();
    if lines.is_empty() || lines.iter().any(|line| line.is_empty()) {
        return Err(ConfigError::at("protocol", "/"));
    }
    Ok(lines)
}

fn parse_line(line: &str, pointer: &str) -> Result<Value, ConfigError> {
    parse_json_value(line, LINE_PROTOCOL).map_err(|_| ConfigError::bare("json").at_pointer(pointer))
}

impl ConfigError {
    fn at_pointer(mut self, pointer: &str) -> Self {
        if self.pointer.is_none() && self.token == "json" {
            self.pointer = Some(pointer.to_owned());
        }
        self
    }
}

fn parse_header(line: &str, pointer: &str) -> Result<Header, ConfigError> {
    let value = parse_line(line, pointer)?;
    let map = object(&value, pointer)?;
    match map.get("record").and_then(Value::as_str) {
        Some("header") => {}
        _ => return Err(ConfigError::at("protocol", pointer)),
    }
    check_keys(
        map,
        &[
            "columns",
            "max_result_bytes",
            "max_rows",
            "record",
            "turn",
            "version",
        ],
        pointer,
    )?;
    if req_str(map, "version", pointer)? != PROTOCOL {
        return Err(ConfigError::at("schema", child(pointer, "version")));
    }
    let turn = req_bound(map, "turn", pointer, MAX_TURN)?;
    let max_rows = req_bound(
        map,
        "max_rows",
        pointer,
        u64::try_from(MAX_ROWS).unwrap_or(0),
    )?;
    let max_result_bytes = req_bound(map, "max_result_bytes", pointer, MAX_RESULT_BYTES)?;
    let columns = parse_columns(req(map, "columns", pointer)?, pointer)?;
    let width = columns
        .iter()
        .filter(|column| column.ordinal.is_some())
        .count();
    let mut seen = vec![false; width];
    for column in &columns {
        let Some(ordinal) = column.ordinal else {
            continue;
        };
        if ordinal >= width || seen[ordinal] {
            return Err(ConfigError::at("schema", child(pointer, "columns")));
        }
        seen[ordinal] = true;
    }
    if seen.iter().any(|slot| !*slot) {
        return Err(ConfigError::at("schema", child(pointer, "columns")));
    }
    Ok(Header {
        turn,
        max_rows,
        max_result_bytes,
        columns,
        width,
    })
}

fn parse_columns(value: &Value, pointer: &str) -> Result<Vec<Binding>, ConfigError> {
    let items = array(value, &child(pointer, "columns"))?;
    if items.len() > MAX_COLUMNS {
        return Err(ConfigError::at("budget", child(pointer, "columns")));
    }
    let mut columns: Vec<Binding> = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let at = format!("{}/columns/{index}", pointer.trim_end_matches('/'));
        let map = object(item, &at)?;
        check_keys(map, &["name", "ordinal", "type"], &at)?;
        let name = req_str(map, "name", &at)?.to_owned();
        if !valid_name(&name) {
            let token = if name.is_empty() || name.len() > CANONICAL_MAX_FIELD_NAME_BYTES {
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
        let ordinal = match req(map, "ordinal", &at)? {
            Value::Null => None,
            Value::Number(number) => Some(parse_ordinal(number, &at)?),
            _ => return Err(ConfigError::at("schema", format!("{at}/ordinal"))),
        };
        columns.push(Binding { name, ty, ordinal });
    }
    Ok(columns)
}

fn parse_ordinal(number: &Number, pointer: &str) -> Result<usize, ConfigError> {
    let value =
        parse_u64(number).map_err(|_| ConfigError::at("schema", format!("{pointer}/ordinal")))?;
    if value >= u64::try_from(MAX_COLUMNS).unwrap_or(0) {
        return Err(ConfigError::at("budget", format!("{pointer}/ordinal")));
    }
    usize::try_from(value).map_err(|_| ConfigError::at("schema", format!("{pointer}/ordinal")))
}

fn parse_row(
    map: &Map<String, Value>,
    header: &Header,
    pointer: &str,
) -> Result<Vec<OutCell>, ConfigError> {
    check_keys(map, &["cells", "record", "turn"], pointer)?;
    let turn = req_bound(map, "turn", pointer, MAX_TURN)?;
    if turn != header.turn {
        return Err(ConfigError::at("protocol", child(pointer, "turn")));
    }
    let cells = array(req(map, "cells", pointer)?, &child(pointer, "cells"))?;
    if cells.len() != header.width {
        return Err(ConfigError::at("schema", child(pointer, "cells")));
    }
    let mut parsed = Vec::with_capacity(cells.len());
    for (index, cell) in cells.iter().enumerate() {
        parsed.push(parse_gateway_cell(
            cell,
            &format!("{}/cells/{index}", pointer.trim_end_matches('/')),
        )?);
    }
    let mut out = Vec::with_capacity(header.columns.len());
    for column in &header.columns {
        let Some(ordinal) = column.ordinal else {
            out.push(OutCell::Bare(BareCell {
                presence: "missing",
            }));
            continue;
        };
        let cell_pointer = format!("{}/cells/{ordinal}", pointer.trim_end_matches('/'));
        out.push(convert_cell(&parsed[ordinal], column.ty, &cell_pointer)?);
    }
    Ok(out)
}

enum GatewayCell {
    Null,
    Bool(bool),
    Int(String),
    Text(String),
}

fn parse_gateway_cell(value: &Value, pointer: &str) -> Result<GatewayCell, ConfigError> {
    let map = object(value, pointer)?;
    let kind = req_str(map, "kind", pointer)?;
    match kind {
        "null" => {
            check_keys(map, &["kind"], pointer)?;
            Ok(GatewayCell::Null)
        }
        "bool" => {
            check_keys(map, &["kind", "value"], pointer)?;
            Ok(GatewayCell::Bool(req_bool(map, "value", pointer)?))
        }
        "int" => {
            check_keys(map, &["kind", "value"], pointer)?;
            Ok(GatewayCell::Int(req_str(map, "value", pointer)?.to_owned()))
        }
        "text" => {
            check_keys(map, &["kind", "value"], pointer)?;
            Ok(GatewayCell::Text(
                req_str(map, "value", pointer)?.to_owned(),
            ))
        }
        _ => Err(ConfigError::at("protocol", child(pointer, "kind"))),
    }
}

fn convert_cell(cell: &GatewayCell, ty: ColumnType, pointer: &str) -> Result<OutCell, ConfigError> {
    if matches!(cell, GatewayCell::Null) {
        return Ok(OutCell::Bare(BareCell { presence: "null" }));
    }
    match (ty, cell) {
        (ColumnType::Bool, GatewayCell::Bool(value)) => Ok(OutCell::Value(ValueCell {
            presence: "value",
            value: Scalar::Bool(*value),
        })),
        (ColumnType::Int64, GatewayCell::Int(text)) => {
            let spelling = canonical_i64(text).map_err(|_| ConfigError::at("int64", pointer))?;
            Ok(OutCell::Value(ValueCell {
                presence: "value",
                value: Scalar::Text(spelling),
            }))
        }
        (ColumnType::Text, GatewayCell::Text(text)) => {
            if text.chars().count() > QUERY_CELL_MAX_CHARS {
                return Err(ConfigError::at("budget", pointer));
            }
            if text.contains('\0') {
                return Err(ConfigError::at("schema", pointer));
            }
            Ok(OutCell::Value(ValueCell {
                presence: "value",
                value: Scalar::Text(text.clone()),
            }))
        }
        (ColumnType::Decimal, GatewayCell::Int(text)) => {
            canonical_i64(text).map_err(|_| ConfigError::at("decimal", pointer))?;
            decimal_cell(text, true, pointer)
        }
        (ColumnType::Decimal, GatewayCell::Text(text)) => decimal_cell(text, false, pointer),
        _ => Err(ConfigError::at("type", pointer)),
    }
}

fn decimal_cell(text: &str, integer_only: bool, pointer: &str) -> Result<OutCell, ConfigError> {
    let decimal =
        exact_decimal(text, integer_only).map_err(|_| ConfigError::at("decimal", pointer))?;
    Ok(OutCell::Decimal(DecimalCell {
        coefficient: decimal.coefficient().to_owned(),
        negative: decimal.is_negative(),
        presence: "value",
        scale: decimal.scale(),
    }))
}

fn exact_decimal(text: &str, integer_only: bool) -> Result<ExactDecimal, ()> {
    if !decimal_spelling(text, integer_only) {
        return Err(());
    }
    let decimal = ExactDecimal::parse_sql(text).map_err(|_| ())?;
    if text.starts_with('-') && !decimal.is_negative() {
        return Err(());
    }
    if decimal.coefficient().len() > CANONICAL_MAX_COEFFICIENT_DIGITS {
        return Err(());
    }
    Ok(decimal)
}

fn decimal_spelling(text: &str, integer_only: bool) -> bool {
    if text.is_empty() || text.starts_with('+') {
        return false;
    }
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty() || digits.contains(['e', 'E', '+', ' ']) {
        return false;
    }
    let (body, fraction) = match digits.split_once('.') {
        Some((body, fraction)) => {
            if integer_only || fraction.is_empty() || fraction.contains('.') {
                return false;
            }
            (body, Some(fraction))
        }
        None => (digits, None),
    };
    if body.is_empty() || !body.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    if body.len() > 1 && body.starts_with('0') {
        return false;
    }
    fraction.is_none_or(|fraction| fraction.bytes().all(|byte| byte.is_ascii_digit()))
}

fn parse_summary(
    map: &Map<String, Value>,
    turn: u64,
    pointer: &str,
) -> Result<Summary, ConfigError> {
    check_keys(
        map,
        &[
            "end_audit_ok",
            "executed_sha256",
            "original_sha256",
            "record",
            "result_bytes",
            "returned_rows",
            "rollback_confirmed",
            "socket_dropped",
            "sqlstate",
            "status",
            "stop",
            "truncated",
            "turn",
        ],
        pointer,
    )?;
    let summary_turn = req_bound(map, "turn", pointer, MAX_TURN)?;
    if summary_turn != turn {
        return Err(ConfigError::at("protocol", child(pointer, "turn")));
    }
    let status = match req_str(map, "status", pointer)? {
        "succeeded" => "succeeded",
        "partial" => "partial",
        "failed" => "failed",
        _ => return Err(ConfigError::at("schema", child(pointer, "status"))),
    };
    let _truncated = req_bool(map, "truncated", pointer)?;
    let returned_rows = req_bound(
        map,
        "returned_rows",
        pointer,
        u64::try_from(MAX_ROWS).unwrap_or(0),
    )?;
    let result_bytes = req_bound(map, "result_bytes", pointer, MAX_RESULT_BYTES)?;
    let _rollback = req_bool(map, "rollback_confirmed", pointer)?;
    let _socket = req_bool(map, "socket_dropped", pointer)?;
    let end_audit_ok = req_bool(map, "end_audit_ok", pointer)?;
    if !sha256_hex_ok(req_str(map, "original_sha256", pointer)?)
        || !sha256_hex_ok(req_str(map, "executed_sha256", pointer)?)
    {
        return Err(ConfigError::at("schema", child(pointer, "original_sha256")));
    }
    parse_stop(req(map, "stop", pointer)?, pointer)?;
    parse_sqlstate(req(map, "sqlstate", pointer)?, pointer)?;
    Ok(Summary {
        status,
        returned_rows,
        result_bytes,
        end_audit_ok,
    })
}

fn parse_stop(value: &Value, pointer: &str) -> Result<(), ConfigError> {
    match value {
        Value::Null => Ok(()),
        Value::String(text) => match text.as_str() {
            "timeout" | "canceled" | "type_refused" | "database" | "protocol" => Ok(()),
            _ => Err(ConfigError::at("schema", child(pointer, "stop"))),
        },
        _ => Err(ConfigError::at("schema", child(pointer, "stop"))),
    }
}

fn parse_sqlstate(value: &Value, pointer: &str) -> Result<(), ConfigError> {
    match value {
        Value::Null => Ok(()),
        Value::String(text)
            if text.len() == 5
                && text
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit()) =>
        {
            Ok(())
        }
        _ => Err(ConfigError::at("schema", child(pointer, "sqlstate"))),
    }
}

fn payload_bytes(result: &TypedOut) -> usize {
    let mut total = 0usize;
    for column in &result.columns {
        total = total.saturating_add(column.name.len());
    }
    for row in &result.rows {
        for cell in row {
            total = total.saturating_add(1);
            let extra = match cell {
                OutCell::Value(cell) => match &cell.value {
                    Scalar::Text(text) => text.len(),
                    Scalar::Bool(_) => 0,
                },
                OutCell::Decimal(cell) => cell.coefficient.len(),
                OutCell::Bare(_) => 0,
            };
            total = total.saturating_add(extra);
        }
    }
    total
}

fn valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= CANONICAL_MAX_FIELD_NAME_BYTES
        && bytes
            .iter()
            .all(|byte| (*byte >= 0x80) || ((0x20..0x7f).contains(byte) && *byte != b'/'))
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

fn sha256_hex_ok(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn req_bound(
    map: &Map<String, Value>,
    key: &str,
    pointer: &str,
    max: u64,
) -> Result<u64, ConfigError> {
    let value = req(map, key, pointer)?;
    let Value::Number(number) = value else {
        return Err(ConfigError::at("schema", child(pointer, key)));
    };
    let parsed = parse_u64(number).map_err(|_| ConfigError::at("schema", child(pointer, key)))?;
    if parsed > max {
        return Err(ConfigError::at("budget", child(pointer, key)));
    }
    Ok(parsed)
}

fn parse_u64(number: &Number) -> Result<u64, ()> {
    let text = number.to_string();
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(());
    }
    if text.len() > 1 && text.starts_with('0') {
        return Err(());
    }
    text.parse().map_err(|_| ())
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

#[derive(Serialize)]
struct TypedOut {
    kind: &'static str,
    completion: &'static str,
    warnings: Vec<WarningOut>,
    columns: Vec<ColumnOut>,
    rows: Vec<Vec<OutCell>>,
}

#[derive(Serialize)]
struct WarningOut {}

#[derive(Serialize)]
struct ColumnOut {
    name: String,
    #[serde(rename = "type")]
    ty: &'static str,
}

#[derive(Serialize)]
#[serde(untagged)]
enum OutCell {
    Decimal(DecimalCell),
    Value(ValueCell),
    Bare(BareCell),
}

#[derive(Serialize)]
struct DecimalCell {
    coefficient: String,
    negative: bool,
    presence: &'static str,
    scale: i32,
}

#[derive(Serialize)]
struct ValueCell {
    presence: &'static str,
    value: Scalar,
}

#[derive(Serialize)]
struct BareCell {
    presence: &'static str,
}

#[derive(Serialize)]
#[serde(untagged)]
enum Scalar {
    Bool(bool),
    Text(String),
}

impl ColumnType {
    fn name(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Int64 => "int64",
            Self::Decimal => "decimal",
            Self::Text => "text",
        }
    }
}
