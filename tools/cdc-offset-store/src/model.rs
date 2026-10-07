//! Request and output shapes. Unknown JSON fields are refused.
//! A caller cannot supply the receipt body.

use serde::{Deserialize, Serialize};

use crate::rules::{pos_ok, IDENT, LINEAGE, TXN};

pub const CONTRACT: &str = "cdc-offset-store-v1";
pub const PROVENANCE: &str = "caller_declared_pending_real_normalizer";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password_file: String,
    pub ca_file: String,
    pub server_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointRef {
    pub position_codec: String,
    pub position_lineage: String,
    pub source_epoch: i32,
    pub applied_pos: String,
    pub txn_or_batch_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredTxn {
    pub txn_or_batch_id: String,
    pub applied_pos: String,
    pub required_table_generations: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredNext {
    pub txn_or_batch_id: String,
    pub applied_pos: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrderBinding {
    pub provenance: String,
    pub expected_previous: Option<CheckpointRef>,
    pub declared_transaction: DeclaredTxn,
    pub declared_next: DeclaredNext,
    pub source_gap: bool,
    pub epoch_barrier: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub contract: String,
    pub target: Endpoint,
    pub control: Endpoint,
    pub source: String,
    pub table_generation: String,
    pub txn_or_batch_id: String,
    pub order_binding: OrderBinding,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallRequest {
    pub contract: String,
    pub control: Endpoint,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Output {
    pub contract: &'static str,
    pub kind: String,
    pub fault: Option<String>,
    pub durable_control_mirror: bool,
    pub checkpoint_advanced: bool,
    pub checkpoint_provenance: &'static str,
    pub watermark_hold: Option<String>,
    pub source_applied: bool,
    pub fence_proof: bool,
    pub ack_allowed: bool,
    pub external_xack: bool,
    pub external_xtrim: bool,
    pub routing_verified: bool,
    pub real_source_provenance_verified: bool,
    pub debezium_normalizer: bool,
    pub receipt_hash: Option<String>,
    pub watermark_applied_pos: Option<String>,
    pub watermark_txn_or_batch_id: Option<String>,
    pub watermark_source_epoch: Option<i32>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct InstallOutput {
    pub contract: &'static str,
    pub kind: String,
    pub fault: Option<String>,
    pub sql_sha256: Option<String>,
    pub target_touched: bool,
}

impl Output {
    pub fn refused(fault: &str) -> Self {
        Self {
            contract: CONTRACT,
            kind: "refused".to_owned(),
            fault: Some(fault.to_owned()),
            durable_control_mirror: false,
            checkpoint_advanced: false,
            checkpoint_provenance: PROVENANCE,
            watermark_hold: None,
            source_applied: false,
            fence_proof: false,
            ack_allowed: false,
            external_xack: false,
            external_xtrim: false,
            routing_verified: false,
            real_source_provenance_verified: false,
            debezium_normalizer: false,
            receipt_hash: None,
            watermark_applied_pos: None,
            watermark_txn_or_batch_id: None,
            watermark_source_epoch: None,
        }
    }

    pub fn unknown() -> Self {
        let mut out = Self::refused("commit_unknown");
        out.kind = "commit_unknown".to_owned();
        out
    }
}

impl InstallOutput {
    pub fn refused(fault: &str) -> Self {
        Self {
            contract: CONTRACT,
            kind: "refused".to_owned(),
            fault: Some(fault.to_owned()),
            sql_sha256: None,
            target_touched: false,
        }
    }
}

pub fn validate_mirror(request: &Request) -> Result<(), &'static str> {
    if request.contract != CONTRACT {
        return Err("input");
    }
    validate_endpoint(&request.target)?;
    validate_endpoint(&request.control)?;
    if request.target.database == request.control.database && request.target.host == request.control.host
    {
        return Err("input");
    }
    if !ident(&request.source) || !ident(&request.table_generation) || !txn(&request.txn_or_batch_id) {
        return Err("input");
    }
    let binding = &request.order_binding;
    if binding.provenance != PROVENANCE {
        return Err("input");
    }
    if binding.declared_transaction.txn_or_batch_id != request.txn_or_batch_id {
        return Err("binding_txn");
    }
    if !pos_ok(&binding.declared_transaction.applied_pos) || !pos_ok(&binding.declared_next.applied_pos) {
        return Err("input");
    }
    if !txn(&binding.declared_next.txn_or_batch_id)
        || binding.declared_next.txn_or_batch_id == request.txn_or_batch_id
    {
        return Err("declared_next");
    }
    if crate::rules::cmp_pos(&binding.declared_next.applied_pos, &binding.declared_transaction.applied_pos)
        != std::cmp::Ordering::Greater
    {
        return Err("declared_next");
    }
    let tables = &binding.declared_transaction.required_table_generations;
    if tables.is_empty() || tables.len() > 32 {
        return Err("required_tables");
    }
    if !tables.contains(&request.table_generation) {
        return Err("required_tables");
    }
    let mut previous = "";
    for table in tables {
        if !ident(table) || table.as_str() <= previous {
            return Err("required_tables");
        }
        previous = table;
    }
    if let Some(previous) = &binding.expected_previous
        && (!crate::rules::codec_ok(&previous.position_codec)
            || !lineage(&previous.position_lineage)
            || previous.source_epoch < 0
            || !pos_ok(&previous.applied_pos)
            || !txn(&previous.txn_or_batch_id))
    {
        return Err("input");
    }
    Ok(())
}

pub fn validate_install(request: &InstallRequest) -> Result<(), &'static str> {
    if request.contract != CONTRACT {
        return Err("input");
    }
    validate_endpoint(&request.control)
}

fn validate_endpoint(endpoint: &Endpoint) -> Result<(), &'static str> {
    if endpoint.port == 0 || endpoint.host != endpoint.server_name || !dns(&endpoint.host) {
        return Err("endpoint");
    }
    if !db_name(&endpoint.database) || !db_name(&endpoint.user) {
        return Err("endpoint");
    }
    if !absolute_path(&endpoint.password_file) || !absolute_path(&endpoint.ca_file) {
        return Err("endpoint");
    }
    Ok(())
}

fn ident(value: &str) -> bool {
    regex_match(value, IDENT)
}

fn txn(value: &str) -> bool {
    regex_match(value, TXN)
}

fn lineage(value: &str) -> bool {
    regex_match(value, LINEAGE)
}

fn dns(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 253
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'.' || *byte == b'-')
}

fn db_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && (bytes[0].is_ascii_lowercase() || bytes[0] == b'_')
        && bytes.iter().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
}

fn absolute_path(value: &str) -> bool {
    let path = std::path::Path::new(value);
    path.is_absolute() && !value.contains("..") && !value.contains('\0')
}

fn regex_match(value: &str, pattern: &str) -> bool {
    // Patterns here are the closed tokens from the target receipt checks.
    match pattern {
        IDENT => {
            let bytes = value.as_bytes();
            !bytes.is_empty()
                && bytes.len() <= 63
                && bytes[0].is_ascii_lowercase()
                && bytes.iter().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
        }
        TXN => {
            let bytes = value.as_bytes();
            (1..=128).contains(&bytes.len())
                && bytes.iter().all(|b| {
                    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-')
                })
        }
        LINEAGE => {
            let bytes = value.as_bytes();
            !bytes.is_empty()
                && bytes.len() <= 128
                && bytes[0].is_ascii_lowercase()
                && bytes.iter().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.' | b':' | b'-')
                })
        }
        _ => false,
    }
}
