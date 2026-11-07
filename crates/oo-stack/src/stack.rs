use serde::{Deserialize, Serialize};

pub type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretRef {
    pub secret_ref: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stack {
    pub version: String,
    pub sources: Vec<Source>,
    pub model: Model,
    #[serde(default)]
    pub deploy: Deploy,
    #[serde(default)]
    pub gateway: Gateway,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: String,
    #[serde(default = "auto")]
    pub engine: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password: SecretRef,
    /// Caller trust anchor; source TLS always verifies the server hostname.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls_ca: Option<SecretRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh: Option<Ssh>,
    #[serde(default)]
    pub scope: Scope,
    #[serde(default)]
    pub sync: Sync,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ssh {
    pub host: String,
    #[serde(default = "ssh_port")]
    pub port: u16,
    pub user: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<SecretRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<SecretRef>,
    pub host_key_sha256: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    #[serde(default = "all")]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
}
impl Default for Scope {
    fn default() -> Self {
        Self {
            include: all(),
            exclude: vec![],
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sync {
    #[serde(default = "auto")]
    pub mode: String,
    #[serde(default = "interval")]
    pub interval: String,
}
impl Default for Sync {
    fn default() -> Self {
        Self {
            mode: auto(),
            interval: interval(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContextWindow {
    Auto(String),
    Tokens(u64),
}
impl Default for ContextWindow {
    fn default() -> Self {
        Self::Auto(auto())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ModelEndpoint {
    Url(String),
    Secret(SecretRef),
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub provider: String,
    pub base_url: ModelEndpoint,
    pub model: String,
    pub key: SecretRef,
    #[serde(default)]
    pub context_window: ContextWindow,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deploy {
    #[serde(default = "local")]
    pub target: String,
    #[serde(default = "lite")]
    pub profile: String,
    /// Remote host for `target: ssh`; credentials are references only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh: Option<DeploySsh>,
}
impl Default for Deploy {
    fn default() -> Self {
        Self {
            target: local(),
            profile: lite(),
            ssh: None,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploySsh {
    pub host: String,
    #[serde(default = "ssh_port")]
    pub port: u16,
    pub user: String,
    /// Exactly one of `key` (private key file) or `password`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<SecretRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<SecretRef>,
    pub host_key_sha256: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gateway {
    #[serde(default = "auto")]
    pub public_url: String,
}
impl Default for Gateway {
    fn default() -> Self {
        Self { public_url: auto() }
    }
}

fn auto() -> String {
    "auto".into()
}
fn interval() -> String {
    "8h".into()
}
fn ssh_port() -> u16 {
    22
}
fn local() -> String {
    "local".into()
}
fn lite() -> String {
    "lite".into()
}
fn all() -> Vec<String> {
    vec!["*".into()]
}

pub fn parse(text: &str) -> Result<Stack> {
    if text.len() > 1_048_576 {
        return Err("STACK_BUDGET");
    }
    // Deserialize directly: duplicate and unknown fields fail, including secret objects.
    let stack: Stack = if let Ok(stack) = serde_json::from_str(text) {
        stack
    } else {
        let options = serde_saphyr::options! {
            budget: serde_saphyr::budget! {
                max_aliases: 0,
                max_anchors: 0,
                max_depth: 32,
                max_documents: 1,
                max_total_scalar_bytes: 1_048_576,
            },
            merge_keys: serde_saphyr::MergeKeyPolicy::Error,
            reject_unsupported_tags: true,
            with_snippet: false,
        };
        serde_saphyr::from_str_with_options(text, options).map_err(|_| "STACK_SCHEMA")?
    };
    validate(&stack)?;
    Ok(stack)
}

pub fn secret_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}

fn text(value: &str) -> bool {
    !value.is_empty() && value.len() <= 2048 && !value.chars().any(char::is_control)
}

fn host(value: &str) -> bool {
    value.parse::<std::net::IpAddr>().is_ok()
        || (!value.is_empty()
            && value.len() <= 253
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b)))
}

/// Portable login name; never parsed by ssh as an option.
pub fn login_name(value: &str) -> bool {
    (1..=32).contains(&value.len())
        && (value.as_bytes()[0].is_ascii_alphanumeric() || value.as_bytes()[0] == b'_')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

/// Remote deployment host: a hostname or IP that cannot be read as an option.
pub fn ssh_host(value: &str) -> bool {
    host(value) && !value.starts_with('-')
}

pub fn interval_seconds(value: &str) -> Option<u64> {
    let (number, scale) = match value.as_bytes().last()? {
        b's' => (&value[..value.len() - 1], 1),
        b'm' => (&value[..value.len() - 1], 60),
        b'h' => (&value[..value.len() - 1], 3600),
        b'd' => (&value[..value.len() - 1], 86400),
        _ => return None,
    };
    if number.starts_with('0') || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    number
        .parse::<u64>()
        .ok()?
        .checked_mul(scale)
        .filter(|n| *n > 0)
}

pub fn fingerprint(value: &str) -> bool {
    // OpenSSH SHA256 is the canonical unpadded base64 of exactly 32 bytes.
    let Some(hash) = value.strip_prefix("SHA256:") else {
        return false;
    };
    hash.len() == 43
        && hash
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"+/".contains(&b))
        && b"AEIMQUYcgkosw048".contains(&hash.as_bytes()[42])
}

pub fn endpoint(value: &str) -> bool {
    if !text(value)
        || value.bytes().any(|b| b <= 32 || b >= 127)
        || value.contains(['@', '?', '#', '\\', '%'])
    {
        return false;
    }
    let Some((scheme, rest)) = value.split_once("://") else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or("");
    let (hostname, port) = if let Some(ipv6) = authority.strip_prefix('[') {
        let Some((ip, suffix)) = ipv6.split_once(']') else {
            return false;
        };
        if ip.parse::<std::net::Ipv6Addr>().is_err() {
            return false;
        }
        (
            ip,
            suffix
                .strip_prefix(':')
                .or(if suffix.is_empty() { None } else { Some("") }),
        )
    } else {
        match authority.split_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (authority, None),
        }
    };
    if !host(hostname) || port.is_some_and(|p| p.parse::<u16>().ok().filter(|n| *n > 0).is_none()) {
        return false;
    }
    scheme == "https"
        || (scheme == "http"
            && (hostname == "localhost"
                || hostname
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())))
}

fn looks_secret(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("-----begin ")
        || ["password=", "api_key=", "api-key=", "token=", "secret="]
            .iter()
            .any(|marker| lower.contains(marker))
        || lower.starts_with("sk-")
        || lower.starts_with("bearer ")
        || lower.starts_with("ghp_")
        || lower.starts_with("github_pat_")
        || lower.starts_with("xoxb-")
        || lower.starts_with("ooct_")
        || lower.starts_with("oost_")
}

pub fn validate(stack: &Stack) -> Result<()> {
    let value = serde_json::to_value(stack).map_err(|_| "STACK_SCHEMA")?;
    fn scan(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::String(s) => !looks_secret(s),
            serde_json::Value::Array(a) => a.iter().all(scan),
            serde_json::Value::Object(o) if o.len() == 1 && o.contains_key("secret_ref") => true,
            serde_json::Value::Object(o) => o.values().all(scan),
            _ => true,
        }
    }
    if !scan(&value) {
        return Err("STACK_INLINE_SECRET");
    }
    if stack.version != "oo.stack/v1" || stack.sources.is_empty() || stack.sources.len() > 1024 {
        return Err("STACK_SCHEMA");
    }
    let mut ids = std::collections::HashSet::new();
    for s in &stack.sources {
        if !secret_name(&s.id)
            || !ids.insert(&s.id)
            || !host(&s.host)
            || s.port == 0
            || !text(&s.database)
            || !text(&s.user)
            || !secret_name(&s.password.secret_ref)
            || s.tls_ca
                .as_ref()
                .is_some_and(|ca| !secret_name(&ca.secret_ref))
            || s.engine.is_empty()
            || s.engine.len() > 64
            || !s
                .engine
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err("STACK_SOURCE");
        }
        if !matches!(s.sync.mode.as_str(), "auto" | "C1" | "C5")
            || interval_seconds(&s.sync.interval).is_none()
        {
            return Err("STACK_INTERVAL");
        }
        if s.scope.include.is_empty()
            || !s
                .scope
                .include
                .iter()
                .chain(&s.scope.exclude)
                .all(|s| text(s))
        {
            return Err("STACK_SCOPE");
        }
        if let Some(ssh) = &s.ssh
            && (!host(&ssh.host)
                || ssh.port == 0
                || !text(&ssh.user)
                || ssh.key.is_some() == ssh.password.is_some()
                || ssh
                    .key
                    .iter()
                    .chain(ssh.password.iter())
                    .any(|r| !secret_name(&r.secret_ref))
                || !fingerprint(&ssh.host_key_sha256))
        {
            return Err("STACK_SSH");
        }
    }
    let m = &stack.model;
    if !matches!(m.provider.as_str(), "openai-compatible" | "anthropic")
        || !match &m.base_url {
            ModelEndpoint::Url(url) => endpoint(url),
            ModelEndpoint::Secret(reference) => secret_name(&reference.secret_ref),
        }
        || !text(&m.model)
        || !secret_name(&m.key.secret_ref)
    {
        return Err("STACK_MODEL");
    }
    match &m.context_window {
        ContextWindow::Auto(s) if s == "auto" => (),
        ContextWindow::Tokens(n) if *n > 0 => (),
        _ => return Err("STACK_CONTEXT_WINDOW"),
    }
    if !matches!(stack.deploy.target.as_str(), "local" | "ssh")
        || !matches!(stack.deploy.profile.as_str(), "lite" | "standard")
    {
        return Err("STACK_DEPLOY");
    }
    if let Some(ssh) = &stack.deploy.ssh
        && (!ssh_host(&ssh.host)
            || ssh.port == 0
            || !login_name(&ssh.user)
            || !fingerprint(&ssh.host_key_sha256)
            || ssh.key.is_some() == ssh.password.is_some()
            || ssh
                .key
                .iter()
                .chain(&ssh.password)
                .any(|r| !secret_name(&r.secret_ref)))
    {
        return Err("STACK_DEPLOY_SSH");
    }
    if stack.deploy.target == "ssh" && stack.deploy.ssh.is_none() {
        return Err("STACK_DEPLOY_SSH");
    }
    if stack.gateway.public_url != "auto" && !endpoint(&stack.gateway.public_url) {
        return Err("STACK_GATEWAY");
    }
    Ok(())
}
