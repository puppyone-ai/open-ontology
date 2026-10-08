//! Random values, hashes, and RS256 verification through ring.
//! JWT parsing is structural. The signature check is `RsaPublicKeyComponents`.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ring::signature::{RsaPublicKeyComponents, RSA_PKCS1_2048_8192_SHA256};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::error::Fault;

const B64_43: usize = 43;

pub fn random_32() -> Result<[u8; 32], Fault> {
    let mut bytes = [0_u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|_| Fault::unavailable())?;
    Ok(bytes)
}

pub fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn b64_decode(value: &str) -> Result<Vec<u8>, Fault> {
    if value.is_empty() || value.len() > 4096 || !value.bytes().all(is_b64_char) {
        return Err(Fault::token());
    }
    URL_SAFE_NO_PAD.decode(value).map_err(|_| Fault::token())
}

pub fn opaque_token() -> Result<String, Fault> {
    Ok(b64(&random_32()?))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(hex_digit(byte >> 4));
        out.push(hex_digit(byte & 0x0f));
    }
    out
}

pub fn hashes_equal(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, b"oo-identity-eq");
    let tag = ring::hmac::sign(&key, left.as_bytes());
    ring::hmac::verify(&key, right.as_bytes(), tag.as_ref()).is_ok()
}

pub fn pkce_challenge(verifier: &str) -> String {
    b64(&Sha256::digest(verifier.as_bytes()))
}

pub fn token_shape(value: &str) -> bool {
    value.len() == B64_43 && value.bytes().all(is_b64_char)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityClaims {
    pub subject: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RsaKey {
    pub kid: String,
    pub n: Vec<u8>,
    pub e: Vec<u8>,
}

pub fn jwt_header_kid(token: &str) -> Result<String, Fault> {
    let (header, _payload, _signature, _input) = split_jwt(token)?;
    let kid = header.get("kid").and_then(Value::as_str).ok_or_else(Fault::token)?;
    if !kid_ok(kid) {
        return Err(Fault::token());
    }
    Ok(kid.to_string())
}

pub struct TokenBinding<'a> {
    pub issuer: &'a str,
    pub client_id: &'a str,
    pub nonce_hash: &'a str,
    pub now: i64,
    pub skew: i64,
}

pub fn verify_id_token(token: &str, key: &RsaKey, binding: &TokenBinding<'_>) -> Result<IdentityClaims, Fault> {
    let (header, payload, signature, signing_input) = split_jwt(token)?;
    let header_kid = header.get("kid").and_then(Value::as_str).ok_or_else(Fault::token)?;
    if header_kid != key.kid {
        return Err(Fault::kid_unknown());
    }
    let components = RsaPublicKeyComponents { n: key.n.as_slice(), e: key.e.as_slice() };
    if components.verify(&RSA_PKCS1_2048_8192_SHA256, signing_input.as_bytes(), &signature).is_err() {
        return Err(Fault::token());
    }
    claims(&payload, binding)
}

pub fn parse_jwks(bytes: &[u8]) -> Result<Vec<RsaKey>, Fault> {
    if bytes.len() > 65_536 {
        return Err(Fault::jwks());
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| Fault::jwks())?;
    let keys = value.get("keys").and_then(Value::as_array).ok_or_else(Fault::jwks)?;
    if keys.is_empty() || keys.len() > 8 {
        return Err(Fault::jwks());
    }
    let mut parsed = Vec::with_capacity(keys.len());
    for key in keys {
        parsed.push(one_jwk(key)?);
    }
    for (index, key) in parsed.iter().enumerate() {
        if parsed.iter().skip(index + 1).any(|other| other.kid == key.kid) {
            return Err(Fault::kid_ambiguous());
        }
    }
    Ok(parsed)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveryDoc {
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub jwks_uri: String,
}

pub fn parse_discovery(bytes: &[u8], issuer: &str) -> Result<DiscoveryDoc, Fault> {
    if bytes.len() > 65_536 {
        return Err(Fault::discovery());
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| Fault::discovery())?;
    let document_issuer = text(&value, "issuer").ok_or_else(Fault::discovery)?;
    if document_issuer != issuer {
        return Err(Fault::issuer());
    }
    let authorization_endpoint = pinned_endpoint(issuer, text(&value, "authorization_endpoint"))?;
    let token_endpoint = pinned_endpoint(issuer, text(&value, "token_endpoint"))?;
    let jwks_uri = pinned_endpoint(issuer, text(&value, "jwks_uri"))?;
    if !string_list_contains(&value, "response_types_supported", "code")
        || !string_list_contains(&value, "id_token_signing_alg_values_supported", "RS256")
        || !string_list_contains(&value, "code_challenge_methods_supported", "S256")
    {
        return Err(Fault::discovery());
    }
    if value.get("token_endpoint_auth_methods_supported").is_some_and(|methods| !list_contains(methods, "client_secret_post")) {
        return Err(Fault::discovery());
    }
    Ok(DiscoveryDoc { authorization_endpoint, token_endpoint, jwks_uri })
}

pub fn pkcs8_rsa_public(der: &[u8]) -> Result<(Vec<u8>, Vec<u8>), Fault> {
    let (body, rest) = expect_tag(der, 0x30).ok_or_else(Fault::jwks)?;
    if !rest.is_empty() {
        return Err(Fault::jwks());
    }
    let (version, body) = expect_tag(body, 0x02).ok_or_else(Fault::jwks)?;
    if version != [0] {
        return Err(Fault::jwks());
    }
    let (_algorithm, body) = expect_tag(body, 0x30).ok_or_else(Fault::jwks)?;
    let (octet, _attributes) = expect_tag(body, 0x04).ok_or_else(Fault::jwks)?;
    let (private_key, rest) = expect_tag(octet, 0x30).ok_or_else(Fault::jwks)?;
    if !rest.is_empty() {
        return Err(Fault::jwks());
    }
    let (_version, private_key) = expect_tag(private_key, 0x02).ok_or_else(Fault::jwks)?;
    let (modulus, private_key) = expect_tag(private_key, 0x02).ok_or_else(Fault::jwks)?;
    let (exponent, _rest) = expect_tag(private_key, 0x02).ok_or_else(Fault::jwks)?;
    Ok((unsigned(modulus), unsigned(exponent)))
}

pub fn unix_now() -> i64 {
    i64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0))
        .unwrap_or(0)
}

fn claims(payload: &Value, binding: &TokenBinding<'_>) -> Result<IdentityClaims, Fault> {
    let token_issuer = text(payload, "iss").ok_or_else(Fault::issuer)?;
    if token_issuer != binding.issuer {
        return Err(Fault::issuer());
    }
    let audiences = audiences(payload.get("aud").ok_or_else(Fault::audience)?)?;
    // OIDC Core 1.0 §3.1.3.7. The configured audience is not a substitute.
    if !audiences.iter().any(|value| value == binding.client_id) {
        return Err(Fault::audience());
    }
    match payload.get("azp") {
        Some(value) => {
            let azp = value.as_str().ok_or_else(Fault::audience)?;
            if azp != binding.client_id {
                return Err(Fault::audience());
            }
        }
        None if audiences.len() > 1 => return Err(Fault::audience()),
        None => {}
    }
    let exp = integer(payload, "exp")?;
    let nbf = integer(payload, "nbf")?;
    let iat = integer(payload, "iat")?;
    if exp <= iat || exp.saturating_sub(iat) > 900 || nbf > exp || iat > binding.now.saturating_add(binding.skew) || nbf > binding.now.saturating_add(binding.skew)
    {
        return Err(Fault::token());
    }
    if exp <= binding.now.saturating_sub(binding.skew) {
        return Err(Fault::expired());
    }
    let nonce = text(payload, "nonce").ok_or_else(Fault::nonce)?;
    if !hashes_equal(&sha256_hex(nonce.as_bytes()), binding.nonce_hash) {
        return Err(Fault::nonce());
    }
    let subject = text(payload, "sub").ok_or_else(Fault::token)?;
    if !subject_ok(subject) {
        return Err(Fault::token());
    }
    Ok(IdentityClaims { subject: subject.to_string() })
}

fn split_jwt(token: &str) -> Result<(Value, Value, Vec<u8>, String), Fault> {
    if token.len() > 8192 || token.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return Err(Fault::token());
    }
    let mut parts = token.split('.');
    let header_b64 = parts.next().ok_or_else(Fault::token)?;
    let payload_b64 = parts.next().ok_or_else(Fault::token)?;
    let signature_b64 = parts.next().ok_or_else(Fault::token)?;
    if parts.next().is_some() || header_b64.is_empty() || payload_b64.is_empty() {
        return Err(Fault::token());
    }
    let header: Value = serde_json::from_slice(&b64_decode(header_b64)?).map_err(|_| Fault::token())?;
    let object = header.as_object().ok_or_else(Fault::token)?;
    for key in object.keys() {
        if key != "alg" && key != "kid" && key != "typ" {
            return Err(Fault::token());
        }
    }
    let alg = object.get("alg").and_then(Value::as_str).ok_or_else(Fault::alg)?;
    if alg != "RS256" {
        return Err(Fault::alg());
    }
    if object.get("typ").is_some_and(|typ| typ.as_str() != Some("JWT")) {
        return Err(Fault::token());
    }
    let payload: Value = serde_json::from_slice(&b64_decode(payload_b64)?).map_err(|_| Fault::token())?;
    if !payload.is_object() {
        return Err(Fault::token());
    }
    let signature = b64_decode(signature_b64)?;
    let signing_input = format!("{header_b64}.{payload_b64}");
    Ok((header, payload, signature, signing_input))
}

fn one_jwk(value: &Value) -> Result<RsaKey, Fault> {
    let object = value.as_object().ok_or_else(Fault::jwks)?;
    for name in ["d", "p", "q", "dp", "dq", "qi", "oth"] {
        if object.contains_key(name) {
            return Err(Fault::jwks());
        }
    }
    if object.get("kty").and_then(Value::as_str) != Some("RSA") {
        return Err(Fault::jwks());
    }
    if object.get("use").is_some_and(|use_value| use_value.as_str() != Some("sig")) || object.get("alg").is_some_and(|alg| alg.as_str() != Some("RS256")) {
        return Err(Fault::jwks());
    }
    if let Some(ops) = object.get("key_ops") {
        let list = ops.as_array().ok_or_else(Fault::jwks)?;
        if list.len() != 1 || list[0].as_str() != Some("verify") {
            return Err(Fault::jwks());
        }
    }
    let kid = object.get("kid").and_then(Value::as_str).ok_or_else(Fault::jwks)?;
    if !kid_ok(kid) {
        return Err(Fault::jwks());
    }
    let n_b64 = object.get("n").and_then(Value::as_str).ok_or_else(Fault::jwks)?;
    let e_b64 = object.get("e").and_then(Value::as_str).ok_or_else(Fault::jwks)?;
    let n = unsigned(&b64_decode(n_b64).map_err(|_| Fault::jwks())?);
    let e = unsigned(&b64_decode(e_b64).map_err(|_| Fault::jwks())?);
    if e != [0x01, 0x00, 0x01] {
        return Err(Fault::jwks());
    }
    let bits = bit_len(&n);
    if !(2048..=8192).contains(&bits) {
        return Err(Fault::jwks());
    }
    Ok(RsaKey { kid: kid.to_string(), n, e })
}

fn audiences(value: &Value) -> Result<Vec<String>, Fault> {
    let mut out = Vec::new();
    match value {
        Value::String(one) => out.push(one.clone()),
        Value::Array(list) if !list.is_empty() && list.len() <= 8 => {
            for item in list {
                let text = item.as_str().ok_or_else(Fault::audience)?;
                out.push(text.to_string());
            }
        }
        _ => return Err(Fault::audience()),
    }
    if out.iter().any(|item| item.is_empty() || item.len() > 128) {
        return Err(Fault::audience());
    }
    for (index, item) in out.iter().enumerate() {
        if out.iter().skip(index + 1).any(|other| other == item) {
            return Err(Fault::audience());
        }
    }
    Ok(out)
}

fn integer(value: &Value, key: &str) -> Result<i64, Fault> {
    value.get(key).and_then(Value::as_i64).filter(|number| *number > 0).ok_or_else(Fault::token)
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str).filter(|text| !text.is_empty())
}

fn string_list_contains(value: &Value, key: &str, expected: &str) -> bool {
    value.get(key).is_some_and(|list| list_contains(list, expected))
}

fn list_contains(value: &Value, expected: &str) -> bool {
    value.as_array().is_some_and(|list| list.iter().any(|item| item.as_str() == Some(expected)))
}

fn pinned_endpoint(issuer: &str, endpoint: Option<&str>) -> Result<String, Fault> {
    let endpoint = endpoint.ok_or_else(Fault::discovery)?;
    let prefix = format!("{issuer}/");
    if !endpoint.starts_with(&prefix) {
        return Err(Fault::discovery());
    }
    let rest = &endpoint[prefix.len()..];
    if rest.is_empty() || rest.contains(['?', '#', '@', '\\', ' ']) {
        return Err(Fault::discovery());
    }
    Ok(endpoint.to_string())
}

fn kid_ok(value: &str) -> bool {
    (1..=128).contains(&value.len()) && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
}

pub fn subject_ok(value: &str) -> bool {
    (1..=256).contains(&value.len()) && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
}

fn bit_len(bytes: &[u8]) -> usize {
    let Some(start) = bytes.iter().position(|byte| *byte != 0) else {
        return 0;
    };
    let zeros = usize::try_from(bytes[start].leading_zeros()).unwrap_or(8);
    (bytes.len() - start) * 8 - zeros
}

fn is_b64_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
}

fn hex_digit(value: u8) -> char {
    char::from(b"0123456789abcdef"[usize::from(value)])
}

fn unsigned(bytes: &[u8]) -> Vec<u8> {
    match bytes.split_first() {
        Some((0, rest)) if !rest.is_empty() => rest.to_vec(),
        _ => bytes.to_vec(),
    }
}

fn expect_tag(input: &[u8], tag: u8) -> Option<(&[u8], &[u8])> {
    let (got, body, rest) = read_tlv(input)?;
    if got != tag {
        return None;
    }
    Some((body, rest))
}

fn read_tlv(input: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = input.split_first()?;
    let (len, rest) = read_len(rest)?;
    if rest.len() < len {
        return None;
    }
    Some((tag, &rest[..len], &rest[len..]))
}

fn read_len(input: &[u8]) -> Option<(usize, &[u8])> {
    let (&first, rest) = input.split_first()?;
    if first < 0x80 {
        return Some((usize::from(first), rest));
    }
    let count = usize::from(first & 0x7f);
    if count == 0 || count > 3 || rest.len() < count {
        return None;
    }
    let mut value = 0_usize;
    for byte in &rest[..count] {
        value = value.checked_shl(8)?.checked_add(usize::from(*byte))?;
    }
    Some((value, &rest[count..]))
}

#[cfg(test)]
mod tests {
    use super::{b64, jwt_header_kid, pkcs8_rsa_public, sha256_hex, verify_id_token, Fault, RsaKey, TokenBinding};
    use ring::rand::SystemRandom;
    use ring::signature::{RsaKeyPair, RSA_PKCS1_SHA256};

    #[test]
    fn sha256_hex_matches_the_empty_and_abc_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn alg_none_and_hs256_are_rejected_before_trust() {
        let header = b64(br#"{"alg":"none","kid":"k1"}"#);
        let payload = b64(br#"{"sub":"fixture-admin"}"#);
        let token = format!("{header}.{payload}.");
        assert_eq!(jwt_header_kid(&token), Err(Fault::alg()));
        let hs = b64(br#"{"alg":"HS256","kid":"k1"}"#);
        let token = format!("{hs}.{payload}.aaaa");
        assert_eq!(jwt_header_kid(&token), Err(Fault::alg()));
    }

    #[test]
    fn ring_signed_token_verifies_and_rejects_a_tampered_claim() {
        let der = openssl_pkcs8();
        let (n, e) = pkcs8_rsa_public(&der).expect("public");
        let pair = RsaKeyPair::from_pkcs8(&der).expect("pair");
        let now = 1_700_000_000_i64;
        let key = RsaKey { kid: "k1".to_string(), n, e };
        let nonce_hash = sha256_hex(b"nonce-value");
        let binding = TokenBinding {
            issuer: "https://idp.example",
            client_id: "oo-client",
            nonce_hash: &nonce_hash,
            now,
            skew: 60,
        };
        let claims = verify_id_token(&sign(&pair, "k1", &claim(now, r#""aud":"oo-client","azp":"oo-client""#)), &key, &binding).expect("verify");
        assert_eq!(claims.subject, "fixture-admin");
        let mut tampered = sign(&pair, "k1", &claim(now, r#""aud":"oo-client","azp":"oo-client""#));
        let last = tampered.pop().expect("sig");
        tampered.push(if last == 'A' { 'B' } else { 'A' });
        assert_eq!(verify_id_token(&tampered, &key, &binding), Err(Fault::token()));
        assert_eq!(
            verify_id_token(&sign(&pair, "k1", &claim(now, r#""aud":"oo-client""#)), &key, &binding).map(|item| item.subject),
            Ok("fixture-admin".to_string())
        );
        assert_eq!(
            verify_id_token(&sign(&pair, "k1", &claim(now, r#""aud":["oo-client","extra"],"azp":"oo-client""#)), &key, &binding)
                .map(|item| item.subject),
            Ok("fixture-admin".to_string())
        );
        for aud in [r#""aud":"oo-control","azp":"oo-client""#, r#""aud":"other-aud","azp":"oo-client""#, r#""aud":["oo-client","extra"]"#, r#""aud":["oo-control","extra"],"azp":"oo-client""#, r#""aud":["oo-client","extra"],"azp":"other-client""#] {
            assert_eq!(verify_id_token(&sign(&pair, "k1", &claim(now, aud)), &key, &binding), Err(Fault::audience()));
        }
    }

    fn claim(now: i64, aud: &str) -> String {
        format!(
            r#"{{"iss":"https://idp.example",{aud},"exp":{},"nbf":{},"iat":{},"nonce":"nonce-value","sub":"fixture-admin"}}"#,
            now + 300,
            now,
            now
        )
    }

    fn sign(pair: &RsaKeyPair, kid: &str, payload: &str) -> String {
        let header = b64(format!(r#"{{"alg":"RS256","kid":"{kid}","typ":"JWT"}}"#).as_bytes());
        let payload_b64 = b64(payload.as_bytes());
        let input = format!("{header}.{payload_b64}");
        let mut signature = vec![0_u8; pair.public().modulus_len()];
        pair.sign(&RSA_PKCS1_SHA256, &SystemRandom::new(), input.as_bytes(), &mut signature).expect("sign");
        format!("{input}.{}", b64(&signature))
    }

    fn openssl_pkcs8() -> Vec<u8> {
        let dir = std::env::temp_dir().join(format!("oo-identity-key-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let pem = dir.join("key.pem");
        let key = dir.join("key.pk8");
        let generated = std::process::Command::new("openssl")
            .args(["genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048", "-out"])
            .arg(&pem)
            .status()
            .expect("openssl");
        assert!(generated.success());
        let wrapped = std::process::Command::new("openssl")
            .args(["pkcs8", "-topk8", "-nocrypt", "-in"])
            .arg(&pem)
            .args(["-outform", "DER", "-out"])
            .arg(&key)
            .status()
            .expect("openssl");
        assert!(wrapped.success());
        let bytes = std::fs::read(&key).expect("read");
        let _ = std::fs::remove_dir_all(&dir);
        bytes
    }
}
