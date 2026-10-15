//! Experimental MySQL 8 preview. One read-only source connection, then the
//! existing PostgreSQL full-import target. Not a certified connector.

#![forbid(unsafe_code)]

mod c1;
mod c1_materialize;
mod config;
mod capture;
mod customer;
mod dba;
mod error;
mod fixture;
mod logistics;
mod manifest;
mod map;
mod product;
mod oracle;
mod role_acl;
mod source;
mod target;
mod values;
mod version;

use std::fs;
use std::path::Path;

use serde_json::json;

use crate::config::{read_ca, read_secret, Stop};

pub use crate::error::PreviewError;
pub use c1::{prepare_onboarding, baseline_stream, capture_business, configure_dba, expire_c1, prepare_random_gold, prove_c1};
pub use capture::capture_native;
pub use customer::run_customer;
pub use dba::{guard_source, load_logistics};
pub use fixture::load_fixture;
pub use manifest::write_manifest;
pub use oracle::{audit_source, audit_target};
pub use role_acl::install_column_acl;

pub async fn run(config_path: &Path, out_dir: &Path) -> Result<(), PreviewError> {
    let config_bytes = fs::read(config_path).map_err(|_| PreviewError::new("config", "path"))?;
    let text = std::str::from_utf8(&config_bytes).map_err(|_| PreviewError::new("config", "json"))?;
    let config = config::parse_config(text)?;
    fs::create_dir_all(out_dir).map_err(|_| PreviewError::new("config", "out"))?;
    let password = read_secret(&config.mysql_password_file)?;
    let source = match source::read_source(&config, password.as_str(), &config_bytes).await {
        Ok(source) => source,
        Err(err) => {
            write_failure(out_dir, &err)?;
            return Err(err);
        }
    };
    if config.stop == Stop::Access {
        write_access(out_dir, &source)?;
        return Ok(());
    }
    let admin_password = read_secret(&config.target_admin_password_file)?;
    let runtime_password = read_secret(&config.target_runtime_password_file)?;
    let admin_ca = read_ca(&config.target_ca_file)?;
    let runtime_ca = read_ca(&config.target_ca_file)?;
    let loaded = match target::load_target(
        &config,
        admin_password.as_str(),
        runtime_password.as_str(),
        &admin_ca,
        &runtime_ca,
        &source,
    )
    .await
    {
        Ok(loaded) => loaded,
        Err(err) => {
            write_failure(out_dir, &err)?;
            return Err(err);
        }
    };
    let body = json!({
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
        "quality": if loaded.quality_stored { quality_array(&source) } else { Vec::new() }
    });
    fs::write(out_dir.join("outcome.json"), serde_json::to_vec_pretty(&body).map_err(|_| PreviewError::new("config", "out"))?)
        .map_err(|_| PreviewError::new("config", "out"))?;
    if loaded.state == "awaiting_publish" {
        let manifest = manifest::build_manifest(&source::access_document(&source), &body)?;
        fs::write(
            out_dir.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).map_err(|_| PreviewError::new("config", "out"))?,
        )
        .map_err(|_| PreviewError::new("config", "out"))?;
    }
    if loaded.state == "retired" && config.stop == Stop::FailBeforeSeal {
        return Err(PreviewError::new("target", "retired"));
    }
    if loaded.state == "open" && config.stop == Stop::PartialAfterFirst {
        return Err(PreviewError::new("target", "partial"));
    }
    Ok(())
}

fn quality_array(source: &source::SourceRead) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    for table in &source.tables {
        for batch in &table.batches {
            out.push(json!({
                "accepted": batch.rows,
                "batch_id": batch.batch_id,
                "payload_digest": batch.payload_digest,
                "rejected": batch.quality_rejects.len(),
                "rejects": batch.quality_rejects.iter().map(|reject| json!({
                    "attnum": reject.attnum,
                    "key_digest": reject.key_digest,
                    "reason": reject.reason,
                    "row_index": reject.row_index,
                    "value_len": reject.value_len
                })).collect::<Vec<_>>()
            }));
        }
    }
    out
}

fn write_access(out_dir: &Path, source: &source::SourceRead) -> Result<(), PreviewError> {
    let body = source::access_document(source);
    fs::write(out_dir.join("outcome.json"), serde_json::to_vec_pretty(&body).map_err(|_| PreviewError::new("config", "out"))?)
        .map_err(|_| PreviewError::new("config", "out"))
}

fn write_failure(out_dir: &Path, err: &PreviewError) -> Result<(), PreviewError> {
    let body = json!({
        "protocol": "oo.mysql-preview/v1",
        "state": "failed",
        "code": err.code,
        "fact": err.fact
    });
    let _ = fs::create_dir_all(out_dir);
    fs::write(out_dir.join("outcome.json"), serde_json::to_vec_pretty(&body).unwrap_or_default())
        .map_err(|_| PreviewError::new("config", "out"))
}
