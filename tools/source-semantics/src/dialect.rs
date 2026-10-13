//! Source-dialect tokens that must not enter an Agent-executable example.
//!
//! The role-pack verifier accepts a small PostgreSQL SELECT subset. This scan
//! runs first so NVL, TOP, ROWNUM, and the other source forms never become
//! `examples/verified.sql`. A hit drops that example. It is not a translation.

pub fn source_dialect_hit(text: &str) -> Option<&'static str> {
    let folded = text.to_ascii_uppercase();
    if folded.contains("NVL2(") || folded.contains("NVL(") {
        return Some("NVL");
    }
    if folded.contains("ISNULL(") {
        return Some("ISNULL");
    }
    if contains_ident(&folded, "ROWNUM") {
        return Some("ROWNUM");
    }
    if folded.contains("CONNECT BY")
        || folded.contains("CONNECT\nBY")
        || folded.contains("CONNECT\tBY")
    {
        return Some("CONNECT BY");
    }
    if folded.contains("(+)") {
        return Some("oracle_outer_join");
    }
    if folded.contains("DATEADD(") {
        return Some("DATEADD");
    }
    if folded.contains("DATEDIFF(") {
        return Some("DATEDIFF");
    }
    if folded.contains("GETDATE(") {
        return Some("GETDATE");
    }
    if contains_ident(&folded, "SYSDATE") {
        return Some("SYSDATE");
    }
    if folded.contains("DECODE(") {
        return Some("DECODE");
    }
    if top_clause(&folded) {
        return Some("TOP");
    }
    None
}

fn contains_ident(folded: &str, word: &str) -> bool {
    let bytes = folded.as_bytes();
    let needle = word.as_bytes();
    let mut index = 0;
    while index + needle.len() <= bytes.len() {
        if &bytes[index..index + needle.len()] == needle {
            let before_ok = index == 0 || !is_ident_byte(bytes[index - 1]);
            let after = index + needle.len();
            let after_ok = after == bytes.len() || !is_ident_byte(bytes[after]);
            if before_ok && after_ok {
                return true;
            }
        }
        index += 1;
    }
    false
}

fn top_clause(folded: &str) -> bool {
    let bytes = folded.as_bytes();
    let mut index = 0;
    while index + 3 <= bytes.len() {
        if &bytes[index..index + 3] == b"TOP" {
            let before_ok = index == 0 || !is_ident_byte(bytes[index - 1]);
            let after = index + 3;
            if before_ok && after < bytes.len() {
                let mut cursor = after;
                if bytes[cursor] == b'(' {
                    return true;
                }
                if bytes[cursor].is_ascii_whitespace() {
                    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                        cursor += 1;
                    }
                    if cursor < bytes.len() && (bytes[cursor].is_ascii_digit() || bytes[cursor] == b'(')
                    {
                        return true;
                    }
                }
            }
        }
        index += 1;
    }
    false
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

#[cfg(test)]
mod tests {
    use super::source_dialect_hit;

    #[test]
    fn unit_dialect_scanner_is_not_capability_acceptance() {
        assert_eq!(source_dialect_hit("select NVL(name, id) from public.customer"), Some("NVL"));
        assert_eq!(source_dialect_hit("select top 10 id from public.customer"), Some("TOP"));
        assert_eq!(source_dialect_hit("select id from public.customer where rownum < 2"), Some("ROWNUM"));
        assert_eq!(source_dialect_hit("select id from public.customer where name is null"), None);
        assert_eq!(source_dialect_hit("select coalesce(name, id) from public.customer"), None);
    }
}
