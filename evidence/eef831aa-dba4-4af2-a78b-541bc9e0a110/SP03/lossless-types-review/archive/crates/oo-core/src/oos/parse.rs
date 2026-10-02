//! Strict YAML for Ontology Spec fragments.
//!
//! `ontology.yaml` is gated on `spec_version`. Every other fragment is gated
//! on `schema_version: oos/v1`. A single file cannot inherit the manifest
//! version. Unknown versions return only the version diagnostic.

use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::config::version::{require_oos_schema_version, require_oos_spec_version};
use crate::diagnostic::{CONTRACT_OOS, Diagnostic, DiagnosticList};
use crate::text::audit_interpolation;
use crate::yaml::parse_yaml_value;

use super::model::{
    CalendarFile, DatasetFile, DecisionsFile, EntityFile, EnumFile, ExamplesFile, GlossaryFile,
    MetricsFile, OntologyFile, RelationshipsFile, ResolutionFile, validate_calendar,
    validate_dataset, validate_decisions, validate_enum, validate_glossary, validate_metrics,
    validate_relationships,
};

pub fn parse_ontology_yaml(input: &str) -> Result<OntologyFile, DiagnosticList> {
    parse_oos(input, require_oos_spec_version, |_| Vec::new())
}

pub fn parse_dataset_yaml(input: &str) -> Result<DatasetFile, DiagnosticList> {
    parse_oos(input, require_oos_schema_version, validate_dataset)
}

pub fn parse_entity_yaml(input: &str) -> Result<EntityFile, DiagnosticList> {
    parse_oos(input, require_oos_schema_version, |_| Vec::new())
}

pub fn parse_relationships_yaml(input: &str) -> Result<RelationshipsFile, DiagnosticList> {
    parse_oos(input, require_oos_schema_version, validate_relationships)
}

pub fn parse_enum_yaml(input: &str) -> Result<EnumFile, DiagnosticList> {
    parse_oos(input, require_oos_schema_version, validate_enum)
}

pub fn parse_metrics_yaml(input: &str) -> Result<MetricsFile, DiagnosticList> {
    parse_oos(input, require_oos_schema_version, validate_metrics)
}

pub fn parse_glossary_yaml(input: &str) -> Result<GlossaryFile, DiagnosticList> {
    parse_oos(input, require_oos_schema_version, validate_glossary)
}

pub fn parse_examples_yaml(input: &str) -> Result<ExamplesFile, DiagnosticList> {
    parse_oos(input, require_oos_schema_version, |_| Vec::new())
}

pub fn parse_calendar_yaml(input: &str) -> Result<CalendarFile, DiagnosticList> {
    parse_oos(input, require_oos_schema_version, validate_calendar)
}

pub fn parse_resolution_yaml(input: &str) -> Result<ResolutionFile, DiagnosticList> {
    parse_oos(input, require_oos_schema_version, |_| Vec::new())
}

pub fn parse_decisions_yaml(input: &str) -> Result<DecisionsFile, DiagnosticList> {
    parse_oos(input, require_oos_schema_version, validate_decisions)
}

fn parse_oos<T>(
    input: &str,
    version: fn(&Value) -> Result<(), Diagnostic>,
    validate: fn(&T) -> Vec<Diagnostic>,
) -> Result<T, DiagnosticList>
where
    T: DeserializeOwned + JsonSchema,
{
    let value = parse_yaml_value(input, CONTRACT_OOS)?;
    version(&value).map_err(DiagnosticList::one)?;
    let mut errors = Vec::new();
    let schema = schemars::generate::SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<T>()
        .to_value();
    match crate::schema_util::compile_schema(&schema) {
        Ok(validator) => {
            for error in validator.iter_errors(&value) {
                let path = pointer(error.instance_path().to_string());
                errors.push(Diagnostic::error(
                    CONTRACT_OOS,
                    "schema",
                    path,
                    crate::schema_util::scrub_endpoint_text(&error.to_string()),
                ));
            }
        }
        Err(error) => {
            errors.push(Diagnostic::error(
                CONTRACT_OOS,
                "schema",
                "/",
                crate::schema_util::scrub_endpoint_text(&format!(
                    "generated schema could not be compiled: {error}"
                )),
            ));
        }
    }
    if !errors.is_empty() {
        audit_interpolation(&value, "", &no_template, CONTRACT_OOS, &mut errors);
        return Err(DiagnosticList::new(errors));
    }
    let typed = serde_json::from_value::<T>(value.clone()).map_err(|error| {
        DiagnosticList::one(Diagnostic::error(
            CONTRACT_OOS,
            "schema",
            "/",
            crate::schema_util::scrub_endpoint_text(&format!(
                "schema accepted a document serde rejected: {error}"
            )),
        ))
    })?;
    errors.extend(validate(&typed));
    audit_interpolation(&value, "", &no_template, CONTRACT_OOS, &mut errors);
    if errors.is_empty() {
        Ok(typed)
    } else {
        Err(DiagnosticList::new(errors))
    }
}

fn pointer(path: String) -> String {
    if path.is_empty() {
        "/".to_owned()
    } else {
        path
    }
}

fn no_template(_: &str) -> bool {
    false
}
