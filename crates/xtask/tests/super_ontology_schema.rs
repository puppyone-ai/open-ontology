//! Super Ontology contract files (spec 10 §8.7, B-S1-01): every schema is a
//! valid draft 2020-12 schema, every `$def` carries examples that validate
//! against that `$def`, and the error and event tables are internally sound.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde_json::{Value, json};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn load(rel: &str) -> Value {
    let text = std::fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn compile(schema: &Value) -> jsonschema::Validator {
    jsonschema::draft202012::options()
        .offline()
        .should_validate_formats(true)
        .build(schema)
        .expect("schema compiles")
}

/// A schema rooted at one `$def` of `doc`, sharing its `$defs` for references.
fn def_schema(doc: &Value, name: &str) -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$defs": doc["$defs"].clone(),
        "$ref": format!("#/$defs/{name}"),
    })
}

fn assert_examples(rel: &str) {
    let doc = load(rel);
    assert!(
        jsonschema::meta::is_valid(&doc),
        "{rel} is not a valid JSON Schema"
    );
    let defs = doc["$defs"].as_object().expect("$defs");
    let mut failures = Vec::new();
    for (name, def) in defs {
        let examples = def["examples"]
            .as_array()
            .unwrap_or_else(|| panic!("{rel} {name}: no examples"));
        assert!(!examples.is_empty(), "{rel} {name}: empty examples");
        let validator = compile(&def_schema(&doc, name));
        for example in examples {
            if !validator.is_valid(example) {
                failures.push(format!("{name}: {example}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{rel}: examples failing their own $def:\n{}",
        failures.join("\n")
    );
}

#[test]
fn super_ontology_schema_docs_api_examples_validate() {
    assert_examples("contracts/docs-api/docs-api.v1.schema.json");
    let doc = load("contracts/docs-api/docs-api.v1.schema.json");
    assert_eq!(doc["x-docs-api-version"], "1.0-draft");
    let endpoints = doc["x-endpoints"].as_array().expect("x-endpoints");
    let defs = doc["$defs"].as_object().unwrap();
    let mut seen = BTreeSet::new();
    for ep in endpoints {
        let key = format!(
            "{} {}",
            ep["method"].as_str().unwrap(),
            ep["path"].as_str().unwrap()
        );
        assert!(seen.insert(key.clone()), "duplicate endpoint {key}");
        if let Some(req) = ep["request"].as_str() {
            assert!(defs.contains_key(req), "{key}: request {req} is not a $def");
            assert_eq!(
                defs[req]["additionalProperties"], false,
                "{key}: request {req} must be closed"
            );
        }
    }
}

#[test]
fn super_ontology_schema_context_pack_and_ir_are_valid() {
    for rel in [
        "contracts/context-pack/context-pack.v1.schema.json",
        "contracts/docs-ir/doc-ir.v1.schema.json",
    ] {
        assert!(jsonschema::meta::is_valid(&load(rel)), "{rel}");
    }
    let doc = load("contracts/context-pack/context-pack.v1.schema.json");
    let aigc = &doc["$defs"]["aigc"];
    assert_eq!(aigc["properties"]["Label"]["const"], "1");
    for def in ["manifest", "share"] {
        let required: Vec<&str> = doc["$defs"][def]["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(
            required.contains(&"aigc"),
            "{def} must require the AIGC label (D66)"
        );
    }
}

#[test]
fn super_ontology_schema_errors_and_events_tables() {
    let errors = load("contracts/docs-api/errors.v1.json");
    let pattern =
        load("contracts/docs-api/docs-api.v1.schema.json")["$defs"]["Error"]["properties"]["code"]
            ["pattern"]
            .as_str()
            .unwrap()
            .to_owned();
    let code_check = compile(&json!({"type": "string", "pattern": pattern}));
    let mut codes = BTreeSet::new();
    for entry in errors["codes"].as_array().unwrap() {
        let code = entry["code"].as_str().unwrap();
        assert!(codes.insert(code), "duplicate {code}");
        assert!(
            code_check.is_valid(&json!(code)),
            "{code} does not match Error.code"
        );
        assert!(
            entry["retryable"].is_boolean(),
            "{code}: retryable must be boolean"
        );
        assert_eq!(
            entry["i18n_key"],
            format!("super-drive.errors.{code}"),
            "{code}: G8 i18n key"
        );
    }
    let events = load("contracts/docs-api/events.v1.schema.json");
    assert!(jsonschema::meta::is_valid(&events));
    let validator = compile(&events);
    for example in events["examples"].as_array().unwrap() {
        assert!(validator.is_valid(example), "event example {example}");
    }
    let bad = json!({"type": "file.progress", "seq": 3, "at": "2026-11-17T00:00:00Z", "data": {}});
    assert!(
        !validator.is_valid(&bad),
        "transient events must not carry seq"
    );
}

#[test]
fn super_ontology_schema_error_details_are_plain_field_names() {
    let errors = load("contracts/docs-api/errors.v1.json");
    let field = compile(&json!({"type": "string", "pattern": "^[a-z][a-z0-9_.]*(\\[\\])?$"}));
    for entry in errors["codes"].as_array().unwrap() {
        for detail in entry["details"].as_array().unwrap() {
            assert!(
                field.is_valid(detail),
                "{}: detail {detail} is not a field name",
                entry["code"]
            );
        }
    }
}
