//! Policy columns copied from the emitted effective document.
//!
//! `explicit_allow` is set only because the caller put the column on a
//! visibility allowlist. That binding is configuration, not authentication.

use oo_gateway::{ClassStatus, ColumnAclPolicy, ColumnSpec, LogicalTable, PiiClass};
use serde_json::{json, Value};

use crate::live::{LOGIN, SOURCE_ID};
use crate::stop::Stop;

pub struct Derived {
    pub name: String,
    pub semantic_type: String,
    pub pii_token: String,
}

pub fn derive_columns(effective: &Value, dataset: &str, allow: &[&str]) -> Result<Vec<Derived>, Stop> {
    let mut out = Vec::with_capacity(allow.len());
    for name in allow {
        out.push(one_column(effective, dataset, name)?);
    }
    Ok(out)
}

fn one_column(effective: &Value, dataset: &str, name: &str) -> Result<Derived, Stop> {
    let column = find_column(effective, dataset, name)?;
    if column["classification"]["state"] != "confirmed" {
        return Err(Stop::new("effective-oos", format!("{dataset}.{name} is not confirmed")));
    }
    if column["classification"]["by"] != "ada.owner" || column["classification"]["at"] != "2026-10-06" {
        return Err(Stop::new("effective-oos", format!("{dataset}.{name} reviewer")));
    }
    let semantic = column["semantic_type"]
        .as_str()
        .ok_or_else(|| Stop::new("effective-oos", format!("{dataset}.{name} semantic_type")))?;
    let pii = column["pii"]
        .as_str()
        .ok_or_else(|| Stop::new("effective-oos", format!("{dataset}.{name} pii")))?;
    match pii {
        "none" | "personal" | "sensitive" | "restricted" => {}
        _ => return Err(Stop::new("effective-oos", format!("{dataset}.{name} pii token"))),
    }
    Ok(Derived {
        name: (*name).to_string(),
        semantic_type: semantic.to_string(),
        pii_token: pii.to_string(),
    })
}

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

pub fn policy_json(columns: &[Derived], digest: &str, generation: i64, role: &str, table: &str) -> Value {
    json!({
        "project": SOURCE_ID,
        "config_digest": digest,
        "generation": generation,
        "role": role,
        "login": LOGIN,
        "table": {
            "namespace": "shop",
            "name": table,
            "columns": columns.iter().map(column_json).collect::<Vec<_>>()
        }
    })
}

fn column_json(column: &Derived) -> Value {
    let pii = if non_none_pii(column) {
        json!({"class": "declared", "explicit_allow": true})
    } else {
        json!({"class": "none"})
    };
    json!({
        "name": column.name,
        "status": "confirmed",
        "semantic_type": column.semantic_type,
        "pii": pii
    })
}

pub fn column_policy(columns: &[Derived], digest: &str, generation: i64, role: &str, table: &str) -> ColumnAclPolicy {
    ColumnAclPolicy {
        project: SOURCE_ID.to_string(),
        config_digest: digest.to_string(),
        generation,
        role: role.to_string(),
        login: LOGIN.to_string(),
        table: LogicalTable {
            namespace: "shop".to_string(),
            name: table.to_string(),
            columns: columns.iter().map(column_spec).collect(),
        },
        row_filter: None,
    }
}

fn non_none_pii(column: &Derived) -> bool {
    matches!(column.pii_token.as_str(), "personal" | "sensitive" | "restricted")
}

fn column_spec(column: &Derived) -> ColumnSpec {
    let pii = if non_none_pii(column) {
        PiiClass::Declared { explicit_allow: true }
    } else {
        PiiClass::None
    };
    ColumnSpec {
        name: column.name.clone(),
        status: ClassStatus::Confirmed,
        semantic_type: column.semantic_type.clone(),
        pii,
    }
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
