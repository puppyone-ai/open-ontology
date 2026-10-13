//! Process config. Secrets are file paths, never arguments.

use std::net::IpAddr;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use zeroize::Zeroizing;

use crate::error::Fault;
use crate::paths::{directory_private, executable_file, read_regular};
use crate::scope::read_key;

pub struct Config {
    pub deployment: String,
    pub listen_host: IpAddr,
    pub listen_port: u16,
    pub https_origin: String,
    pub pg_host: String,
    pub pg_port: u16,
    pub pg_user: String,
    pub pg_database: String,
    pub pg_password: Zeroizing<String>,
    pub pg_ca_pem: Vec<u8>,
    pub tls_cert_pem: Vec<u8>,
    pub tls_key_pem: Zeroizing<Vec<u8>>,
    pub aead_key: Zeroizing<Vec<u8>>,
    pub scope_file: PathBuf,
    pub scope_mac_file: PathBuf,
    pub object_dir: PathBuf,
    pub intake_root: PathBuf,
    pub export_bin: PathBuf,
    pub workspace_enabled: bool,
}

pub fn load_config(path: &Path) -> Result<Config, Fault> {
    let bytes = read_regular(path, 65_536, None)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| Fault::malformed())?;
    let value: Value = serde_json::from_str(text).map_err(|_| Fault::malformed())?;
    let object = value.as_object().ok_or_else(Fault::malformed)?;
    if required(object, "version")? != "oo.artifact-control/v1" {
        return Err(Fault::malformed());
    }
    let workspace_enabled = match object.get("workspace_enabled") {
        None => false,
        Some(Value::Bool(false)) => false,
        Some(Value::Bool(true)) => return Err(Fault::workspace_disabled()),
        Some(_) => return Err(Fault::malformed()),
    };
    let allowed = [
        "version",
        "deployment",
        "listen_host",
        "listen_port",
        "https_origin",
        "pg_host",
        "pg_port",
        "pg_user",
        "pg_database",
        "pg_password_file",
        "pg_ca_file",
        "tls_cert_file",
        "tls_key_file",
        "aead_key_file",
        "scope_file",
        "scope_mac_file",
        "object_dir",
        "intake_root",
        "export_bin",
        "workspace_enabled",
    ];
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) || crate::scope::grant_key(key) {
            return Err(Fault::malformed());
        }
    }
    let deployment = required(object, "deployment")?;
    if !deployment_ok(&deployment) {
        return Err(Fault::malformed());
    }
    let listen_host: IpAddr = required(object, "listen_host")?.parse().map_err(|_| Fault::malformed())?;
    let listen_port = port(object, "listen_port")?;
    let https_origin = required(object, "https_origin")?;
    if !origin_ok(&https_origin) {
        return Err(Fault::malformed());
    }
    let pg_port = port(object, "pg_port")?;
    let password = read_secret(&PathBuf::from(required(object, "pg_password_file")?))?;
    let pg_ca_pem = read_regular(Path::new(&required(object, "pg_ca_file")?), 1_048_576, None)?;
    let tls_cert_pem = read_regular(Path::new(&required(object, "tls_cert_file")?), 1_048_576, None)?;
    let tls_key = read_regular(Path::new(&required(object, "tls_key_file")?), 1_048_576, Some(0o600))?;
    let object_dir = PathBuf::from(required(object, "object_dir")?);
    let intake_root = PathBuf::from(required(object, "intake_root")?);
    let export_bin = PathBuf::from(required(object, "export_bin")?);
    directory_private(&object_dir)?;
    directory_private(&intake_root)?;
    executable_file(&export_bin)?;
    Ok(Config {
        deployment,
        listen_host,
        listen_port,
        https_origin,
        pg_host: required(object, "pg_host")?,
        pg_port,
        pg_user: required(object, "pg_user")?,
        pg_database: required(object, "pg_database")?,
        pg_password: password,
        pg_ca_pem,
        tls_cert_pem,
        tls_key_pem: Zeroizing::new(tls_key),
        aead_key: read_key(Path::new(&required(object, "aead_key_file")?))?,
        scope_file: PathBuf::from(required(object, "scope_file")?),
        scope_mac_file: PathBuf::from(required(object, "scope_mac_file")?),
        object_dir,
        intake_root,
        export_bin,
        workspace_enabled,
    })
}

fn required(object: &Map<String, Value>, key: &str) -> Result<String, Fault> {
    object.get(key).and_then(Value::as_str).filter(|text| !text.is_empty()).map(str::to_owned).ok_or_else(Fault::malformed)
}

fn port(object: &Map<String, Value>, key: &str) -> Result<u16, Fault> {
    let number = object.get(key).and_then(Value::as_u64).ok_or_else(Fault::malformed)?;
    u16::try_from(number).map_err(|_| Fault::malformed())
}

fn deployment_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-')
}

fn origin_ok(value: &str) -> bool {
    value.starts_with("https://")
        && value.len() < 256
        && !value.contains('@')
        && !value.contains('?')
        && !value.contains('#')
        && !value[8..].contains('/')
}

fn read_secret(path: &Path) -> Result<Zeroizing<String>, Fault> {
    let bytes = read_regular(path, 4096, Some(0o600))?;
    let text = std::str::from_utf8(&bytes).map_err(|_| Fault::path())?;
    let text = text.trim_end_matches(['\n', '\r']);
    if text.is_empty() || text.bytes().any(|byte| byte == 0) {
        return Err(Fault::path());
    }
    Ok(Zeroizing::new(text.to_owned()))
}
