//! Persist a `pgident/v1` logical name as a PostgreSQL identifier.
//!
//! Callers pass an identifier that `pgident/v1` already produced. This module
//! does not compose, case-fold, or truncate that string. The database stores
//! the binding. [`LOOKUP_SQL`] reads the stored name.

use oo_core::canonical::hash_bytes;

/// Algorithm stored with every row.
pub const ALGORITHM: &str = "physical-mapping/v1";

/// Largest logical name this binding accepts, in bytes.
pub const MAX_LOGICAL_BYTES: usize = 4096;

/// Stored PostgreSQL identifier length. The catalog limit is 63 bytes.
pub const PHYSICAL_OCTETS: usize = 63;

/// Identical registration.
pub const OUTCOME_INSERTED: &str = "inserted";

/// Same identity and the same stored binding.
pub const OUTCOME_REPLAY: &str = "replay";

/// Extended-query text. Bind the eight parameters. Do not interpolate values.
pub const REGISTER_SQL: &str =
    "SELECT _oo.register_physical_mapping($1, $2, $3, $4, $5, $6, $7, $8)";

/// Extended-query text. Bind the seven parameters. Do not interpolate values.
pub const LOOKUP_SQL: &str = "SELECT _oo.lookup_physical_mapping($1, $2, $3, $4, $5, $6, $7)";

/// Where the logical name sits in the catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// Schema name.
    Namespace,
    /// Table name inside a namespace.
    Table,
    /// Column name inside a table.
    Column,
}

impl Level {
    /// Word stored in the register and lookup calls.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Namespace => "namespace",
            Self::Table => "table",
            Self::Column => "column",
        }
    }

    fn byte(self) -> u8 {
        match self {
            Self::Namespace => b'n',
            Self::Table => b't',
            Self::Column => b'c',
        }
    }

    fn prefix(self) -> &'static str {
        match self {
            Self::Namespace => "pn_",
            Self::Table => "pt_",
            Self::Column => "pc_",
        }
    }
}

/// Scope and logical identity to bind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MappingRequest {
    /// Closed source token. First byte is `a-z`.
    pub source: String,
    /// Lowercase hex SHA-256 of the caller config.
    pub config_digest: String,
    /// Run generation, starting at 1.
    pub run_generation: i64,
    /// Catalog level.
    pub level: Level,
    /// `pgident/v1` namespace.
    pub logical_namespace: String,
    /// `pgident/v1` table, or empty at namespace level.
    pub logical_table: String,
    /// `pgident/v1` column, or empty above column level.
    pub logical_column: String,
}

/// Deterministic binding. The database stores these bytes and names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhysicalPlan {
    /// [`ALGORITHM`].
    pub algorithm: &'static str,
    /// Source token.
    pub source: String,
    /// Config digest.
    pub config_digest: String,
    /// Run generation.
    pub run_generation: i64,
    /// Catalog level.
    pub level: Level,
    /// Logical namespace.
    pub logical_namespace: String,
    /// Logical table.
    pub logical_table: String,
    /// Logical column.
    pub logical_column: String,
    /// SHA-256 of the canonical payload.
    pub identity_digest: [u8; 32],
    /// Sixty-three-byte PostgreSQL identifier.
    pub physical_name: String,
    /// Physical namespace in this scope.
    pub scope_namespace_physical: String,
    /// Physical table in this scope. Empty at namespace level.
    pub scope_table_physical: Option<String>,
}

/// Closed input or a name that is not a generated identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanError {
    /// Source token is closed.
    ClosedSource,
    /// Config digest is closed.
    ClosedConfig,
    /// Generation is closed.
    ClosedGeneration,
    /// Logical name is closed.
    ClosedLogicalName,
    /// The text is not a generated physical name.
    NotPhysicalName,
}

impl PlanError {
    fn text(self) -> &'static str {
        match self {
            Self::NotPhysicalName => "physical mapping name is closed",
            Self::ClosedSource
            | Self::ClosedConfig
            | Self::ClosedGeneration
            | Self::ClosedLogicalName => "physical mapping payload is closed",
        }
    }
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.text())
    }
}

impl std::error::Error for PlanError {}

/// Plan one binding. This does not open a database connection.
pub fn plan_physical_mapping(request: &MappingRequest) -> Result<PhysicalPlan, PlanError> {
    if !source_ok(&request.source) {
        return Err(PlanError::ClosedSource);
    }
    if !config_ok(&request.config_digest) {
        return Err(PlanError::ClosedConfig);
    }
    if request.run_generation < 1 {
        return Err(PlanError::ClosedGeneration);
    }
    if !logical_ok(&request.logical_namespace) {
        return Err(PlanError::ClosedLogicalName);
    }
    match request.level {
        Level::Namespace => {
            if !request.logical_table.is_empty() || !request.logical_column.is_empty() {
                return Err(PlanError::ClosedLogicalName);
            }
        }
        Level::Table => {
            if !logical_ok(&request.logical_table) || !request.logical_column.is_empty() {
                return Err(PlanError::ClosedLogicalName);
            }
        }
        Level::Column => {
            if !logical_ok(&request.logical_table) || !logical_ok(&request.logical_column) {
                return Err(PlanError::ClosedLogicalName);
            }
        }
    }
    let digest = identity_digest(request);
    let generated = physical_name(request.level, &digest);
    let scope_namespace_physical = match request.level {
        Level::Namespace => generated.clone(),
        Level::Table | Level::Column => physical_name(
            Level::Namespace,
            &identity_digest(&with_level(request, Level::Namespace)),
        ),
    };
    let scope_table_physical = match request.level {
        Level::Namespace => None,
        Level::Table => Some(generated.clone()),
        Level::Column => Some(physical_name(
            Level::Table,
            &identity_digest(&with_level(request, Level::Table)),
        )),
    };
    Ok(PhysicalPlan {
        algorithm: ALGORITHM,
        source: request.source.clone(),
        config_digest: request.config_digest.clone(),
        run_generation: request.run_generation,
        level: request.level,
        logical_namespace: request.logical_namespace.clone(),
        logical_table: request.logical_table.clone(),
        logical_column: request.logical_column.clone(),
        identity_digest: digest,
        physical_name: generated,
        scope_namespace_physical,
        scope_table_physical,
    })
}

/// Double-quote a generated identifier. Any other text is refused.
pub fn quote_physical_name(name: &str) -> Result<String, PlanError> {
    if !is_physical_name(name) {
        return Err(PlanError::NotPhysicalName);
    }
    let mut quoted = String::with_capacity(name.len() + 2);
    quoted.push('"');
    quoted.push_str(name);
    quoted.push('"');
    Ok(quoted)
}

fn with_level(request: &MappingRequest, level: Level) -> MappingRequest {
    let mut parent = request.clone();
    parent.level = level;
    if level == Level::Namespace {
        parent.logical_table.clear();
        parent.logical_column.clear();
    } else if level == Level::Table {
        parent.logical_column.clear();
    }
    parent
}

fn identity_digest(request: &MappingRequest) -> [u8; 32] {
    hash_bytes(&canonical_bytes(request))
}

fn canonical_bytes(request: &MappingRequest) -> Vec<u8> {
    let mut out = Vec::with_capacity(
        64 + request.logical_namespace.len()
            + request.logical_table.len()
            + request.logical_column.len(),
    );
    out.extend_from_slice(b"physical-mapping/v1\0");
    push_cstr(&mut out, request.source.as_bytes());
    push_cstr(&mut out, request.config_digest.as_bytes());
    push_cstr(&mut out, request.run_generation.to_string().as_bytes());
    out.push(request.level.byte());
    push_cstr(&mut out, request.logical_namespace.as_bytes());
    push_cstr(&mut out, request.logical_table.as_bytes());
    push_cstr(&mut out, request.logical_column.as_bytes());
    out
}

fn push_cstr(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(bytes);
    out.push(0);
}

fn physical_name(level: Level, digest: &[u8; 32]) -> String {
    let hex = hex_encode(digest);
    let name = format!("{}{}", level.prefix(), &hex[..60]);
    debug_assert_eq!(name.len(), PHYSICAL_OCTETS);
    name
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn source_ok(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 {
        return false;
    }
    let mut chars = bytes.iter();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    chars.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn config_ok(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() == 64 && bytes.iter().all(|byte| is_lower_hex(*byte))
}

fn logical_ok(name: &str) -> bool {
    if name.is_empty() || name.len() > MAX_LOGICAL_BYTES || name.as_bytes().contains(&0) {
        return false;
    }
    is_safe_pgident(name) || is_full_hex(name)
}

fn is_safe_pgident(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 || bytes.starts_with(b"q_") {
        return false;
    }
    let mut chars = bytes.iter();
    let Some(first) = chars.next() else {
        return false;
    };
    if *first != b'_' && !first.is_ascii_lowercase() {
        return false;
    }
    if !chars.all(|byte| *byte == b'_' || byte.is_ascii_lowercase() || byte.is_ascii_digit()) {
        return false;
    }
    oo_connectors::PG17_RESERVED.binary_search(&name).is_err()
}

fn is_full_hex(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.len() < 4 || bytes.len() > MAX_LOGICAL_BYTES || bytes.len() == 63 || !bytes.starts_with(b"q_")
    {
        return false;
    }
    let hex = &bytes[2..];
    hex.len().is_multiple_of(2) && hex.iter().all(|byte| is_lower_hex(*byte))
}

fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

fn is_physical_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.len() != PHYSICAL_OCTETS {
        return false;
    }
    let prefix = &bytes[..3];
    if prefix != b"pn_" && prefix != b"pt_" && prefix != b"pc_" {
        return false;
    }
    bytes[3..].iter().all(|byte| is_lower_hex(*byte))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    const MIGRATION: &str = include_str!("../migrations/0004_physical_mapping.sql");
    const GOLDEN_DIGEST: &str = "d528fe7ee77dde914147ad1edc2b2d6f2f42b25106ccfe6f39d5f8996f1fc398";
    const GOLDEN_NAME: &str = "pn_d528fe7ee77dde914147ad1edc2b2d6f2f42b25106ccfe6f39d5f8996f1f";

    fn reserved() -> BTreeSet<String> {
        oo_connectors::PG17_RESERVED.iter().map(|word| (*word).to_string()).collect()
    }

    fn golden() -> MappingRequest {
        MappingRequest {
            source: "src_main".to_string(),
            config_digest: "ab".repeat(32),
            run_generation: 1,
            level: Level::Namespace,
            logical_namespace: "customer".to_string(),
            logical_table: String::new(),
            logical_column: String::new(),
        }
    }

    fn q_utf8(original: &str) -> String {
        format!("q_{}", hex_encode(original.as_bytes()))
    }

    #[test]
    fn golden_namespace_digest_is_63_bytes() {
        let plan = plan_physical_mapping(&golden()).expect("plan");
        assert_eq!(hex_encode(&plan.identity_digest), GOLDEN_DIGEST);
        assert_eq!(plan.physical_name, GOLDEN_NAME);
        assert_eq!(plan.physical_name.len(), 63);
        assert_eq!(canonical_bytes(&golden()).len(), 108);
        assert_eq!(plan.scope_namespace_physical, plan.physical_name);
        assert!(plan.scope_table_physical.is_none());
        assert_eq!(quote_physical_name(&plan.physical_name).expect("quote"), format!("\"{GOLDEN_NAME}\""));
    }

    #[test]
    fn long_names_that_share_a_63_byte_prefix_stay_distinct() {
        let left = format!("q_{}", "ab".repeat(40));
        let right = format!("{}cd", &left[..left.len() - 2]);
        assert_eq!(left.len(), 82);
        assert_eq!(right.len(), 82);
        assert_eq!(&left.as_bytes()[..63], &right.as_bytes()[..63]);
        assert_ne!(left, right);
        let mut request = golden();
        request.logical_namespace = left;
        let planned_left = plan_physical_mapping(&request).expect("left");
        request.logical_namespace = right;
        let planned_right = plan_physical_mapping(&request).expect("right");
        assert_ne!(planned_left.physical_name, planned_right.physical_name);
        assert_eq!(planned_left.physical_name.len(), 63);
        assert_eq!(planned_right.physical_name.len(), 63);
    }

    #[test]
    fn pgident_forms_round_trip_and_raw_unicode_stays_closed() {
        let words = reserved();
        assert_eq!(oo_connectors::stable_ident("é", &words).expect("composed"), "q_c3a9");
        assert_eq!(oo_connectors::stable_ident("e\u{0301}", &words).expect("decomposed"), "q_c3a9");
        assert_eq!(oo_connectors::stable_ident("ﬁ", &words).expect("ligature"), "q_efac81");
        assert_eq!(oo_connectors::stable_ident("fi", &words).expect("ascii"), "fi");
        assert_eq!(oo_connectors::stable_ident("select", &words).expect("reserved"), "q_73656c656374");
        assert_eq!(oo_connectors::stable_ident("Customer", &words).expect("case"), "q_437573746f6d6572");
        assert_eq!(oo_connectors::stable_ident("customer", &words).expect("lower"), "customer");
        assert_eq!(oo_connectors::stable_ident("a\"b", &words).expect("quote"), "q_612262");
        assert_eq!(oo_connectors::stable_ident("客户", &words).expect("han"), "q_e5aea2e688b7");
        let repeated = "一".repeat(80);
        assert_eq!(oo_connectors::stable_ident(&repeated, &words).expect("long han"), q_utf8(&repeated));
        assert!(q_utf8(&repeated).len() > 63);

        let truncated = format!("q_{}", "a".repeat(61));
        assert_eq!(truncated.len(), 63);
        let mut request = golden();
        for closed in ["客户", "select", "Customer", "a\"b", "é", "q_", truncated.as_str()] {
            request.logical_namespace = closed.to_string();
            assert_eq!(
                plan_physical_mapping(&request).expect_err("closed"),
                PlanError::ClosedLogicalName
            );
        }
        request.logical_namespace = "q_c3a9".to_string();
        assert!(plan_physical_mapping(&request).is_ok());
        request.logical_namespace = "fi".to_string();
        let fi = plan_physical_mapping(&request).expect("fi");
        request.logical_namespace = "q_efac81".to_string();
        let ligature = plan_physical_mapping(&request).expect("ligature");
        assert_ne!(fi.physical_name, ligature.physical_name);
        request.logical_namespace = "customer".to_string();
        let lower = plan_physical_mapping(&request).expect("lower");
        request.logical_namespace = "q_437573746f6d6572".to_string();
        let cased = plan_physical_mapping(&request).expect("cased");
        assert_ne!(lower.physical_name, cased.physical_name);
    }

    #[test]
    fn length_bounds_and_scope_follow_the_parent_identity() {
        let mut request = golden();
        request.logical_namespace = format!("q_{}", "ab".repeat(2047));
        assert_eq!(request.logical_namespace.len(), 4096);
        assert!(plan_physical_mapping(&request).is_ok());
        request.logical_namespace = format!("q_{}", "ab".repeat(2048));
        assert_eq!(request.logical_namespace.len(), 4098);
        assert_eq!(plan_physical_mapping(&request).expect_err("long"), PlanError::ClosedLogicalName);

        let mut request = golden();
        request.level = Level::Table;
        request.logical_table = "orders".to_string();
        let table = plan_physical_mapping(&request).expect("table");
        request.level = Level::Column;
        request.logical_column = "amount".to_string();
        let column = plan_physical_mapping(&request).expect("column");
        let namespace = plan_physical_mapping(&golden()).expect("namespace");
        assert_eq!(table.scope_namespace_physical, namespace.physical_name);
        assert_eq!(table.scope_table_physical.as_deref(), Some(table.physical_name.as_str()));
        assert_eq!(column.scope_namespace_physical, namespace.physical_name);
        assert_eq!(column.scope_table_physical.as_deref(), Some(table.physical_name.as_str()));
        assert!(column.physical_name.starts_with("pc_"));
        assert_ne!(column.physical_name, table.physical_name);
    }

    #[test]
    fn closed_tokens_do_not_echo_the_input() {
        let mut request = golden();
        request.source = "_main".to_string();
        let error = plan_physical_mapping(&request).expect_err("source");
        assert_eq!(error, PlanError::ClosedSource);
        assert!(!error.to_string().contains("_main"));
        request.source = "src_main".to_string();
        request.config_digest = "AB".repeat(32);
        assert_eq!(plan_physical_mapping(&request).expect_err("config"), PlanError::ClosedConfig);
        request.config_digest = "ab".repeat(32);
        request.run_generation = 0;
        assert_eq!(plan_physical_mapping(&request).expect_err("generation"), PlanError::ClosedGeneration);
        assert_eq!(quote_physical_name("customer").expect_err("quote"), PlanError::NotPhysicalName);
        assert_eq!(quote_physical_name(&format!("\"{GOLDEN_NAME}\"")).expect_err("already"), PlanError::NotPhysicalName);
    }

    #[test]
    fn migration_keeps_the_pgident_reserved_list_and_its_own_writer_gate() {
        assert!(oo_connectors::PG17_RESERVED.windows(2).all(|pair| pair[0] < pair[1]));
        let start = MIGRATION.find("-- pm-reserved-begin").expect("begin");
        let end = MIGRATION.find("-- pm-reserved-end").expect("end");
        let mut words = Vec::new();
        for line in MIGRATION[start..end].lines() {
            let line = line.trim().trim_end_matches(',');
            if let Some(word) = line.strip_prefix('\'').and_then(|text| text.strip_suffix('\'')) {
                words.push(word);
            }
        }
        assert_eq!(words, oo_connectors::PG17_RESERVED);
        assert!(!MIGRATION.contains("_oo.writer"));
        assert!(!MIGRATION.contains("CREATE EXTENSION"));
        assert!(!MIGRATION.contains("UNLOGGED"));
        assert!(!MIGRATION.contains("apply_receipts"));
        assert!(MIGRATION.contains("_oo.physical_mapping_writer"));
        assert!(MIGRATION.contains("0003_target_segment_preflight_expand.sql"));
        assert_eq!(REGISTER_SQL.matches('$').count(), 8);
        assert_eq!(LOOKUP_SQL.matches('$').count(), 7);
        assert!(!REGISTER_SQL.contains('\''));
        assert!(!LOOKUP_SQL.contains('\''));
    }

    #[test]
    fn source_config_and_generation_change_the_physical_name() {
        let base = plan_physical_mapping(&golden()).expect("base");
        let mut request = golden();
        request.source = "src_other".to_string();
        let other_source = plan_physical_mapping(&request).expect("source");
        request = golden();
        request.config_digest = "cd".repeat(32);
        let other_config = plan_physical_mapping(&request).expect("config");
        request = golden();
        request.run_generation = 2;
        let other_generation = plan_physical_mapping(&request).expect("generation");
        assert_ne!(base.physical_name, other_source.physical_name);
        assert_ne!(base.physical_name, other_config.physical_name);
        assert_ne!(base.physical_name, other_generation.physical_name);
        assert_eq!(plan_physical_mapping(&golden()).expect("again"), base);
    }
}
