//! Ok-mode HTTPS issuer for this card's proof. Negative token modes are not copied.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{RsaKeyPair, RSA_PKCS1_SHA256};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;

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

struct State {
    issuer: String,
    client_id: String,
    client_secret: String,
    redirect_uri: String,
    key: SigningKey,
    codes: Mutex<HashMap<String, Issued>>,
    subject: Mutex<String>,
}

pub struct Idp {
    state: Arc<State>,
}

pub struct Parts<'a> {
    pub cert_pem: &'a [u8],
    pub key_pem: &'a [u8],
    pub issuer: &'a str,
    pub client_id: &'a str,
    pub client_secret: &'a str,
    pub redirect_uri: &'a str,
    pub pkcs8_der: &'a [u8],
}

pub async fn start(listener: TcpListener, parts: Parts<'_>) -> Idp {
    let (n, e) = rsa_public(parts.pkcs8_der).expect("rsa public");
    let pair = RsaKeyPair::from_pkcs8(parts.pkcs8_der).expect("rsa pair");
    let state = Arc::new(State {
        issuer: parts.issuer.to_owned(),
        client_id: parts.client_id.to_owned(),
        client_secret: parts.client_secret.to_owned(),
        redirect_uri: parts.redirect_uri.to_owned(),
        key: SigningKey { kid: "k1", pair, n: URL_SAFE_NO_PAD.encode(n), e: URL_SAFE_NO_PAD.encode(e) },
        codes: Mutex::new(HashMap::new()),
        subject: Mutex::new("subadmin".to_owned()),
    });
    let tls = server_config(parts.cert_pem, parts.key_pem).expect("idp tls");
    let acceptor = TlsAcceptor::from(Arc::new(tls));
    let shared = Arc::clone(&state);
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else { continue };
            let acceptor = acceptor.clone();
            let state = Arc::clone(&shared);
            tokio::spawn(async move {
                let Ok(stream) = acceptor.accept(tcp).await else { return };
                let _ = handle(stream, state).await;
            });
        }
    });
    Idp { state }
}

impl Idp {
    pub fn set_subject(&self, subject: &str) {
        *self.state.subject.lock().expect("subject") = subject.to_owned();
    }
}

async fn handle(mut stream: tokio_rustls::server::TlsStream<TcpStream>, state: Arc<State>) -> Result<(), ()> {
    let (method, target, body) = read_request(&mut stream).await?;
    let (path, query) = target.split_once('?').unwrap_or((target.as_str(), ""));
    let response = match (method.as_str(), path) {
        ("GET", "/.well-known/openid-configuration") => json_bytes(&json!({
            "issuer": state.issuer,
            "authorization_endpoint": format!("{}/authorize", state.issuer),
            "token_endpoint": format!("{}/token", state.issuer),
            "jwks_uri": format!("{}/jwks", state.issuer),
            "response_types_supported": ["code"],
            "id_token_signing_alg_values_supported": ["RS256"],
            "code_challenge_methods_supported": ["S256"],
            "token_endpoint_auth_methods_supported": ["client_secret_post"]
        })),
        ("GET", "/jwks") => json_bytes(&json!({
            "keys": [{
                "kty": "RSA",
                "kid": state.key.kid,
                "use": "sig",
                "alg": "RS256",
                "n": state.key.n,
                "e": state.key.e
            }]
        })),
        ("GET", "/authorize") => authorize(&state, query),
        ("POST", "/token") => token(&state, &body),
        _ => text(404, "missing"),
    };
    stream.write_all(&response).await.map_err(|_| ())?;
    Ok(())
}

fn authorize(state: &State, query: &str) -> Vec<u8> {
    let Ok(map) = form_map(query) else { return text(400, "query") };
    let required = ["response_type", "client_id", "redirect_uri", "scope", "state", "nonce", "code_challenge", "code_challenge_method"];
    if map.len() != required.len() || required.iter().any(|key| !map.contains_key(*key)) {
        return text(400, "query");
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
        return text(400, "query");
    }
    let code = random_token();
    state.codes.lock().expect("codes").insert(code.clone(), Issued {
        challenge: map["code_challenge"].clone(),
        nonce: map["nonce"].clone(),
        used: false,
    });
    redirect(&format!("{}?code={}&state={}", state.redirect_uri, url_encode(&code), url_encode(&map["state"])))
}

fn token(state: &State, body: &[u8]) -> Vec<u8> {
    let Ok(text_body) = std::str::from_utf8(body) else { return text(400, "body") };
    let Ok(fields) = form_map(text_body) else { return text(400, "form") };
    let required = ["grant_type", "code", "redirect_uri", "client_id", "code_verifier", "client_secret"];
    if fields.len() != required.len() || required.iter().any(|key| !fields.contains_key(*key)) {
        return text(400, "form");
    }
    if fields["grant_type"] != "authorization_code"
        || fields["client_id"] != state.client_id
        || fields["redirect_uri"] != state.redirect_uri
        || fields["client_secret"] != state.client_secret
    {
        return text(400, "client");
    }
    let mut codes = state.codes.lock().expect("codes");
    let Some(issued) = codes.get_mut(fields["code"].as_str()) else { return text(400, "code") };
    if issued.used || sha256_b64(&fields["code_verifier"]) != issued.challenge {
        return text(400, "pkce");
    }
    issued.used = true;
    let nonce = issued.nonce.clone();
    drop(codes);
    let subject = state.subject.lock().expect("subject").clone();
    let id_token = sign(state, &subject, &nonce);
    json_bytes(&json!({"id_token": id_token, "token_type": "Bearer", "expires_in": 300}))
}

fn sign(state: &State, subject: &str, nonce: &str) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let payload = format!(
        r#"{{"iss":"{}","aud":"{}","azp":"{}","exp":{},"nbf":{now},"iat":{now},"nonce":"{nonce}","sub":"{subject}"}}"#,
        state.issuer,
        state.client_id,
        state.client_id,
        now + 300
    );
    let header = URL_SAFE_NO_PAD.encode(format!(r#"{{"alg":"RS256","kid":"{}","typ":"JWT"}}"#, state.key.kid).as_bytes());
    let body = URL_SAFE_NO_PAD.encode(payload.as_bytes());
    let input = format!("{header}.{body}");
    let mut signature = vec![0_u8; state.key.pair.public().modulus_len()];
    state.key.pair.sign(&RSA_PKCS1_SHA256, &SystemRandom::new(), input.as_bytes(), &mut signature).expect("sign");
    format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature))
}

fn server_config(cert_pem: &[u8], key_pem: &[u8]) -> Result<ServerConfig, ()> {
    let certs = CertificateDer::pem_slice_iter(cert_pem).collect::<Result<Vec<_>, _>>().map_err(|_| ())?;
    let key = PrivateKeyDer::from_pem_slice(key_pem).map_err(|_| ())?;
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let mut config = ServerConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .map_err(|_| ())?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|_| ())?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(config)
}

async fn read_request(stream: &mut tokio_rustls::server::TlsStream<TcpStream>) -> Result<(String, String, Vec<u8>), ()> {
    let mut buf = Vec::new();
    let head_end = loop {
        let mut chunk = [0_u8; 2048];
        let n = stream.read(&mut chunk).await.map_err(|_| ())?;
        if n == 0 {
            return Err(());
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(end) = buf.windows(4).position(|window| window == b"\r\n\r\n") {
            break end;
        }
        if buf.len() > 65_536 {
            return Err(());
        }
    };
    let head = std::str::from_utf8(&buf[..head_end]).map_err(|_| ())?;
    let mut lines = head.lines();
    let request = lines.next().ok_or(())?;
    let mut parts = request.split(' ');
    let method = parts.next().unwrap_or("").to_owned();
    let target = parts.next().unwrap_or("").to_owned();
    let mut length = 0_usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().map_err(|_| ())?;
        }
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < length {
        let mut chunk = [0_u8; 2048];
        let n = stream.read(&mut chunk).await.map_err(|_| ())?;
        if n == 0 {
            return Err(());
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(length);
    Ok((method, target, body))
}

fn json_bytes(value: &Value) -> Vec<u8> {
    text_typed(200, "application/json", &value.to_string())
}

fn text(status: u16, body: &str) -> Vec<u8> {
    text_typed(status, "text/plain", body)
}

fn text_typed(status: u16, content_type: &str, body: &str) -> Vec<u8> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Error",
    };
    format!("HTTP/1.1 {status} {reason}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).into_bytes()
}

fn redirect(location: &str) -> Vec<u8> {
    format!("HTTP/1.1 302 Found\r\nlocation: {location}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n").into_bytes()
}

fn random_token() -> String {
    let mut bytes = [0_u8; 32];
    SystemRandom::new().fill(&mut bytes).expect("random");
    URL_SAFE_NO_PAD.encode(bytes)
}

fn sha256_b64(value: &str) -> String {
    URL_SAFE_NO_PAD.encode(ring::digest::digest(&ring::digest::SHA256, value.as_bytes()))
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
        } else if bytes[index] == b'+' {
            out.push(b' ');
            index += 1;
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
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(char::from(b"0123456789ABCDEF"[(byte >> 4) as usize]));
            out.push(char::from(b"0123456789ABCDEF"[(byte & 0x0f) as usize]));
        }
    }
    out
}

fn rsa_public(der: &[u8]) -> Result<(Vec<u8>, Vec<u8>), ()> {
    let (body, rest) = der_tag(der, 0x30).ok_or(())?;
    if !rest.is_empty() {
        return Err(());
    }
    let (version, body) = der_tag(body, 0x02).ok_or(())?;
    if version != [0] {
        return Err(());
    }
    let (_algorithm, body) = der_tag(body, 0x30).ok_or(())?;
    let (octet, _) = der_tag(body, 0x04).ok_or(())?;
    let (private_key, rest) = der_tag(octet, 0x30).ok_or(())?;
    if !rest.is_empty() {
        return Err(());
    }
    let (_version, private_key) = der_tag(private_key, 0x02).ok_or(())?;
    let (modulus, private_key) = der_tag(private_key, 0x02).ok_or(())?;
    let (exponent, _) = der_tag(private_key, 0x02).ok_or(())?;
    Ok((unsigned(modulus), unsigned(exponent)))
}

fn der_tag(bytes: &[u8], tag: u8) -> Option<(&[u8], &[u8])> {
    if bytes.first().copied()? != tag {
        return None;
    }
    let (len, rest) = der_len(&bytes[1..])?;
    if rest.len() < len {
        return None;
    }
    Some((&rest[..len], &rest[len..]))
}

fn der_len(bytes: &[u8]) -> Option<(usize, &[u8])> {
    let first = *bytes.first()?;
    if first < 0x80 {
        return Some((first as usize, &bytes[1..]));
    }
    let count = (first & 0x7f) as usize;
    if count == 0 || count > 3 || bytes.len() < 1 + count {
        return None;
    }
    let mut len = 0_usize;
    for byte in &bytes[1..1 + count] {
        len = (len << 8) | *byte as usize;
    }
    Some((len, &bytes[1 + count..]))
}

fn unsigned(bytes: &[u8]) -> Vec<u8> {
    let start = bytes.iter().position(|byte| *byte != 0).unwrap_or(bytes.len().saturating_sub(1));
    bytes[start..].to_vec()
}
