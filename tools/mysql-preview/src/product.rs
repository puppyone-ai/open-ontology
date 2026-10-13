//! One customer product invocation.
//!
//! The approved objects are read through `read_source`, sealed by `load_target`,
//! catalogued by `oo-pipeline catalog-import`, then packed and queried by the
//! existing builder and typed consumer. This module does not write a pack,
//! a gold file, or a target row by hand.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use mysql_async::prelude::Queryable;
use mysql_async::{Conn, OptsBuilder, Row, SslOpts, Value as MyValue};
use oo_pipeline::target_installer::sha256_hex;
use serde_json::{json, Map, Value};
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row as PgRow};
use tokio::time::timeout;

use crate::config::{self, read_ca, read_secret};
use crate::customer::CustomerInput;
use crate::error::PreviewError;
use crate::manifest::build_manifest;
use crate::role_acl::install_column_acl;
use crate::source::{self, access_document, read_source, StatementNote};
use crate::target::load_target;

const PRODUCT_KEYS: &[&str] = &[
    "audit_secret_file",
    "audit_secret_ref",
    "audit_user",
    "builder_bin",
    "generation",
    "hidden_column",
    "login_secret_file",
    "login_user",
    "model_bearer_file",
    "operator_bin",
    "owner",
    "pg_role",
    "pipeline_bin",
    "project",
    "reviewed_at",
    "reviewer",
    "runtime_secret_file",
    "shard",
    "source_id",
];

const WRITE_CLASSES: [&str; 12] = [
    "statement/sql/alter_table",
    "statement/sql/create_table",
    "statement/sql/delete",
    "statement/sql/delete_multi",
    "statement/sql/drop_table",
    "statement/sql/insert",
    "statement/sql/insert_select",
    "statement/sql/replace",
    "statement/sql/replace_select",
    "statement/sql/truncate",
    "statement/sql/update",
    "statement/sql/update_multi",
];

const WRITE_CLASS_SQL: &str = "SELECT EVENT_NAME, CAST(COUNT_STAR AS CHAR), CAST(SUM_ERRORS AS CHAR) FROM performance_schema.events_statements_summary_by_user_by_event_name WHERE USER = ? AND EVENT_NAME IN ('statement/sql/alter_table','statement/sql/create_table','statement/sql/delete','statement/sql/delete_multi','statement/sql/drop_table','statement/sql/insert','statement/sql/insert_select','statement/sql/replace','statement/sql/replace_select','statement/sql/truncate','statement/sql/update','statement/sql/update_multi')";

#[derive(Debug)]
pub struct ProductSpec {
    pub source_id: String,
    pub shard: String,
    pub owner: String,
    pub generation: u64,
    pub runtime_secret_file: PathBuf,
    pub login_user: String,
    pub login_secret_file: PathBuf,
    pub pipeline_bin: PathBuf,
    pub builder_bin: PathBuf,
    pub operator_bin: PathBuf,
    pub model_bearer_file: PathBuf,
    pub project: String,
    pub pg_role: String,
    pub hidden_column: String,
    pub reviewer: String,
    pub reviewed_at: String,
    pub audit_user: String,
    pub audit_secret_ref: String,
    pub audit_secret_file: PathBuf,
}

pub fn parse_product(map: &Map<String, Value>) -> Result<ProductSpec, PreviewError> {
    exact(map, PRODUCT_KEYS)?;
    let source_id = lower_ident(text(map, "source_id")?, "source_id")?;
    let project = lower_ident(text(map, "project")?, "project")?;
    let role = text(map, "pg_role")?;
    if !role.starts_with("oo_r_") || role.len() < 6 || !role.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'_') {
        return Err(PreviewError::new("config", "role"));
    }
    let generation = map.get("generation").and_then(Value::as_u64).filter(|value| *value >= 1).ok_or_else(|| PreviewError::new("budget", "generation"))?;
    let reviewed_at = text(map, "reviewed_at")?;
    if reviewed_at.len() != 10 || !reviewed_at.as_bytes().iter().enumerate().all(|(index, byte)| if index == 4 || index == 7 { *byte == b'-' } else { byte.is_ascii_digit() }) {
        return Err(PreviewError::new("config", "reviewed_at"));
    }
    let reviewer = text(map, "reviewer")?;
    if reviewer.is_empty() || reviewer.len() > 64 || !reviewer.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')) {
        return Err(PreviewError::new("config", "reviewer"));
    }
    Ok(ProductSpec {
        source_id,
        shard: lower_ident(text(map, "shard")?, "shard")?,
        owner: {
            let owner = text(map, "owner")?;
            if owner.len() != 36 {
                return Err(PreviewError::new("config", "owner"));
            }
            owner.to_string()
        },
        generation,
        runtime_secret_file: absolute(text(map, "runtime_secret_file")?)?,
        login_user: lower_ident(text(map, "login_user")?, "user")?,
        login_secret_file: absolute(text(map, "login_secret_file")?)?,
        pipeline_bin: absolute(text(map, "pipeline_bin")?)?,
        builder_bin: absolute(text(map, "builder_bin")?)?,
        operator_bin: absolute(text(map, "operator_bin")?)?,
        model_bearer_file: absolute(text(map, "model_bearer_file")?)?,
        project,
        pg_role: role.to_string(),
        hidden_column: {
            let name = text(map, "hidden_column")?;
            if !config::mysql_ident(name) {
                return Err(PreviewError::new("ident", "ident"));
            }
            name.to_string()
        },
        reviewer: reviewer.to_string(),
        reviewed_at: reviewed_at.to_string(),
        audit_user: lower_ident(text(map, "audit_user")?, "user")?,
        audit_secret_ref: audit_ref(text(map, "audit_secret_ref")?)?,
        audit_secret_file: absolute(text(map, "audit_secret_file")?)?,
    })
}

pub async fn close_loop(input: &CustomerInput, out_dir: &Path) -> Result<Value, PreviewError> {
    let spec = input.product.as_ref().ok_or_else(|| PreviewError::new("config", "stop"))?;
    fs::create_dir_all(out_dir).map_err(|_| PreviewError::new("config", "out"))?;
    let hidden_on = input.requested.iter().find(|object| object.columns.iter().any(|column| column == &spec.hidden_column));
    if hidden_on.is_none() {
        return Err(mark(out_dir, "hidden", PreviewError::new("config", "columns")));
    }
    let (preview, preview_bytes) = preview_config(input, spec)?;
    fs::write(out_dir.join("preview.json"), &preview_bytes).map_err(|_| PreviewError::new("config", "out"))?;
    let config = config::parse_config(std::str::from_utf8(&preview_bytes).map_err(|_| PreviewError::new("config", "json"))?)?;
    let _ = preview;
    let password = read_secret(&config.mysql_password_file)?;
    let mut source = read_source(&config, password.as_str(), &preview_bytes).await.map_err(|err| mark(out_dir, "read", err))?;
    if source.tables.iter().any(|table| table.batches.iter().map(|batch| batch.rows).sum::<u64>() == 0) {
        return Err(mark(out_dir, "rows", PreviewError::new("budget", "max_rows")));
    }
    for table in &source.tables {
        let rows = table.batches.iter().map(|batch| batch.rows).sum::<u64>();
        if rows > config.batch_rows && table.batches.len() < 2 {
            return Err(mark(out_dir, "stream", PreviewError::new("batch", "arrow")));
        }
    }
    let admin_password = read_secret(&config.target_admin_password_file)?;
    let runtime_password = read_secret(&config.target_runtime_password_file)?;
    let admin_ca = read_ca(&config.target_ca_file)?;
    let loaded = load_target(&config, admin_password.as_str(), runtime_password.as_str(), &admin_ca, &admin_ca, &source)
        .await
        .map_err(|err| mark(out_dir, "load", err))?;
    if loaded.state != "awaiting_publish" {
        return Err(mark(out_dir, "seal", PreviewError::new("target", "state")));
    }
    let seal = seal_body(input, &config, &source, &loaded);
    let manifest = build_manifest(&access_document(&source), &seal).map_err(|err| mark(out_dir, "manifest", err))?;
    write_json(&out_dir.join("seal.json"), &seal)?;
    write_json(&out_dir.join("manifest.json"), &manifest)?;
    let catalog_bytes = catalog_import(spec, &config, &source, &manifest, out_dir).map_err(|err| mark(out_dir, "catalog", err))?;
    fs::write(out_dir.join("catalog.json"), &catalog_bytes).map_err(|_| PreviewError::new("config", "out"))?;
    let catalog_text = std::str::from_utf8(&catalog_bytes).map_err(|_| PreviewError::new("catalog", "protocol"))?;
    if !catalog_text.contains("oo.pipeline.sealed-import-catalog/v1") || catalog_text.contains("oo.source.pgsql") {
        return Err(mark(out_dir, "catalog-protocol", PreviewError::new("catalog", "protocol")));
    }
    let settings = settings_json(input, spec);
    let visibility = visibility_json(input, spec);
    write_json(&out_dir.join("settings.json"), &settings)?;
    write_json(&out_dir.join("visibility.json"), &visibility)?;
    let acl_cfg = out_dir.join("acl.json");
    write_json(
        &acl_cfg,
        &json!({
            "protocol": "oo.mysql-preview.role-acl/v1",
            "source_id": spec.source_id,
            "visibility_file": out_dir.join("visibility.json"),
            "target": {
                "admin_secret_file": input.target_admin_secret_file,
                "admin_user": input.target_admin_user,
                "ca_file": input.target_ca_file,
                "connect_timeout_ms": input.connect_timeout_ms,
                "database": input.target_database,
                "host": input.target_host,
                "port": input.target_port
            }
        }),
    )?;
    install_column_acl(&acl_cfg, &out_dir.join("acl-outcome.json")).await.map_err(|err| mark(out_dir, "acl", err))?;
    let oracle = capture_oracle(input, &config, password.as_str(), &mut source).await.map_err(|err| mark(out_dir, "oracle", err))?;
    write_json(&out_dir.join("oracle.json"), &oracle)?;
    let (writes, audit_receipt) = audit_writes(input, &config, password.as_str(), &mut source.statements).await.map_err(|err| mark(out_dir, "audit", err))?;
    bind_external_audit(input, &audit_receipt, &source.statements).map_err(|err| mark(out_dir, "audit-bind", err))?;
    if !writes.is_empty() {
        return Err(mark(out_dir, "audit-writes", PreviewError::new("privilege", "write_verb")));
    }
    write_json(&out_dir.join("audit-receipt.json"), &audit_receipt)?;
    let cache = out_dir.join("cache");
    fs::create_dir_all(&cache).map_err(|_| PreviewError::new("config", "out"))?;
    let fresh_dir = out_dir.join("builder-fresh");
    let cache_dir = out_dir.join("builder-cache");
    let denied_dir = out_dir.join("builder-401");
    let timeout_dir = out_dir.join("builder-timeout");
    let unknown_dir = out_dir.join("builder-cost");
    let berth_dir = out_dir.join("builder-berth");
    let denied_bearer = out_dir.join("bearer-401");
    fs::write(&denied_bearer, b"invalid-bearer-token\n").map_err(|_| PreviewError::new("config", "out"))?;
    let dock = dataset_name(input, 0)?;
    let other = dataset_name(input, 1).unwrap_or_else(|_| dock.clone());
    let declared = json!({"currency": "USD", "per_total_token": "0.000001", "status": "declared_non_billing"});
    let unknown = json!({"status": "unknown"});
    run_builder(&BuilderRun {
        spec,
        input,
        catalog: &catalog_bytes,
        root: out_dir,
        out_dir: &denied_dir,
        cache: &cache,
        bearer: &denied_bearer,
        deadline_ms: 90_000,
        price: &declared,
        dataset: &dock,
    })
    .map_err(|err| mark(out_dir, "builder-401", err))?;
    run_builder(&BuilderRun {
        spec,
        input,
        catalog: &catalog_bytes,
        root: out_dir,
        out_dir: &timeout_dir,
        cache: &cache,
        bearer: &spec.model_bearer_file,
        deadline_ms: 1,
        price: &declared,
        dataset: &dock,
    })
    .map_err(|err| mark(out_dir, "builder-timeout", err))?;
    run_builder(&BuilderRun {
        spec,
        input,
        catalog: &catalog_bytes,
        root: out_dir,
        out_dir: &unknown_dir,
        cache: &cache,
        bearer: &spec.model_bearer_file,
        deadline_ms: 90_000,
        price: &unknown,
        dataset: &dock,
    })
    .map_err(|err| mark(out_dir, "builder-cost", err))?;
    run_builder(&BuilderRun {
        spec,
        input,
        catalog: &catalog_bytes,
        root: out_dir,
        out_dir: &fresh_dir,
        cache: &cache,
        bearer: &spec.model_bearer_file,
        deadline_ms: 90_000,
        price: &declared,
        dataset: &dock,
    })
    .map_err(|err| mark(out_dir, "builder-fresh", err))?;
    run_builder(&BuilderRun {
        spec,
        input,
        catalog: &catalog_bytes,
        root: out_dir,
        out_dir: &cache_dir,
        cache: &cache,
        bearer: &spec.model_bearer_file,
        deadline_ms: 90_000,
        price: &declared,
        dataset: &dock,
    })
    .map_err(|err| mark(out_dir, "builder-cache", err))?;
    if other != dock {
        run_builder(&BuilderRun {
            spec,
            input,
            catalog: &catalog_bytes,
            root: out_dir,
            out_dir: &berth_dir,
            cache: &cache,
            bearer: &spec.model_bearer_file,
            deadline_ms: 90_000,
            price: &declared,
            dataset: &other,
        })
        .map_err(|err| mark(out_dir, "builder-berth", err))?;
    }
    expect_builder(&denied_dir, "provider_failure", "not_produced", Some(401), None).map_err(|err| mark(out_dir, "expect-401", err))?;
    expect_builder(&timeout_dir, "provider_failure", "not_produced", None, Some("timeout")).map_err(|err| mark(out_dir, "expect-timeout", err))?;
    expect_builder(&unknown_dir, "cost_unknown", "not_produced", None, Some("unknown_cost")).map_err(|err| mark(out_dir, "expect-cost", err))?;
    expect_builder(&fresh_dir, "packed", "produced", None, None).map_err(|err| mark(out_dir, "expect-fresh", err))?;
    let fresh = read_json(&fresh_dir.join("result.json"))?;
    if fresh.get("cache").and_then(Value::as_str) != Some("miss") || fresh.get("socket").and_then(Value::as_str) != Some("opened") {
        return Err(mark(out_dir, "expect-fresh-socket", PreviewError::new("target", "pack")));
    }
    expect_builder(&cache_dir, "packed", "produced", None, None).map_err(|err| mark(out_dir, "expect-cache", err))?;
    let cached = read_json(&cache_dir.join("result.json"))?;
    if cached.get("cache").and_then(Value::as_str) != Some("hit") || cached.get("socket").and_then(Value::as_str) != Some("not_opened") {
        return Err(mark(out_dir, "expect-cache-hit", PreviewError::new("target", "pack")));
    }
    if other != dock {
        expect_builder(&berth_dir, "packed", "produced", None, None).map_err(|err| mark(out_dir, "expect-berth", err))?;
    }
    refuse_prompt_values(out_dir, &oracle).map_err(|err| mark(out_dir, "prompt", err))?;
    let dock_authority = fresh_dir.join("authority.json");
    let other_authority = if other != dock { berth_dir.join("authority.json") } else { dock_authority.clone() };
    let dock_query = query_visible(input, spec, 0, &out_dir.join("query-dock.json"))?;
    let other_query = if other != dock { Some(query_visible(input, spec, 1, &out_dir.join("query-berth.json"))?) } else { None };
    let dock_out = operator_query(spec, &dock_authority, &dock_query, &out_dir.join("op-dock")).map_err(|err| mark(out_dir, "query-dock", err))?;
    let other_out = if let Some(path) = &other_query {
        Some(operator_query(spec, &other_authority, path, &out_dir.join("op-berth")).map_err(|err| mark(out_dir, "query-berth", err))?)
    } else {
        None
    };
    let dock_columns = visible_names(input, spec, 0).map_err(|err| mark(out_dir, "visible-dock", err))?;
    let other_columns = if other != dock {
        Some(visible_names(input, spec, 1).map_err(|err| mark(out_dir, "visible-other", err))?)
    } else {
        None
    };
    match_oracle(&oracle, 0, &dock_out, &dock_columns).map_err(|err| mark(out_dir, "oracle-dock", err))?;
    if other != dock {
        match_oracle(
            &oracle,
            1,
            other_out.as_ref().ok_or_else(|| PreviewError::new("protocol", "query"))?,
            other_columns.as_deref().unwrap_or(&[]),
        )
        .map_err(|err| mark(out_dir, "oracle-other", err))?;
    }
    match_hidden(input, spec, &seal, &oracle).await.map_err(|err| mark(out_dir, "hidden-stored", err))?;
    let hidden_sql = hidden_query(input, spec, &out_dir.join("query-hidden.json"))?;
    let hidden_code = operator_exit(spec, &dock_authority, &hidden_sql, &out_dir.join("op-hidden")).map_err(|err| mark(out_dir, "query-hidden", err))?;
    if hidden_code != 6 {
        return Err(mark(out_dir, "hidden-exit", PreviewError::new("privilege", "hidden_readable")));
    }
    let widen_code = widen_describe(spec, &dock_authority, out_dir).map_err(|err| mark(out_dir, "widen", err))?;
    if widen_code != 9 {
        return Err(mark(out_dir, "widen-exit", PreviewError::new("target", "pack")));
    }
    let inflight = cancel_inflight(input, spec, &dock_authority, &seal, out_dir).await.map_err(|err| mark(out_dir, "inflight", err))?;
    fs::write(out_dir.join("stage-sealed"), b"{\"stage\":\"sealed\"}\n").map_err(|_| PreviewError::new("config", "out"))?;
    wait_source(input, password.as_str(), false).await.map_err(|err| mark(out_dir, "unreachable", err))?;
    let again = operator_query(spec, &dock_authority, &dock_query, &out_dir.join("op-unreachable")).map_err(|err| mark(out_dir, "query-unreachable", err))?;
    match_oracle(&oracle, 0, &again, &dock_columns).map_err(|err| mark(out_dir, "oracle-unreachable", err))?;
    fs::write(out_dir.join("stage-queried"), b"{\"stage\":\"queried\"}\n").map_err(|_| PreviewError::new("config", "out"))?;
    wait_source(input, password.as_str(), true).await.map_err(|err| mark(out_dir, "reopen", err))?;
    let binaries = json!({
        "mysql_preview": file_sha(&std::env::current_exe().map_err(|_| PreviewError::new("config", "path"))?),
        "pipeline": file_sha(&spec.pipeline_bin),
        "builder": file_sha(&spec.builder_bin),
        "operator": file_sha(&spec.operator_bin)
    });
    Ok(json!({
        "protocol": "oo.mysql-customer/v1",
        "state": "closed",
        "role": input.role,
        "database": input.database,
        "source_id": spec.source_id,
        "project": spec.project,
        "pg_role": spec.pg_role,
        "login_user": spec.login_user,
        "generation": spec.generation,
        "source_generation": loaded.source_generation,
        "target_generation": loaded.target_generation,
        "run_id": loaded.run_id,
        "config_digest": source.config_digest,
        "schema_digest": source.schema_digest,
        "type_digest": source.type_digest,
        "input_digest": source.input_digest,
        "secret_ref_sha256": sha256_hex(input.secret_ref.as_bytes()),
        "rows_retained": source.tables.iter().map(|table| table.batches.iter().map(|batch| batch.rows).sum::<u64>()).sum::<u64>(),
        "batches": source.tables.iter().map(|table| json!({
            "table": table.logical_table,
            "batches": table.batches.iter().map(|batch| json!({"batch_id": batch.batch_id, "rows": batch.rows, "payload_digest": batch.payload_digest})).collect::<Vec<_>>()
        })).collect::<Vec<_>>(),
        "arrow": true,
        "import_run_opened": true,
        "target_opened": true,
        "half_ready": false,
        "query_killed": false,
        "builder": {
            "fresh": brief(&fresh),
            "cache": brief(&cached),
            "unauthorized": brief_file(&denied_dir)?,
            "timeout": brief_file(&timeout_dir)?,
            "cost_unknown": brief_file(&unknown_dir)?,
            "second": if other != dock { brief_file(&berth_dir)? } else { Value::Null }
        },
        "pack_digest": fresh.get("pack_digest").cloned().unwrap_or(Value::Null),
        "queries": {"dock": 0, "second": if other != dock { Value::from(0) } else { Value::Null }, "hidden": hidden_code, "widen": widen_code, "inflight": 0, "unreachable": 0},
        "inflight": inflight,
        "unreachable_queried": true,
        "reopened": true,
        "oracle_match": true,
        "prompt_has_values": false,
        "write_events": writes,
        "audit_receipt": audit_receipt,
        "statements": source.statements.iter().map(|note| json!({"verb": note.verb, "sha256": note.sha256})).collect::<Vec<_>>(),
        "binaries": binaries,
        "objects": input.requested.iter().map(|object| json!({"schema": object.schema, "table": object.table, "columns": object.columns})).collect::<Vec<_>>()
    }))
}

fn preview_config(input: &CustomerInput, spec: &ProductSpec) -> Result<(Value, Vec<u8>), PreviewError> {
    let tables = input
        .requested
        .iter()
        .map(|object| json!({"schema": object.schema, "table": object.table, "columns": object.columns}))
        .collect::<Vec<_>>();
    let body = json!({
        "version": "oo.mysql-preview/v1",
        "stop": "seal",
        "source_id": spec.source_id,
        "shard": spec.shard,
        "owner": spec.owner,
        "generation": spec.generation,
        "boolean_tinyint1": input.boolean_tinyint1,
        "mysql": {
            "host": input.host,
            "port": input.port,
            "database": input.database,
            "user": input.user,
            "password_file": input.secret_file,
            "ca_file": input.ca_file
        },
        "target": {
            "host": input.target_host,
            "port": input.target_port,
            "database": input.target_database,
            "admin_user": input.target_admin_user,
            "admin_password_file": input.target_admin_secret_file,
            "runtime_user": input.target_runtime_user,
            "runtime_password_file": spec.runtime_secret_file,
            "ca_file": input.target_ca_file
        },
        "tables": tables,
        "budgets": {
            "connect_timeout_ms": input.connect_timeout_ms,
            "statement_timeout_ms": input.statement_timeout_ms,
            "lock_timeout_ms": 1000,
            "batch_rows": input.batch_rows,
            "snapshot_deadline_ms": 30000,
            "max_rows": input.max_rows,
            "max_bytes": input.max_bytes
        }
    });
    let bytes = serde_json::to_vec_pretty(&body).map_err(|_| PreviewError::new("config", "json"))?;
    Ok((body, bytes))
}

fn seal_body(input: &CustomerInput, config: &config::PreviewConfig, source: &source::SourceRead, loaded: &crate::target::LoadOutcome) -> Value {
    let mut quality = Vec::new();
    for table in &source.tables {
        for batch in &table.batches {
            quality.push(json!({
                "accepted": batch.rows,
                "batch_id": batch.batch_id,
                "payload_digest": batch.payload_digest,
                "rejected": batch.quality_rejects.len()
            }));
        }
    }
    json!({
        "protocol": "oo.mysql-preview/v1",
        "state": loaded.state,
        "source_id": config.source_id,
        "config_digest": source.config_digest,
        "type_digest": source.type_digest,
        "input_digest": source.input_digest,
        "schema_digest": source.schema_digest,
        "hook_digest": source.hook_digest,
        "source_generation": loaded.source_generation,
        "target_generation": loaded.target_generation,
        "run_id": loaded.run_id,
        "mysql_version": source.version,
        "version_comment": source.version_comment,
        "sql_mode": source.sql_mode,
        "time_zone": source.time_zone,
        "ssl_cipher_present": !source.ssl_cipher.is_empty(),
        "statements": source.statements.iter().map(|note| json!({"verb": note.verb, "sha256": note.sha256})).collect::<Vec<_>>(),
        "tables": loaded.tables,
        "exhausted_tables": loaded.exhausted_tables,
        "open_tables": loaded.open_tables,
        "target_state": loaded.target_state,
        "quality_stored": loaded.quality_stored,
        "quality": quality,
        "role": input.role
    })
}

fn catalog_import(spec: &ProductSpec, config: &config::PreviewConfig, source: &source::SourceRead, manifest: &Value, out_dir: &Path) -> Result<Vec<u8>, PreviewError> {
    let request = serde_json::to_vec(&json!({
        "protocol": "oo.pipeline.catalog-import/v1",
        "source_id": spec.source_id,
        "config_digest": source.config_digest,
        "source_generation": config.generation,
        "manifest": manifest,
        "target": {
            "host": config.target_host,
            "port": config.target_port,
            "database": config.target_database,
            "runtime_principal": config.target_runtime_user,
            "password_file": spec.runtime_secret_file,
            "ca_file": config.target_ca_file
        }
    }))
    .map_err(|_| PreviewError::new("catalog", "protocol"))?;
    let output = command(&spec.pipeline_bin, &["catalog-import".to_string()], Some(&request), &out_dir.join("catalog-spawn"))?;
    if !output.status.success() || output.stdout.is_empty() {
        return Err(PreviewError::new("catalog", "protocol"));
    }
    Ok(output.stdout)
}

fn settings_json(input: &CustomerInput, spec: &ProductSpec) -> Value {
    let mut schemas = Vec::new();
    for object in &input.requested {
        if !schemas.contains(&object.schema) {
            schemas.push(object.schema.clone());
        }
    }
    let schema = schemas.first().map(String::as_str).unwrap_or("source");
    json!({
        "catalog_version": "pending",
        "knowledge": {"id": format!("know-{schema}"), "version": "1"},
        "ontology": {
            "classification": {"auto_accept": "rule_high_confidence"},
            "compile": {
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 4000},
                "per_role": true,
                "sample_values": {"default": "masked", "max_per_column": 0}
            },
            "default_tier_preference": ["raw"],
            "include_schemas": schemas,
            "lifecycle": {"deprecation_min_releases": 2},
            "locale": "en-US",
            "name": schema,
            "publish": {"include_unreviewed": []},
            "spec_version": "oos/v1"
        },
        "ontology_version": "ont-1",
        "policy": {"id": format!("pol-{schema}"), "version": "1"},
        "project": spec.project
    })
}

fn visibility_json(input: &CustomerInput, spec: &ProductSpec) -> Value {
    let mut datasets = Vec::new();
    for object in &input.requested {
        let columns = object
            .columns
            .iter()
            .filter(|column| *column != &spec.hidden_column || object.table != hidden_table(input, spec))
            .cloned()
            .collect::<Vec<_>>();
        datasets.push(json!({
            "columns": columns,
            "dataset": format!("{}.{}", object.schema, object.table),
            "global_stats": "denied",
            "row_security": "none"
        }));
    }
    let schema = input.requested.first().map(|object| object.schema.as_str()).unwrap_or("source");
    json!({
        "catalog_version": "pending",
        "datasets": datasets,
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "knowledge": {"id": format!("know-{schema}"), "version": "1"},
        "limits": {"max_entries": 128, "max_file_bytes": 262144, "max_total_bytes": 1048576},
        "ontology_version": "ont-1",
        "policy": {"id": format!("pol-{schema}"), "version": "1"},
        "principal": "operator",
        "project": spec.project,
        "protocol": "oo.builder.role-visibility/v1",
        "role": spec.pg_role
    })
}

struct BuilderRun<'a> {
    spec: &'a ProductSpec,
    input: &'a CustomerInput,
    catalog: &'a [u8],
    root: &'a Path,
    out_dir: &'a Path,
    cache: &'a Path,
    bearer: &'a Path,
    deadline_ms: u64,
    price: &'a Value,
    dataset: &'a str,
}

fn run_builder(run: &BuilderRun<'_>) -> Result<(), PreviewError> {
    let spec = run.spec;
    let input = run.input;
    let catalog = run.catalog;
    let root = run.root;
    let out_dir = run.out_dir;
    let cache = run.cache;
    let bearer = run.bearer;
    let deadline_ms = run.deadline_ms;
    let price = run.price;
    let dataset = run.dataset;
    fs::create_dir_all(out_dir).map_err(|_| PreviewError::new("config", "out"))?;
    let config_path = out_dir.join("builder-config.json");
    write_json(&config_path, &builder_config(bearer, deadline_ms, price))?;
    let catalog_path = root.join("catalog.json");
    if !catalog_path.is_file() {
        fs::write(&catalog_path, catalog).map_err(|_| PreviewError::new("config", "out"))?;
    }
    let args = vec![
        "sealed".to_string(),
        "--config".to_string(),
        config_path.display().to_string(),
        "--catalog".to_string(),
        catalog_path.display().to_string(),
        "--settings".to_string(),
        root.join("settings.json").display().to_string(),
        "--visibility".to_string(),
        root.join("visibility.json").display().to_string(),
        "--cache".to_string(),
        cache.display().to_string(),
        "--out".to_string(),
        out_dir.display().to_string(),
        "--reviewer".to_string(),
        spec.reviewer.clone(),
        "--reviewed-at".to_string(),
        spec.reviewed_at.clone(),
        "--dataset".to_string(),
        dataset.to_string(),
        "--target-host".to_string(),
        input.target_host.clone(),
        "--target-port".to_string(),
        input.target_port.to_string(),
        "--target-database".to_string(),
        input.target_database.clone(),
        "--target-user".to_string(),
        spec.login_user.clone(),
        "--login".to_string(),
        spec.login_user.clone(),
        "--target-ca".to_string(),
        input.target_ca_file.display().to_string(),
        "--target-password".to_string(),
        spec.login_secret_file.display().to_string(),
    ];
    let output = command(&spec.builder_bin, &args, None, out_dir)?;
    let _ = output.status;
    if !out_dir.join("result.json").is_file() {
        return Err(PreviewError::new("target", "pack"));
    }
    Ok(())
}

fn builder_config(bearer: &Path, deadline_ms: u64, price: &Value) -> Value {
    json!({
        "adapter": "loopback_http_local_dev",
        "budget": {
            "deadline_ms": deadline_ms,
            "max_completion_tokens": 4000,
            "max_in_flight": 1,
            "max_prompt_tokens": 8000,
            "max_request_bytes": 262144,
            "max_response_bytes": 262144,
            "max_tables": 16,
            "max_total_tokens": 12000
        },
        "declared_price": price,
        "endpoint": {"model_id": "gpt-6.1-sol", "origin": "http://127.0.0.1:8317", "path": "/v1/chat/completions"},
        "namespace": "pg-builder",
        "protocol": "oo.pg-builder.config/v1",
        "purpose": "generate",
        "sample_policy": "none",
        "reasoning_effort": "low",
        "secret": {"bearer_file": bearer},
        "temperature": 0
    })
}

fn expect_builder(dir: &Path, outcome: &str, pack: &str, http_status: Option<u64>, reason: Option<&str>) -> Result<(), PreviewError> {
    let body = read_json(&dir.join("result.json"))?;
    if body.get("outcome").and_then(Value::as_str) != Some(outcome) || body.get("pack").and_then(Value::as_str) != Some(pack) {
        return Err(PreviewError::new("target", "pack"));
    }
    if let Some(status) = http_status
        && body.get("http_status").and_then(Value::as_u64) != Some(status)
    {
        return Err(PreviewError::new("target", "pack"));
    }
    if let Some(reason) = reason && body.get("reason").and_then(Value::as_str) != Some(reason) {
        return Err(PreviewError::new("target", "pack"));
    }
    Ok(())
}

async fn capture_oracle(input: &CustomerInput, config: &config::PreviewConfig, password: &str, source: &mut source::SourceRead) -> Result<Value, PreviewError> {
    let mut conn = source::open_mysql(config, password).await?;
    let mut tables = Vec::new();
    for (index, object) in input.requested.iter().enumerate() {
        let list = quote_mysql(&object.columns)?;
        let key = object.columns.first().ok_or_else(|| PreviewError::new("catalog", "primary_key"))?;
        let sql = format!("SELECT {list} FROM `{}`.`{}` ORDER BY `{key}`", object.schema, object.table);
        note(&mut source.statements, &sql);
        let rows: Vec<Row> = timeout(Duration::from_secs(30), conn.query(sql))
            .await
            .map_err(|_| PreviewError::new("budget", "statement_timeout"))?
            .map_err(|_| PreviewError::new("protocol", "query"))?;
        let mut encoded = Vec::new();
        for row in &rows {
            let mut cells = Vec::new();
            for column in 0..object.columns.len() {
                cells.push(if matches!(row.get::<MyValue, _>(column), Some(MyValue::NULL)) { Value::Null } else { Value::String(mysql_cell(row, column)?) });
            }
            encoded.push(cells);
        }
        let oids = source.tables.get(index).map(|table| table.columns.iter().map(|column| column.oid).collect::<Vec<_>>()).unwrap_or_default();
        tables.push(json!({"table": object.table, "schema": object.schema, "columns": object.columns, "oids": oids, "rows": encoded}));
    }
    drop(conn);
    verbs_clean(&source.statements)?;
    Ok(json!({"protocol": "oo.mysql-customer.oracle/v1", "tables": tables}))
}

fn refuse_prompt_values(root: &Path, oracle: &Value) -> Result<(), PreviewError> {
    let mut needles = Vec::new();
    let tables = oracle.get("tables").and_then(Value::as_array).ok_or_else(|| PreviewError::new("protocol", "query"))?;
    for table in tables {
        let names = table.get("columns").and_then(Value::as_array).cloned().unwrap_or_default();
        let rows = table.get("rows").and_then(Value::as_array).ok_or_else(|| PreviewError::new("protocol", "query"))?;
        for row in rows {
            let cells = row.as_array().ok_or_else(|| PreviewError::new("protocol", "query"))?;
            for cell in cells {
                let Some(text) = cell.as_str() else { continue };
                if text.len() < 4 || text.bytes().all(|byte| byte.is_ascii_digit()) {
                    continue;
                }
                if names.iter().any(|name| name.as_str() == Some(text)) {
                    continue;
                }
                needles.push(text.to_string());
            }
        }
    }
    for name in ["builder-fresh", "builder-cache", "builder-401", "builder-timeout", "builder-cost", "builder-berth"] {
        let path = root.join(name).join("model-input.json");
        if !path.is_file() {
            continue;
        }
        let text = fs::read_to_string(&path).map_err(|_| PreviewError::new("config", "path"))?;
        if needles.iter().any(|needle| text.contains(needle)) {
            return Err(PreviewError::new("protocol", "cell"));
        }
    }
    Ok(())
}

fn visible_names(input: &CustomerInput, spec: &ProductSpec, index: usize) -> Result<Vec<String>, PreviewError> {
    let object = input.requested.get(index).ok_or_else(|| PreviewError::new("config", "objects"))?;
    let columns = object
        .columns
        .iter()
        .filter(|column| *column != &spec.hidden_column)
        .cloned()
        .collect::<Vec<_>>();
    if columns.is_empty() {
        return Err(PreviewError::new("config", "columns"));
    }
    Ok(columns)
}

fn query_visible(input: &CustomerInput, spec: &ProductSpec, index: usize, path: &Path) -> Result<PathBuf, PreviewError> {
    let object = input.requested.get(index).ok_or_else(|| PreviewError::new("config", "objects"))?;
    let columns = visible_names(input, spec, index)?;
    let key = columns.first().ok_or_else(|| PreviewError::new("catalog", "primary_key"))?;
    let sql = format!("SELECT {} FROM {}.{} ORDER BY {key}", columns.join(", "), object.schema, object.table);
    write_query(path, &sql)?;
    Ok(path.to_path_buf())
}

fn hidden_query(input: &CustomerInput, spec: &ProductSpec, path: &Path) -> Result<PathBuf, PreviewError> {
    let object = input
        .requested
        .iter()
        .find(|object| object.columns.iter().any(|column| column == &spec.hidden_column))
        .ok_or_else(|| PreviewError::new("config", "columns"))?;
    let sql = format!("SELECT {} FROM {}.{}", spec.hidden_column, object.schema, object.table);
    write_query(path, &sql)?;
    Ok(path.to_path_buf())
}

fn write_query(path: &Path, sql: &str) -> Result<(), PreviewError> {
    // operator-preview rejects a result budget above 256 KiB.
    write_json(
        path,
        &json!({
            "protocol": "oo.operator-preview.query/v1",
            "sql": sql,
            "max_rows": 20,
            "max_result_bytes": 262_144,
            "wall_clock_ms": 8000
        }),
    )
}

fn operator_query(spec: &ProductSpec, authority: &Path, query: &Path, out_dir: &Path) -> Result<Value, PreviewError> {
    let output = operator_exit(spec, authority, query, out_dir)?;
    if output != 0 {
        return Err(PreviewError::new("protocol", "query"));
    }
    read_json(&out_dir.join("stdout.json"))
}

fn operator_exit(spec: &ProductSpec, authority: &Path, query: &Path, out_dir: &Path) -> Result<i32, PreviewError> {
    fs::create_dir_all(out_dir).map_err(|_| PreviewError::new("config", "out"))?;
    let output = command(
        &spec.operator_bin,
        &["query".to_string(), "--authority".to_string(), authority.display().to_string(), "--query".to_string(), query.display().to_string()],
        None,
        out_dir,
    )?;
    if !output.stdout.is_empty() {
        fs::write(out_dir.join("stdout.json"), &output.stdout).map_err(|_| PreviewError::new("config", "out"))?;
    }
    Ok(output.status.code().unwrap_or(1))
}

fn match_oracle(oracle: &Value, index: usize, query: &Value, visible_names: &[String]) -> Result<(), PreviewError> {
    if query.get("outcome").and_then(Value::as_str) != Some("succeeded") {
        return Err(PreviewError::new("protocol", "query"));
    }
    let table = oracle.get("tables").and_then(Value::as_array).and_then(|tables| tables.get(index)).ok_or_else(|| PreviewError::new("protocol", "query"))?;
    let names = table.get("columns").and_then(Value::as_array).ok_or_else(|| PreviewError::new("protocol", "query"))?;
    let source_rows = table.get("rows").and_then(Value::as_array).ok_or_else(|| PreviewError::new("protocol", "query"))?;
    let got = query.get("rows").and_then(Value::as_array).ok_or_else(|| PreviewError::new("protocol", "query"))?;
    if source_rows.len() != got.len() || source_rows.is_empty() || visible_names.is_empty() {
        return Err(PreviewError::new("value", "row"));
    }
    let mut visible = Vec::with_capacity(visible_names.len());
    for name in visible_names {
        let Some(found) = names.iter().position(|item| item.as_str() == Some(name.as_str())) else {
            return Err(PreviewError::new("catalog", "missing_column"));
        };
        visible.push(found);
    }
    for (source_row, got_row) in source_rows.iter().zip(got.iter()) {
        let source_row = source_row.as_array().ok_or_else(|| PreviewError::new("value", "row"))?;
        let got_row = got_row.as_array().ok_or_else(|| PreviewError::new("value", "row"))?;
        if got_row.len() != visible.len() {
            return Err(PreviewError::new("value", "row"));
        }
        for (slot, source_index) in visible.iter().enumerate() {
            let source_cell = source_row.get(*source_index).ok_or_else(|| PreviewError::new("value", "row"))?;
            let same = if source_cell.is_null() { got_row[slot].get("kind").and_then(Value::as_str) == Some("null") } else { source_cell.as_str().is_some_and(|text| cell_same(text, &got_row[slot])) };
            if !same {
                return Err(PreviewError::new("value", "row"));
            }
        }
    }
    Ok(())
}

fn cell_same(source: &str, cell: &Value) -> bool {
    let kind = cell.get("kind").and_then(Value::as_str).unwrap_or("");
    match kind {
        "null" => false,
        "int" | "int2" | "int4" | "int8" | "text" => cell.get("value").and_then(Value::as_str) == Some(source),
        "decimal" => decimal_same(source, cell),
        _ => false,
    }
}

fn decimal_same(source: &str, cell: &Value) -> bool {
    let Some((neg, digits, scale)) = split_decimal(source) else {
        return false;
    };
    if cell.get("negative").and_then(Value::as_bool) != Some(neg) {
        return false;
    }
    let Some(coef) = cell.get("coefficient").and_then(number_text) else {
        return false;
    };
    let Some(cell_scale) = cell.get("scale").and_then(Value::as_u64) else {
        return false;
    };
    let target = scale.max(cell_scale);
    magnitude(&digits, scale, target) == magnitude(&coef, cell_scale, target)
}

fn magnitude(digits: &str, scale: u64, target: u64) -> String {
    let trimmed = digits.trim_start_matches('0');
    if trimmed.is_empty() {
        return "0".to_string();
    }
    let mut body = trimmed.to_string();
    for _ in scale..target {
        body.push('0');
    }
    body
}

fn split_decimal(source: &str) -> Option<(bool, String, u64)> {
    let neg = source.starts_with('-');
    let text = source.strip_prefix('-').unwrap_or(source);
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit() || byte == b'.') {
        return None;
    }
    let (whole, frac) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty() || frac.bytes().any(|byte| !byte.is_ascii_digit()) || whole.bytes().any(|byte| !byte.is_ascii_digit()) {
        return None;
    }
    Some((neg, format!("{whole}{frac}"), frac.len() as u64))
}

fn number_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

async fn match_hidden(input: &CustomerInput, spec: &ProductSpec, seal: &Value, oracle: &Value) -> Result<(), PreviewError> {
    let object = hidden_object(input, spec)?;
    let table = physical_name(seal, &object.table, "pt_")?;
    let column = physical_column(seal, &object.table, &spec.hidden_column)?;
    let key_name = object.columns.first().ok_or_else(|| PreviewError::new("catalog", "primary_key"))?;
    let key = physical_column(seal, &object.table, key_name)?;
    let mut conn = pg_admin(input).await?;
    let sql = format!("SELECT \"{key}\"::text, \"{column}\"::text FROM \"_oo\".\"{table}\" ORDER BY 1");
    // key, column, and table already passed hex_physical (pc_/pt_ plus 60 lowercase hex).
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql)).fetch_all(&mut conn).await.map_err(|_| PreviewError::new("target", "probe"))?;
    let _ = conn.close().await;
    let source_rows = oracle_rows(oracle, &object.table)?;
    let hidden_index = object.columns.iter().position(|name| name == &spec.hidden_column).ok_or_else(|| PreviewError::new("catalog", "missing_column"))?;
    if rows.len() != source_rows.len() {
        return Err(PreviewError::new("value", "row"));
    }
    for (row, source) in rows.iter().zip(source_rows.iter()) {
        let stored: String = row.try_get(1).map_err(|_| PreviewError::new("value", "row"))?;
        let expected = source.get(hidden_index).and_then(Value::as_str).ok_or_else(|| PreviewError::new("value", "row"))?;
        if stored != expected {
            return Err(PreviewError::new("value", "row"));
        }
    }
    Ok(())
}

async fn cancel_inflight(input: &CustomerInput, spec: &ProductSpec, authority: &Path, seal: &Value, out_dir: &Path) -> Result<Value, PreviewError> {
    let object = input.requested.first().ok_or_else(|| PreviewError::new("config", "objects"))?;
    let table = physical_name(seal, &object.table, "pt_")?;
    let mut admin = pg_admin(input).await?;
    sqlx::query("BEGIN").execute(&mut admin).await.map_err(|_| PreviewError::new("target", "begin"))?;
    let lock = format!("LOCK TABLE \"_oo\".\"{table}\" IN ACCESS EXCLUSIVE MODE");
    // table already passed hex_physical (pt_ plus 60 lowercase hex).
    if sqlx::query(sqlx::AssertSqlSafe(lock)).execute(&mut admin).await.is_err() {
        let _ = sqlx::query("ROLLBACK").execute(&mut admin).await;
        return Err(PreviewError::new("target", "lock"));
    }
    let query_path = out_dir.join("query-inflight.json");
    let columns = object.columns.iter().filter(|column| *column != &spec.hidden_column).cloned().collect::<Vec<_>>();
    let key = columns.first().ok_or_else(|| PreviewError::new("catalog", "primary_key"))?;
    write_json(
        &query_path,
        &json!({
            "protocol": "oo.operator-preview.query/v1",
            "sql": format!("SELECT {} FROM {}.{} ORDER BY {key}", columns.join(", "), object.schema, object.table),
            "max_rows": 20,
            "max_result_bytes": 262_144,
            "wall_clock_ms": 20000
        }),
    )?;
    let state_dir = authority_state(authority)?;
    let before = list_json(&state_dir.join("executions"))?;
    let op_dir = out_dir.join("op-inflight");
    fs::create_dir_all(&op_dir).map_err(|_| PreviewError::new("config", "out"))?;
    let mut child = Command::new(&spec.operator_bin)
        .arg("query")
        .arg("--authority")
        .arg(authority)
        .arg("--query")
        .arg(&query_path)
        .env_clear()
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| PreviewError::new("protocol", "query"))?;
    let execution = wait_running(&state_dir.join("executions"), &before).await;
    let execution = match execution {
        Ok(id) => id,
        Err(err) => {
            let _ = child.kill();
            let _ = sqlx::query("ROLLBACK").execute(&mut admin).await;
            return Err(err);
        }
    };
    let cancel_dir = out_dir.join("op-cancel");
    let cancel = command(
        &spec.operator_bin,
        &["cancel".to_string(), "--authority".to_string(), authority.display().to_string(), "--execution".to_string(), execution.clone()],
        None,
        &cancel_dir,
    );
    let output = child.wait_with_output().map_err(|_| PreviewError::new("cancel", "not_killed"))?;
    let _ = sqlx::query("ROLLBACK").execute(&mut admin).await;
    let _ = admin.close().await;
    fs::write(op_dir.join("stdout.json"), &output.stdout).map_err(|_| PreviewError::new("config", "out"))?;
    fs::write(op_dir.join("stderr.txt"), &output.stderr).map_err(|_| PreviewError::new("config", "out"))?;
    let cancel = cancel?;
    if !cancel.status.success() || output.status.code() != Some(0) {
        return Err(PreviewError::new("cancel", "not_killed"));
    }
    let body = serde_json::from_slice::<Value>(&output.stdout).map_err(|_| PreviewError::new("cancel", "not_killed"))?;
    if body.get("outcome").and_then(Value::as_str) != Some("canceled")
        || body.get("sqlstate").and_then(Value::as_str) != Some("57014")
        || body.get("rollback_confirmed").and_then(Value::as_bool) != Some(true)
    {
        return Err(PreviewError::new("cancel", "not_killed"));
    }
    Ok(json!({
        "execution_id": execution,
        "outcome": "canceled",
        "sqlstate": "57014",
        "rollback_confirmed": true,
        "cancel": "sent"
    }))
}

async fn wait_running(dir: &Path, before: &[String]) -> Result<String, PreviewError> {
    let start = Instant::now();
    loop {
        if let Ok(names) = list_json(dir) {
            for name in names {
                if before.iter().any(|item| item == &name) {
                    continue;
                }
                let body = read_json(&dir.join(&name)).unwrap_or(Value::Null);
                if body.get("status").and_then(Value::as_str) == Some("running") && body.get("backend_pid").and_then(Value::as_i64).unwrap_or(0) > 0 {
                    return Ok(name.trim_end_matches(".json").to_string());
                }
            }
        }
        if start.elapsed() > Duration::from_secs(20) {
            return Err(PreviewError::new("cancel", "not_running"));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn wait_source(input: &CustomerInput, password: &str, want_up: bool) -> Result<(), PreviewError> {
    let start = Instant::now();
    loop {
        let up = source_up(input, password).await;
        if up == want_up {
            return Ok(());
        }
        if start.elapsed() > Duration::from_secs(180) {
            return Err(PreviewError::new("connect", "connect_timeout"));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn source_up(input: &CustomerInput, password: &str) -> bool {
    let ssl = SslOpts::default()
        .with_root_certs(vec![input.ca_file.clone().into()])
        .with_danger_accept_invalid_certs(false)
        .with_danger_skip_domain_validation(false)
        .with_disable_built_in_roots(true);
    let opts = OptsBuilder::default()
        .ip_or_hostname(input.host.clone())
        .tcp_port(input.port)
        .user(Some(input.user.clone()))
        .pass(Some(password.to_string()))
        .db_name(Some(input.database.clone()))
        .prefer_socket(false)
        .ssl_opts(ssl);
    matches!(timeout(Duration::from_millis(1000), mysql_async::Conn::new(opts)).await, Ok(Ok(_)))
}

async fn audit_writes(input: &CustomerInput, config: &config::PreviewConfig, password: &str, notes: &mut Vec<StatementNote>) -> Result<(Vec<String>, Value), PreviewError> {
    verbs_clean(notes)?;
    let spec = input.product.as_ref().ok_or_else(|| PreviewError::new("config", "stop"))?;
    if spec.audit_user == input.user || spec.audit_secret_file == input.secret_file || spec.audit_secret_ref == input.secret_ref {
        return Err(PreviewError::new("privilege", "audit_identity"));
    }
    let mut product_conn = source::open_mysql(config, password).await?;
    note(notes, WRITE_CLASS_SQL);
    let product_read = match read_classes(&mut product_conn, input.user.as_str()).await {
        Ok(_) => "zero",
        Err(err) if err.fact == "audit_unreadable" || err.fact == "statement_timeout" => "unreadable",
        Err(err) => return Err(err),
    };
    drop(product_conn);
    let audit_password = read_secret(&spec.audit_secret_file)?;
    let mut auditor = open_account(input, spec.audit_user.as_str(), audit_password.as_str()).await?;
    let current = scalar_quiet(&mut auditor, "SELECT CURRENT_USER()").await?;
    let account = current.split('@').next().unwrap_or("");
    if account.is_empty() || account == input.user || account != spec.audit_user {
        drop(auditor);
        return Err(PreviewError::new("privilege", "audit_identity"));
    }
    let classes = read_classes(&mut auditor, input.user.as_str()).await?;
    drop(auditor);
    let window = statement_window(notes);
    let receipt = json!({
        "protocol": "oo.mysql-customer.audit-receipt/v1",
        "source_id": spec.source_id,
        "database": input.database,
        "subject_user": input.user,
        "auditor_user": spec.audit_user,
        "auditor_account": current,
        "product_read": product_read,
        "window_sha256": window,
        "auditor_statement_sha256": sha256_hex(WRITE_CLASS_SQL.as_bytes()),
        "classes": classes
    });
    Ok((Vec::new(), receipt))
}

fn statement_window(notes: &[StatementNote]) -> String {
    sha256_hex(notes.iter().map(|item| item.sha256.as_str()).collect::<Vec<_>>().join("\n").as_bytes())
}

/// Closed state requires this external receipt. A missing or sentinel audit
/// does not become `write_ok`.
fn bind_external_audit(input: &CustomerInput, receipt: &Value, notes: &[StatementNote]) -> Result<(), PreviewError> {
    let spec = input.product.as_ref().ok_or_else(|| PreviewError::new("config", "stop"))?;
    receipt_is_bound(receipt, &input.user, &spec.audit_user, &spec.source_id, &input.database, &statement_window(notes))
}

fn receipt_is_bound(receipt: &Value, subject: &str, auditor_expected: &str, source_id: &str, database: &str, window: &str) -> Result<(), PreviewError> {
    let map = receipt.as_object().ok_or_else(|| PreviewError::new("privilege", "audit_unreadable"))?;
    if map.get("protocol").and_then(Value::as_str) != Some("oo.mysql-customer.audit-receipt/v1") {
        return Err(PreviewError::new("privilege", "audit_unreadable"));
    }
    if map.get("source_id").and_then(Value::as_str) != Some(source_id)
        || map.get("database").and_then(Value::as_str) != Some(database)
        || map.get("subject_user").and_then(Value::as_str) != Some(subject)
    {
        return Err(PreviewError::new("privilege", "audit_unreadable"));
    }
    let auditor = map.get("auditor_user").and_then(Value::as_str).unwrap_or("");
    if auditor.is_empty() || auditor == subject || auditor != auditor_expected {
        return Err(PreviewError::new("privilege", "audit_identity"));
    }
    let product_read = map.get("product_read").and_then(Value::as_str).unwrap_or("");
    if product_read == "performance_schema_unavailable" || (product_read != "zero" && product_read != "unreadable") {
        return Err(PreviewError::new("privilege", "audit_unreadable"));
    }
    if map.get("window_sha256").and_then(Value::as_str) != Some(window) {
        return Err(PreviewError::new("privilege", "audit_unreadable"));
    }
    let classes = map.get("classes").and_then(Value::as_array).ok_or_else(|| PreviewError::new("privilege", "audit_unreadable"))?;
    if classes.len() != WRITE_CLASSES.len() {
        return Err(PreviewError::new("privilege", "audit_unreadable"));
    }
    for (index, class) in classes.iter().enumerate() {
        let event = class.get("event").and_then(Value::as_str).unwrap_or("");
        if event == "performance_schema_unavailable" || event != WRITE_CLASSES[index] {
            return Err(PreviewError::new("privilege", "audit_unreadable"));
        }
        if class.get("count_star").and_then(Value::as_u64) != Some(0) || class.get("sum_errors").and_then(Value::as_u64) != Some(0) {
            return Err(PreviewError::new("privilege", "write_verb"));
        }
    }
    Ok(())
}

async fn read_classes(conn: &mut Conn, user: &str) -> Result<Vec<Value>, PreviewError> {
    let rows = timeout(Duration::from_secs(30), conn.exec::<Row, _, _>(WRITE_CLASS_SQL, (user,)))
        .await
        .map_err(|_| PreviewError::new("budget", "statement_timeout"))?
        .map_err(|_| PreviewError::new("privilege", "audit_unreadable"))?;
    classify_writes(rows)
}

fn classify_writes(rows: Vec<Row>) -> Result<Vec<Value>, PreviewError> {
    let mut counts = [0u64; WRITE_CLASSES.len()];
    let mut errors = [0u64; WRITE_CLASSES.len()];
    let mut seen = [false; WRITE_CLASSES.len()];
    for row in rows {
        let name = mysql_cell(&row, 0)?;
        let count_star = mysql_cell(&row, 1)?.parse::<u64>().map_err(|_| PreviewError::new("protocol", "cell"))?;
        let sum_errors = mysql_cell(&row, 2)?.parse::<u64>().map_err(|_| PreviewError::new("protocol", "cell"))?;
        let Some(index) = WRITE_CLASSES.iter().position(|item| *item == name) else {
            return Err(PreviewError::new("protocol", "cell"));
        };
        if seen[index] {
            return Err(PreviewError::new("protocol", "cell"));
        }
        seen[index] = true;
        counts[index] = count_star;
        errors[index] = sum_errors;
    }
    if counts.iter().any(|value| *value > 0) || errors.iter().any(|value| *value > 0) {
        return Err(PreviewError::new("privilege", "write_verb"));
    }
    if seen.iter().any(|flag| !flag) {
        return Err(PreviewError::new("privilege", "audit_unreadable"));
    }
    Ok(WRITE_CLASSES
        .iter()
        .enumerate()
        .map(|(index, name)| json!({"event": name, "count_star": counts[index], "sum_errors": errors[index]}))
        .collect())
}

async fn scalar_quiet(conn: &mut Conn, sql: &str) -> Result<String, PreviewError> {
    let row: Option<Row> = timeout(Duration::from_secs(30), conn.query_first(sql))
        .await
        .map_err(|_| PreviewError::new("budget", "statement_timeout"))?
        .map_err(|_| PreviewError::new("privilege", "audit_unreadable"))?;
    let row = row.ok_or_else(|| PreviewError::new("protocol", "query"))?;
    mysql_cell(&row, 0)
}

async fn open_account(input: &CustomerInput, user: &str, password: &str) -> Result<Conn, PreviewError> {
    let ssl = SslOpts::default()
        .with_root_certs(vec![input.ca_file.clone().into()])
        .with_danger_accept_invalid_certs(false)
        .with_danger_skip_domain_validation(false)
        .with_disable_built_in_roots(true);
    // The external auditor is not granted the product schema. The receipt is a
    // performance_schema read, so this session does not enter `input.database`.
    let opts = OptsBuilder::default()
        .ip_or_hostname(input.host.clone())
        .tcp_port(input.port)
        .user(Some(user.to_string()))
        .pass(Some(password.to_string()))
        .prefer_socket(false)
        .ssl_opts(ssl);
    match timeout(Duration::from_millis(input.connect_timeout_ms), Conn::new(opts)).await {
        Ok(Ok(conn)) => Ok(conn),
        Ok(Err(_)) => Err(PreviewError::new("connect", "connect")),
        Err(_) => Err(PreviewError::new("connect", "connect_timeout")),
    }
}

async fn pg_admin(input: &CustomerInput) -> Result<sqlx::PgConnection, PreviewError> {
    let password = read_secret(&input.target_admin_secret_file)?;
    let ca = read_ca(&input.target_ca_file)?;
    let options = PgConnectOptions::from_explicit()
        .host(&input.target_host)
        .port(input.target_port)
        .username(&input.target_admin_user)
        .password(password.as_str())
        .database(&input.target_database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca)
        .disable_statement_logging();
    let mut conn = timeout(Duration::from_millis(input.connect_timeout_ms), options.connect())
        .await
        .map_err(|_| PreviewError::new("connect", "connect_timeout"))?
        .map_err(|_| PreviewError::new("connect", "target"))?;
    sqlx::query("SET statement_timeout = '120s'").execute(&mut conn).await.map_err(|_| PreviewError::new("target", "session"))?;
    sqlx::query("SET lock_timeout = '15s'").execute(&mut conn).await.map_err(|_| PreviewError::new("target", "session"))?;
    sqlx::query("SET idle_in_transaction_session_timeout = 0").execute(&mut conn).await.map_err(|_| PreviewError::new("target", "session"))?;
    Ok(conn)
}

fn physical_name(seal: &Value, table: &str, prefix: &str) -> Result<String, PreviewError> {
    let tables = seal.get("tables").and_then(Value::as_array).ok_or_else(|| PreviewError::new("target", "physical_name"))?;
    for item in tables {
        if item.get("table").and_then(Value::as_str) == Some(table) {
            let name = item.get("physical").and_then(Value::as_str).unwrap_or("");
            hex_physical(name, prefix)?;
            return Ok(name.to_string());
        }
    }
    Err(PreviewError::new("target", "physical_name"))
}

fn physical_column(seal: &Value, table: &str, column: &str) -> Result<String, PreviewError> {
    let tables = seal.get("tables").and_then(Value::as_array).ok_or_else(|| PreviewError::new("target", "physical_name"))?;
    for item in tables {
        if item.get("table").and_then(Value::as_str) != Some(table) {
            continue;
        }
        let columns = item.get("columns").and_then(Value::as_array).ok_or_else(|| PreviewError::new("target", "physical_name"))?;
        for field in columns {
            if field.get("name").and_then(Value::as_str) == Some(column) {
                let name = field.get("physical").and_then(Value::as_str).unwrap_or("");
                hex_physical(name, "pc_")?;
                return Ok(name.to_string());
            }
        }
    }
    Err(PreviewError::new("target", "physical_name"))
}

fn hex_physical(name: &str, prefix: &str) -> Result<(), PreviewError> {
    let rest = name.strip_prefix(prefix).unwrap_or("");
    if rest.len() == 60 && rest.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
        Ok(())
    } else {
        Err(PreviewError::new("target", "physical_name"))
    }
}

fn authority_state(path: &Path) -> Result<PathBuf, PreviewError> {
    let body = read_json(path)?;
    let state = body.get("state_dir").and_then(Value::as_str).ok_or_else(|| PreviewError::new("config", "path"))?;
    Ok(PathBuf::from(state))
}

fn command(bin: &Path, args: &[String], stdin_bytes: Option<&[u8]>, out_dir: &Path) -> Result<std::process::Output, PreviewError> {
    fs::create_dir_all(out_dir).map_err(|_| PreviewError::new("config", "out"))?;
    let mut child = Command::new(bin)
        .args(args)
        .env_clear()
        .stdin(if stdin_bytes.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| PreviewError::new("config", "path"))?;
    if let Some(bytes) = stdin_bytes
        && let Some(mut input) = child.stdin.take()
    {
        input.write_all(bytes).map_err(|_| PreviewError::new("config", "path"))?;
    }
    let output = child.wait_with_output().map_err(|_| PreviewError::new("config", "path"))?;
    let _ = fs::write(out_dir.join("stdout.bin"), &output.stdout);
    let _ = fs::write(out_dir.join("stderr.txt"), &output.stderr);
    Ok(output)
}

fn verbs_clean(notes: &[StatementNote]) -> Result<(), PreviewError> {
    for note in notes {
        if !matches!(note.verb.as_str(), "SELECT" | "SHOW" | "START" | "COMMIT") {
            return Err(PreviewError::new("privilege", "write_verb"));
        }
    }
    Ok(())
}

fn note(notes: &mut Vec<StatementNote>, sql: &str) {
    let verb = sql.split_whitespace().next().unwrap_or("").to_ascii_uppercase();
    notes.push(StatementNote { verb, sha256: sha256_hex(sql.as_bytes()) });
}

fn mysql_cell(row: &Row, index: usize) -> Result<String, PreviewError> {
    match row.get(index).ok_or_else(|| PreviewError::new("protocol", "cell"))? {
        MyValue::NULL => Ok("null".to_string()),
        MyValue::Bytes(bytes) => String::from_utf8(bytes).map_err(|_| PreviewError::new("protocol", "cell")),
        MyValue::Int(value) => Ok(value.to_string()),
        MyValue::UInt(value) => Ok(value.to_string()),
        _ => Err(PreviewError::new("protocol", "cell")),
    }
}

fn quote_mysql(names: &[String]) -> Result<String, PreviewError> {
    let mut list = String::new();
    for name in names {
        if !config::mysql_ident(name) {
            return Err(PreviewError::new("ident", "ident"));
        }
        if !list.is_empty() {
            list.push_str(", ");
        }
        list.push('`');
        list.push_str(name);
        list.push('`');
    }
    Ok(list)
}

fn dataset_name(input: &CustomerInput, index: usize) -> Result<String, PreviewError> {
    let object = input.requested.get(index).ok_or_else(|| PreviewError::new("config", "objects"))?;
    Ok(format!("{}.{}", object.schema, object.table))
}

fn hidden_table<'a>(input: &'a CustomerInput, spec: &ProductSpec) -> &'a str {
    input
        .requested
        .iter()
        .find(|object| object.columns.iter().any(|column| column == &spec.hidden_column))
        .map(|object| object.table.as_str())
        .unwrap_or("")
}

fn hidden_object<'a>(input: &'a CustomerInput, spec: &ProductSpec) -> Result<&'a crate::customer::CustomerObject, PreviewError> {
    input
        .requested
        .iter()
        .find(|object| object.columns.iter().any(|column| column == &spec.hidden_column))
        .ok_or_else(|| PreviewError::new("config", "columns"))
}

fn oracle_rows(oracle: &Value, table: &str) -> Result<Vec<Vec<Value>>, PreviewError> {
    let tables = oracle.get("tables").and_then(Value::as_array).ok_or_else(|| PreviewError::new("protocol", "query"))?;
    for item in tables {
        if item.get("table").and_then(Value::as_str) == Some(table) {
            let rows = item.get("rows").and_then(Value::as_array).ok_or_else(|| PreviewError::new("value", "row"))?;
            return Ok(rows.iter().filter_map(|row| row.as_array().cloned()).collect());
        }
    }
    Err(PreviewError::new("catalog", "missing_table"))
}

fn list_json(dir: &Path) -> Result<Vec<String>, PreviewError> {
    let mut names = Vec::new();
    if !dir.is_dir() {
        return Ok(names);
    }
    for entry in fs::read_dir(dir).map_err(|_| PreviewError::new("config", "path"))? {
        let entry = entry.map_err(|_| PreviewError::new("config", "path"))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".json") {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

fn brief(body: &Value) -> Value {
    json!({
        "outcome": body.get("outcome").cloned().unwrap_or(Value::Null),
        "pack": body.get("pack").cloned().unwrap_or(Value::Null),
        "cache": body.get("cache").cloned().unwrap_or(Value::Null),
        "socket": body.get("socket").cloned().unwrap_or(Value::Null),
        "model": body.get("model").cloned().unwrap_or(Value::Null),
        "http_status": body.get("http_status").cloned().unwrap_or(Value::Null),
        "reason": body.get("reason").cloned().unwrap_or(Value::Null),
        "pack_digest": body.get("pack_digest").cloned().unwrap_or(Value::Null)
    })
}

fn brief_file(dir: &Path) -> Result<Value, PreviewError> {
    Ok(brief(&read_json(&dir.join("result.json"))?))
}

fn file_sha(path: &Path) -> String {
    fs::read(path).map(|bytes| sha256_hex(&bytes)).unwrap_or_default()
}

fn write_json(path: &Path, body: &Value) -> Result<(), PreviewError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| PreviewError::new("config", "out"))?;
    }
    fs::write(path, serde_json::to_vec_pretty(body).map_err(|_| PreviewError::new("config", "out"))?).map_err(|_| PreviewError::new("config", "out"))
}

fn read_json(path: &Path) -> Result<Value, PreviewError> {
    let bytes = fs::read(path).map_err(|_| PreviewError::new("config", "path"))?;
    serde_json::from_slice(&bytes).map_err(|_| PreviewError::new("config", "json"))
}

fn mark(out_dir: &Path, step: &str, err: PreviewError) -> PreviewError {
    let _ = fs::write(out_dir.join("step-error.txt"), format!("{step} {} {}\n", err.code, err.fact));
    err
}

fn exact(map: &Map<String, Value>, allowed: &[&str]) -> Result<(), PreviewError> {
    if map.len() == allowed.len() && allowed.iter().all(|key| map.contains_key(*key)) {
        Ok(())
    } else {
        Err(PreviewError::new("config", "keys"))
    }
}

fn text<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a str, PreviewError> {
    map.get(key).and_then(Value::as_str).ok_or_else(|| PreviewError::config_field(key))
}

fn absolute(value: &str) -> Result<PathBuf, PreviewError> {
    let path = PathBuf::from(value);
    if path.is_absolute() { Ok(path) } else { Err(PreviewError::new("config", "path")) }
}

fn audit_ref(value: &str) -> Result<String, PreviewError> {
    let bytes = value.as_bytes();
    if !(12..=160).contains(&bytes.len()) || !value.starts_with("secret://") || value.contains(char::is_whitespace) {
        return Err(PreviewError::new("config", "secret_ref"));
    }
    if !bytes[9..].iter().all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'/' | b'_' | b'-' | b'.' | b':')) {
        return Err(PreviewError::new("config", "secret_ref"));
    }
    Ok(value.to_string())
}

fn lower_ident(value: &str, fact: &'static str) -> Result<String, PreviewError> {
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 63
        || !bytes[0].is_ascii_lowercase()
        || !bytes[1..].iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
    {
        return Err(PreviewError::new("config", fact));
    }
    Ok(value.to_string())
}

fn widen_describe(spec: &ProductSpec, authority: &Path, out_dir: &Path) -> Result<i32, PreviewError> {
    let body = read_json(authority)?;
    let policy_path = PathBuf::from(body.get("policy_path").and_then(Value::as_str).ok_or_else(|| PreviewError::new("config", "path"))?);
    let mut policy = read_json(&policy_path)?;
    let columns = policy
        .pointer_mut("/table/columns")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| PreviewError::new("target", "pack"))?;
    let mut extra = columns.first().cloned().ok_or_else(|| PreviewError::new("target", "pack"))?;
    if let Some(map) = extra.as_object_mut() {
        map.insert("name".to_string(), json!(spec.hidden_column));
    }
    columns.push(extra);
    let widen = out_dir.join("widen");
    fs::create_dir_all(&widen).map_err(|_| PreviewError::new("config", "out"))?;
    let widened_policy = widen.join("policy.json");
    write_json(&widened_policy, &policy)?;
    let mut authority_body = body;
    if let Some(map) = authority_body.as_object_mut() {
        map.insert("policy_path".to_string(), json!(widened_policy));
    }
    let widened_authority = widen.join("authority.json");
    write_json(&widened_authority, &authority_body)?;
    let output = command(
        &spec.operator_bin,
        &["describe".to_string(), "--authority".to_string(), widened_authority.display().to_string()],
        None,
        &widen.join("describe"),
    )?;
    Ok(output.status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn typed_integers_and_null_do_not_alias_text() {
        assert!(cell_same("1", &json!({"kind":"int4","value":"1"})));
        assert!(!cell_same("1", &json!({"kind":"int4","value":"2"})));
        assert!(!cell_same("", &json!({"kind":"null"})));
        assert!(!cell_same("null", &json!({"kind":"null"})));
        assert!(cell_same("null", &json!({"kind":"text","value":"null"})));
    }

    fn zero_receipt(product_read: &str) -> Value {
        let classes = WRITE_CLASSES
            .iter()
            .map(|name| json!({"event": name, "count_star": 0, "sum_errors": 0}))
            .collect::<Vec<_>>();
        json!({
            "protocol": "oo.mysql-customer.audit-receipt/v1",
            "source_id": "yard",
            "database": "yard",
            "subject_user": "yard_reader",
            "auditor_user": "yard_auditor",
            "product_read": product_read,
            "window_sha256": "abc",
            "classes": classes
        })
    }

    #[test]
    fn missing_audit_and_the_unavailable_sentinel_do_not_bind() {
        let err = receipt_is_bound(&json!({}), "yard_reader", "yard_auditor", "yard", "yard", "abc").expect_err("empty");
        assert_eq!(err.fact, "audit_unreadable");
        let mut sentinel = zero_receipt("performance_schema_unavailable");
        let err = receipt_is_bound(&sentinel, "yard_reader", "yard_auditor", "yard", "yard", "abc").expect_err("sentinel");
        assert_eq!(err.fact, "audit_unreadable");
        sentinel = zero_receipt("unreadable");
        assert!(receipt_is_bound(&sentinel, "yard_reader", "yard_auditor", "yard", "yard", "abc").is_ok());
        let err = receipt_is_bound(&sentinel, "yard_reader", "yard_reader", "yard", "yard", "abc").expect_err("same user");
        assert_eq!(err.fact, "audit_identity");
        let classes = sentinel.get_mut("classes").and_then(Value::as_array_mut).expect("classes");
        classes.pop();
        let err = receipt_is_bound(&sentinel, "yard_reader", "yard_auditor", "yard", "yard", "abc").expect_err("short");
        assert_eq!(err.fact, "audit_unreadable");
    }

    #[test]
    fn decimal_scales_align_before_compare() {
        let wider = json!({"kind": "decimal", "negative": false, "coefficient": "450", "scale": 2});
        let shorter = json!({"kind": "decimal", "negative": false, "coefficient": "45", "scale": 1});
        assert!(decimal_same("4.50", &wider));
        assert!(decimal_same("4.50", &shorter));
        assert!(decimal_same("4.5", &wider));
        assert!(decimal_same("0.00", &json!({"kind": "decimal", "negative": false, "coefficient": "0", "scale": 0})));
        assert!(!decimal_same("4.50", &json!({"kind": "decimal", "negative": true, "coefficient": "450", "scale": 2})));
        assert!(!decimal_same("4.51", &wider));
    }
}
