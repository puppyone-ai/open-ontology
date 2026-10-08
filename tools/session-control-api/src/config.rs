//! Operator document. Passwords live only in this value and are wiped on drop.

use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::error::Fault;
use crate::sha256::{hex, sha256};

pub const CONTRACT: &str = "session-control-api-v1";
pub const OWNER_UUID: &str = "eef831aa-dba4-4af2-a78b-541bc9e0a387";
pub const PRODUCTION_SIGNED: bool = false;
pub const DEPLOYMENT: &str = "localprocess";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub principal_id: String,
    pub project_id: String,
    pub role: String,
    pub claims: Vec<(String, String)>,
    pub policy_version: String,
    pub knowledge_version: String,
    pub config_version: String,
    pub capabilities: Vec<String>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PgConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub ca_pem: String,
    pub api_admin_password: String,
    pub api_runtime_password: String,
    pub session_admin_password: String,
    pub session_runtime_password: String,
    pub cancel_password: String,
    pub query_password: String,
    pub token_admin_password: String,
    pub token_runtime_password: String,
}

impl Drop for PgConfig {
    fn drop(&mut self) {
        wipe(&mut self.api_admin_password);
        wipe(&mut self.api_runtime_password);
        wipe(&mut self.session_admin_password);
        wipe(&mut self.session_runtime_password);
        wipe(&mut self.cancel_password);
        wipe(&mut self.query_password);
        wipe(&mut self.token_admin_password);
        wipe(&mut self.token_runtime_password);
        wipe(&mut self.ca_pem);
    }
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IssueRequest {
    pub principal_id: String,
    pub project_id: String,
    pub grant_out: String,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub contract: String,
    pub profile: String,
    pub bind: String,
    pub owner_uuid: String,
    pub state_dir: String,
    pub bindings_path: String,
    pub audience_url: String,
    pub pg: PgConfig,
    #[serde(default)]
    pub issue: Option<IssueRequest>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    LocalProcess,
}

pub fn startup_gate(profile: &str) -> Result<Profile, &'static str> {
    match profile {
        "localprocess" => Ok(Profile::LocalProcess),
        "production" if runsc_present() => Err("production_driver_absent"),
        "production" => Err("runtime_unavailable"),
        _ => Err("profile"),
    }
}

fn runsc_present() -> bool {
    Path::new("/usr/local/bin/runsc").is_file() || Path::new("/usr/bin/runsc").is_file()
}

pub fn load_config(bytes: &[u8]) -> Result<Config, Fault> {
    if bytes.len() > 1_048_576 {
        return Err(Fault::Malformed("too_large"));
    }
    let config: Config = serde_json::from_slice(bytes).map_err(|_| Fault::Malformed("config"))?;
    if config.contract != CONTRACT {
        return Err(Fault::Malformed("contract"));
    }
    if config.owner_uuid != OWNER_UUID {
        return Err(Fault::Malformed("owner_uuid"));
    }
    if !config.state_dir.starts_with('/') || config.pg.host.is_empty() || config.pg.port == 0 {
        return Err(Fault::Malformed("endpoint"));
    }
    if !config.pg.ca_pem.contains("BEGIN CERTIFICATE") {
        return Err(Fault::Malformed("ca_pem"));
    }
    if config.audience_url.is_empty() || config.bind.is_empty() || config.bindings_path.is_empty() {
        return Err(Fault::Malformed("config"));
    }
    Ok(config)
}

pub fn load_bindings(path: &str) -> Result<Vec<Binding>, Fault> {
    let bytes = fs::read(path).map_err(|_| Fault::Unavailable("bindings"))?;
    let document: BindingFile = serde_json::from_slice(&bytes).map_err(|_| Fault::Malformed("bindings"))?;
    if document.bindings.is_empty() || document.bindings.len() > 32 {
        return Err(Fault::Malformed("bindings"));
    }
    Ok(document.bindings)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingFile {
    bindings: Vec<Binding>,
}

pub fn find_binding<'a>(bindings: &'a [Binding], principal_id: &str, project_id: &str) -> Option<&'a Binding> {
    bindings.iter().find(|item| item.principal_id == principal_id && item.project_id == project_id)
}

pub fn claims_hash(claims: &[(String, String)]) -> Result<String, Fault> {
    let mut owned = claims.to_vec();
    owned.sort();
    let body = serde_json::to_vec(&owned).map_err(|_| Fault::Malformed("claims"))?;
    let digest = sha256(&body).ok_or(Fault::Malformed("claims"))?;
    Ok(hex(&digest))
}

pub fn digest_text(text: &str) -> Result<String, Fault> {
    let digest = sha256(text.as_bytes()).ok_or(Fault::Malformed("hash"))?;
    Ok(hex(&digest))
}

fn wipe(value: &mut String) {
    let mut bytes = std::mem::take(value).into_bytes();
    bytes.fill(0);
    std::hint::black_box(bytes);
}

pub fn self_asserted(value: &serde_json::Value) -> bool {
    let Some(object) = value.as_object() else {
        return true;
    };
    const FORBIDDEN: &[&str] = &["principal_id", "project_id", "role", "claims", "claims_hash", "identity"];
    FORBIDDEN.iter().any(|key| object.contains_key(*key))
}
