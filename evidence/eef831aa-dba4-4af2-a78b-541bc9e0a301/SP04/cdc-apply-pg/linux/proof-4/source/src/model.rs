//! Request, binding digests, and declared cell conversion.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cdc_spool::{Cell, Manifest};
use oo_core::config::model::TypePolicy;
use oo_core::convert::{self, MysqlIntKind, RawCell};
use oo_core::decimal::ExactInt;
use oo_pipeline::target_installer::sha256_hex;
use serde::{Deserialize, Serialize};

pub const CONTRACT: &str = "cdc-apply-pg-v1";
pub const APPLY_LIMIT: u64 = 200_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password_file: PathBuf,
    pub ca_file: PathBuf,
    pub server_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Segment {
    pub index: u32,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyDecl {
    pub table: String,
    pub ordinal: u16,
    pub name: String,
    pub pg_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColumnDecl {
    pub table: String,
    pub ordinal: u16,
    pub name: String,
    pub pg_type: String,
    pub nullable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub contract: String,
    pub spool_dir: PathBuf,
    pub endpoint: Endpoint,
    pub shard: String,
    pub owner: String,
    pub generation: i64,
    pub table_generation: String,
    pub run_generation: i64,
    pub lineage: String,
    pub source: String,
    pub epoch: u32,
    pub control_lease_valid: bool,
    pub segment: Option<Segment>,
    pub keys: Vec<KeyDecl>,
    pub columns: Vec<ColumnDecl>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Output {
    pub contract: &'static str,
    pub kind: &'static str,
    pub fault: Option<String>,
    pub outcome: Option<String>,
    pub sqlstate: Option<String>,
    pub source: Option<String>,
    pub txn_id: Option<String>,
    pub generation: Option<i64>,
    pub requested_generation: Option<i64>,
    pub generation_advanced: bool,
    pub applied_pos: Option<String>,
    pub row_writes: u64,
    pub tables: Vec<String>,
    pub target_applied: bool,
    pub ack_allowed: bool,
    pub db_cross_table_consistent: bool,
    pub source_cross_table_consistent: bool,
    pub upstream_commit_lsn_verified: bool,
    pub debezium_normalizer: bool,
    pub p2_verified: bool,
    pub p11_verified: bool,
    pub control_lease_consulted: bool,
    pub commit_position_provenance: &'static str,
}

impl Output {
    pub fn refused(fault: &str, requested_generation: Option<i64>) -> Self {
        Self {
            contract: CONTRACT,
            kind: "refused",
            fault: Some(fault.to_owned()),
            outcome: None,
            sqlstate: None,
            source: None,
            txn_id: None,
            generation: None,
            requested_generation,
            generation_advanced: false,
            applied_pos: None,
            row_writes: 0,
            tables: Vec::new(),
            target_applied: false,
            ack_allowed: false,
            db_cross_table_consistent: false,
            source_cross_table_consistent: false,
            upstream_commit_lsn_verified: false,
            debezium_normalizer: false,
            p2_verified: false,
            p11_verified: false,
            control_lease_consulted: false,
            commit_position_provenance: "caller_declared",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PgType {
    Bool,
    Int2,
    Int4,
    Int8,
    Text,
}

impl PgType {
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "bool" => Self::Bool,
            "int2" => Self::Int2,
            "int4" => Self::Int4,
            "int8" => Self::Int8,
            "text" => Self::Text,
            _ => return None,
        })
    }

    pub fn cast(self) -> &'static str {
        match self {
            Self::Bool => "::text::bool",
            Self::Int2 => "::text::int2",
            Self::Int4 => "::text::int4",
            Self::Int8 => "::text::int8",
            Self::Text => "::text",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldPlan {
    pub ordinal: u16,
    pub name: String,
    pub pg_type: PgType,
    pub nullable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TablePlan {
    pub logical: String,
    pub namespace: String,
    pub table: String,
    pub keys: Vec<FieldPlan>,
    pub columns: Vec<FieldPlan>,
}

/// Binding fields that enter the config, schema, and type digests.
#[derive(Clone, Debug)]
pub struct Binding<'a> {
    pub source: &'a str,
    pub epoch: u32,
    pub lineage: &'a str,
    pub shard: &'a str,
    pub table_generation: &'a str,
    pub run_generation: i64,
    pub tables: &'a [TablePlan],
}

pub fn config_preimage(binding: &Binding<'_>) -> String {
    let source = binding.source;
    let epoch = binding.epoch;
    let lineage = binding.lineage;
    let shard = binding.shard;
    let table_generation = binding.table_generation;
    let run_generation = binding.run_generation;
    format!(
        "cdc-apply-config/v1\nsource={source}\nepoch={epoch}\nlineage={lineage}\nshard={shard}\ntable_generation={table_generation}\nrun_generation={run_generation}\nposition_codec=pg-commit-lsn/v1\nprovenance=caller_declared\n"
    )
}

pub fn schema_preimage(tables: &[TablePlan]) -> String {
    let mut out = String::from("cdc-apply-schema/v1\n");
    push_tables(&mut out, tables);
    out
}

pub fn type_preimage(tables: &[TablePlan]) -> String {
    let mut out = String::from(
        "cdc-apply-type-policy/v1\nzero_date=null\ntinyint1=smallint\nstrip_nul=true\n",
    );
    push_tables(&mut out, tables);
    out
}

pub fn hook_preimage(hooks: &[String]) -> String {
    let mut out = String::from("cdc-apply-hooks/v1\n");
    for hook in hooks {
        out.push_str(hook);
        out.push('\n');
    }
    out
}

pub fn payload_preimage(event_hashes: &[String]) -> String {
    let mut out = String::from("cdc-apply-payload/v1\n");
    for hash in event_hashes {
        out.push_str(hash);
        out.push('\n');
    }
    out
}

pub fn digest_text(text: &str) -> String {
    sha256_hex(text.as_bytes())
}

fn push_tables(out: &mut String, tables: &[TablePlan]) {
    for table in tables {
        out.push_str("table ");
        out.push_str(&table.logical);
        out.push('\n');
        for key in &table.keys {
            let ordinal = key.ordinal;
            let name = &key.name;
            let ty = pg_name(key.pg_type);
            out.push_str(&format!("key {ordinal} {name} {ty}\n"));
        }
        for column in &table.columns {
            let ordinal = column.ordinal;
            let name = &column.name;
            let ty = pg_name(column.pg_type);
            let nulls = u8::from(column.nullable);
            out.push_str(&format!("col {ordinal} {name} {ty} {nulls}\n"));
        }
    }
}

fn pg_name(pg_type: PgType) -> &'static str {
    match pg_type {
        PgType::Bool => "bool",
        PgType::Int2 => "int2",
        PgType::Int4 => "int4",
        PgType::Int8 => "int8",
        PgType::Text => "text",
    }
}

pub fn request_closed(request: &Request) -> Result<(), &'static str> {
    if request.contract != CONTRACT {
        return Err("usage");
    }
    if request.endpoint.host.is_empty()
        || request.endpoint.host != request.endpoint.server_name
        || request.endpoint.port == 0
        || request.endpoint.database.is_empty()
        || request.endpoint.user.is_empty()
    {
        return Err("endpoint");
    }
    if !shard_ok(&request.shard) || !table_generation_ok(&request.table_generation) {
        return Err("identity");
    }
    if !uuid_ok(&request.owner) || request.generation < 1 || request.run_generation < 1 {
        return Err("identity");
    }
    if i32::try_from(request.epoch).is_err() {
        return Err("epoch_range");
    }
    // The installed receipt accepts position_lineage only as
    // ^[a-z][a-z0-9_.:-]{0,127}$. A 64-hex spool lineage fits only when it
    // starts with a-f. Refuse the rest before any connection.
    if !hex64(&request.lineage)
        || !request.lineage.as_bytes()[0].is_ascii_lowercase()
        || !source_ok(&request.source)
    {
        return Err("identity");
    }
    Ok(())
}

pub fn plan_tables(request: &Request, selected: &[String]) -> Result<Vec<TablePlan>, &'static str> {
    let mut grouped: BTreeMap<String, (Vec<FieldPlan>, Vec<FieldPlan>)> = BTreeMap::new();
    for name in selected {
        grouped.insert(name.clone(), (Vec::new(), Vec::new()));
    }
    for key in &request.keys {
        let slot = grouped.get_mut(&key.table).ok_or("binding_mismatch")?;
        let pg_type = PgType::parse(&key.pg_type).ok_or("unsupported_type")?;
        if !logical_piece_ok(&key.name) {
            return Err("mapping_name");
        }
        slot.0.push(FieldPlan {
            ordinal: key.ordinal,
            name: key.name.clone(),
            pg_type,
            nullable: false,
        });
    }
    for column in &request.columns {
        let slot = grouped.get_mut(&column.table).ok_or("binding_mismatch")?;
        let pg_type = PgType::parse(&column.pg_type).ok_or("unsupported_type")?;
        if !logical_piece_ok(&column.name) {
            return Err("mapping_name");
        }
        slot.1.push(FieldPlan {
            ordinal: column.ordinal,
            name: column.name.clone(),
            pg_type,
            nullable: column.nullable,
        });
    }
    if request.keys.iter().any(|key| !selected.iter().any(|name| name == &key.table)) {
        return Err("binding_mismatch");
    }
    if request
        .columns
        .iter()
        .any(|column| !selected.iter().any(|name| name == &column.table))
    {
        return Err("binding_mismatch");
    }
    let mut tables = Vec::new();
    for (logical, (mut keys, mut columns)) in grouped {
        if keys.is_empty() {
            return Err("key_required");
        }
        keys.sort_by_key(|field| field.ordinal);
        columns.sort_by_key(|field| field.ordinal);
        if !strict_ordinals(&keys) || !strict_ordinals(&columns) {
            return Err("ordinal_duplicate");
        }
        let (namespace, table) = split_table(&logical).ok_or("binding_mismatch")?;
        tables.push(TablePlan {
            logical,
            namespace,
            table,
            keys,
            columns,
        });
    }
    Ok(tables)
}

fn strict_ordinals(fields: &[FieldPlan]) -> bool {
    fields.windows(2).all(|pair| pair[0].ordinal < pair[1].ordinal)
}

pub fn binding_from<'a>(request: &'a Request, tables: &'a [TablePlan]) -> Binding<'a> {
    Binding {
        source: &request.source,
        epoch: request.epoch,
        lineage: &request.lineage,
        shard: &request.shard,
        table_generation: &request.table_generation,
        run_generation: request.run_generation,
        tables,
    }
}

pub fn manifest_matches(request: &Request, manifest: &Manifest, tables: &[TablePlan]) -> Result<(), &'static str> {
    if manifest.contract != "cdc-spool-v1"
        || manifest.ack_allowed
        || manifest.target_applied
        || manifest.cross_table_consistent
        || manifest.upstream_commit_lsn_verified
        || manifest.debezium_normalizer
        || manifest.p2_verified
        || manifest.p11_verified
        || !manifest.applied.is_null()
        || manifest.commit_position_provenance != "caller_declared"
    {
        return Err("spool_attestation");
    }
    if manifest.source != request.source
        || manifest.lineage != request.lineage
        || manifest.epoch != request.epoch
    {
        return Err("binding_mismatch");
    }
    if !batch_ok(&manifest.txn_id) {
        return Err("batch_identity");
    }
    let binding = binding_from(request, tables);
    if manifest.config_hash != digest_text(&config_preimage(&binding))
        || manifest.schema_hash != digest_text(&schema_preimage(tables))
        || manifest.type_hash != digest_text(&type_preimage(tables))
    {
        return Err("binding_mismatch");
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stored {
    Null,
    Unavailable,
    Value(String),
}

pub fn convert_cell(cell: &Cell, pg_type: PgType) -> Result<Stored, &'static str> {
    match cell {
        Cell::Null { .. } => Ok(Stored::Null),
        Cell::Unavailable { .. } => Ok(Stored::Unavailable),
        Cell::Bool { value, .. } => {
            if pg_type != PgType::Bool {
                return Err("type_mismatch");
            }
            let outcome = convert::convert_cell(&RawCell::Bool(*value), &TypePolicy::default(), "UTC", false);
            match outcome.value {
                Some(oo_core::canonical::LogicalValue::Bool(bit)) if outcome.reject.is_none() => {
                    Ok(Stored::Value(if bit { "true" } else { "false" }.to_owned()))
                }
                _ => Err("conversion_rejected"),
            }
        }
        Cell::Int { value, .. } => convert_int(value, pg_type),
        Cell::Text { value, .. } => {
            if pg_type != PgType::Text {
                return Err("type_mismatch");
            }
            let outcome =
                convert::convert_cell(&RawCell::Text(value.clone()), &TypePolicy::default(), "UTC", false);
            if outcome.reject.is_some() {
                return Err("conversion_rejected");
            }
            match outcome.value {
                Some(oo_core::canonical::LogicalValue::Text(text)) => Ok(Stored::Value(text)),
                _ => Err("conversion_rejected"),
            }
        }
        Cell::Decimal { .. } | Cell::Bytes { .. } => Err("type_mismatch"),
    }
}

fn convert_int(value: &str, pg_type: PgType) -> Result<Stored, &'static str> {
    let width = match pg_type {
        PgType::Int2 | PgType::Int4 | PgType::Int8 => pg_type,
        PgType::Bool | PgType::Text => return Err("type_mismatch"),
    };
    let outcome = convert::convert_cell(
        &RawCell::MysqlInt {
            text: value.to_owned(),
            unsigned: false,
            kind: MysqlIntKind::Big,
        },
        &TypePolicy::default(),
        "UTC",
        false,
    );
    if outcome.reject.is_some() {
        return Err("conversion_rejected");
    }
    let Some(oo_core::canonical::LogicalValue::Int(integer)) = outcome.value else {
        return Err("conversion_rejected");
    };
    let number = fit_int(&integer, width)?;
    Ok(Stored::Value(number.to_string()))
}

fn fit_int(integer: &ExactInt, pg_type: PgType) -> Result<i64, &'static str> {
    let digits = integer.coefficient();
    let negative = integer.is_negative();
    let magnitude: i64 = if negative && digits == "9223372036854775808" && pg_type == PgType::Int8 {
        return Ok(i64::MIN);
    } else {
        digits.parse().map_err(|_| "out_of_range")?
    };
    let signed = if negative {
        magnitude.checked_neg().ok_or("out_of_range")?
    } else {
        magnitude
    };
    let ok = match pg_type {
        PgType::Int2 => i64::from(i16::MIN) <= signed && signed <= i64::from(i16::MAX),
        PgType::Int4 => i64::from(i32::MIN) <= signed && signed <= i64::from(i32::MAX),
        PgType::Int8 => true,
        PgType::Bool | PgType::Text => false,
    };
    if ok { Ok(signed) } else { Err("out_of_range") }
}

pub fn sql_text(stored: &Stored) -> Option<Option<String>> {
    match stored {
        Stored::Unavailable => None,
        Stored::Null => Some(None),
        Stored::Value(text) => Some(Some(text.clone())),
    }
}

fn split_table(logical: &str) -> Option<(String, String)> {
    let (namespace, table) = logical.split_once('.')?;
    if logical_piece_ok(namespace) && logical_piece_ok(table) && !logical.contains("..") {
        Some((namespace.to_owned(), table.to_owned()))
    } else {
        None
    }
}

fn logical_piece_ok(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 || bytes.starts_with(b"q_") {
        return false;
    }
    let mut chars = bytes.iter();
    let Some(first) = chars.next() else {
        return false;
    };
    (*first == b'_' || first.is_ascii_lowercase())
        && chars.all(|byte| *byte == b'_' || byte.is_ascii_lowercase() || byte.is_ascii_digit())
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
    first.is_ascii_lowercase()
        && chars.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn shard_ok(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 128 {
        return false;
    }
    let mut chars = bytes.iter();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_lowercase()
        && chars.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(*byte, b'_' | b'.' | b':' | b'-')
        })
}

fn table_generation_ok(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 {
        return false;
    }
    let mut chars = bytes.iter();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_lowercase()
        && chars.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn batch_ok(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && bytes.iter().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'.' | b':' | b'-')
        })
}

fn uuid_ok(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    bytes.iter().enumerate().all(|(index, byte)| match index {
        8 | 13 | 18 | 23 => *byte == b'-',
        _ => byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase(),
    })
}

fn hex64(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 64 && bytes.iter().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
