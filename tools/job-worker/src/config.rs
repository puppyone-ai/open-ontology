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
    pub admin_user: String,
    pub admin_password: String,
    pub ca_pem: Vec<u8>,
    pub migrations: PathBuf,
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
                _ => return Err(Error::new("config")),
            }
        }
        let runtime_password = read_secret(runtime_password_file.as_deref())?;
        let admin_password = read_secret(admin_password_file.as_deref())?;
        let ca_pem = fs::read(ca_file.ok_or_else(|| Error::new("config"))?).map_err(|_| Error::new("config"))?;
        Ok(Self {
            host: host.ok_or_else(|| Error::new("config"))?,
            port: port.ok_or_else(|| Error::new("config"))?,
            database: database.ok_or_else(|| Error::new("config"))?,
            runtime_user: runtime_user.ok_or_else(|| Error::new("config"))?,
            runtime_password,
            admin_user: admin_user.ok_or_else(|| Error::new("config"))?,
            admin_password,
            ca_pem,
            migrations: migrations.ok_or_else(|| Error::new("config"))?,
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
