//! Loopback client for the `oo serve` proxy: reads `serve.json` and the token
//! file, sends requests, maps failures to `oo context` exit codes (10 §7.9).

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use super::secrets;

/// Exit codes of 10 §7.9.
pub mod exit {
    pub const OK: u8 = 0;
    pub const OTHER: u8 = 1;
    pub const ARGS: u8 = 2;
    pub const ENGINE_UNAVAILABLE: u8 = 3;
    pub const STRICT_BUDGET: u8 = 4;
    pub const SCOPE: u8 = 5;
    pub const TARGET: u8 = 6;
    pub const GATE: u8 = 7;
    pub const SHARE_STATE: u8 = 8;
}

/// A failure with the exit code it maps to. The text never carries a token.
#[derive(Debug, PartialEq, Eq)]
pub struct CliError {
    pub exit: u8,
    pub code: String,
    pub message: String,
}

impl CliError {
    pub fn new(exit: u8, code: &str, message: impl Into<String>) -> Self {
        Self {
            exit,
            code: code.to_owned(),
            message: message.into(),
        }
    }

    /// Map an engine error code to the exit code of the table in 10 §7.9.
    pub fn from_engine(status: u16, body: &[u8]) -> Self {
        let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
        let code = v["code"]
            .as_str()
            .unwrap_or("DOCS_ENGINE_INTERNAL")
            .to_owned();
        let message = v["message"].as_str().unwrap_or("request failed").to_owned();
        let exit = match code.as_str() {
            "DOCS_ENGINE_UNAVAILABLE" | "DOCS_ENGINE_DOCKER_UNAVAILABLE" => {
                exit::ENGINE_UNAVAILABLE
            }
            "DOCS_PACK_SCOPE_INVALID" | "DOCS_PACK_NOTHING_VISIBLE" => exit::SCOPE,
            "DOCS_SHARE_TARGET_OCCUPIED" | "DOCS_SHARE_TARGET_INVALID" => exit::TARGET,
            "DOCS_PACK_LEAK_GUARD" | "DOCS_PACK_PROVENANCE_GAP" => exit::GATE,
            "DOCS_SHARE_NOT_FOUND"
            | "DOCS_SHARE_INVALID_TRANSITION"
            | "DOCS_SHARE_PINNED"
            | "DOCS_SHARE_GROUP_MEMBER" => exit::SHARE_STATE,
            // A coverage the caller refused (`--require-complete`) is a compile gate.
            "DOCS_PACK_COVERAGE_INCOMPLETE" => exit::GATE,
            _ if status == 400 || status == 422 => exit::ARGS,
            _ => exit::OTHER,
        };
        Self {
            exit,
            code,
            message,
        }
    }
}

/// An authenticated connection to the daemon.
pub struct Daemon {
    base: String,
    token: String,
    http: reqwest::Client,
}

/// Read `serve.json` and the token (must be a private file).
pub fn read_connection(state: &Path) -> Result<(u16, String), CliError> {
    let down = |why: &str| CliError::new(exit::ENGINE_UNAVAILABLE, "DOCS_ENGINE_UNAVAILABLE", why);
    let doc = secrets::read_serve_json(state)
        .ok_or_else(|| down("oo serve is not running (no serve.json); start it with `oo serve`"))?;
    let port = doc["port"]
        .as_u64()
        .and_then(|p| u16::try_from(p).ok())
        .ok_or_else(|| down("serve.json has no port"))?;
    let token_file = PathBuf::from(
        doc["token_file"]
            .as_str()
            .ok_or_else(|| down("serve.json has no token_file"))?,
    );
    let meta = std::fs::symlink_metadata(&token_file).map_err(|_| down("token file is missing"))?;
    if !meta.is_file() || meta.mode() & 0o077 != 0 {
        return Err(CliError::new(
            exit::ENGINE_UNAVAILABLE,
            "DOCS_ENGINE_UNAVAILABLE",
            "token file is not a private regular file",
        ));
    }
    let token = std::fs::read_to_string(&token_file)
        .map_err(|_| down("token file is unreadable"))?
        .trim()
        .to_owned();
    Ok((port, token))
}

fn agent_headers(agent: &str) -> Result<reqwest::header::HeaderMap, CliError> {
    let mut h = reqwest::header::HeaderMap::new();
    h.insert(
        "oo-docs-agent",
        reqwest::header::HeaderValue::from_str(agent).map_err(|_| {
            CliError::new(
                exit::ARGS,
                "DOCS_ENGINE_BAD_REQUEST",
                "--agent has characters a header cannot carry",
            )
        })?,
    );
    Ok(h)
}

impl Daemon {
    /// Connect using the state directory.
    pub fn connect(state: &Path) -> Result<Self, CliError> {
        let (port, token) = read_connection(state)?;
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|_| CliError::new(exit::OTHER, "DOCS_ENGINE_INTERNAL", "http client"))?;
        Ok(Self {
            base: format!("http://127.0.0.1:{port}"),
            token,
            http,
        })
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<(u16, Vec<u8>), CliError> {
        let resp = req
            .header("authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .map_err(|_| {
                CliError::new(
                    exit::ENGINE_UNAVAILABLE,
                    "DOCS_ENGINE_UNAVAILABLE",
                    "cannot reach oo serve; is it running?",
                )
            })?;
        let status = resp.status().as_u16();
        let body = resp
            .bytes()
            .await
            .map_err(|_| {
                CliError::new(
                    exit::ENGINE_UNAVAILABLE,
                    "DOCS_ENGINE_UNAVAILABLE",
                    "connection lost",
                )
            })?
            .to_vec();
        if status >= 400 {
            return Err(CliError::from_engine(status, &body));
        }
        Ok((status, body))
    }

    /// `GET`, body as JSON.
    pub async fn get_json(&self, path: &str) -> Result<Value, CliError> {
        let (_, body) = self
            .send(self.http.get(format!("{}{path}", self.base)))
            .await?;
        serde_json::from_slice(&body)
            .map_err(|_| CliError::new(exit::OTHER, "DOCS_ENGINE_INTERNAL", "response is not JSON"))
    }

    /// `DELETE` (with `If-Match` when given); the body may be empty.
    pub async fn delete(&self, path: &str, if_match: Option<&str>) -> Result<Value, CliError> {
        let key = secrets::new_token("").map_err(|_| {
            CliError::new(exit::OTHER, "DOCS_ENGINE_INTERNAL", "no randomness source")
        })?;
        let mut req = self
            .http
            .delete(format!("{}{path}", self.base))
            .header("idempotency-key", key);
        if let Some(m) = if_match {
            req = req.header("if-match", m);
        }
        let (_, body) = self.send(req).await?;
        Ok(serde_json::from_slice(&body).unwrap_or(Value::Null))
    }

    /// `POST` JSON with an `If-Match` header and a fresh idempotency key.
    pub async fn post_json_if(
        &self,
        path: &str,
        body: &Value,
        if_match: &str,
    ) -> Result<Value, CliError> {
        let key = secrets::new_token("").map_err(|_| {
            CliError::new(exit::OTHER, "DOCS_ENGINE_INTERNAL", "no randomness source")
        })?;
        let req = self
            .http
            .post(format!("{}{path}", self.base))
            .header("idempotency-key", key)
            .header("if-match", if_match)
            .header("content-type", "application/json")
            .body(body.to_string());
        let (_, body) = self.send(req).await?;
        serde_json::from_slice(&body)
            .map_err(|_| CliError::new(exit::OTHER, "DOCS_ENGINE_INTERNAL", "response is not JSON"))
    }

    /// `GET`, raw bytes.
    pub async fn get_bytes(&self, path: &str) -> Result<Vec<u8>, CliError> {
        Ok(self
            .send(self.http.get(format!("{}{path}", self.base)))
            .await?
            .1)
    }

    /// The engine's cloud entry at `base` (`http(s)://host:port`) with an `oodc_` token; `agent` is sent as
    /// `OO-Docs-Agent` so the engine's audit log says who a share is for (B-S12-02).
    pub fn remote(base: &str, token: &str, agent: Option<&str>) -> Result<Self, CliError> {
        let mut b = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(120));
        if let Some(a) = agent {
            b = b.default_headers(agent_headers(a)?);
        }
        let http = b
            .build()
            .map_err(|_| CliError::new(exit::OTHER, "DOCS_ENGINE_INTERNAL", "http client"))?;
        Ok(Self {
            base: base.trim_end_matches('/').to_owned(),
            token: token.to_owned(),
            http,
        })
    }

    /// The same client, sending `OO-Docs-Agent: <agent>` with every request.
    pub fn with_agent(self, agent: &str) -> Result<Self, CliError> {
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(120))
            .default_headers(agent_headers(agent)?)
            .build()
            .map_err(|_| CliError::new(exit::OTHER, "DOCS_ENGINE_INTERNAL", "http client"))?;
        Ok(Self { http, ..self })
    }

    /// A client that authenticates with a share's refs credential at `base`
    /// (`http://127.0.0.1:<port>`), for the task-time calls of `oo context check`.
    pub fn with_credential(base: &str, credential: &str) -> Result<Self, CliError> {
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|_| CliError::new(exit::OTHER, "DOCS_ENGINE_INTERNAL", "http client"))?;
        Ok(Self {
            base: base.trim_end_matches('/').to_owned(),
            token: credential.to_owned(),
            http,
        })
    }

    /// `POST` that returns the status and the JSON body without mapping
    /// statuses to errors. Only a connection failure is an error (exit 3).
    pub async fn post_raw(&self, path: &str, body: &Value) -> Result<(u16, Value), CliError> {
        let key = secrets::new_token("").map_err(|_| {
            CliError::new(exit::OTHER, "DOCS_ENGINE_INTERNAL", "no randomness source")
        })?;
        let resp = self
            .http
            .post(format!("{}{path}", self.base))
            .header("authorization", format!("Bearer {}", self.token))
            .header("idempotency-key", key)
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .map_err(|_| {
                CliError::new(
                    exit::ENGINE_UNAVAILABLE,
                    "DOCS_ENGINE_UNAVAILABLE",
                    "cannot reach the engine",
                )
            })?;
        let status = resp.status().as_u16();
        let bytes = resp.bytes().await.map_err(|_| {
            CliError::new(
                exit::ENGINE_UNAVAILABLE,
                "DOCS_ENGINE_UNAVAILABLE",
                "connection lost",
            )
        })?;
        Ok((
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        ))
    }

    /// `POST` JSON with a fresh idempotency key.
    pub async fn post_json(&self, path: &str, body: &Value) -> Result<Value, CliError> {
        let key = secrets::new_token("").map_err(|_| {
            CliError::new(exit::OTHER, "DOCS_ENGINE_INTERNAL", "no randomness source")
        })?;
        let req = self
            .http
            .post(format!("{}{path}", self.base))
            .header("idempotency-key", key)
            .header("content-type", "application/json")
            .body(body.to_string());
        let (_, body) = self.send(req).await?;
        serde_json::from_slice(&body)
            .map_err(|_| CliError::new(exit::OTHER, "DOCS_ENGINE_INTERNAL", "response is not JSON"))
    }
}

/// Percent-encode one path for a URL (keeps `/`).
pub fn encode_path(path: &str) -> String {
    let mut out = String::new();
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_codes_map_to_the_documented_exit_codes() {
        let e = |code: &str, status: u16| {
            CliError::from_engine(
                status,
                format!("{{\"code\":\"{code}\",\"message\":\"m\"}}").as_bytes(),
            )
            .exit
        };
        assert_eq!(e("DOCS_ENGINE_UNAVAILABLE", 503), 3);
        assert_eq!(e("DOCS_PACK_SCOPE_INVALID", 422), 5);
        assert_eq!(e("DOCS_PACK_NOTHING_VISIBLE", 422), 5);
        assert_eq!(e("DOCS_SHARE_TARGET_OCCUPIED", 409), 6);
        assert_eq!(e("DOCS_SHARE_TARGET_INVALID", 422), 6);
        assert_eq!(e("DOCS_PACK_LEAK_GUARD", 500), 7);
        assert_eq!(e("DOCS_PACK_PROVENANCE_GAP", 500), 7);
        assert_eq!(e("DOCS_SHARE_NOT_FOUND", 404), 8);
        assert_eq!(e("DOCS_ENGINE_INTERNAL", 500), 1);
        assert_eq!(e("DOCS_ENGINE_BAD_REQUEST", 400), 2);
        assert_eq!(CliError::from_engine(502, b"<html>").exit, 1);
    }

    #[test]
    fn paths_are_percent_encoded_per_byte() {
        assert_eq!(encode_path("people/a-1.md"), "people/a-1.md");
        assert_eq!(encode_path("people/林-1.md"), "people/%E6%9E%97-1.md");
        assert_eq!(encode_path("a b+c"), "a%20b%2Bc");
    }

    #[test]
    fn a_missing_serve_json_is_exit_3() {
        let d = std::env::temp_dir().join(format!("oo-client-none-{}", std::process::id()));
        let e = read_connection(&d).unwrap_err();
        assert_eq!(e.exit, 3);
    }
}
