//! The Agent DB the projection writes into (D83): where it is, and how a connection to it is opened.
//!
//! The location is a secret REFERENCE, never a URL in a config file or an environment variable that gets logged:
//! `OO_DOCS_AGENT_DATABASE_URL_REF` (read at start), or the row of `_oo_docs.agent_db_binding` (set through
//! `PUT /v1/docs/agent-db-binding`, so the CLI can bind it without restarting the engine). A reference is
//! `env:NAME`, `file:/absolute/path` or a name in the secrets directory (`OO_DOCS_SECRETS_DIR`, else
//! `$HOME/.config/open-ontology/secrets`, the same directory the Drive credentials use). Its content is the
//! connection URL of an Agent DB login that may create and write `_oo_docproj` and read the linked tables.
//! Without a binding nothing here runs and the engine behaves as before (documents only).

use std::path::PathBuf;
use std::str::FromStr;

use sqlx::Connection;
use sqlx::postgres::{PgConnectOptions, PgConnection};

/// Why the Agent DB could not be used. No text of the URL or of the server ever ends up here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentDbError {
    /// The reference is not a valid reference.
    BadRef,
    /// The reference does not resolve (unset variable, missing file).
    Unresolved,
    /// The URL is not a Postgres URL.
    BadUrl,
    /// The server did not accept the connection.
    Connect,
}

impl AgentDbError {
    /// Stable code for status and logs.
    pub fn code(self) -> &'static str {
        match self {
            Self::BadRef => "agent_db_bad_ref",
            Self::Unresolved => "agent_db_unresolved",
            Self::BadUrl => "agent_db_bad_url",
            Self::Connect => "agent_db_connect",
        }
    }
}

/// Is `r` a secret reference: `env:NAME`, `file:/abs/path` or a plain name?
pub fn valid_ref(r: &str) -> bool {
    if r.is_empty() || r.len() > 300 || r.chars().any(char::is_control) {
        return false;
    }
    if let Some(n) = r.strip_prefix("env:") {
        return (1..=128).contains(&n.len())
            && n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            && !n.as_bytes()[0].is_ascii_digit();
    }
    if let Some(p) = r.strip_prefix("file:") {
        return p.starts_with('/') && !p.split('/').any(|s| s == "..");
    }
    (1..=128).contains(&r.len())
        && r.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        && !r.starts_with('.')
}

/// Where the secrets live and the reference the process was started with.
#[derive(Clone, Debug, Default)]
pub struct AgentDbConfig {
    pub secrets_dir: PathBuf,
    /// `OO_DOCS_AGENT_DATABASE_URL_REF`; wins over the stored binding.
    pub env_ref: Option<String>,
}

impl AgentDbConfig {
    /// From the environment (the engine process).
    pub fn from_env() -> Self {
        let secrets_dir = std::env::var_os("OO_DOCS_SECRETS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                    .join(".config/open-ontology/secrets")
            });
        let env_ref = std::env::var("OO_DOCS_AGENT_DATABASE_URL_REF")
            .ok()
            .filter(|r| valid_ref(r));
        Self {
            secrets_dir,
            env_ref,
        }
    }

    /// The reference in force: the environment's, else the stored binding.
    pub async fn binding(&self, docs: &mut PgConnection) -> Result<Option<String>, sqlx::Error> {
        if let Some(r) = &self.env_ref {
            return Ok(Some(r.clone()));
        }
        sqlx::query_scalar("SELECT secret_ref FROM _oo_docs.agent_db_binding")
            .fetch_optional(&mut *docs)
            .await
    }

    /// The connection URL a reference names.
    pub fn resolve(&self, reference: &str) -> Result<String, AgentDbError> {
        if !valid_ref(reference) {
            return Err(AgentDbError::BadRef);
        }
        let text = if let Some(name) = reference.strip_prefix("env:") {
            std::env::var(name).map_err(|_| AgentDbError::Unresolved)?
        } else {
            let path = match reference.strip_prefix("file:") {
                Some(p) => PathBuf::from(p),
                None => self.secrets_dir.join(reference),
            };
            std::fs::read_to_string(path).map_err(|_| AgentDbError::Unresolved)?
        };
        let url = text.trim().to_owned();
        if url.is_empty() {
            return Err(AgentDbError::Unresolved);
        }
        Ok(url)
    }

    /// Open one connection to the Agent DB named by `reference`.
    pub async fn connect(&self, reference: &str) -> Result<PgConnection, AgentDbError> {
        let url = self.resolve(reference)?;
        let opts = PgConnectOptions::from_str(&url).map_err(|_| AgentDbError::BadUrl)?;
        PgConnection::connect_with(&opts)
            .await
            .map_err(|_| AgentDbError::Connect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_are_env_file_or_a_name() {
        for ok in [
            "env:OO_AGENT_DB",
            "file:/run/secrets/agent-db",
            "agentdb.main",
            "a-b_c",
        ] {
            assert!(valid_ref(ok), "{ok}");
        }
        for bad in [
            "",
            "env:",
            "env:1X",
            "env:A B",
            "file:relative/path",
            "file:/a/../b",
            "../x",
            ".hidden",
            "a/b",
            "postgres://u:p@h/db",
        ] {
            assert!(!valid_ref(bad), "{bad}");
        }
    }

    #[test]
    fn a_name_resolves_inside_the_secrets_directory_only() {
        let dir = std::env::temp_dir().join(format!("oo-agentdb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("agentdb"), "postgres://u@127.0.0.1:1/x\n").unwrap();
        let cfg = AgentDbConfig {
            secrets_dir: dir.clone(),
            env_ref: None,
        };
        assert_eq!(
            cfg.resolve("agentdb").unwrap(),
            "postgres://u@127.0.0.1:1/x"
        );
        assert_eq!(cfg.resolve("missing"), Err(AgentDbError::Unresolved));
        assert_eq!(cfg.resolve("../agentdb"), Err(AgentDbError::BadRef));
        let _ = std::fs::remove_dir_all(dir);
    }
}
