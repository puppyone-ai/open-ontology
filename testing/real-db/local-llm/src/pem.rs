use base64::Engine;

use crate::fault::Fault;

pub fn cert_der(pem: &[u8]) -> Result<Vec<Vec<u8>>, Fault> {
    let text = std::str::from_utf8(pem).map_err(|_| Fault::tls())?;
    if text.contains("PRIVATE KEY") || !text.contains("BEGIN CERTIFICATE") {
        return Err(Fault::tls());
    }
    let mut certs = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("-----BEGIN CERTIFICATE-----") {
        let after = &rest[start + "-----BEGIN CERTIFICATE-----".len()..];
        let end = after.find("-----END CERTIFICATE-----").ok_or_else(Fault::tls)?;
        certs.push(decode_b64(&after[..end])?);
        rest = &after[end + "-----END CERTIFICATE-----".len()..];
    }
    if certs.is_empty() {
        return Err(Fault::tls());
    }
    Ok(certs)
}

pub fn key_der(pem: &[u8]) -> Result<Vec<u8>, Fault> {
    let text = std::str::from_utf8(pem).map_err(|_| Fault::tls())?;
    let label = if text.contains("BEGIN PRIVATE KEY") {
        "PRIVATE KEY"
    } else if text.contains("BEGIN RSA PRIVATE KEY") {
        "RSA PRIVATE KEY"
    } else {
        return Err(Fault::tls());
    };
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let start = text.find(&begin).ok_or_else(Fault::tls)?;
    let after = &text[start + begin.len()..];
    let stop = after.find(&end).ok_or_else(Fault::tls)?;
    decode_b64(&after[..stop])
}

fn decode_b64(body: &str) -> Result<Vec<u8>, Fault> {
    let joined: String = body.chars().filter(|ch| !ch.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(joined)
        .map_err(|_| Fault::tls())
}
