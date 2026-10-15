use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

#[derive(Clone, Debug)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub runtime_user: String,
    pub runtime_password: String,
    pub runtime_password_file: PathBuf,
    pub admin_user: String,
    pub admin_password: String,
    pub ca_pem: Vec<u8>,
    pub ca_file: Option<PathBuf>,
    pub migrations: PathBuf,
    pub target_database: Option<String>,
    pub c1_target_database: Option<String>,
    pub target_runtime_user: Option<String>,
    pub target_runtime_password: Option<String>,
    pub pipeline_migrations: Option<PathBuf>,
    pub capture_root: Option<PathBuf>,
    pub secret_ref: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct CaptureConfig {
    pub target_database: String,
    pub target_runtime_user: String,
    pub target_runtime_password: String,
    pub pipeline_migrations: PathBuf,
    pub capture_root: PathBuf,
    pub secret_ref: PathBuf,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path).map_err(|_| Error::new("config"))?;
        let mut host = None;
        let mut port = None;
        let mut database = None;
        let mut runtime_user = None;
        let mut runtime_password_file = None;
        let mut admin_user = None;
        let mut admin_password_file = None;
        let mut ca_file = None;
        let mut migrations = None;
        let mut target_database = None;
        let mut c1_target_database = None;
        let mut target_runtime_user = None;
        let mut target_runtime_password_file = None;
        let mut pipeline_migrations = None;
        let mut capture_root = None;
        let mut secret_ref = None;
        for line in text.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(Error::new("config"));
            };
            if value.is_empty() {
                return Err(Error::new("config"));
            }
            match key {
                "host" => host = Some(value.to_string()),
                "port" => {
                    port = Some(value.parse::<u16>().map_err(|_| Error::new("config"))?);
                }
                "database" => database = Some(value.to_string()),
                "runtime_user" => runtime_user = Some(value.to_string()),
                "runtime_password_file" => runtime_password_file = Some(PathBuf::from(value)),
                "admin_user" => admin_user = Some(value.to_string()),
                "admin_password_file" => admin_password_file = Some(PathBuf::from(value)),
                "ca_file" => ca_file = Some(PathBuf::from(value)),
                "migrations" => migrations = Some(PathBuf::from(value)),
                "target_database" => target_database = Some(value.to_string()),
                "c1_target_database" => c1_target_database = Some(value.to_string()),
                "target_runtime_user" => target_runtime_user = Some(value.to_string()),
                "target_runtime_password_file" => {
                    target_runtime_password_file = Some(PathBuf::from(value));
                }
                "pipeline_migrations" => pipeline_migrations = Some(PathBuf::from(value)),
                "capture_root" => capture_root = Some(PathBuf::from(value)),
                "secret_ref" => secret_ref = Some(PathBuf::from(value)),
                _ => return Err(Error::new("config")),
            }
        }
        let runtime_password = read_secret(runtime_password_file.as_deref())?;
        let admin_password = read_secret(admin_password_file.as_deref())?;
        let ca_pem = match ca_file.as_deref() {
            Some(path) => fs::read(path).map_err(|_| Error::new("config"))?,
            None if host
                .as_deref()
                .is_some_and(|h| h.starts_with('/') && !h.contains("..")) =>
            {
                Vec::new()
            }
            None => return Err(Error::new("config")),
        };
        Ok(Self {
            host: host.ok_or_else(|| Error::new("config"))?,
            port: port.ok_or_else(|| Error::new("config"))?,
            database: database.ok_or_else(|| Error::new("config"))?,
            runtime_user: runtime_user.ok_or_else(|| Error::new("config"))?,
            runtime_password,
            runtime_password_file: runtime_password_file.ok_or_else(|| Error::new("config"))?,
            admin_user: admin_user.ok_or_else(|| Error::new("config"))?,
            admin_password,
            ca_pem,
            ca_file,
            migrations: migrations.ok_or_else(|| Error::new("config"))?,
            target_database,
            c1_target_database,
            target_runtime_user,
            target_runtime_password: match target_runtime_password_file {
                Some(path) => Some(read_secret(Some(path.as_path()))?),
                None => None,
            },
            pipeline_migrations,
            capture_root,
            secret_ref,
        })
    }

    pub fn capture(&self) -> Result<CaptureConfig> {
        Ok(CaptureConfig {
            target_database: self
                .target_database
                .clone()
                .ok_or_else(|| Error::new("config"))?,
            target_runtime_user: self
                .target_runtime_user
                .clone()
                .ok_or_else(|| Error::new("config"))?,
            target_runtime_password: self
                .target_runtime_password
                .clone()
                .ok_or_else(|| Error::new("config"))?,
            pipeline_migrations: self
                .pipeline_migrations
                .clone()
                .ok_or_else(|| Error::new("config"))?,
            capture_root: self
                .capture_root
                .clone()
                .ok_or_else(|| Error::new("config"))?,
            secret_ref: self
                .secret_ref
                .clone()
                .ok_or_else(|| Error::new("config"))?,
        })
    }
}

fn read_secret(path: Option<&std::path::Path>) -> Result<String> {
    let path = path.ok_or_else(|| Error::new("config"))?;
    let mut text = fs::read_to_string(path).map_err(|_| Error::new("config"))?;
    if text.ends_with('\n') {
        text.pop();
    }
    if !(16..=128).contains(&text.len()) || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::new("config"));
    }
    Ok(text)
}
