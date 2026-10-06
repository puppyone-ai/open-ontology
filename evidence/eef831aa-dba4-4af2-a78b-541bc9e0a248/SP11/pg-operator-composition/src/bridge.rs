//! Column lookup on the emitted effective document.
//!
//! The policy DTO comes from `reviewed-policy`. This module does not build one.

use serde_json::Value;

use crate::stop::Stop;

pub fn find_column<'a>(effective: &'a Value, dataset: &str, name: &str) -> Result<&'a Value, Stop> {
    let datasets = effective["datasets"]
        .as_array()
        .ok_or_else(|| Stop::new("effective-oos", "datasets"))?;
    let table = datasets
        .iter()
        .find(|item| item["dataset"] == dataset)
        .ok_or_else(|| Stop::new("effective-oos", dataset))?;
    table["columns"]
        .as_array()
        .ok_or_else(|| Stop::new("effective-oos", "columns"))?
        .iter()
        .find(|column| column["name"] == name)
        .ok_or_else(|| Stop::new("effective-oos", format!("{dataset}.{name}")))
}

pub fn hidden_reason(doc: &Value, dataset: &str, column: &str) -> Result<String, Stop> {
    doc["hidden"]
        .as_array()
        .ok_or_else(|| Stop::new("effective-oos", "hidden"))?
        .iter()
        .find(|item| item["dataset"] == dataset && item["column"] == column)
        .and_then(|item| item["reason"].as_str())
        .map(str::to_string)
        .ok_or_else(|| Stop::new("effective-oos", format!("hidden {dataset}.{column}")))
}
