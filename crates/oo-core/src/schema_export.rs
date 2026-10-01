//! Checked-in JSON Schema and OpenAPI, generated from the Rust types.
//!
//! JSON Schema draft 2020-12 is compiled with the network and file resolvers
//! disabled. The OpenAPI document lists component schemas only. It has no
//! operations and does not describe a running server.

use schemars::JsonSchema;
use serde_json::{Map, Value};
use utoipa::OpenApi;

use crate::api::{
    AcceptedOperation, ErrorEnvelope, ExportCompletion, PageRequest, QueryRequest, QueryResponse,
};
use crate::batch::BatchMeta;
use crate::config::model::{
    AgentConfig, EvalSuite, FilesFile, HooksFile, OoConfig, RolesFile, SourcesFile,
};
use crate::oos::{
    CalendarFile, DatasetFile, DecisionsFile, EntityFile, EnumFile, ExamplesFile, GlossaryFile,
    MetricsFile, OntologyFile, RelationshipsFile, ResolutionFile,
};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Open Ontology contracts",
        version = "api/v1",
        description = "SP00 component schemas only. This document has no operations. It is not a running server, and a schema match is not evidence of HTTP behavior."
    ),
    components(schemas(
        ErrorEnvelope,
        QueryRequest,
        QueryResponse,
        AcceptedOperation,
        PageRequest,
        ExportCompletion
    ))
)]
struct ContractApi;

pub fn contract_documents() -> Result<Vec<(&'static str, Value)>, String> {
    Ok(vec![
        (
            "config-v1.json",
            bundle(
                "https://openontology.dev/contracts/config/v1",
                "Open Ontology configuration v1",
                vec![
                    ("OoConfig", root_schema::<OoConfig>()?),
                    ("SourcesFile", root_schema::<SourcesFile>()?),
                    ("FilesFile", root_schema::<FilesFile>()?),
                    ("RolesFile", root_schema::<RolesFile>()?),
                    ("AgentConfig", root_schema::<AgentConfig>()?),
                    ("HooksFile", root_schema::<HooksFile>()?),
                    ("EvalSuite", root_schema::<EvalSuite>()?),
                ],
            )?,
        ),
        (
            "oos-v1.json",
            bundle(
                "https://openontology.dev/contracts/oos/v1",
                "Ontology Spec fragments v1",
                vec![
                    ("OntologyFile", root_schema::<OntologyFile>()?),
                    ("DatasetFile", root_schema::<DatasetFile>()?),
                    ("EntityFile", root_schema::<EntityFile>()?),
                    ("RelationshipsFile", root_schema::<RelationshipsFile>()?),
                    ("EnumFile", root_schema::<EnumFile>()?),
                    ("MetricsFile", root_schema::<MetricsFile>()?),
                    ("GlossaryFile", root_schema::<GlossaryFile>()?),
                    ("ExamplesFile", root_schema::<ExamplesFile>()?),
                    ("CalendarFile", root_schema::<CalendarFile>()?),
                    ("ResolutionFile", root_schema::<ResolutionFile>()?),
                    ("DecisionsFile", root_schema::<DecisionsFile>()?),
                ],
            )?,
        ),
        (
            "batch-v1.json",
            stamp(
                "https://openontology.dev/contracts/batch/v1",
                "Batch metadata v1",
                root_schema::<BatchMeta>()?,
            )?,
        ),
        (
            "api-v1.json",
            bundle(
                "https://openontology.dev/contracts/api/v1",
                "API JSON bodies v1",
                vec![
                    ("QueryRequest", root_schema::<QueryRequest>()?),
                    ("QueryResponse", root_schema::<QueryResponse>()?),
                    ("AcceptedOperation", root_schema::<AcceptedOperation>()?),
                    ("PageRequest", root_schema::<PageRequest>()?),
                    ("ExportCompletion", root_schema::<ExportCompletion>()?),
                ],
            )?,
        ),
        (
            "error-v1.json",
            stamp(
                "https://openontology.dev/contracts/error/v1",
                "API error envelope v1",
                root_schema::<ErrorEnvelope>()?,
            )?,
        ),
        ("openapi-v1.json", openapi_document()?),
    ])
}

pub fn to_pretty(value: &Value) -> String {
    let mut text = serde_json::to_string_pretty(value).expect("schema value is JSON");
    text.push('\n');
    text
}

fn root_schema<T: JsonSchema>() -> Result<Value, String> {
    Ok(schemars::generate::SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<T>()
        .to_value())
}

fn stamp(id: &str, title: &str, mut schema: Value) -> Result<Value, String> {
    let object = schema
        .as_object_mut()
        .ok_or("schema root is not an object")?;
    object.insert("$id".to_owned(), Value::String(id.to_owned()));
    object.insert("title".to_owned(), Value::String(title.to_owned()));
    Ok(schema)
}

fn bundle(id: &str, title: &str, parts: Vec<(&str, Value)>) -> Result<Value, String> {
    let mut defs = Map::new();
    let mut one_of = Vec::new();
    for (name, mut schema) in parts {
        let object = schema
            .as_object_mut()
            .ok_or("schema root is not an object")?;
        if let Some(inner) = object.remove("$defs") {
            let nested = inner.as_object().ok_or("$defs is not an object")?;
            for (key, value) in nested {
                if let Some(existing) = defs.get(key) {
                    if existing != value {
                        return Err(format!("definition `{key}` differs between bundled roots"));
                    }
                } else {
                    defs.insert(key.clone(), value.clone());
                }
            }
        }
        object.remove("$schema");
        object.remove("$id");
        defs.insert(name.to_owned(), schema);
        one_of.push(serde_json::json!({ "$ref": format!("#/$defs/{name}") }));
    }
    Ok(serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": id,
        "title": title,
        "oneOf": one_of,
        "$defs": defs
    }))
}

fn openapi_document() -> Result<Value, String> {
    serde_json::to_value(ContractApi::openapi()).map_err(|error| error.to_string())
}
