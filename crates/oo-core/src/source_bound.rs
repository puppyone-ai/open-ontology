//! Source-semantic contract derived from a sealed catalog and a source observation.
//!
//! The sealed column `caveat` stays the storage token checked by `caveat_agrees`.
//! This module does not widen that match and does not rewrite `schema_digest`.
//! Column flags come from the dictionary row plus the sealed type. A caller
//! cannot submit the flag strings as the contract.

use std::fmt;

use serde::Deserialize;

use crate::api::payload_sha256;
use crate::sealed_import::{self, SealedImportCatalog, SealedImportColumn};

pub const SOURCE_BOUND_PROTOCOL: &str = "oo.source-semantics.source-bound/v1";
pub const TYPE_DICTIONARY_PROTOCOL: &str = "oo.source-semantics.type-dictionary/v1";
pub const DERIVED_PROTOCOL: &str = "oo.source-semantics.derived/v1";

/// Static failure. Messages do not echo catalog text, banners, or SQL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceBoundError {
    pub code: String,
    pub path: String,
    pub message: &'static str,
}

impl SourceBoundError {
    fn new(name: &'static str, path: impl Into<String>, message: &'static str) -> Self {
        Self {
            code: format!("{DERIVED_PROTOCOL}/{name}"),
            path: path.into(),
            message,
        }
    }
}

impl fmt::Display for SourceBoundError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} at {}: {}", self.code, self.path, self.message)
    }
}

impl std::error::Error for SourceBoundError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedColumn {
    pub dataset: String,
    pub logical_name: String,
    pub caveats: Vec<String>,
    pub native_note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedSourceBound {
    pub identity_hash: String,
    pub engine: String,
    pub source_id: String,
    pub run_id: String,
    pub schema_digest: String,
    pub type_digest: String,
    pub config_digest: String,
    pub input_digest: String,
    pub source_generation: i64,
    pub target_generation: i64,
    pub version: String,
    pub capture: String,
    pub family: String,
    pub product: String,
    pub source_kind: String,
    pub consistency_grade: Option<String>,
    pub delete_detection: Option<String>,
    pub scn: String,
    pub registry_snapshot: String,
    pub columns: Vec<DerivedColumn>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundIn {
    protocol: String,
    engine: String,
    source_id: String,
    source_generation: i64,
    target_generation: i64,
    run_id: String,
    schema_digest: String,
    type_digest: String,
    config_digest: String,
    input_digest: String,
    banner: String,
    scn: String,
    registry_snapshot: String,
    published_scn: String,
    dictionary: DictionaryIn,
    bindings: Vec<BindingIn>,
    #[serde(default)]
    consistency_grade: Option<String>,
    #[serde(default)]
    delete_detection: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DictionaryIn {
    protocol: String,
    columns: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingIn {
    sealed_column: String,
    dictionary_column: String,
}

struct DictColumn {
    name: String,
    native: Native,
    precision: Option<u32>,
    scale: Option<u32>,
    char_length: u32,
}

enum Native {
    Char,
    Varchar2,
    Number,
    Date,
    Raw,
    Timestamp9,
    Timestamp9Tz,
}

/// Derive column semantics from one sealed catalog and one source-bound observation.
///
/// `bound_json` carries dictionary rows and source identity. It has no semantic
/// caveat list. Grade and delete detection are copied only when the observation
/// records them.
pub fn derive_source_bound(
    catalog_json: &str,
    bound_json: &str,
) -> Result<DerivedSourceBound, SourceBoundError> {
    let catalog = sealed_import::parse_sealed_import_catalog(catalog_json).map_err(lift_sealed)?;
    let bound: BoundIn = serde_json::from_str(bound_json).map_err(map_shape)?;
    if bound.protocol != SOURCE_BOUND_PROTOCOL {
        return Err(SourceBoundError::new(
            "protocol.mismatch",
            "/source_bound/protocol",
            "source-bound protocol is not oo.source-semantics.source-bound/v1",
        ));
    }
    if bound.dictionary.protocol != TYPE_DICTIONARY_PROTOCOL {
        return Err(SourceBoundError::new(
            "protocol.mismatch",
            "/source_bound/dictionary/protocol",
            "dictionary protocol is not oo.source-semantics.type-dictionary/v1",
        ));
    }
    match_identity(&catalog, &bound)?;
    let version = release_token(&bound.banner).ok_or_else(|| {
        SourceBoundError::new(
            "observation.missing",
            "/source_bound/banner",
            "banner does not contain one release token",
        )
    })?;
    let grade = optional_grade(bound.consistency_grade.as_deref())?;
    let delete_detection = optional_delete(bound.delete_detection.as_deref())?;
    if catalog.engine != "oracle" {
        return Err(SourceBoundError::new(
            "token.rejected",
            "/source_bound/engine",
            "source-bound derivation accepts an oracle dictionary",
        ));
    }
    if catalog.tables.len() != 1 {
        return Err(SourceBoundError::new(
            "schema.invalid",
            "/tables",
            "source-bound derivation accepts one sealed table",
        ));
    }
    let table = &catalog.tables[0];
    let dictionary = parse_dictionary(&bound.dictionary.columns)?;
    let bindings = bind_columns(&dictionary, &table.columns, &bound.bindings)?;
    let dataset = format!("{}.{}", table.logical_schema, table.logical_table);
    let mut columns = Vec::new();
    for (dict, sealed) in bindings {
        let (caveats, native_note) = derive_column(dict, sealed)?;
        columns.push(DerivedColumn {
            dataset: dataset.clone(),
            logical_name: sealed.logical_name.clone(),
            caveats,
            native_note,
        });
    }
    columns.sort_by(|left, right| left.logical_name.cmp(&right.logical_name));
    let identity_hash = payload_sha256(
        identity_preimage(
            &catalog,
            &bound,
            &version,
            grade.as_deref(),
            delete_detection.as_deref(),
        )
        .as_bytes(),
    );
    Ok(DerivedSourceBound {
        identity_hash,
        engine: catalog.engine,
        source_id: catalog.source_id,
        run_id: catalog.run_id,
        schema_digest: catalog.schema_digest,
        type_digest: catalog.type_digest,
        config_digest: catalog.config_digest,
        input_digest: catalog.input_digest,
        source_generation: catalog.source_generation,
        target_generation: catalog.target_generation,
        version,
        capture: catalog.capture,
        family: "oracle".to_string(),
        product: "oracle".to_string(),
        source_kind: "oracle_table".to_string(),
        consistency_grade: grade,
        delete_detection,
        scn: bound.scn,
        registry_snapshot: bound.registry_snapshot,
        columns,
    })
}

fn lift_sealed(err: sealed_import::SealedImportError) -> SourceBoundError {
    SourceBoundError {
        code: err.code,
        path: err.path,
        message: err.message,
    }
}

fn map_shape(err: serde_json::Error) -> SourceBoundError {
    let text = err.to_string();
    if text.contains("unknown field") {
        SourceBoundError::new(
            "field.unknown",
            "/source_bound",
            "source-bound observation has an unknown field",
        )
    } else if text.contains("missing field") {
        SourceBoundError::new(
            "field.missing",
            "/source_bound",
            "source-bound observation is missing a required field",
        )
    } else {
        SourceBoundError::new(
            "schema.invalid",
            "/source_bound",
            "source-bound observation is not valid JSON",
        )
    }
}

fn match_identity(catalog: &SealedImportCatalog, bound: &BoundIn) -> Result<(), SourceBoundError> {
    if bound.engine != catalog.engine || bound.source_id != catalog.source_id {
        return Err(SourceBoundError::new(
            "cross_source",
            "/source_bound/source_id",
            "source-bound identity is a different source",
        ));
    }
    if bound.source_generation != catalog.source_generation
        || bound.target_generation != catalog.target_generation
    {
        return Err(SourceBoundError::new(
            "cross_source",
            "/source_bound/source_generation",
            "source-bound generation is a different source",
        ));
    }
    if bound.run_id != catalog.run_id {
        return Err(SourceBoundError::new(
            "cross_run",
            "/source_bound/run_id",
            "source-bound run is not the sealed run",
        ));
    }
    if bound.schema_digest != catalog.schema_digest
        || bound.type_digest != catalog.type_digest
        || bound.config_digest != catalog.config_digest
        || bound.input_digest != catalog.input_digest
    {
        return Err(SourceBoundError::new(
            "digest.mismatch",
            "/source_bound/schema_digest",
            "source-bound digest does not match the sealed catalog",
        ));
    }
    if !digits(&bound.scn) || bound.scn.len() > 20 {
        return Err(SourceBoundError::new(
            "schema.invalid",
            "/source_bound/scn",
            "scn is not a decimal token",
        ));
    }
    if bound.published_scn != bound.scn {
        return Err(SourceBoundError::new(
            "fidelity.contradiction",
            "/source_bound/published_scn",
            "published scn does not match the observed scn",
        ));
    }
    if !snapshot_token(&bound.registry_snapshot)
        || !bound.registry_snapshot.ends_with(&format!("-{}", bound.scn))
    {
        return Err(SourceBoundError::new(
            "fidelity.contradiction",
            "/source_bound/registry_snapshot",
            "registry snapshot does not end with the observed scn",
        ));
    }
    if bound.banner.len() > 400 || bound.banner.bytes().any(|byte| byte < 0x20) {
        return Err(SourceBoundError::new(
            "schema.invalid",
            "/source_bound/banner",
            "banner is not a bounded observation string",
        ));
    }
    Ok(())
}

fn digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn snapshot_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && bytes.iter().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-')
        })
}

fn release_token(banner: &str) -> Option<String> {
    let bytes = banner.as_bytes();
    let mut found = None;
    let mut index = 0;
    while index < bytes.len() {
        if !bytes[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        let mut dots = 0;
        while index < bytes.len() && bytes[index] == b'.' {
            let next = index + 1;
            if next >= bytes.len() || !bytes[next].is_ascii_digit() {
                break;
            }
            dots += 1;
            index = next;
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
            }
        }
        if dots >= 3 {
            if found.is_some() {
                return None;
            }
            found = Some(banner[start..index].to_string());
        }
    }
    found
}

fn optional_grade(value: Option<&str>) -> Result<Option<String>, SourceBoundError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if matches!(value, "T" | "S1" | "S0" | "B") {
        Ok(Some(value.to_string()))
    } else {
        Err(SourceBoundError::new(
            "token.rejected",
            "/source_bound/consistency_grade",
            "consistency grade is not T, S1, S0, or B",
        ))
    }
}

fn optional_delete(value: Option<&str>) -> Result<Option<String>, SourceBoundError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if matches!(value, "native" | "partition_replace" | "reconcile" | "none") {
        Ok(Some(value.to_string()))
    } else {
        Err(SourceBoundError::new(
            "token.rejected",
            "/source_bound/delete_detection",
            "delete detection is not a closed observation token",
        ))
    }
}

fn parse_dictionary(lines: &[String]) -> Result<Vec<DictColumn>, SourceBoundError> {
    if lines.is_empty() || lines.len() > 64 {
        return Err(SourceBoundError::new(
            "schema.invalid",
            "/source_bound/dictionary/columns",
            "dictionary column count is outside 1..=64",
        ));
    }
    let mut rows = Vec::new();
    for line in lines {
        let row = parse_line(line)?;
        if rows.iter().any(|item: &DictColumn| item.name == row.name) {
            return Err(SourceBoundError::new(
                "schema.invalid",
                "/source_bound/dictionary/columns",
                "dictionary repeats a column",
            ));
        }
        rows.push(row);
    }
    Ok(rows)
}

fn parse_line(line: &str) -> Result<DictColumn, SourceBoundError> {
    let parts: Vec<&str> = line.split('|').collect();
    if parts.len() != 5 {
        return Err(SourceBoundError::new(
            "token.rejected",
            "/source_bound/dictionary/columns",
            "dictionary row is not a closed type line",
        ));
    }
    if !oracle_name(parts[0]) {
        return Err(SourceBoundError::new(
            "token.rejected",
            "/source_bound/dictionary/columns",
            "dictionary column name is not a closed token",
        ));
    }
    let native = match parts[1] {
        "CHAR" => Native::Char,
        "VARCHAR2" => Native::Varchar2,
        "NUMBER" => Native::Number,
        "DATE" => Native::Date,
        "RAW" => Native::Raw,
        "TIMESTAMP(9)" => Native::Timestamp9,
        "TIMESTAMP(9) WITH TIME ZONE" => Native::Timestamp9Tz,
        _ => {
            return Err(SourceBoundError::new(
                "token.rejected",
                "/source_bound/dictionary/columns",
                "dictionary native type is not a closed token",
            ));
        }
    };
    let precision = optional_number(parts[2])?;
    let scale = optional_number(parts[3])?;
    let char_length = required_number(parts[4])?;
    match native {
        Native::Char | Native::Varchar2 => {
            if precision.is_some() || scale.is_some() || char_length < 1 {
                return Err(SourceBoundError::new(
                    "token.rejected",
                    "/source_bound/dictionary/columns",
                    "character dictionary metadata does not match the native type",
                ));
            }
        }
        Native::Number => {
            if precision.is_some() != scale.is_some() || char_length != 0 {
                return Err(SourceBoundError::new(
                    "token.rejected",
                    "/source_bound/dictionary/columns",
                    "number dictionary metadata is incomplete",
                ));
            }
        }
        Native::Date | Native::Raw => {
            if precision.is_some() || scale.is_some() || char_length != 0 {
                return Err(SourceBoundError::new(
                    "token.rejected",
                    "/source_bound/dictionary/columns",
                    "dictionary metadata does not match the native type",
                ));
            }
        }
        Native::Timestamp9 | Native::Timestamp9Tz => {
            if precision.is_some() || scale != Some(9) || char_length != 0 {
                return Err(SourceBoundError::new(
                    "token.rejected",
                    "/source_bound/dictionary/columns",
                    "timestamp dictionary metadata does not match fractional seconds 9",
                ));
            }
        }
    }
    Ok(DictColumn {
        name: parts[0].to_string(),
        native,
        precision,
        scale,
        char_length,
    })
}

fn oracle_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_uppercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn optional_number(value: &str) -> Result<Option<u32>, SourceBoundError> {
    if value == "-" {
        return Ok(None);
    }
    Ok(Some(required_number(value)?))
}

fn required_number(value: &str) -> Result<u32, SourceBoundError> {
    if digits(value) && value.len() <= 4 {
        value.parse().map_err(|_| {
            SourceBoundError::new(
                "token.rejected",
                "/source_bound/dictionary/columns",
                "dictionary numeric field is not a closed token",
            )
        })
    } else {
        Err(SourceBoundError::new(
            "token.rejected",
            "/source_bound/dictionary/columns",
            "dictionary numeric field is not a closed token",
        ))
    }
}

fn bind_columns<'a>(
    dictionary: &'a [DictColumn],
    sealed: &'a [SealedImportColumn],
    bindings: &[BindingIn],
) -> Result<Vec<(&'a DictColumn, &'a SealedImportColumn)>, SourceBoundError> {
    if bindings.len() != sealed.len() {
        return Err(SourceBoundError::new(
            "column.mismatch",
            "/source_bound/bindings",
            "bindings do not cover the sealed columns",
        ));
    }
    let mut pairs = Vec::new();
    for binding in bindings {
        if pairs
            .iter()
            .any(|(_, sealed): &(&DictColumn, &SealedImportColumn)| {
                sealed.logical_name == binding.sealed_column
            })
        {
            return Err(SourceBoundError::new(
                "column.mismatch",
                "/source_bound/bindings",
                "bindings repeat a sealed column",
            ));
        }
        let Some(dict) = dictionary
            .iter()
            .find(|column| column.name == binding.dictionary_column)
        else {
            return Err(SourceBoundError::new(
                "column.mismatch",
                "/source_bound/bindings",
                "binding names a dictionary column that is absent",
            ));
        };
        if pairs.iter().any(|(item, _): &(&DictColumn, &SealedImportColumn)| {
            item.name == dict.name
        }) {
            return Err(SourceBoundError::new(
                "column.mismatch",
                "/source_bound/bindings",
                "bindings repeat a dictionary column",
            ));
        }
        let Some(column) = sealed
            .iter()
            .find(|column| column.logical_name == binding.sealed_column)
        else {
            return Err(SourceBoundError::new(
                "column.mismatch",
                "/source_bound/bindings",
                "binding names a sealed column that is absent",
            ));
        };
        pairs.push((dict, column));
    }
    Ok(pairs)
}

fn derive_column(
    dict: &DictColumn,
    sealed: &SealedImportColumn,
) -> Result<(Vec<String>, Option<String>), SourceBoundError> {
    if sealed.caveat.is_some() {
        return Err(SourceBoundError::new(
            "fidelity.contradiction",
            "/columns/caveat",
            "storage caveat is not a source semantic",
        ));
    }
    match dict.native {
        Native::Char => {
            if sealed.oid != 1042
                || sealed.source_type.as_deref() != Some("utf8")
                || sealed.typmod != i32::try_from(dict.char_length).unwrap_or(i32::MAX) + 4
            {
                return Err(column_mismatch());
            }
            Ok((vec!["empty_string_is_null".to_string()], None))
        }
        Native::Varchar2 => {
            if sealed.oid != 1043
                || sealed.typmod != -1
                || sealed.source_type.as_deref() != Some("utf8")
            {
                return Err(column_mismatch());
            }
            Ok((vec!["empty_string_is_null".to_string()], None))
        }
        Native::Number if dict.precision.is_none() => {
            if sealed.oid != 1043
                || sealed.typmod != -1
                || sealed.source_type.as_deref() != Some("utf8")
            {
                return Err(column_mismatch());
            }
            Ok((Vec::new(), Some("unconstrained_number".to_string())))
        }
        Native::Number => {
            if sealed.source_type.as_deref() == Some("utf8") {
                return Err(column_mismatch());
            }
            let precision = dict.precision.unwrap_or(0);
            let scale = dict.scale.unwrap_or(0);
            match sealed.oid {
                1700 => {
                    let Some((stored_precision, stored_scale)) =
                        sealed_import::numeric_parts(sealed.typmod)
                    else {
                        return Err(column_mismatch());
                    };
                    if stored_precision != i32::try_from(precision).unwrap_or(i32::MAX)
                        || stored_scale != i32::try_from(scale).unwrap_or(i32::MAX)
                    {
                        return Err(column_mismatch());
                    }
                    Ok((Vec::new(), None))
                }
                21 if scale == 0 && precision <= 4 && sealed.typmod < 0 => Ok((Vec::new(), None)),
                _ => Err(column_mismatch()),
            }
        }
        Native::Date => {
            if sealed.oid != 1114
                || sealed.typmod != -1
                || sealed.source_type.as_deref() != Some("timestamp_micros")
            {
                return Err(column_mismatch());
            }
            Ok((Vec::new(), None))
        }
        Native::Raw => {
            if sealed.oid != 17
                || sealed.typmod != -1
                || sealed.source_type.as_deref() != Some("binary")
            {
                return Err(column_mismatch());
            }
            Ok((Vec::new(), None))
        }
        Native::Timestamp9 | Native::Timestamp9Tz => {
            if sealed.oid != 1043
                || sealed.typmod != -1
                || sealed.source_type.as_deref() != Some("utf8")
            {
                return Err(column_mismatch());
            }
            Ok((vec!["precision_loss".to_string()], None))
        }
    }
}

fn column_mismatch() -> SourceBoundError {
    SourceBoundError::new(
        "column.mismatch",
        "/source_bound/bindings",
        "dictionary type does not match the sealed column",
    )
}

fn identity_preimage(
    catalog: &SealedImportCatalog,
    bound: &BoundIn,
    version: &str,
    grade: Option<&str>,
    delete_detection: Option<&str>,
) -> String {
    let mut bindings = bound
        .bindings
        .iter()
        .map(|binding| format!("{}\t{}", binding.sealed_column, binding.dictionary_column))
        .collect::<Vec<_>>();
    bindings.sort();
    format!(
        "source-bound-identity/v1\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
        catalog.engine,
        catalog.source_id,
        catalog.source_generation,
        catalog.target_generation,
        catalog.run_id,
        catalog.schema_digest,
        catalog.type_digest,
        catalog.config_digest,
        catalog.input_digest,
        bound.scn,
        bound.registry_snapshot,
        version,
        bound.dictionary.columns.join("\n"),
        bindings.join("\n"),
        grade.unwrap_or("-"),
        delete_detection.unwrap_or("-"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::path::PathBuf;

    fn read(relative: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/live-chain")
            .join(relative);
        std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
    }

    fn bound_for(catalog_text: &str, audit_relative: &str, bindings: &[(&str, &str)]) -> String {
        let catalog: Value = serde_json::from_str(catalog_text).unwrap();
        let audit: Value = serde_json::from_str(&read(audit_relative)).unwrap();
        assert_eq!(audit["source_id"], catalog["source_id"]);
        assert_eq!(audit["run_id"], catalog["run_id"]);
        assert_eq!(audit["schema_digest"], catalog["schema_digest"]);
        assert!(audit.get("consistency_grade").is_none());
        assert!(audit.get("delete_detection").is_none());
        assert!(!audit.to_string().contains("\"S1\""));
        let columns = audit["dictionary_columns"].as_array().unwrap();
        serde_json::to_string(&json!({
            "protocol": SOURCE_BOUND_PROTOCOL,
            "engine": catalog["engine"],
            "source_id": catalog["source_id"],
            "source_generation": catalog["source_generation"],
            "target_generation": catalog["target_generation"],
            "run_id": catalog["run_id"],
            "schema_digest": catalog["schema_digest"],
            "type_digest": catalog["type_digest"],
            "config_digest": catalog["config_digest"],
            "input_digest": catalog["input_digest"],
            "banner": audit["banner"],
            "scn": audit["scn"],
            "registry_snapshot": audit["registry_snapshot"],
            "published_scn": audit["published_scn"],
            "dictionary": {"protocol": TYPE_DICTIONARY_PROTOCOL, "columns": columns},
            "bindings": bindings.iter().map(|(sealed, dictionary)| json!({
                "sealed_column": sealed,
                "dictionary_column": dictionary
            })).collect::<Vec<_>>()
        }))
        .unwrap()
    }

    #[test]
    fn real_oracle_dictionaries_derive_three_flags_without_stamped_grade() {
        let batch = read("catalog-batch/catalog.json");
        let derived = derive_source_bound(
            &batch,
            &bound_for(
                &batch,
                "identity-67/batch/audit/readonly-audit.json",
                &[
                    ("id", "ID"),
                    ("n16", "N16"),
                    ("n1", "N1"),
                    ("d", "D"),
                    ("c6", "C6"),
                    ("v2", "V2"),
                    ("r", "R"),
                ],
            ),
        )
        .expect("batch");
        assert_eq!(derived.schema_digest, "59a1d30cfedb66d93fa8277124851ed6fcb3504c697688ddcf92dcadfcc289dc");
        assert_eq!(derived.version, "23.26.3.0.0");
        assert_eq!(derived.capture, "sealed_import");
        assert!(derived.consistency_grade.is_none());
        assert!(derived.delete_detection.is_none());
        let c6 = derived.columns.iter().find(|column| column.logical_name == "c6").unwrap();
        assert_eq!(c6.caveats, vec!["empty_string_is_null".to_string()]);
        assert!(c6.native_note.is_none());
        let v2 = derived.columns.iter().find(|column| column.logical_name == "v2").unwrap();
        assert_eq!(v2.caveats, vec!["empty_string_is_null".to_string()]);
        let date = derived.columns.iter().find(|column| column.logical_name == "d").unwrap();
        assert!(date.caveats.is_empty());
        assert!(date.native_note.is_none());
        assert!(!derived.columns.iter().any(|column| {
            column.native_note.as_deref() == Some("oracle_date_includes_time")
                || column.caveats.iter().any(|token| token == "offset_lost" || token == "time_ambiguous")
        }));

        let wide = read("catalog-wide/catalog.json");
        let derived = derive_source_bound(
            &wide,
            &bound_for(
                &wide,
                "identity-67/wide/audit/readonly-audit.json",
                &[("id", "ID"), ("n_free_text", "N_FREE")],
            ),
        )
        .expect("wide");
        assert_eq!(derived.schema_digest, "71eba817dac14915743a455dc02d55305d1cde2294a0391541bafc1fd347978c");
        let number = derived.columns.iter().find(|column| column.logical_name == "n_free_text").unwrap();
        assert!(number.caveats.is_empty());
        assert_eq!(number.native_note.as_deref(), Some("unconstrained_number"));

        let ts = read("catalog-ts/catalog.json");
        let derived = derive_source_bound(
            &ts,
            &bound_for(
                &ts,
                "identity-67/ts/audit/readonly-audit.json",
                &[("id", "ID"), ("ts_text", "TS"), ("tstz_text", "TSTZ")],
            ),
        )
        .expect("ts");
        assert_eq!(derived.schema_digest, "89ab99d0ae23b23f05435bbbd3818f736c8fb6ff30ab0ff16dbc3da59b8a4a04");
        for name in ["ts_text", "tstz_text"] {
            let column = derived.columns.iter().find(|column| column.logical_name == name).unwrap();
            assert_eq!(column.caveats, vec!["precision_loss".to_string()]);
            assert!(column.native_note.is_none());
        }
    }

    #[test]
    fn cross_source_digest_column_and_token_are_refused() {
        let batch = read("catalog-batch/catalog.json");
        let mut text = bound_for(
            &batch,
            "identity-67/batch/audit/readonly-audit.json",
            &[
                ("id", "ID"),
                ("n16", "N16"),
                ("n1", "N1"),
                ("d", "D"),
                ("c6", "C6"),
                ("v2", "V2"),
                ("r", "R"),
            ],
        );
        let mut value: Value = serde_json::from_str(&text).unwrap();
        value["source_id"] = json!("oo04_t_wide_text");
        let err = derive_source_bound(&batch, &serde_json::to_string(&value).unwrap()).unwrap_err();
        assert!(err.code.ends_with("/cross_source"));

        value = serde_json::from_str(&text).unwrap();
        value["run_id"] = json!("7ac85454-5082-429a-a94c-b3cca4833113");
        let err = derive_source_bound(&batch, &serde_json::to_string(&value).unwrap()).unwrap_err();
        assert!(err.code.ends_with("/cross_run"));

        value = serde_json::from_str(&text).unwrap();
        value["schema_digest"] = json!("71eba817dac14915743a455dc02d55305d1cde2294a0391541bafc1fd347978c");
        let err = derive_source_bound(&batch, &serde_json::to_string(&value).unwrap()).unwrap_err();
        assert!(err.code.ends_with("/digest.mismatch"));

        value = serde_json::from_str(&text).unwrap();
        value["bindings"][4]["sealed_column"] = json!("id");
        value["bindings"][0]["sealed_column"] = json!("c6");
        let err = derive_source_bound(&batch, &serde_json::to_string(&value).unwrap()).unwrap_err();
        assert!(err.code.ends_with("/column.mismatch"));

        value = serde_json::from_str(&text).unwrap();
        value["dictionary"]["columns"][4] = json!("C6|CHAR empty_string_is_null|-|-|6");
        let err = derive_source_bound(&batch, &serde_json::to_string(&value).unwrap()).unwrap_err();
        assert!(err.code.ends_with("/token.rejected"));

        value = serde_json::from_str(&text).unwrap();
        value["caveats"] = json!(["empty_string_is_null"]);
        let err = derive_source_bound(&batch, &serde_json::to_string(&value).unwrap()).unwrap_err();
        assert!(err.code.ends_with("/field.unknown"));

        value = serde_json::from_str(&text).unwrap();
        value["consistency_grade"] = json!("A");
        let err = derive_source_bound(&batch, &serde_json::to_string(&value).unwrap()).unwrap_err();
        assert!(err.code.ends_with("/token.rejected"));

        text = bound_for(
            &batch,
            "identity-67/batch/audit/readonly-audit.json",
            &[
                ("id", "ID"),
                ("n16", "N16"),
                ("n1", "N1"),
                ("d", "D"),
                ("c6", "C6"),
                ("v2", "V2"),
                ("r", "R"),
            ],
        );
        value = serde_json::from_str(&text).unwrap();
        value["consistency_grade"] = json!("S0");
        value["delete_detection"] = json!("none");
        let derived = derive_source_bound(&batch, &serde_json::to_string(&value).unwrap()).unwrap();
        assert_eq!(derived.consistency_grade.as_deref(), Some("S0"));
        assert_eq!(derived.delete_detection.as_deref(), Some("none"));
    }
}
