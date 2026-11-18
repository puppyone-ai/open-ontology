//! The unified ontology tools over MCP (D83, WP4): `ontology_search`, `ontology_entity`,
//! `ontology_sources`, `ontology_query`. They are the CLI's `oo ontology` verbs and call the same
//! `/v1/ontology/*` surface of `oo serve` with the Agent's own `ooa_` token; scope, drive and
//! revocation are decided there, and every answer carries its provenance.
//!
//! Enabled by `OO_MCP_ONTOLOGY_URL` (loopback) and `OO_MCP_ONTOLOGY_TOKEN_FILE`; `oo mcp --stdio
//! --endpoint <url> --token-file <file>` sets them.

use std::path::PathBuf;
use std::sync::Arc;

use rmcp::ErrorData as McpError;
use rmcp::model::{Tool, ToolAnnotations};
use serde_json::{Map as JsonMap, Value, json};

use crate::http::{GatewayClient, parse_loopback_port};

pub(crate) const INSTRUCTIONS: &str = "One ontology over documents and databases. ontology_search finds entities, facts, text blocks and rows (each item has a urn and a provenance); ontology_entity opens a urn with its facts and links to the other side; ontology_query runs one read-only SELECT; ontology_sources lists what you can reach. Cite the provenance of every answer (file anchor or table row). A refusal is not an empty result.";

pub(crate) struct Ontology {
    pub(crate) client: Arc<GatewayClient>,
}

impl Ontology {
    pub(crate) fn from_env() -> Result<Option<Self>, &'static str> {
        let Ok(url) = std::env::var("OO_MCP_ONTOLOGY_URL") else {
            return Ok(None);
        };
        let port = parse_loopback_port(&url).map_err(|_| "ontology_url")?;
        let file =
            std::env::var("OO_MCP_ONTOLOGY_TOKEN_FILE").map_err(|_| "ontology_token_file")?;
        if file.is_empty() {
            return Err("ontology_token_file");
        }
        Ok(Some(Self {
            client: Arc::new(GatewayClient::docs(port, PathBuf::from(file))),
        }))
    }
}

pub(crate) fn pct(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~:".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

fn schema(properties: Value, required: &[&str]) -> Arc<JsonMap<String, Value>> {
    let mut m = JsonMap::new();
    m.insert("type".into(), json!("object"));
    m.insert("properties".into(), properties);
    m.insert("required".into(), json!(required));
    m.insert("additionalProperties".into(), json!(false));
    Arc::new(m)
}

pub(crate) fn names() -> [&'static str; 4] {
    [
        "ontology_search",
        "ontology_entity",
        "ontology_sources",
        "ontology_query",
    ]
}

pub(crate) fn tools() -> Vec<Tool> {
    let ro = || {
        ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(false)
    };
    vec![
        Tool::new(
            "ontology_search",
            "Search the documents (entity names, facts and text blocks, full text, Chinese and English) and the database rows by name. Each item has a kind (entity, fact or block) and every item has a stable urn, a side (doc or db) and a provenance (file anchor or table row); linked items are cross-referenced.",
            schema(
                json!({"q":{"type":"string","minLength":1,"maxLength":200},
                       "type":{"type":"string","maxLength":64},
                       "limit":{"type":"integer","minimum":1,"maximum":50}}),
                &["q"],
            ),
        )
        .with_annotations(ro()),
        Tool::new(
            "ontology_entity",
            "Open one urn (oo:doc/... or oo:db/...): its facts or row, each with provenance, and its links to the other side.",
            schema(json!({"urn":{"type":"string","minLength":1,"maxLength":512}}), &["urn"]),
        )
        .with_annotations(ro()),
        Tool::new(
            "ontology_sources",
            "List the Drives and the database this token reaches, with freshness.",
            schema(json!({}), &[]),
        )
        .with_annotations(ro()),
        Tool::new(
            "ontology_query",
            "Run one read-only SELECT through the Gateway (needs db:read). Writes are refused. The answer carries the query id and publication.",
            schema(
                json!({"sql":{"type":"string","minLength":1,"maxLength":16384},
                       "max_rows":{"type":"integer","minimum":1,"maximum":2000}}),
                &["sql"],
            ),
        )
        .with_annotations(ro()),
    ]
}

/// `(post body, path)` for a tool call; a `None` body is a GET.
pub(crate) fn request(
    name: &str,
    args: Option<&JsonMap<String, Value>>,
) -> Result<(String, Option<Value>), McpError> {
    let bad = || McpError::invalid_params("invalid tool arguments", None);
    let empty = JsonMap::new();
    let a = args.unwrap_or(&empty);
    let allowed = |keys: &[&str]| -> Result<(), McpError> {
        if a.keys().any(|k| !keys.contains(&k.as_str())) {
            Err(bad())
        } else {
            Ok(())
        }
    };
    let text = |k: &str| {
        a.get(k)
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
    };
    match name {
        "ontology_search" => {
            allowed(&["q", "type", "limit"])?;
            let mut p = format!("/v1/ontology/search?q={}", pct(text("q").ok_or_else(bad)?));
            if let Some(t) = a.get("type") {
                p.push_str(&format!("&type={}", pct(t.as_str().ok_or_else(bad)?)));
            }
            if let Some(l) = a.get("limit") {
                p.push_str(&format!("&limit={}", l.as_u64().ok_or_else(bad)?));
            }
            Ok((p, None))
        }
        "ontology_entity" => {
            allowed(&["urn"])?;
            Ok((
                format!(
                    "/v1/ontology/entities/{}",
                    pct(text("urn").ok_or_else(bad)?)
                ),
                None,
            ))
        }
        "ontology_sources" => {
            allowed(&[])?;
            Ok(("/v1/ontology/sources".into(), None))
        }
        "ontology_query" => {
            allowed(&["sql", "max_rows"])?;
            let mut body = json!({"sql": text("sql").ok_or_else(bad)?});
            if let Some(n) = a.get("max_rows") {
                body["max_rows"] = json!(n.as_u64().ok_or_else(bad)?);
            }
            Ok(("/v1/ontology/query".into(), Some(body)))
        }
        _ => Err(bad()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_read_only_tools() {
        let t = tools();
        assert_eq!(t.len(), 4);
        assert!(
            t.iter()
                .all(|t| t.annotations.as_ref().unwrap().read_only_hint == Some(true))
        );
    }

    #[test]
    fn requests_are_built_from_checked_arguments() {
        let a = json!({"q": "Acme Ltd", "limit": 5});
        assert_eq!(
            request("ontology_search", a.as_object()).unwrap().0,
            "/v1/ontology/search?q=Acme%20Ltd&limit=5"
        );
        let u = json!({"urn": "oo:doc/d/org:a"});
        assert_eq!(
            request("ontology_entity", u.as_object()).unwrap().0,
            "/v1/ontology/entities/oo:doc%2Fd%2Forg:a"
        );
        let q = json!({"sql": "SELECT 1"});
        assert!(
            request("ontology_query", q.as_object())
                .unwrap()
                .1
                .is_some()
        );
        assert!(request("ontology_search", json!({"q":"a","x":1}).as_object()).is_err());
        assert!(request("ontology_search", None).is_err());
    }
}
