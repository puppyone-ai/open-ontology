//! Seams to the non-UI lanes. Lane B owns profiles, readiness (`oo check`),
//! share-prompt rendering and MCP configuration; until its modules land the
//! TUI uses `Pending`, which writes nothing and says so on screen.
use super::screens::{Check, Status};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum McpClient {
    ClaudeCode,
    Cursor,
    Codex,
}

impl McpClient {
    pub const ALL: [McpClient; 3] = [McpClient::ClaudeCode, McpClient::Cursor, McpClient::Codex];

    pub fn name(self) -> &'static str {
        match self {
            McpClient::ClaudeCode => "Claude Code",
            McpClient::Cursor => "Cursor",
            McpClient::Codex => "Codex",
        }
    }
}

pub trait Delivery {
    /// Store the Gateway token in the named CLI profile. Returns a short note.
    fn save_profile(&self, profile: &str) -> Result<String, String>;
    /// The prompt an agent needs. Never contains credentials.
    fn share_prompt(&self, profile: &str) -> String;
    /// MCP configuration naming the profile only.
    fn mcp_config(&self, client: McpClient, profile: &str) -> String {
        mcp_config(client, profile)
    }
    /// Readiness: pack published, gateway reachable, visible tables, freshness.
    fn check(&self, profile: &str) -> Vec<Check>;
}

/// Profile names are config identifiers, never paths or secrets.
pub fn profile_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        && !name.starts_with('.')
}

/// Default MCP snippets. They reference the profile by name; the token stays
/// in the profile store. The `oo mcp` entry point is V3-B-02.
pub fn mcp_config(client: McpClient, profile: &str) -> String {
    match client {
        McpClient::ClaudeCode => {
            format!("claude mcp add open-ontology -- oo mcp --profile {profile}")
        }
        McpClient::Cursor => format!(
            "// .cursor/mcp.json\n{{\n  \"mcpServers\": {{\n    \"open-ontology\": {{\n      \"command\": \"oo\",\n      \"args\": [\"mcp\", \"--profile\", \"{profile}\"]\n    }}\n  }}\n}}"
        ),
        McpClient::Codex => format!(
            "# ~/.codex/config.toml\n[mcp_servers.open-ontology]\ncommand = \"oo\"\nargs = [\"mcp\", \"--profile\", \"{profile}\"]"
        ),
    }
}

/// Placeholder until Lane B's profile/check modules are wired.
pub struct Pending;

impl Delivery for Pending {
    fn save_profile(&self, _profile: &str) -> Result<String, String> {
        Err("profile store not available in this build (V3-B-01)".into())
    }

    fn share_prompt(&self, profile: &str) -> String {
        format!(
            "You can answer business questions from the \"{profile}\" ontology with the oo CLI. \
             Start with `oo --profile {profile} tables`, then `oo --profile {profile} describe <table>`, \
             and run read-only SQL with `oo --profile {profile} query`. Always report freshness."
        )
    }

    fn check(&self, _profile: &str) -> Vec<Check> {
        [
            "pack published",
            "gateway reachable",
            "visible tables",
            "freshness",
        ]
        .into_iter()
        .map(|name| {
            Check::new(
                name,
                Status::Waiting,
                "readiness check not available in this build",
            )
        })
        .collect()
    }
}

/// Copy text through the terminal (OSC 52). Returns the escape sequence; the
/// runner writes it to the TTY only.
pub fn osc52(text: &str) -> String {
    const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(bytes.len() * 4 / 3 + 8);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    format!("\x1b]52;c;{out}\x07")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_configs_name_the_profile_and_carry_no_secret() {
        for client in McpClient::ALL {
            let text = mcp_config(client, "logistics");
            assert!(
                text.contains("--profile") && text.contains("logistics"),
                "{text}"
            );
            for secret in ["token", "Bearer", "password", "sk-", "secret"] {
                assert!(
                    !text.to_lowercase().contains(&secret.to_lowercase()),
                    "{text}"
                );
            }
        }
        assert!(profile_name("logistics-prod"));
        assert!(!profile_name("../etc"));
        assert!(!profile_name(""));
    }

    #[test]
    fn osc52_is_base64() {
        assert_eq!(osc52("hi"), "\x1b]52;c;aGk=\x07");
        assert_eq!(osc52("abc"), "\x1b]52;c;YWJj\x07");
    }
}
