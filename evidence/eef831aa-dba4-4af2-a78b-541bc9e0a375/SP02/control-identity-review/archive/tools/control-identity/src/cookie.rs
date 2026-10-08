//! `__Host-` cookies. The login cookie is the only place the PKCE verifier exists.

use crate::crypto::{b64, b64_decode, sha256_hex, token_shape};
use crate::error::Fault;

pub const LOGIN_COOKIE: &str = "__Host-oo-login";
pub const SESSION_COOKIE: &str = "__Host-oo-session";

pub struct LoginCookie {
    pub handle: [u8; 32],
    pub verifier: [u8; 32],
}

impl LoginCookie {
    pub fn verifier_string(&self) -> String {
        b64(&self.verifier)
    }

    pub fn handle_hash(&self) -> String {
        sha256_hex(&self.handle)
    }

    pub fn encode(&self) -> String {
        let mut raw = [0_u8; 64];
        raw[..32].copy_from_slice(&self.handle);
        raw[32..].copy_from_slice(&self.verifier);
        b64(&raw)
    }

    pub fn decode(value: &str) -> Result<Self, Fault> {
        if value.len() != 86 {
            return Err(Fault::unauthenticated());
        }
        let raw = b64_decode(value).map_err(|_| Fault::unauthenticated())?;
        if raw.len() != 64 {
            return Err(Fault::unauthenticated());
        }
        let mut handle = [0_u8; 32];
        let mut verifier = [0_u8; 32];
        handle.copy_from_slice(&raw[..32]);
        verifier.copy_from_slice(&raw[32..]);
        Ok(Self { handle, verifier })
    }
}

pub fn session_hash(cookie: &str) -> Result<String, Fault> {
    if !token_shape(cookie) {
        return Err(Fault::unauthenticated());
    }
    let raw = b64_decode(cookie).map_err(|_| Fault::unauthenticated())?;
    if raw.len() != 32 {
        return Err(Fault::unauthenticated());
    }
    Ok(sha256_hex(&raw))
}

pub fn set_cookie(name: &str, value: &str) -> String {
    format!("{name}={value}; Secure; HttpOnly; SameSite=Lax; Path=/")
}

pub fn clear_cookie(name: &str) -> String {
    format!("{name}=; Secure; HttpOnly; SameSite=Lax; Path=/; Max-Age=0")
}

pub fn cookie_value<'a>(header: &'a str, name: &str) -> Result<Option<&'a str>, Fault> {
    let mut found = None;
    for part in header.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let Some((key, value)) = part.split_once('=') else {
            return Err(Fault::malformed());
        };
        if key.trim() != name {
            continue;
        }
        if found.is_some() {
            return Err(Fault::unauthenticated());
        }
        found = Some(value.trim());
    }
    Ok(found)
}

pub fn return_path(value: Option<&str>) -> Result<String, Fault> {
    let Some(value) = value else {
        return Ok("/".to_string());
    };
    if value.is_empty() || value.len() > 128 || !value.starts_with('/') {
        return Err(Fault::redirect());
    }
    if value.contains("//") || value.contains('\\') || value.contains('?') || value.contains('#') || value.contains("..") {
        return Err(Fault::redirect());
    }
    if !value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.')) {
        return Err(Fault::redirect());
    }
    Ok(value.to_string())
}

pub fn percent_decode(value: &str) -> Result<String, Fault> {
    if value.contains('+') {
        return Err(Fault::malformed());
    }
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(Fault::malformed());
            }
            let high = hex_value(bytes[index + 1]).ok_or_else(Fault::malformed)?;
            let low = hex_value(bytes[index + 2]).ok_or_else(Fault::malformed)?;
            out.push((high << 4) | low);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(out).map_err(|_| Fault::malformed())
}

pub fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(char::from(byte)),
            _ => {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                out.push('%');
                out.push(char::from(HEX[usize::from(byte >> 4)]));
                out.push(char::from(HEX[usize::from(byte & 0x0f)]));
            }
        }
    }
    out
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{clear_cookie, return_path, set_cookie, LoginCookie, LOGIN_COOKIE, SESSION_COOKIE};
    use crate::crypto::random_32;
    use crate::error::Fault;

    #[test]
    fn cookie_attributes_are_host_only() {
        let value = set_cookie(SESSION_COOKIE, "abc");
        assert_eq!(value, "__Host-oo-session=abc; Secure; HttpOnly; SameSite=Lax; Path=/");
        assert!(!value.to_ascii_lowercase().contains("domain="));
        let cleared = clear_cookie(LOGIN_COOKIE);
        assert!(cleared.starts_with("__Host-oo-login=; Secure; HttpOnly; SameSite=Lax; Path=/; Max-Age=0"));
    }

    #[test]
    fn return_paths_reject_off_site_targets() {
        assert_eq!(return_path(None).expect("default"), "/");
        assert_eq!(return_path(Some("/app/board")).expect("path"), "/app/board");
        for rejected in ["//evil", "/a/../b", "/a?b", "/a#b", "https://evil", "/a\\b", ""] {
            assert_eq!(return_path(Some(rejected)), Err(Fault::redirect()));
        }
    }

    #[test]
    fn login_cookie_round_trip_keeps_the_verifier() {
        let cookie = LoginCookie { handle: random_32().expect("handle"), verifier: random_32().expect("verifier") };
        let encoded = cookie.encode();
        assert_eq!(encoded.len(), 86);
        let decoded = LoginCookie::decode(&encoded).expect("decode");
        assert_eq!(decoded.handle, cookie.handle);
        assert_eq!(decoded.verifier, cookie.verifier);
    }
}
