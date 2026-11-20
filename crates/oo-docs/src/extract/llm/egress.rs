//! The model port (10 §5.2): the engine talks to `model_egress.py` on
//! loopback (OpenAI compatible `POST /v1/chat/completions`), which holds the
//! endpoint and the key. This module never sees either: the egress process
//! reads them from `--config` (a `secret_ref` path under
//! `~/.config/open-ontology/secrets/`).
//!
//! [`LlmPort`] is what the extraction loop needs; [`EgressClient`] is the real
//! one, [`ReplayPort`] serves recorded replies and counts calls (no network).

use std::collections::BTreeMap;
use std::future::Future;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::pin::Pin;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::Value;

use super::prompt::ChatRequest;

/// A model reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatReply {
    pub content: String,
    pub tokens_in: u64,
    pub tokens_out: u64,
}

/// Why a call produced no reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LlmError {
    /// The egress or the model cannot be reached or answered with an error.
    Unavailable(String),
    /// Rate limited; try again after the given milliseconds.
    RateLimited(u64),
    /// The egress refused the request (`UNSUPPORTED_*`, `USAGE_LIMIT`...).
    Refused(String),
}

/// Boxed future of one call.
pub type CallFuture<'a> = Pin<Box<dyn Future<Output = Result<ChatReply, LlmError>> + Send + 'a>>;

/// What the extraction loop calls.
pub trait LlmPort: Send + Sync {
    /// The model name used in cache keys.
    fn model(&self) -> &str;
    fn complete<'a>(&'a self, req: &'a ChatRequest) -> CallFuture<'a>;
}

/// Client of the loopback egress.
pub struct EgressClient {
    pub addr: SocketAddr,
    pub model: String,
    pub timeout: Duration,
}

impl EgressClient {
    pub fn new(addr: SocketAddr, model: impl Into<String>) -> Self {
        Self {
            addr,
            model: model.into(),
            timeout: Duration::from_secs(60),
        }
    }

    fn call(&self, body: &str) -> Result<ChatReply, LlmError> {
        let unavailable = |why: &str| LlmError::Unavailable(why.to_owned());
        let mut s = TcpStream::connect_timeout(&self.addr, Duration::from_secs(3))
            .map_err(|_| unavailable("connect"))?;
        s.set_read_timeout(Some(self.timeout))
            .map_err(|_| unavailable("timeout"))?;
        s.set_write_timeout(Some(self.timeout))
            .map_err(|_| unavailable("timeout"))?;
        let head = format!(
            "POST /v1/chat/completions HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n",
            self.addr,
            body.len()
        );
        s.write_all(head.as_bytes())
            .and_then(|()| s.write_all(body.as_bytes()))
            .map_err(|_| unavailable("write"))?;
        let mut raw = Vec::new();
        s.take(1 << 20)
            .read_to_end(&mut raw)
            .map_err(|_| unavailable("read"))?;
        let text = String::from_utf8_lossy(&raw);
        let (head, payload) = text
            .split_once("\r\n\r\n")
            .ok_or_else(|| unavailable("response"))?;
        let status: u16 = head
            .split_whitespace()
            .nth(1)
            .and_then(|c| c.parse().ok())
            .ok_or_else(|| unavailable("status"))?;
        let v: Value = serde_json::from_str(payload).unwrap_or(Value::Null);
        match status {
            200 => {
                let content = v["choices"][0]["message"]["content"]
                    .as_str()
                    .ok_or_else(|| unavailable("no content"))?
                    .to_owned();
                let n = |k: &str| v["usage"][k].as_u64().unwrap_or(0);
                Ok(ChatReply {
                    content,
                    tokens_in: n("prompt_tokens"),
                    tokens_out: n("completion_tokens"),
                })
            }
            429 => Err(LlmError::RateLimited(
                v["error"]["retry_after_ms"].as_u64().unwrap_or(5_000),
            )),
            400 | 403 => Err(LlmError::Refused(
                v["error"]["code"].as_str().unwrap_or("REFUSED").to_owned(),
            )),
            _ => Err(LlmError::Unavailable(format!("status {status}"))),
        }
    }
}

impl LlmPort for EgressClient {
    fn model(&self) -> &str {
        &self.model
    }

    fn complete<'a>(&'a self, req: &'a ChatRequest) -> CallFuture<'a> {
        let body = req.body().to_string();
        Box::pin(async move {
            let me = EgressClient {
                addr: self.addr,
                model: self.model.clone(),
                timeout: self.timeout,
            };
            tokio::task::spawn_blocking(move || me.call(&body))
                .await
                .unwrap_or_else(|_| Err(LlmError::Unavailable("join".into())))
        })
    }
}

/// The command line of the egress process for a stack config: the key stays
/// a `secret_ref` inside `config`, nothing secret is on the command line.
pub fn egress_args(
    config_path: &str,
    meter_path: &str,
    port: u16,
    rpm: u32,
    upstream_timeout_secs: u64,
) -> Vec<String> {
    vec![
        "--config".into(),
        config_path.into(),
        "--meter".into(),
        meter_path.into(),
        "--listen".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string(),
        "--rpm".into(),
        rpm.to_string(),
        "--timeout".into(),
        upstream_timeout_secs.to_string(),
    ]
}

/// How long the egress waits for the model's answer, in seconds. The egress script's own default (45 s) is shorter than a
/// reasoning model needs for an extraction prompt: the first packaged run against grok-4.7 logged only UPSTREAM_TIMEOUT and read
/// no file. `OO_DOCS_MODEL_TIMEOUT_SECS` changes it (10 to 900); anything else means the default.
pub const MODEL_TIMEOUT_SECS: u64 = 180;
/// The engine waits this much longer than the egress, so the egress's own timeout answer arrives first.
pub const CLIENT_GRACE_SECS: u64 = 30;

pub fn model_timeout_secs(env: Option<&str>) -> u64 {
    env.and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|n| (10..=900).contains(n))
        .unwrap_or(MODEL_TIMEOUT_SECS)
}

/// Serves recorded replies keyed by the cache key of the batch, and counts
/// the calls it served. A request without a recording is `Unavailable`.
pub struct ReplayPort {
    model: String,
    by_user: Mutex<BTreeMap<String, Vec<Result<ChatReply, LlmError>>>>,
    calls: Mutex<Vec<String>>,
}

impl ReplayPort {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            by_user: Mutex::new(BTreeMap::new()),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Record the replies for the request whose user message is `user`; they
    /// are served in order, the last one repeats.
    pub fn record(&self, user: &str, replies: Vec<Result<ChatReply, LlmError>>) {
        if let Ok(mut m) = self.by_user.lock() {
            m.insert(user.to_owned(), replies);
        }
    }

    /// User messages of the calls served, in order.
    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().map(|c| c.clone()).unwrap_or_default()
    }
}

impl LlmPort for ReplayPort {
    fn model(&self) -> &str {
        &self.model
    }

    fn complete<'a>(&'a self, req: &'a ChatRequest) -> CallFuture<'a> {
        let out = (|| {
            let n = {
                let mut c = self.calls.lock().ok()?;
                c.push(req.user.clone());
                c.iter().filter(|u| **u == req.user).count()
            };
            let m = self.by_user.lock().ok()?;
            let replies = m.get(&req.user)?;
            let i = (n - 1).min(replies.len().saturating_sub(1));
            replies.get(i).cloned()
        })();
        Box::pin(async move {
            out.unwrap_or_else(|| Err(LlmError::Unavailable("no recorded reply".into())))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_line_holds_no_secret() {
        let a = egress_args("/state/model.json", "/state/meter.jsonl", 8317, 30, 180);
        assert!(a.windows(2).any(|w| w == ["--timeout", "180"]));
        assert_eq!(a[0], "--config");
        assert!(
            a.iter()
                .all(|x| !x.to_lowercase().contains("key") && !x.contains("secret"))
        );
        assert!(a.contains(&"127.0.0.1".to_owned()));
    }

    #[test]
    fn the_model_timeout_is_longer_than_the_egress_default_and_can_be_set() {
        assert_eq!(model_timeout_secs(None), 180);
        assert_eq!(model_timeout_secs(Some(" 300 ")), 300);
        for bad in ["", "abc", "5", "901", "-1"] {
            assert_eq!(model_timeout_secs(Some(bad)), MODEL_TIMEOUT_SECS, "{bad}");
        }
    }

    #[test]
    fn the_client_speaks_to_a_loopback_server() {
        use std::net::TcpListener;
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let t = std::thread::spawn(move || {
            for (status, body) in [
                (
                    200,
                    r#"{"choices":[{"message":{"content":"{\"entities\":[],\"relations\":[]}"}}],"usage":{"prompt_tokens":10,"completion_tokens":3}}"#,
                ),
                (429, r#"{"error":{"retry_after_ms":1500}}"#),
                (403, r#"{"error":{"code":"USAGE_LIMIT"}}"#),
                (500, "{}"),
            ] {
                let (mut s, _) = l.accept().unwrap();
                // Read the whole request (head, then Content-Length bytes) so the
                // close does not reset the connection under the client.
                let mut got = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    let n = s.read(&mut buf).unwrap();
                    got.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&got).into_owned();
                    if let Some((head, rest)) = text.split_once("\r\n\r\n") {
                        let want: usize = head
                            .lines()
                            .find_map(|l| l.strip_prefix("Content-Length: "))
                            .and_then(|v| v.trim().parse().ok())
                            .unwrap_or(0);
                        if rest.len() >= want {
                            break;
                        }
                    }
                    if n == 0 {
                        break;
                    }
                }
                let r = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                s.write_all(r.as_bytes()).unwrap();
            }
        });
        let c = EgressClient::new(addr, "m");
        let r = c.call("{}").unwrap();
        assert_eq!((r.tokens_in, r.tokens_out), (10, 3));
        assert!(r.content.starts_with("{\"entities\""));
        assert_eq!(c.call("{}").unwrap_err(), LlmError::RateLimited(1500));
        assert_eq!(
            c.call("{}").unwrap_err(),
            LlmError::Refused("USAGE_LIMIT".into())
        );
        assert!(matches!(
            c.call("{}").unwrap_err(),
            LlmError::Unavailable(_)
        ));
        t.join().unwrap();
        // Nothing listens any more.
        assert!(matches!(
            c.call("{}").unwrap_err(),
            LlmError::Unavailable(_)
        ));
    }
}
