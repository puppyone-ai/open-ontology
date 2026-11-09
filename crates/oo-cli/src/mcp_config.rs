//! Render configuration with a profile name only, never authentication material.
use std::io::Read;

/// Private stdio adapter used by the packaged MCP bridge. No credential argv.
pub(crate) async fn request(
    profile: &serde_json::Value,
) -> Result<serde_json::Value, &'static str> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|_| "MCP_INPUT")?;
    if bytes.len() > 65_536 {
        return Err("MCP_INPUT");
    }
    let input: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| "MCP_INPUT")?;
    let path = input["path"].as_str().ok_or("MCP_INPUT")?;
    let body = input.get("body").filter(|v| !v.is_null()).cloned();
    if !allowed_request(path, body.is_some()) {
        return Err("MCP_ROUTE");
    }
    let http = crate::lifecycle::Http::new(profile, false).map_err(|_| "MCP_CONNECTION")?;
    let token = crate::lifecycle::read_token(profile).map_err(|_| "MCP_AUTH")?;
    let (status, value) = http
        .call(path, body, Some(&token), false)
        .await
        .map_err(|_| "MCP_TRANSPORT")?;
    // The caller receives a closed error rather than a potentially sensitive remote body.
    Ok(
        if (200..300).contains(&status) && value.get("error").is_none() {
            serde_json::json!({"status":status, "body":value})
        } else {
            serde_json::json!({"status":status, "body":{"error":{"code":"GATEWAY_REFUSED"}}})
        },
    )
}

fn allowed_request(path: &str, post: bool) -> bool {
    if !path.is_ascii()
        || path.bytes().any(|b| b < 0x21 || b == 0x7f)
        || path.contains(['?', '#', '%', '\\'])
        || path.split('/').any(|s| s == "." || s == "..")
    {
        return false;
    }
    if post {
        [
            "/v1/query",
            "/v1/sql/validate",
            "/v1/knowledge/search",
            "/v1/values/search",
            "/v1/feedback/gap",
            "/v1/check/answerable",
        ]
        .contains(&path)
            || path
                .strip_prefix("/v1/entities/")
                .and_then(|s| s.strip_suffix("/resolve"))
                .is_some_and(|s| !s.is_empty() && !s.contains('/'))
    } else {
        [
            "/v1/tables",
            "/v1/freshness",
            "/v1/check/readiness",
            "/v1/session/current",
            "/v1/pack/metadata",
        ]
        .contains(&path)
            || path
                .strip_prefix("/v1/tables/")
                .is_some_and(|s| !s.is_empty() && !s.contains('/'))
            || path
                .strip_prefix("/v1/knowledge/")
                .is_some_and(|s| !s.is_empty() && !s.split('/').any(str::is_empty))
    }
}

#[cfg(test)]
mod tests {
    use super::allowed_request;
    #[test]
    fn bridge_cannot_reach_administration_or_escape_paths() {
        for path in [
            "/internal/v1/tokens",
            "/v1/knowledge/../../internal/v1/tokens",
            "/v1/knowledge/%2e%2e",
            "/v1/knowledge/x?role=admin",
            "/v1/tables/a\r\nx:y",
        ] {
            assert!(!allowed_request(path, true));
            assert!(!allowed_request(path, false));
        }
        assert!(allowed_request("/v1/query", true));
        assert!(!allowed_request("/v1/query", false));
        assert!(allowed_request("/v1/knowledge/entities/orders.md", false));
    }
}

pub fn render(agent: &str, profile: &str) -> Result<String, &'static str> {
    if !crate::profile::valid_name(profile) {
        return Err("PROFILE_NAME");
    }
    match agent {
        "claude" | "cursor" => Ok(serde_json::json!({"mcpServers": {
            "open-ontology": {"command": "oo", "args": ["mcp", "--stdio", "--profile", profile]}
        }})
        .to_string()),
        "codex" => Ok(format!(
            "[mcp_servers.open-ontology]\ncommand = \"oo\"\nargs = [\"mcp\", \"--stdio\", \"--profile\", \"{profile}\"]\n"
        )),
        _ => Err("MCP_AGENT"),
    }
}
