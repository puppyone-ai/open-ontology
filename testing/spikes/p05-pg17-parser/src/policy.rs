use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RelationPolicy {
    pub columns: Vec<String>,
    pub visible: Vec<String>,
    pub large: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Policy {
    pub relations: BTreeMap<String, RelationPolicy>,
}

impl Policy {
    pub fn parse(text: &str) -> Result<Self, String> {
        let policy: Self = serde_json::from_str(text).map_err(|err| err.to_string())?;
        for (name, rel) in &policy.relations {
            let mut parts = name.split('.');
            let schema = parts.next().unwrap_or("");
            let relname = parts.next().unwrap_or("");
            if schema.is_empty() || relname.is_empty() || parts.next().is_some() {
                return Err(format!("relation key must be schema.name: {name}"));
            }
            let mut rest = rel.columns.iter();
            for visible in &rel.visible {
                let found = rest.any(|column| column == visible);
                if !found {
                    return Err(format!("visible column {visible} is missing or out of order in {name}"));
                }
            }
        }
        Ok(policy)
    }
}

pub fn forbidden_name(schema: &str, rel: &str) -> bool {
    let schema = schema.to_ascii_lowercase();
    let rel = rel.to_ascii_lowercase();
    schema == "pg_catalog"
        || schema == "information_schema"
        || schema == "_oo"
        || schema.ends_with("__stage")
        || rel.ends_with("__stage")
        || rel == "_oo"
}
