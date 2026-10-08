//! HTTPS test issuer. Codes, PKCE, and RS256 signatures are real.
//! Fault modes still leave the token endpoint on the real code path.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{RawQuery, Request, State};
use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ring::rand::SystemRandom;
use ring::signature::{RsaKeyPair, RSA_PKCS1_SHA256};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

use control_identity::{pkcs8_rsa_public, server_config, TlsListener};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Ok,
    AlgNone,
    Hs256,
    WrongIssuer,
    WrongAudience,
    ConfiguredAudienceOnly,
    WrongAzp,
    MultiAud,
    MultiAudAzp,
    SingleAudNoAzp,
    BadNonce,
    Expired,
    FutureIat,
    UnknownKid,
    RotateKid,
    Ambiguous,
    IdpError,
}

struct Issued {
    challenge: String,
    nonce: String,
    used: bool,
}

struct SigningKey {
    kid: &'static str,
    pair: RsaKeyPair,
    n: String,
    e: String,
}

pub struct Idp {
    state: Arc<IdpState>,
}

struct IdpState {
    issuer: String,
    client_id: String,
    configured_audience: String,
    client_secret: String,
    redirect_uri: String,
    keys: Vec<SigningKey>,
    codes: Mutex<HashMap<String, Issued>>,
    mode: Mutex<Mode>,
    subject: Mutex<String>,
    jwks_hits: AtomicUsize,
    token_hits: AtomicUsize,
    rotate_arm: AtomicUsize,
}

pub struct IdpParts<'a> {
    pub cert_pem: &'a [u8],
    pub key_pem: &'a [u8],
    pub issuer: &'a str,
    pub client_id: &'a str,
    pub configured_audience: &'a str,
    pub client_secret: &'a str,
    pub redirect_uri: &'a str,
    pub der_keys: &'a [Vec<u8>],
}

pub async fn start(listener: TcpListener, parts: IdpParts<'_>) -> Idp {
    let IdpParts { cert_pem, key_pem, issuer, client_id, configured_audience, client_secret, redirect_uri, der_keys } = parts;
    let keys = der_keys
        .iter()
        .enumerate()
        .map(|(index, der)| {
            let (n, e) = pkcs8_rsa_public(der).expect("public");
            let pair = RsaKeyPair::from_pkcs8(der).expect("pair");
            let kid = if index == 0 { "k1" } else { "k2" };
            SigningKey { kid, pair, n: URL_SAFE_NO_PAD.encode(n), e: URL_SAFE_NO_PAD.encode(e) }
        })
        .collect();
    let state = Arc::new(IdpState {
        issuer: issuer.to_string(),
        client_id: client_id.to_string(),
        configured_audience: configured_audience.to_string(),
        client_secret: client_secret.to_string(),
        redirect_uri: redirect_uri.to_string(),
        keys,
        codes: Mutex::new(HashMap::new()),
        mode: Mutex::new(Mode::Ok),
        subject: Mutex::new("fixture-admin".to_string()),
        jwks_hits: AtomicUsize::new(0),
        token_hits: AtomicUsize::new(0),
        rotate_arm: AtomicUsize::new(0),
    });
    let tls = server_config(cert_pem, key_pem).expect("tls");
    let incoming = TlsListener { tcp: listener, acceptor: TlsAcceptor::from(Arc::new(tls)) };
    let router = router(Arc::clone(&state));
    tokio::spawn(async move {
        let _ = axum::serve(incoming, router).await;
    });
    Idp { state }
}

impl Idp {
    pub fn set_mode(&self, mode: Mode) {
        *self.state.mode.lock().expect("mode") = mode;
    }

    pub fn set_subject(&self, subject: &str) {
        *self.state.subject.lock().expect("subject") = subject.to_string();
    }

    pub fn jwks_hits(&self) -> usize {
        self.state.jwks_hits.load(Ordering::SeqCst)
    }

    pub fn token_hits(&self) -> usize {
        self.state.token_hits.load(Ordering::SeqCst)
    }

    pub fn arm_rotation(&self) {
        self.state.rotate_arm.store(self.jwks_hits().saturating_add(1), Ordering::SeqCst);
    }
}

fn router(state: Arc<IdpState>) -> Router {
    Router::new()
        .route("/.well-known/openid-configuration", get(discovery))
        .route("/jwks", get(jwks))
        .route("/authorize", get(authorize))
        .route("/token", post(token))
        .with_state(state)
}

async fn discovery(State(state): State<Arc<IdpState>>) -> Response {
    json_response(json!({
        "issuer": state.issuer,
        "authorization_endpoint": format!("{}/authorize", state.issuer),
        "token_endpoint": format!("{}/token", state.issuer),
        "jwks_uri": format!("{}/jwks", state.issuer),
        "response_types_supported": ["code"],
        "id_token_signing_alg_values_supported": ["RS256"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["client_secret_post"]
    }))
}

async fn jwks(State(state): State<Arc<IdpState>>) -> Response {
    let hit = state.jwks_hits.fetch_add(1, Ordering::SeqCst) + 1;
    let mode = *state.mode.lock().expect("mode");
    let keys = match mode {
        Mode::Ambiguous => vec![jwk(&state.keys[0]), jwk(&state.keys[0])],
        Mode::RotateKid if hit > state.rotate_arm.load(Ordering::SeqCst) => vec![jwk(&state.keys[0]), jwk(&state.keys[1])],
        _ => vec![jwk(&state.keys[0])],
    };
    json_response(json!({ "keys": keys }))
}

async fn authorize(State(state): State<Arc<IdpState>>, RawQuery(query): RawQuery) -> Response {
    let Some(query) = query else {
        return text(StatusCode::BAD_REQUEST);
    };
    let Ok(map) = form_map(&query) else {
        return text(StatusCode::BAD_REQUEST);
    };
    let required = ["response_type", "client_id", "redirect_uri", "scope", "state", "nonce", "code_challenge", "code_challenge_method"];
    if map.len() != required.len() || required.iter().any(|key| !map.contains_key(*key)) {
        return text(StatusCode::BAD_REQUEST);
    }
    if map["response_type"] != "code"
        || map["client_id"] != state.client_id
        || map["redirect_uri"] != state.redirect_uri
        || map["scope"] != "openid"
        || map["code_challenge_method"] != "S256"
        || map["code_challenge"].len() != 43
        || map["state"].is_empty()
        || map["nonce"].is_empty()
    {
        return text(StatusCode::BAD_REQUEST);
    }
    if *state.mode.lock().expect("mode") == Mode::IdpError {
        let location = format!("{}?error=access_denied&state={}", state.redirect_uri, url_encode(&map["state"]));
        return redirect(&location);
    }
    let code = random_token();
    state.codes.lock().expect("codes").insert(
        code.clone(),
        Issued { challenge: map["code_challenge"].clone(), nonce: map["nonce"].clone(), used: false },
    );
    redirect(&format!("{}?code={}&state={}", state.redirect_uri, url_encode(&code), url_encode(&map["state"])))
}

async fn token(State(state): State<Arc<IdpState>>, request: Request) -> Response {
    state.token_hits.fetch_add(1, Ordering::SeqCst);
    let bytes = axum::body::to_bytes(request.into_body(), 8192).await.unwrap_or_default();
    let Ok(body) = std::str::from_utf8(&bytes) else {
        return text(StatusCode::BAD_REQUEST);
    };
    let Ok(fields) = form_map(body) else {
        return text(StatusCode::BAD_REQUEST);
    };
    let required = ["grant_type", "code", "redirect_uri", "client_id", "code_verifier", "client_secret"];
    if fields.len() != required.len() || required.iter().any(|key| !fields.contains_key(*key)) {
        return text(StatusCode::BAD_REQUEST);
    }
    if fields["grant_type"] != "authorization_code"
        || fields["client_id"] != state.client_id
        || fields["redirect_uri"] != state.redirect_uri
        || !constant_eq(&fields["client_secret"], &state.client_secret)
    {
        return text(StatusCode::BAD_REQUEST);
    }
    let mut codes = state.codes.lock().expect("codes");
    let Some(issued) = codes.get_mut(fields["code"].as_str()) else {
        return text(StatusCode::BAD_REQUEST);
    };
    if issued.used || sha256_b64(&fields["code_verifier"]) != issued.challenge {
        return text(StatusCode::BAD_REQUEST);
    }
    issued.used = true;
    let nonce = issued.nonce.clone();
    drop(codes);
    let mode = *state.mode.lock().expect("mode");
    let subject = state.subject.lock().expect("subject").clone();
    let id_token = mint(&state, mode, &subject, &nonce);
    json_response(json!({ "id_token": id_token, "token_type": "Bearer", "expires_in": 300 }))
}

fn mint(state: &IdpState, mode: Mode, subject: &str, nonce: &str) -> String {
    let now = control_identity_now();
    let client = state.client_id.as_str();
    let configured = state.configured_audience.as_str();
    let (iss, aud, azp, token_nonce, exp, iat, nbf, key_index, alg) = match mode {
        Mode::WrongIssuer => ("https://evil.example", client, client, nonce, now + 300, now, now, 0, "RS256"),
        Mode::WrongAudience => (state.issuer.as_str(), "other-aud", client, nonce, now + 300, now, now, 0, "RS256"),
        Mode::ConfiguredAudienceOnly => (state.issuer.as_str(), configured, client, nonce, now + 300, now, now, 0, "RS256"),
        Mode::WrongAzp => (state.issuer.as_str(), client, "other-client", nonce, now + 300, now, now, 0, "RS256"),
        Mode::BadNonce => (state.issuer.as_str(), client, client, "wrong-nonce", now + 300, now, now, 0, "RS256"),
        Mode::Expired => (state.issuer.as_str(), client, client, nonce, now - 30, now - 120, now - 120, 0, "RS256"),
        Mode::FutureIat => (state.issuer.as_str(), client, client, nonce, now + 4000, now + 3600, now + 3600, 0, "RS256"),
        Mode::UnknownKid | Mode::RotateKid => (state.issuer.as_str(), client, client, nonce, now + 300, now, now, 1, "RS256"),
        Mode::AlgNone => (state.issuer.as_str(), client, client, nonce, now + 300, now, now, 0, "none"),
        Mode::Hs256 => (state.issuer.as_str(), client, client, nonce, now + 300, now, now, 0, "HS256"),
        _ => (state.issuer.as_str(), client, client, nonce, now + 300, now, now, 0, "RS256"),
    };
    let payload = match mode {
        Mode::MultiAud => format!(
            r#"{{"iss":"{}","aud":["{client}","extra"],"exp":{exp},"nbf":{nbf},"iat":{iat},"nonce":"{token_nonce}","sub":"{subject}"}}"#,
            state.issuer
        ),
        Mode::MultiAudAzp => format!(
            r#"{{"iss":"{}","aud":["{client}","extra"],"azp":"{client}","exp":{exp},"nbf":{nbf},"iat":{iat},"nonce":"{token_nonce}","sub":"{subject}"}}"#,
            state.issuer
        ),
        Mode::SingleAudNoAzp => format!(
            r#"{{"iss":"{iss}","aud":"{aud}","exp":{exp},"nbf":{nbf},"iat":{iat},"nonce":"{token_nonce}","sub":"{subject}"}}"#
        ),
        _ => format!(
            r#"{{"iss":"{iss}","aud":"{aud}","azp":"{azp}","exp":{exp},"nbf":{nbf},"iat":{iat},"nonce":"{token_nonce}","sub":"{subject}"}}"#
        ),
    };
    let kid = state.keys[key_index].kid;
    if alg != "RS256" {
        let header = URL_SAFE_NO_PAD.encode(format!(r#"{{"alg":"{alg}","kid":"{kid}"}}"#).as_bytes());
        let body = URL_SAFE_NO_PAD.encode(payload.as_bytes());
        return format!("{header}.{body}.x");
    }
    sign(&state.keys[key_index], &payload)
}

fn sign(key: &SigningKey, payload: &str) -> String {
    let header = URL_SAFE_NO_PAD.encode(format!(r#"{{"alg":"RS256","kid":"{}","typ":"JWT"}}"#, key.kid).as_bytes());
    let body = URL_SAFE_NO_PAD.encode(payload.as_bytes());
    let input = format!("{header}.{body}");
    let mut signature = vec![0_u8; key.pair.public().modulus_len()];
    key.pair.sign(&RSA_PKCS1_SHA256, &SystemRandom::new(), input.as_bytes(), &mut signature).expect("sign");
    format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature))
}

fn jwk(key: &SigningKey) -> Value {
    json!({ "kty": "RSA", "kid": key.kid, "use": "sig", "alg": "RS256", "n": key.n, "e": key.e })
}

fn control_identity_now() -> i64 {
    i64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)).unwrap_or(0)
}

fn sha256_b64(value: &str) -> String {
    URL_SAFE_NO_PAD.encode(ring::digest::digest(&ring::digest::SHA256, value.as_bytes()))
}

fn random_token() -> String {
    let mut bytes = [0_u8; 32];
    SystemRandom::new().fill(&mut bytes).expect("random");
    URL_SAFE_NO_PAD.encode(bytes)
}

fn form_map(body: &str) -> Result<HashMap<String, String>, ()> {
    let mut map = HashMap::new();
    if body.is_empty() {
        return Err(());
    }
    for pair in body.split('&') {
        let (key, value) = pair.split_once('=').ok_or(())?;
        if map.insert(percent_decode(key)?, percent_decode(value)?).is_some() {
            return Err(());
        }
    }
    Ok(map)
}

fn percent_decode(value: &str) -> Result<String, ()> {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(());
            }
            let high = hex(bytes[index + 1])?;
            let low = hex(bytes[index + 2])?;
            out.push((high << 4) | low);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).map_err(|_| ())
}

fn hex(byte: u8) -> Result<u8, ()> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(()),
    }
}

fn url_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(char::from(byte)),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn constant_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0_u8;
    for (left, right) in left.bytes().zip(right.bytes()) {
        diff |= left ^ right;
    }
    diff == 0
}

fn json_response(body: Value) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(body.to_string()))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn redirect(location: &str) -> Response {
    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, location)
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::empty())
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn text(status: StatusCode) -> Response {
    Response::builder().status(status).body(Body::from("rejected")).unwrap_or_else(|_| Response::new(Body::empty()))
}

trait FillRandom {
    fn fill(&self, dest: &mut [u8]) -> Result<(), ring::error::Unspecified>;
}

impl FillRandom for SystemRandom {
    fn fill(&self, dest: &mut [u8]) -> Result<(), ring::error::Unspecified> {
        ring::rand::SecureRandom::fill(self, dest)
    }
}
