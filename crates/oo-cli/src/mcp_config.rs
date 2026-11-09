//! Render configuration with a profile name only, never authentication material.
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
