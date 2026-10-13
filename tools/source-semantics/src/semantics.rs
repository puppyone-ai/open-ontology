//! Source-family facts to semantic_caveats, constraint_trust, and blocks.
//!
//! The captured catalog is a PostgreSQL projection. These facts describe the
//! source. Warehouse products that cannot enforce constraints are rewritten
//! to `declared_only` before the inclusion check. Inclusion uses integer
//! thousandths. 980 is the 0.980 threshold. Live binary sort is never claimed.

use std::collections::BTreeSet;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use oo_core::oos::{
    SourceColumnSemantics, SourceConsistencyGrade, SourceConstraintDisposition, SourceConstraintKind,
    SourceConstraintTrust, SourceConstraintTrustLevel, SourceInclusion, SourceSemantics,
};
use oo_core::text::{PluginName, SourceName, TableName, VersionToken};

use crate::SemanticsError;

pub const INCLUSION_THRESHOLD_THOUSANDTHS: u32 = 980;

const WAREHOUSE_PRODUCTS: &[&str] = &[
    "snowflake",
    "bigquery",
    "redshift",
    "synapse",
    "databricks",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub family: String,
    pub product: String,
    pub version: String,
    pub capture: String,
    /// Closed consistency token. Absent when the command does not claim one.
    #[serde(default)]
    pub source_consistency: Option<String>,
    /// Closed delete-detection token. Absent when the command does not claim one.
    #[serde(default)]
    pub delete_detection: Option<String>,
    pub source_kind: String,
    #[serde(default)]
    pub cursor: Option<CursorFact>,
    pub chunk: ChunkFact,
    #[serde(default)]
    pub flags: Flags,
    #[serde(default)]
    pub columns: Vec<ColumnFact>,
    #[serde(default)]
    pub constraints: Vec<ConstraintFact>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CursorFact {
    pub schema: String,
    pub table: String,
    pub columns: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChunkFact {
    pub key_kind: String,
    #[serde(default)]
    pub binary_order: Option<String>,
    #[serde(default)]
    pub hash_chunking: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Flags {
    #[serde(default)]
    pub synthetic_binlog: bool,
    #[serde(default)]
    pub append_only: bool,
    #[serde(default)]
    pub eventual_consistency: bool,
    #[serde(default)]
    pub default_for_missing: bool,
    #[serde(default)]
    pub accent_insensitive: bool,
    #[serde(default)]
    pub soft_delete_hint: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColumnFact {
    pub schema: String,
    pub table: String,
    pub name: String,
    pub native_type: String,
    #[serde(default)]
    pub empty_string_is_null: bool,
    #[serde(default)]
    pub case_insensitive: bool,
    #[serde(default)]
    pub collation: Option<String>,
    #[serde(default)]
    pub trailing_space_insensitive: bool,
    #[serde(default)]
    pub precision_loss: bool,
    #[serde(default)]
    pub fractional_seconds: Option<u32>,
    #[serde(default)]
    pub offset_lost: bool,
    #[serde(default)]
    pub unconstrained_number: bool,
    #[serde(default)]
    pub lossy_type_fallback: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConstraintFact {
    pub schema: String,
    pub table: String,
    pub name: String,
    pub kind: String,
    pub columns: Vec<String>,
    pub trust: String,
    #[serde(default)]
    pub inclusion: Option<InclusionFact>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InclusionFact {
    pub rows_checked: u64,
    pub ratio_thousandths: u32,
}

#[derive(Debug, Clone)]
pub struct TrustRow {
    pub schema: String,
    pub table: String,
    pub name: String,
    pub kind: String,
    pub columns: Vec<String>,
    pub claimed_trust: String,
    pub trust: String,
    pub disposition: String,
    pub inclusion: Option<(u64, u32)>,
}

/// Facts copied from a source-bound observation after derivation.
#[derive(Debug, Clone)]
pub struct BoundFacts {
    pub family: String,
    pub product: String,
    pub version: String,
    pub capture: String,
    pub grade: Option<String>,
    pub delete_detection: Option<String>,
    pub source_kind: String,
}

pub struct Normalized {
    pub semantic_caveats: Vec<String>,
    pub columns: Vec<Value>,
    pub native_notes: Vec<Value>,
    pub constraint_trust: Vec<TrustRow>,
    pub blocks: Vec<Value>,
    pub chunk: Value,
    pub source_semantics: Value,
    /// Set only after `derive_source_bound` accepts the observation.
    pub bound_facts: Option<BoundFacts>,
    pub source_profile: Option<Value>,
    pub bound_freshness: Option<Value>,
}

pub fn normalize(target: &Target) -> Result<Normalized, SemanticsError> {
    normalize_mode(target, true)
}

/// Same checks as [`normalize`], except consistency and delete detection may be absent.
///
/// Absence is not a grade. A present token is still closed. Column booleans stay
/// claims for the source-bound check; they are not the derived contract.
pub fn normalize_bound(target: &Target) -> Result<Normalized, SemanticsError> {
    normalize_mode(target, false)
}

fn normalize_mode(target: &Target, require_observation: bool) -> Result<Normalized, SemanticsError> {
    require_source_name(&target.family, "/target/family")?;
    require_source_name(&target.product, "/target/product")?;
    require_version(&target.version, "/target/version")?;
    require_source_name(&target.capture, "/target/capture")?;
    let grade = optional_consistency(target.source_consistency.as_deref(), require_observation)?;
    let delete_detection = optional_delete(target.delete_detection.as_deref(), require_observation)?;
    require_source_name(&target.source_kind, "/target/source_kind")?;
    let warehouse = warehouse_product(&target.product);
    let mut column_tokens: Vec<(String, Vec<String>, Option<String>, String)> = Vec::new();
    let mut all_tokens = tokens_from_flags(&target.flags);
    if warehouse {
        all_tokens.push("unenforced_constraints".to_string());
    }
    for column in &target.columns {
        let path = format!("{}.{}", column.schema, column.table);
        let qualified = format!("{path}.{}", column.name);
        let (tokens, note) = column_semantics(target, column)?;
        for token in &tokens {
            push_token(&mut all_tokens, token);
        }
        column_tokens.push((qualified, tokens, note, column.native_type.clone()));
    }
    all_tokens.sort();
    all_tokens.dedup();
    column_tokens.sort_by(|left, right| left.0.cmp(&right.0));

    let mut blocks = Vec::new();
    let mut trust_rows = Vec::new();
    for constraint in &target.constraints {
        let row = trust_of(constraint, warehouse, &target.columns, &mut blocks)?;
        trust_rows.push(row);
    }
    if let Some(cursor) = &target.cursor {
        cursor_block(cursor, &target.columns, &mut blocks)?;
    }
    trust_rows.sort_by(|left, right| {
        (&left.schema, &left.table, &left.name).cmp(&(&right.schema, &right.table, &right.name))
    });
    blocks.sort_by(|left, right| left["code"].as_str().cmp(&right["code"].as_str()));

    let columns_json: Vec<Value> = column_tokens
        .iter()
        .map(|(qualified, tokens, note, native)| {
            json!({
                "caveats": tokens,
                "column": qualified,
                "native_note": note,
                "native_type": native,
            })
        })
        .collect();
    let native_notes: Vec<Value> = column_tokens
        .iter()
        .filter_map(|(qualified, _, note, _)| {
            note.as_ref().map(|text| json!({"column": qualified, "note": text}))
        })
        .collect();
    let chunk = chunk_decision(&target.chunk)?;
    let source_semantics = if let (Some(grade), Some(delete_detection)) = (grade, delete_detection.as_deref()) {
        json!({
            "capture": target.capture,
            "caveats": all_tokens,
            "contract": "native_dataset_field",
            "delete_detection": delete_detection,
            "family": target.family,
            "grade": grade_token(grade),
            "native_type_semantics": native_notes,
            "product": target.product,
            "source_kind": target.source_kind,
            "version": target.version,
        })
    } else {
        let mut object = json!({
            "capture": target.capture,
            "caveats": all_tokens,
            "contract": "native_dataset_field",
            "family": target.family,
            "native_type_semantics": native_notes,
            "product": target.product,
            "source_kind": target.source_kind,
            "version": target.version,
        });
        if let Some(grade) = grade {
            object["grade"] = json!(grade_token(grade));
        }
        if let Some(delete_detection) = delete_detection.as_deref() {
            object["delete_detection"] = json!(delete_detection);
        }
        object
    };
    Ok(Normalized {
        semantic_caveats: all_tokens,
        columns: columns_json,
        native_notes,
        constraint_trust: trust_rows,
        blocks,
        chunk,
        source_semantics,
        bound_facts: None,
        source_profile: None,
        bound_freshness: None,
    })
}

pub fn source_semantics_for(
    dataset: &str,
    target: &Target,
    normalized: &Normalized,
) -> Result<Value, SemanticsError> {
    let (family, product, version, capture, grade, source_kind, delete_detection) =
        if let Some(facts) = &normalized.bound_facts {
            (
                source_name(&facts.family, "/source_bound/engine")?,
                source_name(&facts.product, "/source_bound/engine")?,
                version_token(&facts.version, "/source_bound/banner")?,
                source_name(&facts.capture, "/catalog/capture")?,
                match facts.grade.as_deref() {
                    Some(token) => Some(consistency(token)?),
                    None => None,
                },
                source_name(&facts.source_kind, "/source_bound")?,
                match facts.delete_detection.as_deref() {
                    Some(token) => Some(source_name(token, "/source_bound/delete_detection")?),
                    None => None,
                },
            )
        } else {
            (
                source_name(&target.family, "/target/family")?,
                source_name(&target.product, "/target/product")?,
                version_token(&target.version, "/target/version")?,
                source_name(&target.capture, "/target/capture")?,
                Some(consistency(required_text(
                    target.source_consistency.as_deref(),
                    "/target/source_consistency",
                )?)?),
                source_name(&target.source_kind, "/target/source_kind")?,
                Some(source_name(
                    required_text(target.delete_detection.as_deref(), "/target/delete_detection")?,
                    "/target/delete_detection",
                )?),
            )
        };
    let semantics = SourceSemantics {
        family,
        product,
        version,
        capture,
        grade,
        caveats: table_caveats(&normalized.semantic_caveats)?,
        source_kind,
        delete_detection,
        columns: dataset_column_semantics(dataset, &normalized.columns)?,
        constraint_trust: constraint_semantics(dataset, &normalized.constraint_trust)?,
    };
    serde_json::to_value(&semantics).map_err(|_| {
        SemanticsError::new(
            "schema.invalid",
            format!("/datasets/{dataset}/source_semantics"),
            "source semantics is not JSON",
        )
    })
}

fn table_caveats(tokens: &[String]) -> Result<Vec<PluginName>, SemanticsError> {
    let mut names = BTreeSet::new();
    for token in tokens {
        if is_table_token(token) {
            names.insert(plugin_name(token, "/target")?);
        }
    }
    Ok(names.into_iter().collect())
}

fn dataset_column_semantics(dataset: &str, columns: &[Value]) -> Result<Vec<SourceColumnSemantics>, SemanticsError> {
    let mut rows = Vec::new();
    for column in columns {
        let qualified = column.get("column").and_then(Value::as_str).unwrap_or("");
        if qualified != dataset && !qualified.starts_with(&format!("{dataset}.")) {
            continue;
        }
        let name = qualified.rsplit_once('.').map(|(_, name)| name).unwrap_or(qualified);
        let path = format!("/target/columns/{qualified}");
        let mut caveats = BTreeSet::new();
        if let Some(list) = column.get("caveats").and_then(Value::as_array) {
            for token in list {
                let Some(text) = token.as_str() else {
                    continue;
                };
                caveats.insert(plugin_name(text, &path)?);
            }
        }
        let native_note = match column.get("native_note").and_then(Value::as_str) {
            Some(text) => Some(plugin_name(text, &path)?),
            None => None,
        };
        if caveats.is_empty() && native_note.is_none() {
            continue;
        }
        rows.push(SourceColumnSemantics {
            column: table_name(name, &path)?,
            caveats: caveats.into_iter().collect(),
            native_note,
        });
    }
    rows.sort_by(|left, right| left.column.as_str().cmp(right.column.as_str()));
    Ok(rows)
}

fn constraint_semantics(
    dataset: &str,
    rows: &[TrustRow],
) -> Result<Vec<SourceConstraintTrust>, SemanticsError> {
    let mut kept = Vec::new();
    for row in rows {
        if format!("{}.{}", row.schema, row.table) != dataset {
            continue;
        }
        let path = format!("/target/constraints/{}", row.name);
        let mut columns = Vec::new();
        for name in &row.columns {
            columns.push(table_name(name, &path)?);
        }
        columns.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        kept.push(SourceConstraintTrust {
            name: source_name(&row.name, &path)?,
            kind: constraint_kind(&row.kind, &path)?,
            columns,
            trust: trust_level(&row.trust, &path)?,
            disposition: disposition(&row.disposition, &path)?,
            inclusion: row.inclusion.map(|(rows_checked, ratio_thousandths)| SourceInclusion {
                rows_checked,
                ratio_thousandths,
            }),
        });
    }
    kept.sort_by(|left, right| left.name.as_str().cmp(right.name.as_str()));
    Ok(kept)
}

fn optional_consistency(
    value: Option<&str>,
    required: bool,
) -> Result<Option<SourceConsistencyGrade>, SemanticsError> {
    match value {
        Some(text) => Ok(Some(consistency(text)?)),
        None if required => Err(SemanticsError::new(
            "field.missing",
            "/target/source_consistency",
            "command is missing a required field",
        )),
        None => Ok(None),
    }
}

fn optional_delete(value: Option<&str>, required: bool) -> Result<Option<String>, SemanticsError> {
    match value {
        Some(text) => {
            require_source_name(text, "/target/delete_detection")?;
            Ok(Some(text.to_string()))
        }
        None if required => Err(SemanticsError::new(
            "field.missing",
            "/target/delete_detection",
            "command is missing a required field",
        )),
        None => Ok(None),
    }
}

fn required_text<'a>(value: Option<&'a str>, path: &str) -> Result<&'a str, SemanticsError> {
    value.ok_or_else(|| {
        SemanticsError::new("field.missing", path, "command is missing a required field")
    })
}

fn consistency(value: &str) -> Result<SourceConsistencyGrade, SemanticsError> {
    match value {
        "T" => Ok(SourceConsistencyGrade::T),
        "S1" => Ok(SourceConsistencyGrade::S1),
        "S0" => Ok(SourceConsistencyGrade::S0),
        "B" => Ok(SourceConsistencyGrade::B),
        "A" | "C" | "D" | "E" | "F" => Err(SemanticsError::new(
            "schema.invalid",
            "/target/source_consistency",
            "profile quality grades are not source consistency grades",
        )),
        _ => Err(SemanticsError::new(
            "schema.invalid",
            "/target/source_consistency",
            "source consistency is T, S1, S0, or B",
        )),
    }
}

fn grade_token(grade: SourceConsistencyGrade) -> &'static str {
    match grade {
        SourceConsistencyGrade::T => "T",
        SourceConsistencyGrade::S1 => "S1",
        SourceConsistencyGrade::S0 => "S0",
        SourceConsistencyGrade::B => "B",
    }
}

fn require_source_name(value: &str, path: &str) -> Result<(), SemanticsError> {
    source_name(value, path).map(|_| ())
}

fn require_version(value: &str, path: &str) -> Result<(), SemanticsError> {
    version_token(value, path).map(|_| ())
}

fn source_name(value: &str, path: &str) -> Result<SourceName, SemanticsError> {
    SourceName::new(value.to_string()).map_err(|_| {
        SemanticsError::new(
            "schema.invalid",
            path,
            "value is not a bounded source name",
        )
    })
}

fn version_token(value: &str, path: &str) -> Result<VersionToken, SemanticsError> {
    VersionToken::new(value.to_string()).map_err(|_| {
        SemanticsError::new(
            "schema.invalid",
            path,
            "value is not a bounded version token",
        )
    })
}

fn plugin_name(value: &str, path: &str) -> Result<PluginName, SemanticsError> {
    PluginName::new(value.to_string()).map_err(|_| {
        SemanticsError::new(
            "schema.invalid",
            path,
            "value is not a bounded caveat token",
        )
    })
}

fn table_name(value: &str, path: &str) -> Result<TableName, SemanticsError> {
    TableName::new(value.to_string()).map_err(|_| {
        SemanticsError::new(
            "schema.invalid",
            path,
            "value is not a bounded column name",
        )
    })
}

fn constraint_kind(value: &str, path: &str) -> Result<SourceConstraintKind, SemanticsError> {
    match value {
        "primary_key" => Ok(SourceConstraintKind::PrimaryKey),
        "unique" => Ok(SourceConstraintKind::Unique),
        "foreign_key" => Ok(SourceConstraintKind::ForeignKey),
        _ => Err(SemanticsError::new(
            "schema.invalid",
            path,
            "constraint kind is not primary_key, unique, or foreign_key",
        )),
    }
}

fn trust_level(value: &str, path: &str) -> Result<SourceConstraintTrustLevel, SemanticsError> {
    match value {
        "enforced" => Ok(SourceConstraintTrustLevel::Enforced),
        "declared_only" => Ok(SourceConstraintTrustLevel::DeclaredOnly),
        "absent" => Ok(SourceConstraintTrustLevel::Absent),
        _ => Err(SemanticsError::new(
            "schema.invalid",
            path,
            "constraint trust is not enforced, declared_only, or absent",
        )),
    }
}

fn disposition(value: &str, path: &str) -> Result<SourceConstraintDisposition, SemanticsError> {
    match value {
        "enforced" => Ok(SourceConstraintDisposition::Enforced),
        "candidate" => Ok(SourceConstraintDisposition::Candidate),
        "blocked" => Ok(SourceConstraintDisposition::Blocked),
        "recorded" => Ok(SourceConstraintDisposition::Recorded),
        "stripped" => Ok(SourceConstraintDisposition::Stripped),
        _ => Err(SemanticsError::new(
            "schema.invalid",
            path,
            "constraint disposition is not a known decision",
        )),
    }
}

pub(crate) fn is_table_token(token: &str) -> bool {
    matches!(
        token,
        "synthetic_binlog"
            | "unenforced_constraints"
            | "append_only_source"
            | "eventual_consistency"
            | "default_for_missing"
            | "source_accent_insensitive"
            | "soft_delete_hint"
            | "postgresql_target_storage"
    )
}

fn tokens_from_flags(flags: &Flags) -> Vec<String> {
    let mut tokens = Vec::new();
    if flags.synthetic_binlog {
        tokens.push("synthetic_binlog".to_string());
    }
    if flags.append_only {
        tokens.push("append_only_source".to_string());
    }
    if flags.eventual_consistency {
        tokens.push("eventual_consistency".to_string());
    }
    if flags.default_for_missing {
        tokens.push("default_for_missing".to_string());
    }
    if flags.accent_insensitive {
        tokens.push("source_accent_insensitive".to_string());
    }
    if flags.soft_delete_hint {
        tokens.push("soft_delete_hint".to_string());
    }
    tokens
}

pub(crate) fn column_semantics(
    target: &Target,
    column: &ColumnFact,
) -> Result<(Vec<String>, Option<String>), SemanticsError> {
    let mut tokens = Vec::new();
    if column.empty_string_is_null {
        push_token(&mut tokens, "empty_string_is_null");
    }
    if column.case_insensitive || column.collation.as_deref().is_some_and(collation_is_ci) {
        push_token(&mut tokens, "source_collation_ci");
    }
    if column.trailing_space_insensitive {
        push_token(&mut tokens, "trailing_space_insensitive");
    }
    if column.lossy_type_fallback {
        push_token(&mut tokens, "lossy_type_fallback");
    }
    if precision_loss(column) {
        push_token(&mut tokens, "precision_loss");
    }
    if offset_lost(column) {
        push_token(&mut tokens, "offset_lost");
    }
    let note = native_note(target, column);
    tokens.sort();
    tokens.dedup();
    Ok((tokens, note))
}

fn precision_loss(column: &ColumnFact) -> bool {
    if column.precision_loss {
        return true;
    }
    if column.fractional_seconds.is_some_and(|value| value > 6) {
        return true;
    }
    native_fractional_seconds(&column.native_type).is_some_and(|value| value > 6)
}

fn offset_lost(column: &ColumnFact) -> bool {
    if column.offset_lost {
        return true;
    }
    let folded = fold_type(&column.native_type);
    folded.contains("datetimeoffset")
        || folded.contains("timestamp_tz")
        || folded.contains("withtimezone")
        || folded.contains("timestamptz")
}

fn native_note(target: &Target, column: &ColumnFact) -> Option<String> {
    let folded = fold_type(&column.native_type);
    let oracle = target.family.eq_ignore_ascii_case("oracle")
        || target.product.eq_ignore_ascii_case("oracle");
    if oracle && folded == "date" {
        return Some("oracle_date_includes_time".to_string());
    }
    if column.unconstrained_number || folded == "number" || folded == "number(*,*)" {
        return Some("unconstrained_number".to_string());
    }
    None
}

fn native_fractional_seconds(native: &str) -> Option<u32> {
    let folded = fold_type(native);
    for marker in ["timestamp(", "datetime2(", "datetimeoffset(", "datetime64(", "time("] {
        if let Some(value) = paren_precision(&folded, marker) {
            return Some(value);
        }
    }
    None
}

fn paren_precision(folded: &str, marker: &str) -> Option<u32> {
    let start = folded.find(marker)?;
    let rest = &folded[start + marker.len()..];
    let end = rest.find(')')?;
    if rest[..end].is_empty() || !rest[..end].bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    rest[..end].parse().ok()
}

fn fold_type(native: &str) -> String {
    native
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn collation_is_ci(value: &str) -> bool {
    let folded = value.to_ascii_lowercase();
    folded == "linguistic_ci"
        || folded == "binary_ci"
        || folded == "ci_as"
        || folded.ends_with("_ci")
        || folded.contains("_ci_")
        || folded.starts_with("ci_")
}

fn trust_of(
    constraint: &ConstraintFact,
    warehouse: bool,
    columns: &[ColumnFact],
    blocks: &mut Vec<Value>,
) -> Result<TrustRow, SemanticsError> {
    if !matches!(constraint.kind.as_str(), "primary_key" | "unique" | "foreign_key") {
        return Err(SemanticsError::new(
            "schema.invalid",
            format!("/target/constraints/{}", constraint.name),
            "constraint kind is not primary_key, unique, or foreign_key",
        ));
    }
    if !matches!(constraint.trust.as_str(), "enforced" | "declared_only" | "absent") {
        return Err(SemanticsError::new(
            "schema.invalid",
            format!("/target/constraints/{}", constraint.name),
            "constraint trust is not enforced, declared_only, or absent",
        ));
    }
    let claimed = constraint.trust.clone();
    let mut trust = claimed.clone();
    if warehouse && trust == "enforced" {
        trust = "declared_only".to_string();
    }
    let path = format!("/target/constraints/{}", constraint.name);
    if matches!(constraint.kind.as_str(), "primary_key" | "unique")
        && key_is_precision_lossy(
            &constraint.schema,
            &constraint.table,
            &constraint.columns,
            columns,
        )
    {
        blocks.push(block(
            "precision_loss_key",
            &path,
            "a precision-loss column is still the key",
        ));
    }
    let mut disposition = if trust == "enforced" {
        "enforced"
    } else {
        "recorded"
    };
    if constraint.kind == "foreign_key" && trust == "declared_only" {
        match &constraint.inclusion {
            None => {
                disposition = "blocked";
                blocks.push(block(
                    "declared_only_fk_untested",
                    &path,
                    "declared_only foreign key has no inclusion test",
                ));
            }
            Some(inclusion) => {
                if inclusion.ratio_thousandths > 1000 {
                    return Err(SemanticsError::new(
                        "schema.invalid",
                        path,
                        "ratio_thousandths is above 1000",
                    ));
                }
                if inclusion.rows_checked == 0 {
                    disposition = "blocked";
                    blocks.push(block(
                        "declared_only_fk_empty_sample",
                        &path,
                        "inclusion test checked zero rows",
                    ));
                } else if inclusion.ratio_thousandths < INCLUSION_THRESHOLD_THOUSANDTHS {
                    disposition = "blocked";
                    blocks.push(block(
                        "declared_only_fk_below_threshold",
                        &path,
                        "inclusion ratio is below 980 thousandths",
                    ));
                } else {
                    disposition = "candidate";
                }
            }
        }
    }
    if constraint.kind == "foreign_key" && trust == "absent" {
        disposition = "stripped";
    }
    Ok(TrustRow {
        schema: constraint.schema.clone(),
        table: constraint.table.clone(),
        name: constraint.name.clone(),
        kind: constraint.kind.clone(),
        columns: constraint.columns.clone(),
        claimed_trust: claimed,
        trust,
        disposition: disposition.to_string(),
        inclusion: constraint
            .inclusion
            .as_ref()
            .map(|item| (item.rows_checked, item.ratio_thousandths)),
    })
}

fn cursor_block(
    cursor: &CursorFact,
    columns: &[ColumnFact],
    blocks: &mut Vec<Value>,
) -> Result<(), SemanticsError> {
    if cursor.columns.is_empty() {
        return Err(SemanticsError::new(
            "schema.invalid",
            "/target/cursor/columns",
            "cursor columns are empty",
        ));
    }
    if key_is_precision_lossy(&cursor.schema, &cursor.table, &cursor.columns, columns) {
        blocks.push(block(
            "precision_loss_cursor",
            "/target/cursor",
            "a precision-loss column is still the cursor",
        ));
    }
    Ok(())
}

fn key_is_precision_lossy(
    schema: &str,
    table: &str,
    key_columns: &[String],
    columns: &[ColumnFact],
) -> bool {
    key_columns.iter().any(|name| {
        columns.iter().any(|column| {
            column.schema == schema
                && column.table == table
                && column.name == *name
                && precision_loss(column)
        })
    })
}

fn chunk_decision(chunk: &ChunkFact) -> Result<Value, SemanticsError> {
    let kind = chunk.key_kind.as_str();
    if !matches!(kind, "integer" | "number" | "string" | "binary") {
        return Err(SemanticsError::new(
            "schema.invalid",
            "/target/chunk/key_kind",
            "key_kind is not integer, number, string, or binary",
        ));
    }
    let (decision, reason) = if matches!(kind, "integer" | "number") {
        ("not_applicable", "numeric_key")
    } else {
        match chunk.binary_order.as_deref() {
            Some("forced") => ("binary", "forced_binary_order"),
            Some("unavailable") if chunk.hash_chunking => ("hash", "binary_order_unavailable"),
            Some("unavailable") => ("read_keys", "binary_order_unavailable"),
            Some("index_unusable") if chunk.hash_chunking => ("hash", "index_unusable"),
            Some("index_unusable") => ("read_keys", "index_unusable"),
            None => ("read_keys", "omitted_does_not_claim_binary"),
            Some(_) => {
                return Err(SemanticsError::new(
                    "schema.invalid",
                    "/target/chunk/binary_order",
                    "binary_order is not forced, unavailable, or index_unusable",
                ));
            }
        }
    };
    Ok(json!({
        "binary_order": chunk.binary_order,
        "decision": decision,
        "hash_chunking": chunk.hash_chunking,
        "key_kind": kind,
        "live_binary_sort_proof": "not_run",
        "reason": reason,
    }))
}

fn warehouse_product(product: &str) -> bool {
    WAREHOUSE_PRODUCTS
        .iter()
        .any(|name| product.eq_ignore_ascii_case(name))
}

fn block(code: &str, path: &str, detail: &str) -> Value {
    json!({"code": code, "detail": detail, "path": path})
}

fn push_token(tokens: &mut Vec<String>, token: &str) {
    if !tokens.iter().any(|item| item == token) {
        tokens.push(token.to_string());
    }
}

pub fn trust_json(rows: &[TrustRow]) -> Vec<Value> {
    rows.iter()
        .map(|row| {
            let mut object = Map::new();
            object.insert("claimed_trust".to_string(), json!(row.claimed_trust));
            object.insert("columns".to_string(), json!(row.columns));
            object.insert("disposition".to_string(), json!(row.disposition));
            object.insert(
                "inclusion".to_string(),
                match row.inclusion {
                    Some((rows_checked, ratio)) => {
                        json!({"ratio_thousandths": ratio, "rows_checked": rows_checked})
                    }
                    None => Value::Null,
                },
            );
            object.insert("kind".to_string(), json!(row.kind));
            object.insert("name".to_string(), json!(row.name));
            object.insert("schema".to_string(), json!(row.schema));
            object.insert("table".to_string(), json!(row.table));
            object.insert("trust".to_string(), json!(row.trust));
            Value::Object(object)
        })
        .collect()
}

pub fn enforced_fk(rows: &[TrustRow], dataset: &str, name: &str, columns: &[String]) -> bool {
    rows.iter().any(|row| {
        row.kind == "foreign_key"
            && row.trust == "enforced"
            && (row.name == name
                || (format!("{}.{}", row.schema, row.table) == dataset && row.columns == columns))
    })
}

#[cfg(test)]
mod tests {
    use super::INCLUSION_THRESHOLD_THOUSANDTHS;

    #[test]
    fn unit_threshold_constant_is_not_capability_acceptance() {
        assert_eq!(INCLUSION_THRESHOLD_THOUSANDTHS, 980);
    }
}
