use std::path::Path;

use crate::fault::Fault;
use crate::limits::MAX_FONT_BYTES;
use crate::path;
use crate::sha256::{is_lower_hex, sha256_hex};

const MANIFEST: &str = include_str!("../native/font.json");

pub struct FontBytes {
    pub bytes: Vec<u8>,
    pub sha256: String,
}

pub fn load(path: &Path) -> Result<FontBytes, Fault> {
    let expected = field(MANIFEST, "otf_sha256").ok_or(Fault::engine())?;
    if !is_lower_hex(&expected, Some(64)) {
        return Err(Fault::engine());
    }
    let bytes = path::read_capped(path, MAX_FONT_BYTES)?;
    let sha256 = sha256_hex(&bytes);
    if sha256 != expected {
        return Err(Fault::engine());
    }
    Ok(FontBytes { bytes, sha256 })
}

fn field(doc: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{key}\"");
    let index = doc.find(&pattern)?;
    let rest = doc[index + pattern.len()..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}
