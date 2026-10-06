//! Allowlist names are data. They become identifiers only after this grammar.

use crate::error::StoreError;

pub const ADMIN_ROLE: &str = "oo_enum_admin";
pub const VIEW_ROLE: &str = "oo_enum_view";

pub fn runtime_role_ok(role: &str) -> bool {
    let bytes = role.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && (bytes[0].is_ascii_lowercase() || bytes[0] == b'_')
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
        && role != ADMIN_ROLE
        && role != VIEW_ROLE
        && role != "public"
        && role != "postgres"
}

pub fn grant_usage_sql(role: &str) -> Result<String, StoreError> {
    if !runtime_role_ok(role) {
        return Err(StoreError::Allowlist);
    }
    Ok(format!("GRANT USAGE ON SCHEMA oo_dict TO {role}"))
}

pub fn grant_select_batch(role: &str) -> Result<String, StoreError> {
    if !runtime_role_ok(role) {
        return Err(StoreError::Allowlist);
    }
    Ok(format!(
        "SET LOCAL ROLE oo_enum_view; GRANT SELECT ON TABLE oo_dict.enums TO {role}; RESET ROLE"
    ))
}

pub fn token_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'.' | b'_' | b':' | b'-'))
}

pub fn hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::{grant_select_batch, grant_usage_sql, hex64, runtime_role_ok, token_ok};

    #[test]
    fn role_grammar_rejects_injection_and_fixed_roles() {
        assert!(runtime_role_ok("oo_r_enum"));
        assert!(!runtime_role_ok("oo_r_enum; DROP ROLE postgres"));
        assert!(!runtime_role_ok("oo_enum_admin"));
        assert!(!runtime_role_ok("oo_enum_view"));
        assert!(!runtime_role_ok("public"));
        assert!(!runtime_role_ok("Public"));
        assert!(!runtime_role_ok(""));
        assert!(grant_usage_sql("oo_r_enum").unwrap().ends_with("TO oo_r_enum"));
        assert!(grant_select_batch("oo_r_bad;drop").is_err());
        assert!(token_ok("project-one"));
        assert!(token_ok("hist-2020"));
        assert!(hex64("ab".repeat(32).as_str()));
        assert!(!hex64("AB".repeat(32).as_str()));
    }
}
