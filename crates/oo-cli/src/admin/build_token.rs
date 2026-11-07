//! First-user issuance reserves the credential before the authenticated request.
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
    time::Duration,
};

use serde_json::{Value, json};

use super::{Failure, build_tail, review};
type Result<T> = std::result::Result<T, Failure>;

fn atomic(root: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    let temporary = root.join(format!("{name}-{}.tmp", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|_| Failure::local("BUILD_TOKEN_WRITE"))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| Failure::local("BUILD_TOKEN_WRITE"))?;
    fs::rename(temporary, root.join(name))
        .and_then(|_| fs::File::open(root)?.sync_all())
        .map_err(|_| Failure::local("BUILD_TOKEN_WRITE"))
}

fn token() -> Result<String> {
    let mut raw = [0u8; 32];
    fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut raw))
        .map_err(|_| Failure::local("BUILD_TOKEN_RANDOM"))?;
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut token = String::from("ooct_");
    for chunk in raw.chunks(3) {
        let bits = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for shift in [18, 12, 6, 0].into_iter().take(chunk.len() + 1) {
            token.push(ALPHABET[((bits >> shift) & 63) as usize] as char);
        }
    }
    Ok(token)
}

fn issue(publication: &Value, request: &Value) -> Result<Value> {
    let text = |key: &str| {
        publication[key]
            .as_str()
            .ok_or_else(|| Failure::local("BUILD_PUBLICATION_RECEIPT"))
    };
    let url = reqwest::Url::parse(text("gateway_url")?)
        .map_err(|_| Failure::local("BUILD_PUBLICATION_RECEIPT"))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || request["grant"]["audience_url"]
            .as_str()
            .is_none_or(|s| s.trim_end_matches('/') != url.as_str().trim_end_matches('/'))
    {
        return Err(Failure::local("BUILD_PUBLICATION_RECEIPT"));
    }
    let ca = reqwest::Certificate::from_pem_bundle(&crate::load::read_ca_file(text("ca_file")?)?)
        .map_err(|_| Failure::local("BUILD_TOKEN_TLS"))?;
    if ca.is_empty() {
        return Err(Failure::local("BUILD_TOKEN_TLS"));
    }
    let identity = reqwest::Identity::from_pem(&crate::load::read_client_pem(text("client_pem")?)?)
        .map_err(|_| Failure::local("BUILD_TOKEN_TLS"))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| Failure::local("BUILD_TOKEN_RUNTIME"))?;
    runtime.block_on(async {
        let client = reqwest::Client::builder()
            .https_only(true)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .tls_certs_only(ca)
            .identity(identity)
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|_| Failure::local("BUILD_TOKEN_TLS"))?;
        let mut response = client
            .post(format!(
                "{}/internal/v1/tokens",
                url.as_str().trim_end_matches('/')
            ))
            .header("content-type", "application/json")
            .body(request.to_string())
            .send()
            .await
            .map_err(|_| Failure::local("BUILD_TOKEN_REQUEST_RETRYABLE"))?;
        if !response.status().is_success() {
            return Err(Failure::local("BUILD_TOKEN_ISSUANCE_REFUSED"));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| Failure::local("BUILD_TOKEN_REQUEST_RETRYABLE"))?
        {
            if bytes.len() + chunk.len() > 16384 {
                return Err(Failure::local("BUILD_TOKEN_RESPONSE"));
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| Failure::local("BUILD_TOKEN_RESPONSE"))
    })
}

pub(super) fn run(root: &Path, publication: &Value) -> Result<Value> {
    run_with(root, publication, |request| issue(publication, request))
}

fn run_with(
    root: &Path,
    publication: &Value,
    execute: impl FnOnce(&Value) -> Result<Value>,
) -> Result<Value> {
    let fingerprint = review::hash(publication);
    let receipt = root.join("first_token-receipt.json");
    if receipt.exists() {
        let saved = review::read(&receipt)?;
        if saved["input_sha256"] != fingerprint {
            return Err(Failure::local("BUILD_TAIL_RECEIPT_BINDING"));
        }
        build_tail::verify_token(root)?;
        return Ok(saved["result"].clone());
    }
    let path = root.join("first-token-request.json");
    if !path.exists() {
        // Old versions could dispatch before saving material; never guess their outcome.
        if root.join("first_token-intent.json").exists() || root.join("first-user.token").exists() {
            return Err(Failure::local("BUILD_ISSUANCE_UNCERTAIN_RECONCILE_AUDIT"));
        }
        let token = token()?;
        let key = oo_core::api::payload_sha256(token.as_bytes());
        atomic(
            root,
            "first-token-request.json",
            json!({"input_sha256":fingerprint,
            "request":{"operation":"issue","grant":publication["first_user_grant"],
                "retry":{"key":key,"token":token}}})
            .to_string()
            .as_bytes(),
        )?;
    }
    let saved = review::read(&path)?;
    let request = &saved["request"];
    let token = request["retry"]["token"]
        .as_str()
        .ok_or_else(|| Failure::local("BUILD_TOKEN_REQUEST"))?;
    let hash = oo_core::api::payload_sha256(token.as_bytes());
    if saved["input_sha256"] != fingerprint
        || request["operation"] != "issue"
        || request["grant"] != publication["first_user_grant"]
        || request["retry"]["key"] != hash
        || token.len() != 48
        || !token.starts_with("ooct_")
        || !token[5..]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(Failure::local("BUILD_TAIL_RECEIPT_BINDING"));
    }
    if root.join("first-user.token").exists()
        && build_tail::credential_hash(&root.join("first-user.token"))? != hash
    {
        return Err(Failure::local("BUILD_TOKEN_RECEIPT"));
    }
    let response = execute(request)?;
    if response["token"] != token
        || response["token_sha256"] != hash
        || response["expires_at_ms"].as_i64().is_none_or(|n| n <= 0)
    {
        return Err(Failure::local("BUILD_TOKEN_RECEIPT"));
    }
    if !root.join("first-user.token").exists() {
        atomic(root, "first-user.token", token.as_bytes())?;
    }
    let result = json!({"token_sha256":hash,"expires_at_ms":response["expires_at_ms"]});
    atomic(
        root,
        "first_token-receipt.json",
        json!({"input_sha256":fingerprint,"result":result})
            .to_string()
            .as_bytes(),
    )?;
    build_tail::verify_token(root)?;
    Ok(result)
}

#[cfg(test)]
#[path = "build_token_tests.rs"]
mod tests;
