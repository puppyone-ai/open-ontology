//! L5 Suggest over MCP (10 §11.1, §16; B-S7-04, B-S7-07): the `suggest` and `context_check` tools and the
//! `puppyone://share/<sid>/updates` resource. They talk to the docs engine (`oo serve`) directly, never
//! through the Gateway data plane, with the share's own `oodr_` credential (scope `suggest:write` for
//! `suggest`; `:check` and `updates` are the credential's own share calls).
//!
//! Enabled by `OO_MCP_DOCS_URL` (loopback), `OO_MCP_DOCS_CREDENTIAL_FILE` and `OO_MCP_DOCS_SHARE_ID`.

use std::path::PathBuf;
use std::sync::Arc;

use rmcp::ErrorData as McpError;
use rmcp::model::{Tool, ToolAnnotations};
use serde_json::{Map as JsonMap, Value, json};

use crate::http::{GatewayClient, parse_loopback_port};

pub(crate) const SUGGEST_KEYS: &[&str] = &[
    "kind",
    "subject",
    "predicate",
    "object",
    "value",
    "valid_from",
    "supersedes",
    "rationale",
    "evidence",
    "idempotency_key",
];

pub(crate) struct Docs {
    pub(crate) client: Arc<GatewayClient>,
    pub(crate) share_id: String,
}

impl Docs {
    pub(crate) fn from_env() -> Result<Option<Self>, &'static str> {
        let Ok(url) = std::env::var("OO_MCP_DOCS_URL") else {
            return Ok(None);
        };
        let port = parse_loopback_port(&url).map_err(|_| "docs_url")?;
        let file =
            std::env::var("OO_MCP_DOCS_CREDENTIAL_FILE").map_err(|_| "docs_credential_file")?;
        if file.is_empty() {
            return Err("docs_credential_file");
        }
        let share_id = std::env::var("OO_MCP_DOCS_SHARE_ID").map_err(|_| "docs_share_id")?;
        if !is_uuid(&share_id) {
            return Err("docs_share_id");
        }
        Ok(Some(Self {
            client: Arc::new(GatewayClient::docs(port, PathBuf::from(file))),
            share_id,
        }))
    }

    pub(crate) fn updates_uri(&self) -> String {
        format!("puppyone://share/{}/updates", self.share_id)
    }

    /// The share id of an updates URI, when it is this share's.
    pub(crate) fn uri_ok(&self, uri: &str) -> bool {
        uri == self.updates_uri()
    }
}

pub(crate) fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

fn schema(properties: Value, required: &[&str]) -> Arc<JsonMap<String, Value>> {
    let mut m = JsonMap::new();
    m.insert("type".into(), json!("object"));
    m.insert("properties".into(), properties);
    m.insert("required".into(), json!(required));
    m.insert("additionalProperties".into(), json!(false));
    Arc::new(m)
}

pub(crate) fn tools() -> Vec<Tool> {
    let suggest = Tool::new(
        "suggest",
        "Write one suggestion back to the ontology (oo.suggest/v1): a new fact, a supersession or correction, a \
         preference or a type change. The impact rules decide what is automatic; a person sees the rest. \
         Resending the same idempotency_key changes nothing.",
        schema(
            json!({
                "kind": {"type":"string","enum":["new_fact","supersede","correction","preference","type_change"]},
                "subject": {"type":"string","minLength":1,"maxLength":256},
                "predicate": {"type":"string","minLength":1,"maxLength":64},
                "object": {"type":"string","maxLength":256},
                "value": {"type":"string","maxLength":2000},
                "valid_from": {"type":"string","pattern":"^[0-9]{4}-[0-9]{2}-[0-9]{2}$"},
                "supersedes": {"type":"string","maxLength":512},
                "rationale": {"type":"string","minLength":1,"maxLength":500},
                "evidence": {"type":"array","maxItems":20,"items":{"type":"object","properties":{
                    "kind":{"type":"string","enum":["brief_line","ref","external"]},
                    "ref":{"type":"string","minLength":1,"maxLength":1024}},"required":["kind","ref"]}},
                "idempotency_key": {"type":"string","minLength":8,"maxLength":128}
            }),
            &["kind", "subject", "predicate", "rationale"],
        ),
    )
    .with_annotations(
        ToolAnnotations::new()
            .read_only(false)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
    );
    let check = Tool::new(
        "context_check",
        "Ask whether the context this share was built from is still current: exit_code 0 continue, 10 refresh \
         recommended, 20 stop, 30 the share ended. Lists the update entries after `since`.",
        schema(json!({"since": {"type":"integer","minimum":0}}), &[]),
    )
    .with_annotations(
        ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
    );
    vec![suggest, check]
}

/// `POST /v1/docs/suggestions` body for the tool arguments.
pub(crate) fn suggest_body(args: Option<&JsonMap<String, Value>>) -> Result<Value, McpError> {
    let bad = || McpError::invalid_params("invalid tool arguments", None);
    let args = args.ok_or_else(bad)?;
    if args.keys().any(|k| !SUGGEST_KEYS.contains(&k.as_str())) {
        return Err(bad());
    }
    for k in ["kind", "subject", "predicate", "rationale"] {
        if !args
            .get(k)
            .is_some_and(|v| v.as_str().is_some_and(|s| !s.trim().is_empty()))
        {
            return Err(bad());
        }
    }
    let mut body = Value::Object(args.clone());
    body["protocol"] = json!("oo.suggest/v1");
    Ok(body)
}

/// `(path, body)` for `context_check`.
pub(crate) fn check_request(
    share_id: &str,
    args: Option<&JsonMap<String, Value>>,
) -> Result<(String, Value), McpError> {
    let bad = || McpError::invalid_params("invalid tool arguments", None);
    let mut since = 0u64;
    if let Some(a) = args {
        if a.keys().any(|k| k != "since") {
            return Err(bad());
        }
        if let Some(v) = a.get("since") {
            since = v.as_u64().ok_or_else(bad)?;
        }
    }
    Ok((
        format!("/v1/docs/shares/{share_id}:check?since={since}"),
        json!({}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_annotations_are_the_contracted_ones() {
        let t = tools();
        let s = t.iter().find(|t| t.name == "suggest").unwrap();
        let a = s.annotations.as_ref().unwrap();
        assert_eq!(a.read_only_hint, Some(false));
        assert_eq!(a.destructive_hint, Some(false));
        assert_eq!(a.idempotent_hint, Some(true));
        let c = t.iter().find(|t| t.name == "context_check").unwrap();
        assert_eq!(c.annotations.as_ref().unwrap().read_only_hint, Some(true));
    }

    #[test]
    fn suggest_arguments_are_checked_before_they_leave() {
        let ok = json!({"kind":"new_fact","subject":"a","predicate":"attr.status","value":"x","rationale":"r"});
        let body = suggest_body(ok.as_object()).unwrap();
        assert_eq!(body["protocol"], "oo.suggest/v1");
        let extra = json!({"kind":"new_fact","subject":"a","predicate":"p","rationale":"r","share_id":"other"});
        assert!(
            suggest_body(extra.as_object()).is_err(),
            "the share is the credential's, not an argument"
        );
        assert!(suggest_body(json!({"kind":"new_fact"}).as_object()).is_err());
        assert!(suggest_body(None).is_err());
    }

    #[test]
    fn check_path() {
        let sid = "00000000-0000-4000-8000-000000000001";
        assert_eq!(
            check_request(sid, None).unwrap().0,
            format!("/v1/docs/shares/{sid}:check?since=0")
        );
        let a = json!({"since": 7});
        assert!(
            check_request(sid, a.as_object())
                .unwrap()
                .0
                .ends_with("since=7")
        );
        assert!(check_request(sid, json!({"x":1}).as_object()).is_err());
        assert!(check_request(sid, json!({"since":-1}).as_object()).is_err());
    }

    #[test]
    fn uuid_shape() {
        assert!(is_uuid("00000000-0000-4000-8000-000000000001"));
        assert!(!is_uuid("../etc/passwd"));
        assert!(!is_uuid("0000000000004000800000000000000001"));
    }
}
