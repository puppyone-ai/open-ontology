//! Markdown and JSON rendering.
//!
//! JSON is the typed response with null fields removed so the core door can
//! read it again. Integers stay decimal integers. Cell text is not parsed as
//! a float. Markdown escapes control characters and keeps every warning.

use std::fmt::Write;

use oo_core::api::{QueryCell, QueryResponse};
use serde::Serialize;

const RENDER_MAX_BYTES: usize = 16 * 1024 * 1024;

pub enum RenderError {
    Encode,
    Budget,
}

pub fn response_json(response: &QueryResponse) -> Result<Vec<u8>, RenderError> {
    let mut value = serde_json::to_value(response).map_err(|_| RenderError::Encode)?;
    omit_nulls(&mut value);
    let mut bytes = serde_json::to_vec(&value).map_err(|_| RenderError::Encode)?;
    bytes.push(b'\n');
    if bytes.len() > RENDER_MAX_BYTES {
        return Err(RenderError::Budget);
    }
    Ok(bytes)
}

pub fn response_markdown(response: &QueryResponse) -> Result<Vec<u8>, RenderError> {
    let mut lines = Vec::new();
    if !response.columns.is_empty() {
        let mut header = String::from("|");
        for column in &response.columns {
            header.push(' ');
            header.push_str(&escape(column.name().as_str()));
            header.push_str(" |");
        }
        lines.push(header);
        let mut separator = String::from("|");
        for _ in &response.columns {
            separator.push_str(" --- |");
        }
        lines.push(separator);
        for row in &response.rows {
            let mut line = String::from("|");
            for cell in row {
                line.push(' ');
                line.push_str(&cell_text(cell));
                line.push_str(" |");
            }
            lines.push(line);
        }
    }
    lines.push(format!(
        "-- query_id: {}",
        escape(response.query_id.as_str())
    ));
    lines.push(format!("-- returned_rows: {}", response.returned_rows));
    lines.push(format!("-- duration_ms: {}", response.duration_ms));
    lines.push(format!("-- truncated: {}", response.truncated));
    let reasons = response
        .truncation_reasons
        .iter()
        .map(json_scalar)
        .collect::<Vec<_>>()
        .join(", ");
    if reasons.is_empty() {
        lines.push("-- truncation_reasons:".to_owned());
    } else {
        lines.push(format!("-- truncation_reasons: {reasons}"));
    }
    if response.freshness.is_empty() {
        lines.push("-- freshness:".to_owned());
    } else {
        for item in &response.freshness {
            lines.push(format!(
                "-- freshness: table={} as_of={} lag_ms={} state={}",
                escape(item.table.as_str()),
                escape(item.as_of.as_str()),
                item.lag_ms,
                json_scalar(&item.state)
            ));
        }
    }
    if response.sources.is_empty() {
        lines.push("-- sources:".to_owned());
    } else {
        for item in &response.sources {
            lines.push(format!(
                "-- source: source={} consistency={} cross_table_consistent={} applied_pos={}",
                escape(item.source.as_str()),
                json_scalar(&item.consistency),
                item.cross_table_consistent,
                json_scalar(&item.applied_pos)
            ));
        }
    }
    if response.warnings.is_empty() {
        lines.push("-- warnings:".to_owned());
    } else {
        for warning in &response.warnings {
            let mut line = format!("-- warning: code={}", json_scalar(&warning.code));
            if let Some(table) = &warning.table {
                line.push_str(" table=");
                line.push_str(&escape(table.as_str()));
            }
            if let Some(column) = &warning.column {
                line.push_str(" column=");
                line.push_str(&escape(column.as_str()));
            }
            if let Some(impact) = &warning.impact {
                line.push_str(" impact=");
                line.push_str(&escape(impact.as_str()));
            }
            if let Some(rows) = warning.rows {
                line.push_str(&format!(" rows={rows}"));
            }
            line.push_str(" message=");
            line.push_str(&escape(warning.message.as_str()));
            lines.push(line);
        }
    }
    if response.hints.is_empty() {
        lines.push("-- hints:".to_owned());
    } else {
        for hint in &response.hints {
            lines.push(format!("-- hint: {}", escape(hint.as_str())));
        }
    }
    lines.push(format!(
        "-- knowledge_version: {}",
        escape(response.knowledge_version.as_str())
    ));
    lines.push(format!(
        "-- policy_version: {}",
        escape(response.policy_version.as_str())
    ));
    match &response.star_expanded {
        None => lines.push("-- star_expanded: none".to_owned()),
        Some(tables) if tables.is_empty() => lines.push("-- star_expanded:".to_owned()),
        Some(tables) => {
            let names = tables
                .iter()
                .map(|table| escape(table.as_str()))
                .collect::<Vec<_>>()
                .join(" ");
            lines.push(format!("-- star_expanded: {names}"));
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    if out.len() > RENDER_MAX_BYTES {
        return Err(RenderError::Budget);
    }
    Ok(out.into_bytes())
}

fn cell_text(cell: &QueryCell) -> String {
    match cell {
        QueryCell::Null => "null".to_owned(),
        QueryCell::Bool(value) => {
            if *value {
                "true".to_owned()
            } else {
                "false".to_owned()
            }
        }
        QueryCell::Text(text) => escape(text),
    }
}

pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '|' => out.push_str("\\|"),
            '`' => out.push_str("\\`"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if control_or_bidi(other) => {
                let _ = write!(out, "\\u{{{:x}}}", u32::from(other));
            }
            other => out.push(other),
        }
    }
    out
}

fn control_or_bidi(ch: char) -> bool {
    matches!(
        ch,
        '\u{0000}'..='\u{001f}'
            | '\u{007f}'..='\u{009f}'
            | '\u{061c}'
            | '\u{200b}'..='\u{200f}'
            | '\u{2028}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}'
            | '\u{feff}'
            | '\u{fff9}'..='\u{fffb}'
    )
}

fn json_scalar<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(text)) => escape(&text),
        Ok(serde_json::Value::Bool(bit)) => {
            if bit {
                "true".to_owned()
            } else {
                "false".to_owned()
            }
        }
        Ok(serde_json::Value::Number(number)) => number.to_string(),
        _ => "omitted".to_owned(),
    }
}

fn omit_nulls(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            map.retain(|_, child| !child.is_null());
            for child in map.values_mut() {
                omit_nulls(child);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                omit_nulls(item);
            }
        }
        _ => {}
    }
}
