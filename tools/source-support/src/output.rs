//! One versioned JSON object on stdout. Field order is part of the contract.

use crate::json::Json;
use crate::matrix::MATRIX_SCHEMA;
use crate::sha256::sha256_hex;

pub const CONTRACT: &str = "oo.source-support/v1";
pub const SOURCE_CONTROL_CONTRACT: &str = "oo.source-control-api/v1";

pub struct Outcome {
    pub operation: &'static str,
    pub ok: bool,
    pub exit_code: i32,
    pub code: &'static str,
    pub reason: &'static str,
    pub matrix_sha256: String,
    pub extra: Vec<(&'static str, Json)>,
}

impl Outcome {
    pub fn new(
        operation: &'static str,
        ok: bool,
        exit_code: i32,
        code: &'static str,
        reason: &'static str,
        matrix_text: &str,
    ) -> Self {
        Self {
            operation,
            ok,
            exit_code,
            code,
            reason,
            matrix_sha256: sha256_hex(matrix_text.as_bytes()),
            extra: Vec::new(),
        }
    }

    pub fn field(mut self, key: &'static str, value: Json) -> Self {
        self.extra.push((key, value));
        self
    }

    pub fn to_json(&self) -> Json {
        let mut items = vec![
            ("contract".to_string(), Json::String(CONTRACT.to_string())),
            ("operation".to_string(), Json::String(self.operation.to_string())),
            ("ok".to_string(), Json::Bool(self.ok)),
            ("code".to_string(), Json::String(self.code.to_string())),
            ("reason".to_string(), Json::String(self.reason.to_string())),
            ("exit".to_string(), Json::Int(i64::from(self.exit_code))),
            ("matrix_schema".to_string(), Json::String(MATRIX_SCHEMA.to_string())),
            ("matrix_sha256".to_string(), Json::String(self.matrix_sha256.clone())),
            ("matrix_mutated".to_string(), Json::Bool(false)),
            ("parent_api_integration".to_string(), Json::String("pending".to_string())),
            ("source_control_contract".to_string(), Json::String(SOURCE_CONTROL_CONTRACT.to_string())),
            ("build_review".to_string(), Json::String("not_claimed".to_string())),
            ("release".to_string(), Json::String("not_claimed".to_string())),
            ("customer_integration".to_string(), Json::String("not_claimed".to_string())),
        ];
        for (key, value) in &self.extra {
            items.push(((*key).to_string(), value.clone()));
        }
        Json::Object(items)
    }
}

pub fn strings(values: &[String]) -> Json {
    Json::Array(values.iter().cloned().map(Json::String).collect())
}

pub fn static_strings(values: &[&str]) -> Json {
    Json::Array(values.iter().map(|value| Json::String((*value).to_string())).collect())
}
