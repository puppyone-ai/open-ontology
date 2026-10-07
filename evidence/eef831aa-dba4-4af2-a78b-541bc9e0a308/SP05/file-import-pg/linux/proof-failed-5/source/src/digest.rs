//! SHA-256 preimages from `file-import-pg/v1`. File bytes are not inputs
//! except for `input_digest` and the payload object line.

use crate::request::{ColumnSpec, ImportRequest};
use crate::{hex_encode, sha256};

pub(crate) fn config_preimage(request: &ImportRequest) -> String {
    let mut lines = Vec::new();
    lines.push("file-import-pg/v1 config".to_string());
    lines.push("strategy=replace".to_string());
    lines.push("parsing_mode=trusted_public_fixture".to_string());
    lines.push(format!("locale={}", request.locale()));
    lines.push("type_policy=preserve_row".to_string());
    lines.push(format!("source={}", request.source()));
    lines.push("source_generation=1".to_string());
    lines.push(format!("namespace={}", request.namespace()));
    lines.push(format!("table={}", request.table()));
    lines.push(format!("format={}", request.format_name()));
    lines.push(format!("profile_id={}", request.profile_id()));
    lines.push(format!("bom={}", request.bom_line()));
    lines.push(format!("header={}", request.header_line()));
    lines.push(format!("null={}", request.null_line()));
    for column in request.columns() {
        lines.push(column_config(column));
    }
    finish(&lines)
}

pub(crate) fn type_preimage(columns: &[ColumnSpec]) -> String {
    let mut lines = vec![
        "file-import-pg/v1 type".to_string(),
        "type_policy=preserve_row".to_string(),
    ];
    for column in columns {
        lines.push(format!(
            "column={} oid={} typmod={} nullable={}",
            column.name(),
            column.oid(),
            column.typmod(),
            bool_word(column.nullable())
        ));
    }
    finish(&lines)
}

pub(crate) fn schema_preimage(namespace: &str, table: &str, columns: &[ColumnSpec]) -> String {
    let mut lines = vec![
        "file-import-pg/v1 schema".to_string(),
        format!("namespace={namespace}"),
        format!("table={table}"),
    ];
    for column in columns {
        lines.push(format!(
            "column={} oid={} typmod={} not_null={} primary_key={}",
            column.name(),
            column.oid(),
            column.typmod(),
            bool_word(!column.nullable()),
            bool_word(column.primary_key())
        ));
    }
    finish(&lines)
}

pub(crate) fn canonical_preimage(hashes: &[String]) -> String {
    let mut lines = vec![
        "file-import-pg/v1 canonical".to_string(),
        format!("count={}", hashes.len()),
    ];
    lines.extend(hashes.iter().cloned());
    finish(&lines)
}

pub(crate) fn payload_preimage(object_sha256: &str, copy_sha256: &str, rows: u64, batch_index: u64) -> String {
    finish(&[
        "file-import-pg/v1 payload".to_string(),
        format!("object_sha256={object_sha256}"),
        format!("copy_sha256={copy_sha256}"),
        format!("rows={rows}"),
        format!("batch_index={batch_index}"),
    ])
}

/// Exact bytes `file-import-pg/v1 hook none` with no trailing newline.
pub(crate) fn hook_digest() -> String {
    hex_encode(&sha256(b"file-import-pg/v1 hook none"))
}

pub(crate) fn digest_text(text: &str) -> String {
    hex_encode(&sha256(text.as_bytes()))
}

fn column_config(column: &ColumnSpec) -> String {
    format!(
        "column={} parse={} pg={} nullable={} primary_key={} precision={} scale={}",
        column.name(),
        column.parse_name(),
        column.pg_name(),
        bool_word(column.nullable()),
        bool_word(column.primary_key()),
        column.precision_word(),
        column.scale_word()
    )
}

fn finish(lines: &[String]) -> String {
    let mut out = String::new();
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn bool_word(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}
