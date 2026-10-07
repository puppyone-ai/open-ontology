use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use base64::Engine;

use crate::fault::Fault;
use crate::files::read_regular;
use crate::grammar::{visible_ascii, BEARER_MAX, DIALOGUE_BEARER_MAX};

pub const CLIENT_TOKEN_MIN: usize = 32;
const RANDOM_BYTES: usize = 32;

#[derive(Clone)]
pub struct Boundary {
    pub client_authorization: String,
    pub upstream_authorization: String,
}

pub fn load_boundary(client_file: &Path, upstream_file: &Path) -> Result<Boundary, Fault> {
    let client = read_token(client_file, CLIENT_TOKEN_MIN, DIALOGUE_BEARER_MAX)?;
    let upstream = read_token(upstream_file, 1, BEARER_MAX)?;
    if ct_eq(client.as_bytes(), upstream.as_bytes()) {
        return Err(Fault::config());
    }
    let client_authorization = format!("Bearer {client}");
    let upstream_authorization = format!("Bearer {upstream}");
    if client_authorization.len() > 600 || upstream_authorization.len() > 600 {
        return Err(Fault::config());
    }
    Ok(Boundary {
        client_authorization,
        upstream_authorization,
    })
}

pub fn authorization_matches(presented: &str, expected: &str) -> bool {
    ct_eq(presented.as_bytes(), expected.as_bytes())
}

pub fn body_has_token(body: &[u8], authorization: &str) -> bool {
    let Some(token) = authorization.strip_prefix("Bearer ") else {
        return false;
    };
    !token.is_empty() && body.windows(token.len()).any(|window| window == token.as_bytes())
}

pub fn mint_client_token(path: &Path) -> Result<(), Fault> {
    if path.exists() {
        return Err(Fault::config());
    }
    let mut raw = [0_u8; RANDOM_BYTES];
    let mut random = File::open("/dev/urandom").map_err(|_| Fault::io())?;
    random.read_exact(&mut raw).map_err(|_| Fault::io())?;
    let text = base64::engine::general_purpose::STANDARD.encode(raw);
    if text.len() < CLIENT_TOKEN_MIN || !visible_ascii(text.as_bytes()) {
        return Err(Fault::config());
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        fs::create_dir_all(parent).map_err(|_| Fault::io())?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| Fault::config())?;
    file.write_all(text.as_bytes()).map_err(|_| Fault::io())?;
    Ok(())
}

fn read_token(path: &Path, min: usize, max: usize) -> Result<String, Fault> {
    let mut bytes = read_regular(path, max.saturating_add(1))?;
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    if bytes.len() < min || bytes.len() > max || !visible_ascii(&bytes) {
        return Err(Fault::config());
    }
    String::from_utf8(bytes).map_err(|_| Fault::config())
}

fn ct_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0_u8;
    for (left_byte, right_byte) in left.iter().zip(right.iter()) {
        diff |= left_byte ^ right_byte;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::mint_client_token;
    use super::CLIENT_TOKEN_MIN;
    use crate::token::load_boundary;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn minted_token_is_private_and_long_enough() {
        let path = std::env::temp_dir().join(format!("oo-a334-mint-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        mint_client_token(&path).expect("mint");
        let bytes = std::fs::read(&path).expect("read");
        assert!(bytes.len() >= CLIENT_TOKEN_MIN);
        assert!(!bytes.contains(&b'\n'));
        let mode = std::fs::metadata(&path).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert!(mint_client_token(&path).is_err());
        let upstream = path.with_extension("upstream");
        std::fs::write(&upstream, b"local-upstream-token\n").expect("upstream");
        let boundary = load_boundary(&path, &upstream).expect("boundary");
        assert!(boundary.client_authorization.starts_with("Bearer "));
        assert!(boundary.upstream_authorization.starts_with("Bearer "));
        assert_ne!(boundary.client_authorization, boundary.upstream_authorization);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&upstream);
    }
}
