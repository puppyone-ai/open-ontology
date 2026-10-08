//! File configuration. `key_source=kms` refuses before any key file is opened.

use std::fs;
use std::net::IpAddr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use serde::Deserialize;
use zeroize::Zeroizing;

use crate::error::Fault;
use crate::ids::{deployment_ok, ident_ok, path_ok};

#[derive(Clone)]
pub struct Config {
    pub https_origin: String,
    pub listen_host: IpAddr,
    pub management_port: u16,
    pub component_port: u16,
    pub tls_cert_pem: Vec<u8>,
    pub tls_key_pem: Zeroizing<Vec<u8>>,
    pub client_ca_pem: Vec<u8>,
    pub identity_origin: String,
    pub identity_ca_pem: Vec<u8>,
    pub deployment: String,
    pub master_key: Zeroizing<Vec<u8>>,
    pub pg_host: String,
    pub pg_port: u16,
    pub pg_database: String,
    pub pg_user: String,
    pub pg_password: Zeroizing<String>,
    pub pg_ca_pem: Vec<u8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    https_origin: String,
    listen_host: String,
    management_port: u16,
    component_port: u16,
    tls_cert_file: String,
    tls_key_file: String,
    client_ca_file: String,
    identity_origin: String,
    identity_ca_file: String,
    deployment: String,
    key_source: String,
    master_key_file: String,
    pg: FilePg,
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

pub fn load_config(path: &Path) -> Result<Config, Fault> {
    let bytes = read_regular(path, 65_536, false)?;
    if bytes.contains(&0) {
        return Err(Fault::malformed());
    }
    let file: FileConfig = serde_json::from_slice(&bytes).map_err(|_| Fault::malformed())?;
    if file.key_source == "kms" {
        return Err(Fault::kms());
    }
    if file.key_source != "file" {
        return Err(Fault::malformed());
    }
    if file.pg.user != "oo_secret_admin" || !ident_ok(&file.pg.database) || !host_ok(&file.pg.host) || file.pg.port == 0 {
        return Err(Fault::malformed());
    }
    if !deployment_ok(&file.deployment) {
        return Err(Fault::malformed());
    }
    if file.management_port != 0 && file.management_port == file.component_port {
        return Err(Fault::malformed());
    }
    let listen_host: IpAddr = file.listen_host.parse().map_err(|_| Fault::malformed())?;
    let https_origin = exact_origin(&file.https_origin)?;
    let identity_origin = exact_origin(&file.identity_origin)?;
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let tls_cert_pem = read_certificate(&resolve(directory, &file.tls_cert_file)?)?;
    let tls_key_pem = Zeroizing::new(read_private_key(&resolve(directory, &file.tls_key_file)?)?);
    let client_ca_pem = read_certificate(&resolve(directory, &file.client_ca_file)?)?;
    let identity_ca_pem = read_certificate(&resolve(directory, &file.identity_ca_file)?)?;
    let pg_ca_pem = read_certificate(&resolve(directory, &file.pg.ca_file)?)?;
    let pg_password = Zeroizing::new(read_password(&resolve(directory, &file.pg.password_file)?)?);
    let master_key = Zeroizing::new(read_master_key(&resolve(directory, &file.master_key_file)?)?);
    Ok(Config {
        https_origin,
        listen_host,
        management_port: file.management_port,
        component_port: file.component_port,
        tls_cert_pem,
        tls_key_pem,
        client_ca_pem,
        identity_origin,
        identity_ca_pem,
        deployment: file.deployment,
        master_key,
        pg_host: file.pg.host,
        pg_port: file.pg.port,
        pg_database: file.pg.database,
        pg_user: file.pg.user,
        pg_password,
        pg_ca_pem,
    })
}

fn exact_origin(value: &str) -> Result<String, Fault> {
    if value.starts_with("https://") && value.len() > "https://".len() && value.len() <= 256 && !value[8..].contains(['/', '?', '#', ' ']) {
        Ok(value.to_string())
    } else {
        Err(Fault::malformed())
    }
}

fn host_ok(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
}

fn resolve(directory: &Path, value: &str) -> Result<PathBuf, Fault> {
    if !path_ok(value) {
        return Err(Fault::malformed());
    }
    let path = Path::new(value);
    if path.is_absolute() { Ok(path.to_path_buf()) } else { Ok(directory.join(path)) }
}

fn read_regular(path: &Path, limit: usize, private: bool) -> Result<Vec<u8>, Fault> {
    let metadata = fs::symlink_metadata(path).map_err(|_| Fault::malformed())?;
    if !metadata.file_type().is_file() || metadata.len() == 0 || metadata.len() > limit as u64 {
        return Err(Fault::malformed());
    }
    if private && metadata.permissions().mode() & 0o777 != 0o600 {
        return Err(Fault::malformed());
    }
    let bytes = fs::read(path).map_err(|_| Fault::malformed())?;
    if bytes.is_empty() || bytes.len() > limit {
        return Err(Fault::malformed());
    }
    Ok(bytes)
}

fn read_certificate(path: &Path) -> Result<Vec<u8>, Fault> {
    let bytes = read_regular(path, 65_536, false)?;
    if bytes.contains(&0) {
        return Err(Fault::malformed());
    }
    CertificateDer::from_pem_slice(&bytes).map_err(|_| Fault::malformed())?;
    Ok(bytes)
}

fn read_private_key(path: &Path) -> Result<Vec<u8>, Fault> {
    let bytes = read_regular(path, 65_536, true)?;
    if bytes.contains(&0) {
        return Err(Fault::malformed());
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| Fault::malformed())?;
    if text.contains("ENCRYPTED PRIVATE KEY") || !text.contains("PRIVATE KEY") {
        return Err(Fault::malformed());
    }
    PrivateKeyDer::from_pem_slice(&bytes).map_err(|_| Fault::malformed())?;
    Ok(bytes)
}

fn read_password(path: &Path) -> Result<String, Fault> {
    let bytes = read_regular(path, 512, true)?;
    if bytes.contains(&0) {
        return Err(Fault::malformed());
    }
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

fn read_master_key(path: &Path) -> Result<Vec<u8>, Fault> {
    let bytes = read_regular(path, 32, true)?;
    if bytes.len() != 32 {
        return Err(Fault::malformed());
    }
    Ok(bytes)
}
