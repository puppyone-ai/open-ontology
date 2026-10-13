//! MongoDB controlled preview.
//!
//! `compat_mode` stays `mongodb-native` and the granted role stays `read`.
//! `source_admission::admit` is the only admission decision. A denial does not
//! call pg-builder. An admission writes the native manifest and calls the
//! pinned `pg-builder import` and `pg-builder import-cards` commands. Those
//! commands refuse this engine. This crate does not relabel the source as
//! MySQL or PostgreSQL, does not load `oo.source.pgsql.budget/v1`, and does
//! not call `compile_sealed_import_catalog` without a sealed catalog.

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub mod arrow_utf8;
pub mod bindpack;
pub mod capture;
pub mod docseal;
use source_admission::{
    admit, Budget, CaptureMethod, CostModel, DebeziumOptions, Decision, DeclaredCapability, Driver,
    Permission, SourceConfig, TargetReport, WriteOptions, CONFIG_PROTOCOL, REPORT_PROTOCOL,
};

pub const OBSERVED_COMPAT: &str = "mongodb-native";
pub const ENGINE: &str = "mongodb";
pub const TARGET_ID: &str = "mongodb-8.0.32-community-standalone";
pub const DRIVER_NAME: &str = "mongodb";
pub const DRIVER_VERSION: &str = "3.3.0";
pub const FROZEN_DRIVER_DIGEST: &str =
    "622f272c59e54a3c85f5902c6b8e7b1653a6b6681f45e4c42d6581301119a4b8";
pub const FROZEN_ARROW_SHA256: &str =
    "4764e423c574ab54043873d93250596c290ed08ebb93048bee1e9cc64c4c96c8";
pub const FROZEN_ARROW_BYTES: u64 = 9506;
pub const MANIFEST_PROTOCOL: &str = "oo.mongodb-preview.manifest/v1";
pub const IMPORT_REQUEST_PROTOCOL: &str = "oo.pg-builder.import-request/v1";
pub const SEALED_CATALOG_PROTOCOL: &str = "oo.pipeline.sealed-import-catalog/v1";
pub const SHARED_PATCH: &str = "\
pg-builder import calls run_import. tools/pg-builder/src/import.rs:48 refuses a manifest whose protocol is not oo.mysql-preview.manifest/v1 before spawn and before reading a password. \
The command is: pg-builder import --request REQUEST --out OUT --stderr STDERR. \
This source sends protocol oo.mongodb-preview.manifest/v1 with engine mongodb. \
pg-builder import-cards calls import_cards. tools/pg-builder/src/import.rs:139 refuses engine other than mysql, and line 192 would write engine mysql. \
The command is: pg-builder import-cards --catalog CATALOG --out OUT. \
The child of a mysql manifest is: oo-pipeline catalog-import, stdin protocol oo.pipeline.catalog-import/v1. \
crates/oo-pipeline/src/catalog_import.rs:34 sets MANIFEST_PROTOCOL to oo.mysql-preview.manifest/v1. \
parse_request at line 185 returns source engine unless manifest.engine is mysql. \
The result body at line 367 sets engine to the literal mysql. \
Minimal patch for the MySQL06 owner of pipeline and pg-builder: accept oo.mongodb-preview.manifest/v1 with the existing Manifest fields, require engine mongodb, and copy that engine into the sealed catalog and the import cards. \
Do not retag the source as mysql or postgres-native. Do not load oo.source.pgsql.budget/v1. \
Target oid and typmod stay unset here because the shared loader did not map BSON types.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomerInput {
    pub source_id: String,
    pub secret_ref: String,
    pub driver_digest: String,
    pub permission: String,
    pub budget_unit: String,
    pub budget_limit: u64,
    pub namespace: String,
    pub approved_objects: Vec<String>,
    pub approved_fields: Vec<String>,
    pub role: String,
    pub batch_object: String,
    pub deny_object: String,
    pub deny_fields: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeOutput {
    pub import_exit: i32,
    pub import_stderr: String,
    pub cards_exit: i32,
    pub cards_stderr: String,
}

pub fn validate(input: &CustomerInput) -> Result<(), &'static str> {
    if !input.secret_ref.starts_with("secret://")
        || input.secret_ref.len() < 10
        || input.secret_ref.len() > 200
        || input.secret_ref.contains("PASSWORD")
        || input.secret_ref.contains("mongodb://")
        || input.secret_ref.contains('@')
        || input.secret_ref.chars().any(char::is_whitespace)
    {
        return Err("secret_ref");
    }
    if input.driver_digest.len() != 64
        || !input
            .driver_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("driver_digest");
    }
    if input.permission.is_empty()
        || input.permission.len() > 80
        || input.permission.contains(':')
        || input.permission.contains('/')
        || input.permission.chars().any(char::is_whitespace)
    {
        return Err("permission");
    }
    if input.budget_unit.is_empty() || input.budget_unit.chars().any(char::is_whitespace) {
        return Err("budget_unit");
    }
    if input.source_id.is_empty() || input.source_id.len() > 63 {
        return Err("source_id");
    }
    if input.role != "read" {
        return Err("role");
    }
    if input.namespace != input.source_id || !ident_ok(&input.namespace) {
        return Err("namespace");
    }
    if input.approved_objects.is_empty()
        || input.approved_fields.is_empty()
        || !input.approved_objects.iter().any(|object| object == &input.batch_object)
    {
        return Err("allowlist");
    }
    if input.approved_objects.iter().any(|object| !object_ok(&input.namespace, object))
        || !object_ok(&input.namespace, &input.batch_object)
    {
        return Err("allowlist");
    }
    if input.approved_fields.iter().any(|field| !ident_ok(field)) || input.deny_fields.iter().any(|field| !ident_ok(field)) {
        return Err("field");
    }
    if authorize_read(input, &input.deny_object, &input.deny_fields).is_ok() {
        return Err("deny_object");
    }
    Ok(())
}

pub fn authorize_read(input: &CustomerInput, object: &str, fields: &[String]) -> Result<(), &'static str> {
    if !input.approved_objects.iter().any(|item| item == object) || !object_ok(&input.namespace, object) {
        return Err("unapproved_object");
    }
    if fields.is_empty() || fields.iter().any(|field| !input.approved_fields.iter().any(|item| item == field)) {
        return Err("unapproved_field");
    }
    Ok(())
}

pub fn customer_from_json(text: &str) -> Result<CustomerInput, &'static str> {
    let value: Value = serde_json::from_str(text).map_err(|_| "customer")?;
    let input = CustomerInput {
        source_id: json_string(&value, "source_id")?,
        secret_ref: json_string(&value, "secret_ref")?,
        driver_digest: json_string(&value, "driver_digest")?,
        permission: json_string(&value, "permission")?,
        budget_unit: json_string(&value, "budget_unit")?,
        budget_limit: value.get("budget_limit").and_then(Value::as_u64).ok_or("budget_limit")?,
        namespace: json_string(&value, "namespace")?,
        approved_objects: json_strings(&value, "approved_objects")?,
        approved_fields: json_strings(&value, "approved_fields")?,
        role: json_string(&value, "role")?,
        batch_object: json_string(&value, "batch_object")?,
        deny_object: json_string(&value, "deny_object")?,
        deny_fields: json_strings(&value, "deny_fields")?,
    };
    validate(&input)?;
    Ok(input)
}

fn json_string(value: &Value, key: &str) -> Result<String, &'static str> {
    value.get(key).and_then(Value::as_str).map(str::to_string).ok_or("customer")
}

fn json_strings(value: &Value, key: &str) -> Result<Vec<String>, &'static str> {
    let items = value.get(key).and_then(Value::as_array).ok_or("customer")?;
    let mut names = Vec::new();
    for item in items {
        names.push(item.as_str().ok_or("customer")?.to_string());
    }
    if names.is_empty() {
        return Err("customer");
    }
    Ok(names)
}

fn ident_ok(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    value.len() <= 63 && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn object_ok(namespace: &str, object: &str) -> bool {
    let Some((owner, name)) = object.split_once('.') else {
        return false;
    };
    owner == namespace && ident_ok(name) && !name.contains('.')
}

pub fn frozen_driver(input: &CustomerInput) -> bool {
    input.driver_digest == FROZEN_DRIVER_DIGEST
}

pub fn inputs(input: &CustomerInput) -> Result<(TargetReport, SourceConfig), &'static str> {
    validate(input)?;
    let permissions = vec![Permission {
        name: input.permission.clone(),
        status: "known".to_string(),
        granted: true,
        scope: None,
    }];
    let report = TargetReport {
        protocol: REPORT_PROTOCOL.to_string(),
        target_id: TARGET_ID.to_string(),
        source_id: input.source_id.clone(),
        secret_ref: input.secret_ref.clone(),
        driver: Driver {
            kind: "native-rust".to_string(),
            name: DRIVER_NAME.to_string(),
            version: DRIVER_VERSION.to_string(),
            digest: input.driver_digest.clone(),
        },
        compat_mode: OBSERVED_COMPAT.to_string(),
        license_flags: vec!["Apache-2.0".to_string()],
        permissions: permissions.clone(),
        capabilities: vec![
            capability("Catalog", None),
            capability("BatchFull", None),
            capability("Budget", Some(input.budget_unit.clone())),
        ],
        capture_methods: vec![CaptureMethod {
            method: "C5".to_string(),
            write_class: "R0".to_string(),
            consistency: "S0".to_string(),
            token_order: "None".to_string(),
            retention_window: None,
            cursor: None,
            feed: None,
            cdc: None,
            delete_detection: Some("reconcile".to_string()),
        }],
        cost_model: CostModel {
            unit: input.budget_unit.clone(),
            confidence: "unknown".to_string(),
            estimate: "not_collected".to_string(),
        },
        r1w: None,
        native: None,
        capture_witness: None,
    };
    let config = SourceConfig {
        protocol: CONFIG_PROTOCOL.to_string(),
        target_id: report.target_id.clone(),
        source_id: report.source_id.clone(),
        secret_ref: report.secret_ref.clone(),
        compat_mode: OBSERVED_COMPAT.to_string(),
        permissions,
        budget: Budget {
            unit: input.budget_unit.clone(),
            hard_limit: input.budget_limit,
            confidence: "unknown".to_string(),
        },
        write_options: WriteOptions {
            signal_table: false,
            heartbeat_write: false,
            create_stream: false,
            create_trigger: false,
        },
        debezium: DebeziumOptions {
            signal_data_collection: None,
            heartbeat_action_query: None,
            snapshot_mode: "never".to_string(),
            create_stream: false,
            create_trigger: false,
        },
        requested_write_class: "R0".to_string(),
        r1w: None,
        native: None,
    };
    Ok((report, config))
}

pub fn decide(input: &CustomerInput) -> Result<Decision, &'static str> {
    let (report, config) = inputs(input)?;
    Ok(admit(&report, &config))
}

pub fn import_started(_decision: &Decision) -> bool {
    false
}

pub fn arrow_matches(bytes: &[u8]) -> bool {
    bytes.len() as u64 == FROZEN_ARROW_BYTES && sha256_hex(bytes) == FROZEN_ARROW_SHA256
}

pub fn write_probe_inputs(
    work: &Path,
    input: &CustomerInput,
    census: &Value,
    arrow_sha256: &str,
) -> Result<(), &'static str> {
    if census.get("sample_not_enforced_schema").and_then(Value::as_bool) != Some(true) {
        return Err("census");
    }
    if census.get("documents").and_then(Value::as_u64) != Some(input.budget_limit) {
        return Err("census");
    }
    let logical_table = input
        .batch_object
        .split_once('.')
        .map(|(_, name)| name)
        .ok_or("allowlist")?;
    let fields = census
        .get("fields")
        .and_then(Value::as_array)
        .ok_or("census")?;
    if fields.is_empty() {
        return Err("census");
    }
    let mut columns = Vec::new();
    for field in fields {
        let name = field.get("field").and_then(Value::as_str).ok_or("census")?;
        let types = field
            .get("bson_types")
            .and_then(Value::as_array)
            .ok_or("census")?;
        let names = types
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        if names.len() != types.len() || names.is_empty() {
            return Err("census");
        }
        let mixed = names.len() != 1;
        let joined = names.join("+");
        columns.push(json!({
            "logical_name": name,
            "source_type": if mixed { Value::Null } else { json!(names[0]) },
            "caveat": if mixed { json!(format!("mixed:{joined}")) } else { Value::Null },
            "target_oid": Value::Null,
            "target_typmod": Value::Null
        }));
    }
    let manifest = json!({
        "protocol": MANIFEST_PROTOCOL,
        "engine": ENGINE,
        "source_id": input.source_id,
        "config_digest": sha256_hex(format!("config\n{source_id}\n{OBSERVED_COMPAT}\n", source_id = input.source_id).as_bytes()),
        "source_generation": 1,
        "target_generation": 1,
        "run_id": "00000000-0000-4000-8000-000000000041",
        "schema_digest": sha256_hex(format!("schema\n{arrow_sha256}\n").as_bytes()),
        "type_digest": sha256_hex(b"type\nmongodb-bson-census\n"),
        "input_digest": arrow_sha256,
        "hook_digest": sha256_hex(b"hook\nnone\n"),
        "payload_includes_reject_lines": false,
        "accepted_rows": input.budget_limit,
        "logical_schema": input.source_id,
        "logical_table": logical_table,
        "columns": columns,
        "sample_not_enforced_schema": true
    });
    let manifest_path = work.join("manifest.json");
    let request = json!({
        "protocol": IMPORT_REQUEST_PROTOCOL,
        "catalog_bin": work.join("not-a-pipeline"),
        "source_id": input.source_id,
        "config_digest": manifest.get("config_digest").and_then(Value::as_str).unwrap_or(""),
        "source_generation": 1,
        "manifest_file": manifest_path,
        "target": {
            "host": "127.0.0.1",
            "port": 1,
            "database": "oo_w141_target",
            "runtime_principal": "oo_w141_runtime",
            "password_file": work.join("password-not-read"),
            "ca_file": work.join("ca-not-read")
        }
    });
    let cards = json!({
        "protocol": SEALED_CATALOG_PROTOCOL,
        "engine": ENGINE,
        "source_id": input.source_id,
        "capture": "sealed_import"
    });
    fs::create_dir_all(work).map_err(|_| "work")?;
    fs::write(work.join("not-a-pipeline"), b"not an oo-pipeline binary\n").map_err(|_| "work")?;
    write_json(&manifest_path, &manifest)?;
    write_json(&work.join("import-request.json"), &request)?;
    write_json(&work.join("cards-catalog.json"), &cards)?;
    Ok(())
}

pub fn run_shared_probe(pg_builder: &Path, work: &Path) -> Result<ProbeOutput, &'static str> {
    let (import_args, cards_args) = probe_argv(work);
    let import_refs = import_args.iter().map(String::as_str).collect::<Vec<_>>();
    let cards_refs = cards_args.iter().map(String::as_str).collect::<Vec<_>>();
    let import = command_output(pg_builder, &import_refs)?;
    let cards = command_output(pg_builder, &cards_refs)?;
    Ok(ProbeOutput {
        import_exit: import.0,
        import_stderr: import.1,
        cards_exit: cards.0,
        cards_stderr: cards.1,
    })
}

pub fn shared_contract_refused(probe: &ProbeOutput) -> bool {
    probe.import_exit == 1
        && probe
            .import_stderr
            .contains("the manifest protocol is not the sealed import manifest")
        && probe.cards_exit == 1
        && probe
            .cards_stderr
            .contains("the catalog source engine is not mysql")
}

pub fn gap_document(input: &CustomerInput, decision: &Decision, probe: Option<&ProbeOutput>) -> Value {
    let classification = match probe {
        None => "not_run",
        Some(probe) if shared_contract_refused(probe) => "shared_contract",
        Some(_) => "unexpected",
    };
    json!({
        "admitted": decision.admitted,
        "blocks": decision.blocks,
        "gap": SHARED_PATCH,
        "compat_observed": OBSERVED_COMPAT,
        "permission_observed": input.permission,
        "namespace": input.namespace,
        "role": input.role,
        "approved_objects": input.approved_objects,
        "batch_object": input.batch_object,
        "find_sent": false,
        "engine": ENGINE,
        "driver_digest_frozen": frozen_driver(input),
        "target_storage_label_does_not_replace_engine": "postgresql_target_storage",
        "import_started": false,
        "builder_started": false,
        "operator_preview_started": false,
        "pg_budget_protocol_used": false,
        "sealed_import_called": false,
        "role_pack_called": false,
        "model_called": false,
        "change_stream_declared": false,
        "change_stream_excluded": true,
        "sspl_redistribution_clearance": false,
        "tls": false,
        "enterprise_audit": false,
        "independent_approval": false,
        "source_acceptance": false,
        "matrix_mutated": decision.matrix_mutated,
        "zero_write_proven": decision.zero_write.proven,
        "shared_probe": classification,
        "import_exit": probe.map(|item| item.import_exit),
        "cards_exit": probe.map(|item| item.cards_exit),
        "import_stderr": probe.map(|item| truncate_redacted(&item.import_stderr)),
        "cards_stderr": probe.map(|item| truncate_redacted(&item.cards_stderr)),
        "postgres_started": false,
        "manifest_protocol": MANIFEST_PROTOCOL
    })
}

pub fn fixture_input() -> CustomerInput {
    CustomerInput {
        source_id: "oo_w141".to_string(),
        secret_ref: "secret://oo-dl01w1-41/reader".to_string(),
        driver_digest: FROZEN_DRIVER_DIGEST.to_string(),
        permission: "read".to_string(),
        budget_unit: "Rows".to_string(),
        budget_limit: 16,
        namespace: "oo_w141".to_string(),
        approved_objects: vec!["oo_w141.shipments".to_string()],
        approved_fields: frozen_columns().iter().map(|column| column.name.to_string()).collect(),
        role: "read".to_string(),
        batch_object: "oo_w141.shipments".to_string(),
        deny_object: "oo_w141.not_approved".to_string(),
        deny_fields: vec!["notApproved".to_string()],
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn capability(name: &str, budget_unit: Option<String>) -> DeclaredCapability {
    DeclaredCapability {
        name: name.to_string(),
        cursor: None,
        feed: None,
        cdc: None,
        token_order: None,
        budget_unit,
        level: "designed".to_string(),
        write_class: "R0".to_string(),
        consistency: "S0".to_string(),
    }
}

fn write_json(path: &Path, value: &Value) -> Result<(), &'static str> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|_| "json")?;
    fs::write(path, bytes).map_err(|_| "work")
}

fn command_output(program: &Path, args: &[&str]) -> Result<(i32, String), &'static str> {
    let output = Command::new(program)
        .args(args)
        .env_clear()
        .output()
        .map_err(|_| "probe")?;
    let mut stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if stderr.len() > 4000 {
        stderr.truncate(4000);
    }
    let code = output.status.code().ok_or("probe")?;
    Ok((code, stderr))
}

fn truncate_redacted(text: &str) -> String {
    if text.contains("mongodb://") || text.contains("CANARY-SECRET") {
        return "redacted".to_string();
    }
    let mut owned = text.to_string();
    if owned.len() > 500 {
        owned.truncate(500);
    }
    owned
}

pub fn probe_argv(work: &Path) -> (Vec<String>, Vec<String>) {
    let import = vec![
        "import".to_string(),
        "--request".to_string(),
        work.join("import-request.json").display().to_string(),
        "--out".to_string(),
        work.join("import-out.json").display().to_string(),
        "--stderr".to_string(),
        work.join("import-tool.stderr").display().to_string(),
    ];
    let cards = vec![
        "import-cards".to_string(),
        "--catalog".to_string(),
        work.join("cards-catalog.json").display().to_string(),
        "--out".to_string(),
        work.join("cards-out.json").display().to_string(),
    ];
    (import, cards)
}

pub fn work_has_password_file(work: &Path) -> bool {
    work.join("password-not-read").is_file()
}

pub fn manifest_engine(work: &Path) -> Result<String, &'static str> {
    let bytes = fs::read(work.join("manifest.json")).map_err(|_| "work")?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "json")?;
    value
        .get("engine")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or("json")
}

/// Physical import protocol accepted by `pg-builder import-native`.
pub const NATIVE_IMPORT_PROTOCOL: &str = "oo.pipeline.native-arrow-import/v1";
/// Operator registry protocol. It is not a client field on the manifest.
pub const BINDING_REGISTRY_PROTOCOL: &str = "oo.source-admission.binding-registry/v1";
/// One capture binding inside that registry.
pub const CAPTURE_BINDING_PROTOCOL: &str = "oo.source-admission.capture-binding/v1";

/// Keys a public manifest must not carry. The operator environment is the authority.
pub const CLIENT_AUTHORITY_KEYS: [&str; 5] = [
    "admitted",
    "admission_bin",
    "registry_path",
    "registry_sha256",
    "approval",
];

/// One frozen Arrow column, in stream order, using the capture decoder's type token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrozenColumn {
    /// Arrow field name.
    pub name: &'static str,
    /// Type token from the owned decoder of the frozen stream.
    pub arrow_type: &'static str,
    /// Flatbuffer nullable bit.
    pub nullable: bool,
    /// Decimal precision when the token has one.
    pub precision: Option<i32>,
    /// Decimal scale when the token has one.
    pub scale: Option<i32>,
}

/// The 16-shipment stream. Types stay the capture tokens, including millisecond timestamps,
/// fixed-width binaries, uint8 presence, the dense union, list of utf8, and the pack struct.
pub fn frozen_columns() -> &'static [FrozenColumn] {
    const COLUMNS: &[FrozenColumn] = &[
        FrozenColumn { name: "_id", arrow_type: "fixedsizebinary(12)", nullable: false, precision: None, scale: None },
        FrozenColumn { name: "code", arrow_type: "utf8", nullable: false, precision: None, scale: None },
        FrozenColumn { name: "flag", arrow_type: "int32", nullable: false, precision: None, scale: None },
        FrozenColumn { name: "units", arrow_type: "int64", nullable: false, precision: None, scale: None },
        FrozenColumn { name: "price", arrow_type: "decimal128(34,7)", nullable: false, precision: Some(34), scale: Some(7) },
        FrozenColumn { name: "created", arrow_type: "timestamp(ms,utc)", nullable: false, precision: None, scale: None },
        FrozenColumn { name: "uid", arrow_type: "fixedsizebinary(16)", nullable: false, precision: None, scale: None },
        FrozenColumn { name: "blob", arrow_type: "binary", nullable: false, precision: None, scale: None },
        FrozenColumn { name: "blob_presence", arrow_type: "uint8", nullable: false, precision: None, scale: None },
        FrozenColumn { name: "weight", arrow_type: "float64", nullable: false, precision: None, scale: None },
        FrozenColumn { name: "warehouse", arrow_type: "utf8", nullable: false, precision: None, scale: None },
        FrozenColumn { name: "whCode", arrow_type: "utf8", nullable: false, precision: None, scale: None },
        FrozenColumn { name: "lane", arrow_type: "union(dense,0:utf8,1:int64)", nullable: false, precision: None, scale: None },
        FrozenColumn { name: "memo", arrow_type: "utf8", nullable: true, precision: None, scale: None },
        FrozenColumn { name: "memo_presence", arrow_type: "uint8", nullable: false, precision: None, scale: None },
        FrozenColumn { name: "marks", arrow_type: "list(utf8)", nullable: true, precision: None, scale: None },
        FrozenColumn { name: "marks_presence", arrow_type: "uint8", nullable: false, precision: None, scale: None },
        FrozenColumn { name: "pack", arrow_type: "struct(kind:utf8,n:int32)", nullable: true, precision: None, scale: None },
        FrozenColumn { name: "pack_presence", arrow_type: "uint8", nullable: false, precision: None, scale: None },
    ];
    COLUMNS
}

/// Import request for `pg-builder import-native`. No client authority keys.
pub fn native_import_document(
    config_digest: &str,
    schema_digest: &str,
    type_digest: &str,
    input_digest: &str,
    arrow_path: &str,
    batch_sha256: &str,
) -> Value {
    let columns: Vec<Value> = frozen_columns()
        .iter()
        .map(|column| {
            json!({
                "name": column.name,
                "logical_name": column.name,
                "arrow_type": column.arrow_type,
                "nullable": column.nullable,
                "precision": column.precision,
                "scale": column.scale,
            })
        })
        .collect();
    json!({
        "protocol": NATIVE_IMPORT_PROTOCOL,
        "source_engine": ENGINE,
        "source_id": "oo_w141",
        "source_generation": 1,
        "snapshot": {"kind": "token", "value": "oo-dl01w1-41-frozen-batch"},
        "target_storage": "postgresql",
        "catalog_identity": {
            "config_digest": config_digest,
            "schema_digest": schema_digest,
            "type_digest": type_digest,
            "input_digest": input_digest,
        },
        "tables": [{
            "logical_schema": "oo_w141",
            "logical_table": "shipments",
            "row_count": 16,
            "batch_sha256": batch_sha256,
            "arrow_path": arrow_path,
            "columns": columns,
        }],
        "target": {
            "host": "oo-dl01w1-41-pg",
            "port": 5432,
            "database": "oo_w141",
            "runtime_principal": "oo_w141_runtime",
            "password_file": "/evidence/plan/password-not-read",
            "ca_file": "/evidence/plan/ca-not-read",
            "owner": "23df6145-4592-4dc0-82df-fd226dd020fd",
            "shard": "oo_w141",
        }
    })
}

/// True when a JSON object, or its admission object, carries a client authority key.
pub fn carries_client_authority(document: &Value) -> bool {
    let present = |value: &Value| CLIENT_AUTHORITY_KEYS.iter().any(|key| value.get(*key).is_some());
    present(document) || document.get("admission").is_some_and(present)
}
