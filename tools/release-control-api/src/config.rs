//! Operator file. Passwords and CA bytes are read from named files.

use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::error::Fault;
use crate::ids::{ident_ok, origin_ok, token_ok};
use crate::jsonutil::parse_document;

pub struct PgEndpoint {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password: String,
    pub ca_pem: Vec<u8>,
}

pub struct Config {
    pub https_origin: String,
    pub listen_host: IpAddr,
    pub listen_port: u16,
    pub tls_cert_pem: Vec<u8>,
    pub tls_key_pem: Vec<u8>,
    pub identity_origin: String,
    pub identity_ca_pem: Vec<u8>,
    pub deployment_origins: Vec<String>,
    pub pack_dir: PathBuf,
    pub project_id: String,
    pub authority_id: String,
    pub api_pg: PgEndpoint,
    pub producer_pg: PgEndpoint,
    pub reader_pg: PgEndpoint,
}

pub fn load_config(path: &Path) -> Result<Config, Fault> {
    let bytes = read_limited(path, 65_536)?;
    let value = parse_document(&bytes).map_err(|_| Fault::malformed())?;
    let object = value.as_object().ok_or(Fault::malformed())?;
    require_keys(
        object,
        &[
            "api_pg",
            "authority_id",
            "deployment_origins",
            "https_origin",
            "identity_ca_file",
            "identity_origin",
            "listen_host",
            "listen_port",
            "pack_dir",
            "producer_pg",
            "project_id",
            "reader_pg",
            "tls_cert_file",
            "tls_key_file",
        ],
    )?;
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let https_origin = require_str(object, "https_origin")?;
    if !origin_ok(&https_origin) {
        return Err(Fault::malformed());
    }
    let identity_origin = require_str(object, "identity_origin")?;
    if !origin_ok(&identity_origin) {
        return Err(Fault::malformed());
    }
    let listen_host: IpAddr = require_str(object, "listen_host")?.parse().map_err(|_| Fault::malformed())?;
    let listen_port = require_port(object, "listen_port")?;
    let project_id = require_str(object, "project_id")?;
    let authority_id = require_str(object, "authority_id")?;
    if !ident_ok(&project_id) || !token_ok(&authority_id) {
        return Err(Fault::malformed());
    }
    let deployment_origins = origins(object, &https_origin)?;
    let pack_dir = resolve(directory, &require_str(object, "pack_dir")?);
    if !pack_dir.is_dir() {
        return Err(Fault::malformed());
    }
    let tls_cert_pem = read_limited(&resolve(directory, &require_str(object, "tls_cert_file")?), 65_536)?;
    let tls_key_pem = read_limited(&resolve(directory, &require_str(object, "tls_key_file")?), 65_536)?;
    let identity_ca_pem = read_limited(&resolve(directory, &require_str(object, "identity_ca_file")?), 65_536)?;
    let api_pg = endpoint(directory, object.get("api_pg"), "oo_release_api_admin")?;
    let producer_pg = endpoint(directory, object.get("producer_pg"), "oo_release_admin")?;
    let reader_pg = endpoint(directory, object.get("reader_pg"), "oo_release_reader")?;
    if api_pg.database != producer_pg.database || api_pg.database != reader_pg.database {
        return Err(Fault::malformed());
    }
    Ok(Config {
        https_origin,
        listen_host,
        listen_port,
        tls_cert_pem,
        tls_key_pem,
        identity_origin,
        identity_ca_pem,
        deployment_origins,
        pack_dir,
        project_id,
        authority_id,
        api_pg,
        producer_pg,
        reader_pg,
    })
}

fn endpoint(directory: &Path, value: Option<&Value>, user: &str) -> Result<PgEndpoint, Fault> {
    let object = value.and_then(Value::as_object).ok_or(Fault::malformed())?;
    require_keys(object, &["ca_file", "database", "host", "password_file", "port", "user"])?;
    let got = require_str(object, "user")?;
    let database = require_str(object, "database")?;
    let host = require_str(object, "host")?;
    if got != user || !ident_ok(&database) || host.is_empty() || host.len() > 253 || host.as_bytes().contains(&0) {
        return Err(Fault::malformed());
    }
    let password = read_secret(&resolve(directory, &require_str(object, "password_file")?))?;
    let ca_pem = read_limited(&resolve(directory, &require_str(object, "ca_file")?), 65_536)?;
    if !ca_text(&ca_pem) {
        return Err(Fault::malformed());
    }
    Ok(PgEndpoint { host, port: require_port(object, "port")?, database, user: got, password, ca_pem })
}

fn origins(object: &Map<String, Value>, https_origin: &str) -> Result<Vec<String>, Fault> {
    let values = object.get("deployment_origins").and_then(Value::as_array).ok_or(Fault::malformed())?;
    if values.is_empty() || values.len() > 8 {
        return Err(Fault::malformed());
    }
    let mut out = Vec::new();
    for value in values {
        let origin = value.as_str().ok_or(Fault::malformed())?;
        if !origin_ok(origin) {
            return Err(Fault::malformed());
        }
        if out.iter().any(|item: &String| item == origin) {
            return Err(Fault::malformed());
        }
        out.push(origin.to_owned());
    }
    if !out.iter().any(|item| item == https_origin) {
        return Err(Fault::malformed());
    }
    Ok(out)
}

fn require_keys(object: &Map<String, Value>, keys: &[&str]) -> Result<(), Fault> {
    if object.len() != keys.len() || keys.iter().any(|key| !object.contains_key(*key)) {
        Err(Fault::malformed())
    } else {
        Ok(())
    }
}

fn require_str(object: &Map<String, Value>, key: &str) -> Result<String, Fault> {
    object.get(key).and_then(Value::as_str).filter(|text| !text.is_empty()).map(str::to_owned).ok_or(Fault::malformed())
}

fn require_port(object: &Map<String, Value>, key: &str) -> Result<u16, Fault> {
    let value = object.get(key).and_then(Value::as_u64).ok_or(Fault::malformed())?;
    u16::try_from(value).map_err(|_| Fault::malformed())
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
    if bytes.is_empty() || bytes.len() > limit { Err(Fault::malformed()) } else { Ok(bytes) }
}

fn read_secret(path: &Path) -> Result<String, Fault> {
    let bytes = read_limited(path, 512)?;
    let mut text = String::from_utf8(bytes).map_err(|_| Fault::malformed())?;
    if text.ends_with('\n') {
        text.pop();
    }
    if text.is_empty() || text.bytes().any(|byte| byte.is_ascii_whitespace() || !byte.is_ascii()) {
        return Err(Fault::malformed());
    }
    Ok(text)
}

fn ca_text(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_ok_and(|text| text.contains("BEGIN CERTIFICATE"))
}
