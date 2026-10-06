//! Caller config. The bearer token is read from a file and is not a JSON field.

use std::fs::File;
use std::io::Read;
use std::time::Duration;

use reqwest::Certificate;
use reqwest::Url;
use serde::Deserialize;
use serde_json::Value;

use crate::error::ProviderError;
use crate::price::Price;

const CONFIG_MAX: usize = 65_536;
const CA_MAX: usize = 65_536;
const BEARER_FILE_MAX: usize = 512;

pub(crate) struct Config {
    pub(crate) url: Url,
    pub(crate) model_id: String,
    pub(crate) certs: Vec<Certificate>,
    pub(crate) token: String,
    pub(crate) budget: Budget,
    pub(crate) price: Price,
}

#[derive(Debug, Clone)]
pub(crate) struct Budget {
    pub(crate) max_tables: usize,
    pub(crate) max_request_bytes: usize,
    pub(crate) max_response_bytes: usize,
    pub(crate) max_response_chunks: usize,
    pub(crate) deadline: Duration,
    pub(crate) max_prompt_tokens: u64,
    pub(crate) max_completion_tokens: u64,
    pub(crate) max_total_tokens: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigIn {
    protocol: String,
    endpoint: EndpointIn,
    residency: ResidencyIn,
    sample_policy: String,
    temperature: serde_json::Number,
    output: String,
    trust: TrustIn,
    secret: SecretIn,
    budget: BudgetIn,
    declared_price: PriceIn,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EndpointIn {
    origin: String,
    path: String,
    model_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResidencyIn {
    approved: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustIn {
    ca_file: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SecretIn {
    bearer_file: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BudgetIn {
    max_tables: u64,
    max_request_bytes: u64,
    max_response_bytes: u64,
    max_response_chunks: u64,
    deadline_ms: u64,
    max_prompt_tokens: u64,
    max_completion_tokens: u64,
    max_total_tokens: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PriceIn {
    currency: String,
    per_total_token: String,
}

impl Config {
    pub(crate) fn read_file(path: &str) -> Result<String, ProviderError> {
        let bytes = read_limited(path, CONFIG_MAX, "config.schema")?;
        String::from_utf8(bytes).map_err(|_| {
            ProviderError::new("config.schema", "the config is not one JSON object")
        })
    }

    pub(crate) fn parse(text: &str) -> Result<Self, ProviderError> {
        if text.len() > CONFIG_MAX {
            return Err(ProviderError::new(
                "config.schema",
                "the config exceeds the byte budget",
            ));
        }
        let value: Value = serde_json::from_str(text).map_err(|_| {
            ProviderError::new("config.schema", "the config is not one JSON object")
        })?;
        let parsed: ConfigIn = serde_json::from_value(value).map_err(|_| {
            ProviderError::new("config.schema", "the config does not match the provider schema")
        })?;
        if parsed.protocol != "oo.builder.description-provider-config/v1" {
            return Err(ProviderError::new(
                "config.schema",
                "the config protocol is not the description provider config",
            ));
        }
        if !parsed.residency.approved {
            return Err(ProviderError::new(
                "config.residency",
                "residency is not explicitly approved",
            ));
        }
        if parsed.sample_policy != "none" {
            return Err(ProviderError::new(
                "config.sample_policy",
                "the sample policy is not none",
            ));
        }
        if parsed.temperature.to_string() != "0" {
            return Err(ProviderError::new(
                "config.temperature",
                "temperature must be the integer zero",
            ));
        }
        if parsed.output != "json_schema" {
            return Err(ProviderError::new(
                "config.output",
                "the output mode is not json_schema",
            ));
        }
        if !model_ok(&parsed.endpoint.model_id) {
            return Err(ProviderError::new(
                "config.model",
                "the model id is not an allowed token",
            ));
        }
        if parsed.declared_price.currency != "USD" {
            return Err(ProviderError::new(
                "config.price",
                "the declared currency is not USD",
            ));
        }
        let url = join_url(&parsed.endpoint.origin, &parsed.endpoint.path)?;
        let budget = budget_of(parsed.budget)?;
        let price = Price::parse(&parsed.declared_price.per_total_token)?;
        let pem = read_limited(&parsed.trust.ca_file, CA_MAX, "config.trust")?;
        let certs = Certificate::from_pem_bundle(&pem).map_err(|_| {
            ProviderError::new("config.trust", "the trust file is not a certificate bundle")
        })?;
        if certs.is_empty() {
            return Err(ProviderError::new(
                "config.trust",
                "the trust file is not a certificate bundle",
            ));
        }
        let token = read_bearer(&parsed.secret.bearer_file)?;
        if text.contains(&token) {
            return Err(ProviderError::new(
                "config.secret",
                "a secret is present in the local documents",
            ));
        }
        Ok(Self {
            url,
            model_id: parsed.endpoint.model_id,
            certs,
            token,
            budget,
            price,
        })
    }
}

fn model_ok(model: &str) -> bool {
    let bytes = model.as_bytes();
    (1..=128).contains(&bytes.len())
        && bytes.iter().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-')
        })
}

fn join_url(origin: &str, path: &str) -> Result<Url, ProviderError> {
    if origin.len() > 256 || path.len() > 256 {
        return Err(ProviderError::new(
            "config.origin",
            "the approved origin is not an https URL",
        ));
    }
    let base = Url::parse(origin).map_err(|_| {
        ProviderError::new("config.origin", "the approved origin is not an https URL")
    })?;
    if base.scheme() != "https"
        || base.host_str().is_none()
        || !base.username().is_empty()
        || base.password().is_some()
        || base.query().is_some()
        || base.fragment().is_some()
        || !(base.path().is_empty() || base.path() == "/")
    {
        return Err(ProviderError::new(
            "config.origin",
            "the approved origin is not an https URL",
        ));
    }
    if !path_ok(path) {
        return Err(ProviderError::new(
            "config.path",
            "the approved path is not an absolute path",
        ));
    }
    let mut joined = base.clone();
    joined.set_path(path);
    if joined.scheme() != "https"
        || joined.host_str() != base.host_str()
        || joined.port() != base.port()
        || joined.path() != path
        || joined.query().is_some()
    {
        return Err(ProviderError::new(
            "config.origin",
            "the approved origin is not an https URL",
        ));
    }
    Ok(joined)
}

fn path_ok(path: &str) -> bool {
    path.starts_with('/')
        && !path.starts_with("//")
        && !path.contains("..")
        && !path.contains(['\\', '?', '#'])
        && path.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-' | b'~')
        })
}

fn budget_of(input: BudgetIn) -> Result<Budget, ProviderError> {
    let bad = ProviderError::new("config.budget", "a budget field is outside its range");
    let tables = range(input.max_tables, 1, 16, &bad)?;
    let request = range(input.max_request_bytes, 1, 65_536, &bad)?;
    let response = range(input.max_response_bytes, 1, 65_536, &bad)?;
    let chunks = range(input.max_response_chunks, 1, 64, &bad)?;
    let deadline = range(input.deadline_ms, 1, 30_000, &bad)?;
    if !(1..=100_000).contains(&input.max_prompt_tokens)
        || !(1..=100_000).contains(&input.max_completion_tokens)
        || !(1..=200_000).contains(&input.max_total_tokens)
    {
        return Err(bad);
    }
    Ok(Budget {
        max_tables: tables,
        max_request_bytes: request,
        max_response_bytes: response,
        max_response_chunks: chunks,
        deadline: Duration::from_millis(u64::try_from(deadline).unwrap_or(0)),
        max_prompt_tokens: input.max_prompt_tokens,
        max_completion_tokens: input.max_completion_tokens,
        max_total_tokens: input.max_total_tokens,
    })
}

fn range(value: u64, low: u64, high: u64, err: &ProviderError) -> Result<usize, ProviderError> {
    if !(low..=high).contains(&value) {
        return Err(err.clone());
    }
    usize::try_from(value).map_err(|_| err.clone())
}

fn read_bearer(path: &str) -> Result<String, ProviderError> {
    let bytes = read_limited(path, BEARER_FILE_MAX, "config.secret")?;
    let text = std::str::from_utf8(&bytes).map_err(|_| {
        ProviderError::new("config.secret", "the bearer file is not an allowed token")
    })?;
    let token = text.strip_suffix('\n').unwrap_or(text);
    let bytes = token.as_bytes();
    if !(16..=256).contains(&bytes.len())
        || !bytes.iter().all(|byte| (0x21..=0x7e).contains(byte))
    {
        return Err(ProviderError::new(
            "config.secret",
            "the bearer file is not an allowed token",
        ));
    }
    Ok(token.to_owned())
}

fn read_limited(path: &str, max: usize, code: &'static str) -> Result<Vec<u8>, ProviderError> {
    let mut file = File::open(path).map_err(|_| {
        ProviderError::new(code, "a local file could not be read")
    })?;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let read = file.read(&mut tmp).map_err(|_| {
            ProviderError::new(code, "a local file could not be read")
        })?;
        if read == 0 {
            break;
        }
        if buf.len().saturating_add(read) > max {
            return Err(ProviderError::new(code, "a local file exceeds its byte budget"));
        }
        buf.extend_from_slice(&tmp[..read]);
    }
    Ok(buf)
}

pub(crate) fn same_url(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
        && left.path() == right.path()
        && left.query().is_none()
        && right.query().is_none()
}
