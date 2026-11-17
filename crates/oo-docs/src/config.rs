//! Engine configuration (10 §2.8, `oo.docs/v1`). Board WS-BE5.
//!
//! Two sources: process environment for what the daemon decides (listen
//! address, database URLs, token files, state directory) and the optional
//! `<state>/docs.json` for the limits Desktop may tune. Secrets are never
//! values here: the database URLs carry no password in the trust setups used
//! for tests, and tokens are read from 0600 files, not from the environment.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

use crate::ingest::IngestLimits;
use crate::ingest::poll::PollConfig;
use crate::ingest::sniff::Format;
use crate::jobs::JobKind;

/// Why the configuration is unusable.
#[derive(Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// A required variable is absent.
    Missing(&'static str),
    /// A value does not parse.
    Invalid(&'static str, String),
    /// `docs.json` has another `version`.
    Version(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing(k) => write!(f, "{k} is required"),
            Self::Invalid(k, why) => write!(f, "{k} is invalid: {why}"),
            Self::Version(v) => write!(
                f,
                "docs.json version {v:?} is not {}",
                crate::CONFIG_PROTOCOL
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Everything the engine process needs to start.
#[derive(Clone, Debug)]
pub struct EngineConfig {
    /// `OO_DOCS_LISTEN`, default `127.0.0.1:0`.
    pub listen: SocketAddr,
    /// `OO_DOCS_STATE_DIR`.
    pub state_dir: PathBuf,
    /// `OO_DOCS_DATABASE_URL`: the `oo_docs_runtime` login.
    pub database_url: String,
    /// `OO_DOCS_ADMIN_DATABASE_URL`: installs the schema when set.
    pub admin_database_url: Option<String>,
    /// `OO_DOCS_ENGINE_TOKEN_FILE`: files holding accepted tokens.
    pub token_files: Vec<PathBuf>,
    /// `OO_DOCS_ALLOWED_HOSTS`: extra `Host` values.
    pub allowed_hosts: Vec<String>,
    /// Ingest limits (`docs.json` `limits`, `sync`).
    pub ingest: IngestLimits,
    /// Poll planner settings.
    pub poll: PollConfig,
    /// `concurrency.*`.
    pub concurrency: HashMap<JobKind, usize>,
    /// `OO_DOCS_PARSER_URL` (`http://host:port`) and
    /// `OO_DOCS_PARSER_TOKEN_FILE`: the Docling sidecar. Without it pdf, docx
    /// and pptx are `unsupported`.
    pub parser: Option<ParserEndpoint>,
}

/// Where the sidecar is and the file holding its bearer token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParserEndpoint {
    /// `host:port`.
    pub addr: String,
    /// Token file path.
    pub token_file: PathBuf,
}

impl EngineConfig {
    /// Build from an environment lookup and the text of `docs.json`.
    pub fn from_parts(
        env: &dyn Fn(&str) -> Option<String>,
        docs_json: Option<&str>,
    ) -> Result<Self, ConfigError> {
        let listen = match env("OO_DOCS_LISTEN") {
            Some(v) => v
                .parse::<SocketAddr>()
                .map_err(|e| ConfigError::Invalid("OO_DOCS_LISTEN", e.to_string()))?,
            None => SocketAddr::from(([127, 0, 0, 1], 0)),
        };
        let database_url =
            env("OO_DOCS_DATABASE_URL").ok_or(ConfigError::Missing("OO_DOCS_DATABASE_URL"))?;
        let state_dir = PathBuf::from(
            env("OO_DOCS_STATE_DIR").ok_or(ConfigError::Missing("OO_DOCS_STATE_DIR"))?,
        );
        let token_files: Vec<PathBuf> = env("OO_DOCS_ENGINE_TOKEN_FILE")
            .ok_or(ConfigError::Missing("OO_DOCS_ENGINE_TOKEN_FILE"))?
            .split(',')
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .collect();
        if token_files.is_empty() {
            return Err(ConfigError::Missing("OO_DOCS_ENGINE_TOKEN_FILE"));
        }
        let allowed_hosts = env("OO_DOCS_ALLOWED_HOSTS")
            .map(|v| {
                v.split(',')
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();

        let mut ingest = IngestLimits::default();
        // csv and xlsx are parsed by the engine itself.
        ingest.parsers.insert(Format::Csv);
        ingest.parsers.insert(Format::Xlsx);
        let parser = match (env("OO_DOCS_PARSER_URL"), env("OO_DOCS_PARSER_TOKEN_FILE")) {
            (Some(url), Some(file)) => {
                let addr = url
                    .strip_prefix("http://")
                    .unwrap_or(&url)
                    .trim_end_matches('/')
                    .to_owned();
                if addr.is_empty() || addr.contains('/') {
                    return Err(ConfigError::Invalid(
                        "OO_DOCS_PARSER_URL",
                        "expected http://host:port".into(),
                    ));
                }
                ingest
                    .parsers
                    .extend([Format::Pdf, Format::Docx, Format::Pptx]);
                Some(ParserEndpoint {
                    addr,
                    token_file: PathBuf::from(file),
                })
            }
            (None, None) => None,
            (Some(_), None) => return Err(ConfigError::Missing("OO_DOCS_PARSER_TOKEN_FILE")),
            (None, Some(_)) => return Err(ConfigError::Missing("OO_DOCS_PARSER_URL")),
        };
        let mut poll = PollConfig::default();
        let mut concurrency: HashMap<JobKind, usize> =
            HashMap::from([(JobKind::Sync, 1), (JobKind::Hash, 4)]);
        if let Some(text) = docs_json {
            let doc: Value = serde_json::from_str(text)
                .map_err(|e| ConfigError::Invalid("docs.json", e.to_string()))?;
            let version = doc["version"].as_str().unwrap_or_default();
            if version != crate::CONFIG_PROTOCOL {
                return Err(ConfigError::Version(version.to_owned()));
            }
            if let Some(n) = doc["limits"]["max_file_bytes"].as_u64() {
                ingest.max_file_bytes = n;
            }
            if let Some(n) = doc["sync"]["settle_seconds"].as_u64() {
                ingest.settle = Duration::from_secs(n.min(60));
            }
            if let Some(n) = doc["sync"]["fs_poll_seconds"].as_u64() {
                poll.interval = Duration::from_secs(n.max(1));
            }
            if let Some(b) = doc["sync"]["weekly_hash_audit"].as_bool() {
                poll.weekly_audit = b;
            }
            if let Some(n) = doc["concurrency"]["hash"].as_u64() {
                concurrency.insert(JobKind::Hash, (n as usize).clamp(1, 32));
            }
            if let Some(n) = doc["concurrency"]["extract_llm"].as_u64() {
                concurrency.insert(JobKind::ExtractLlm, (n as usize).clamp(1, 8));
            }
        }
        Ok(Self {
            listen,
            state_dir,
            database_url,
            admin_database_url: env("OO_DOCS_ADMIN_DATABASE_URL"),
            token_files,
            allowed_hosts,
            ingest,
            poll,
            concurrency,
            parser,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |k| {
            pairs
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    const BASE: &[(&str, &str)] = &[
        ("OO_DOCS_DATABASE_URL", "postgres://oo@127.0.0.1/docs"),
        ("OO_DOCS_STATE_DIR", "/state"),
        ("OO_DOCS_ENGINE_TOKEN_FILE", "/state/home/engine.token"),
    ];

    #[test]
    fn defaults() {
        let c = EngineConfig::from_parts(&env_of(BASE), None).unwrap();
        assert_eq!(c.listen, "127.0.0.1:0".parse().unwrap());
        assert_eq!(c.ingest.max_file_bytes, 200 * 1024 * 1024);
        assert_eq!(c.poll.interval, Duration::from_secs(30));
        assert_eq!(c.concurrency[&JobKind::Hash], 4);
        assert!(c.admin_database_url.is_none());
    }

    #[test]
    fn the_parser_endpoint_admits_the_sidecar_formats() {
        use crate::ingest::sniff::Format;
        let c = EngineConfig::from_parts(&env_of(BASE), None).unwrap();
        assert!(c.parser.is_none());
        assert!(
            c.ingest.parsers.contains(&Format::Csv) && c.ingest.parsers.contains(&Format::Xlsx)
        );
        assert!(
            !c.ingest.parsers.contains(&Format::Pdf),
            "no sidecar, no pdf"
        );
        let with: Vec<(&str, &str)> = BASE
            .iter()
            .copied()
            .chain([
                ("OO_DOCS_PARSER_URL", "http://doc-parser:7481/"),
                ("OO_DOCS_PARSER_TOKEN_FILE", "/run/oo/parser-token"),
            ])
            .collect();
        let env = move |k: &str| {
            with.iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| (*v).to_owned())
        };
        let c = EngineConfig::from_parts(&env, None).unwrap();
        let ep = c.parser.unwrap();
        assert_eq!(ep.addr, "doc-parser:7481");
        assert_eq!(ep.token_file, PathBuf::from("/run/oo/parser-token"));
        for f in [Format::Pdf, Format::Docx, Format::Pptx] {
            assert!(c.ingest.parsers.contains(&f));
        }
    }

    #[test]
    fn a_parser_url_without_a_token_file_is_refused() {
        let with: Vec<(&str, &str)> = BASE
            .iter()
            .copied()
            .chain([("OO_DOCS_PARSER_URL", "http://127.0.0.1:7481")])
            .collect();
        let env = move |k: &str| {
            with.iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| (*v).to_owned())
        };
        assert!(matches!(
            EngineConfig::from_parts(&env, None),
            Err(ConfigError::Missing("OO_DOCS_PARSER_TOKEN_FILE"))
        ));
    }

    #[test]
    fn required_values() {
        for missing in [
            "OO_DOCS_DATABASE_URL",
            "OO_DOCS_STATE_DIR",
            "OO_DOCS_ENGINE_TOKEN_FILE",
        ] {
            let env = move |k: &str| {
                (k != missing)
                    .then(|| {
                        BASE.iter()
                            .find(|(n, _)| *n == k)
                            .map(|(_, v)| (*v).to_owned())
                    })
                    .flatten()
            };
            assert!(matches!(
                EngineConfig::from_parts(&env, None),
                Err(ConfigError::Missing(m)) if m == missing
            ));
        }
    }

    #[test]
    fn docs_json_overrides() {
        let json = r#"{"version":"oo.docs/v1","limits":{"max_file_bytes":1000},
            "sync":{"fs_poll_seconds":5,"settle_seconds":0,"weekly_hash_audit":false},
            "concurrency":{"hash":2,"extract_llm":4},"model":null,"unknown":{"x":1}}"#;
        let c = EngineConfig::from_parts(&env_of(BASE), Some(json)).unwrap();
        assert_eq!(c.ingest.max_file_bytes, 1000);
        assert_eq!(c.ingest.settle, Duration::ZERO);
        assert_eq!(c.poll.interval, Duration::from_secs(5));
        assert!(!c.poll.weekly_audit);
        assert_eq!(c.concurrency[&JobKind::Hash], 2);
        assert_eq!(c.concurrency[&JobKind::ExtractLlm], 4);
        assert!(matches!(
            EngineConfig::from_parts(&env_of(BASE), Some(r#"{"version":"oo.docs/v2"}"#)),
            Err(ConfigError::Version(_))
        ));
        assert!(EngineConfig::from_parts(&env_of(BASE), Some("{")).is_err());
    }

    #[test]
    fn bad_listen_address() {
        let env = |k: &str| match k {
            "OO_DOCS_LISTEN" => Some("not-an-address".to_owned()),
            other => BASE
                .iter()
                .find(|(n, _)| *n == other)
                .map(|(_, v)| (*v).to_owned()),
        };
        assert!(matches!(
            EngineConfig::from_parts(&env, None),
            Err(ConfigError::Invalid("OO_DOCS_LISTEN", _))
        ));
    }
}
