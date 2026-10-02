//! Strict YAML to a typed configuration document.
//!
//! Order: byte and tag scan, version gate, plaintext-secret and `tls: disable`
//! scan, offline JSON Schema, serde, cross-field rules, interpolation audit.
//! A missing or unknown `schema_version` returns only the version diagnostic.

use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::diagnostic::{CONTRACT_CONFIG, Diagnostic, DiagnosticList};
use crate::text::{SecretRef, audit_interpolation, claim_template_pointer};
use crate::yaml::parse_yaml_value;

use super::model::{
    AgentConfig, EvalSuite, FilesFile, HooksFile, OoConfig, RolesFile, SourcesFile,
};
use super::validate::{
    validate_agent, validate_eval, validate_files, validate_hooks, validate_oo, validate_roles,
    validate_sources,
};
use super::version::require_config_version;

pub fn parse_oo_yaml(input: &str) -> Result<OoConfig, DiagnosticList> {
    parse_config(input, validate_oo)
}

pub fn parse_sources_yaml(input: &str) -> Result<SourcesFile, DiagnosticList> {
    parse_config(input, validate_sources)
}

pub fn parse_files_yaml(input: &str) -> Result<FilesFile, DiagnosticList> {
    parse_config(input, validate_files)
}

pub fn parse_roles_yaml(input: &str) -> Result<RolesFile, DiagnosticList> {
    parse_config(input, validate_roles)
}

pub fn parse_agent_yaml(input: &str) -> Result<AgentConfig, DiagnosticList> {
    parse_config(input, validate_agent)
}

pub fn parse_hooks_yaml(input: &str) -> Result<HooksFile, DiagnosticList> {
    parse_config(input, validate_hooks)
}

pub fn parse_eval_suite_yaml(input: &str) -> Result<EvalSuite, DiagnosticList> {
    parse_config(input, validate_eval)
}

fn parse_config<T>(input: &str, validate: fn(&T) -> Vec<Diagnostic>) -> Result<T, DiagnosticList>
where
    T: DeserializeOwned + JsonSchema,
{
    let value = parse_yaml_value(input, CONTRACT_CONFIG)?;
    require_config_version(&value).map_err(DiagnosticList::one)?;
    let mut errors = Vec::new();
    let mut suppressed = Vec::new();
    scan_specials(&value, "", &mut errors, &mut suppressed);
    let schema = schemars::generate::SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<T>()
        .to_value();
    match crate::schema_util::compile_schema(&schema) {
        Ok(validator) => {
            for error in validator.iter_errors(&value) {
                let path = pointer(error.instance_path().to_string());
                if suppressed.iter().any(|item| item == &path) {
                    continue;
                }
                errors.push(Diagnostic::error(
                    CONTRACT_CONFIG,
                    "schema",
                    path,
                    crate::schema_util::scrub_endpoint_text(&error.to_string()),
                ));
            }
        }
        Err(error) => {
            errors.push(Diagnostic::error(
                CONTRACT_CONFIG,
                "schema",
                "/",
                crate::schema_util::scrub_endpoint_text(&format!(
                    "generated schema could not be compiled: {error}"
                )),
            ));
        }
    }
    if !errors.is_empty() {
        audit_interpolation(
            &value,
            "",
            &claim_template_pointer,
            CONTRACT_CONFIG,
            &mut errors,
        );
        return Err(DiagnosticList::new(errors));
    }
    let typed = serde_json::from_value::<T>(value.clone()).map_err(|error| {
        DiagnosticList::one(Diagnostic::error(
            CONTRACT_CONFIG,
            "schema",
            "/",
            crate::schema_util::scrub_endpoint_text(&format!(
                "schema accepted a document serde rejected: {error}"
            )),
        ))
    })?;
    errors.extend(validate(&typed));
    audit_interpolation(
        &value,
        "",
        &claim_template_pointer,
        CONTRACT_CONFIG,
        &mut errors,
    );
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

fn scan_specials(
    value: &Value,
    pointer: &str,
    errors: &mut Vec<Diagnostic>,
    suppressed: &mut Vec<String>,
) {
    let Value::Object(map) = value else {
        if let Value::Array(items) = value {
            for (index, item) in items.iter().enumerate() {
                scan_specials(item, &format!("{pointer}/{index}"), errors, suppressed);
            }
        }
        return;
    };
    if let Some(Value::String(mode)) = map.get("tls")
        && mode == "disable"
    {
        let path = format!("{pointer}/tls");
        errors.push(Diagnostic::error(
            CONTRACT_CONFIG,
            "tls.insecure",
            &path,
            "tls disable is not a production mode",
        ));
        suppressed.push(path);
    }
    if let Some(Value::String(credential)) = map.get("credential")
        && SecretRef::new(credential).is_err()
    {
        let path = format!("{pointer}/credential");
        errors.push(Diagnostic::error(
            CONTRACT_CONFIG,
            "secret.plaintext",
            &path,
            "credential must be a secret:// reference",
        ));
        suppressed.push(path);
    }
    for (key, item) in map {
        scan_specials(item, &format!("{pointer}/{key}"), errors, suppressed);
    }
}
