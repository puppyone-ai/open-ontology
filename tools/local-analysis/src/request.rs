use std::path::Path;

use crate::analyze::Operation;
use crate::fault::Fault;
use crate::json::{self, Json, JsonError};
use crate::limits::{MAX_LABEL_BYTES, MAX_REQUEST_BYTES};
use crate::path;

#[derive(Clone, Debug)]
pub struct Request {
    pub export_dir: String,
    pub lineage_file: String,
    pub output_dir: String,
    pub operation: Operation,
    pub group_column: usize,
    pub value_column: usize,
    pub group_header: String,
    pub value_header: String,
    pub chart_title: String,
    pub cancel_file: Option<String>,
    pub parquet: bool,
    pub xlsx: bool,
    pub duckdb: bool,
}

pub fn load_request(path: &Path) -> Result<Request, Fault> {
    let bytes = path::read_regular(path, MAX_REQUEST_BYTES, Fault::request())?;
    let text = std::str::from_utf8(&bytes).map_err(|_| Fault::request())?;
    let value = match json::parse_json(text) {
        Ok(value) => value,
        Err(JsonError::Budget) => return Err(Fault::budget()),
        Err(JsonError::Syntax) => return Err(Fault::request()),
    };
    let mut fields = json::object(value).map_err(|()| Fault::request())?;
    let version = take_string(&mut fields, "version")?;
    if version != "oo.local-analysis/v1" {
        return Err(Fault::request());
    }
    let export_dir = take_string(&mut fields, "export_dir")?;
    let lineage_file = take_string(&mut fields, "lineage_file")?;
    let output_dir = take_string(&mut fields, "output_dir")?;
    let operation = match take_string(&mut fields, "operation")?.as_str() {
        "sum" => Operation::Sum,
        "count" => Operation::Count,
        _ => return Err(Fault::request()),
    };
    let group_column = take_index(&mut fields, "group_column")?;
    let value_column = take_index(&mut fields, "value_column")?;
    let group_header = take_string(&mut fields, "group_header")?;
    let value_header = take_string(&mut fields, "value_header")?;
    let chart_title = take_string(&mut fields, "chart_title")?;
    label(&group_header)?;
    label(&value_header)?;
    label(&chart_title)?;
    let cancel_file = take_cancel(&mut fields)?;
    let engine = take_string(&mut fields, "engine")?;
    let duckdb = match engine.as_str() {
        "rust_exact" => false,
        "duckdb" => true,
        _ => return Err(Fault::request()),
    };
    let xlsx = take_bool(&mut fields, "xlsx")?;
    let parquet = take_bool(&mut fields, "parquet")?;
    json::finish(&fields).map_err(|()| Fault::request())?;
    Ok(Request {
        export_dir,
        lineage_file,
        output_dir,
        operation,
        group_column,
        value_column,
        group_header,
        value_header,
        chart_title,
        cancel_file,
        parquet,
        xlsx,
        duckdb,
    })
}

fn label(text: &str) -> Result<(), Fault> {
    if text.is_empty() || text.len() > MAX_LABEL_BYTES || text.bytes().any(|byte| byte == 0) {
        Err(Fault::request())
    } else {
        Ok(())
    }
}

fn take_cancel(fields: &mut Vec<(String, Json)>) -> Result<Option<String>, Fault> {
    match json::take(fields, "cancel_file").map_err(|()| Fault::request())? {
        Json::Null => Ok(None),
        Json::String(text) => Ok(Some(text)),
        _ => Err(Fault::request()),
    }
}

fn take_string(fields: &mut Vec<(String, Json)>, key: &str) -> Result<String, Fault> {
    let value = json::take(fields, key).map_err(|()| Fault::request())?;
    json::as_str(&value)
        .map(str::to_string)
        .map_err(|()| Fault::request())
}

fn take_bool(fields: &mut Vec<(String, Json)>, key: &str) -> Result<bool, Fault> {
    let value = json::take(fields, key).map_err(|()| Fault::request())?;
    json::as_bool(&value).map_err(|()| Fault::request())
}

fn take_index(fields: &mut Vec<(String, Json)>, key: &str) -> Result<usize, Fault> {
    let value = json::take(fields, key).map_err(|()| Fault::request())?;
    let number = json::as_u64(&value).map_err(|()| Fault::request())?;
    usize::try_from(number).map_err(|_| Fault::request())
}
