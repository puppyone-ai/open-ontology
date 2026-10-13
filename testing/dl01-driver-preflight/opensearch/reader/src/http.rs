//! One audited reqwest client. Passwords stay in memory and are zeroized.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

pub struct Identities {
    pub dba_user: String,
    pub dba_password: String,
    pub reader_user: String,
    pub reader_password: String,
    pub endpoint: String,
    pub ca_pem: Vec<u8>,
}

impl Drop for Identities {
    fn drop(&mut self) {
        self.dba_password.zeroize();
        self.reader_password.zeroize();
    }
}

impl Identities {
    pub fn load(dir: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(dir.join("identities.env")).map_err(|error| format!("identities:{error}"))?;
        let mut pairs = std::collections::BTreeMap::new();
        for line in text.lines() {
            if let Some((key, value)) = line.split_once('=') {
                pairs.insert(key.to_string(), value.to_string());
            }
        }
        let endpoint = fs::read_to_string(dir.join("endpoint.url")).map_err(|error| format!("endpoint:{error}"))?;
        let ca_pem = fs::read(dir.join("certs").join("ca.pem")).map_err(|error| format!("ca:{error}"))?;
        Ok(Self {
            dba_user: pairs.get("dba_user").cloned().ok_or("missing dba_user")?,
            dba_password: pairs.get("dba_password").cloned().ok_or("missing dba_password")?,
            reader_user: pairs.get("reader_user").cloned().ok_or("missing reader_user")?,
            reader_password: pairs.get("reader_password").cloned().ok_or("missing reader_password")?,
            endpoint: endpoint.trim().trim_end_matches('/').to_string(),
            ca_pem,
        })
    }

    pub fn secrets(&self) -> Vec<String> {
        vec![self.dba_password.clone(), self.reader_password.clone()]
    }
}

#[derive(Clone, Debug)]
pub struct Audit {
    pub n: u64,
    pub identity: String,
    pub method: String,
    pub path: String,
    pub class: String,
    pub status: u16,
    pub bytes_out: u64,
    pub bytes_in: u64,
    pub error: Option<String>,
}

impl Audit {
    pub fn to_json(&self) -> Value {
        json!({
            "n": self.n,
            "identity": self.identity,
            "method": self.method,
            "path": self.path,
            "class": self.class,
            "status": self.status,
            "bytes_out": self.bytes_out,
            "bytes_in": self.bytes_in,
            "error": self.error,
        })
    }
}

pub struct Api {
    pub identity: String,
    http: reqwest::Client,
    base: String,
    user: String,
    password: String,
    pub audit: Vec<Audit>,
    seq: u64,
}

impl Drop for Api {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}

fn describe_transport(error: &reqwest::Error) -> String {
    let mut message = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(item) = source {
        message.push_str(": ");
        message.push_str(&item.to_string());
        source = std::error::Error::source(item);
    }
    let lower = message.to_ascii_lowercase();
    if error.is_timeout() && !lower.contains("timed out") && !lower.contains("timeout") {
        message.push_str(": operation timed out");
    }
    message
}

impl Api {
    pub fn new(identity: &str, user: &str, password: &str, endpoint: &str, ca_pem: &[u8], timeout: Duration) -> Result<Self, String> {
        let cert = reqwest::Certificate::from_pem(ca_pem).map_err(|error| format!("cert:{error}"))?;
        let http = reqwest::Client::builder()
            .tls_backend_rustls()
            .tls_certs_only(vec![cert])
            .tls_version_min(reqwest::tls::Version::TLS_1_2)
            .http1_only()
            .pool_max_idle_per_host(1)
            .timeout(timeout)
            .build()
            .map_err(|error| format!("client:{error}"))?;
        Ok(Self {
            identity: identity.to_string(),
            http,
            base: endpoint.to_string(),
            user: user.to_string(),
            password: password.to_string(),
            audit: Vec::new(),
            seq: 0,
        })
    }

    pub fn public_ca(_endpoint: &str, timeout: Duration) -> Result<reqwest::Client, String> {
        reqwest::Client::builder()
            .tls_backend_rustls()
            .tls_version_min(reqwest::tls::Version::TLS_1_2)
            .http1_only()
            .timeout(timeout)
            .build()
            .map_err(|error| format!("public_client:{error}"))
    }

    pub async fn call(&mut self, method: &str, path: &str, body: Option<Vec<u8>>, content_type: &str, class: &str) -> Result<(u16, Vec<u8>), String> {
        self.seq += 1;
        let url = format!("{}{path}", self.base);
        let mut request = self
            .http
            .request(method.parse().map_err(|_| format!("method:{method}"))?, url)
            .basic_auth(&self.user, Some(self.password.as_str()));
        let bytes_out = body.as_ref().map(Vec::len).unwrap_or(0) as u64;
        if let Some(body) = body {
            request = request.header("content-type", content_type).body(body);
        }
        let started = Instant::now();
        match request.send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                match response.bytes().await {
                    Ok(bytes) => {
                        let body = bytes.to_vec();
                        self.audit.push(Audit {
                            n: self.seq,
                            identity: self.identity.clone(),
                            method: method.to_string(),
                            path: path.to_string(),
                            class: class.to_string(),
                            status,
                            bytes_out,
                            bytes_in: body.len() as u64,
                            error: None,
                        });
                        let _ = started;
                        Ok((status, body))
                    }
                    Err(error) => {
                        let message = format!("body:{error}");
                        self.push_error(method, path, class, bytes_out, &message);
                        Err(message)
                    }
                }
            }
            Err(error) => {
                let message = format!("transport:{}", describe_transport(&error));
                self.push_error(method, path, class, bytes_out, &message);
                Err(message)
            }
        }
    }

    fn push_error(&mut self, method: &str, path: &str, class: &str, bytes_out: u64, message: &str) {
        self.audit.push(Audit {
            n: self.seq,
            identity: self.identity.clone(),
            method: method.to_string(),
            path: path.to_string(),
            class: class.to_string(),
            status: 0,
            bytes_out,
            bytes_in: 0,
            error: Some(message.to_string()),
        });
    }

    pub async fn delete_pit(&mut self, pit_id: &str) -> Result<u16, String> {
        let body = serde_json::to_vec(&json!({"pit_id": [pit_id]})).map_err(|error| format!("json:{error}"))?;
        let (status, _) = self
            .call("DELETE", "/_search/point_in_time", Some(body), "application/json", "cursor_close")
            .await?;
        Ok(status)
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub fn redact(text: &str, secrets: &[String]) -> String {
    let mut out = text.to_string();
    for secret in secrets {
        if secret.len() >= 8 {
            out = out.replace(secret, "[redacted]");
        }
    }
    out
}

pub fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("mkdir:{error}"))?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|error| format!("json:{error}"))?;
    fs::write(path, text + "\n").map_err(|error| format!("write:{error}"))
}

pub fn arg_map(args: &[String]) -> Result<(String, std::collections::HashMap<String, String>), String> {
    if args.len() < 2 {
        return Err("usage: os-reader <command> [--key value]".into());
    }
    let mut map = std::collections::HashMap::new();
    let mut index = 2;
    while index < args.len() {
        let key = args[index].as_str();
        if let Some(name) = key.strip_prefix("--") {
            let value = args.get(index + 1).ok_or_else(|| format!("missing value for {key}"))?;
            map.insert(name.to_string(), value.clone());
            index += 2;
        } else {
            return Err(format!("unexpected arg {key}"));
        }
    }
    Ok((args[1].clone(), map))
}

pub fn required<'a>(map: &'a std::collections::HashMap<String, String>, key: &str) -> Result<&'a str, String> {
    map.get(key).map(String::as_str).ok_or_else(|| format!("missing --{key}"))
}

pub fn number(map: &std::collections::HashMap<String, String>, key: &str, default: u64) -> Result<u64, String> {
    match map.get(key) {
        None => Ok(default),
        Some(value) => value.parse::<u64>().map_err(|_| format!("bad --{key}")),
    }
}
