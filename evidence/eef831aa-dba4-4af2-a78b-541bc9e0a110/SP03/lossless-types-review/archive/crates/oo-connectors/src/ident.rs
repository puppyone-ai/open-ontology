//! Stable identifier mapping `pgident/v1`.
//!
//! Safe lowercase identifiers are kept. Everything else, including reserved
//! words and names that already start with `q_`, becomes `q_` plus lowercase
//! hex of the NFC UTF-8 bytes. Two different originals that share a mapping
//! are a collision and the catalog is refused.

use std::collections::BTreeSet;

use unicode_normalization::UnicodeNormalization;

use crate::error::SourceError;

/// Reserved keywords embedded for PostgreSQL 17. The live session also reads
/// `pg_get_keywords()` and refuses the catalog if the server has a reserved
/// word that is not in this set.
pub const PG17_RESERVED: &[&str] = &[
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

pub(crate) fn reserved_set() -> BTreeSet<String> {
    PG17_RESERVED.iter().map(|w| (*w).to_string()).collect()
}

/// Map one original catalog name. `reserved` holds lowercase words.
pub fn stable_ident(original: &str, reserved: &BTreeSet<String>) -> Result<String, SourceError> {
    if original.is_empty() || original.contains('\0') {
        return Err(SourceError::identifier("empty_name"));
    }
    let nfc: String = original.nfc().collect();
    if nfc.is_empty() || nfc.contains('\0') {
        return Err(SourceError::identifier("empty_name"));
    }
    if is_safe(&nfc, reserved) {
        Ok(nfc)
    } else {
        Ok(format!("q_{}", hex_bytes(nfc.as_bytes())))
    }
}

fn is_safe(name: &str, reserved: &BTreeSet<String>) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 || name.starts_with("q_") {
        return false;
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_ascii_lowercase()) {
        return false;
    }
    if !chars.all(|c| c == '_' || c.is_ascii_lowercase() || c.is_ascii_digit()) {
        return false;
    }
    !reserved.contains(name)
}

pub(crate) fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Assign stable idents. A repeated original is one name. Two originals that
/// share a stable ident, including distinct byte strings with one NFC form,
/// refuse the whole set.
pub fn assign_idents(
    originals: &[String],
    reserved: &BTreeSet<String>,
) -> Result<Vec<String>, SourceError> {
    let mut seen: Vec<(String, String)> = Vec::new();
    let mut out = Vec::with_capacity(originals.len());
    for original in originals {
        let stable = stable_ident(original, reserved)?;
        if let Some((prev, _)) = seen.iter().find(|(_, mapped)| mapped == &stable) {
            if prev != original {
                return Err(SourceError::identifier("identifier_collision"));
            }
        } else {
            seen.push((original.clone(), stable.clone()));
        }
        out.push(stable);
    }
    Ok(out)
}

/// Server reserved words are uppercase. Extra words in the embedded set are
/// conservative. A server word we do not know fails closed.
pub(crate) fn reserved_covers_server(server_words: &[String]) -> bool {
    let embedded = reserved_set();
    server_words.iter().all(|word| {
        let lower = word.to_ascii_lowercase();
        embedded.contains(&lower)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reserved() -> BTreeSet<String> {
        reserved_set()
    }

    #[test]
    fn safe_names_stay_verbatim() {
        let reserved = reserved();
        assert_eq!(stable_ident("customer", &reserved).unwrap(), "customer");
        assert_eq!(stable_ident("_id", &reserved).unwrap(), "_id");
        assert_eq!(stable_ident("a1", &reserved).unwrap(), "a1");
    }

    #[test]
    fn reserved_uppercase_space_and_q_prefix_are_encoded() {
        let reserved = reserved();
        let select_id = stable_ident("select", &reserved).unwrap();
        assert_eq!(select_id, format!("q_{}", hex_bytes(b"select")));
        assert_ne!(select_id, "select");
        let weird = stable_ident("Weird Name", &reserved).unwrap();
        assert!(weird.starts_with("q_"));
        let prefixed = stable_ident("q_ab", &reserved).unwrap();
        assert_ne!(prefixed, "q_ab");
        assert!(prefixed.starts_with("q_"));
    }

    #[test]
    fn unicode_uses_nfc_hex_and_collides_on_equivalent_forms() {
        let reserved = reserved();
        let composed = "客户";
        let id = stable_ident(composed, &reserved).unwrap();
        assert_eq!(id, format!("q_{}", hex_bytes(composed.as_bytes())));
        let nfc = "é";
        let nfd = "e\u{0301}";
        let err = assign_idents(&[nfc.to_string(), nfd.to_string()], &reserved).unwrap_err();
        assert_eq!(err.fact(), Some("identifier_collision"));
        let same = assign_idents(&[nfc.to_string(), nfc.to_string()], &reserved).unwrap();
        assert_eq!(same[0], same[1]);
    }
}
