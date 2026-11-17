//! Parse routing, local and sidecar parsers (10 §4.4). Board WS-BE2.
//!
//! This card (B-S2-06) covers the local `md` and `txt` parsers and the frame
//! protocol of the `oo-docs-parse` subprocess. Parsers produce an
//! [`crate::normalize::IrFragment`]; [`crate::normalize::finish`] turns it
//! into canonical `oo.doc-ir/v1` plus its `ir_b3`.
//!
//! Anchors follow 10 §3.5: lines are 1-based closed ranges counted over the
//! whole decoded file (frontmatter included, a BOM is not part of the text);
//! `char_start`/`char_end` are Unicode scalar offsets into the decoded text,
//! half-open `[char_start, char_end)`, newlines included. A line ends at
//! `\n`; a `\r` before it stays in the line.

pub mod decode;
pub mod frontmatter;
pub mod lines;
pub mod local;
pub mod markdown;
pub mod text;

#[cfg(test)]
mod tests;

use oo_core::docir::{ParserId, ParserInfo};
use sha2::{Digest, Sha256};

use crate::normalize::NormalizeError;

/// Version written into `parser.version` for both local text parsers.
pub const LOCAL_PARSER_VERSION: &str = "1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// Not UTF-8 (optionally with BOM) or UTF-16 with BOM. GB18030 and other
    /// legacy encodings are not decoded yet (no `encoding_rs` dependency).
    Encoding(&'static str),
    /// Decoded text contains NUL: binary content, not text.
    NotText,
    /// The assembled IR violates `oo.doc-ir/v1` (a parser defect).
    IrInvalid,
    /// More than `oo_core::docir::MAX_BLOCKS` blocks.
    LimitBlocks,
}

impl ParseError {
    /// `DOCS_PARSE_*` code (10 §8.4).
    pub fn code(&self) -> &'static str {
        match self {
            ParseError::Encoding(_) => "DOCS_PARSE_ENCODING",
            ParseError::NotText => "DOCS_PARSE_UNSUPPORTED_FORMAT",
            ParseError::IrInvalid => "DOCS_PARSE_IR_INVALID",
            ParseError::LimitBlocks => "DOCS_PARSE_LIMIT_BLOCKS",
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(self, ParseError::IrInvalid)
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Encoding(why) => f.write_str(why),
            ParseError::NotText => f.write_str("content is not text"),
            ParseError::IrInvalid => f.write_str("parser output violates oo.doc-ir/v1"),
            ParseError::LimitBlocks => f.write_str("block limit exceeded"),
        }
    }
}

impl std::error::Error for ParseError {}

impl From<NormalizeError> for ParseError {
    fn from(e: NormalizeError) -> Self {
        match e {
            NormalizeError::TooManyBlocks => ParseError::LimitBlocks,
            NormalizeError::IrInvalid(_) => ParseError::IrInvalid,
        }
    }
}

/// `parser` header: id, version and the hash of the profile text, so any
/// change of parsing options changes `profile_sha256` and invalidates caches.
pub(crate) fn parser_info(id: ParserId, profile: &str) -> ParserInfo {
    let digest = Sha256::digest(profile.as_bytes());
    let mut hex = String::with_capacity(64);
    for b in digest {
        hex.push_str(&format!("{b:02x}"));
    }
    ParserInfo {
        id,
        version: LOCAL_PARSER_VERSION.to_owned(),
        adapter_version: None,
        profile_sha256: hex,
    }
}

pub(crate) fn blob_b3(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// Truncate to at most `max` characters.
pub(crate) fn clip_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}
