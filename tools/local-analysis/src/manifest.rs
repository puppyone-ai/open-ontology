use crate::analyze::Analysis;
use crate::fault::Fault;
use crate::json::json_string;
use crate::limits::{MAX_OUTPUT_BYTES, TRANSFORMATION};
use crate::lineage::Lineage;
use crate::sha256::sha256_hex;
use crate::source::Export;

pub struct Input<'a> {
    pub export: &'a Export,
    pub lineage: &'a Lineage,
    pub analysis: &'a Analysis,
    pub svg: &'a [u8],
    pub operation: &'a str,
    pub group_column: usize,
    pub value_column: usize,
    pub engine: &'a str,
    pub duckdb: &'a str,
    pub arrow: &'a str,
    pub parquet: &'a str,
    pub xlsx: &'a str,
    pub cjk_font: &'a str,
    pub warnings: &'a [&'a str],
    pub parquet_sha256: Option<&'a str>,
    pub arrow_sha256: Option<&'a str>,
    pub xlsx_sha256: Option<&'a str>,
    pub font_sha256: Option<&'a str>,
    pub duckdb_binary_sha256: Option<&'a str>,
}

pub fn render(input: &Input<'_>) -> Result<Vec<u8>, Fault> {
    let export = input.export;
    let lineage = input.lineage;
    let analysis = input.analysis;
    let svg = input.svg;
    let operation = input.operation;
    let group_column = input.group_column;
    let value_column = input.value_column;
    let citable = export.clean;
    let warnings = warnings_json(input.warnings);
    let groups = groups_json(analysis);
    let mut out = String::new();
    out.push('{');
    push_str(&mut out, "version", "oo.local-analysis/v1", true);
    push_raw(&mut out, "closes_sp09_w05", "false");
    push_raw(&mut out, "audited", "false");
    push_raw(&mut out, "citable", if citable { "true" } else { "false" });
    push_str(&mut out, "authorization", "not_claimed", false);
    push_raw(&mut out, "authenticated", "false");
    push_str(&mut out, "trust", "local_operator_declared", false);
    push_str(&mut out, "lineage", "bound", false);
    push_str(&mut out, "query_id", &lineage.query_id, false);
    push_str(&mut out, "role", &lineage.role, false);
    push_str(&mut out, "project", &lineage.project, false);
    push_str(&mut out, "engine", input.engine, false);
    push_str(&mut out, "duckdb", input.duckdb, false);
    push_str(&mut out, "arrow", input.arrow, false);
    push_str(&mut out, "parquet", input.parquet, false);
    push_str(&mut out, "xlsx", input.xlsx, false);
    push_str(&mut out, "svg", "rust_native", false);
    push_str(&mut out, "cjk_font", input.cjk_font, false);
    push_str(&mut out, "formula_transformation", TRANSFORMATION, false);
    push_str(&mut out, "source_sha256", &export.source_sha256, false);
    push_str(&mut out, "csv_sha256", &export.csv_sha256, false);
    push_str(&mut out, "sidecar_sha256", &export.sidecar_sha256, false);
    push_str(&mut out, "receipt_sha256", &lineage.receipt_sha256, false);
    push_str(&mut out, "original_sha256", &export.facts.original_sha256, false);
    push_str(&mut out, "executed_sha256", &export.facts.executed_sha256, false);
    push_str(&mut out, "analysis_csv_sha256", &sha256_hex(&analysis.csv), false);
    push_str(&mut out, "svg_sha256", &sha256_hex(svg), false);
    push_nullable(&mut out, "parquet_sha256", input.parquet_sha256);
    push_nullable(&mut out, "arrow_sha256", input.arrow_sha256);
    push_nullable(&mut out, "xlsx_sha256", input.xlsx_sha256);
    push_nullable(&mut out, "font_sha256", input.font_sha256);
    push_nullable(&mut out, "duckdb_binary_sha256", input.duckdb_binary_sha256);
    push_raw(&mut out, "export_complete", if citable { "true" } else { "false" });
    push_raw(
        &mut out,
        "truncated",
        if export.facts.truncated { "true" } else { "false" },
    );
    push_str(&mut out, "operation", operation, false);
    push_raw(&mut out, "group_column", &group_column.to_string());
    push_raw(&mut out, "value_column", &value_column.to_string());
    push_raw(&mut out, "group_count", &analysis.groups.len().to_string());
    push_raw(&mut out, "null_groups", &analysis.null_groups.to_string());
    push_raw(&mut out, "null_values", &analysis.null_values.to_string());
    push_raw(&mut out, "warnings", &warnings);
    push_raw(&mut out, "groups", &groups);
    out.push('}');
    out.push('\n');
    if out.len() > MAX_OUTPUT_BYTES {
        return Err(Fault::budget());
    }
    Ok(out.into_bytes())
}

fn push_str(out: &mut String, key: &str, value: &str, first: bool) {
    if !first {
        out.push(',');
    }
    out.push('"');
    out.push_str(key);
    out.push_str("\":");
    out.push_str(&json_string(value));
}

fn push_raw(out: &mut String, key: &str, value: &str) {
    out.push(',');
    out.push('"');
    out.push_str(key);
    out.push_str("\":");
    out.push_str(value);
}

fn push_nullable(out: &mut String, key: &str, value: Option<&str>) {
    match value {
        Some(text) => push_str(out, key, text, false),
        None => push_raw(out, key, "null"),
    }
}

fn warnings_json(names: &[&str]) -> String {
    let mut out = String::from("[");
    for (index, name) in names.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(name);
        out.push('"');
    }
    out.push(']');
    out
}

fn groups_json(analysis: &Analysis) -> String {
    let mut out = String::from("[");
    for (index, group) in analysis.groups.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"key\":");
        out.push_str(&json_string(&group.key));
        out.push_str(",\"text\":");
        out.push_str(&json_string(&group.value.text));
        out.push_str(",\"negative\":");
        out.push_str(if group.value.negative { "true" } else { "false" });
        out.push_str(",\"coefficient\":");
        out.push_str(&json_string(&group.value.coefficient));
        out.push_str(",\"scale\":");
        out.push_str(&group.value.scale.to_string());
        out.push('}');
    }
    out.push(']');
    out
}
