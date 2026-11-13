//! Delivery adapter for the profile store and authenticated Gateway readiness.
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
    crate::profile::valid_name(name)
}

/// Default MCP snippets. They reference the profile by name; the token stays
/// in the profile store. The `oo mcp` entry point is V3-B-02.
pub fn mcp_config(client: McpClient, profile: &str) -> String {
    let agent = match client {
        McpClient::ClaudeCode => "claude",
        McpClient::Cursor => "cursor",
        McpClient::Codex => "codex",
    };
    crate::mcp_config::render(agent, profile).unwrap_or_else(str::to_owned)
}

/// The caller supplies a credential reference only after issuance succeeds.
/// An unprovisioned demo must never fall back to an existing user's profile.
pub struct Local {
    pub root: std::path::PathBuf,
    pub issued_profile: Option<serde_json::Value>,
    pub zh: bool,
}

impl Delivery for Local {
    fn save_profile(&self, profile: &str) -> Result<String, String> {
        let value = self.issued_profile.as_ref().ok_or_else(|| {
            if self.zh {
                "尚未签发凭据"
            } else {
                "credentials not issued"
            }
            .to_owned()
        })?;
        crate::profile::save(&self.root, profile, value).map_err(str::to_owned)?;
        Ok(if self.zh {
            format!("已保存凭据引用 · 使用 oo use {profile} 切换")
        } else {
            format!("credential reference saved · switch with oo use {profile}")
        })
    }

    fn share_prompt(&self, profile: &str) -> String {
        crate::share_prompt::render_localized(profile, self.zh).unwrap_or_else(str::to_owned)
    }

    fn check(&self, profile: &str) -> Vec<Check> {
        let report = (|| {
            if self.issued_profile.is_none() {
                return Err("PROFILE_NOT_PROVISIONED");
            }
            let saved = crate::profile::load(&self.root, profile)?;
            // Do not report another profile's readiness if enrollment failed.
            if Some(&saved) != self.issued_profile.as_ref() {
                return Err("PROFILE_BINDING");
            }
            let runtime = crate::lifecycle::runtime().map_err(|_| "READINESS_RUNTIME")?;
            runtime.block_on(crate::check::readiness(&saved))
        })();
        readiness_rows(report, self.zh)
    }
}

pub fn readiness_rows(report: Result<serde_json::Value, &str>, zh: bool) -> Vec<Check> {
    let names = if zh {
        ["本体包已发布", "网关可达", "可见表", "新鲜度"]
    } else {
        [
            "pack published",
            "gateway reachable",
            "visible tables",
            "freshness",
        ]
    };
    let report = match report {
        Ok(report) => report,
        Err(code) => {
            return names
                .into_iter()
                .map(|name| Check::new(name, Status::Fail, code))
                .collect();
        }
    };
    let pass = |ok| if ok { Status::Pass } else { Status::Fail };
    vec![
        Check::new(names[0], pass(report["pack_published"] == true), ""),
        Check::new(names[1], pass(report["gateway_reachable"] == true), ""),
        Check::new(
            names[2],
            pass(report["visible_tables"].as_u64().is_some_and(|n| n > 0)),
            &report["visible_tables"]
                .as_u64()
                .map(|n| n.to_string())
                .unwrap_or_default(),
        ),
        Check::new(
            names[3],
            pass(report["ready"] == true && report["freshness"]["publication"] == "live"),
            if zh {
                "发布快照；不保证源端实时或跨表一致"
            } else {
                "published snapshot; not source-realtime or cross-table consistency"
            },
        ),
    ]
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
        for (client, agent) in McpClient::ALL
            .into_iter()
            .zip(["claude", "cursor", "codex"])
        {
            let text = mcp_config(client, "logistics");
            assert_eq!(text, crate::mcp_config::render(agent, "logistics").unwrap());
            assert!(text.contains("--stdio"));
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
        assert!(!profile_name("with.dot"));
    }

    #[test]
    fn readiness_rows_are_localized_and_fail_closed() {
        for zh in [false, true] {
            let error = readiness_rows(Err("READINESS_AUTH"), zh);
            assert!(error.iter().all(|row| row.status == Status::Fail));
            assert_eq!(
                error[0].name,
                if zh {
                    "本体包已发布"
                } else {
                    "pack published"
                }
            );
            let mut report = serde_json::json!({
                "ready": true, "pack_published": true, "gateway_reachable": true,
                "visible_tables": 1, "freshness": {"publication": "live"}
            });
            assert!(
                readiness_rows(Ok(report.clone()), zh)
                    .iter()
                    .all(|r| r.status == Status::Pass)
            );
            report["ready"] = false.into();
            assert!(
                readiness_rows(Ok(report), zh)
                    .iter()
                    .any(|r| r.status == Status::Fail)
            );
        }
    }

    #[test]
    fn unprovisioned_delivery_never_loads_an_existing_profile() {
        let backend = Local {
            root: std::path::PathBuf::from("must-not-be-accessed"),
            issued_profile: None,
            zh: false,
        };
        assert!(backend.save_profile("demo").is_err());
        assert!(
            backend
                .check("demo")
                .iter()
                .all(|row| row.detail == "PROFILE_NOT_PROVISIONED")
        );
        assert!(backend.share_prompt("demo").contains("oo check --json"));
    }

    #[test]
    fn osc52_is_base64() {
        assert_eq!(osc52("hi"), "\x1b]52;c;aGk=\x07");
        assert_eq!(osc52("abc"), "\x1b]52;c;YWJj\x07");
    }
    #[test]
    fn tui_delivery_saves_the_issued_reference_without_replacing_a_profile() {
        use crate::profile;
        use serde_json::json;
        use std::fs;
        let root = std::env::temp_dir().join(format!("oo-tui-delivery-{}", std::process::id()));
        let value = json!({"schema_version":1, "gateway":"https://demo.invalid",
        "token_file":root.join("issued.credential")});
        let backend = Local {
            root: root.clone(),
            issued_profile: Some(value.clone()),
            zh: true,
        };
        assert!(
            backend
                .save_profile("demo")
                .unwrap()
                .contains("oo use demo")
        );
        assert_eq!(profile::load(&root, "demo").unwrap(), value);
        assert!(backend.save_profile("demo").is_err());
        assert_eq!(
            profile::active_path(&root).unwrap(),
            root.join("profiles/demo.json")
        );
        assert!(backend.share_prompt("demo").contains("本机"));
        fs::remove_dir_all(root).unwrap();
    }
}
