//! Authorization Code + S256 against the pinned issuer. No redirects, no userinfo.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use reqwest::header::CONTENT_TYPE;
use zeroize::Zeroize;

use crate::config::Config;
use crate::cookie::percent_encode;
use crate::crypto::{jwt_header_kid, parse_discovery, parse_jwks, unix_now, verify_id_token, DiscoveryDoc, IdentityClaims, RsaKey, TokenBinding};
use crate::error::Fault;

const CACHE_TTL: Duration = Duration::from_secs(60);
const DEADLINE: Duration = Duration::from_secs(5);
const MAX_BYTES: usize = 65_536;

struct Timed<T> {
    value: T,
    fetched: Instant,
}

pub struct Oidc {
    client: reqwest::Client,
    issuer: String,
    client_id: String,
    client_secret: zeroize::Zeroizing<String>,
    skew: i64,
    discovery: Mutex<Option<Timed<DiscoveryDoc>>>,
    jwks: Mutex<Option<Timed<Vec<RsaKey>>>>,
}

impl Oidc {
    pub fn new(config: &Config) -> Result<Self, Fault> {
        let certs = reqwest::Certificate::from_pem_bundle(&config.idp_ca_pem).map_err(|_| Fault::malformed())?;
        if certs.is_empty() {
            return Err(Fault::malformed());
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_proxy()
            .https_only(true)
            .http1_only()
            .referer(false)
            .connection_verbose(false)
            .user_agent("oo-control-identity/v1")
            .tls_certs_only(certs)
            .connect_timeout(DEADLINE)
            .timeout(DEADLINE)
            .build()
            .map_err(|_| Fault::malformed())?;
        Ok(Self {
            client,
            issuer: config.issuer.clone(),
            client_id: config.client_id.clone(),
            client_secret: zeroize::Zeroizing::new(config.client_secret.to_string()),
            skew: config.clock_skew_seconds,
            discovery: Mutex::new(None),
            jwks: Mutex::new(None),
        })
    }

    pub async fn authorization_endpoint(&self) -> Result<String, Fault> {
        Ok(self.discovery().await?.authorization_endpoint)
    }

    pub async fn verify_code(&self, code: &str, redirect_uri: &str, verifier: &str, nonce_hash: &str) -> Result<IdentityClaims, Fault> {
        let endpoint = self.discovery().await?.token_endpoint;
        let mut body = token_form(&self.client_id, &self.client_secret, code, redirect_uri, verifier).into_bytes();
        let response = self
            .client
            .post(&endpoint)
            .timeout(DEADLINE)
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(body.clone())
            .send()
            .await;
        body.zeroize();
        let response = response.map_err(|_| Fault::token())?;
        let bytes = read_checked(response, &endpoint, Fault::token()).await?;
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| Fault::token())?;
        let token = value.get("id_token").and_then(|item| item.as_str()).ok_or_else(Fault::token)?;
        let kid = jwt_header_kid(token)?;
        let key = self.key_for(&kid).await?;
        verify_id_token(
            token,
            &key,
            &TokenBinding {
                issuer: &self.issuer,
                client_id: &self.client_id,
                nonce_hash,
                now: unix_now(),
                skew: self.skew,
            },
        )
    }

    async fn discovery(&self) -> Result<DiscoveryDoc, Fault> {
        if let Some(hit) = fresh(self.discovery.lock().unwrap_or_else(|poison| poison.into_inner())) {
            return Ok(hit);
        }
        let url = format!("{}/.well-known/openid-configuration", self.issuer);
        let bytes = self.get_bytes(&url, Fault::discovery()).await?;
        let document = parse_discovery(&bytes, &self.issuer)?;
        *self.discovery.lock().unwrap_or_else(|poison| poison.into_inner()) = Some(Timed { value: document.clone(), fetched: Instant::now() });
        Ok(document)
    }

    async fn key_for(&self, kid: &str) -> Result<RsaKey, Fault> {
        let keys = self.jwks(false).await?;
        if let Some(key) = keys.iter().find(|key| key.kid == kid) {
            return Ok(key.clone());
        }
        let keys = self.jwks(true).await?;
        keys.into_iter().find(|key| key.kid == kid).ok_or_else(Fault::kid_unknown)
    }

    async fn jwks(&self, force: bool) -> Result<Vec<RsaKey>, Fault> {
        if !force
            && let Some(hit) = fresh(self.jwks.lock().unwrap_or_else(|poison| poison.into_inner()))
        {
            return Ok(hit);
        }
        let url = self.discovery().await?.jwks_uri;
        let bytes = match self.get_bytes(&url, Fault::jwks()).await {
            Ok(bytes) => bytes,
            Err(fault) => return Err(fault),
        };
        let keys = parse_jwks(&bytes)?;
        *self.jwks.lock().unwrap_or_else(|poison| poison.into_inner()) = Some(Timed { value: keys.clone(), fetched: Instant::now() });
        Ok(keys)
    }

    async fn get_bytes(&self, url: &str, fault: Fault) -> Result<Vec<u8>, Fault> {
        let response = self.client.get(url).timeout(DEADLINE).send().await.map_err(|_| fault)?;
        read_checked(response, url, fault).await
    }
}

fn fresh<T: Clone>(guard: std::sync::MutexGuard<'_, Option<Timed<T>>>) -> Option<T> {
    guard.as_ref().filter(|item| item.fetched.elapsed() <= CACHE_TTL).map(|item| item.value.clone())
}

async fn read_checked(response: reqwest::Response, url: &str, fault: Fault) -> Result<Vec<u8>, Fault> {
    let parsed = reqwest::Url::parse(url).map_err(|_| fault)?;
    if response.url() != &parsed {
        return Err(fault);
    }
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(fault);
    }
    let bytes = response.bytes().await.map_err(|_| fault)?;
    if bytes.len() > MAX_BYTES {
        return Err(fault);
    }
    Ok(bytes.to_vec())
}

pub fn authorization_url(endpoint: &str, client_id: &str, redirect_uri: &str, state: &str, nonce: &str, challenge: &str) -> String {
    let query = format!(
        "response_type=code&client_id={}&redirect_uri={}&scope=openid&state={}&nonce={}&code_challenge={}&code_challenge_method=S256",
        percent_encode(client_id),
        percent_encode(redirect_uri),
        percent_encode(state),
        percent_encode(nonce),
        percent_encode(challenge)
    );
    format!("{endpoint}?{query}")
}

fn token_form(client_id: &str, client_secret: &str, code: &str, redirect_uri: &str, verifier: &str) -> String {
    format!(
        "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}&client_secret={}",
        percent_encode(code),
        percent_encode(redirect_uri),
        percent_encode(client_id),
        percent_encode(verifier),
        percent_encode(client_secret)
    )
}

#[cfg(test)]
mod tests {
    use super::authorization_url;

    #[test]
    fn authorization_url_is_s256_and_does_not_carry_bootstrap_material() {
        let url = authorization_url(
            "https://idp.example/authorize",
            "client",
            "https://control.example/control/v1/auth/callback",
            "state-value",
            "nonce-value",
            "challenge-value",
        );
        assert!(url.contains("response_type=code"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("scope=openid"));
        assert!(!url.contains("bootstrap"));
        assert!(!url.contains("client_secret"));
        assert!(!url.contains("code_verifier"));
    }
}
