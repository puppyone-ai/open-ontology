use crate::limits::TRANSFORMATION;
use crate::parse::Parsed;
use crate::sha256::sha256_hex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservedKind {
    None,
    Null,
    Int,
    Bool,
    Text,
    Decimal,
}

impl ObservedKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Null => "null",
            Self::Int => "int",
            Self::Bool => "bool",
            Self::Text => "text",
            Self::Decimal => "decimal",
        }
    }
}

pub struct ColumnStat {
    pub name: String,
    pub kind: ObservedKind,
    pub nulls: u32,
    pub empty_text: u32,
}

pub struct SidecarParts<'a> {
    pub complete: bool,
    pub source: &'a [u8],
    pub csv: &'a [u8],
    pub parsed: &'a Parsed,
    pub columns: &'a [ColumnStat],
    pub formula_cells: &'a [[u32; 2]],
    pub null_cells: &'a [[u32; 2]],
    pub empty_text_cells: &'a [[u32; 2]],
    pub int_cells: &'a [[u32; 2]],
    pub decimal_cells: &'a [[u32; 2]],
}

pub fn sidecar_bytes(parts: &SidecarParts<'_>) -> Result<Vec<u8>, ()> {
    let summary = &parts.parsed.summary;
    let mut out = String::new();
    out.push('{');
    push_key(&mut out, "version");
    push_string(&mut out, "oo.query-result-export/v1");
    push_sep(&mut out);
    push_key(&mut out, "format");
    push_string(&mut out, "csv");
    push_sep(&mut out);
    push_key(&mut out, "xlsx");
    push_string(&mut out, "unsupported");
    push_sep(&mut out);
    push_key(&mut out, "http_auth");
    push_bool(&mut out, false);
    push_sep(&mut out);
    push_key(&mut out, "authenticated");
    push_bool(&mut out, false);
    push_sep(&mut out);
    push_key(&mut out, "complete");
    push_bool(&mut out, parts.complete);
    push_sep(&mut out);
    push_key(&mut out, "partial");
    push_bool(&mut out, !parts.complete);
    push_sep(&mut out);
    push_key(&mut out, "source_sha256");
    push_string(&mut out, &sha256_hex(parts.source));
    push_sep(&mut out);
    push_key(&mut out, "source_bytes");
    push_u64(&mut out, parts.source.len() as u64);
    push_sep(&mut out);
    push_key(&mut out, "turn");
    push_u64(&mut out, u64::from(summary.turn));
    push_sep(&mut out);
    push_key(&mut out, "status");
    push_string(&mut out, summary.status.as_str());
    push_sep(&mut out);
    push_key(&mut out, "truncated");
    push_bool(&mut out, summary.truncated);
    push_sep(&mut out);
    push_key(&mut out, "stop");
    match summary.stop {
        None => out.push_str("null"),
        Some(stop) => push_string(&mut out, stop.as_str()),
    }
    push_sep(&mut out);
    push_key(&mut out, "sqlstate");
    match &summary.sqlstate {
        None => out.push_str("null"),
        Some(text) => push_string(&mut out, text),
    }
    push_sep(&mut out);
    push_key(&mut out, "end_audit_ok");
    push_bool(&mut out, summary.end_audit_ok);
    push_sep(&mut out);
    push_key(&mut out, "rollback_confirmed");
    push_bool(&mut out, summary.rollback_confirmed);
    push_sep(&mut out);
    push_key(&mut out, "socket_dropped");
    push_bool(&mut out, summary.socket_dropped);
    push_sep(&mut out);
    push_key(&mut out, "original_sha256");
    push_string(&mut out, &summary.original_sha256);
    push_sep(&mut out);
    push_key(&mut out, "executed_sha256");
    push_string(&mut out, &summary.executed_sha256);
    push_sep(&mut out);
    push_key(&mut out, "citation");
    out.push_str("{\"in_source\":false,\"query_id\":null}");
    push_sep(&mut out);
    push_key(&mut out, "returned_rows");
    push_u64(&mut out, u64::from(summary.returned_rows));
    push_sep(&mut out);
    push_key(&mut out, "result_bytes");
    push_u64(&mut out, summary.result_bytes);
    push_sep(&mut out);
    push_key(&mut out, "parsed_rows");
    push_u64(&mut out, parts.parsed.rows.len() as u64);
    push_sep(&mut out);
    push_key(&mut out, "parsed_result_bytes");
    push_u64(&mut out, parts.parsed.parsed_result_bytes);
    push_sep(&mut out);
    push_key(&mut out, "formula_transformation");
    push_string(&mut out, TRANSFORMATION);
    push_sep(&mut out);
    push_key(&mut out, "formula_prefixed");
    push_u64(&mut out, parts.formula_cells.len() as u64);
    push_sep(&mut out);
    push_key(&mut out, "formula_cells");
    push_pairs(&mut out, parts.formula_cells);
    push_sep(&mut out);
    push_key(&mut out, "null_count");
    push_u64(&mut out, parts.null_cells.len() as u64);
    push_sep(&mut out);
    push_key(&mut out, "empty_text_count");
    push_u64(&mut out, parts.empty_text_cells.len() as u64);
    push_sep(&mut out);
    push_key(&mut out, "null_cells");
    push_pairs(&mut out, parts.null_cells);
    push_sep(&mut out);
    push_key(&mut out, "empty_text_cells");
    push_pairs(&mut out, parts.empty_text_cells);
    push_sep(&mut out);
    push_key(&mut out, "columns");
    out.push('[');
    for (index, column) in parts.columns.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('{');
        push_key(&mut out, "name");
        push_string(&mut out, &column.name);
        push_sep(&mut out);
        push_key(&mut out, "kind");
        push_string(&mut out, column.kind.as_str());
        push_sep(&mut out);
        push_key(&mut out, "nulls");
        push_u64(&mut out, u64::from(column.nulls));
        push_sep(&mut out);
        push_key(&mut out, "empty_text");
        push_u64(&mut out, u64::from(column.empty_text));
        out.push('}');
    }
    out.push(']');
    push_sep(&mut out);
    push_key(&mut out, "int_count");
    push_u64(&mut out, parts.int_cells.len() as u64);
    push_sep(&mut out);
    push_key(&mut out, "int_cells");
    out.push('[');
    for (index, pair) in parts.int_cells.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let row = usize::try_from(pair[0]).map_err(|_| ())?;
        let column = usize::try_from(pair[1]).map_err(|_| ())?;
        let text = match parts.parsed.rows.get(row).and_then(|cells| cells.get(column)) {
            Some(crate::parse::Cell::Int(text)) => text,
            _ => return Err(()),
        };
        out.push('{');
        push_key(&mut out, "row");
        push_u64(&mut out, u64::from(pair[0]));
        push_sep(&mut out);
        push_key(&mut out, "column");
        push_u64(&mut out, u64::from(pair[1]));
        push_sep(&mut out);
        push_key(&mut out, "text");
        push_string(&mut out, text);
        out.push('}');
    }
    out.push(']');
    push_sep(&mut out);
    push_key(&mut out, "decimal_count");
    push_u64(&mut out, parts.decimal_cells.len() as u64);
    push_sep(&mut out);
    push_key(&mut out, "decimal_cells");
    out.push('[');
    for (index, pair) in parts.decimal_cells.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let row = usize::try_from(pair[0]).map_err(|_| ())?;
        let column = usize::try_from(pair[1]).map_err(|_| ())?;
        let cell = match parts.parsed.rows.get(row).and_then(|cells| cells.get(column)) {
            Some(crate::parse::Cell::Decimal(cell)) => cell,
            _ => return Err(()),
        };
        out.push('{');
        push_key(&mut out, "row");
        push_u64(&mut out, u64::from(pair[0]));
        push_sep(&mut out);
        push_key(&mut out, "column");
        push_u64(&mut out, u64::from(pair[1]));
        push_sep(&mut out);
        push_key(&mut out, "negative");
        push_bool(&mut out, cell.negative);
        push_sep(&mut out);
        push_key(&mut out, "coefficient");
        push_string(&mut out, &cell.coefficient);
        push_sep(&mut out);
        push_key(&mut out, "scale");
        push_i32(&mut out, cell.scale);
        push_sep(&mut out);
        push_key(&mut out, "text");
        push_string(&mut out, &cell.text);
        out.push('}');
    }
    out.push(']');
    push_sep(&mut out);
    push_key(&mut out, "csv_sha256");
    push_string(&mut out, &sha256_hex(parts.csv));
    push_sep(&mut out);
    push_key(&mut out, "csv_bytes");
    push_u64(&mut out, parts.csv.len() as u64);
    out.push('}');
    out.push('\n');
    Ok(out.into_bytes())
}

fn push_pairs(out: &mut String, pairs: &[[u32; 2]]) {
    out.push('[');
    for (index, pair) in pairs.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('[');
        push_u64(out, u64::from(pair[0]));
        out.push(',');
        push_u64(out, u64::from(pair[1]));
        out.push(']');
    }
    out.push(']');
}

fn push_key(out: &mut String, key: &str) {
    push_string(out, key);
    out.push(':');
}

fn push_sep(out: &mut String) {
    out.push(',');
}

fn push_bool(out: &mut String, value: bool) {
    out.push_str(if value { "true" } else { "false" });
}

fn push_u64(out: &mut String, value: u64) {
    out.push_str(&value.to_string());
}

fn push_i32(out: &mut String, value: i32) {
    out.push_str(&value.to_string());
}

fn push_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if other.is_control() => {
                out.push_str(&format!("\\u{code:04x}", code = u32::from(other)));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}
