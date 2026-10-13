//! Trusted catalog bytes to the existing `pg-builder` seal and cards.
//!
//! `seal_request` and `assemble_cards` are the producer. A catalog that is not
//! `oo.source.pgsql/v1` goes to `import_seal`, which records the compiler
//! refusal. That refusal is not `seal.json`. A draft is not a model result.
//! This module does not retag a protocol, invent an OOS type, or call the
//! native catalog command.

use serde_json::{json, Value};

use pg_builder::{assemble_cards, canonical_json, import_seal, seal_request};

pub const PREPARE_PROTOCOL: &str = "oo.selfbuild-control-api.builder-prepare/v1";
pub const PGSQL_PROTOCOL: &str = "oo.source.pgsql/v1";
pub const SEAL_PROTOCOL: &str = "oo.pg-builder.seal/v1";
pub const CARDS_PROTOCOL: &str = "oo.pg-builder.cards/v1";

const NATIVE_REQUEST: &str = "oo.pipeline.native-sealed-catalog/v1";
const NATIVE_CATALOG: &str = "oo.pipeline.sealed-import-catalog/v1";
const NATIVE_COMMAND: &str = "oo-pipeline catalog-import-native";

pub struct Prepared {
    pub document: Vec<u8>,
    pub seal: Option<Vec<u8>>,
    pub cards: Option<Vec<u8>>,
}

pub fn prepare(catalog_bytes: &[u8], profile_bytes: &[u8], comments_bytes: Option<&[u8]>, max_tables: usize) -> Prepared {
    let Some(catalog_text) = std::str::from_utf8(catalog_bytes).ok() else {
        return finish("", "input.schema", "the catalog is not UTF-8", "pg_builder::seal_request", None, None, None);
    };
    let protocol = serde_json::from_str::<Value>(catalog_text)
        .ok()
        .and_then(|value| value.get("protocol").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_default();
    if protocol != PGSQL_PROTOCOL {
        return refuse_foreign(&protocol, catalog_text);
    }
    let sealed = match seal_request(catalog_text) {
        Ok(value) => value,
        Err(error) => {
            return finish(PGSQL_PROTOCOL, &error.code, error.message, "pg_builder::seal_request", None, None, None);
        }
    };
    if !seal_accepted(&sealed) {
        return finish(
            PGSQL_PROTOCOL,
            "oo.pg-builder/v1/compiler.seal",
            "the captured catalog was refused",
            "pg_builder::seal_request",
            None,
            None,
            None,
        );
    }
    let seal_bytes = producer_bytes(&sealed);
    let schema = sealed.get("schema_hash").and_then(Value::as_str).unwrap_or("");
    let evidence = sealed.get("evidence_hash").and_then(Value::as_str).unwrap_or("");
    let Some(cards_bytes) = cards_of(catalog_text, profile_bytes, comments_bytes, schema, evidence, max_tables) else {
        return finish(
            PGSQL_PROTOCOL,
            "oo.pg-builder/v1/cards.refused",
            "the evidence cards were refused",
            "pg_builder::seal_request",
            Some(seal_bytes),
            None,
            Some(sealed.get("prepare_status").and_then(Value::as_str).unwrap_or("").to_owned()),
        );
    };
    finish(
        PGSQL_PROTOCOL,
        "oo.pg-builder/v1/sealed",
        "the producer wrote seal and cards",
        "pg_builder::seal_request",
        Some(seal_bytes),
        Some(cards_bytes),
        Some(sealed.get("prepare_status").and_then(Value::as_str).unwrap_or("").to_owned()),
    )
}

pub fn prepare_observed(
    catalog_bytes: &[u8],
    profile_bytes: &[u8],
    comments_bytes: Option<&[u8]>,
    bases: &[&[u8]],
    audits: &[&[u8]],
    max_tables: usize,
) -> Prepared {
    let protocol = protocol_of(catalog_bytes);
    if protocol != crate::adapt::CAPTURE_CATALOG || audits.is_empty() {
        if protocol.starts_with("oo.source.") && protocol != PGSQL_PROTOCOL {
            return finish(
                &protocol,
                "sealer.engine",
                "the postgres sealer was not called; the dictionary keeps its own protocol",
                "not_called",
                None,
                None,
                None,
            );
        }
        return prepare(catalog_bytes, profile_bytes, comments_bytes, max_tables);
    }
    match crate::project_catalog::project(catalog_bytes, profile_bytes, bases, audits) {
        Ok(projected) => {
            let mut prepared = prepare(&projected.catalog, &projected.profile, comments_bytes, max_tables);
            annotate(&mut prepared, projected.note);
            prepared
        }
        Err(message) => finish(&protocol, "catalog.incomplete", message, "pg_builder::seal_request", None, None, None),
    }
}

fn annotate(prepared: &mut Prepared, note: Value) {
    let mut document: Value = serde_json::from_slice(&prepared.document).unwrap_or(Value::Null);
    if let Some(object) = document.as_object_mut() {
        object.insert("projection".to_owned(), note);
    }
    prepared.document = producer_bytes(&document);
}

fn protocol_of(catalog_bytes: &[u8]) -> String {
    let Ok(text) = std::str::from_utf8(catalog_bytes) else {
        return String::new();
    };
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|value| value.get("protocol").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_default()
}

fn refuse_foreign(protocol: &str, catalog_text: &str) -> Prepared {
    match import_seal(catalog_text) {
        Ok(document) => {
            let outcome = document.get("outcome").and_then(Value::as_str).unwrap_or("");
            let mapped = match outcome {
                "compiler_refused" => "compiler_refused",
                "draft_compiled" => "draft_not_model",
                _ => "producer_refused",
            };
            let code = document.get("code").and_then(Value::as_str).unwrap_or("oo.pg-builder/v1/compiler.seal");
            let message = document.get("message").and_then(Value::as_str).unwrap_or("the captured catalog was refused");
            let mut prepared = finish(protocol, code, message, "pg_builder::import_seal", None, None, None);
            if mapped != "compiler_refused" {
                prepared = retag_outcome(&prepared, mapped);
            }
            prepared
        }
        Err(error) => finish(protocol, &error.code, error.message, "pg_builder::import_seal", None, None, None),
    }
}

fn retag_outcome(prepared: &Prepared, outcome: &str) -> Prepared {
    let mut document: Value = serde_json::from_slice(&prepared.document).unwrap_or(Value::Null);
    if let Some(object) = document.as_object_mut() {
        object.insert("outcome".to_owned(), json!(outcome));
        object.insert("seal_written".to_owned(), json!(false));
        object.insert("cards_written".to_owned(), json!(false));
    }
    Prepared {
        document: producer_bytes(&document),
        seal: None,
        cards: None,
    }
}

fn cards_of(
    catalog_text: &str,
    profile_bytes: &[u8],
    comments_bytes: Option<&[u8]>,
    schema: &str,
    evidence: &str,
    max_tables: usize,
) -> Option<Vec<u8>> {
    let catalog: Value = serde_json::from_str(catalog_text).ok()?;
    let profile: Value = serde_json::from_slice(profile_bytes).ok()?;
    let comments = match comments_bytes {
        Some(bytes) => serde_json::from_slice(bytes).ok()?,
        None => json!({
            "authority": "untrusted_data",
            "protocol": "oo.pg-builder.comments/v1",
            "tables": []
        }),
    };
    let cards = assemble_cards(&catalog, &profile, &comments, schema, evidence, max_tables).ok()?;
    if !cards_accepted(&cards) {
        return None;
    }
    Some(producer_bytes(&cards))
}

fn seal_accepted(value: &Value) -> bool {
    value.get("protocol").and_then(Value::as_str) == Some(SEAL_PROTOCOL)
        && value.get("command").and_then(Value::as_str).is_some_and(|text| !text.is_empty())
        && value.get("schema_hash").and_then(Value::as_str).is_some_and(|text| text.len() == 64)
        && value.get("evidence_hash").and_then(Value::as_str).is_some_and(|text| text.len() == 64)
        && value.get("ready") != Some(&Value::Bool(true))
        && value.get("published") != Some(&Value::Bool(true))
}

fn cards_accepted(value: &Value) -> bool {
    if value.get("protocol").and_then(Value::as_str) != Some(CARDS_PROTOCOL) {
        return false;
    }
    if value.get("sample_policy").and_then(Value::as_str) != Some("none") {
        return false;
    }
    if value.get("ready") == Some(&Value::Bool(true)) || value.get("published") == Some(&Value::Bool(true)) {
        return false;
    }
    let Some(tables) = value.get("tables").and_then(Value::as_array) else {
        return false;
    };
    if tables.is_empty() {
        return false;
    }
    for table in tables {
        let Some(columns) = table.get("columns").and_then(Value::as_array) else {
            return false;
        };
        for column in columns {
            if column.get("most_common_vals").and_then(Value::as_str) != Some("withheld") {
                return false;
            }
            if column.get("histogram_bounds").and_then(Value::as_str) != Some("withheld") {
                return false;
            }
        }
    }
    true
}

fn finish(
    catalog_protocol: &str,
    code: &str,
    message: &str,
    producer: &str,
    seal: Option<Vec<u8>>,
    cards: Option<Vec<u8>>,
    prepare_status: Option<String>,
) -> Prepared {
    let outcome = if seal.is_some() && cards.is_some() {
        "sealed"
    } else if seal.is_some() {
        "cards_refused"
    } else if code.contains("compiler") || message.contains("oo.source.pgsql/v1") || message.contains("refused") {
        "compiler_refused"
    } else {
        "producer_refused"
    };
    let document = json!({
        "bytea_remapped": false,
        "catalog_protocol": catalog_protocol,
        "cards_protocol": cards.as_ref().map(|_| CARDS_PROTOCOL),
        "cards_written": cards.is_some(),
        "code": code,
        "llm_calls": 0,
        "message": message,
        "model": "not_called",
        "native_catalog_called": false,
        "native_catalog_protocol": NATIVE_CATALOG,
        "native_command": NATIVE_COMMAND,
        "native_env": ["OO_NATIVE_ADMISSION_BIN", "OO_NATIVE_BINDING_REGISTRY", "OO_NATIVE_BINDING_REGISTRY_SHA256"],
        "native_request_protocol": NATIVE_REQUEST,
        "outcome": outcome,
        "pack": "not_produced",
        "prepare_status": prepare_status,
        "producer": producer,
        "protocol": PREPARE_PROTOCOL,
        "published": false,
        "ready": false,
        "seal_protocol": seal.as_ref().map(|_| SEAL_PROTOCOL),
        "seal_written": seal.is_some(),
        "structural_draft": false
    });
    Prepared {
        document: producer_bytes(&document),
        seal,
        cards,
    }
}

fn producer_bytes(value: &Value) -> Vec<u8> {
    let mut text = canonical_json(value);
    text.push('\n');
    text.into_bytes()
}
