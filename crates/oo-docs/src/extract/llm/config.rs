//! The engine's model: configuration and the egress process behind it
//! (10 §8.8 G3, D18, D19).
//!
//! `GET/PUT /v1/docs/config/model` read and write [`ModelConfig`]. The model's
//! endpoint and key never pass through the engine as values: `key_ref` is
//! `file:/absolute/path` (a 0600 file) or `env:NAME`, `base_url` is an https
//! URL, a loopback http URL or the same kind of reference. The engine hands the
//! references to `tools/model-egress/model_egress.py` in a config file and
//! talks to that process on loopback; the process reads the secrets itself.
//! Nothing here reads, prints or stores a secret; error messages name fields,
//! never values.
//!
//! The configuration (references only) lives in `<state>/model.json` with a
//! revision counter for `If-Match`. A runtime without a state directory or
//! without an egress script is "unavailable" once configured: the stages then
//! run rules only (D19).

use std::io::Read as _;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::egress::{CLIENT_GRACE_SECS, EgressClient, LlmPort, egress_args, model_timeout_secs};

/// Providers the egress speaks.
pub const PROVIDERS: [&str; 2] = ["openai-compatible", "anthropic"];
/// File the configuration is kept in, inside the state directory.
pub const CONFIG_FILE: &str = "model.json";
/// Calls per minute the egress admits.
pub const EGRESS_RPM: u32 = 30;

/// The configured model. References only, never a secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelConfig {
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub key_ref: String,
    pub revision: i64,
}

impl ModelConfig {
    /// The `ModelConfig` response object (`status` says whether the egress is up).
    pub fn to_json(&self, status: &str) -> Value {
        json!({
            "configured": true,
            "provider": self.provider,
            "base_url": self.base_url,
            "model": self.model,
            "key_ref": self.key_ref,
            "revision": self.revision,
            "status": status,
        })
    }
}

/// A rejected `PUT` body: JSON pointer and a message that never holds a value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invalid {
    pub pointer: &'static str,
    pub message: &'static str,
}

fn env_name_ok(n: &str) -> bool {
    let mut c = n.chars();
    matches!(c.next(), Some(f) if f.is_ascii_alphabetic() || f == '_')
        && n.len() <= 128
        && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn path_ref_ok(p: &str) -> bool {
    p.starts_with('/')
        && p.len() <= 2048
        && !p.chars().any(|c| c.is_control())
        && !p.split('/').any(|s| s == "..")
}

/// `file:/abs/path` or `env:NAME`.
pub fn valid_ref(s: &str) -> bool {
    match (s.strip_prefix("file:"), s.strip_prefix("env:")) {
        (Some(p), _) => path_ref_ok(p),
        (_, Some(n)) => env_name_ok(n),
        _ => false,
    }
}

fn valid_base_url(s: &str) -> bool {
    if s.starts_with("file:") || s.starts_with("env:") {
        return valid_ref(s);
    }
    if s.len() > 2048
        || s.chars().any(|c| c <= ' ' || c as u32 >= 127)
        || s.chars().any(|c| "@?#\\%".contains(c))
    {
        return false;
    }
    let (scheme, rest) = match s.split_once("://") {
        Some(x) => x,
        None => return false,
    };
    let host = rest
        .split(['/', ':'])
        .next()
        .filter(|h| !h.is_empty())
        .unwrap_or_default();
    match scheme {
        "https" => !host.is_empty(),
        "http" => matches!(host, "localhost" | "127.0.0.1" | "[::1]"),
        _ => false,
    }
}

fn valid_model(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._+:-".contains(c))
}

/// Validate a `PUT` body into (provider, base_url, model, key_ref).
pub fn validate_put(body: &Value) -> Result<(String, String, String, String), Invalid> {
    let obj = body.as_object().ok_or(Invalid {
        pointer: "",
        message: "expected an object",
    })?;
    for k in obj.keys() {
        if !["provider", "base_url", "model", "key_ref"].contains(&k.as_str()) {
            return Err(Invalid {
                pointer: "",
                message: "unknown field",
            });
        }
    }
    let text = |k: &'static str| -> Result<String, Invalid> {
        obj.get(k)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or(Invalid {
                pointer: k,
                message: "a string is required",
            })
    };
    let provider = text("provider")?;
    if !PROVIDERS.contains(&provider.as_str()) {
        return Err(Invalid {
            pointer: "provider",
            message: "provider is openai-compatible or anthropic",
        });
    }
    let base_url = text("base_url")?;
    if !valid_base_url(&base_url) {
        return Err(Invalid {
            pointer: "base_url",
            message: "base_url is an https URL, a loopback http URL, file:/absolute/path or env:NAME",
        });
    }
    let model = text("model")?;
    if !valid_model(&model) {
        return Err(Invalid {
            pointer: "model",
            message: "model is 1 to 128 characters of letters, digits and /._+:-",
        });
    }
    let key_ref = text("key_ref")?;
    if !valid_ref(&key_ref) {
        return Err(Invalid {
            pointer: "key_ref",
            message: "key_ref is file:/absolute/path or env:NAME; a key value is never accepted",
        });
    }
    Ok((provider, base_url, model, key_ref))
}

/// Why a `PUT` did not apply.
#[derive(Debug, PartialEq, Eq)]
pub enum PutError {
    /// `If-Match` named another revision.
    Conflict { current: i64 },
    /// The configuration could not be written.
    Store,
}

struct Inner {
    cfg: Option<ModelConfig>,
    port: Option<Arc<dyn LlmPort>>,
    child: Option<Child>,
    /// Code the egress gave when it refused to start (a fixed vocabulary).
    last_error: Option<String>,
}

/// The model of this engine: configuration, the egress child and the port the
/// stages call. Cheap to share (`Arc`); `None` port means rules only.
pub struct ModelRuntime {
    dir: Option<PathBuf>,
    script: Option<PathBuf>,
    python: String,
    inner: Mutex<Inner>,
}

impl Drop for ModelRuntime {
    fn drop(&mut self) {
        if let Ok(mut g) = self.inner.lock()
            && let Some(mut c) = g.child.take()
        {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn free_port() -> Option<u16> {
    TcpListener::bind("127.0.0.1:0")
        .ok()
        .and_then(|l| l.local_addr().ok())
        .map(|a| a.port())
}

#[cfg(unix)]
fn private_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let tmp = path.with_extension("tmp");
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    std::fs::rename(tmp, path)
}

#[cfg(not(unix))]
fn private_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}

impl ModelRuntime {
    /// No model: the stages run rules only.
    pub fn disabled() -> Arc<Self> {
        Arc::new(Self {
            dir: None,
            script: None,
            python: "python3".into(),
            inner: Mutex::new(Inner {
                cfg: None,
                port: None,
                child: None,
                last_error: None,
            }),
        })
    }

    /// A model that answers through `port` (recorded replies in tests): the
    /// configuration is a placeholder and no process is started.
    pub fn fixed(port: Arc<dyn LlmPort>) -> Arc<Self> {
        let cfg = ModelConfig {
            provider: "openai-compatible".into(),
            base_url: "http://127.0.0.1:1".into(),
            model: port.model().to_owned(),
            key_ref: "env:OO_RECORDED_MODEL_KEY".into(),
            revision: 2,
        };
        Arc::new(Self {
            dir: None,
            script: None,
            python: "python3".into(),
            inner: Mutex::new(Inner {
                cfg: Some(cfg),
                port: Some(port),
                child: None,
                last_error: None,
            }),
        })
    }

    /// The runtime of a real engine: `dir` is the state directory (the
    /// configuration is read from it and written back), `script` the egress
    /// (`OO_DOCS_MODEL_EGRESS`), `python` its interpreter. A saved
    /// configuration is started at once.
    pub fn open(dir: PathBuf, script: Option<PathBuf>, python: Option<String>) -> Arc<Self> {
        let rt = Arc::new(Self {
            dir: Some(dir.clone()),
            script,
            python: python.unwrap_or_else(|| "python3".into()),
            inner: Mutex::new(Inner {
                cfg: None,
                port: None,
                child: None,
                last_error: None,
            }),
        });
        if let Some(cfg) = std::fs::read_to_string(dir.join(CONFIG_FILE))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| {
                let s = |k: &str| v[k].as_str().map(str::to_owned);
                Some(ModelConfig {
                    provider: s("provider")?,
                    base_url: s("base_url")?,
                    model: s("model")?,
                    key_ref: s("key_ref")?,
                    revision: v["revision"].as_i64()?,
                })
            })
        {
            rt.start(cfg);
        }
        rt
    }

    /// Current configuration.
    pub fn config(&self) -> Option<ModelConfig> {
        self.inner.lock().ok().and_then(|g| g.cfg.clone())
    }

    /// The port the stages call; `None` when no model is configured or its
    /// egress is not running.
    pub fn port(&self) -> Option<Arc<dyn LlmPort>> {
        self.inner.lock().ok().and_then(|g| g.port.clone())
    }

    /// `not_configured`, `unavailable` (configured, egress not up) or `ok`.
    pub fn status(&self) -> &'static str {
        match self.inner.lock() {
            Ok(g) if g.cfg.is_none() => "not_configured",
            Ok(g) if g.port.is_some() => "ok",
            _ => "unavailable",
        }
    }

    /// The refusal code of the egress, when it did not start.
    pub fn last_error(&self) -> Option<String> {
        self.inner.lock().ok().and_then(|g| g.last_error.clone())
    }

    /// Apply a validated `PUT`. `want` is the revision of `If-Match`; an
    /// unconfigured engine is revision 1.
    pub fn put(
        &self,
        want: i64,
        parts: (String, String, String, String),
    ) -> Result<ModelConfig, PutError> {
        let current = self
            .inner
            .lock()
            .map_or(1, |g| g.cfg.as_ref().map_or(1, |c| c.revision));
        if want != current {
            return Err(PutError::Conflict { current });
        }
        let cfg = ModelConfig {
            provider: parts.0,
            base_url: parts.1,
            model: parts.2,
            key_ref: parts.3,
            revision: current + 1,
        };
        if let Some(dir) = &self.dir {
            let body = json!({
                "version": "oo.docs.model/v1",
                "provider": cfg.provider, "base_url": cfg.base_url,
                "model": cfg.model, "key_ref": cfg.key_ref, "revision": cfg.revision,
            });
            private_write(&dir.join(CONFIG_FILE), body.to_string().as_bytes())
                .map_err(|_| PutError::Store)?;
        }
        self.start(cfg.clone());
        Ok(cfg)
    }

    /// Stop the egress process (engine shutdown).
    pub fn stop(&self) {
        if let Ok(mut g) = self.inner.lock() {
            Self::stop_child(&mut g);
        }
    }

    fn stop_child(g: &mut Inner) {
        if let Some(mut c) = g.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        g.port = None;
    }

    /// Record `cfg` and (re)start the egress for it.
    fn start(&self, cfg: ModelConfig) {
        let Ok(mut g) = self.inner.lock() else { return };
        Self::stop_child(&mut g);
        g.last_error = None;
        g.cfg = Some(cfg.clone());
        let (Some(dir), Some(script)) = (&self.dir, &self.script) else {
            g.last_error = Some("EGRESS_NOT_INSTALLED".into());
            return;
        };
        let stack = json!({"model": {
            "provider": cfg.provider,
            "base_url": if cfg.base_url.starts_with("file:") || cfg.base_url.starts_with("env:") {
                json!({"secret_ref": cfg.base_url})
            } else { json!(cfg.base_url) },
            "model": cfg.model,
            "key": {"secret_ref": cfg.key_ref},
        }});
        let conf = dir.join("model-egress.json");
        let meter = dir.join("model-usage.jsonl");
        if private_write(&conf, stack.to_string().as_bytes()).is_err() {
            g.last_error = Some("CONFIG_WRITE".into());
            return;
        }
        let Some(port) = free_port() else {
            g.last_error = Some("NO_PORT".into());
            return;
        };
        let upstream =
            model_timeout_secs(std::env::var("OO_DOCS_MODEL_TIMEOUT_SECS").ok().as_deref());
        let mut cmd = Command::new(&self.python);
        cmd.arg(script)
            .args(egress_args(
                &conf.to_string_lossy(),
                &meter.to_string_lossy(),
                port,
                EGRESS_RPM,
                upstream,
            ))
            .arg("--exit-with-parent")
            .arg(std::process::id().to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(_) => {
                g.last_error = Some("EGRESS_SPAWN".into());
                return;
            }
        };
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let t = Instant::now();
        loop {
            if let Ok(Some(_)) = child.try_wait() {
                // It refused (bad reference, permissions...): its first line is
                // `{"error":{"code":"..."}}`, a fixed vocabulary.
                let mut text = String::new();
                if let Some(mut o) = child.stdout.take() {
                    let _ = o.read_to_string(&mut text);
                }
                let code = serde_json::from_str::<Value>(text.trim())
                    .ok()
                    .and_then(|v| v["error"]["code"].as_str().map(str::to_owned))
                    .filter(|c| {
                        c.len() <= 40 && c.chars().all(|x| x.is_ascii_uppercase() || x == '_')
                    })
                    .unwrap_or_else(|| "EGRESS_EXITED".into());
                g.last_error = Some(code);
                return;
            }
            if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
                break;
            }
            if t.elapsed() > Duration::from_secs(8) {
                let _ = child.kill();
                let _ = child.wait();
                g.last_error = Some("EGRESS_TIMEOUT".into());
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        child.stdout.take();
        let mut client = EgressClient::new(addr, cfg.model.clone());
        client.timeout = Duration::from_secs(upstream + CLIENT_GRACE_SECS);
        g.port = Some(Arc::new(client));
        g.child = Some(child);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(k: &str, v: &str) -> Value {
        let mut b = json!({"provider": "openai-compatible", "base_url": "https://models.example.com/v1",
            "model": "m-1", "key_ref": "file:/tmp/k"});
        b[k] = json!(v);
        b
    }

    #[test]
    fn a_key_value_is_never_accepted() {
        for bad in [
            "sk-abc123",
            "abc",
            "file:relative",
            "env:1X",
            "env:A-B",
            "file:/a/../b",
        ] {
            let e = validate_put(&body("key_ref", bad)).unwrap_err();
            assert_eq!(e.pointer, "key_ref", "{bad}");
            assert!(
                !e.message.contains(bad),
                "the message must not echo the value"
            );
        }
        assert!(validate_put(&body("key_ref", "env:MODEL_KEY")).is_ok());
        assert!(validate_put(&body("key_ref", "file:/Users/x/.config/k")).is_ok());
    }

    #[test]
    fn endpoints_and_other_fields_are_checked() {
        for ok in [
            "https://a.example.com/v1",
            "http://127.0.0.1:8317/v1",
            "file:/x/endpoint",
            "env:EP",
        ] {
            assert!(validate_put(&body("base_url", ok)).is_ok(), "{ok}");
        }
        for bad in [
            "http://example.com/v1",
            "ftp://x",
            "https://u:p@h/",
            "https://h/?k=1",
            "example.com",
        ] {
            assert_eq!(
                validate_put(&body("base_url", bad)).unwrap_err().pointer,
                "base_url",
                "{bad}"
            );
        }
        assert_eq!(
            validate_put(&body("provider", "x")).unwrap_err().pointer,
            "provider"
        );
        assert_eq!(
            validate_put(&body("model", "a b")).unwrap_err().pointer,
            "model"
        );
        let mut extra = body("model", "m");
        extra["key"] = json!("sk-1");
        assert!(validate_put(&extra).is_err());
    }

    #[test]
    fn revisions_follow_if_match_and_the_file_holds_references_only() {
        let dir = std::env::temp_dir().join(format!("oo-model-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let rt = ModelRuntime::open(dir.clone(), None, None);
        assert_eq!(rt.status(), "not_configured");
        let parts = validate_put(&body("model", "m-1")).unwrap();
        assert_eq!(
            rt.put(5, parts.clone()),
            Err(PutError::Conflict { current: 1 })
        );
        let c = rt.put(1, parts.clone()).unwrap();
        assert_eq!(c.revision, 2);
        // No egress script: configured, but the stages cannot use it.
        assert_eq!(rt.status(), "unavailable");
        assert!(rt.port().is_none());
        let saved = std::fs::read_to_string(dir.join(CONFIG_FILE)).unwrap();
        assert!(saved.contains("file:/tmp/k") && !saved.contains("sk-"));
        // A restart finds it.
        let again = ModelRuntime::open(dir.clone(), None, None);
        assert_eq!(again.config().unwrap().revision, 2);
        assert_eq!(rt.put(1, parts), Err(PutError::Conflict { current: 2 }));
        let _ = std::fs::remove_dir_all(dir);
    }
}
