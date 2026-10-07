use crate::fault::Fault;
use crate::json::{self, Json, JsonError};
use crate::limits::MAX_ROWS;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cell {
    Null,
    Bool(bool),
    Int(String),
    Text(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Succeeded,
    Partial,
    Failed,
}

impl Status {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Partial => "partial",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    Timeout,
    Canceled,
    TypeRefused,
    Database,
    Protocol,
}

impl Stop {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Canceled => "canceled",
            Self::TypeRefused => "type_refused",
            Self::Database => "database",
            Self::Protocol => "protocol",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub turn: u32,
    pub status: Status,
    pub truncated: bool,
    pub returned_rows: u32,
    pub result_bytes: u64,
    pub rollback_confirmed: bool,
    pub socket_dropped: bool,
    pub end_audit_ok: bool,
    pub original_sha256: String,
    pub executed_sha256: String,
    pub stop: Option<Stop>,
    pub sqlstate: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parsed {
    pub rows: Vec<Vec<Cell>>,
    pub summary: Summary,
    pub parsed_result_bytes: u64,
}

impl Parsed {
    pub fn counts_match(&self) -> bool {
        self.summary.returned_rows as usize == self.rows.len()
            && self.summary.result_bytes == self.parsed_result_bytes
    }

    pub fn clean_success(&self) -> bool {
        self.summary.status == Status::Succeeded
            && !self.summary.truncated
            && self.summary.stop.is_none()
            && self.summary.sqlstate.is_none()
            && self.summary.end_audit_ok
            && self.summary.rollback_confirmed
            && !self.summary.socket_dropped
            && self.counts_match()
    }
}

pub fn parse_result(source: &[u8]) -> Result<Parsed, Fault> {
    if source.is_empty() || source.contains(&b'\r') {
        return Err(Fault::source());
    }
    let text = std::str::from_utf8(source).map_err(|_| Fault::source())?;
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    if lines.is_empty() || lines.iter().any(|line| line.is_empty()) {
        return Err(Fault::source());
    }
    let mut rows = Vec::new();
    let mut summary: Option<Summary> = None;
    for line in lines {
        let value = json::parse_json(line).map_err(json_source)?;
        let mut fields = expect_object(value)?;
        let record = take_string(&mut fields, "record")?;
        match record.as_str() {
            "row" => {
                if summary.is_some() {
                    return Err(Fault::source());
                }
                if rows.len() == MAX_ROWS {
                    return Err(Fault::budget());
                }
                rows.push(parse_row(&mut fields)?);
            }
            "summary" => {
                if summary.is_some() {
                    return Err(Fault::source());
                }
                summary = Some(parse_summary(&mut fields)?);
            }
            _ => return Err(Fault::source()),
        }
    }
    let summary = summary.ok_or_else(Fault::source)?;
    if rows.iter().any(|row| row.turn != summary.turn) {
        return Err(Fault::source());
    }
    let parsed_result_bytes = rows.iter().try_fold(0_u64, |total, row| {
        row.cells.iter().try_fold(total, |sum, cell| {
            sum.checked_add(cell_bytes(cell)).ok_or_else(Fault::budget)
        })
    })?;
    Ok(Parsed {
        rows: strip_turns(rows),
        summary,
        parsed_result_bytes,
    })
}

struct RowLine {
    turn: u32,
    cells: Vec<Cell>,
}

fn strip_turns(rows: Vec<RowLine>) -> Vec<Vec<Cell>> {
    rows.into_iter().map(|row| row.cells).collect()
}

fn parse_row(fields: &mut Vec<(String, Json)>) -> Result<RowLine, Fault> {
    let turn = take_u32(fields, "turn")?;
    let cells_value = take(fields, "cells")?;
    finish(fields)?;
    let Json::Array(items) = cells_value else {
        return Err(Fault::source());
    };
    if items.len() > crate::limits::MAX_COLUMNS {
        return Err(Fault::budget());
    }
    let mut cells = Vec::with_capacity(items.len());
    for item in items {
        cells.push(parse_cell(item)?);
    }
    Ok(RowLine { turn, cells })
}

fn parse_cell(value: Json) -> Result<Cell, Fault> {
    let mut fields = expect_object(value)?;
    let kind = take_string(&mut fields, "kind")?;
    let cell = match kind.as_str() {
        "null" => Cell::Null,
        "bool" => Cell::Bool(take_bool(&mut fields, "value")?),
        "int" => {
            let text = take_string(&mut fields, "value")?;
            if !canonical_i64(&text) {
                return Err(Fault::source());
            }
            Cell::Int(text)
        }
        "text" => Cell::Text(take_string(&mut fields, "value")?),
        _ => return Err(Fault::source()),
    };
    finish(&fields)?;
    Ok(cell)
}

fn parse_summary(fields: &mut Vec<(String, Json)>) -> Result<Summary, Fault> {
    let turn = take_u32(fields, "turn")?;
    let status = match take_string(fields, "status")?.as_str() {
        "succeeded" => Status::Succeeded,
        "partial" => Status::Partial,
        "failed" => Status::Failed,
        _ => return Err(Fault::source()),
    };
    let truncated = take_bool(fields, "truncated")?;
    let returned_rows = take_u32(fields, "returned_rows")?;
    let result_bytes = take_u64(fields, "result_bytes")?;
    let rollback_confirmed = take_bool(fields, "rollback_confirmed")?;
    let socket_dropped = take_bool(fields, "socket_dropped")?;
    let end_audit_ok = take_bool(fields, "end_audit_ok")?;
    let original_sha256 = take_hash(fields, "original_sha256")?;
    let executed_sha256 = take_hash(fields, "executed_sha256")?;
    let stop = take_stop(fields)?;
    let sqlstate = take_sqlstate(fields)?;
    finish(fields)?;
    Ok(Summary {
        turn,
        status,
        truncated,
        returned_rows,
        result_bytes,
        rollback_confirmed,
        socket_dropped,
        end_audit_ok,
        original_sha256,
        executed_sha256,
        stop,
        sqlstate,
    })
}

pub fn cell_bytes(cell: &Cell) -> u64 {
    match cell {
        Cell::Null => 0,
        Cell::Bool(true) => 4,
        Cell::Bool(false) => 5,
        Cell::Int(text) | Cell::Text(text) => u64::try_from(text.len()).unwrap_or(u64::MAX),
    }
}

pub fn canonical_i64(text: &str) -> bool {
    let Ok(value) = text.parse::<i64>() else {
        return false;
    };
    value.to_string() == text
}

fn expect_object(value: Json) -> Result<Vec<(String, Json)>, Fault> {
    match value {
        Json::Object(fields) => Ok(fields),
        _ => Err(Fault::source()),
    }
}

fn take(fields: &mut Vec<(String, Json)>, key: &str) -> Result<Json, Fault> {
    let Some(index) = fields.iter().position(|(name, _)| name == key) else {
        return Err(Fault::source());
    };
    Ok(fields.remove(index).1)
}

fn take_string(fields: &mut Vec<(String, Json)>, key: &str) -> Result<String, Fault> {
    match take(fields, key)? {
        Json::String(text) => Ok(text),
        _ => Err(Fault::source()),
    }
}

fn take_bool(fields: &mut Vec<(String, Json)>, key: &str) -> Result<bool, Fault> {
    match take(fields, key)? {
        Json::Bool(value) => Ok(value),
        _ => Err(Fault::source()),
    }
}

fn take_u32(fields: &mut Vec<(String, Json)>, key: &str) -> Result<u32, Fault> {
    match take(fields, key)? {
        Json::Number(value) => u32::try_from(value).map_err(|_| Fault::source()),
        _ => Err(Fault::source()),
    }
}

fn take_u64(fields: &mut Vec<(String, Json)>, key: &str) -> Result<u64, Fault> {
    match take(fields, key)? {
        Json::Number(value) => Ok(value),
        _ => Err(Fault::source()),
    }
}

fn take_hash(fields: &mut Vec<(String, Json)>, key: &str) -> Result<String, Fault> {
    let text = take_string(fields, key)?;
    if text.len() == 64
        && text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        Ok(text)
    } else {
        Err(Fault::source())
    }
}

fn take_stop(fields: &mut Vec<(String, Json)>) -> Result<Option<Stop>, Fault> {
    match take(fields, "stop")? {
        Json::Null => Ok(None),
        Json::String(text) => Ok(Some(match text.as_str() {
            "timeout" => Stop::Timeout,
            "canceled" => Stop::Canceled,
            "type_refused" => Stop::TypeRefused,
            "database" => Stop::Database,
            "protocol" => Stop::Protocol,
            _ => return Err(Fault::source()),
        })),
        _ => Err(Fault::source()),
    }
}

fn take_sqlstate(fields: &mut Vec<(String, Json)>) -> Result<Option<String>, Fault> {
    match take(fields, "sqlstate")? {
        Json::Null => Ok(None),
        Json::String(text) => {
            if text.len() == 5
                && text
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
            {
                Ok(Some(text))
            } else {
                Err(Fault::source())
            }
        }
        _ => Err(Fault::source()),
    }
}

fn finish(fields: &[(String, Json)]) -> Result<(), Fault> {
    if fields.is_empty() {
        Ok(())
    } else {
        Err(Fault::source())
    }
}

fn json_source(error: JsonError) -> Fault {
    match error {
        JsonError::Syntax => Fault::source(),
        JsonError::Budget => Fault::budget(),
    }
}

pub fn request_json_error(error: JsonError) -> Fault {
    match error {
        JsonError::Syntax => Fault::request(),
        JsonError::Budget => Fault::budget(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestDoc {
    pub source_file: String,
    pub output_dir: String,
    pub columns: Vec<String>,
}

pub fn parse_request_doc(bytes: &[u8]) -> Result<RequestDoc, Fault> {
    let text = std::str::from_utf8(bytes).map_err(|_| Fault::request())?;
    let value = json::parse_json(text).map_err(request_json_error)?;
    let mut fields = match value {
        Json::Object(fields) => fields,
        _ => return Err(Fault::request()),
    };
    let version = take_string_fault(&mut fields, "version", Fault::request())?;
    if version != "oo.query-result-export/v1" {
        return Err(Fault::request());
    }
    let source_file = take_string_fault(&mut fields, "source_file", Fault::request())?;
    let output_dir = take_string_fault(&mut fields, "output_dir", Fault::request())?;
    let columns_value = take_fault(&mut fields, "columns", Fault::request())?;
    if !fields.is_empty() {
        return Err(Fault::request());
    }
    if source_file.is_empty() || output_dir.is_empty() {
        return Err(Fault::request());
    }
    let Json::Array(items) = columns_value else {
        return Err(Fault::request());
    };
    if items.is_empty() {
        return Err(Fault::request());
    }
    if items.len() > crate::limits::MAX_COLUMNS {
        return Err(Fault::budget());
    }
    let mut columns = Vec::with_capacity(items.len());
    for item in items {
        let Json::String(name) = item else {
            return Err(Fault::request());
        };
        if name.is_empty()
            || name.len() > crate::limits::MAX_COLUMN_NAME_BYTES
            || name.bytes().any(|byte| byte == 0)
        {
            return Err(Fault::request());
        }
        if columns.iter().any(|other| other == &name) {
            return Err(Fault::request());
        }
        columns.push(name);
    }
    Ok(RequestDoc {
        source_file,
        output_dir,
        columns,
    })
}

fn take_fault(fields: &mut Vec<(String, Json)>, key: &str, bad: Fault) -> Result<Json, Fault> {
    let Some(index) = fields.iter().position(|(name, _)| name == key) else {
        return Err(bad);
    };
    Ok(fields.remove(index).1)
}

fn take_string_fault(
    fields: &mut Vec<(String, Json)>,
    key: &str,
    bad: Fault,
) -> Result<String, Fault> {
    match take_fault(fields, key, bad)? {
        Json::String(text) => Ok(text),
        _ => Err(bad),
    }
}
