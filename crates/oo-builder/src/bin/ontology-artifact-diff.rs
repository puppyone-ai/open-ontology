//! Structural diff of two captured PostgreSQL draft artifacts.
//!
//! The inputs are `oo.builder.captured-pg-draft-command/v1` documents. The
//! diff reads identities and review values those documents already carry.
//! It does not connect, call a model, import rows, accept, grant, certify,
//! or publish.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs::File;
use std::io::{Read, Write};
use std::process::ExitCode;

use oo_core::api::payload_sha256;
use oo_core::limits::DOCUMENT_MAX_BYTES;
use oo_core::parse_json_value;
use serde_json::{Map, Value};

const PROTOCOL: &str = "oo.builder.artifact-diff/v1";
const COMPILER: &str = "ontology-artifact-diff/v1";
const COMMAND_PROTOCOL: &str = "oo.builder.captured-pg-draft-command/v1";
const CAPTURED_PROTOCOL: &str = "oo.builder.captured-pg-draft/v1";
const CAPTURED_COMPILER: &str = "oo-builder/captured-pg-draft/v1";
const DRAFT_PROTOCOL: &str = "oo.builder.structure-draft/v1";
const DRAFT_COMPILER: &str = "oo-builder/structure-draft/v1";
const EVIDENCE_PROTOCOL: &str = "oo.builder.catalog-evidence/v1";
const PG_PROTOCOL: &str = "oo.source.pgsql/v1";
const HISTORICAL: &str = "b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74";
const CONFIG_VERSION: &str = "oo.config/v1";
const HASH_ALGORITHM: &str = "pg-catalog-hash/v1";
const IDENT_ALGORITHM: &str = "pgident/v1";
const OOS_VERSION: &str = "oos/v1";

#[derive(Clone, Copy)]
struct Fail {
    code: &'static str,
    message: &'static str,
}

struct Failure {
    side: Option<&'static str>,
    fail: Fail,
}

const OPEN: Fail = Fail {
    code: "io.open",
    message: "file could not be opened",
};
const REGULAR: Fail = Fail {
    code: "io.regular",
    message: "path is not a regular file",
};
const READ_ERR: Fail = Fail {
    code: "io.read",
    message: "file could not be read",
};
const UTF8: Fail = Fail {
    code: "json.utf8",
    message: "document is not UTF-8",
};
const SYNTAX: Fail = Fail {
    code: "json.syntax",
    message: "document is not valid JSON",
};
const DUP_KEY: Fail = Fail {
    code: "json.duplicate_key",
    message: "duplicate JSON object key",
};
const BYTES: Fail = Fail {
    code: "json.budget.bytes",
    message: "document exceeds the byte budget",
};
const DEPTH: Fail = Fail {
    code: "json.budget.depth",
    message: "JSON nesting exceeds the depth budget",
};
const NODES: Fail = Fail {
    code: "json.budget.nodes",
    message: "JSON node count exceeds the budget",
};
const MISSING: Fail = Fail {
    code: "field.missing",
    message: "artifact is missing a required field",
};
const UNKNOWN: Fail = Fail {
    code: "field.unknown",
    message: "artifact has an unknown field",
};
const INVALID: Fail = Fail {
    code: "field.invalid",
    message: "artifact field is not the captured-pg-draft shape",
};
const VERSION: Fail = Fail {
    code: "version.unsupported",
    message: "artifact version is not the frozen captured-pg-draft set",
};
const STATUS: Fail = Fail {
    code: "status.refused",
    message: "artifact claims a status this contract does not emit",
};
const HASH: Fail = Fail {
    code: "hash.mismatch",
    message: "artifact hash does not match the canonical bytes",
};
const DUP_FACT: Fail = Fail {
    code: "duplicate.fact",
    message: "artifact repeats a structural identity",
};

struct Fact {
    status: &'static str,
    kind: &'static str,
    id: String,
    layer: &'static str,
    before: Value,
    after: Value,
}

struct Projected {
    source_id: String,
    database: String,
    schema_hash: String,
    config_hash: String,
    type_hash: String,
    evidence_hash: String,
    draft_hash: String,
    manifest_hash: String,
    wrapper_hash: String,
    source: BTreeMap<String, Value>,
    tables: BTreeMap<String, Value>,
    columns: BTreeMap<String, Value>,
    primary_keys: BTreeMap<String, Value>,
    foreign_keys: BTreeMap<String, Value>,
    generated_columns: BTreeMap<String, Value>,
    generated_primary_keys: BTreeMap<String, Value>,
    generated_foreign_keys: BTreeMap<String, Value>,
    generated_classification: BTreeMap<String, Value>,
    generated_review: BTreeMap<String, Value>,
    curated_classification: BTreeMap<String, Value>,
    curated_review: BTreeMap<String, Value>,
    decisions: BTreeMap<String, Value>,
    stale_queue: BTreeMap<String, Value>,
}

fn main() -> ExitCode {
    match run() {
        Ok(bytes) => write_stdout(&bytes),
        Err(Usage) => {
            usage();
            ExitCode::from(2)
        }
        Err(Failed(failure)) => {
            emit(&failure);
            ExitCode::from(1)
        }
    }
}

enum RunError {
    Usage,
    Failed(Failure),
}

use RunError::{Failed, Usage};

fn usage() {
    eprintln!("ontology-artifact-diff: usage: ontology-artifact-diff --before <file> --after <file>");
    eprintln!(
        "ontology-artifact-diff: reads two oo.builder.captured-pg-draft-command/v1 artifacts"
    );
    eprintln!("ontology-artifact-diff: writes one oo.builder.artifact-diff/v1 structural diff");
    eprintln!(
        "ontology-artifact-diff: does not connect, call a model, import rows, accept, grant, certify, or publish"
    );
}

fn emit(failure: &Failure) {
    if let Some(side) = failure.side {
        eprintln!(
            "ontology-artifact-diff: {side}: {PROTOCOL}/{} {}",
            failure.fail.code, failure.fail.message
        );
    } else {
        eprintln!(
            "ontology-artifact-diff: {PROTOCOL}/{} {}",
            failure.fail.code, failure.fail.message
        );
    }
}

fn write_stdout(bytes: &str) -> ExitCode {
    let mut out = std::io::stdout().lock();
    if out.write_all(bytes.as_bytes()).is_err() {
        eprintln!("ontology-artifact-diff: {PROTOCOL}/io.write stdout could not be written");
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn run() -> Result<String, RunError> {
    let (before_path, after_path) = parse_args()?;
    let before = read_file(&before_path).map_err(|fail| failed("before", fail))?;
    let after = read_file(&after_path).map_err(|fail| failed("after", fail))?;
    let before = project_text(&before).map_err(|fail| failed("before", fail))?;
    let after = project_text(&after).map_err(|fail| failed("after", fail))?;
    let diff = render(&before, &after);
    parse_json_value(&diff, PROTOCOL).map_err(|_| {
        Failed(Failure {
            side: None,
            fail: INVALID,
        })
    })?;
    Ok(diff)
}

fn failed(side: &'static str, fail: Fail) -> RunError {
    Failed(Failure {
        side: Some(side),
        fail,
    })
}

fn parse_args() -> Result<(String, String), RunError> {
    let mut args = env::args().skip(1);
    let mut before = None;
    let mut after = None;
    while let Some(arg) = args.next() {
        let slot = match arg.as_str() {
            "--before" => &mut before,
            "--after" => &mut after,
            _ => return Err(Usage),
        };
        if slot.is_some() {
            return Err(Usage);
        }
        let Some(value) = args.next() else {
            return Err(Usage);
        };
        if value.is_empty() || value.starts_with('-') || value.as_bytes().contains(&0) {
            return Err(Usage);
        }
        *slot = Some(value);
    }
    match (before, after) {
        (Some(before), Some(after)) => Ok((before, after)),
        _ => Err(Usage),
    }
}

fn read_file(path: &str) -> Result<String, Fail> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| OPEN)?;
    if !meta.file_type().is_file() {
        return Err(REGULAR);
    }
    if meta.len() > DOCUMENT_MAX_BYTES as u64 {
        return Err(BYTES);
    }
    let file = File::open(path).map_err(|_| OPEN)?;
    let opened = file.metadata().map_err(|_| READ_ERR)?;
    if !opened.is_file() {
        return Err(REGULAR);
    }
    if opened.len() > DOCUMENT_MAX_BYTES as u64 {
        return Err(BYTES);
    }
    let mut buf = Vec::new();
    file.take(DOCUMENT_MAX_BYTES as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|_| READ_ERR)?;
    if buf.len() > DOCUMENT_MAX_BYTES {
        return Err(BYTES);
    }
    let text = std::str::from_utf8(&buf).map_err(|_| UTF8)?;
    Ok(text.to_string())
}

fn project_text(text: &str) -> Result<Projected, Fail> {
    let value = parse_json_value(text, PROTOCOL).map_err(map_scan)?;
    let root = as_map(&value)?;
    require_only(
        root,
        &["draft", "evidence_envelope", "protocol", "wrapper"],
    )?;
    version_is(root, "protocol", COMMAND_PROTOCOL)?;
    let wrapper = as_map(key(root, "wrapper")?)?;
    let envelope = as_map(key(root, "evidence_envelope")?)?;
    let draft = as_map(key(root, "draft")?)?;
    check_wrapper(wrapper)?;
    check_envelope(envelope)?;
    check_draft_top(draft)?;
    check_seals(root, wrapper, draft)?;
    project_draft(wrapper, draft)
}

fn map_scan(err: oo_core::DiagnosticList) -> Fail {
    let code = err.items.first().map(|item| item.code.as_str()).unwrap_or("");
    if code.ends_with("/json.duplicate_key") {
        DUP_KEY
    } else if code.ends_with("/json.budget.bytes") {
        BYTES
    } else if code.ends_with("/json.budget.depth") {
        DEPTH
    } else if code.ends_with("/json.budget.nodes") {
        NODES
    } else {
        SYNTAX
    }
}

fn check_wrapper(wrapper: &Map<String, Value>) -> Result<(), Fail> {
    require_only(
        wrapper,
        &[
            "captured_at",
            "captured_catalog_hash",
            "certification",
            "compiler",
            "config_hash",
            "connector_protocol",
            "database",
            "draft_hash",
            "evidence_envelope_hash",
            "evidence_hash",
            "grants",
            "historical",
            "live_catalog_review",
            "llm",
            "manifest_hash",
            "protocol",
            "publication",
            "row_import",
            "schema_hash",
            "source_connection",
            "source_id",
            "type_hash",
            "wrapper_hash",
        ],
    )?;
    version_is(wrapper, "protocol", CAPTURED_PROTOCOL)?;
    version_is(wrapper, "compiler", CAPTURED_COMPILER)?;
    version_is(wrapper, "connector_protocol", PG_PROTOCOL)?;
    status_is(wrapper, "certification", "none")?;
    status_is(wrapper, "grants", "not_authorized")?;
    status_is(wrapper, "live_catalog_review", "pending")?;
    status_is(wrapper, "llm", "not_used")?;
    status_is(wrapper, "publication", "unavailable")?;
    status_is(wrapper, "row_import", "not_proven")?;
    status_is(wrapper, "source_connection", "not_used")?;
    require_text(wrapper, "captured_at")?;
    require_hex(wrapper, "captured_catalog_hash")?;
    require_hex(wrapper, "config_hash")?;
    require_hex(wrapper, "draft_hash")?;
    require_hex(wrapper, "evidence_envelope_hash")?;
    require_hex(wrapper, "evidence_hash")?;
    require_hex(wrapper, "manifest_hash")?;
    require_hex(wrapper, "schema_hash")?;
    require_hex(wrapper, "type_hash")?;
    require_hex(wrapper, "wrapper_hash")?;
    require_ident(wrapper, "source_id")?;
    require_name(wrapper, "database")?;
    let historical = as_map(key(wrapper, "historical")?)?;
    require_only(
        historical,
        &[
            "compiler",
            "pg_catalog_contract",
            "pg_catalog_contract_commit",
            "pg_catalog_integration",
            "protocol",
        ],
    )?;
    version_is(historical, "compiler", DRAFT_COMPILER)?;
    version_is(historical, "pg_catalog_contract", PG_PROTOCOL)?;
    version_is(historical, "pg_catalog_contract_commit", HISTORICAL)?;
    version_is(historical, "pg_catalog_integration", "not_accepted")?;
    version_is(historical, "protocol", DRAFT_PROTOCOL)?;
    Ok(())
}

fn check_envelope(envelope: &Map<String, Value>) -> Result<(), Fail> {
    allow_only(
        envelope,
        &[
            "adapter",
            "bindings",
            "catalog",
            "curated",
            "protocol",
            "reviews",
            "selection",
            "source",
            "versions",
        ],
    )?;
    for name in [
        "adapter",
        "bindings",
        "catalog",
        "protocol",
        "reviews",
        "selection",
        "source",
        "versions",
    ] {
        if !envelope.contains_key(name) {
            return Err(MISSING);
        }
    }
    version_is(envelope, "protocol", EVIDENCE_PROTOCOL)?;
    version_is(envelope, "adapter", "pgsql")?;
    if !key(envelope, "catalog")?.is_object() || !key(envelope, "reviews")?.is_array() {
        return Err(INVALID);
    }
    let selection = as_map(key(envelope, "selection")?)?;
    require_only(selection, &["tables"])?;
    if !key(selection, "tables")?.is_array() {
        return Err(INVALID);
    }
    if let Some(curated) = envelope.get("curated")
        && !curated.is_object()
    {
        return Err(INVALID);
    }
    let bindings = as_map(key(envelope, "bindings")?)?;
    require_only(
        bindings,
        &["config_hash", "evidence_hash", "schema_hash", "type_hash"],
    )?;
    require_hex(bindings, "config_hash")?;
    require_hex(bindings, "evidence_hash")?;
    require_hex(bindings, "schema_hash")?;
    require_hex(bindings, "type_hash")?;
    let source = as_map(key(envelope, "source")?)?;
    require_only(source, &["database", "source_id"])?;
    require_ident(source, "source_id")?;
    require_name(source, "database")?;
    let versions = as_map(key(envelope, "versions")?)?;
    require_only(versions, &["config", "evidence", "schema", "type"])?;
    version_is(versions, "config", CONFIG_VERSION)?;
    version_is(versions, "evidence", EVIDENCE_PROTOCOL)?;
    version_is(versions, "schema", HASH_ALGORITHM)?;
    version_is(versions, "type", PG_PROTOCOL)?;
    Ok(())
}

fn check_draft_top(draft: &Map<String, Value>) -> Result<(), Fail> {
    require_only(
        draft,
        &[
            "compiler",
            "curated",
            "effective",
            "generated",
            "physical",
            "protocol",
            "provenance",
            "review_queue",
            "reviews",
        ],
    )?;
    version_is(draft, "protocol", DRAFT_PROTOCOL)?;
    version_is(draft, "compiler", DRAFT_COMPILER)?;
    if !key(draft, "effective")?.is_object() {
        return Err(INVALID);
    }
    let provenance = as_map(key(draft, "provenance")?)?;
    require_only(
        provenance,
        &[
            "adapter",
            "captured_at",
            "certification",
            "config_hash",
            "config_version",
            "database",
            "evidence_hash",
            "evidence_version",
            "ident_algorithm",
            "llm",
            "manifest_hash",
            "oos_version",
            "pg_catalog_contract",
            "pg_catalog_contract_commit",
            "pg_catalog_integration",
            "publication",
            "schema_hash",
            "schema_version",
            "source_connection",
            "source_id",
            "type_hash",
            "type_version",
        ],
    )?;
    version_is(provenance, "adapter", "pgsql")?;
    version_is(provenance, "config_version", CONFIG_VERSION)?;
    version_is(provenance, "evidence_version", EVIDENCE_PROTOCOL)?;
    version_is(provenance, "ident_algorithm", IDENT_ALGORITHM)?;
    version_is(provenance, "oos_version", OOS_VERSION)?;
    version_is(provenance, "pg_catalog_contract", PG_PROTOCOL)?;
    version_is(provenance, "pg_catalog_contract_commit", HISTORICAL)?;
    version_is(provenance, "pg_catalog_integration", "not_accepted")?;
    version_is(provenance, "schema_version", HASH_ALGORITHM)?;
    version_is(provenance, "type_version", PG_PROTOCOL)?;
    status_is(provenance, "certification", "none")?;
    status_is(provenance, "llm", "not_used")?;
    status_is(provenance, "publication", "unavailable")?;
    status_is(provenance, "source_connection", "not_used")?;
    require_text(provenance, "captured_at")?;
    require_hex(provenance, "config_hash")?;
    require_hex(provenance, "evidence_hash")?;
    require_hex(provenance, "manifest_hash")?;
    require_hex(provenance, "schema_hash")?;
    require_hex(provenance, "type_hash")?;
    require_ident(provenance, "source_id")?;
    require_name(provenance, "database")?;
    Ok(())
}

fn check_seals(
    root: &Map<String, Value>,
    wrapper: &Map<String, Value>,
    draft: &Map<String, Value>,
) -> Result<(), Fail> {
    let provenance = as_map(key(draft, "provenance")?)?;
    let envelope = as_map(key(root, "evidence_envelope")?)?;
    same(wrapper, provenance, "schema_hash")?;
    same(wrapper, provenance, "config_hash")?;
    same(wrapper, provenance, "type_hash")?;
    same(wrapper, provenance, "evidence_hash")?;
    same(wrapper, provenance, "manifest_hash")?;
    same(wrapper, provenance, "source_id")?;
    same(wrapper, provenance, "database")?;
    let bindings = as_map(key(envelope, "bindings")?)?;
    same(bindings, provenance, "schema_hash")?;
    same(bindings, provenance, "config_hash")?;
    same(bindings, provenance, "type_hash")?;
    same(bindings, provenance, "evidence_hash")?;
    let source = as_map(key(envelope, "source")?)?;
    same(source, provenance, "source_id")?;
    same(source, provenance, "database")?;
    if canonical_hash(&Value::Object(draft.clone())) != text(wrapper, "draft_hash")? {
        return Err(HASH);
    }
    if canonical_hash(&Value::Object(envelope.clone())) != text(wrapper, "evidence_envelope_hash")?
    {
        return Err(HASH);
    }
    let mut bare = wrapper.clone();
    bare.remove("wrapper_hash");
    if canonical_hash(&Value::Object(bare)) != text(wrapper, "wrapper_hash")? {
        return Err(HASH);
    }
    let mut preimage = draft.clone();
    let Some(object) = preimage.get_mut("provenance").and_then(Value::as_object_mut) else {
        return Err(INVALID);
    };
    if object.remove("manifest_hash").is_none() {
        return Err(MISSING);
    }
    if canonical_hash(&Value::Object(preimage)) != text(provenance, "manifest_hash")? {
        return Err(HASH);
    }
    Ok(())
}

fn project_draft(wrapper: &Map<String, Value>, draft: &Map<String, Value>) -> Result<Projected, Fail> {
    let provenance = as_map(key(draft, "provenance")?)?;
    let mut projected = Projected {
        source_id: text(provenance, "source_id")?,
        database: text(provenance, "database")?,
        schema_hash: text(provenance, "schema_hash")?,
        config_hash: text(provenance, "config_hash")?,
        type_hash: text(provenance, "type_hash")?,
        evidence_hash: text(provenance, "evidence_hash")?,
        draft_hash: text(wrapper, "draft_hash")?,
        manifest_hash: text(provenance, "manifest_hash")?,
        wrapper_hash: text(wrapper, "wrapper_hash")?,
        source: BTreeMap::new(),
        tables: BTreeMap::new(),
        columns: BTreeMap::new(),
        primary_keys: BTreeMap::new(),
        foreign_keys: BTreeMap::new(),
        generated_columns: BTreeMap::new(),
        generated_primary_keys: BTreeMap::new(),
        generated_foreign_keys: BTreeMap::new(),
        generated_classification: BTreeMap::new(),
        generated_review: BTreeMap::new(),
        curated_classification: BTreeMap::new(),
        curated_review: BTreeMap::new(),
        decisions: BTreeMap::new(),
        stale_queue: BTreeMap::new(),
    };
    let source_id = format!(
        "source/{}/{}",
        projected.source_id, projected.database
    );
    insert(
        &mut projected.source,
        source_id,
        serde_json::json!({
            "config_hash": projected.config_hash,
            "database": projected.database,
            "evidence_hash": projected.evidence_hash,
            "schema_hash": projected.schema_hash,
            "source_id": projected.source_id,
            "type_hash": projected.type_hash,
        }),
    )?;
    project_physical(as_map(key(draft, "physical")?)?, &mut projected)?;
    project_generated(as_map(key(draft, "generated")?)?, &mut projected)?;
    project_curated(as_map(key(draft, "curated")?)?, &mut projected)?;
    project_reviews(key(draft, "reviews")?, &mut projected)?;
    project_queue(key(draft, "review_queue")?, &mut projected)?;
    Ok(projected)
}

fn project_physical(physical: &Map<String, Value>, out: &mut Projected) -> Result<(), Fail> {
    require_only(physical, &["tables"])?;
    for table in as_array(key(physical, "tables")?)? {
        let table = as_map(table)?;
        require_only(
            table,
            &[
                "columns",
                "comment",
                "foreign_keys",
                "name",
                "name_ident",
                "policies",
                "primary_key",
                "primary_key_nulls_not_distinct",
                "relkind",
                "replica_identity",
                "rls_enabled",
                "rls_forced",
                "schema",
                "schema_ident",
                "unique_keys",
            ],
        )?;
        evidence_or_null(key(table, "comment")?)?;
        let schema_ident = require_token(table, "schema_ident")?;
        let name_ident = require_token(table, "name_ident")?;
        let qualified = format!("{schema_ident}.{name_ident}");
        insert(
            &mut out.tables,
            format!("table/{qualified}"),
            serde_json::json!({
                "name": require_text(table, "name")?,
                "name_ident": name_ident,
                "relkind": require_text(table, "relkind")?,
                "replica_identity": require_text(table, "replica_identity")?,
                "rls_enabled": require_bool(key(table, "rls_enabled")?)?,
                "rls_forced": require_bool(key(table, "rls_forced")?)?,
                "schema": require_text(table, "schema")?,
                "schema_ident": schema_ident,
            }),
        )?;
        project_columns(key(table, "columns")?, &qualified, out)?;
        project_primary_key(table, &qualified, out)?;
        project_foreign_keys(key(table, "foreign_keys")?, &qualified, out)?;
        walk_unique_keys(key(table, "unique_keys")?)?;
        walk_policies(key(table, "policies")?)?;
    }
    Ok(())
}

fn project_columns(value: &Value, qualified: &str, out: &mut Projected) -> Result<(), Fail> {
    for column in as_array(value)? {
        let column = as_map(column)?;
        require_only(
            column,
            &[
                "attnum",
                "base_type_oid",
                "collation_name",
                "collation_oid",
                "comment",
                "default_expr",
                "domain_checks",
                "enum_label_count",
                "enum_labels_sha256",
                "generated",
                "ident",
                "identity",
                "name",
                "not_null",
                "numeric_precision",
                "numeric_scale",
                "type_name",
                "type_oid",
                "type_schema",
                "type_support",
                "typmod",
                "typtype",
            ],
        )?;
        evidence_or_null(key(column, "comment")?)?;
        evidence_or_null(key(column, "default_expr")?)?;
        walk_evidence_list(key(column, "domain_checks")?)?;
        optional_text(key(column, "collation_name")?)?;
        require_number(key(column, "attnum")?)?;
        require_number(key(column, "collation_oid")?)?;
        require_number(key(column, "enum_label_count")?)?;
        require_number(key(column, "type_oid")?)?;
        require_number(key(column, "typmod")?)?;
        number_or_null(key(column, "base_type_oid")?)?;
        number_or_null(key(column, "numeric_precision")?)?;
        number_or_null(key(column, "numeric_scale")?)?;
        let support = as_map(key(column, "type_support")?)?;
        require_only(support, &["converted", "reason"])?;
        if require_bool(key(support, "converted")?)? {
            return Err(INVALID);
        }
        let ident = require_token(column, "ident")?;
        let labels = require_text(column, "enum_labels_sha256")?;
        if !is_hex(&labels) {
            return Err(INVALID);
        }
        insert(
            &mut out.columns,
            format!("column/{qualified}/{ident}"),
            serde_json::json!({
                "attnum": key(column, "attnum")?,
                "base_type_oid": key(column, "base_type_oid")?,
                "collation_oid": key(column, "collation_oid")?,
                "enum_label_count": key(column, "enum_label_count")?,
                "enum_labels_sha256": labels,
                "generated": require_marker(column, "generated")?,
                "ident": ident,
                "identity": require_marker(column, "identity")?,
                "name": require_text(column, "name")?,
                "not_null": require_bool(key(column, "not_null")?)?,
                "numeric_precision": key(column, "numeric_precision")?,
                "numeric_scale": key(column, "numeric_scale")?,
                "type_name": require_text(column, "type_name")?,
                "type_oid": key(column, "type_oid")?,
                "type_reason": require_text(support, "reason")?,
                "type_schema": require_text(column, "type_schema")?,
                "typmod": key(column, "typmod")?,
                "typtype": require_text(column, "typtype")?,
            }),
        )?;
    }
    Ok(())
}

fn project_primary_key(
    table: &Map<String, Value>,
    qualified: &str,
    out: &mut Projected,
) -> Result<(), Fail> {
    let members = as_array(key(table, "primary_key")?)?;
    let nulls = key(table, "primary_key_nulls_not_distinct")?;
    if !(nulls.is_null() || nulls.is_boolean()) {
        return Err(INVALID);
    }
    if members.is_empty() {
        return Ok(());
    }
    let mut columns = Vec::new();
    for member in members {
        let member = as_map(member)?;
        require_only(member, &["attnum", "ident", "name"])?;
        require_number(key(member, "attnum")?)?;
        columns.push(serde_json::json!({
            "attnum": key(member, "attnum")?,
            "ident": require_token(member, "ident")?,
            "name": require_text(member, "name")?,
        }));
    }
    insert(
        &mut out.primary_keys,
        format!("primary_key/{qualified}"),
        serde_json::json!({
            "columns": columns,
            "nulls_not_distinct": nulls,
        }),
    )
}

fn project_foreign_keys(value: &Value, qualified: &str, out: &mut Projected) -> Result<(), Fail> {
    for key_value in as_array(value)? {
        let foreign = as_map(key_value)?;
        require_only(foreign, &["columns", "ident", "name"])?;
        let ident = require_token(foreign, "ident")?;
        let mut columns = Vec::new();
        for column in as_array(key(foreign, "columns")?)? {
            let column = as_map(column)?;
            require_only(
                column,
                &[
                    "attnum",
                    "ident",
                    "name",
                    "ref_attnum",
                    "ref_ident",
                    "ref_name",
                    "ref_schema",
                    "ref_schema_ident",
                    "ref_table",
                    "ref_table_ident",
                ],
            )?;
            require_number(key(column, "attnum")?)?;
            require_number(key(column, "ref_attnum")?)?;
            columns.push(serde_json::json!({
                "attnum": key(column, "attnum")?,
                "ident": require_token(column, "ident")?,
                "name": require_text(column, "name")?,
                "ref_attnum": key(column, "ref_attnum")?,
                "ref_ident": require_token(column, "ref_ident")?,
                "ref_name": require_text(column, "ref_name")?,
                "ref_schema": require_text(column, "ref_schema")?,
                "ref_schema_ident": require_token(column, "ref_schema_ident")?,
                "ref_table": require_text(column, "ref_table")?,
                "ref_table_ident": require_token(column, "ref_table_ident")?,
            }));
        }
        insert(
            &mut out.foreign_keys,
            format!("foreign_key/{qualified}/{ident}"),
            serde_json::json!({
                "columns": columns,
                "name": require_text(foreign, "name")?,
            }),
        )?;
    }
    Ok(())
}

fn project_generated(generated: &Map<String, Value>, out: &mut Projected) -> Result<(), Fail> {
    require_only(generated, &["datasets", "relationships"])?;
    for dataset in as_array(key(generated, "datasets")?)? {
        let dataset = as_map(dataset)?;
        require_only(
            dataset,
            &[
                "caveats",
                "columns",
                "dataset",
                "description",
                "entity",
                "grain",
                "label",
                "primary_key",
                "recommended",
                "row_count_estimate",
                "schema_version",
                "source_name",
                "superseded_by",
                "tier",
                "time_columns",
            ],
        )?;
        version_is(dataset, "schema_version", OOS_VERSION)?;
        let name = require_token(dataset, "dataset")?;
        walk_generated_ignored(dataset)?;
        let primary = string_list(key(dataset, "primary_key")?)?;
        if !primary.is_empty() {
            insert(
                &mut out.generated_primary_keys,
                format!("primary_key/{name}"),
                serde_json::json!({"columns": primary}),
            )?;
        }
        for column in as_array(key(dataset, "columns")?)? {
            project_generated_column(column, &name, out)?;
        }
    }
    for relationship in as_array(key(generated, "relationships")?)? {
        project_generated_relationship(relationship, out)?;
    }
    Ok(())
}

fn project_generated_column(
    value: &Value,
    dataset: &str,
    out: &mut Projected,
) -> Result<(), Fail> {
    let column = as_map(value)?;
    require_only(
        column,
        &[
            "classification",
            "data_type",
            "deprecated_at",
            "description",
            "enum",
            "label",
            "name",
            "pii",
            "provenance",
            "replaced_by",
            "review",
            "searchable",
            "semantic_type",
            "status",
            "unit",
        ],
    )?;
    let name = require_token(column, "name")?;
    optional_text(key(column, "data_type")?)?;
    insert(
        &mut out.generated_columns,
        format!("column/{dataset}/{name}"),
        serde_json::json!({
            "data_type": key(column, "data_type")?,
            "name": name,
            "provenance": require_text(column, "provenance")?,
            "status": require_text(column, "status")?,
        }),
    )?;
    let classification = semantic_payload(key(column, "classification")?)?;
    insert(
        &mut out.generated_classification,
        format!("classification/{dataset}/{name}"),
        classification,
    )?;
    let review = semantic_payload(key(column, "review")?)?;
    insert(
        &mut out.generated_review,
        format!("review/{dataset}/{name}"),
        review,
    )?;
    Ok(())
}

fn project_generated_relationship(value: &Value, out: &mut Projected) -> Result<(), Fail> {
    let relationship = as_map(value)?;
    require_only(
        relationship,
        &[
            "cardinality",
            "confidence",
            "evidence",
            "from",
            "join_hint",
            "name",
            "provenance",
            "review",
            "status",
            "to",
        ],
    )?;
    if !(key(relationship, "confidence")?.is_null() || key(relationship, "confidence")?.is_number())
    {
        return Err(INVALID);
    }
    if let Some(evidence) = relationship.get("evidence")
        && !evidence.is_null()
    {
        let evidence = as_map(evidence)?;
        allow_only(
            evidence,
            &[
                "containment",
                "name_match",
                "seen_in_query_log",
                "type_compatible",
            ],
        )?;
    }
    let from = end_payload(key(relationship, "from")?)?;
    let to = end_payload(key(relationship, "to")?)?;
    let name = require_token(relationship, "name")?;
    let from_dataset = from.0;
    insert(
        &mut out.generated_foreign_keys,
        format!("foreign_key/{from_dataset}/{name}"),
        serde_json::json!({
            "cardinality": require_text(relationship, "cardinality")?,
            "from_columns": from.1,
            "from_dataset": from_dataset,
            "join_hint": require_text(relationship, "join_hint")?,
            "name": name,
            "provenance": require_text(relationship, "provenance")?,
            "status": require_text(relationship, "status")?,
            "to_columns": to.1,
            "to_dataset": to.0,
        }),
    )
}

fn project_curated(curated: &Map<String, Value>, out: &mut Projected) -> Result<(), Fail> {
    allow_only(curated, &["datasets", "relationships"])?;
    if let Some(datasets) = curated.get("datasets") {
        project_curated_datasets(as_array(datasets)?, out)?;
    }
    if let Some(relationships) = curated.get("relationships") {
        for relationship in as_array(relationships)? {
            let relationship = as_map(relationship)?;
            allow_only(relationship, &["name", "status"])?;
            require_text(relationship, "name")?;
        }
    }
    Ok(())
}

fn project_curated_datasets(
    datasets: &[Value],
    out: &mut Projected,
) -> Result<(), Fail> {
    for dataset in datasets {
        let dataset = as_map(dataset)?;
        allow_only(
            dataset,
            &[
                "columns",
                "dataset",
                "description",
                "entity",
                "grain",
                "label",
                "primary_key",
                "source_name",
            ],
        )?;
        let name = require_token(dataset, "dataset")?;
        let Some(columns) = dataset.get("columns") else {
            continue;
        };
        for column in as_array(columns)? {
            let column = as_map(column)?;
            allow_only(
                column,
                &[
                    "classification",
                    "description",
                    "label",
                    "name",
                    "pii",
                    "review",
                    "semantic_type",
                    "status",
                    "unit",
                ],
            )?;
            let column_name = require_token(column, "name")?;
            if let Some(value) = column.get("classification")
                && !value.is_null()
            {
                insert(
                    &mut out.curated_classification,
                    format!("classification/{name}/{column_name}"),
                    class_payload(value),
                )?;
            }
            if let Some(value) = column.get("review")
                && !value.is_null()
            {
                insert(
                    &mut out.curated_review,
                    format!("review/{name}/{column_name}"),
                    class_payload(value),
                )?;
            }
        }
    }
    Ok(())
}

fn project_reviews(value: &Value, out: &mut Projected) -> Result<(), Fail> {
    for review in as_array(value)? {
        let review = as_map(review)?;
        require_only(
            review,
            &["action", "fact_fingerprint", "item_id", "schema_hash"],
        )?;
        let action = require_text(review, "action")?;
        if !matches!(action.as_str(), "accept" | "reject" | "override") {
            return Err(INVALID);
        }
        let item_id = require_item(review, "item_id")?;
        let schema_hash = require_text(review, "schema_hash")?;
        let fingerprint = require_text(review, "fact_fingerprint")?;
        if !is_hex(&schema_hash) || !is_hex(&fingerprint) {
            return Err(INVALID);
        }
        insert(
            &mut out.decisions,
            item_id,
            serde_json::json!({
                "action": action,
                "fact_fingerprint": fingerprint,
                "schema_hash": schema_hash,
            }),
        )?;
    }
    Ok(())
}

fn project_queue(value: &Value, out: &mut Projected) -> Result<(), Fail> {
    let mut seen = BTreeSet::new();
    for item in as_array(value)? {
        let item = as_map(item)?;
        require_only(
            item,
            &[
                "detail",
                "fact_fingerprint",
                "item_id",
                "kind",
                "publishable",
                "state",
            ],
        )?;
        if require_bool(key(item, "publishable")?)? {
            return Err(STATUS);
        }
        let item_id = require_item(item, "item_id")?;
        let kind = require_text(item, "kind")?;
        if !seen.insert((item_id.clone(), kind.clone())) {
            return Err(DUP_FACT);
        }
        let fingerprint = key(item, "fact_fingerprint")?;
        if !(fingerprint.is_null() || fingerprint.as_str().is_some_and(is_hex)) {
            return Err(INVALID);
        }
        if kind == "stale_decision" {
            insert(
                &mut out.stale_queue,
                item_id,
                serde_json::json!({
                    "detail": require_text(item, "detail")?,
                    "fact_fingerprint": fingerprint,
                    "kind": "stale_decision",
                    "publishable": false,
                    "state": require_text(item, "state")?,
                }),
            )?;
        } else {
            require_text(item, "detail")?;
            require_text(item, "state")?;
        }
    }
    Ok(())
}

fn render(before: &Projected, after: &Projected) -> String {
    let schema_equal = before.schema_hash == after.schema_hash;
    let mut facts = Vec::new();
    let mut unchanged = 0u64;
    diff_map(
        &before.source,
        &after.source,
        schema_equal,
        MapSpec {
            kind: "source",
            layer: "physical",
            stale_when_equal: false,
        },
        &mut facts,
        &mut unchanged,
    );
    diff_map(
        &before.tables,
        &after.tables,
        schema_equal,
        MapSpec {
            kind: "table",
            layer: "physical",
            stale_when_equal: true,
        },
        &mut facts,
        &mut unchanged,
    );
    diff_map(
        &before.columns,
        &after.columns,
        schema_equal,
        MapSpec {
            kind: "column",
            layer: "physical",
            stale_when_equal: true,
        },
        &mut facts,
        &mut unchanged,
    );
    diff_map(
        &before.generated_columns,
        &after.generated_columns,
        schema_equal,
        MapSpec {
            kind: "column",
            layer: "generated",
            stale_when_equal: true,
        },
        &mut facts,
        &mut unchanged,
    );
    diff_map(
        &before.primary_keys,
        &after.primary_keys,
        schema_equal,
        MapSpec {
            kind: "primary_key",
            layer: "physical",
            stale_when_equal: true,
        },
        &mut facts,
        &mut unchanged,
    );
    diff_map(
        &before.generated_primary_keys,
        &after.generated_primary_keys,
        schema_equal,
        MapSpec {
            kind: "primary_key",
            layer: "generated",
            stale_when_equal: true,
        },
        &mut facts,
        &mut unchanged,
    );
    diff_map(
        &before.foreign_keys,
        &after.foreign_keys,
        schema_equal,
        MapSpec {
            kind: "foreign_key",
            layer: "physical",
            stale_when_equal: true,
        },
        &mut facts,
        &mut unchanged,
    );
    diff_map(
        &before.generated_foreign_keys,
        &after.generated_foreign_keys,
        schema_equal,
        MapSpec {
            kind: "foreign_key",
            layer: "generated",
            stale_when_equal: true,
        },
        &mut facts,
        &mut unchanged,
    );
    diff_semantic(
        &before.generated_classification,
        &after.generated_classification,
        schema_equal,
        "classification",
        "generated",
        &mut facts,
        &mut unchanged,
    );
    diff_semantic(
        &before.generated_review,
        &after.generated_review,
        schema_equal,
        "review",
        "generated",
        &mut facts,
        &mut unchanged,
    );
    diff_map(
        &before.curated_classification,
        &after.curated_classification,
        schema_equal,
        MapSpec {
            kind: "classification",
            layer: "curated",
            stale_when_equal: true,
        },
        &mut facts,
        &mut unchanged,
    );
    diff_map(
        &before.curated_review,
        &after.curated_review,
        schema_equal,
        MapSpec {
            kind: "review",
            layer: "curated",
            stale_when_equal: true,
        },
        &mut facts,
        &mut unchanged,
    );
    diff_decisions(
        &before.decisions,
        &after.decisions,
        &after.schema_hash,
        schema_equal,
        &mut facts,
        &mut unchanged,
    );
    diff_stale_queue(
        &before.stale_queue,
        &after.stale_queue,
        &mut facts,
    );
    facts.sort_by(|left, right| {
        left.status
            .cmp(right.status)
            .then(left.kind.cmp(right.kind))
            .then(left.id.cmp(&right.id))
            .then(left.layer.cmp(right.layer))
    });
    let mut added = 0u64;
    let mut changed = 0u64;
    let mut removed = 0u64;
    let mut stale = 0u64;
    let mut rendered = Vec::with_capacity(facts.len());
    for fact in facts {
        match fact.status {
            "added" => added += 1,
            "changed" => changed += 1,
            "removed" => removed += 1,
            "stale" => stale += 1,
            _ => {}
        }
        rendered.push(serde_json::json!({
            "after": fact.after,
            "before": fact.before,
            "id": fact.id,
            "kind": fact.kind,
            "layer": fact.layer,
            "status": fact.status,
        }));
    }
    let mut document = serde_json::json!({
        "after": summary(after),
        "auto_accept": false,
        "before": summary(before),
        "certification": "none",
        "compiler": COMPILER,
        "counts": {
            "added": added,
            "changed": changed,
            "removed": removed,
            "stale": stale,
            "unchanged": unchanged,
        },
        "facts": rendered,
        "grants": "not_authorized",
        "protocol": PROTOCOL,
        "publication": "unavailable",
        "schema_hash_equal": schema_equal,
    });
    let diff_hash = canonical_hash(&document);
    if let Some(object) = document.as_object_mut() {
        object.insert("diff_hash".to_string(), Value::String(diff_hash));
    }
    canonical_json(&document)
}

fn summary(projected: &Projected) -> Value {
    serde_json::json!({
        "config_hash": projected.config_hash,
        "database": projected.database,
        "draft_hash": projected.draft_hash,
        "evidence_hash": projected.evidence_hash,
        "manifest_hash": projected.manifest_hash,
        "schema_hash": projected.schema_hash,
        "source_id": projected.source_id,
        "type_hash": projected.type_hash,
        "wrapper_hash": projected.wrapper_hash,
    })
}

struct MapSpec {
    kind: &'static str,
    layer: &'static str,
    stale_when_equal: bool,
}

fn diff_map(
    before: &BTreeMap<String, Value>,
    after: &BTreeMap<String, Value>,
    schema_equal: bool,
    spec: MapSpec,
    facts: &mut Vec<Fact>,
    unchanged: &mut u64,
) {
    for id in union_keys(before, after) {
        let left = before.get(&id);
        let right = after.get(&id);
        let status = match (left, right) {
            (Some(left), Some(right)) if left == right => {
                if !schema_equal && spec.stale_when_equal {
                    "stale"
                } else {
                    *unchanged += 1;
                    continue;
                }
            }
            (Some(_), Some(_)) => "changed",
            (Some(_), None) => "removed",
            (None, Some(_)) => "added",
            (None, None) => continue,
        };
        push_fact(facts, status, spec.kind, id, spec.layer, left, right);
    }
}

fn diff_semantic(
    before: &BTreeMap<String, Value>,
    after: &BTreeMap<String, Value>,
    schema_equal: bool,
    kind: &'static str,
    layer: &'static str,
    facts: &mut Vec<Fact>,
    unchanged: &mut u64,
) {
    for id in union_keys(before, after) {
        let left = before.get(&id);
        let right = after.get(&id);
        let left_unknown = left.is_some_and(is_unknown);
        let right_unknown = right.is_some_and(is_unknown);
        if left.is_some() && right.is_some() && left_unknown && right_unknown {
            *unchanged += 1;
            continue;
        }
        let status = match (left, right) {
            (Some(left), Some(right)) if left == right && schema_equal => {
                *unchanged += 1;
                continue;
            }
            (Some(left), Some(right)) if left == right => "stale",
            (Some(_), Some(_)) => "changed",
            (Some(_), None) if !left_unknown => "removed",
            (None, Some(_)) if !right_unknown => "added",
            _ => continue,
        };
        push_fact(facts, status, kind, id, layer, left, right);
    }
}

fn diff_decisions(
    before: &BTreeMap<String, Value>,
    after: &BTreeMap<String, Value>,
    after_schema: &str,
    schema_equal: bool,
    facts: &mut Vec<Fact>,
    unchanged: &mut u64,
) {
    for id in union_keys(before, after) {
        let left = before.get(&id);
        let right = after.get(&id);
        let fresh = right
            .and_then(|item| item.get("schema_hash"))
            .and_then(Value::as_str)
            == Some(after_schema);
        let status = match (left, right) {
            (Some(left), Some(right)) if left == right && fresh => {
                *unchanged += 1;
                continue;
            }
            (Some(left), Some(right)) if left == right || !fresh => "stale",
            (Some(_), Some(_)) => "changed",
            (Some(_), None) if schema_equal => "removed",
            (Some(_), None) => "stale",
            (None, Some(_)) if fresh => "added",
            (None, Some(_)) => "stale",
            (None, None) => continue,
        };
        push_fact(facts, status, "review", id, "decision", left, right);
    }
}

fn diff_stale_queue(
    before: &BTreeMap<String, Value>,
    after: &BTreeMap<String, Value>,
    facts: &mut Vec<Fact>,
) {
    for id in union_keys(before, after) {
        let left = before.get(&id);
        let right = after.get(&id);
        let status = if right.is_some() { "stale" } else { "removed" };
        push_fact(facts, status, "review", id, "queue", left, right);
    }
}

fn push_fact(
    facts: &mut Vec<Fact>,
    status: &'static str,
    kind: &'static str,
    id: String,
    layer: &'static str,
    before: Option<&Value>,
    after: Option<&Value>,
) {
    facts.push(Fact {
        status,
        kind,
        id,
        layer,
        before: before.cloned().unwrap_or(Value::Null),
        after: after.cloned().unwrap_or(Value::Null),
    });
}

fn union_keys(before: &BTreeMap<String, Value>, after: &BTreeMap<String, Value>) -> BTreeSet<String> {
    before.keys().chain(after.keys()).cloned().collect()
}

fn is_unknown(value: &Value) -> bool {
    value.get("unknown").and_then(Value::as_bool) == Some(true)
}

fn semantic_payload(value: &Value) -> Result<Value, Fail> {
    if value.is_null() {
        return Ok(serde_json::json!({"unknown": true}));
    }
    check_record(value)?;
    Ok(class_payload(value))
}

fn class_payload(value: &Value) -> Value {
    let digest = payload_sha256(canonical_json(value).as_bytes());
    let state = value
        .get("state")
        .and_then(Value::as_str)
        .filter(|text| is_state_token(text));
    if let Some(state) = state {
        serde_json::json!({"state": state, "unknown": false, "value_sha256": digest})
    } else {
        serde_json::json!({"unknown": false, "value_sha256": digest})
    }
}

fn check_record(value: &Value) -> Result<(), Fail> {
    let object = as_map(value)?;
    allow_only(object, &["at", "by", "state"])?;
    if !object.contains_key("state") {
        return Err(MISSING);
    }
    require_text(object, "state")?;
    if let Some(item) = object.get("by") {
        optional_text(item)?;
    }
    if let Some(item) = object.get("at") {
        optional_text(item)?;
    }
    Ok(())
}

fn is_state_token(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    value.len() <= 32
        && first.is_ascii_lowercase()
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

fn end_payload(value: &Value) -> Result<(String, Vec<String>), Fail> {
    let end = as_map(value)?;
    require_only(end, &["columns", "dataset"])?;
    Ok((
        require_token(end, "dataset")?,
        string_list(key(end, "columns")?)?,
    ))
}

fn walk_generated_ignored(dataset: &Map<String, Value>) -> Result<(), Fail> {
    for name in [
        "description",
        "entity",
        "grain",
        "label",
        "source_name",
        "superseded_by",
        "tier",
    ] {
        let value = key(dataset, name)?;
        if !(value.is_null() || value.is_string()) {
            return Err(INVALID);
        }
    }
    let recommended = key(dataset, "recommended")?;
    if !(recommended.is_null() || recommended.is_boolean()) {
        return Err(INVALID);
    }
    let rows = key(dataset, "row_count_estimate")?;
    if !(rows.is_null() || rows.is_number()) {
        return Err(INVALID);
    }
    for caveat in as_array(key(dataset, "caveats")?)? {
        if !caveat.is_string() {
            return Err(INVALID);
        }
    }
    for column in as_array(key(dataset, "time_columns")?)? {
        let column = as_map(column)?;
        require_only(column, &["name", "role"])?;
        require_text(column, "name")?;
        require_text(column, "role")?;
    }
    Ok(())
}

fn walk_unique_keys(value: &Value) -> Result<(), Fail> {
    for key_value in as_array(value)? {
        let unique = as_map(key_value)?;
        require_only(unique, &["columns", "ident", "name", "nulls_not_distinct"])?;
        require_token(unique, "ident")?;
        require_text(unique, "name")?;
        require_bool(key(unique, "nulls_not_distinct")?)?;
        for column in as_array(key(unique, "columns")?)? {
            let column = as_map(column)?;
            require_only(column, &["attnum", "ident", "name"])?;
            require_number(key(column, "attnum")?)?;
            require_token(column, "ident")?;
            require_text(column, "name")?;
        }
    }
    Ok(())
}

fn walk_policies(value: &Value) -> Result<(), Fail> {
    for policy in as_array(value)? {
        let policy = as_map(policy)?;
        require_only(
            policy,
            &["check_expr", "command", "name", "permissive", "using_expr"],
        )?;
        evidence_or_null(key(policy, "check_expr")?)?;
        evidence_or_null(key(policy, "using_expr")?)?;
        require_text(policy, "command")?;
        require_text(policy, "name")?;
        require_bool(key(policy, "permissive")?)?;
    }
    Ok(())
}

fn walk_evidence_list(value: &Value) -> Result<(), Fail> {
    for item in as_array(value)? {
        evidence_or_null(item)?;
        if item.is_null() {
            return Err(INVALID);
        }
    }
    Ok(())
}

fn evidence_or_null(value: &Value) -> Result<(), Fail> {
    if value.is_null() {
        return Ok(());
    }
    let object = as_map(value)?;
    require_only(object, &["byte_len", "sha256"])?;
    require_number(key(object, "byte_len")?)?;
    let digest = require_text(object, "sha256")?;
    if is_hex(&digest) { Ok(()) } else { Err(INVALID) }
}

fn string_list(value: &Value) -> Result<Vec<String>, Fail> {
    let mut out = Vec::new();
    for item in as_array(value)? {
        let text = item.as_str().ok_or(INVALID)?;
        if !is_token(text) {
            return Err(INVALID);
        }
        out.push(text.to_string());
    }
    Ok(out)
}

fn insert(map: &mut BTreeMap<String, Value>, id: String, payload: Value) -> Result<(), Fail> {
    if map.insert(id, payload).is_some() {
        Err(DUP_FACT)
    } else {
        Ok(())
    }
}

fn same(left: &Map<String, Value>, right: &Map<String, Value>, name: &str) -> Result<(), Fail> {
    if key(left, name)? == key(right, name)? {
        Ok(())
    } else {
        Err(HASH)
    }
}

fn version_is(object: &Map<String, Value>, name: &str, expected: &str) -> Result<(), Fail> {
    if require_text(object, name)? == expected {
        Ok(())
    } else {
        Err(VERSION)
    }
}

fn status_is(object: &Map<String, Value>, name: &str, expected: &str) -> Result<(), Fail> {
    if require_text(object, name)? == expected {
        Ok(())
    } else {
        Err(STATUS)
    }
}

fn require_only(object: &Map<String, Value>, keys: &[&str]) -> Result<(), Fail> {
    allow_only(object, keys)?;
    for key in keys {
        if !object.contains_key(*key) {
            return Err(MISSING);
        }
    }
    Ok(())
}

fn allow_only(object: &Map<String, Value>, keys: &[&str]) -> Result<(), Fail> {
    for key in object.keys() {
        if !keys.iter().any(|item| *item == key) {
            return Err(UNKNOWN);
        }
    }
    Ok(())
}

fn key<'a>(object: &'a Map<String, Value>, name: &str) -> Result<&'a Value, Fail> {
    object.get(name).ok_or(MISSING)
}

fn text(object: &Map<String, Value>, name: &str) -> Result<String, Fail> {
    require_text(object, name)
}

fn require_text(object: &Map<String, Value>, name: &str) -> Result<String, Fail> {
    key(object, name)?
        .as_str()
        .filter(|text| !text.is_empty() && !has_control(text))
        .map(str::to_string)
        .ok_or(INVALID)
}

fn require_marker(object: &Map<String, Value>, name: &str) -> Result<String, Fail> {
    match key(object, name)?.as_str() {
        Some(text) if !has_control(text) => Ok(text.to_string()),
        _ => Err(INVALID),
    }
}

fn require_token(object: &Map<String, Value>, name: &str) -> Result<String, Fail> {
    let text = require_text(object, name)?;
    if is_token(&text) { Ok(text) } else { Err(INVALID) }
}

fn require_ident(object: &Map<String, Value>, name: &str) -> Result<(), Fail> {
    let text = require_text(object, name)?;
    if is_source_id(&text) { Ok(()) } else { Err(INVALID) }
}

fn require_name(object: &Map<String, Value>, name: &str) -> Result<(), Fail> {
    let text = require_text(object, name)?;
    if is_token(&text) { Ok(()) } else { Err(INVALID) }
}

fn require_item(object: &Map<String, Value>, name: &str) -> Result<String, Fail> {
    let text = require_text(object, name)?;
    if text.len() <= 512 { Ok(text) } else { Err(INVALID) }
}

fn require_hex(object: &Map<String, Value>, name: &str) -> Result<(), Fail> {
    let text = require_text(object, name)?;
    if is_hex(&text) { Ok(()) } else { Err(INVALID) }
}

fn require_bool(value: &Value) -> Result<bool, Fail> {
    value.as_bool().ok_or(INVALID)
}

fn require_number(value: &Value) -> Result<(), Fail> {
    if value.is_number() { Ok(()) } else { Err(INVALID) }
}

fn number_or_null(value: &Value) -> Result<(), Fail> {
    if value.is_null() || value.is_number() {
        Ok(())
    } else {
        Err(INVALID)
    }
}

fn optional_text(value: &Value) -> Result<(), Fail> {
    if value.is_null() {
        return Ok(());
    }
    match value.as_str() {
        Some(text) if !text.is_empty() && !has_control(text) => Ok(()),
        _ => Err(INVALID),
    }
}

fn as_map(value: &Value) -> Result<&Map<String, Value>, Fail> {
    value.as_object().ok_or(INVALID)
}

fn as_array(value: &Value) -> Result<&Vec<Value>, Fail> {
    value.as_array().ok_or(INVALID)
}

fn is_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn is_source_id(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    value.len() <= 63
        && first.is_ascii_lowercase()
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-')
}

fn is_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && !value.contains('/')
        && !has_control(value)
}

fn has_control(value: &str) -> bool {
    value.chars().any(|ch| (ch as u32) < 0x20 || ch == '\u{7f}')
}

fn canonical_hash(value: &Value) -> String {
    payload_sha256(canonical_json(value).as_bytes())
}

fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::String(text) => push_string(out, text),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                push_string(out, key);
                out.push(':');
                write_value(out, &map[*key]);
            }
            out.push('}');
        }
    }
}

fn push_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", u32::from(ch)));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}
