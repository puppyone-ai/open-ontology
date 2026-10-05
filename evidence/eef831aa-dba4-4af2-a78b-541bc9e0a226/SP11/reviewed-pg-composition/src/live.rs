//! Process environment. Passwords are read from hex files and are not logged.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::stop::{disk, Stop};

pub const SOURCE_ID: &str = "a226src";
pub const SOURCE_DB: &str = "oo_a226_src";
pub const TARGET_DB: &str = "oo_a226_dst";
pub const READER: &str = "oo_a226_reader";
pub const WRITER: &str = "oo_a226_writer";
pub const RUNTIME: &str = "oo_a226_runtime";
pub const LOGIN: &str = "oo_a226_login";
pub const SCHEMA: &str = "shop";
pub const OWNER: &str = "eef831aa-dba4-4af2-a78b-541bc9e0a226";
pub const SHARD: &str = "a226main";
pub const GENERATION: i64 = 26;
pub const FIXED: &str = "ed661809cf6c1e8df37af5842838dd1eb839c98a";

pub struct Live {
    pub evidence: PathBuf,
    pub host: String,
    pub port: u16,
    pub admin_user: String,
    pub admin_password: String,
    pub reader_password: String,
    pub writer_password: String,
    pub runtime_password: String,
    pub login_password: String,
    pub ca: Vec<u8>,
    pub pipeline: PathBuf,
    pub builder: PathBuf,
    pub query: PathBuf,
    pub init_pg: PathBuf,
    pub reviewed: PathBuf,
    pub bundle: PathBuf,
    pub fixture: PathBuf,
    pub bundle_dir: PathBuf,
}

impl Live {
    pub fn load() -> Result<Self, Stop> {
        let port: u16 = env_var("OO_A226_PORT")?
            .parse()
            .map_err(|_| Stop::new("env", "OO_A226_PORT"))?;
        let ca = fs::read(env_var("OO_A226_CA_FILE")?).map_err(|err| disk("ca", &err))?;
        Ok(Self {
            evidence: PathBuf::from(env_var("OO_A226_EVIDENCE")?),
            host: env_var("OO_A226_HOST")?,
            port,
            admin_user: env_var("OO_A226_ADMIN_USER")?,
            admin_password: read_hex(&env_var("OO_A226_ADMIN_PASSWORD_FILE")?)?,
            reader_password: read_hex(&env_var("OO_A226_READER_PASSWORD_FILE")?)?,
            writer_password: read_hex(&env_var("OO_A226_WRITER_PASSWORD_FILE")?)?,
            runtime_password: read_hex(&env_var("OO_A226_RUNTIME_PASSWORD_FILE")?)?,
            login_password: read_hex(&env_var("OO_A226_LOGIN_PASSWORD_FILE")?)?,
            ca,
            pipeline: PathBuf::from(env_var("OO_A226_PIPELINE")?),
            builder: PathBuf::from(env_var("OO_A226_BUILDER")?),
            query: PathBuf::from(env_var("OO_A226_QUERY")?),
            init_pg: PathBuf::from(env_var("OO_A226_INIT_PG")?),
            reviewed: PathBuf::from(env_var("OO_A226_REVIEWED_EFFECTIVE")?),
            bundle: PathBuf::from(env_var("OO_A226_ROLE_PACK_BUNDLE")?),
            fixture: PathBuf::from(env_var("OO_A226_FIXTURE")?),
            bundle_dir: PathBuf::from(env_var("OO_A226_BUNDLE_DIR")?),
        })
    }

    pub fn passwords(&self) -> [&str; 5] {
        [
            self.admin_password.as_str(),
            self.reader_password.as_str(),
            self.writer_password.as_str(),
            self.runtime_password.as_str(),
            self.login_password.as_str(),
        ]
    }
}

fn env_var(name: &str) -> Result<String, Stop> {
    env::var(name).map_err(|_| Stop::new("env", name))
}

fn read_hex(path: &str) -> Result<String, Stop> {
    let text = fs::read_to_string(path).map_err(|err| disk("secret_file", &err))?;
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() || !trimmed.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(Stop::new("secret_file", "password file was rejected"));
    }
    Ok(trimmed)
}

pub fn ensure_dir(path: &Path) -> Result<(), Stop> {
    fs::create_dir_all(path).map_err(|err| disk("evidence", &err))
}
