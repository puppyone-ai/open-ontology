//! The one closed grammar for a per-table stale reason.
//!
//! ```text
//! reason  = code / "schema_drift:" column ":" kind
//! code    = "schema_unproven" / "schema_drift" / "rebaseline_required"
//!         / "source_changed" / "source_permission" / "source_timeout"
//!         / "table_budget"
//! column  = "unpublished_column" / "c." enc / "h." 16hexlower
//! enc     = 1*( ALPHA / DIGIT / "_" / "%" 2hexlower )   ; UTF-8 bytes
//! kind    = "dropped" / "renamed" / "nullability" / "type" / "key"
//!         / "collation" / "ordinal"
//! ```
//!
//! A reason is at most [`MAX_LEN`] bytes, the storage bound of every ledger
//! that keeps it. `c.` names a published column; any other byte than ASCII
//! alphanumerics and `_` is percent-encoded, so case, `:` and non-ASCII
//! survive unambiguously. A name whose encoding does not fit is referenced by
//! `h.` and the first 16 hex digits of the SHA-256 of its UTF-8 bytes. A column
//! outside the published projection is never named.
//!
//! This file has no dependencies. The release coordinator and the Gateway
//! compile the same file with `#[path]`, so producer and validator cannot drift.

pub const MAX_LEN: usize = 192;
pub const CODES: [&str; 7] = [
    "schema_unproven",
    "schema_drift",
    "rebaseline_required",
    "source_changed",
    "source_permission",
    "source_timeout",
    "table_budget",
];
pub const DRIFT_KINDS: [&str; 7] = [
    "dropped",
    "renamed",
    "nullability",
    "type",
    "key",
    "collation",
    "ordinal",
];
pub const UNPUBLISHED: &str = "unpublished_column";
const PREFIX: &str = "schema_drift:";
/// Longest column token that still fits the longest kind.
const MAX_COLUMN: usize = MAX_LEN - PREFIX.len() - 1 - "nullability".len();

/// The column a drift reason refers to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DriftColumn {
    Unpublished,
    Published(String),
    /// First 16 hex digits of SHA-256 over the published name.
    Hashed(String),
}

fn hex(b: u8) -> [u8; 2] {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    [DIGITS[usize::from(b >> 4)], DIGITS[usize::from(b & 15)]]
}

/// Encode a published column name, or `None` when the caller must fall back
/// to [`hashed`] (empty, or too long once encoded).
pub fn encode_column(name: &str) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    let mut out = String::from("c.");
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || b == b'_' {
            out.push(char::from(b));
        } else {
            out.push('%');
            out.extend(hex(b).map(char::from));
        }
        if out.len() > MAX_COLUMN {
            return None;
        }
    }
    Some(out)
}

/// The `h.` token for a name; `sha256_hex` is the lowercase SHA-256 of it.
pub fn hashed(sha256_hex: &str) -> Option<String> {
    let token = format!("h.{}", sha256_hex.get(..16)?);
    column(&token).is_some().then_some(token)
}

/// Build a drift reason from a column token and kind.
pub fn drift(column_token: &str, kind: &str) -> Option<String> {
    let reason = format!("{PREFIX}{column_token}:{kind}");
    valid(&reason).then_some(reason)
}

fn column(token: &str) -> Option<DriftColumn> {
    if token == UNPUBLISHED {
        return Some(DriftColumn::Unpublished);
    }
    if token.len() > MAX_COLUMN {
        return None;
    }
    if let Some(digest) = token.strip_prefix("h.") {
        return (digest.len() == 16
            && digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        .then(|| DriftColumn::Hashed(digest.into()));
    }
    let enc = token.strip_prefix("c.")?.as_bytes();
    let mut bytes = Vec::new();
    let mut i = 0;
    while i < enc.len() {
        match enc[i] {
            b if b.is_ascii_alphanumeric() || b == b'_' => {
                bytes.push(b);
                i += 1;
            }
            b'%' => {
                let pair = enc.get(i + 1..i + 3)?;
                let nibble = |c: u8| match c {
                    b'0'..=b'9' => Some(c - b'0'),
                    b'a'..=b'f' => Some(c - b'a' + 10),
                    _ => None,
                };
                let b = nibble(pair[0])? << 4 | nibble(pair[1])?;
                // Canonical: never escape a byte that has a literal form.
                if b.is_ascii_alphanumeric() || b == b'_' {
                    return None;
                }
                bytes.push(b);
                i += 3;
            }
            _ => return None,
        }
    }
    let name = String::from_utf8(bytes).ok()?;
    (!name.is_empty()).then_some(DriftColumn::Published(name))
}

/// Parse a drift reason into its column and kind.
pub fn parse_drift(reason: &str) -> Option<(DriftColumn, &str)> {
    if reason.len() > MAX_LEN {
        return None;
    }
    let (token, kind) = reason.strip_prefix(PREFIX)?.rsplit_once(':')?;
    if !DRIFT_KINDS.contains(&kind) {
        return None;
    }
    Some((column(token)?, kind))
}

/// Whether `reason` is exactly one sentence of the grammar.
pub fn valid(reason: &str) -> bool {
    CODES.contains(&reason) || parse_drift(reason).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_case_non_ascii_and_63_byte_names_round_trip() {
        let long = "a".repeat(63);
        let upper = "P".repeat(63);
        let wide = "é".repeat(31) + "x"; // 63 bytes
        for name in [
            "PAYLOAD",
            "userId",
            "a:b",
            "名前",
            "with space",
            "%",
            long.as_str(),
            upper.as_str(),
        ] {
            let token = encode_column(name).unwrap();
            for kind in DRIFT_KINDS {
                let reason = drift(&token, kind).unwrap();
                assert!(reason.len() <= MAX_LEN && valid(&reason), "{reason}");
                assert_eq!(
                    parse_drift(&reason),
                    Some((DriftColumn::Published(name.into()), kind))
                );
            }
        }
        // 63 non-ASCII bytes encode to 189 bytes: the hash reference is used.
        assert_eq!(encode_column(&wide), None);
        let token = hashed(&"0123456789abcdef".repeat(4)).unwrap();
        assert!(valid(&drift(&token, "nullability").unwrap()));
        assert!(drift(UNPUBLISHED, "key").is_some_and(|r| valid(&r)));
    }

    #[test]
    fn everything_outside_the_grammar_is_refused() {
        for reason in [
            "",
            "schema_drift:",
            "schema_drift::type",
            "schema_drift:payload:type",
            "schema_drift:c.payload:unknown",
            "schema_drift:c.pay:load:type",
            "schema_drift:c.payload\n:type",
            "schema_drift:c.%61:type",
            "schema_drift:c.%4:type",
            "schema_drift:c.%C3%A9:type",
            "schema_drift:c.%ff:type",
            "schema_drift:h.0123:type",
            "schema_drift:h.0123456789ABCDEF:type",
            "schema_drift:other_column:key",
            "Schema_unproven",
            "source_timeout ",
            "c5_rebaseline_required_lineage",
        ] {
            assert!(!valid(reason), "{reason:?}");
        }
        for code in CODES {
            assert!(valid(code));
        }
    }
}
