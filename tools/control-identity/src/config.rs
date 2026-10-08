//! Operator file. `config_commit` covers the public binding, not the secret files.

use std::fmt;
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Value};
use zeroize::Zeroizing;

use crate::crypto::{sha256_hex, subject_ok};
use crate::error::Fault;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Admin,
    Reviewer,
    Steward,
    Auditor,
    BusinessOwner,
}

impl Role {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Reviewer => "reviewer",
            Self::Steward => "steward",
            Self::Auditor => "auditor",
            Self::BusinessOwner => "business_owner",
        }
    }

    fn parse(value: &str) -> Result<Self, Fault> {
        Ok(match value {
            "admin" => Self::Admin,
            "reviewer" => Self::Reviewer,
            "steward" => Self::Steward,
            "auditor" => Self::Auditor,
            "business_owner" => Self::BusinessOwner,
            _ => return Err(Fault::malformed()),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub subject: String,
    pub project_id: String,
    pub role: Role,
}

pub struct Config {
    pub https_origin: String,
    pub listen_host: IpAddr,
    pub listen_port: u16,
    pub tls_cert_pem: Vec<u8>,
    pub tls_key_pem: Zeroizing<Vec<u8>>,
    pub issuer: String,
    pub audience: String,
    pub client_id: String,
    pub client_secret: Zeroizing<String>,
    pub redirect_uris: Vec<String>,
    pub callback_uri: String,
    pub config_commit: String,
    pub revision: i64,
    pub idp_ca_pem: Vec<u8>,
    pub pg_host: String,
    pub pg_port: u16,
    pub pg_database: String,
    pub pg_user: String,
    pub pg_password: Zeroizing<String>,
    pub pg_ca_pem: Vec<u8>,
    pub bindings: Vec<Binding>,
    pub bootstrap: Binding,
    pub idle_seconds: i64,
    pub absolute_seconds: i64,
    pub oauth_ttl_seconds: i64,
    pub bootstrap_ttl_seconds: i64,
    pub clock_skew_seconds: i64,
}

impl fmt::Debug for Config {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Config")
            .field("https_origin", &self.https_origin)
            .field("issuer", &self.issuer)
            .field("revision", &self.revision)
            .field("config_commit", &self.config_commit)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    https_origin: String,
    listen_host: String,
    listen_port: u16,
    tls_cert_file: String,
    tls_key_file: String,
    issuer: String,
    audience: String,
    client_id: String,
    client_secret_file: String,
    redirect_uris: Vec<String>,
    config_commit: String,
    revision: i64,
    idp_ca_file: String,
    pg: FilePg,
    bindings: Vec<FileBinding>,
    bootstrap: FileBinding,
    idle_seconds: i64,
    absolute_seconds: i64,
    oauth_ttl_seconds: i64,
    bootstrap_ttl_seconds: i64,
    clock_skew_seconds: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FilePg {
    host: String,
    port: u16,
    database: String,
    user: String,
    password_file: String,
    ca_file: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileBinding {
    subject: String,
    project_id: String,
    role: String,
}

pub fn load_config(path: &Path) -> Result<Config, Fault> {
    let bytes = read_limited(path, 65_536)?;
    let file: FileConfig = serde_json::from_slice(&bytes).map_err(|_| Fault::malformed())?;
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let bindings = bindings(&file.bindings)?;
    let bootstrap = one_binding(&file.bootstrap)?;
    if !bindings.iter().any(|item| item == &bootstrap) {
        return Err(Fault::malformed());
    }
    if file.absolute_seconds < file.idle_seconds {
        return Err(Fault::malformed());
    }
    range(file.idle_seconds, 5, 86_400)?;
    range(file.absolute_seconds, 5, 86_400)?;
    range(file.oauth_ttl_seconds, 5, 600)?;
    range(file.bootstrap_ttl_seconds, 5, 1_800)?;
    range(file.clock_skew_seconds, 0, 120)?;
    range(file.revision, 1, 1_000_000_000)?;
    if file.pg.user != "oo_identity_admin" || !ident_ok(&file.pg.database) || file.pg.host.is_empty() || file.pg.port == 0 {
        return Err(Fault::malformed());
    }
    let listen_host: IpAddr = file.listen_host.parse().map_err(|_| Fault::malformed())?;
    let https_origin = exact_origin(&file.https_origin)?;
    let issuer = exact_issuer(&file.issuer)?;
    if file.audience.is_empty() || file.audience.len() > 128 || file.client_id.is_empty() || file.client_id.len() > 128 {
        return Err(Fault::malformed());
    }
    let mut redirect_uris = file.redirect_uris.clone();
    if redirect_uris.is_empty() || !redirect_uris.iter().all(|item| redirect_ok(item)) {
        return Err(Fault::malformed());
    }
    redirect_uris.sort();
    if redirect_uris.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(Fault::malformed());
    }
    let callback_uri = format!("{https_origin}/control/v1/auth/callback");
    if !redirect_uris.iter().any(|item| item == &callback_uri) {
        return Err(Fault::malformed());
    }
    if !hex64(&file.config_commit) {
        return Err(Fault::malformed());
    }
    let commit = canonical_commit(&issuer, &file.audience, &file.client_id, &redirect_uris, &bindings, &bootstrap, file.revision)?;
    if !crate::crypto::hashes_equal(&commit, &file.config_commit) {
        return Err(Fault::malformed());
    }
    let tls_cert_pem = read_pem(&resolve(directory, &file.tls_cert_file), "CERTIFICATE")?;
    let tls_key_pem = Zeroizing::new(read_pem(&resolve(directory, &file.tls_key_file), "PRIVATE KEY")?);
    let idp_ca_pem = read_pem(&resolve(directory, &file.idp_ca_file), "CERTIFICATE")?;
    let pg_ca_pem = read_pem(&resolve(directory, &file.pg.ca_file), "CERTIFICATE")?;
    let client_secret = Zeroizing::new(read_secret(&resolve(directory, &file.client_secret_file))?);
    let pg_password = Zeroizing::new(read_secret(&resolve(directory, &file.pg.password_file))?);
    Ok(Config {
        https_origin,
        listen_host,
        listen_port: file.listen_port,
        tls_cert_pem,
        tls_key_pem,
        issuer,
        audience: file.audience,
        client_id: file.client_id,
        client_secret,
        redirect_uris,
        callback_uri,
        config_commit: file.config_commit,
        revision: file.revision,
        idp_ca_pem,
        pg_host: file.pg.host,
        pg_port: file.pg.port,
        pg_database: file.pg.database,
        pg_user: file.pg.user,
        pg_password,
        pg_ca_pem,
        bindings,
        bootstrap,
        idle_seconds: file.idle_seconds,
        absolute_seconds: file.absolute_seconds,
        oauth_ttl_seconds: file.oauth_ttl_seconds,
        bootstrap_ttl_seconds: file.bootstrap_ttl_seconds,
        clock_skew_seconds: file.clock_skew_seconds,
    })
}

pub fn canonical_commit(
    issuer: &str,
    audience: &str,
    client_id: &str,
    redirect_uris: &[String],
    bindings: &[Binding],
    bootstrap: &Binding,
    revision: i64,
) -> Result<String, Fault> {
    let text = canonical_preimage(issuer, audience, client_id, redirect_uris, bindings, bootstrap, revision)?;
    Ok(sha256_hex(text.as_bytes()))
}

pub fn canonical_preimage(
    issuer: &str,
    audience: &str,
    client_id: &str,
    redirect_uris: &[String],
    bindings: &[Binding],
    bootstrap: &Binding,
    revision: i64,
) -> Result<String, Fault> {
    let mut redirects = redirect_uris.to_vec();
    redirects.sort();
    let mut rows = bindings.to_vec();
    rows.sort_by(|left, right| (&left.project_id, &left.subject, left.role.as_str()).cmp(&(&right.project_id, &right.subject, right.role.as_str())));
    let value = json!({
        "audience": audience,
        "bindings": rows.iter().map(binding_value).collect::<Vec<_>>(),
        "bootstrap": {
            "customer_owner": false,
            "fixture_class": "explicit_test_fixture",
            "project_id": bootstrap.project_id,
            "role": bootstrap.role.as_str(),
            "subject": bootstrap.subject
        },
        "client_id": client_id,
        "issuer": issuer,
        "protocol": "oo.control-identity/config-commit/v1",
        "redirect_uris": redirects,
        "revision": revision
    });
    serde_json::to_string(&value).map_err(|_| Fault::malformed())
}

fn binding_value(binding: &Binding) -> Value {
    json!({
        "project_id": binding.project_id,
        "role": binding.role.as_str(),
        "subject": binding.subject
    })
}

fn bindings(rows: &[FileBinding]) -> Result<Vec<Binding>, Fault> {
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let binding = one_binding(row)?;
        if out.iter().any(|item: &Binding| item.project_id == binding.project_id && item.subject == binding.subject) {
            return Err(Fault::malformed());
        }
        out.push(binding);
    }
    Ok(out)
}

fn one_binding(row: &FileBinding) -> Result<Binding, Fault> {
    if !subject_ok(&row.subject) || !ident_ok(&row.project_id) {
        return Err(Fault::malformed());
    }
    Ok(Binding { subject: row.subject.clone(), project_id: row.project_id.clone(), role: Role::parse(&row.role)? })
}

fn range(value: i64, min: i64, max: i64) -> Result<(), Fault> {
    if (min..=max).contains(&value) { Ok(()) } else { Err(Fault::malformed()) }
}

fn ident_ok(value: &str) -> bool {
    let mut bytes = value.bytes();
    match bytes.next() {
        Some(first) if first.is_ascii_lowercase() => bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_') && value.len() <= 63,
        _ => false,
    }
}

fn exact_origin(value: &str) -> Result<String, Fault> {
    if value.starts_with("https://") && value.len() > "https://".len() && !value[8..].contains('/') && !value.contains(['?', '#', ' ']) {
        Ok(value.to_string())
    } else {
        Err(Fault::malformed())
    }
}

fn exact_issuer(value: &str) -> Result<String, Fault> {
    if value.starts_with("https://") && value.len() > "https://".len() && !value[8..].contains(['/', '?', '#']) {
        Ok(value.to_string())
    } else {
        Err(Fault::malformed())
    }
}

fn redirect_ok(value: &str) -> bool {
    value.starts_with("https://") && !value.contains(['?', '#']) && value.len() > "https://".len()
}

fn hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn resolve(directory: &Path, value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() { path.to_path_buf() } else { directory.join(path) }
}

fn read_limited(path: &Path, limit: usize) -> Result<Vec<u8>, Fault> {
    let metadata = fs::metadata(path).map_err(|_| Fault::malformed())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > limit as u64 {
        return Err(Fault::malformed());
    }
    let bytes = fs::read(path).map_err(|_| Fault::malformed())?;
    if bytes.is_empty() || bytes.len() > limit || bytes.contains(&0) {
        return Err(Fault::malformed());
    }
    Ok(bytes)
}

fn read_pem(path: &Path, label: &str) -> Result<Vec<u8>, Fault> {
    let bytes = read_limited(path, 65_536)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| Fault::malformed())?;
    if !text.contains(&format!("BEGIN {label}")) || text.contains("ENCRYPTED PRIVATE KEY") {
        return Err(Fault::malformed());
    }
    Ok(bytes)
}

fn read_secret(path: &Path) -> Result<String, Fault> {
    let bytes = read_limited(path, 512)?;
    let mut text = String::from_utf8(bytes).map_err(|_| Fault::malformed())?;
    if text.ends_with('\n') {
        text.pop();
        if text.ends_with('\r') {
            text.pop();
        }
    }
    if text.is_empty() || text.len() > 256 || text.bytes().any(|byte| byte.is_ascii_whitespace() || !byte.is_ascii()) {
        return Err(Fault::malformed());
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::{canonical_preimage, Binding, Role};

    #[test]
    fn canonical_preimage_is_sorted_compact_json() {
        let binding = Binding { subject: "sub".to_string(), project_id: "alpha".to_string(), role: Role::Admin };
        let text = canonical_preimage(
            "https://idp.example",
            "aud",
            "client",
            &["https://control.example/control/v1/auth/callback".to_string()],
            std::slice::from_ref(&binding),
            &binding,
            1,
        )
        .expect("preimage");
        assert_eq!(
            text,
            concat!(
                r#"{"audience":"aud","bindings":[{"project_id":"alpha","role":"admin","subject":"sub"}],"#,
                r#""bootstrap":{"customer_owner":false,"fixture_class":"explicit_test_fixture","project_id":"alpha","role":"admin","subject":"sub"},"#,
                r#""client_id":"client","issuer":"https://idp.example","protocol":"oo.control-identity/config-commit/v1","#,
                r#""redirect_uris":["https://control.example/control/v1/auth/callback"],"revision":1}"#
            )
        );
    }
}
