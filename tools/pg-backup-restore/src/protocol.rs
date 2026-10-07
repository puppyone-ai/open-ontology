//! Frozen names for this session. A different manifest is refused.

use std::fs;
use std::path::{Path, PathBuf};

use crate::json::{self, Value};
use crate::sha256::is_hex64;
use crate::{fail, Fail};

pub const LINEAGE: &str = "pg-backup-restore/v1";
pub const MANIFEST_VERSION: &str = "oo-pg-backup-manifest/v1";
pub const CALLER: &str = "trusted_local_admin";
pub const OWNER: &str = "a356";
pub const SESSION: &str = "eef831aa-dba4-4af2-a78b-541bc9e0a356";
pub const SOURCE_CUT: &str = "cut-a356-1";
pub const TARGET_FENCE: &str = "fence-a356-restore";
pub const CONTROL_DB: &str = "oo_a356_control";
pub const AGENT_DB: &str = "oo_a356_agent";
pub const RESTORE_CONTROL_DB: &str = "oo_a356_restore_control";
pub const RESTORE_AGENT_DB: &str = "oo_a356_restore_agent";
pub const ADMIN: &str = "oo_a356_admin";
pub const HOST: &str = "/var/run/postgresql";
pub const PORT: &str = "5432";
pub const VERSION_NEEDLE: &str = "(PostgreSQL) 17.10";

pub const SCHEMAS: &[(&str, &str, &str)] = &[
    (
        "cdc-offset-store-v1",
        CONTROL_DB,
        "tools/cdc-offset-store/migrations/oo_cdc_offset_control_v1.sql",
    ),
    (
        "session-store-v1",
        CONTROL_DB,
        "tools/session-store/migrations/oo_session_control_v1.sql",
    ),
    (
        "calendar-store-v1",
        CONTROL_DB,
        "tools/calendar-store/migrations/oo_calendar_control_v1.sql",
    ),
    (
        "enum-store-v1",
        AGENT_DB,
        "tools/enum-store/migrations/oo_enum_control_v1.sql",
    ),
];

pub struct CopySpec {
    pub file_name: &'static str,
    pub source_db: &'static str,
    pub restore_db: &'static str,
    pub select_sql: &'static str,
    pub shop: bool,
}

pub fn copies() -> &'static [CopySpec] {
    COPIES
}

const COPIES: &[CopySpec] = &[
    CopySpec { file_name: "cdc-install.copy", source_db: CONTROL_DB, restore_db: RESTORE_CONTROL_DB, select_sql: "SELECT singleton, sql_sha256 FROM oo_cdc_offset.install_manifest ORDER BY singleton", shop: false },
    CopySpec { file_name: "cdc-receipt.copy", source_db: CONTROL_DB, restore_db: RESTORE_CONTROL_DB, select_sql: "SELECT source, table_generation, txn_or_batch_id, config_version, config_digest, type_policy_version, type_digest, hook_digest, schema_digest, payload_digest, protocol_version, position_codec, position_lineage, source_epoch, applied_pos, partial_txn, cross_table_consistent, txn_complete, segment_index, segment_count, segment_bitmap, fence_shard, fence_generation, target_commit_at, receipt_hash FROM oo_cdc_offset.mirrored_receipt ORDER BY source, table_generation, txn_or_batch_id", shop: false },
    CopySpec { file_name: "cdc-watermark.copy", source_db: CONTROL_DB, restore_db: RESTORE_CONTROL_DB, select_sql: "SELECT source, position_codec, position_lineage, source_epoch, applied_pos, applied_txn_id, declared_next_txn, declared_next_pos, binding_provenance FROM oo_cdc_offset.watermark ORDER BY source, position_codec, position_lineage, source_epoch", shop: false },
    CopySpec { file_name: "calendar-install.copy", source_db: CONTROL_DB, restore_db: RESTORE_CONTROL_DB, select_sql: "SELECT singleton, sql_sha256 FROM oo_calendar_control.install_manifest ORDER BY singleton", shop: false },
    CopySpec { file_name: "calendar-snapshot.copy", source_db: CONTROL_DB, restore_db: RESTORE_CONTROL_DB, select_sql: "SELECT snapshot_id, project_id, calendar_version, closure_date, closure_label, closure_fingerprint FROM oo_calendar_control.snapshot ORDER BY snapshot_id", shop: false },
    CopySpec { file_name: "calendar-published.copy", source_db: CONTROL_DB, restore_db: RESTORE_CONTROL_DB, select_sql: "SELECT snapshot_id, project_id, calendar_version, last_closed_period_end_date, last_closed_period_end_midnight_status, last_closed_period_end_midnight FROM oo_calendar_control.published ORDER BY snapshot_id", shop: false },
    CopySpec { file_name: "session-head.copy", source_db: CONTROL_DB, restore_db: RESTORE_CONTROL_DB, select_sql: "SELECT project_id, session_id, revision, owner_generation, state_json, journal_len FROM oo_session_control.session_head ORDER BY project_id, session_id", shop: false },
    CopySpec { file_name: "key-issued.copy", source_db: CONTROL_DB, restore_db: RESTORE_CONTROL_DB, select_sql: "SELECT entity_key, source_member, issued_generation FROM oo_key_fixture.issued_key ORDER BY entity_key", shop: false },
    CopySpec { file_name: "key-negative.copy", source_db: CONTROL_DB, restore_db: RESTORE_CONTROL_DB, select_sql: "SELECT left_key, right_key, decision, operator_label FROM oo_key_fixture.negative_decision ORDER BY left_key, right_key", shop: false },
    CopySpec { file_name: "key-cut.copy", source_db: CONTROL_DB, restore_db: RESTORE_CONTROL_DB, select_sql: "SELECT singleton, source_cut, entry_count FROM oo_key_fixture.cut_marker ORDER BY singleton", shop: false },
    CopySpec { file_name: "enum-published.copy", source_db: AGENT_DB, restore_db: RESTORE_AGENT_DB, select_sql: "SELECT snapshot_id, line_no, code, status, enum_id FROM oo_enum_control.published ORDER BY snapshot_id, line_no", shop: false },
    CopySpec { file_name: "enum-install.copy", source_db: AGENT_DB, restore_db: RESTORE_AGENT_DB, select_sql: "SELECT singleton, sql_sha256 FROM oo_enum_control.install_manifest ORDER BY singleton", shop: false },
    CopySpec { file_name: "shop-party.copy", source_db: AGENT_DB, restore_db: RESTORE_AGENT_DB, select_sql: "SELECT party_key, display_name, active, phone, national_id FROM shop.party ORDER BY party_key", shop: true },
    CopySpec { file_name: "shop-account.copy", source_db: AGENT_DB, restore_db: RESTORE_AGENT_DB, select_sql: "SELECT account_id, party_key, opened, label FROM shop.account ORDER BY account_id", shop: true },
    CopySpec { file_name: "shop-entry.copy", source_db: AGENT_DB, restore_db: RESTORE_AGENT_DB, select_sql: "SELECT entry_id, account_id, qty, amount, memo FROM shop.entry ORDER BY entry_id", shop: true },
];

#[derive(Debug)]
pub struct Operator {
    pub tool_dir: PathBuf,
    pub password_file: PathBuf,
    pub schema_root: PathBuf,
    pub backup_dir: PathBuf,
    pub report_path: PathBuf,
    pub work_dir: PathBuf,
}

const OPERATOR_KEYS: &[&str] = &[
    "backup_dir",
    "caller",
    "host",
    "lineage_version",
    "maintenance_database",
    "owner",
    "password_file",
    "port",
    "report_path",
    "restore_databases",
    "schema_root",
    "schemas",
    "session",
    "source_cut",
    "source_databases",
    "target_fence",
    "tool_dir",
    "user",
    "work_dir",
];

pub fn load_operator(path: &Path) -> Result<Operator, Fail> {
    let text = fs::read_to_string(path).map_err(|err| fail(2, format!("operator file: {err}")))?;
    let value = json::parse(&text)?;
    json::require_keys(&value, OPERATOR_KEYS)?;
    expect(&value, "lineage_version", LINEAGE)?;
    expect(&value, "caller", CALLER)?;
    expect(&value, "owner", OWNER)?;
    expect(&value, "session", SESSION)?;
    expect(&value, "source_cut", SOURCE_CUT)?;
    expect(&value, "target_fence", TARGET_FENCE)?;
    expect(&value, "host", HOST)?;
    expect(&value, "port", PORT)?;
    expect(&value, "user", ADMIN)?;
    expect(&value, "maintenance_database", "postgres")?;
    expect_list(&value, "source_databases", &[CONTROL_DB, AGENT_DB])?;
    expect_list(&value, "restore_databases", &[RESTORE_CONTROL_DB, RESTORE_AGENT_DB])?;
    expect_schemas(&value)?;
    let tool_dir = absolute(value.string("tool_dir")?, "tool_dir")?;
    let tool_text = tool_dir.to_string_lossy();
    if tool_text.contains("homebrew") || tool_text.contains("Cellar") {
        return Err(fail(2, "tool dir is a host package path"));
    }
    Ok(Operator {
        tool_dir,
        password_file: absolute(value.string("password_file")?, "password_file")?,
        schema_root: absolute(value.string("schema_root")?, "schema_root")?,
        backup_dir: absolute(value.string("backup_dir")?, "backup_dir")?,
        report_path: absolute(value.string("report_path")?, "report_path")?,
        work_dir: absolute(value.string("work_dir")?, "work_dir")?,
    })
}

fn expect(value: &Value, key: &str, wanted: &str) -> Result<(), Fail> {
    let got = value.string(key)?;
    if got != wanted {
        return Err(fail(2, format!("{key} is not the frozen value")));
    }
    Ok(())
}

fn expect_list(value: &Value, key: &str, wanted: &[&str]) -> Result<(), Fail> {
    let items = value.array(key)?;
    if items.len() != wanted.len() {
        return Err(fail(2, format!("{key} length is not frozen")));
    }
    for (item, expect) in items.iter().zip(wanted.iter()) {
        let Value::String(text) = item else {
            return Err(fail(2, format!("{key} entry is not a string")));
        };
        if text != expect {
            return Err(fail(2, format!("{key} is not the frozen list")));
        }
    }
    Ok(())
}

fn expect_schemas(value: &Value) -> Result<(), Fail> {
    let items = value.array("schemas")?;
    if items.len() != SCHEMAS.len() {
        return Err(fail(2, "schemas length is not frozen"));
    }
    for (item, expect) in items.iter().zip(SCHEMAS.iter()) {
        json::require_keys(item, &["database", "file", "id"])?;
        if item.string("id")? != expect.0 || item.string("database")? != expect.1 || item.string("file")? != expect.2 {
            return Err(fail(2, "schemas are not the frozen list"));
        }
    }
    Ok(())
}

fn absolute(text: &str, label: &str) -> Result<PathBuf, Fail> {
    let path = PathBuf::from(text);
    if !path.is_absolute() || text.split('/').any(|part| part == "..") || text.contains('\0') {
        return Err(fail(2, format!("{label} path is refused")));
    }
    Ok(path)
}

pub fn schema_path(root: &Path, relative: &str) -> Result<PathBuf, Fail> {
    if relative.split('/').any(|part| part == ".." || part.is_empty()) {
        return Err(fail(2, "schema path is refused"));
    }
    let root = root.canonicalize().map_err(|err| fail(2, format!("schema root: {err}")))?;
    let path = root.join(relative);
    let path = path.canonicalize().map_err(|err| fail(2, format!("schema file: {err}")))?;
    if !path.starts_with(&root) {
        return Err(fail(2, "schema file escapes the root"));
    }
    Ok(path)
}

pub fn known_database(name: &str) -> bool {
    matches!(
        name,
        "postgres" | CONTROL_DB | AGENT_DB | RESTORE_CONTROL_DB | RESTORE_AGENT_DB
    )
}

pub fn ident(name: &str) -> Result<&str, Fail> {
    let bytes = name.as_bytes();
    let ok = !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_');
    if !ok {
        return Err(fail(2, "identifier is refused"));
    }
    Ok(name)
}

pub fn hex_digest(text: &str) -> Result<&str, Fail> {
    if is_hex64(text) {
        Ok(text)
    } else {
        Err(fail(1, "digest is not 64 lowercase hex characters"))
    }
}
