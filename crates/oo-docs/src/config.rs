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
    /// `OO_DOCS_OCR_URL` and `OO_DOCS_OCR_TOKEN_FILE`: the `ocr-parser` (scanned pdf, images, 10 17.1).
    /// Needs the Docling parser, which probes the pdf and hands scanned ones over.
    pub ocr: Option<ParserEndpoint>,
    /// `OO_DOCS_MEDIA_URL` and `OO_DOCS_MEDIA_TOKEN_FILE`: the `media-parser` (audio, video, 17.1).
    pub media: Option<MediaEndpoint>,
}

/// The `media-parser` endpoint and the `oo.docs/v1` `media.*` settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaEndpoint {
    /// Sidecar address and token file.
    pub endpoint: ParserEndpoint,
    /// `media.external_ffmpeg`: absolute path of the customer's FFmpeg inside the sidecar (Linux, D65).
    pub external_ffmpeg: Option<String>,
}

fn endpoint(
    env: &dyn Fn(&str) -> Option<String>,
    url_key: &'static str,
    token_key: &'static str,
) -> Result<Option<ParserEndpoint>, ConfigError> {
    // An empty value means off (compose passes the keys through with an empty default).
    let get = |k: &'static str| env(k).filter(|v| !v.is_empty());
    match (get(url_key), get(token_key)) {
        (Some(url), Some(file)) => {
            let addr = url
                .strip_prefix("http://")
                .unwrap_or(&url)
                .trim_end_matches('/')
                .to_owned();
            if addr.is_empty() || addr.contains('/') {
                return Err(ConfigError::Invalid(
                    url_key,
                    "expected http://host:port".into(),
                ));
            }
            Ok(Some(ParserEndpoint {
                addr,
                token_file: PathBuf::from(file),
            }))
        }
        (None, None) => Ok(None),
        (Some(_), None) => Err(ConfigError::Missing(token_key)),
        (None, Some(_)) => Err(ConfigError::Missing(url_key)),
    }
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
        let ocr = endpoint(env, "OO_DOCS_OCR_URL", "OO_DOCS_OCR_TOKEN_FILE")?;
        if ocr.is_some() && parser.is_none() {
            return Err(ConfigError::Missing("OO_DOCS_PARSER_URL"));
        }
        if ocr.is_some() {
            ingest.parsers.insert(Format::Image);
        }
        let mut media_ep =
            endpoint(env, "OO_DOCS_MEDIA_URL", "OO_DOCS_MEDIA_TOKEN_FILE")?.map(|endpoint| {
                MediaEndpoint {
                    endpoint,
                    external_ffmpeg: None,
                }
            });
        if media_ep.is_some() {
            ingest.parsers.extend([Format::Audio, Format::Video]);
        }
        let mut poll = PollConfig::default();
        let mut concurrency: HashMap<JobKind, usize> = HashMap::from([
            (JobKind::Sync, 1),
            (JobKind::Hash, 4),
            (
                JobKind::ParseMedia,
                crate::jobs::media_pool::DEFAULT_MEDIA_SLOTS,
            ),
        ]);
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
            if let Some(m) = media_ep.as_mut()
                && let Some(p) = doc["media"]["external_ffmpeg"].as_str()
            {
                if !p.starts_with('/') || p.contains('\0') {
                    return Err(ConfigError::Invalid(
                        "docs.json",
                        "media.external_ffmpeg must be an absolute path".into(),
                    ));
                }
                m.external_ffmpeg = Some(p.to_owned());
            }
            if let Some(n) = doc["concurrency"]["media"].as_u64() {
                concurrency.insert(
                    JobKind::ParseMedia,
                    crate::jobs::media_pool::clamp_media_slots(n),
                );
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
            ocr,
            media: media_ep,
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
    fn media_concurrency_defaults_to_one_and_is_read_from_docs_json() {
        let c = EngineConfig::from_parts(&env_of(BASE), None).unwrap();
        assert_eq!(c.concurrency[&JobKind::ParseMedia], 1);
        let json = r#"{"version":"oo.docs/v1","concurrency":{"media":3}}"#;
        let c = EngineConfig::from_parts(&env_of(BASE), Some(json)).unwrap();
        assert_eq!(c.concurrency[&JobKind::ParseMedia], 3);
        let json = r#"{"version":"oo.docs/v1","concurrency":{"media":50}}"#;
        let c = EngineConfig::from_parts(&env_of(BASE), Some(json)).unwrap();
        assert_eq!(c.concurrency[&JobKind::ParseMedia], 4);
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

    #[test]
    fn ocr_and_media_sidecars_admit_images_audio_and_video() {
        use crate::ingest::sniff::Format;
        let env_with = |extra: Vec<(&'static str, &'static str)>| {
            let all: Vec<(&str, &str)> = BASE.iter().copied().chain(extra).collect();
            move |k: &str| {
                all.iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| (*v).to_owned())
            }
        };
        let c = EngineConfig::from_parts(&env_with(vec![]), None).unwrap();
        assert!(c.ocr.is_none() && c.media.is_none());
        assert!(!c.ingest.parsers.contains(&Format::Image));
        assert!(!c.ingest.parsers.contains(&Format::Audio));
        let parser = [
            ("OO_DOCS_PARSER_URL", "http://doc-parser:7481"),
            ("OO_DOCS_PARSER_TOKEN_FILE", "/run/p"),
        ];
        let c = EngineConfig::from_parts(
            &env_with(
                parser
                    .into_iter()
                    .chain([
                        ("OO_DOCS_OCR_URL", "http://ocr-parser:7481"),
                        ("OO_DOCS_OCR_TOKEN_FILE", "/run/o"),
                        ("OO_DOCS_MEDIA_URL", "http://media-parser:7481"),
                        ("OO_DOCS_MEDIA_TOKEN_FILE", "/run/m"),
                    ])
                    .collect(),
            ),
            Some(
                r#"{"version":"oo.docs/v1","media":{"external_ffmpeg":"/opt/ffmpeg/bin/ffmpeg"}}"#,
            ),
        )
        .unwrap();
        assert_eq!(c.ocr.unwrap().addr, "ocr-parser:7481");
        let m = c.media.unwrap();
        assert_eq!(m.endpoint.addr, "media-parser:7481");
        assert_eq!(m.external_ffmpeg.as_deref(), Some("/opt/ffmpeg/bin/ffmpeg"));
        for f in [Format::Image, Format::Audio, Format::Video] {
            assert!(c.ingest.parsers.contains(&f));
        }
        // The ocr-parser needs the Docling parser that probes the pdf.
        assert!(
            EngineConfig::from_parts(
                &env_with(vec![
                    ("OO_DOCS_OCR_URL", "http://ocr-parser:7481"),
                    ("OO_DOCS_OCR_TOKEN_FILE", "/run/o"),
                ]),
                None
            )
            .is_err()
        );
        // A relative external FFmpeg path is refused.
        assert!(
            EngineConfig::from_parts(
                &env_with(vec![
                    ("OO_DOCS_MEDIA_URL", "http://media-parser:7481"),
                    ("OO_DOCS_MEDIA_TOKEN_FILE", "/run/m"),
                ]),
                Some(r#"{"version":"oo.docs/v1","media":{"external_ffmpeg":"ffmpeg"}}"#),
            )
            .is_err()
        );
    }
}
