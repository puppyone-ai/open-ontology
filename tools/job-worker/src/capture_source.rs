//! Fixed Doris capture-tick handler.
//!
//! The job payload stays `{v, binding, pin}`. Argv, the shell, and the
//! password are not accepted from that payload. The binary path, digest,
//! and command are the retained reader. A replay receipt builds no argv.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::sha256::sha256_hex;

pub const SOURCE_SQL_SHA: &str = "68a4496489e45aba7b807e30890aee8a600cf2902d4f663943f64fcca0f1729f";
pub const SOURCE_SQL: &str = include_str!("../capture_source.sql");

pub const READER_NAME: &str = "doris-reader";
pub const READER_SHA256: &str = "d5dc8555ad76ecc9d4550ac5d2abd6b00bb2fee6ca5a17c7c206d294b206010c";
pub const READER_REL: &str =
    "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product/capture-live/build/doris-reader";
pub const READER_COMMAND: &str = "capture-tick";
pub const SOURCE_DB: &str = "oo_dl01w1_43";
pub const SOURCE_USER: &str = "oo_reader";
pub const DORIS_PROTOCOL: &str = "oo.doris.capture-tick/v1";
pub const DORIS_DATABASE: &str = "oo_dl01w1_43";
pub const APPROVED_SECRET_REF: &str = "secret://oo-dl01w1-43/reader";
pub const APPROVED_SECRET_DIR: &str = "/tmp/oo-dl01w1-43-ns2-reader";
/// Historical loader. It trusted a receipt this worker had just written.
/// It stays off the allowlist.
pub const UNREPAIRED_ELF_SHA256: &str =
    "4673c8b6e58526c46cac9697b13fd015601992179ac6c38f0e21cf3566acabfe";
/// Linux ELF from `e37a6f9dead618283d654c70c65302f14c430d37`.
/// It locks a presented lease and does not call `jobs.claim`.
/// It does not set `target_fence_certified`.
pub const NATIVE_ELF_SHA256: &str =
    "56514ae7e734ab211365c37b8889644f93a979dc82326708a7cfc3a7012e9866";
pub const NATIVE_ELF_PATH: &str = "/src/evidence/b5ac4bc8-1872-4fe4-a56a-04a70f7103a2/DL01/native-physical-import/trust-registration/raw/194c/oo-pipeline";
pub const REPAIRED_LOADER_SHA256: &[&str] = &[NATIVE_ELF_SHA256];
pub const AUTHORITATIVE_TRUST_ENTRY: &str = "verify_inputs";
pub const NATIVE_PROTOCOL: &str = "oo.pipeline.native-update/v1";
pub const INSTALL_PROTOCOL: &str = "oo.pipeline.install-native-update/v1";
pub const LOADER_COMMIT: &str = "e37a6f9dead618283d654c70c65302f14c430d37";
pub const SOURCE_ID: &str = "doris_oo_dl01w1_43";
pub const CAPTURE_RECEIPT_PROTOCOL: &str = "oo.source-admission.capture-receipt/v1";
pub const BINDING_REGISTRY_PROTOCOL: &str = "oo.source-admission.binding-registry/v1";
pub const CAPTURE_BINDING_PROTOCOL: &str = "oo.source-admission.capture-binding/v1";

const DORIS_COMMITS: [&str; 2] = [
    "a5aed1167caa54e68e3a2be7335142465fb2d31a",
    "d9255c6a69a9b534a5f5c792a964c43023764f58",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReaderAction {
    Skip,
    Read,
    Refuse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoaderGap {
    Snapshot,
    PartitionReplace,
    Tombstone,
    ColumnUpdate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceRow {
    pub driver_name: String,
    pub driver_commit: String,
    pub driver_sha256: String,
    pub binary_rel: String,
    pub secret_ref: String,
    pub source_host: String,
    pub source_port: i32,
    pub evidence_rel: String,
    pub command: String,
}

pub fn sql_pinned() -> bool {
    sha256_hex(SOURCE_SQL.as_bytes()) == SOURCE_SQL_SHA
}

pub fn reader_action(recover: &str) -> ReaderAction {
    match recover {
        "replay" => ReaderAction::Skip,
        "absent" => ReaderAction::Read,
        _ => ReaderAction::Refuse,
    }
}

pub fn label_for_pin(pin: i64) -> Result<String, &'static str> {
    if pin < 1 {
        return Err("label");
    }
    let label = format!("pin{pin}");
    if !label_ok(&label) {
        return Err("label");
    }
    Ok(label)
}

pub fn label_ok(label: &str) -> bool {
    (1..=32).contains(&label.len())
        && label
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

pub fn ipv4_ok(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    parts.len() == 4
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.len() <= 3
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && part.parse::<u8>().is_ok()
                && (part.len() == 1 || !part.starts_with('0'))
        })
}

pub fn evidence_rel_ok(value: &str) -> bool {
    let Some((head, leaf)) = value.rsplit_once('/') else {
        return false;
    };
    let Some(run) = head.strip_prefix("linux/run-source-") else {
        return false;
    };
    !run.is_empty()
        && run.len() <= 40
        && leaf.len() <= 40
        && !leaf.is_empty()
        && run
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && leaf
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.contains("..")
}

pub fn resolve_secret_dir(secret_ref: &str) -> Result<&str, &'static str> {
    if secret_ref == APPROVED_SECRET_REF {
        return Ok(APPROVED_SECRET_DIR);
    }
    if secret_dir_ok(secret_ref) {
        return Ok(secret_ref);
    }
    Err("source_secret_rejected")
}

pub fn secret_dir_ok(value: &str) -> bool {
    value.starts_with('/')
        && !value.contains("..")
        && !value.contains('\0')
        && !value.contains("reader.password")
        && (2..=256).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-'))
}

pub fn fixed_source(row: &SourceRow) -> Result<(), &'static str> {
    if row.driver_name != READER_NAME || row.command != READER_COMMAND {
        return Err("source_config_rejected");
    }
    if row.driver_sha256 != READER_SHA256 || row.binary_rel != READER_REL {
        return Err("source_config_rejected");
    }
    if !DORIS_COMMITS.contains(&row.driver_commit.as_str()) {
        return Err("source_config_rejected");
    }
    if !ipv4_ok(&row.source_host) || !(1..=65535).contains(&row.source_port) {
        return Err("source_config_rejected");
    }
    if !evidence_rel_ok(&row.evidence_rel) || resolve_secret_dir(&row.secret_ref).is_err() {
        return Err("source_config_rejected");
    }
    Ok(())
}

pub fn binary_digest_ok(bytes: &[u8]) -> bool {
    sha256_hex(bytes) == READER_SHA256
}

pub struct TickSpec<'a> {
    pub action: ReaderAction,
    pub binary: &'a Path,
    pub evidence: &'a Path,
    pub secret_dir: &'a str,
    pub host: &'a str,
    pub port: i32,
    pub every_ms: i32,
    pub label: &'a str,
}

/// Replay builds an empty argv. A read builds the fixed capture-tick vector.
/// The password is not an argument.
pub fn tick_argv(spec: TickSpec<'_>) -> Result<Vec<String>, &'static str> {
    match spec.action {
        ReaderAction::Skip => Ok(Vec::new()),
        ReaderAction::Refuse => Err("recover"),
        ReaderAction::Read => {
            if !(1000..=86_400_000).contains(&spec.every_ms) || !label_ok(spec.label) || !ipv4_ok(spec.host) {
                return Err("source_config_rejected");
            }
            if !secret_dir_ok(spec.secret_dir) {
                return Err("source_secret_rejected");
            }
            let binary = arg_text(spec.binary)?;
            let evidence = arg_text(spec.evidence)?;
            if binary.contains("..") || evidence.contains("..") {
                return Err("source_config_rejected");
            }
            Ok(vec![
                binary,
                READER_COMMAND.to_string(),
                evidence,
                spec.secret_dir.to_string(),
                spec.host.to_string(),
                spec.port.to_string(),
                spec.every_ms.to_string(),
                spec.label.to_string(),
            ])
        }
    }
}

fn arg_text(path: &Path) -> Result<String, &'static str> {
    let text = path.to_str().ok_or("source_config_rejected")?;
    if text.is_empty() || text.contains('\0') || text.contains('\n') || text.contains('\r') {
        return Err("source_config_rejected");
    }
    Ok(text.to_string())
}

pub fn join_under(root: &Path, rel: &str) -> Result<PathBuf, &'static str> {
    if rel.is_empty() || rel.starts_with('/') || rel.contains("..") || rel.contains('\0') {
        return Err("source_config_rejected");
    }
    Ok(root.join(rel))
}

pub fn password_file_mode_ok(mode: u32) -> bool {
    mode & 0o077 == 0
}

pub fn scrub_secret(text: &str, secret: &str) -> String {
    if secret.len() >= 8 {
        text.replace(secret, "[redacted]")
    } else {
        text.to_string()
    }
}

pub fn reader_refusal(stderr: &str) -> &'static str {
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("secret") {
        "source_secret_rejected"
    } else if lower.contains("connect") || lower.contains("timed out") || lower.contains("timeout") {
        "source_endpoint_unavailable"
    } else if lower.contains("version") || lower.contains("show backends") {
        "source_version_unmatched"
    } else if lower.contains("schedule") || lower.contains("every_ms") || lower.contains("overlap") {
        "source_schedule_refused"
    } else {
        "source_reader_failed"
    }
}

pub fn missing_parameter(reason: &str, host: &str, port: i32) -> String {
    match reason {
        "source_endpoint_unavailable" => {
            format!("fe_query_host_{host}_port_{port}_db_{SOURCE_DB}_user_{SOURCE_USER}")
        }
        "source_secret_rejected" => "reader.password".to_string(),
        "source_version_unmatched" => "show_backends_doris_3_1_cell".to_string(),
        "source_schedule_refused" => "every_ms".to_string(),
        _ => "capture_tick_exit".to_string(),
    }
}

pub fn failure_kind(reason: &str) -> &'static str {
    match reason {
        "source_endpoint_unavailable" => "dependency_unavailable",
        "source_schedule_refused" => "external_timeout",
        "source_secret_rejected" | "source_config_rejected" => "payload_rejected",
        _ => "worker_failed",
    }
}

pub fn loader_gap(bytes: &[u8], table: &str, prior: Option<&[String]>) -> Result<LoaderGap, &'static str> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| "source_not_legal_window")?;
    let protocol = value.get("protocol").and_then(Value::as_str).unwrap_or("");
    let database = value.get("database").and_then(Value::as_str).unwrap_or("");
    if protocol != DORIS_PROTOCOL || database != DORIS_DATABASE {
        return Err("source_not_legal_window");
    }
    let tables = value
        .get("tables")
        .and_then(Value::as_array)
        .ok_or("source_not_legal_window")?;
    let found = tables
        .iter()
        .find(|item| item.get("name").and_then(Value::as_str) == Some(table))
        .ok_or("source_not_legal_window")?;
    let route = found.get("route").and_then(Value::as_str).unwrap_or("");
    let delete_mode = found.get("delete_mode").and_then(Value::as_str).unwrap_or("");
    if route == "C4" || delete_mode == "partition_replace" {
        return Ok(LoaderGap::PartitionReplace);
    }
    let quality = found.get("quality").and_then(Value::as_str).unwrap_or("");
    if route != "C5" || delete_mode != "reconcile" || quality != "DELETES_LAGGED" {
        return Err("source_not_legal_window");
    }
    let rows = found
        .get("rows")
        .and_then(Value::as_array)
        .ok_or("source_not_legal_window")?;
    let mut next = Vec::with_capacity(rows.len());
    for row in rows {
        next.push(row.as_str().ok_or("source_not_legal_window")?.to_string());
    }
    let Some(prior) = prior else {
        return Ok(LoaderGap::Snapshot);
    };
    Ok(row_gap(prior, &next))
}

pub fn row_gap(prior: &[String], next: &[String]) -> LoaderGap {
    let prior_keys = keys(prior);
    let next_keys = keys(next);
    if prior_keys.iter().any(|key| !next_keys.contains(key)) {
        return LoaderGap::Tombstone;
    }
    if prior == next {
        LoaderGap::Snapshot
    } else {
        LoaderGap::ColumnUpdate
    }
}

fn keys(rows: &[String]) -> Vec<String> {
    rows.iter()
        .map(|row| row.split('\u{1f}').next().unwrap_or("").to_string())
        .collect()
}

pub struct NativeColumn {
    pub name: &'static str,
    pub arrow_type: &'static str,
    pub nullable: bool,
    pub precision: Option<i64>,
    pub scale: Option<i64>,
}

const STR_COLUMNS: &[NativeColumn] = &[
    NativeColumn {
        name: "k",
        arrow_type: "utf8",
        nullable: false,
        precision: None,
        scale: None,
    },
    NativeColumn {
        name: "note",
        arrow_type: "utf8",
        nullable: true,
        precision: None,
        scale: None,
    },
];

const NZ_COLUMNS: &[NativeColumn] = &[
    NativeColumn {
        name: "id",
        arrow_type: "int64",
        nullable: false,
        precision: None,
        scale: None,
    },
    NativeColumn {
        name: "qty",
        arrow_type: "decimal128",
        nullable: true,
        precision: Some(10),
        scale: Some(2),
    },
];

const PART_COLUMNS: &[NativeColumn] = &[
    NativeColumn {
        name: "day",
        arrow_type: "date32",
        nullable: false,
        precision: None,
        scale: None,
    },
    NativeColumn {
        name: "region",
        arrow_type: "utf8",
        nullable: false,
        precision: None,
        scale: None,
    },
    NativeColumn {
        name: "amount",
        arrow_type: "decimal128",
        nullable: true,
        precision: Some(12),
        scale: Some(2),
    },
];

pub fn native_columns(table: &str) -> Option<&'static [NativeColumn]> {
    match table {
        "str_key" => Some(STR_COLUMNS),
        "nz_obs" => Some(NZ_COLUMNS),
        "part_day" => Some(PART_COLUMNS),
        _ => None,
    }
}

pub struct NativeTarget<'a> {
    pub host: &'a str,
    pub port: u16,
    pub database: &'a str,
    pub runtime_principal: &'a str,
    pub password_file: &'a str,
    pub ca_file: &'a str,
    pub shard: &'a str,
    pub owner: &'a str,
}

pub struct NativeRequestInput<'a> {
    pub tick: &'a Value,
    pub table: &'a str,
    pub arrow_path: &'a str,
    pub arrow_sha256: &'a str,
    pub audit_sha256: &'a str,
    pub driver_sha256: &'a str,
    pub host: &'a str,
    pub port: i32,
    pub secret_ref: &'a str,
    pub generation: i64,
    pub batch_id: &'a str,
    pub target: NativeTarget<'a>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeDecision {
    Advance,
    MirrorPending,
    Replay,
    Refused,
}

pub fn native_request(input: &NativeRequestInput<'_>) -> Result<Value, &'static str> {
    if input.tick.get("protocol").and_then(Value::as_str) != Some(DORIS_PROTOCOL)
        || input.tick.get("database").and_then(Value::as_str) != Some(DORIS_DATABASE)
    {
        return Err("source_not_legal_window");
    }
    if input.tick.get("product_reader_write_attempts").and_then(Value::as_i64) != Some(0) {
        return Err("source_reader_write");
    }
    if input.tick.get("snapshot_token").and_then(Value::as_bool) != Some(false)
        || input.tick.get("binlog").and_then(Value::as_str) != Some("refused_unpublished")
    {
        return Err("source_not_legal_window");
    }
    let label = input.tick.get("label").and_then(Value::as_str).unwrap_or("");
    if !label_ok(label) {
        return Err("source_not_legal_window");
    }
    let columns = native_columns(input.table).ok_or("loader_columns_unregistered")?;
    let table = tick_table(input.tick, input.table)?;
    let (capture_method, delete_detection) = capture_pair(&table)?;
    if input.generation < 1 || input.driver_sha256 != READER_SHA256 || !ipv4_ok(input.host) {
        return Err("source_config_rejected");
    }
    if !batch_ok(input.batch_id) || !loader_ident(input.target.shard) || !uuid_ok(input.target.owner) {
        return Err("loader_shard_rejected");
    }
    if !file_path_ok(input.arrow_path)
        || !file_path_ok(input.target.password_file)
        || !file_path_ok(input.target.ca_file)
        || !hex64(input.arrow_sha256)
        || !hex64(input.audit_sha256)
    {
        return Err("digest_missing");
    }
    if input.secret_ref.contains("reader.password") {
        return Err("source_secret_rejected");
    }
    let parent = if input.generation == 1 {
        Value::Null
    } else {
        Value::from(input.generation - 1)
    };
    let identity = match table.get("key_model").and_then(Value::as_str).unwrap_or("") {
        "DUPLICATE" => "duplicate",
        "UNIQUE" => "unique",
        "AGGREGATE" => "aggregate",
        _ => return Err("source_not_legal_window"),
    };
    let consistency = table.get("consistency").and_then(Value::as_str).unwrap_or("");
    if !matches!(consistency, "T" | "S1" | "S0" | "B") {
        return Err("source_not_legal_window");
    }
    let config_digest = sha256_hex(
        format!(
            "doris\n{}\n{}\n{SOURCE_DB}\n{SOURCE_USER}\n{}\n{}\n",
            input.host, input.port, input.driver_sha256, input.secret_ref
        )
        .as_bytes(),
    );
    let schema_digest = sha256_hex(column_names(columns).as_bytes());
    let type_digest = sha256_hex(column_types(columns).as_bytes());
    let mut column_json = Vec::with_capacity(columns.len());
    for column in columns {
        let mut item = serde_json::Map::new();
        item.insert("name".to_string(), Value::String(column.name.to_string()));
        item.insert("logical_name".to_string(), Value::String(column.name.to_string()));
        item.insert("arrow_type".to_string(), Value::String(column.arrow_type.to_string()));
        item.insert("nullable".to_string(), Value::Bool(column.nullable));
        if let Some(precision) = column.precision {
            item.insert("precision".to_string(), Value::from(precision));
        }
        if let Some(scale) = column.scale {
            item.insert("scale".to_string(), Value::from(scale));
        }
        column_json.push(Value::Object(item));
    }
    Ok(serde_json::json!({
        "protocol": NATIVE_PROTOCOL,
        "source_engine": "doris",
        "source_id": SOURCE_ID,
        "source_generation": input.generation,
        "parent_source_generation": parent,
        "full_snapshot": true,
        "audit_digest": input.audit_sha256,
        "config_digest": config_digest,
        "schema_digest": schema_digest,
        "type_digest": type_digest,
        "input_digest": input.arrow_sha256,
        "capture_method": capture_method,
        "delete_detection": delete_detection,
        "consistency_grade": consistency,
        "cross_table_consistent": false,
        "position_codec": "snapshot-zero/v1",
        "position_lineage": "doris.snapshot.0",
        "applied_pos": "0",
        "batch_id": input.batch_id,
        "snapshot": {"kind": "capture-tick", "value": label},
        "logical_schema": DORIS_DATABASE,
        "logical_table": input.table,
        "row_identity": identity,
        "key_columns": key_columns(input.table),
        "columns": column_json,
        "arrow_path": input.arrow_path,
        "tombstones": [],
        "mirror": true,
        "target": target_value(&input.target)
    }))
}

pub fn install_request(target: &NativeTarget<'_>) -> Result<Value, &'static str> {
    if !loader_ident(target.shard) || !uuid_ok(target.owner) || !file_path_ok(target.password_file) || !file_path_ok(target.ca_file)
    {
        return Err("loader_shard_rejected");
    }
    Ok(serde_json::json!({
        "protocol": INSTALL_PROTOCOL,
        "target": target_value(target)
    }))
}

pub fn loader_import_allowed(elf_sha: &str) -> bool {
    REPAIRED_LOADER_SHA256.contains(&elf_sha)
}

pub fn producer_trust_authoritative(result: &Value) -> bool {
    result.pointer("/producer_trust/trusted").and_then(Value::as_bool) == Some(true)
        && result.pointer("/producer_trust/entry").and_then(Value::as_str) == Some(AUTHORITATIVE_TRUST_ENTRY)
}

/// The repaired loader locks `jobs.leases` and returns `call=locked`.
/// It does not call `jobs.claim` and it does not certify the fence.
/// A true certificate, a claim call, or a row this worker inserted is refused.
pub fn fence_held(result: &Value) -> bool {
    let Some(receipt) = result.get("jobs_lease_receipt").filter(|value| value.is_object()) else {
        return false;
    };
    let id = receipt.get("id").and_then(Value::as_str).unwrap_or("");
    let generation = receipt.get("generation").and_then(Value::as_i64).unwrap_or(0);
    let owner = receipt.get("owner").and_then(Value::as_str).unwrap_or("");
    let inserted = receipt.get("inserted").and_then(Value::as_bool).unwrap_or(true);
    let certified = receipt.get("target_fence_certified").and_then(Value::as_bool);
    let call = result.pointer("/jobs_observation/call").and_then(Value::as_str);
    let observed = result
        .pointer("/jobs_observation/target_fence_certified")
        .and_then(Value::as_bool);
    certified == Some(false)
        && observed == Some(false)
        && call == Some("locked")
        && !inserted
        && !owner.is_empty()
        && uuid_ok(id)
        && generation >= 1
        && (result.get("loader_invoked").and_then(Value::as_bool) == Some(true)
            || (result.get("load_state").and_then(Value::as_str) == Some("replay")
                && result.get("loader_invoked").and_then(Value::as_bool) == Some(false)))
        && result.get("source_bound").and_then(Value::as_bool) == Some(true)
}

/// Pass the task id already returned by `jobs.claim`.
/// This does not claim again and does not write `target_fence_certified`.
pub fn attach_observed_lease(request: &mut Value, task_id: &str, generation: i64) -> Result<(), &'static str> {
    if !uuid_ok(task_id) || generation < 1 {
        return Err("lease_required");
    }
    let object = request.as_object_mut().ok_or("lease_required")?;
    if object.contains_key("target_fence_certified") {
        return Err("fence");
    }
    object.insert("lease_id".to_string(), Value::String(task_id.to_string()));
    object.insert("lease_generation".to_string(), Value::from(generation));
    Ok(())
}

pub fn native_decision(result: &Value) -> NativeDecision {
    if !producer_trust_authoritative(result) || !fence_held(result) {
        return NativeDecision::Refused;
    }
    let state = result.get("load_state").and_then(Value::as_str).unwrap_or("");
    match state {
        "awaiting_publish" if result.get("mirror_committed").and_then(Value::as_bool) == Some(true) => {
            NativeDecision::Advance
        }
        "awaiting_publish" => NativeDecision::MirrorPending,
        "replay" if mirror_status_ok(result) => NativeDecision::Replay,
        _ => NativeDecision::Refused,
    }
}

pub fn unrepaired_loader_result() -> Value {
    serde_json::json!({
        "protocol": NATIVE_PROTOCOL,
        "load_state": "refused",
        "reason": "trust_unrepaired",
        "loader_invoked": false,
        "source_bound": false,
        "producer_trust": {
            "entry": "absent",
            "trusted": false
        },
        "jobs_lease_receipt": null,
        "jobs_observation": {
            "call": "not_invoked",
            "target_fence_certified": false,
            "lease_interface": "oo_pipeline::native_update::jobs_lease_interface",
            "loader_interface": "oo_pipeline::native_update::trusted_capture"
        },
        "watermark_state": null,
        "tombstone_count": null
    })
}

pub fn native_receipt_key(physical: &str, batch: &str) -> Result<String, &'static str> {
    if physical.len() == 63 && physical.starts_with("pt_") && batch_ok(batch) {
        Ok(format!("native/{physical}/{batch}"))
    } else {
        Err("physical")
    }
}

pub fn split_native_receipt(key: &str) -> Option<(&str, &str)> {
    let rest = key.strip_prefix("native/")?;
    let (physical, batch) = rest.rsplit_once('/')?;
    if physical.len() == 63 && physical.starts_with("pt_") && batch_ok(batch) {
        Some((physical, batch))
    } else {
        None
    }
}

fn target_value(target: &NativeTarget<'_>) -> Value {
    serde_json::json!({
        "host": target.host,
        "port": target.port,
        "database": target.database,
        "runtime_principal": target.runtime_principal,
        "password_file": target.password_file,
        "ssl": "verify-full",
        "ca_file": target.ca_file,
        "shard": target.shard,
        "owner": target.owner
    })
}

fn tick_table(tick: &Value, table: &str) -> Result<Value, &'static str> {
    let tables = tick.get("tables").and_then(Value::as_array).ok_or("source_not_legal_window")?;
    tables
        .iter()
        .find(|item| item.get("name").and_then(Value::as_str) == Some(table))
        .cloned()
        .ok_or("source_not_legal_window")
}

fn capture_pair(table: &Value) -> Result<(&'static str, &'static str), &'static str> {
    let route = table.get("route").and_then(Value::as_str).unwrap_or("");
    let delete_mode = table.get("delete_mode").and_then(Value::as_str).unwrap_or("");
    let quality = table.get("quality").and_then(Value::as_str).unwrap_or("");
    if route == "C5" && delete_mode == "reconcile" && quality == "DELETES_LAGGED" {
        return Ok(("c5_diff", "reconcile"));
    }
    if route == "C4" && delete_mode == "partition_replace" {
        return Ok(("c4_partition", "partition_replace"));
    }
    Err("source_not_legal_window")
}

fn key_columns(table: &str) -> Vec<&'static str> {
    match table {
        "str_key" => vec!["k"],
        "nz_obs" => vec!["id"],
        "part_day" => vec!["day", "region"],
        _ => Vec::new(),
    }
}

fn column_names(columns: &[NativeColumn]) -> String {
    columns.iter().map(|column| column.name).collect::<Vec<_>>().join("\n")
}

fn column_types(columns: &[NativeColumn]) -> String {
    columns
        .iter()
        .map(|column| {
            format!(
                "{} {} {} {:?} {:?}",
                column.name, column.arrow_type, column.nullable, column.precision, column.scale
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn mirror_status_ok(result: &Value) -> bool {
    matches!(
        result.get("mirror").and_then(Value::as_str),
        Some("inserted" | "replay")
    )
}

fn batch_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=128).contains(&bytes.len())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-'))
}

fn loader_ident(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn uuid_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

fn hex64(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 64 && bytes.iter().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn file_path_ok(value: &str) -> bool {
    value.starts_with('/')
        && !value.contains("..")
        && !value.contains('\0')
        && !value.contains('\n')
        && (2..=256).contains(&value.len())
}

pub struct ProducerBundle {
    pub receipt: Vec<u8>,
    pub registry: Vec<u8>,
    pub catalog: Vec<u8>,
}

pub fn arrow_schema_sha256(data: &[u8]) -> Result<String, &'static str> {
    if data.starts_with(b"ARROW1") {
        return Err("arrow_file_refused");
    }
    if data.len() < 8 {
        return Err("arrow_truncated");
    }
    let continuation = i32::from_le_bytes(data[0..4].try_into().unwrap());
    if continuation != -1 {
        return Err("arrow_truncated");
    }
    let meta_len = i32::from_le_bytes(data[4..8].try_into().unwrap());
    if meta_len <= 0 {
        return Err("arrow_truncated");
    }
    let meta_len = usize::try_from(meta_len).map_err(|_| "arrow_truncated")?;
    let meta_end = 8usize.checked_add(meta_len).ok_or("arrow_truncated")?;
    if meta_end > data.len() {
        return Err("arrow_truncated");
    }
    let meta = &data[8..meta_end];
    match flat_header_type(meta) {
        Some(1) => Ok(sha256_hex(meta)),
        Some(_) => Err("arrow_schema"),
        None => Err("arrow_truncated"),
    }
}

pub fn producer_bundle(request: &Value, tick: &Value, table_name: &str, arrow: &[u8]) -> Result<ProducerBundle, &'static str> {
    if arrow.is_empty() || arrow.len() > 16 * 1024 * 1024 {
        return Err("arrow_truncated");
    }
    let schema_hash = arrow_schema_sha256(arrow)?;
    let arrow_hash = sha256_hex(arrow);
    let input = digest_field(request, "input_digest")?;
    if input != arrow_hash {
        return Err("digest_mismatch");
    }
    let table = tick_table(tick, table_name)?;
    let catalog = canonical_line(&table);
    if names_secret(&catalog) {
        return Err("source_secret_rejected");
    }
    let registry_value = registry_value(request, table_name)?;
    let registry = canonical_line(&registry_value);
    if names_secret(&registry) {
        return Err("source_secret_rejected");
    }
    let finished = tick.get("finished_unix_ms").and_then(Value::as_i64).ok_or("source_not_legal_window")?;
    if finished < 0 {
        return Err("source_not_legal_window");
    }
    let label = request.pointer("/snapshot/value").and_then(Value::as_str).ok_or("cross_source")?;
    let run_id = format!("{SOURCE_ID}-{label}-{finished}");
    if run_id.len() > 128 || run_id.bytes().any(|byte| byte < 0x20) {
        return Err("source_not_legal_window");
    }
    let quality = table.get("quality").and_then(Value::as_str).ok_or("source_not_legal_window")?;
    let cross = tick
        .get("cross_table_consistency")
        .and_then(Value::as_str)
        .ok_or("source_not_legal_window")?;
    let receipt_value = serde_json::json!({
        "arrow_schema_sha256": schema_hash,
        "arrow_sha256": [arrow_hash],
        "audit_digest": digest_field(request, "audit_digest")?,
        "catalog_sha256": sha256_hex(&catalog),
        "config_digest": digest_field(request, "config_digest")?,
        "cross_table_consistency": cross,
        "delete_observation": "unknown",
        "driver_digest": READER_SHA256,
        "input_digest": input,
        "quality": quality,
        "registry_sha256": sha256_hex(&registry),
        "run_id": run_id,
        "schema": CAPTURE_RECEIPT_PROTOCOL,
        "schema_digest": digest_field(request, "schema_digest")?,
        "snapshot": {
            "kind": "capture-tick",
            "value": label
        },
        "source_engine": "doris",
        "source_generation": request.get("source_generation").and_then(Value::as_i64).ok_or("stale_run")?,
        "source_id": SOURCE_ID,
        "type_digest": digest_field(request, "type_digest")?
    });
    let receipt = canonical_line(&receipt_value);
    if names_secret(&receipt) {
        return Err("source_secret_rejected");
    }
    Ok(ProducerBundle {
        receipt,
        registry,
        catalog,
    })
}

pub fn attach_admission(request: &mut Value, path: &str, receipt_sha: &str) -> Result<(), &'static str> {
    if !file_path_ok(path) || !hex64(receipt_sha) {
        return Err("digest_missing");
    }
    for name in ["schema_digest", "type_digest", "config_digest", "input_digest", "audit_digest"] {
        if request.get(name).and_then(Value::as_str) == Some(receipt_sha) {
            return Err("digest_mismatch");
        }
    }
    let object = request.as_object_mut().ok_or("source_not_legal_window")?;
    if object.contains_key("admission_path") || object.contains_key("secret_ref") {
        return Err("source_secret_rejected");
    }
    object.insert("admission_path".to_string(), Value::String(path.to_string()));
    object.insert(
        "admission_expect".to_string(),
        serde_json::json!({ "declared_receipt_sha256": receipt_sha }),
    );
    let encoded = serde_json::to_vec(request).map_err(|_| "source_not_legal_window")?;
    if names_secret(&encoded) {
        return Err("source_secret_rejected");
    }
    Ok(())
}

fn digest_field(request: &Value, name: &str) -> Result<String, &'static str> {
    let value = request.get(name).and_then(Value::as_str).ok_or("digest_missing")?;
    if hex64(value) {
        Ok(value.to_string())
    } else {
        Err("digest_mismatch")
    }
}

fn registry_value(request: &Value, table_name: &str) -> Result<Value, &'static str> {
    let snapshot = request.get("snapshot").cloned().ok_or("cross_source")?;
    Ok(serde_json::json!({
        "schema": BINDING_REGISTRY_PROTOCOL,
        "entries": [{
            "protocol": CAPTURE_BINDING_PROTOCOL,
            "source_engine": "doris",
            "source_id": SOURCE_ID,
            "source_generation": request.get("source_generation").and_then(Value::as_i64).ok_or("stale_run")?,
            "snapshot": snapshot,
            "logical_schema": DORIS_DATABASE,
            "logical_table": table_name,
            "config_digest": digest_field(request, "config_digest")?,
            "schema_digest": digest_field(request, "schema_digest")?,
            "type_digest": digest_field(request, "type_digest")?,
            "input_digest": digest_field(request, "input_digest")?
        }]
    }))
}

fn names_secret(bytes: &[u8]) -> bool {
    bytes.windows(9).any(|window| window == b"secret://")
        || bytes.windows(12).any(|window| window == b"\"secret_ref\"")
        || bytes.windows(15).any(|window| window == b"reader.password")
}

fn canonical_line(value: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    write_canonical(&mut out, value);
    out.push(b'\n');
    out
}

fn write_canonical(out: &mut Vec<u8>, value: &Value) {
    match value {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(true) => out.extend_from_slice(b"true"),
        Value::Bool(false) => out.extend_from_slice(b"false"),
        Value::Number(number) => out.extend_from_slice(number.to_string().as_bytes()),
        Value::String(text) => push_json_string(out, text),
        Value::Array(items) => {
            out.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                write_canonical(out, item);
            }
            out.push(b']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push(b'{');
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                push_json_string(out, key);
                out.push(b':');
                write_canonical(out, &map[*key]);
            }
            out.push(b'}');
        }
    }
}

fn push_json_string(out: &mut Vec<u8>, text: &str) {
    out.push(b'"');
    for ch in text.chars() {
        match ch {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\r' => out.extend_from_slice(b"\\r"),
            '\t' => out.extend_from_slice(b"\\t"),
            other if (other as u32) < 0x20 => {
                let code = other as u32;
                out.extend_from_slice(format!("\\u{code:04x}").as_bytes());
            }
            other => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(other.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    out.push(b'"');
}

fn flat_header_type(meta: &[u8]) -> Option<u8> {
    if meta.len() < 8 {
        return None;
    }
    let root = u32::from_le_bytes(meta[0..4].try_into().ok()?) as usize;
    if root.checked_add(4)? > meta.len() {
        return None;
    }
    let soffset = i32::from_le_bytes(meta[root..root + 4].try_into().ok()?);
    let back = isize::try_from(soffset).ok()?;
    let vtable = root.checked_add_signed(back.checked_neg()?)?;
    if vtable + 2 > meta.len() {
        return None;
    }
    let vsize = u16::from_le_bytes(meta[vtable..vtable + 2].try_into().ok()?) as usize;
    let slot = 6usize;
    if slot + 2 > vsize || vtable + slot + 2 > meta.len() {
        return None;
    }
    let rel = u16::from_le_bytes(meta[vtable + slot..vtable + slot + 2].try_into().ok()?) as usize;
    if rel == 0 {
        return None;
    }
    let at = root.checked_add(rel)?;
    meta.get(at).copied()
}

pub fn split_receipt(key: &str) -> Option<(&str, &str)> {
    let rest = key.strip_prefix("src_doris/")?;
    let (physical, batch) = rest.rsplit_once('/')?;
    if physical.len() == 63 && batch.starts_with("pin") && !batch[3..].is_empty() {
        Some((physical, batch))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> SourceRow {
        SourceRow {
            driver_name: READER_NAME.to_string(),
            driver_commit: DORIS_COMMITS[0].to_string(),
            driver_sha256: READER_SHA256.to_string(),
            binary_rel: READER_REL.to_string(),
            secret_ref: "/run/oo-reader-secrets".to_string(),
            source_host: "172.16.43.10".to_string(),
            source_port: 9030,
            evidence_rel: "linux/run-source-1/tick-a2".to_string(),
            command: READER_COMMAND.to_string(),
        }
    }

    fn authoritative_loader(state: &str, mirror_committed: bool, mirror: &str) -> Value {
        serde_json::json!({
            "load_state": state,
            "mirror_committed": mirror_committed,
            "mirror": mirror,
            "loader_invoked": true,
            "source_bound": true,
            "producer_trust": {"entry": AUTHORITATIVE_TRUST_ENTRY, "trusted": true},
            "jobs_lease_receipt": {
                "id": "d101fd4a-3a89-41f6-b7fe-94b4001d5512",
                "generation": 2,
                "owner": "w-observe",
                "inserted": false,
                "target_fence_certified": false
            },
            "jobs_observation": {
                "call": "locked",
                "target_fence_certified": false
            }
        })
    }

    #[test]
    fn source_sql_matches_the_pinned_checksum() {
        assert!(sql_pinned());
    }

    #[test]
    fn replay_builds_no_argv_and_read_is_fixed() {
        assert_eq!(reader_action("replay"), ReaderAction::Skip);
        assert_eq!(reader_action("absent"), ReaderAction::Read);
        assert_eq!(reader_action("other"), ReaderAction::Refuse);
        let skipped = tick_argv(TickSpec {
            action: ReaderAction::Skip,
            binary: Path::new("/bin/doris-reader"),
            evidence: Path::new("/evidence/out"),
            secret_dir: "/run/oo-reader-secrets",
            host: "172.16.43.10",
            port: 9030,
            every_ms: 5000,
            label: "pin1",
        })
        .expect("skip");
        assert!(skipped.is_empty());
        let argv = tick_argv(TickSpec {
            action: ReaderAction::Read,
            binary: Path::new("/src/doris-reader"),
            evidence: Path::new("/evidence/linux/run-source-1/tick-a2"),
            secret_dir: "/run/oo-reader-secrets",
            host: "172.16.43.10",
            port: 9030,
            every_ms: 5000,
            label: "pin1",
        })
        .expect("read");
        assert_eq!(
            argv,
            vec![
                "/src/doris-reader".to_string(),
                "capture-tick".to_string(),
                "/evidence/linux/run-source-1/tick-a2".to_string(),
                "/run/oo-reader-secrets".to_string(),
                "172.16.43.10".to_string(),
                "9030".to_string(),
                "5000".to_string(),
                "pin1".to_string(),
            ]
        );
        assert!(tick_argv(TickSpec {
            action: ReaderAction::Read,
            binary: Path::new("/src/doris-reader"),
            evidence: Path::new("/evidence/out"),
            secret_dir: "/run/oo-reader-secrets",
            host: "172.16.43.10",
            port: 9030,
            every_ms: 5000,
            label: "pin1",
        })
        .unwrap()
        .iter()
        .all(|arg| arg != "dba-setup" && arg != "capture-dba" && !arg.contains("password")));
        assert!(tick_argv(TickSpec {
            action: ReaderAction::Read,
            binary: Path::new("/src/doris-reader"),
            evidence: Path::new("/evidence/out"),
            secret_dir: "/tmp/reader.password",
            host: "172.16.43.10",
            port: 9030,
            every_ms: 5000,
            label: "pin1",
        })
        .is_err());
        assert!(fixed_source(&row()).is_ok());
        let mut drifted = row();
        drifted.command = "capture-dba".to_string();
        assert!(fixed_source(&drifted).is_err());
        drifted = row();
        drifted.binary_rel = "tools/other".to_string();
        assert!(fixed_source(&drifted).is_err());
    }

    #[test]
    fn deletes_and_updates_are_loader_gaps() {
        let prior = [
            "1\u{1f}\\N".to_string(),
            "2\u{1f}\\N".to_string(),
            "3\u{1f}0.00".to_string(),
        ];
        let deleted = ["1\u{1f}\\N".to_string(), "2\u{1f}\\N".to_string()];
        assert_eq!(row_gap(&prior, &deleted), LoaderGap::Tombstone);
        let updated = [
            "1\u{1f}\\N".to_string(),
            "2\u{1f}1.00".to_string(),
            "3\u{1f}0.00".to_string(),
        ];
        assert_eq!(row_gap(&prior, &updated), LoaderGap::ColumnUpdate);
        assert_eq!(row_gap(&prior, &prior), LoaderGap::Snapshot);
        let body = br#"{
          "protocol":"oo.doris.capture-tick/v1",
          "database":"oo_dl01w1_43",
          "tables":[
            {"name":"part_day","route":"C4","delete_mode":"partition_replace","quality":"partition_metadata_recorded","rows":["2026-03-11\u001fwest\u001f33.25"]},
            {"name":"nz_obs","route":"C5","delete_mode":"reconcile","quality":"DELETES_LAGGED","rows":["1\u001f\\N","2\u001f\\N"]}
          ]
        }"#;
        assert_eq!(
            loader_gap(body, "part_day", None).expect("c4"),
            LoaderGap::PartitionReplace
        );
        assert_eq!(
            loader_gap(body, "nz_obs", Some(&prior)).expect("tomb"),
            LoaderGap::Tombstone
        );
        assert_eq!(loader_gap(body, "nz_obs", None).expect("first"), LoaderGap::Snapshot);
        assert!(loader_gap(br#"{"protocol":"other"}"#, "nz_obs", None).is_err());
        assert_eq!(reader_refusal("connect: timed out"), "source_endpoint_unavailable");
        assert_eq!(
            missing_parameter("source_endpoint_unavailable", "172.16.143.10", 9030),
            "fe_query_host_172.16.143.10_port_9030_db_oo_dl01w1_43_user_oo_reader"
        );
        assert_eq!(reader_refusal("secret: no such file"), "source_secret_rejected");
        assert_eq!(missing_parameter("source_secret_rejected", "172.16.143.10", 9030), "reader.password");
        let mut approved = row();
        approved.secret_ref = APPROVED_SECRET_REF.to_string();
        approved.source_host = "172.16.143.10".to_string();
        assert!(fixed_source(&approved).is_ok());
        assert_eq!(resolve_secret_dir(APPROVED_SECRET_REF).expect("ref"), APPROVED_SECRET_DIR);
        assert!(resolve_secret_dir("secret://other/reader").is_err());
        let tick = serde_json::json!({
            "protocol": DORIS_PROTOCOL,
            "database": DORIS_DATABASE,
            "product_reader_write_attempts": 0,
            "snapshot_token": false,
            "binlog": "refused_unpublished",
            "label": "pin1",
            "finished_unix_ms": 1791766363593i64,
            "cross_table_consistency": "B",
            "tables": [{
                "name": "str_key",
                "route": "C5",
                "delete_mode": "reconcile",
                "quality": "DELETES_LAGGED",
                "consistency": "S0",
                "key_model": "DUPLICATE"
            }]
        });
        let digest = "ab".repeat(32);
        let request = native_request(&NativeRequestInput {
            tick: &tick,
            table: "str_key",
            arrow_path: "/evidence/linux/run-source-2/tick-a/pin1-str_key.arrow",
            arrow_sha256: &digest,
            audit_sha256: &digest,
            driver_sha256: READER_SHA256,
            host: "172.16.143.10",
            port: 9030,
            secret_ref: APPROVED_SECRET_REF,
            generation: 1,
            batch_id: "pin1",
            target: NativeTarget {
                host: "pg",
                port: 5432,
                database: "oo_d101_target",
                runtime_principal: "oo_d101_pipe_login",
                password_file: "/secrets/pipe.pass",
                ca_file: "/secrets/ca.crt",
                shard: "src_ns2_a",
                owner: "d101fd4a-3a89-41f6-b7fe-94b4001d5512",
            },
        })
        .expect("request");
        assert!(request.get("admission_path").is_none());
        assert_eq!(request["snapshot"]["kind"], "capture-tick");
        assert_eq!(request["snapshot"]["value"], "pin1");
        assert_eq!(request["protocol"], NATIVE_PROTOCOL);
        assert_eq!(request["capture_method"], "c5_diff");
        assert_eq!(request["parent_source_generation"], Value::Null);
        assert!(request["tombstones"].as_array().expect("tombs").is_empty());
        assert_ne!(request["target"]["password_file"], "reader.password");
        assert!(!request.to_string().contains("reader.password"));
        assert_eq!(
            native_decision(&serde_json::json!({
                "load_state": "awaiting_publish",
                "mirror_committed": true,
                "source_bound": true,
                "producer_trust": {"entry": "capture_receipt_file", "trusted": true}
            })),
            NativeDecision::Refused
        );
        assert_eq!(
            native_decision(&serde_json::json!({"load_state":"awaiting_publish","mirror_committed":false})),
            NativeDecision::Refused
        );
        assert_eq!(
            native_decision(&serde_json::json!({"load_state":"replay","mirror":"replay"})),
            NativeDecision::Refused
        );
        assert_eq!(
            native_decision(&authoritative_loader("awaiting_publish", true, "inserted")),
            NativeDecision::Advance
        );
        assert_eq!(
            native_decision(&authoritative_loader("awaiting_publish", false, "absent")),
            NativeDecision::MirrorPending
        );
        assert_eq!(
            native_decision(&authoritative_loader("replay", true, "replay")),
            NativeDecision::Replay
        );
        // Actual native replay executes the pinned loader but skips COPY.
        let mut no_copy_replay = authoritative_loader("replay", true, "replay");
        no_copy_replay["loader_invoked"] = Value::Bool(false);
        assert_eq!(native_decision(&no_copy_replay), NativeDecision::Replay);
        no_copy_replay["load_state"] = Value::from("awaiting_publish");
        assert_eq!(native_decision(&no_copy_replay), NativeDecision::Refused);
        let mut forged = authoritative_loader("awaiting_publish", true, "inserted");
        forged["producer_trust"]["entry"] = Value::from("capture_receipt_file");
        assert_eq!(native_decision(&forged), NativeDecision::Refused);
        let mut bare = authoritative_loader("awaiting_publish", true, "inserted");
        bare["jobs_lease_receipt"] = Value::Null;
        assert_eq!(native_decision(&bare), NativeDecision::Refused);
        let mut forged_cert = authoritative_loader("awaiting_publish", true, "inserted");
        forged_cert["jobs_lease_receipt"]["target_fence_certified"] = Value::Bool(true);
        assert_eq!(native_decision(&forged_cert), NativeDecision::Refused);
        let mut claimed = authoritative_loader("awaiting_publish", true, "inserted");
        claimed["jobs_observation"]["call"] = Value::from("claimed");
        assert_eq!(native_decision(&claimed), NativeDecision::Refused);
        let mut inserted = authoritative_loader("awaiting_publish", true, "inserted");
        inserted["jobs_lease_receipt"]["inserted"] = Value::Bool(true);
        assert_eq!(native_decision(&inserted), NativeDecision::Refused);
        assert_eq!(
            native_decision(&unrepaired_loader_result()),
            NativeDecision::Refused
        );
        assert!(loader_import_allowed(NATIVE_ELF_SHA256));
        assert!(!loader_import_allowed(UNREPAIRED_ELF_SHA256));
        assert_eq!(REPAIRED_LOADER_SHA256, &[NATIVE_ELF_SHA256]);
        let mut request = serde_json::json!({"protocol": NATIVE_PROTOCOL});
        attach_observed_lease(&mut request, "d101fd4a-3a89-41f6-b7fe-94b4001d5512", 2).expect("lease");
        assert_eq!(request["lease_id"], "d101fd4a-3a89-41f6-b7fe-94b4001d5512");
        assert_eq!(request["lease_generation"], 2);
        assert!(request.get("target_fence_certified").is_none());
        assert!(attach_observed_lease(&mut request, "not-a-uuid", 2).is_err());
        assert_eq!(
            native_decision(&serde_json::json!({"load_state":"not_sealed","reason":"bad_type"})),
            NativeDecision::Refused
        );
        assert!(password_file_mode_ok(0o600));
        assert!(!password_file_mode_ok(0o640));
        let receipt = format!("src_doris/{}/pin2", "pt_".to_string() + &"ab".repeat(30));
        let (physical, batch) = split_receipt(&receipt).expect("receipt");
        assert_eq!(batch, "pin2");
        assert_eq!(physical.len(), 63);
        assert_eq!(scrub_secret("saw SECRETVALUE here", "SECRETVALUE"), "saw [redacted] here");
    }

    #[test]
    fn producer_receipt_uses_the_arrow_schema_message_and_hides_the_secret() {
        let arrow = include_bytes!(
            "../../../evidence/d101fd4a-3a89-41f6-b7fe-94b4001d5512/DL01/capture-jobs-schedule-lease/linux/run-source-2/tick-a/pin1-str_key.arrow"
        );
        let schema = arrow_schema_sha256(arrow).expect("schema");
        assert_eq!(schema, "a918657aae66d6bbbf6c746de67fa67e3354fd0cc35815de0a21a43eaf8df999");
        assert!(arrow_schema_sha256(b"ARROW1").is_err());
        assert!(arrow_schema_sha256(&[0, 0, 0, 0, 1, 0, 0, 0]).is_err());
        let digest = sha256_hex(arrow);
        let tick = serde_json::json!({
            "protocol": DORIS_PROTOCOL,
            "database": DORIS_DATABASE,
            "product_reader_write_attempts": 0,
            "snapshot_token": false,
            "binlog": "refused_unpublished",
            "label": "pin1",
            "finished_unix_ms": 1791766363593i64,
            "cross_table_consistency": "B",
            "tables": [{
                "name": "str_key",
                "route": "C5",
                "delete_mode": "reconcile",
                "quality": "DELETES_LAGGED",
                "consistency": "S0",
                "key_model": "DUPLICATE",
                "rows": ["k"]
            }]
        });
        let mut request = native_request(&NativeRequestInput {
            tick: &tick,
            table: "str_key",
            arrow_path: "/evidence/linux/run-source-2/tick-a/pin1-str_key.arrow",
            arrow_sha256: &digest,
            audit_sha256: &digest,
            driver_sha256: READER_SHA256,
            host: "172.16.143.10",
            port: 9030,
            secret_ref: APPROVED_SECRET_REF,
            generation: 1,
            batch_id: "pin1",
            target: NativeTarget {
                host: "pg",
                port: 5432,
                database: "oo_d101_target",
                runtime_principal: "oo_d101_pipe_login",
                password_file: "/secrets/pipe.pass",
                ca_file: "/secrets/ca.crt",
                shard: "src_ns2_a",
                owner: "d101fd4a-3a89-41f6-b7fe-94b4001d5512",
            },
        })
        .expect("request");
        let bundle = producer_bundle(&request, &tick, "str_key", arrow).expect("bundle");
        let receipt_sha = sha256_hex(&bundle.receipt);
        assert_ne!(receipt_sha, request["schema_digest"].as_str().unwrap());
        assert!(!names_secret(&bundle.receipt));
        assert!(!names_secret(&bundle.registry));
        assert!(bundle.receipt.ends_with(b"\n"));
        attach_admission(
            &mut request,
            "/evidence/linux/run-source-3/tick-a/producer-receipt.json",
            &receipt_sha,
        )
        .expect("attach");
        assert_eq!(request["admission_expect"]["declared_receipt_sha256"], receipt_sha);
        assert!(!request.to_string().contains("secret://"));
        assert!(!request.to_string().contains("secret_ref"));
    }
}
