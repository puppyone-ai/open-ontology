//! Apply the pinned OOS JSON Schema file to one effective document.
//!
//! The file is `contracts/schema/oos-v1.json` from the builder pin. A gate is
//! not admissible while this file has not been compiled and applied. Names,
//! money units, and references stay separate checks.

use std::sync::OnceLock;

use serde_json::Value;

use crate::sha256::sha256_hex;

const SCHEMA_TEXT: &str = include_str!("../schema/oos-v1.json");

struct Fragments {
    sha256: String,
    ontology: jsonschema::Validator,
    dataset: jsonschema::Validator,
    relationship: jsonschema::Validator,
}

pub fn apply(effective: &Value) -> Result<String, String> {
    let compiled = fragments()?;
    let object = effective.as_object().ok_or_else(|| "fragment=effective".to_owned())?;
    let ontology = object.get("ontology").ok_or_else(|| "fragment=ontology_absent".to_owned())?;
    check(&compiled.ontology, ontology, "OntologyFile")?;
    if let Some(datasets) = object.get("datasets").and_then(Value::as_array) {
        for dataset in datasets {
            check(&compiled.dataset, dataset, "DatasetFile")?;
        }
    }
    if let Some(relationships) = object.get("relationships").and_then(Value::as_array) {
        for relationship in relationships {
            check(&compiled.relationship, relationship, "Relationship")?;
        }
    }
    Ok(compiled.sha256.clone())
}

fn fragments() -> Result<&'static Fragments, String> {
    static COMPILED: OnceLock<Result<Fragments, String>> = OnceLock::new();
    COMPILED
        .get_or_init(|| {
            let sha256 = sha256_hex(SCHEMA_TEXT.as_bytes());
            Ok(Fragments {
                sha256,
                ontology: compile("OntologyFile")?,
                dataset: compile("DatasetFile")?,
                relationship: compile("Relationship")?,
            })
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn compile(name: &str) -> Result<jsonschema::Validator, String> {
    let mut root: Value = serde_json::from_str(SCHEMA_TEXT).map_err(|_| "json_schema_file_not_applied".to_owned())?;
    let object = root.as_object_mut().ok_or_else(|| "json_schema_file_not_applied".to_owned())?;
    object.remove("oneOf");
    object.remove("title");
    object.insert("$ref".to_owned(), Value::String(format!("#/$defs/{name}")));
    jsonschema::draft202012::options()
        .offline()
        .should_validate_formats(true)
        .with_keyword("x-utf8-max-bytes", utf8_max_bytes)
        .build(&root)
        .map_err(|_| "json_schema_file_not_applied".to_owned())
}

fn check(validator: &jsonschema::Validator, instance: &Value, fragment: &str) -> Result<(), String> {
    match validator.validate(instance) {
        Ok(()) => Ok(()),
        Err(error) => {
            let mut text = error.to_string();
            text.truncate(80);
            text.retain(|ch| ch != ';' && ch != '\n');
            Err(format!("fragment={fragment};validator={text}"))
        }
    }
}

fn utf8_max_bytes<'a>(
    _parent: &'a serde_json::Map<String, Value>,
    schema: &'a Value,
    _path: jsonschema::paths::Location,
) -> Result<Box<dyn for<'i> jsonschema::Keyword<'i>>, jsonschema::ValidationError<'a>> {
    let Some(max) = schema.as_u64() else {
        return Err(jsonschema::ValidationError::custom("x-utf8-max-bytes must be an integer"));
    };
    let max = usize::try_from(max).map_err(|_| jsonschema::ValidationError::custom("x-utf8-max-bytes does not fit this host"))?;
    Ok(Box::new(Utf8MaxBytes { max }))
}

struct Utf8MaxBytes {
    max: usize,
}

impl<'i> jsonschema::Keyword<'i> for Utf8MaxBytes {
    fn validate(&self, instance: &'i Value) -> Result<(), jsonschema::ValidationError<'i>> {
        if self.is_valid(instance) {
            Ok(())
        } else {
            Err(jsonschema::ValidationError::custom("string exceeds its UTF-8 byte budget"))
        }
    }

    fn is_valid(&self, instance: &'i Value) -> bool {
        match instance.as_str() {
            Some(text) => text.len() <= self.max,
            None => true,
        }
    }
}
