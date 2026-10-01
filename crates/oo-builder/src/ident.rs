//! Physical identifier mapping `pgident/v1`.
//!
//! Safe lowercase identifiers are kept. Reserved words, quoted text, and names
//! that already start with `q_` become `q_` plus lowercase hex of the NFC
//! UTF-8 bytes. A stable id is never an array index. Two different originals
//! that share one mapping are a collision and the catalog is refused.

use crate::error::DraftError;
use crate::limits::MAX_NAME_BYTES;
use crate::nfc::nfc;

/// Reserved keywords embedded for PostgreSQL 17. The live connector also
/// compares `pg_get_keywords()`. This adapter does not open a session, so an
/// unknown server keyword cannot be checked here.
const PG17_RESERVED: &[&str] = &[
    "all",
    "analyse",
    "analyze",
    "and",
    "any",
    "array",
    "as",
    "asc",
    "asymmetric",
    "both",
    "case",
    "cast",
    "check",
    "collate",
    "column",
    "constraint",
    "create",
    "current_catalog",
    "current_date",
    "current_role",
    "current_schema",
    "current_time",
    "current_timestamp",
    "current_user",
    "default",
    "deferrable",
    "desc",
    "distinct",
    "do",
    "else",
    "end",
    "except",
    "false",
    "fetch",
    "for",
    "foreign",
    "from",
    "grant",
    "group",
    "having",
    "in",
    "initially",
    "intersect",
    "into",
    "lateral",
    "leading",
    "limit",
    "localtime",
    "localtimestamp",
    "not",
    "null",
    "offset",
    "on",
    "only",
    "or",
    "order",
    "placing",
    "primary",
    "references",
    "returning",
    "select",
    "session_user",
    "some",
    "symmetric",
    "system_user",
    "table",
    "then",
    "to",
    "trailing",
    "true",
    "union",
    "unique",
    "user",
    "using",
    "variadic",
    "when",
    "where",
    "window",
    "with",
];

pub fn stable_ident(original: &str, path: &str) -> Result<String, DraftError> {
    if original.is_empty() || original.as_bytes().contains(&0) {
        return Err(DraftError::new(
            "ident.empty",
            path,
            "identifier is empty or contains NUL",
        ));
    }
    if original.len() > MAX_NAME_BYTES {
        return Err(DraftError::new(
            "budget.name_bytes",
            path,
            "identifier exceeds the PostgreSQL byte ceiling",
        ));
    }
    if original
        .chars()
        .any(|ch| (ch as u32) < 0x20 || ch == '\u{7f}')
    {
        return Err(DraftError::new(
            "ident.control",
            path,
            "identifier contains an ASCII control character",
        ));
    }
    let normalized = nfc(original);
    if normalized.is_empty() || normalized.as_bytes().contains(&0) {
        return Err(DraftError::new(
            "ident.empty",
            path,
            "identifier is empty after NFC",
        ));
    }
    if is_safe(&normalized) {
        Ok(normalized)
    } else {
        Ok(format!("q_{}", hex_bytes(normalized.as_bytes())))
    }
}

/// Assign stable ids. The same original may repeat. Two originals that share
/// a mapping, including distinct byte strings with one NFC form, refuse the set.
pub fn assign_idents(originals: &[String], path: &str) -> Result<Vec<String>, DraftError> {
    let mut seen: Vec<(String, String)> = Vec::new();
    let mut out = Vec::with_capacity(originals.len());
    for original in originals {
        let stable = stable_ident(original, path)?;
        if let Some((previous, _)) = seen.iter().find(|(_, mapped)| mapped == &stable) {
            if previous != original {
                return Err(DraftError::new(
                    "ident.collision",
                    path,
                    "two originals share one stable identifier",
                ));
            }
        } else {
            seen.push((original.clone(), stable.clone()));
        }
        out.push(stable);
    }
    Ok(out)
}

fn is_safe(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_NAME_BYTES || name.starts_with("q_") {
        return false;
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_ascii_lowercase()) {
        return false;
    }
    if !chars.all(|ch| ch == '_' || ch.is_ascii_lowercase() || ch.is_ascii_digit()) {
        return false;
    }
    PG17_RESERVED.binary_search(&name).is_err()
}

pub fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_reserved_and_unicode_follow_pgident() {
        assert_eq!(stable_ident("customer", "/").unwrap(), "customer");
        assert_eq!(stable_ident("_id", "/").unwrap(), "_id");
        let select_id = stable_ident("select", "/").unwrap();
        assert_eq!(select_id, format!("q_{}", hex_bytes(b"select")));
        let quoted = stable_ident("客户", "/").unwrap();
        assert_eq!(quoted, format!("q_{}", hex_bytes("客户".as_bytes())));
        let composed = stable_ident("é", "/").unwrap();
        let decomposed = stable_ident("e\u{0301}", "/").unwrap();
        assert_eq!(composed, decomposed);
        let err = assign_idents(&["é".to_string(), "e\u{0301}".to_string()], "/").unwrap_err();
        assert!(err.code.ends_with("/ident.collision"));
        assert!(PG17_RESERVED.windows(2).all(|pair| pair[0] < pair[1]));
        assert_ne!(stable_ident("user", "/").unwrap(), "user");
    }
}
