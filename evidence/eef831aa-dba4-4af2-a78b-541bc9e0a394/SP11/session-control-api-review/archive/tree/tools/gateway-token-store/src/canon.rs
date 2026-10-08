//! Canonical claims and audience. No network and no clock.

use crate::error::StoreError;

const FORBIDDEN_CLAIM: &[&str] = &[
    "allow",
    "auth",
    "authenticated",
    "authentication",
    "caller_authenticated",
    "groups",
    "idp",
    "mtls",
    "role",
    "roles",
    "self_claim",
];

pub fn ident(value: &str, max: usize) -> Result<(), StoreError> {
    if value.is_empty() || value.len() > max {
        return Err(StoreError::Input);
    }
    if value.bytes().all(is_ident_byte) {
        Ok(())
    } else {
        Err(StoreError::Input)
    }
}

pub fn one_role(role: &str) -> Result<(), StoreError> {
    if role.contains([',', ' ', '+']) {
        return Err(StoreError::RoleMerge);
    }
    ident(role, 128)?;
    if matches!(
        role,
        "public" | "admin" | "oo_token_admin" | "oo_token_runtime" | "oo_fixture_admin"
    ) {
        Err(StoreError::RoleMerge)
    } else {
        Ok(())
    }
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-')
}

pub fn canonicalize_claims(pairs: &[(String, String)]) -> Result<String, StoreError> {
    if pairs.len() > 32 {
        return Err(StoreError::Input);
    }
    let mut owned = pairs.to_vec();
    owned.sort_by(|left, right| left.0.cmp(&right.0));
    if owned.windows(2).any(|window| window[0].0 == window[1].0) {
        return Err(StoreError::Input);
    }
    let mut out = String::from("{");
    for (index, (key, value)) in owned.iter().enumerate() {
        if FORBIDDEN_CLAIM.contains(&key.as_str()) {
            return Err(StoreError::ForgedAuthClaim);
        }
        ident(key, 64)?;
        if index > 0 {
            out.push(',');
        }
        push_escaped(&mut out, key)?;
        out.push(':');
        push_escaped(&mut out, value)?;
    }
    out.push('}');
    if out.len() > 2048 {
        return Err(StoreError::Input);
    }
    Ok(out)
}

fn push_escaped(out: &mut String, value: &str) -> Result<(), StoreError> {
    out.push('"');
    for character in value.chars() {
        match character {
            '"' | '\\' => {
                out.push('\\');
                out.push(character);
            }
            other if other.is_control() => return Err(StoreError::Input),
            other => out.push(other),
        }
    }
    out.push('"');
    Ok(())
}

pub fn canonicalize_audience(raw: &str) -> Result<String, StoreError> {
    if raw.is_empty() || raw.len() > 256 || raw.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return Err(StoreError::Input);
    }
    let Some((scheme, rest)) = raw.split_once("://") else {
        return Err(StoreError::Input);
    };
    if !scheme.eq_ignore_ascii_case("https") || rest.contains(['@', '?', '#']) {
        return Err(StoreError::Input);
    }
    let (hostport, path) = match rest.split_once('/') {
        Some((host, tail)) => (host, format!("/{tail}")),
        None => (rest, "/".to_owned()),
    };
    if hostport.is_empty() || path.contains("//") {
        return Err(StoreError::Input);
    }
    let (host, port) = match hostport.rsplit_once(':') {
        Some((host, port)) => {
            if host.is_empty() || port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(StoreError::Input);
            }
            if port == "443" {
                (host, None)
            } else {
                (host, Some(port))
            }
        }
        None => (hostport, None),
    };
    if host.starts_with('[') || host.ends_with('.') {
        return Err(StoreError::Input);
    }
    let host = host.to_ascii_lowercase();
    if host.split('.').any(|label| label.is_empty()) {
        return Err(StoreError::Input);
    }
    if path.split('/').skip(1).any(|segment| segment == "." || segment == "..") {
        return Err(StoreError::Input);
    }
    let mut out = String::from("https://");
    out.push_str(&host);
    if let Some(port) = port {
        out.push(':');
        out.push_str(port);
    }
    out.push_str(&path);
    if out.len() > 256 {
        Err(StoreError::Input)
    } else {
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::{canonicalize_audience, canonicalize_claims, one_role};

    #[test]
    fn audience_and_claims_follow_the_contract() {
        let audience = canonicalize_audience("HTTPS://Gateway.Example:443/v1").expect("audience");
        assert_eq!(audience, "https://gateway.example/v1");
        assert_eq!(
            canonicalize_audience("https://gateway.example").expect("root"),
            "https://gateway.example/"
        );
        assert!(canonicalize_audience("http://gateway.example/").is_err());
        assert!(canonicalize_audience("https://user@gateway.example/").is_err());
        assert!(canonicalize_audience("https://gateway.example/v1?x=1").is_err());
        let claims = canonicalize_claims(&[
            ("region".to_owned(), "eu".to_owned()),
            ("desk".to_owned(), "a".to_owned()),
        ])
        .expect("claims");
        assert_eq!(claims, r#"{"desk":"a","region":"eu"}"#);
        assert!(canonicalize_claims(&[("mtls".to_owned(), "true".to_owned())]).is_err());
        assert!(canonicalize_claims(&[("roles".to_owned(), "admin".to_owned())]).is_err());
        assert!(one_role("analyst,admin").is_err());
        assert!(one_role("admin").is_err());
        assert!(one_role("oo_fixture_admin").is_err());
        assert!(one_role("analyst").is_ok());
    }
}
