//! Operator config. Passwords stay in files. The file bytes are not logged.

use std::collections::BTreeMap;
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::error::Fault;
use crate::ids::{hex64, host_ok, ident_ok, plain_token, secret_ref_ok, uuid_ok};
use crate::jsonutil::{self, parse_exact};
use crate::sha256::{contains_secret, sha256_hex};

#[derive(Clone, Debug)]
pub struct Pin {
    pub path: PathBuf,
    pub sha256: String,
}

#[derive(Clone, Debug)]
pub struct PgConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password_file: PathBuf,
    pub ca_file: PathBuf,
}

#[derive(Clone, Debug)]
pub struct TargetConfig {
    pub host: String,
    pub port: u16,
    pub pg_database: String,
    pub file_database: String,
    pub administrator: String,
    pub runtime_principal: String,
    pub admin_password_file: PathBuf,
    pub runtime_password_file: PathBuf,
    pub ca_file: PathBuf,
}

#[derive(Clone, Debug)]
pub struct Producers {
    pub pg_onboarding: Pin,
    pub file_import: Pin,
    pub init_pg: Pin,
    pub pipeline: Pin,
    pub builder: Pin,
    pub worker: Pin,
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
    pub pg: PgConfig,
    pub admitted_hosts: Vec<String>,
    pub secrets: BTreeMap<String, PathBuf>,
    pub upload_max_bytes: usize,
    pub object_dir: PathBuf,
    pub run_dir: PathBuf,
    pub owner: String,
    pub comment_markers: Vec<String>,
    pub producers: Producers,
    pub target: TargetConfig,
    pub secret_values: Vec<Vec<u8>>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, Fault> {
        let bytes = fs::read(path).map_err(|_| Fault::Malformed)?;
        if bytes.len() > 1_048_576 {
            return Err(Fault::Malformed);
        }
        let value = parse_exact(&bytes).map_err(|_| Fault::Malformed)?;
        let root = jsonutil::object(
            &value,
            &[
                "protocol",
                "https_origin",
                "listen_host",
                "listen_port",
                "tls_cert_file",
                "tls_key_file",
                "identity_origin",
                "identity_ca_file",
                "deployment_origins",
                "pg",
                "admitted_hosts",
                "secrets",
                "upload_max_bytes",
                "object_dir",
                "run_dir",
                "owner",
                "comment_markers",
                "producers",
                "target",
            ],
        )
        .map_err(|_| Fault::Malformed)?;
        if root.get("protocol").and_then(Value::as_str) != Some("oo.source-control-api/v1") {
            return Err(Fault::Malformed);
        }
        let https_origin = origin(jsonutil::require_str(root, "https_origin").map_err(|_| Fault::Malformed)?)?;
        let listen_host = jsonutil::require_str(root, "listen_host")
            .map_err(|_| Fault::Malformed)?
            .parse::<IpAddr>()
            .map_err(|_| Fault::Malformed)?;
        let listen_port = port(root.get("listen_port").ok_or(Fault::Malformed)?)?;
        let tls_cert_pem = read_file(jsonutil::require_str(root, "tls_cert_file").map_err(|_| Fault::Malformed)?)?;
        let tls_key_pem = read_file(jsonutil::require_str(root, "tls_key_file").map_err(|_| Fault::Malformed)?)?;
        let identity_origin = https_origin_loose(jsonutil::require_str(root, "identity_origin").map_err(|_| Fault::Malformed)?)?;
        let identity_ca_pem = read_file(jsonutil::require_str(root, "identity_ca_file").map_err(|_| Fault::Malformed)?)?;
        let deployment_origins = string_list(root.get("deployment_origins").ok_or(Fault::Malformed)?, origin)?;
        if deployment_origins.is_empty() {
            return Err(Fault::Malformed);
        }
        let pg = pg_config(root.get("pg").ok_or(Fault::Malformed)?)?;
        let admitted_hosts = string_list(root.get("admitted_hosts").ok_or(Fault::Malformed)?, |text| {
            if host_ok(text) {
                Ok(text.to_string())
            } else {
                Err(Fault::Malformed)
            }
        })?;
        if admitted_hosts.is_empty() {
            return Err(Fault::Malformed);
        }
        let secrets = secrets(root.get("secrets").ok_or(Fault::Malformed)?)?;
        let upload_max = jsonutil::bounded_u64(root.get("upload_max_bytes").ok_or(Fault::Malformed)?, 1, 16_777_216)
            .map_err(|_| Fault::Malformed)?;
        let object_dir = absolute_dir(jsonutil::require_str(root, "object_dir").map_err(|_| Fault::Malformed)?)?;
        let run_dir = absolute_dir(jsonutil::require_str(root, "run_dir").map_err(|_| Fault::Malformed)?)?;
        let owner = jsonutil::require_str(root, "owner").map_err(|_| Fault::Malformed)?;
        if !uuid_ok(owner) {
            return Err(Fault::Malformed);
        }
        let comment_markers = markers(root.get("comment_markers").ok_or(Fault::Malformed)?)?;
        let producers = producers(root.get("producers").ok_or(Fault::Malformed)?)?;
        let target = target(root.get("target").ok_or(Fault::Malformed)?)?;
        let mut secret_values = Vec::new();
        for path in secrets.values() {
            secret_values.push(load_secret(path)?);
        }
        secret_values.push(load_secret(&pg.password_file)?);
        secret_values.push(load_secret(&target.admin_password_file)?);
        secret_values.push(load_secret(&target.runtime_password_file)?);
        if contains_secret(&bytes, &secret_values) {
            return Err(Fault::Refused);
        }
        Ok(Self {
            https_origin,
            listen_host,
            listen_port,
            tls_cert_pem,
            tls_key_pem,
            identity_origin,
            identity_ca_pem,
            deployment_origins,
            pg,
            admitted_hosts,
            secrets,
            upload_max_bytes: usize::try_from(upload_max).map_err(|_| Fault::Malformed)?,
            object_dir,
            run_dir,
            owner: owner.to_string(),
            comment_markers,
            producers,
            target,
            secret_values,
        })
    }

    pub fn verify_pins(&self) -> Result<(), Fault> {
        for pin in [
            &self.producers.pg_onboarding,
            &self.producers.file_import,
            &self.producers.init_pg,
            &self.producers.pipeline,
            &self.producers.builder,
            &self.producers.worker,
        ] {
            verify_pin(pin)?;
        }
        Ok(())
    }
}

pub fn verify_pin(pin: &Pin) -> Result<(), Fault> {
    let meta = fs::symlink_metadata(&pin.path).map_err(|_| Fault::Refused)?;
    if !meta.file_type().is_file() || meta.file_type().is_symlink() {
        return Err(Fault::Refused);
    }
    let bytes = fs::read(&pin.path).map_err(|_| Fault::Refused)?;
    if sha256_hex(&bytes) != pin.sha256 {
        return Err(Fault::Refused);
    }
    Ok(())
}

fn producers(value: &Value) -> Result<Producers, Fault> {
    let map = jsonutil::object(
        value,
        &["pg_onboarding", "file_import", "init_pg", "pipeline", "builder", "worker"],
    )
    .map_err(|_| Fault::Malformed)?;
    Ok(Producers {
        pg_onboarding: pin(map.get("pg_onboarding").ok_or(Fault::Malformed)?)?,
        file_import: pin(map.get("file_import").ok_or(Fault::Malformed)?)?,
        init_pg: pin(map.get("init_pg").ok_or(Fault::Malformed)?)?,
        pipeline: pin(map.get("pipeline").ok_or(Fault::Malformed)?)?,
        builder: pin(map.get("builder").ok_or(Fault::Malformed)?)?,
        worker: pin(map.get("worker").ok_or(Fault::Malformed)?)?,
    })
}

fn pin(value: &Value) -> Result<Pin, Fault> {
    let map = jsonutil::object(value, &["path", "sha256"]).map_err(|_| Fault::Malformed)?;
    let path = PathBuf::from(jsonutil::require_str(map, "path").map_err(|_| Fault::Malformed)?);
    let sha256 = jsonutil::require_str(map, "sha256").map_err(|_| Fault::Malformed)?;
    if !path.is_absolute() || !hex64(sha256) {
        return Err(Fault::Malformed);
    }
    Ok(Pin { path, sha256: sha256.to_string() })
}

fn pg_config(value: &Value) -> Result<PgConfig, Fault> {
    let map = jsonutil::object(value, &["host", "port", "database", "user", "password_file", "ca_file"])
        .map_err(|_| Fault::Malformed)?;
    Ok(PgConfig {
        host: token_field(map, "host")?,
        port: port(map.get("port").ok_or(Fault::Malformed)?)?,
        database: ident_field(map, "database")?,
        user: ident_field(map, "user")?,
        password_file: absolute_file(jsonutil::require_str(map, "password_file").map_err(|_| Fault::Malformed)?)?,
        ca_file: absolute_file(jsonutil::require_str(map, "ca_file").map_err(|_| Fault::Malformed)?)?,
    })
}

fn target(value: &Value) -> Result<TargetConfig, Fault> {
    let map = jsonutil::object(
        value,
        &[
            "host",
            "port",
            "pg_database",
            "file_database",
            "administrator",
            "runtime_principal",
            "admin_password_file",
            "runtime_password_file",
            "ca_file",
        ],
    )
    .map_err(|_| Fault::Malformed)?;
    Ok(TargetConfig {
        host: token_field(map, "host")?,
        port: port(map.get("port").ok_or(Fault::Malformed)?)?,
        pg_database: ident_field(map, "pg_database")?,
        file_database: ident_field(map, "file_database")?,
        administrator: ident_field(map, "administrator")?,
        runtime_principal: ident_field(map, "runtime_principal")?,
        admin_password_file: absolute_file(jsonutil::require_str(map, "admin_password_file").map_err(|_| Fault::Malformed)?)?,
        runtime_password_file: absolute_file(jsonutil::require_str(map, "runtime_password_file").map_err(|_| Fault::Malformed)?)?,
        ca_file: absolute_file(jsonutil::require_str(map, "ca_file").map_err(|_| Fault::Malformed)?)?,
    })
}

fn secrets(value: &Value) -> Result<BTreeMap<String, PathBuf>, Fault> {
    let items = value.as_array().ok_or(Fault::Malformed)?;
    let mut out = BTreeMap::new();
    for item in items {
        let map = jsonutil::object(item, &["secret_ref", "file"]).map_err(|_| Fault::Malformed)?;
        let secret_ref = jsonutil::require_str(map, "secret_ref").map_err(|_| Fault::Malformed)?;
        if !secret_ref_ok(secret_ref) || !plain_token(secret_ref) {
            return Err(Fault::Malformed);
        }
        let file = absolute_file(jsonutil::require_str(map, "file").map_err(|_| Fault::Malformed)?)?;
        if out.insert(secret_ref.to_string(), file).is_some() {
            return Err(Fault::Malformed);
        }
    }
    Ok(out)
}

fn markers(value: &Value) -> Result<Vec<String>, Fault> {
    let items = value.as_array().ok_or(Fault::Malformed)?;
    let mut out = Vec::new();
    for item in items {
        let text = item.as_str().ok_or(Fault::Malformed)?;
        if text.is_empty() || text.len() > 64 {
            return Err(Fault::Malformed);
        }
        out.push(text.to_string());
    }
    Ok(out)
}

fn string_list<F>(value: &Value, mut map: F) -> Result<Vec<String>, Fault>
where
    F: FnMut(&str) -> Result<String, Fault>,
{
    let items = value.as_array().ok_or(Fault::Malformed)?;
    let mut out = Vec::new();
    for item in items {
        out.push(map(item.as_str().ok_or(Fault::Malformed)?)?);
    }
    Ok(out)
}

fn origin(text: &str) -> Result<String, Fault> {
    if text.starts_with("https://") && !text[8..].is_empty() && !text[8..].contains('/') {
        Ok(text.to_string())
    } else {
        Err(Fault::Malformed)
    }
}

fn https_origin_loose(text: &str) -> Result<String, Fault> {
    if text.starts_with("https://") && text.len() > 8 && !text[8..].chars().any(|ch| matches!(ch, '/' | '?' | '#')) {
        Ok(text.to_string())
    } else {
        Err(Fault::Malformed)
    }
}

fn port(value: &Value) -> Result<u16, Fault> {
    let number = value.as_u64().ok_or(Fault::Malformed)?;
    if (1..=65535).contains(&number) {
        Ok(number as u16)
    } else {
        Err(Fault::Malformed)
    }
}

fn ident_field(map: &Map<String, Value>, key: &str) -> Result<String, Fault> {
    let text = jsonutil::require_str(map, key).map_err(|_| Fault::Malformed)?;
    if ident_ok(text) {
        Ok(text.to_string())
    } else {
        Err(Fault::Malformed)
    }
}

fn token_field(map: &Map<String, Value>, key: &str) -> Result<String, Fault> {
    let text = jsonutil::require_str(map, key).map_err(|_| Fault::Malformed)?;
    if plain_token(text) && host_ok(text) {
        Ok(text.to_string())
    } else {
        Err(Fault::Malformed)
    }
}

fn absolute_file(text: &str) -> Result<PathBuf, Fault> {
    let path = PathBuf::from(text);
    if path.is_absolute() && plain_token(text) {
        Ok(path)
    } else {
        Err(Fault::Malformed)
    }
}

fn absolute_dir(text: &str) -> Result<PathBuf, Fault> {
    absolute_file(text)
}

fn read_file(text: &str) -> Result<Vec<u8>, Fault> {
    let path = absolute_file(text)?;
    let meta = fs::symlink_metadata(&path).map_err(|_| Fault::Malformed)?;
    if !meta.file_type().is_file() || meta.file_type().is_symlink() || meta.len() > 1_048_576 {
        return Err(Fault::Malformed);
    }
    fs::read(&path).map_err(|_| Fault::Malformed)
}

pub fn load_secret(path: &Path) -> Result<Vec<u8>, Fault> {
    let meta = fs::symlink_metadata(path).map_err(|_| Fault::Refused)?;
    if !meta.file_type().is_file() || meta.file_type().is_symlink() || meta.len() > 1026 {
        return Err(Fault::Refused);
    }
    let mut bytes = fs::read(path).map_err(|_| Fault::Refused)?;
    if bytes.ends_with(b"\r\n") {
        bytes.truncate(bytes.len() - 2);
    } else if bytes.ends_with(b"\n") || bytes.ends_with(b"\r") {
        bytes.pop();
    }
    if bytes.is_empty() || bytes.contains(&0) || bytes.len() > 1024 {
        return Err(Fault::Refused);
    }
    Ok(bytes)
}
