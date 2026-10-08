use std::path::Path;

use crate::decimal::{Decimal, canonical_decimal, canonical_i64, int_decimal, plain_decimal};
use crate::fault::Fault;
use crate::json::{self, Json, JsonError};
use crate::limits::{
    CSV_NAME, MAX_CELL_BYTES, MAX_COLUMNS, MAX_EXPORT_BYTES, MAX_ROWS, SIDECAR_NAME, SOURCE_NAME,
};
use crate::path;
use crate::sha256::{is_lower_hex, sha256_hex};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cell {
    Null,
    Bool(bool),
    Int(Decimal),
    Text(String),
    Decimal(Decimal),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Facts {
    pub status: String,
    pub truncated: bool,
    pub returned_rows: u64,
    pub result_bytes: u64,
    pub rollback_confirmed: bool,
    pub socket_dropped: bool,
    pub stop: Option<String>,
    pub sqlstate: Option<String>,
    pub original_sha256: String,
    pub executed_sha256: String,
}

#[derive(Clone, Debug)]
pub struct Export {
    pub rows: Vec<Vec<Cell>>,
    pub width: usize,
    pub clean: bool,
    pub facts: Facts,
    pub source_sha256: String,
    pub csv_sha256: String,
    pub sidecar_sha256: String,
}

pub fn load_export(dir: &Path) -> Result<Export, Fault> {
    let root = path::require_dir(dir)?;
    let source_bytes = path::read_regular(&root.join(SOURCE_NAME), MAX_EXPORT_BYTES, Fault::source())?;
    let csv_bytes = path::read_regular(&root.join(CSV_NAME), MAX_EXPORT_BYTES, Fault::source())?;
    let sidecar_bytes = path::read_regular(&root.join(SIDECAR_NAME), MAX_EXPORT_BYTES, Fault::source())?;
    if source_bytes.contains(&b'\r') || sidecar_bytes.contains(&b'\r') {
        return Err(Fault::source());
    }
    let source_sha = sha256_hex(&source_bytes);
    let csv_sha = sha256_hex(&csv_bytes);
    let sidecar_sha = sha256_hex(&sidecar_bytes);
    let sidecar_text = std::str::from_utf8(&sidecar_bytes).map_err(|_| Fault::source())?;
    let sidecar = match json::parse_json(sidecar_text) {
        Ok(value) => value,
        Err(JsonError::Budget) => return Err(Fault::budget()),
        Err(JsonError::Syntax) => return Err(Fault::source()),
    };
    let mut fields = json::object(sidecar).map_err(|()| Fault::source())?;
    let version = take_string(&mut fields, "version")?;
    if version != "oo.query-result-export/v1" {
        return Err(Fault::source());
    }
    expect_string(&mut fields, "format", "csv")?;
    expect_string(&mut fields, "xlsx", "unsupported")?;
    expect_bool(&mut fields, "http_auth", false)?;
    expect_bool(&mut fields, "authenticated", false)?;
    let complete = take_bool(&mut fields, "complete")?;
    let partial = take_bool(&mut fields, "partial")?;
    let source_sha256 = take_hex(&mut fields, "source_sha256")?;
    let source_len = take_u64(&mut fields, "source_bytes")?;
    let turn = take_u64(&mut fields, "turn")?;
    let status = take_status(&mut fields)?;
    let truncated = take_bool(&mut fields, "truncated")?;
    let stop = take_stop(&mut fields, "stop")?;
    let sqlstate = take_sqlstate(&mut fields)?;
    let end_audit_ok = take_bool(&mut fields, "end_audit_ok")?;
    let rollback_confirmed = take_bool(&mut fields, "rollback_confirmed")?;
    let socket_dropped = take_bool(&mut fields, "socket_dropped")?;
    let original_sha256 = take_hex(&mut fields, "original_sha256")?;
    let executed_sha256 = take_hex(&mut fields, "executed_sha256")?;
    let citation = json::take(&mut fields, "citation").map_err(|()| Fault::source())?;
    check_citation(citation)?;
    let returned_rows = take_u64(&mut fields, "returned_rows")?;
    let result_bytes = take_u64(&mut fields, "result_bytes")?;
    let parsed_rows = take_u64(&mut fields, "parsed_rows")?;
    let parsed_result_bytes = take_u64(&mut fields, "parsed_result_bytes")?;
    let columns = json::take(&mut fields, "columns").map_err(|()| Fault::source())?;
    let width = column_width(columns)?;
    let csv_sha256 = take_hex(&mut fields, "csv_sha256")?;
    let csv_len = take_u64(&mut fields, "csv_bytes")?;
    if source_sha256 != source_sha || csv_sha256 != csv_sha {
        return Err(Fault::hash());
    }
    if source_len != source_bytes.len() as u64 || csv_len != csv_bytes.len() as u64 {
        return Err(Fault::source());
    }
    let (rows, bytes, summary) = parse_source(&source_bytes, turn, width)?;
    if returned_rows != rows.len() as u64
        || parsed_rows != rows.len() as u64
        || result_bytes != bytes
        || parsed_result_bytes != bytes
    {
        return Err(Fault::source());
    }
    let facts = Facts {
        status,
        truncated,
        returned_rows,
        result_bytes,
        rollback_confirmed,
        socket_dropped,
        stop,
        sqlstate,
        original_sha256,
        executed_sha256,
    };
    if summary.facts != facts || summary.end_audit_ok != end_audit_ok || summary.turn != turn {
        return Err(Fault::source());
    }
    let clean = facts.status == "succeeded"
        && !facts.truncated
        && facts.stop.is_none()
        && facts.sqlstate.is_none()
        && end_audit_ok
        && facts.rollback_confirmed
        && !facts.socket_dropped;
    if complete != clean || partial != !complete {
        return Err(Fault::source());
    }
    Ok(Export {
        rows,
        width,
        clean,
        facts,
        source_sha256: source_sha,
        csv_sha256: csv_sha,
        sidecar_sha256: sidecar_sha,
    })
}

struct SummaryLine {
    turn: u64,
    end_audit_ok: bool,
    facts: Facts,
}

fn parse_source(bytes: &[u8], turn: u64, width: usize) -> Result<(Vec<Vec<Cell>>, u64, SummaryLine), Fault> {
    let text = std::str::from_utf8(bytes).map_err(|_| Fault::source())?;
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    if lines.is_empty() || lines.iter().any(|line| line.is_empty()) {
        return Err(Fault::source());
    }
    let summary_line = lines.pop().ok_or(Fault::source())?;
    if lines.len() > MAX_ROWS {
        return Err(Fault::budget());
    }
    let mut rows = Vec::with_capacity(lines.len());
    let mut total = 0_u64;
    for line in lines {
        let (row_turn, row, weight) = parse_row(line)?;
        if row_turn != turn || row.len() != width {
            return Err(Fault::source());
        }
        total = total.checked_add(weight).ok_or(Fault::source())?;
        rows.push(row);
    }
    let summary = parse_summary_line(summary_line)?;
    if summary.turn != turn {
        return Err(Fault::source());
    }
    Ok((rows, total, summary))
}

fn parse_row(line: &str) -> Result<(u64, Vec<Cell>, u64), Fault> {
    let mut fields = line_object(line)?;
    expect_string(&mut fields, "record", "row")?;
    let turn = take_u64(&mut fields, "turn")?;
    let cells = json::take(&mut fields, "cells").map_err(|()| Fault::source())?;
    json::finish(&fields).map_err(|()| Fault::source())?;
    let Json::Array(items) = cells else {
        return Err(Fault::source());
    };
    if items.is_empty() || items.len() > MAX_COLUMNS {
        return Err(Fault::budget());
    }
    let mut row = Vec::with_capacity(items.len());
    let mut weight = 0_u64;
    for item in items {
        let (cell, bytes) = parse_cell(item)?;
        weight = weight.checked_add(bytes).ok_or(Fault::source())?;
        row.push(cell);
    }
    Ok((turn, row, weight))
}

fn parse_cell(value: Json) -> Result<(Cell, u64), Fault> {
    let mut fields = json::object(value).map_err(|()| Fault::source())?;
    let kind = take_string(&mut fields, "kind")?;
    let cell = match kind.as_str() {
        "null" => Cell::Null,
        "bool" => Cell::Bool(take_bool(&mut fields, "value")?),
        "int" => {
            let text = take_string(&mut fields, "value")?;
            if text.len() > MAX_CELL_BYTES || !canonical_i64(&text) {
                return Err(Fault::source());
            }
            Cell::Int(int_decimal(&text)?)
        }
        "text" => {
            let text = take_string(&mut fields, "value")?;
            if text.len() > MAX_CELL_BYTES || text.bytes().any(|byte| byte == 0) {
                return Err(Fault::budget());
            }
            Cell::Text(text)
        }
        "decimal" => {
            let negative = take_bool(&mut fields, "negative")?;
            let coefficient = take_string(&mut fields, "coefficient")?;
            let scale = take_i32(&mut fields, "scale")?;
            if !canonical_decimal(negative, &coefficient, scale) {
                return Err(Fault::source());
            }
            let text = plain_decimal(negative, &coefficient, scale);
            Cell::Decimal(Decimal {
                negative,
                coefficient,
                scale,
                text,
            })
        }
        _ => return Err(Fault::source()),
    };
    json::finish(&fields).map_err(|()| Fault::source())?;
    let weight = match &cell {
        Cell::Null => 0,
        Cell::Bool(true) => 4,
        Cell::Bool(false) => 5,
        Cell::Int(dec) | Cell::Decimal(dec) => dec.text.len() as u64,
        Cell::Text(text) => text.len() as u64,
    };
    Ok((cell, weight))
}

fn parse_summary_line(line: &str) -> Result<SummaryLine, Fault> {
    let mut fields = line_object(line)?;
    expect_string(&mut fields, "record", "summary")?;
    let turn = take_u64(&mut fields, "turn")?;
    let status = take_status(&mut fields)?;
    let truncated = take_bool(&mut fields, "truncated")?;
    let returned_rows = take_u64(&mut fields, "returned_rows")?;
    let result_bytes = take_u64(&mut fields, "result_bytes")?;
    let rollback_confirmed = take_bool(&mut fields, "rollback_confirmed")?;
    let socket_dropped = take_bool(&mut fields, "socket_dropped")?;
    let end_audit_ok = take_bool(&mut fields, "end_audit_ok")?;
    let original_sha256 = take_hex(&mut fields, "original_sha256")?;
    let executed_sha256 = take_hex(&mut fields, "executed_sha256")?;
    let stop = take_stop(&mut fields, "stop")?;
    let sqlstate = take_sqlstate(&mut fields)?;
    json::finish(&fields).map_err(|()| Fault::source())?;
    Ok(SummaryLine {
        turn,
        end_audit_ok,
        facts: Facts {
            status,
            truncated,
            returned_rows,
            result_bytes,
            rollback_confirmed,
            socket_dropped,
            stop,
            sqlstate,
            original_sha256,
            executed_sha256,
        },
    })
}

fn line_object(line: &str) -> Result<Vec<(String, Json)>, Fault> {
    match json::parse_json(line) {
        Ok(value) => json::object(value).map_err(|()| Fault::source()),
        Err(JsonError::Budget) => Err(Fault::budget()),
        Err(JsonError::Syntax) => Err(Fault::source()),
    }
}

fn column_width(value: Json) -> Result<usize, Fault> {
    let Json::Array(items) = value else {
        return Err(Fault::source());
    };
    if items.is_empty() {
        return Err(Fault::source());
    }
    if items.len() > MAX_COLUMNS {
        return Err(Fault::budget());
    }
    for item in &items {
        let _ = json::object(item.clone()).map_err(|()| Fault::source())?;
    }
    Ok(items.len())
}

fn check_citation(value: Json) -> Result<(), Fault> {
    let mut fields = json::object(value).map_err(|()| Fault::source())?;
    expect_bool(&mut fields, "in_source", false)?;
    let query = json::take(&mut fields, "query_id").map_err(|()| Fault::source())?;
    if query != Json::Null {
        return Err(Fault::source());
    }
    json::finish(&fields).map_err(|()| Fault::source())?;
    Ok(())
}

fn take_string(fields: &mut Vec<(String, Json)>, key: &str) -> Result<String, Fault> {
    let value = json::take(fields, key).map_err(|()| Fault::source())?;
    json::as_str(&value).map(str::to_string).map_err(|()| Fault::source())
}

fn expect_string(fields: &mut Vec<(String, Json)>, key: &str, expected: &str) -> Result<(), Fault> {
    if take_string(fields, key)? != expected {
        Err(Fault::source())
    } else {
        Ok(())
    }
}

fn take_bool(fields: &mut Vec<(String, Json)>, key: &str) -> Result<bool, Fault> {
    let value = json::take(fields, key).map_err(|()| Fault::source())?;
    json::as_bool(&value).map_err(|()| Fault::source())
}

fn expect_bool(fields: &mut Vec<(String, Json)>, key: &str, expected: bool) -> Result<(), Fault> {
    if take_bool(fields, key)? != expected {
        Err(Fault::source())
    } else {
        Ok(())
    }
}

fn take_u64(fields: &mut Vec<(String, Json)>, key: &str) -> Result<u64, Fault> {
    let value = json::take(fields, key).map_err(|()| Fault::source())?;
    json::as_u64(&value).map_err(|()| Fault::source())
}

fn take_i32(fields: &mut Vec<(String, Json)>, key: &str) -> Result<i32, Fault> {
    let value = json::take(fields, key).map_err(|()| Fault::source())?;
    json::as_i32(&value).map_err(|()| Fault::source())
}

fn take_hex(fields: &mut Vec<(String, Json)>, key: &str) -> Result<String, Fault> {
    let text = take_string(fields, key)?;
    if is_lower_hex(&text, Some(64)) {
        Ok(text)
    } else {
        Err(Fault::source())
    }
}

fn take_status(fields: &mut Vec<(String, Json)>) -> Result<String, Fault> {
    let status = take_string(fields, "status")?;
    if matches!(status.as_str(), "succeeded" | "partial" | "failed") {
        Ok(status)
    } else {
        Err(Fault::source())
    }
}

fn take_stop(fields: &mut Vec<(String, Json)>, key: &str) -> Result<Option<String>, Fault> {
    let value = json::take(fields, key).map_err(|()| Fault::source())?;
    match value {
        Json::Null => Ok(None),
        Json::String(text)
            if matches!(
                text.as_str(),
                "timeout" | "canceled" | "type_refused" | "database" | "protocol"
            ) =>
        {
            Ok(Some(text))
        }
        _ => Err(Fault::source()),
    }
}

fn take_sqlstate(fields: &mut Vec<(String, Json)>) -> Result<Option<String>, Fault> {
    let value = json::take(fields, "sqlstate").map_err(|()| Fault::source())?;
    match value {
        Json::Null => Ok(None),
        Json::String(text) if sqlstate_ok(&text) => Ok(Some(text)),
        _ => Err(Fault::source()),
    }
}

fn sqlstate_ok(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 5
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
}

pub fn facts_agree(left: &Facts, right: &Facts) -> bool {
    left == right
}
