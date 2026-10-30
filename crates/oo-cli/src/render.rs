//! Markdown and JSON rendering.
//!
//! JSON is the typed response with null fields removed so the core door can
//! read it again. Integers stay decimal integers. Cell text is not parsed as
//! a float. Markdown escapes control characters and keeps every warning.

use std::fmt::Write;

use oo_core::api::{QueryCell, QueryResponse};
use serde::Serialize;

const RENDER_MAX_BYTES: usize = 16 * 1024 * 1024;

pub fn with_publication(
    mut bytes: Vec<u8>, json: bool, binding: Option<&oo_sdk::PublicationReceipt>,
) -> Result<Vec<u8>, RenderError> {
    if let Some(binding) = binding {
        if json {
            let mut value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| RenderError::Encode)?;
            value["publication_binding"] = serde_json::to_value(binding).map_err(|_| RenderError::Encode)?;
            bytes = serde_json::to_vec(&value).map_err(|_| RenderError::Encode)?;
            bytes.push(b'\n');
        } else {
            bytes.extend_from_slice(format!("\n-- publication_id: {}\n-- data_vector_sha256: {}\n",
                binding.publication_id, binding.data_vector_sha256).as_bytes());
        }
    }
    if bytes.len() > RENDER_MAX_BYTES { return Err(RenderError::Budget); }
    Ok(bytes)
}

pub fn freshness_summary(value: &serde_json::Value) -> serde_json::Value {
    let mut summary = value.clone();
    let Some(object) = summary.as_object_mut() else {
        return summary;
    };
    let mut omitted = serde_json::Map::new();
    // Evidence arrays can exceed an Agent's context budget for a whole library.
    // Keep warnings and all unknown fields; the unabridged command remains unchanged.
    for key in ["imports", "library_publications", "mappings", "receipts", "runs",
                "source_watermarks", "sources", "table_quality", "target_observations", "freshness"] {
        if let Some(serde_json::Value::Array(items)) = object.get(key) {
            let mut states = std::collections::BTreeMap::<String, usize>::new();
            for item in items {
                if let Some(state) = item.get("state").and_then(serde_json::Value::as_str) {
                    *states.entry(state.to_owned()).or_default() += 1;
                }
            }
            omitted.insert(key.to_owned(), serde_json::json!({
                "count": items.len(), "state_counts": states
            }));
            object.remove(key);
        }
    }
    object.insert("evidence_summary".into(), serde_json::json!({
        "details_omitted": true, "arrays": omitted,
        "detail_command": "oo freshness (without --summary)",
        "scope": "role-wide; not proof of source recency or cross-table consistency"
    }));
    summary
}

#[cfg(test)]
mod summary_tests {
    use super::freshness_summary;
    use serde_json::json;

    #[test]
    fn summary_preserves_warnings_versions_unknowns_and_marks_omissions() {
        let input = json!({
            "source_time": "unmeasured", "knowledge_version": 7, "policy_version": 3,
            "warnings": [{"code":"STALE_DATA", "table":"app.items"},
                         {"code":"CROSS_TABLE_SKEW"}],
            "source_ids": ["source_a"], "future_field": {"keep":true},
            "target_observations":[{"state":"ready"},{"state":"stale"}],
            "receipts": [{"target_commit_at":"2026-10-20T00:00:00Z"}]
        });
        let result = freshness_summary(&input);
        for key in ["source_time", "knowledge_version", "policy_version", "warnings",
                    "source_ids", "future_field"] {
            assert_eq!(result[key], input[key]);
        }
        assert!(result.get("receipts").is_none());
        assert_eq!(result["evidence_summary"]["arrays"]["receipts"]["count"], 1);
        assert_eq!(result["evidence_summary"]["arrays"]["target_observations"]["state_counts"]["stale"], 1);
        assert_eq!(result["evidence_summary"]["details_omitted"], true);
        assert!(input.get("receipts").is_some());
    }

    #[test]
    fn unexpected_shapes_are_not_silently_discarded() {
        let input = json!({"receipts":{"error":"unexpected"}, "warnings":[]});
        assert_eq!(freshness_summary(&input)["receipts"], input["receipts"]);
        assert_eq!(freshness_summary(&json!(null)), json!(null));
    }
}

pub enum RenderError {
    Encode,
    Budget,
}

pub fn describe_markdown(value: &serde_json::Value) -> Result<Vec<u8>, RenderError> {
    let object = value.as_object().ok_or(RenderError::Encode)?;
    let table = object.get("table").and_then(serde_json::Value::as_str)
        .ok_or(RenderError::Encode)?;
    let columns = object.get("columns").and_then(serde_json::Value::as_array)
        .ok_or(RenderError::Encode)?;
    let details = object.get("column_details").and_then(serde_json::Value::as_array);
    let mut out = format!("# {}\n\n{} visible columns (all listed)\n\n| Column | Type |\n| --- | --- |\n",
        escape(table), columns.len());
    for column in columns {
        let name = column.as_str().ok_or(RenderError::Encode)?;
        let detail = details.and_then(|items| items.iter().find(|item| item["name"] == name));
        let kind = detail.and_then(|item| item["type"].as_str()).unwrap_or("unavailable");
        let _ = writeln!(out, "| {} | {} |", escape(name), escape(kind));
    }
    if let Some(caps) = object.get("query_capabilities") {
        let status = caps.get("status").and_then(serde_json::Value::as_str).unwrap_or("unavailable");
        let _ = writeln!(out, "\nQuery capabilities: {}", escape(status));
        if let Some(functions) = caps.get("functions").and_then(serde_json::Value::as_array) {
            let signatures = functions.iter().map(|function| {
                let name = function["name"].as_str().unwrap_or("unavailable");
                let args = if function["aggregate_star"] == true {
                    "*".to_owned()
                } else {
                    function["argument_types"].as_array().map(|args| {
                        args.iter().map(|arg| arg.as_str().unwrap_or("unavailable"))
                            .collect::<Vec<_>>().join(", ")
                    }).unwrap_or_else(|| "unavailable".to_owned())
                };
                escape(&format!("{name}({args})"))
            }).collect::<Vec<_>>();
            let _ = writeln!(out, "Functions: {}", signatures.join(", "));
        }
        if let Some(operators) = caps.get("operators").and_then(serde_json::Value::as_array) {
            let names = operators.iter().filter_map(|item| item["name"].as_str())
                .collect::<std::collections::BTreeSet<_>>();
            let _ = writeln!(out, "Operators: {} ({} typed signatures; see --json)",
                names.into_iter().map(escape).collect::<Vec<_>>().join(", "), operators.len());
        }
        // Only the repetitive overload lists are summarized; keep capability caveats.
        for (key, item) in caps.as_object().into_iter().flatten() {
            if !matches!(key.as_str(), "status" | "functions" | "operators") {
                let _ = writeln!(out, "{}: {}", escape(key), escape(&item.to_string()));
            }
        }
    }
    for (key, item) in object {
        if !matches!(key.as_str(), "table" | "columns" | "column_details" | "query_capabilities") {
            let _ = writeln!(out, "\n{}: {}", escape(key), escape(&item.to_string()));
        }
    }
    out.push_str("\nFull metadata and exact signatures: oo describe --json <schema.table>\n");
    out.push_str("Check the complete SQL before execution: oo validate \"<sql>\"\n");
    if out.len() > RENDER_MAX_BYTES { return Err(RenderError::Budget); }
    Ok(out.into_bytes())
}

#[cfg(test)]
mod describe_tests {
    use super::describe_markdown;
    use serde_json::json;

    #[test]
    fn describe_keeps_every_column_warning_and_capability_caveat() {
        let input = json!({
            "table": "sales.payments", "columns": ["id", "amount", "no_type"],
            "column_details": [{"name":"amount", "type":"numeric"},{"name":"id","type":"int8"}],
            "warnings": [{"code":"CROSS_TABLE_SKEW"}], "future_field": {"keep":true},
            "query_capabilities": {
                "status":"available", "truncated":false, "requires_validate":true,
                "functions":[{"name":"sum","argument_types":["numeric"],"aggregate_star":false},
                             {"name":"count","argument_types":[],"aggregate_star":true}],
                "operators":[{"name":"=","left_type":"int8"},{"name":"=","left_type":"numeric"}]
            }
        });
        let out = String::from_utf8(describe_markdown(&input).ok().unwrap()).unwrap();
        for text in ["3 visible columns", "| id | int8 |", "| amount | numeric |",
                     "| no_type | unavailable |", "CROSS_TABLE_SKEW", "future_field",
                     "sum(numeric), count(*)", "2 typed signatures", "requires_validate: true",
                     "truncated: false", "oo describe --json", "oo validate"] {
            assert!(out.contains(text), "{text}: {out}");
        }
        assert!(!out.contains("left_type"));
    }

    #[test]
    fn describe_escapes_terminal_controls_and_does_not_claim_unavailable_capabilities() {
        let input = json!({"table":"sales.payments", "columns":["id"],
            "query_capabilities":{"status":"unavailable","reason":"catalog\u{1b}[31m\nmissing"}});
        let out = String::from_utf8(describe_markdown(&input).ok().unwrap()).unwrap();
        assert!(!out.contains('\u{1b}'));
        assert!(out.contains("unavailable"));
        assert!(out.contains("reason"));
        assert!(!out.contains("Functions:"));
        assert!(describe_markdown(&json!({"table":"sales.payments"})).is_err());
    }
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
    if response.truncated {
        lines.push("WARNING: Incomplete result (truncated). Do not treat these rows as a complete list or compute a global ranking from this subset.".to_owned());
        lines.push(String::new());
    }
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
