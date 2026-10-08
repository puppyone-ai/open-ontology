//! Operator file for the binding client. It names the identity origin and the local Git repository.

use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use zeroize::Zeroizing;

use crate::error::Fault;
use crate::git::GitSpec;

pub struct Config {
    pub https_origin: String,
    pub listen_host: IpAddr,
    pub listen_port: u16,
    pub tls_cert_pem: Vec<u8>,
    pub tls_key_pem: Zeroizing<Vec<u8>>,
    pub identity_origin: String,
    pub identity_ca_pem: Vec<u8>,
    pub git: GitSpec,
}

pub struct Change {
    pub effect: String,
    pub project_id: String,
    pub subject: String,
    pub role: Option<String>,
    pub expected_revision: i64,
    pub expected_config_commit: String,
    pub expected_git_head: String,
    pub idempotency_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    https_origin: String,
    listen_host: String,
    listen_port: u16,
    tls_cert_file: String,
    tls_key_file: String,
    identity_origin: String,
    identity_ca_file: String,
    git: FileGit,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileGit {
    binary: String,
    root: String,
    reference: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChangeBody {
    effect: String,
    project_id: String,
    subject: String,
    #[serde(default)]
    role: Option<String>,
    expected_revision: i64,
    expected_config_commit: String,
    expected_git_head: String,
    idempotency_key: String,
}

pub fn load_config(path: &Path) -> Result<Config, Fault> {
    let bytes = read_limited(path, 65_536)?;
    let file: FileConfig = serde_json::from_slice(&bytes).map_err(|_| Fault::malformed())?;
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    if file.git.reference != "refs/heads/management" {
        return Err(Fault::malformed());
    }
    let binary = PathBuf::from(&file.git.binary);
    let root = PathBuf::from(&file.git.root);
    if !binary.is_absolute() || !root.is_absolute() || has_whitespace(&file.git.binary) || has_whitespace(&file.git.root) {
        return Err(Fault::malformed());
    }
    let listen_host: IpAddr = file.listen_host.parse().map_err(|_| Fault::malformed())?;
    Ok(Config {
        https_origin: exact_origin(&file.https_origin)?,
        listen_host,
        listen_port: file.listen_port,
        tls_cert_pem: read_pem(&resolve(directory, &file.tls_cert_file), "CERTIFICATE")?,
        tls_key_pem: Zeroizing::new(read_pem(&resolve(directory, &file.tls_key_file), "PRIVATE KEY")?),
        identity_origin: exact_origin(&file.identity_origin)?,
        identity_ca_pem: read_pem(&resolve(directory, &file.identity_ca_file), "CERTIFICATE")?,
        git: GitSpec { binary, root, reference: file.git.reference },
    })
}

pub fn parse_change(bytes: &[u8]) -> Result<Change, Fault> {
    if bytes.len() > 4096 {
        return Err(Fault::malformed());
    }
    let parsed: ChangeBody = serde_json::from_slice(bytes).map_err(|_| Fault::malformed())?;
    if !matches!(parsed.effect.as_str(), "grant" | "revoke") || !key_ok(&parsed.idempotency_key) || !hex64(&parsed.expected_config_commit) || !hex40(&parsed.expected_git_head) {
        return Err(Fault::malformed());
    }
    if parsed.expected_revision < 1 {
        return Err(Fault::malformed());
    }
    match parsed.role.as_deref() {
        None => {}
        Some("admin" | "reviewer" | "steward" | "auditor" | "business_owner") => {}
        Some(_) => return Err(Fault::malformed()),
    }
    if parsed.effect == "grant" && parsed.role.is_none() {
        return Err(Fault::malformed());
    }
    if parsed.effect == "revoke" && parsed.role.is_some() {
        return Err(Fault::malformed());
    }
    Ok(Change {
        effect: parsed.effect,
        project_id: parsed.project_id,
        subject: parsed.subject,
        role: parsed.role,
        expected_revision: parsed.expected_revision,
        expected_config_commit: parsed.expected_config_commit,
        expected_git_head: parsed.expected_git_head,
        idempotency_key: parsed.idempotency_key,
    })
}

pub fn change_value(change: &Change, git_commit: Option<&str>) -> serde_json::Value {
    let mut value = serde_json::json!({
        "effect": change.effect,
        "expected_config_commit": change.expected_config_commit,
        "expected_git_head": change.expected_git_head,
        "expected_revision": change.expected_revision,
        "idempotency_key": change.idempotency_key,
        "project_id": change.project_id,
        "subject": change.subject
    });
    if let Some(role) = &change.role {
        value["role"] = serde_json::Value::String(role.clone());
    }
    if let Some(commit) = git_commit {
        value["git_commit"] = serde_json::Value::String(commit.to_string());
    }
    value
}

fn key_ok(value: &str) -> bool {
    (1..=128).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

fn hex40(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn has_whitespace(value: &str) -> bool {
    value.chars().any(char::is_whitespace)
}

fn exact_origin(value: &str) -> Result<String, Fault> {
    if value.starts_with("https://") && value.len() > "https://".len() && !value[8..].contains('/') && !value.contains(['?', '#', ' ']) {
        Ok(value.to_string())
    } else {
        Err(Fault::malformed())
    }
}

fn resolve(directory: &Path, value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() { path.to_path_buf() } else { directory.join(path) }
}

fn read_limited(path: &Path, limit: usize) -> Result<Vec<u8>, Fault> {
    let metadata = fs::metadata(path).map_err(|_| Fault::malformed())?;
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return Err(Fault::malformed());
    }
    let bytes = fs::read(path).map_err(|_| Fault::malformed())?;
    if bytes.len() > limit { Err(Fault::malformed()) } else { Ok(bytes) }
}

fn read_pem(path: &Path, label: &str) -> Result<Vec<u8>, Fault> {
    let bytes = read_limited(path, 65_536)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| Fault::malformed())?;
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    if text.contains(&begin) && text.contains(&end) { Ok(bytes) } else { Err(Fault::malformed()) }
}

#[cfg(test)]
mod tests {
    use super::parse_change;

    #[test]
    fn caller_authority_json_is_malformed() {
        let body = br#"{"authorized":true,"effect":"grant","project_id":"alpha","subject":"person-reviewer","role":"reviewer","expected_revision":1,"expected_config_commit":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","expected_git_head":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","idempotency_key":"k"}"#;
        assert!(parse_change(body).is_err());
    }

    #[test]
    fn grant_without_a_role_is_malformed() {
        let body = br#"{"effect":"grant","project_id":"alpha","subject":"person-reviewer","expected_revision":1,"expected_config_commit":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","expected_git_head":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","idempotency_key":"k"}"#;
        assert!(parse_change(body).is_err());
    }
}
