//! One native update generation.
//!
//! The command is `oo-pipeline import-native-update`. It copies forward the
//! parent shadow, applies upserts and tombstones, and commits the shadow,
//! apply receipt, freshness, watermark, and quality in that same transaction.
//! `awaiting_publish` is not publication. The loader locks the `jobs.tasks`
//! row that `jobs.claim` already wrote. It does not insert `jobs.leases` and
//! does not set `target_fence_certified`. An empty `lease_id` is refused
//! before seal. A presented task id is locked with `FOR UPDATE`. The
//! generation, owner, and lease time must still be the live claim. Trusted
//! capture opens the server registry, catalog,
//! driver, audit, and Arrow. The receipt's `registry_sha256` is the registry
//! preimage, not the full registry file hash.

use std::fs;
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{ConnectOptions, Connection};

use crate::copy_encoder::{CopyBudgets, ExactDecimal};
use crate::copy_sink::{self, SinkError};
use crate::full_import::hook_digest;
use crate::native_arrow::{
    self, ArrowFault, CopyColumnDecl, DecodedStream, NativeCell, decode_arrow_stream,
    mapping_ident, prepare_table, register_mapping,
};
use crate::native_binding::{self, CaptureReceipt};
use crate::physical_mapping::quote_physical_name;
use crate::target_installer::{self, InstallError};

/// Request protocol.
pub const REQUEST_PROTOCOL: &str = "oo.pipeline.native-update/v1";
pub const PARTITION_PROTOCOL: &str = "oo.pipeline.native-partition/v1";
pub const TRANSACTION_PROTOCOL: &str = "oo.pipeline.native-transaction/v1";
pub const TRANSACTION_RESULT_PROTOCOL: &str = "oo.pipeline.native-transaction-result/v1";

/// Install request protocol.
pub const INSTALL_PROTOCOL: &str = "oo.pipeline.install-native-update/v1";

/// Stdout protocol.
pub const RESULT_PROTOCOL: &str = "oo.pipeline.native-update-result/v1";

const READ_BUDGET: usize = 1_048_576;

/// The loader does not insert a lease and does not certify the target fence.
const JOBS_ROOT: &str = "loader locks the jobs.tasks row jobs.claim wrote; it does not insert jobs.leases and does not set target_fence_certified; an empty lease_id is refused before seal; a presented task id is locked FOR UPDATE and its generation must still be live";

/// Read one update document from stdin and write one result document to stdout.
pub async fn import_native_update() -> Result<(), u8> {
    let mut stdin = Vec::new();
    std::io::Read::read_to_end(&mut std::io::stdin(), &mut stdin).map_err(|_| 2u8)?;
    if stdin.is_empty() || stdin.len() > READ_BUDGET {
        eprintln!("oo-pipeline: usage");
        return Err(2);
    }
    let document: Value = serde_json::from_slice(&stdin).map_err(|_| {
        eprintln!("oo-pipeline: usage");
        2u8
    })?;
    let result = dispatch(&document).await;
    let mut bytes = serde_json::to_vec(&result).map_err(|_| 1u8)?;
    bytes.push(b'\n');
    std::io::Write::write_all(&mut std::io::stdout(), &bytes).map_err(|_| 1u8)?;
    match result.get("load_state").and_then(|value| value.as_str()) {
        Some("awaiting_publish" | "replay") => Ok(()),
        _ => Err(1),
    }
}

/// Apply every source-bound table of one PostgreSQL or GTID transaction in one target transaction.
pub async fn import_native_transaction() -> Result<(), u8> {
    let mut stdin = Vec::new();
    std::io::Read::read_to_end(&mut std::io::stdin(), &mut stdin).map_err(|_| 2u8)?;
    if stdin.is_empty() || stdin.len() > 4 * READ_BUDGET { return Err(2); }
    let document: Value = serde_json::from_slice(&stdin).map_err(|_| 2u8)?;
    let result = match execute_transaction(&document).await {
        Ok(value) => value,
        Err(reason) => json!({"protocol":TRANSACTION_RESULT_PROTOCOL,"load_state":"not_sealed","reason":reason}),
    };
    let mut bytes = serde_json::to_vec(&result).map_err(|_| 1u8)?;
    bytes.push(b'\n');
    std::io::Write::write_all(&mut std::io::stdout(), &bytes).map_err(|_| 1u8)?;
    if matches!(result["load_state"].as_str(), Some("awaiting_publish" | "replay")) { Ok(()) } else { Err(1) }
}

/// Complete the target-side C4 ACK after `jobs.complete_success` commits.
/// Retrying the exact receipt is safe after a lost response.
pub async fn ack_native_partition() -> Result<(), u8> {
    let mut stdin = Vec::new();
    std::io::Read::read_to_end(&mut std::io::stdin(), &mut stdin).map_err(|_| 2u8)?;
    if stdin.is_empty() || stdin.len() > READ_BUDGET { return Err(2); }
    let document: Value = serde_json::from_slice(&stdin).map_err(|_| 2u8)?;
    let result = match ack_partition(&document).await {
        Ok(()) => json!({"protocol":"oo.pipeline.native-partition-ack-result/v1","acknowledged":true}),
        Err(reason) => json!({"protocol":"oo.pipeline.native-partition-ack-result/v1","acknowledged":false,"reason":reason}),
    };
    let mut bytes = serde_json::to_vec(&result).map_err(|_| 1u8)?;
    bytes.push(b'\n');
    std::io::Write::write_all(&mut std::io::stdout(), &bytes).map_err(|_| 1u8)?;
    if result["acknowledged"] == true { Ok(()) } else { Err(1) }
}

/// Rebind an expired target task mirror to the same live control task before
/// replaying its sealed, unacknowledged C4 receipt. No source COPY occurs.
pub async fn recover_native_partition_mirror() -> Result<(), u8> {
    let mut stdin = Vec::new();
    std::io::Read::read_to_end(&mut std::io::stdin(), &mut stdin).map_err(|_| 2u8)?;
    if stdin.is_empty() || stdin.len() > READ_BUDGET { return Err(2); }
    let document: Value = serde_json::from_slice(&stdin).map_err(|_| 2u8)?;
    let result = match recover_partition_mirror(&document).await {
        Ok(generation) => json!({"protocol":"oo.pipeline.native-partition-mirror-rebind-result/v1","rebound":true,"job_generation":generation}),
        Err(reason) => json!({"protocol":"oo.pipeline.native-partition-mirror-rebind-result/v1","rebound":false,"reason":reason}),
    };
    let mut bytes = serde_json::to_vec(&result).map_err(|_| 1u8)?;
    bytes.push(b'\n');
    std::io::Write::write_all(&mut std::io::stdout(), &bytes).map_err(|_| 1u8)?;
    if result["rebound"] == true { Ok(()) } else { Err(1) }
}

/// Mirror only the current control claim through the target owner. The worker
/// never receives jobs.tasks INSERT or UPDATE on the target database.
pub async fn sync_native_task_mirror() -> Result<(), u8> {
    let mut stdin = Vec::new();
    std::io::Read::read_to_end(&mut std::io::stdin(), &mut stdin).map_err(|_| 2u8)?;
    if stdin.is_empty() || stdin.len() > READ_BUDGET { return Err(2); }
    let document: Value = serde_json::from_slice(&stdin).map_err(|_| 2u8)?;
    let result = match sync_task_mirror(&document).await {
        Ok(generation) => json!({"protocol":"oo.pipeline.native-task-mirror-result/v1","mirrored":true,"job_generation":generation}),
        Err(reason) => json!({"protocol":"oo.pipeline.native-task-mirror-result/v1","mirrored":false,"reason":reason}),
    };
    let mut bytes = serde_json::to_vec(&result).map_err(|_| 1u8)?;
    bytes.push(b'\n');
    std::io::Write::write_all(&mut std::io::stdout(), &bytes).map_err(|_| 1u8)?;
    if result["mirrored"] == true { Ok(()) } else { Err(1) }
}

async fn sync_task_mirror(document: &Value) -> Result<i64, &'static str> {
    let object = document.as_object().ok_or("closed")?;
    if object.len() != 2 || document["protocol"] != "oo.pipeline.native-task-mirror/v1"
       || object.keys().any(|key| key != "protocol" && key != "job_task_id") {
        return Err("closed");
    }
    let job = text_field(document, "job_task_id")?;
    if !is_uuid(job) { return Err("closed"); }
    let control_target = pinned_ack_target("OO_NATIVE_CONTROL_TARGET", "OO_NATIVE_CONTROL_TARGET_SHA256")?;
    let mut control = connect_target(&control_target).await?;
    let query = "SELECT row_to_json(task)::text FROM jobs.tasks AS task WHERE task.id=$1::uuid AND task.state='running' AND task.lease_until>clock_timestamp()";
    let control_row: Option<String> = sqlx::query_scalar(query).bind(job)
        .fetch_optional(&mut control).await.map_err(sql_token)?;
    let control_row = control_row.ok_or("jobs_lease_lost")?;
    let observed: Value = serde_json::from_str(&control_row).map_err(|_| "jobs_lease_lost")?;
    let generation = observed["generation"].as_i64().filter(|value| *value >= 1).ok_or("jobs_lease_lost")?;
    let target = pinned_ack_target("OO_NATIVE_MIRROR_TARGET", "OO_NATIVE_MIRROR_TARGET_SHA256")?;
    let mut conn = connect_target(&target).await?;
    let mut tx = conn.begin().await.map_err(sql_token)?;
    let sidecars: Vec<bool> = sqlx::query_scalar(
        "SELECT acknowledged FROM _oo.native_partition_state WHERE job_task_id=$1::uuid FOR UPDATE"
    ).bind(job).fetch_all(&mut *tx).await.map_err(sql_token)?;
    if sidecars.len() > 1 || sidecars.iter().any(|acknowledged| *acknowledged) {
        return Err("partition_prior_ack");
    }
    let result_text: String = sqlx::query_scalar("SELECT jobs.mirror_sync($1::uuid,$2::jsonb)::text")
        .bind(job).bind(&control_row).fetch_one(&mut *tx).await.map_err(sql_token)?;
    let mirrored: Value = serde_json::from_str(&result_text).map_err(|_| "jobs_lease_lost")?;
    if mirrored["mirrored"] != true || mirrored["generation"] != generation {
        return Err("jobs_lease_lost");
    }
    let still_live: Option<String> = sqlx::query_scalar(query).bind(job)
        .fetch_optional(&mut control).await.map_err(sql_token)?;
    if still_live.as_deref().and_then(|row| serde_json::from_str::<Value>(row).ok()) != Some(observed) {
        return Err("jobs_lease_lost");
    }
    tx.commit().await.map_err(sql_token)?;
    Ok(generation)
}

async fn recover_partition_mirror(document: &Value) -> Result<i64, &'static str> {
    let object = document.as_object().ok_or("closed")?;
    const FIELDS: &[&str] = &["protocol", "source_id", "logical_schema", "logical_table",
        "partition_id", "run_id", "job_task_id"];
    if object.len() != FIELDS.len() || object.keys().any(|key| !FIELDS.contains(&key.as_str()))
        || document["protocol"] != "oo.pipeline.native-partition-mirror-rebind/v1" { return Err("closed"); }
    let source = ident(document, "source_id")?;
    let schema = mapped_name(text_field(document, "logical_schema")?)?;
    let table = mapped_name(text_field(document, "logical_table")?)?;
    let partition = document["partition_id"].as_str();
    let run = document["run_id"].as_str();
    if !matches!((partition,run), (None,None) | (Some(_),Some(_))) { return Err("closed"); }
    if partition.is_some_and(|value| value.is_empty() || value.len() > 128 || value.contains('\0')) { return Err("closed"); }
    let job = text_field(document, "job_task_id")?;
    if run.is_some_and(|value| !is_uuid(value)) || !is_uuid(job) { return Err("closed"); }
    let control_target = pinned_ack_target("OO_NATIVE_CONTROL_TARGET", "OO_NATIVE_CONTROL_TARGET_SHA256")?;
    let mut control = connect_target(&control_target).await?;
    let control_row: Option<String> = sqlx::query_scalar(
        "SELECT row_to_json(task)::text FROM jobs.tasks AS task WHERE task.id=$1::uuid AND task.state='running' AND task.lease_until>clock_timestamp()"
    ).bind(job).fetch_optional(&mut control).await.map_err(sql_token)?;
    let control_row = control_row.ok_or("jobs_lease_lost")?;
    let observed: Value = serde_json::from_str(&control_row).map_err(|_| "jobs_lease_lost")?;
    let generation = observed["generation"].as_i64().filter(|value| *value >= 1).ok_or("jobs_lease_lost")?;
    let target = pinned_ack_target("OO_NATIVE_MIRROR_TARGET", "OO_NATIVE_MIRROR_TARGET_SHA256")?;
    let mut conn = connect_target(&target).await?;
    let mut tx = conn.begin().await.map_err(sql_token)?;
    if let (Some(partition), Some(run)) = (partition, run) {
        let sidecar: Option<(bool, String, i64, String)> = sqlx::query_as(
            "SELECT acknowledged,job_task_id::text,job_generation,run_id::text FROM _oo.native_partition_state WHERE source=$1 AND schema_name=$2 AND table_name=$3 AND partition_id=$4 FOR UPDATE"
        ).bind(source).bind(schema).bind(table).bind(partition)
         .fetch_optional(&mut *tx).await.map_err(sql_token)?;
        let Some((acknowledged, stored_job, stored_generation, stored_run)) = sidecar else { return Err("partition_prior_ack"); };
        if acknowledged || stored_job != job || stored_run != run || stored_generation >= generation {
            return Err("partition_prior_ack");
        }
    } else {
        // C4 seals and its sidecar commit atomically. No sidecar means no
        // durable COPY for this task, so its expired mirror can be rebound.
        let sealed: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM _oo.native_partition_state WHERE job_task_id=$1::uuid)"
        ).bind(job).fetch_one(&mut *tx).await.map_err(sql_token)?;
        if sealed { return Err("partition_prior_ack"); }
    }
    let rebound_text: String = sqlx::query_scalar("SELECT jobs.mirror_rebind($1::uuid,$2::jsonb)::text")
        .bind(job).bind(&control_row).fetch_one(&mut *tx).await.map_err(sql_token)?;
    let rebound: Value = serde_json::from_str(&rebound_text).map_err(|_| "jobs_lease_lost")?;
    if rebound["rebound"] != true || rebound["generation"] != generation {
        return Err("jobs_lease_lost");
    }
    let still_live: Option<String> = sqlx::query_scalar(
        "SELECT row_to_json(task)::text FROM jobs.tasks AS task WHERE task.id=$1::uuid AND task.state='running' AND task.lease_until>clock_timestamp()"
    ).bind(job).fetch_optional(&mut control).await.map_err(sql_token)?;
    if still_live.as_deref().and_then(|row| serde_json::from_str::<Value>(row).ok()) != Some(observed) {
        return Err("jobs_lease_lost");
    }
    tx.commit().await.map_err(sql_token)?;
    Ok(generation)
}

async fn ack_partition(document: &Value) -> Result<(), &'static str> {
    let object = document.as_object().ok_or("closed")?;
    const FIELDS: &[&str] = &["protocol", "source_id", "logical_schema", "logical_table",
        "partition_id", "run_id", "batch_id", "payload_digest", "job_task_id",
        "job_generation"];
    if object.len() != FIELDS.len() || object.keys().any(|key| !FIELDS.contains(&key.as_str()))
        || document["protocol"] != "oo.pipeline.native-partition-ack/v1" { return Err("closed"); }
    let source = ident(document, "source_id")?;
    let schema = mapped_name(text_field(document, "logical_schema")?)?;
    let table = mapped_name(text_field(document, "logical_table")?)?;
    let partition = text_field(document, "partition_id")?;
    if partition.is_empty() || partition.len() > 128 || partition.contains('\0') { return Err("closed"); }
    let run = text_field(document, "run_id")?;
    let job = text_field(document, "job_task_id")?;
    if !is_uuid(run) || !is_uuid(job) { return Err("closed"); }
    let batch = batch_token(document)?;
    let payload = hex_field(document, "payload_digest").map_err(|_| "closed")?;
    let generation = document["job_generation"].as_i64().filter(|value| *value >= 1).ok_or("closed")?;
    let control_target = pinned_ack_target("OO_NATIVE_CONTROL_TARGET", "OO_NATIVE_CONTROL_TARGET_SHA256")?;
    let mut control = connect_target(&control_target).await?;
    let completed: Option<bool> = sqlx::query_scalar(
        "SELECT state = 'succeeded' AND generation = $2 FROM jobs.tasks WHERE id = $1::uuid")
        .bind(job).bind(generation).fetch_optional(&mut control).await.map_err(sql_token)?;
    if completed != Some(true) { return Err("jobs_cas_absent"); }
    let target = pinned_ack_target("OO_NATIVE_ACK_TARGET", "OO_NATIVE_ACK_TARGET_SHA256")?;
    let mut conn = connect_target(&target).await?;
    let ack: bool = sqlx::query_scalar(
        "SELECT _oo.native_partition_ack($1,$2,$3,$4,$5::uuid,$6,$7,$8::uuid,$9)")
        .bind(source).bind(schema).bind(table).bind(partition).bind(run)
        .bind(batch).bind(payload).bind(job).bind(generation)
        .fetch_one(&mut conn).await.map_err(sql_token)?;
    if ack { Ok(()) } else { Err("OO010") }
}

fn pinned_ack_target(path_var: &str, digest_var: &str) -> Result<Value, &'static str> {
    let path = std::env::var(path_var).map_err(|_| "control_required")?;
    let digest = std::env::var(digest_var).map_err(|_| "control_required")?;
    if !path.starts_with('/') || !hex64(&digest) { return Err("control_required"); }
    let bytes = fs::read(path).map_err(|_| "control_required")?;
    if bytes.len() > 8192 || hex_bytes(&Sha256::digest(&bytes)) != digest {
        return Err("control_required");
    }
    serde_json::from_slice(&bytes).map_err(|_| "control_required")
}

/// Install `0000` through `0005`, `0009`, then `0010`, on an administrator connection.
///
/// `0009` is the existing jsonb document type script. A document column is oid
/// 3802 typmod -1. This does not invent a lease.
pub async fn install_native_update_cli() -> Result<(), u8> {
    let mut stdin = Vec::new();
    std::io::Read::read_to_end(&mut std::io::stdin(), &mut stdin).map_err(|_| 2u8)?;
    if stdin.is_empty() || stdin.len() > READ_BUDGET {
        eprintln!("oo-pipeline: usage");
        return Err(2);
    }
    let document: Value = serde_json::from_slice(&stdin).map_err(|_| {
        eprintln!("oo-pipeline: usage");
        2u8
    })?;
    let result = install_document(&document).await;
    let mut bytes = serde_json::to_vec(&result).map_err(|_| 1u8)?;
    bytes.push(b'\n');
    std::io::Write::write_all(&mut std::io::stdout(), &bytes).map_err(|_| 1u8)?;
    if result.get("installed").and_then(|value| value.as_bool()) == Some(true) {
        Ok(())
    } else {
        Err(1)
    }
}

/// Administrator-only jobs mirror successor, installed after target jobs hold.
pub async fn install_native_mirror_rebind_cli() -> Result<(), u8> {
    let mut stdin = Vec::new();
    std::io::Read::read_to_end(&mut std::io::stdin(), &mut stdin).map_err(|_| 2u8)?;
    if stdin.is_empty() || stdin.len() > READ_BUDGET { return Err(2); }
    let document: Value = serde_json::from_slice(&stdin).map_err(|_| 2u8)?;
    let result = if document.get("protocol").and_then(Value::as_str) != Some(INSTALL_PROTOCOL) {
        json!({"protocol":"oo.pipeline.native-mirror-rebind-install-result/v1","installed":false,"reason":"invalid_version"})
    } else {
        match document.get("target").ok_or(2u8).and_then(|target| Ok(target.clone())) {
            Ok(target) => match connect_target(&target).await {
                Ok(mut conn) => match target_installer::install_jobs_mirror_rebind(&mut conn).await {
                    Ok(()) => json!({"protocol":"oo.pipeline.native-mirror-rebind-install-result/v1","installed":true,"digest":target_installer::compiled_jobs_receipt_read_digest()}),
                    Err(err) => json!({"protocol":"oo.pipeline.native-mirror-rebind-install-result/v1","installed":false,"reason":install_token(&err),"detail":err.to_string()}),
                },
                Err(reason) => json!({"protocol":"oo.pipeline.native-mirror-rebind-install-result/v1","installed":false,"reason":reason}),
            },
            Err(_) => json!({"protocol":"oo.pipeline.native-mirror-rebind-install-result/v1","installed":false,"reason":"closed"}),
        }
    };
    let mut bytes = serde_json::to_vec(&result).map_err(|_| 1u8)?;
    bytes.push(b'\n');
    std::io::Write::write_all(&mut std::io::stdout(), &bytes).map_err(|_| 1u8)?;
    if result["installed"] == true { Ok(()) } else { Err(1) }
}

/// Install exact-task claiming on the control database under its jobs owner.
pub async fn install_native_claim_selected_cli() -> Result<(), u8> {
    let mut stdin = Vec::new();
    std::io::Read::read_to_end(&mut std::io::stdin(), &mut stdin).map_err(|_| 2u8)?;
    if stdin.is_empty() || stdin.len() > READ_BUDGET { return Err(2); }
    let document: Value = serde_json::from_slice(&stdin).map_err(|_| 2u8)?;
    let result = if document.get("protocol").and_then(Value::as_str) != Some(INSTALL_PROTOCOL) {
        json!({"protocol":"oo.pipeline.native-claim-selected-install-result/v1","installed":false,"reason":"invalid_version"})
    } else {
        match document.get("target") {
            Some(target) => match connect_target(target).await {
                Ok(mut conn) => match target_installer::install_jobs_claim_selected(&mut conn).await {
                    Ok(()) => json!({"protocol":"oo.pipeline.native-claim-selected-install-result/v1","installed":true,"digest":target_installer::compiled_jobs_claim_selected_digest()}),
                    Err(err) => json!({"protocol":"oo.pipeline.native-claim-selected-install-result/v1","installed":false,"reason":install_token(&err),"detail":err.to_string()}),
                },
                Err(reason) => json!({"protocol":"oo.pipeline.native-claim-selected-install-result/v1","installed":false,"reason":reason}),
            },
            None => json!({"protocol":"oo.pipeline.native-claim-selected-install-result/v1","installed":false,"reason":"closed"}),
        }
    };
    let mut bytes = serde_json::to_vec(&result).map_err(|_| 1u8)?;
    bytes.push(b'\n');
    std::io::Write::write_all(&mut std::io::stdout(), &bytes).map_err(|_| 1u8)?;
    if result["installed"] == true { Ok(()) } else { Err(1) }
}

async fn dispatch(document: &Value) -> Value {
    if document.get("protocol").and_then(|value| value.as_str()) == Some(INSTALL_PROTOCOL) {
        return install_document(document).await;
    }
    match execute(document).await {
        Ok(value) => value,
        Err(token) => refused(document, token),
    }
}

fn refused(document: &Value, token: &'static str) -> Value {
    json!({
        "protocol": RESULT_PROTOCOL,
        "load_state": "not_sealed",
        "reason": token,
        "loader_invoked": false,
        "run_id": null,
        "receipt": null,
        "jobs_lease_receipt": null,
        "jobs_observation": jobs_observation(None, "not_invoked"),
        "source_bound": false,
        "source_id": document.get("source_id").cloned().unwrap_or(Value::Null),
        "source_generation": document.get("source_generation").cloned().unwrap_or(Value::Null),
        "freshness_state": null,
        "stage_select_granted": null
    })
}

async fn install_document(document: &Value) -> Value {
    if document.get("protocol").and_then(|value| value.as_str()) != Some(INSTALL_PROTOCOL) {
        return json!({"protocol": RESULT_PROTOCOL, "installed": false, "reason": "invalid_version"});
    }
    let target = document.get("target").cloned().unwrap_or(Value::Null);
    let mut conn = match connect_target(&target).await {
        Ok(conn) => conn,
        Err(token) => {
            return json!({"protocol": RESULT_PROTOCOL, "installed": false, "reason": token});
        }
    };
    if let Err(err) = target_installer::install(&mut conn).await {
        return json!({"protocol": RESULT_PROTOCOL, "installed": false, "reason": "install_failed", "detail": err.to_string()});
    }
    if let Err(err) = target_installer::install_full_import(&mut conn).await {
        return json!({"protocol": RESULT_PROTOCOL, "installed": false, "reason": "install_failed", "detail": err.to_string()});
    }
    if let Err(err) = target_installer::install_catalog_before_empty(&mut conn).await {
        return json!({"protocol": RESULT_PROTOCOL, "installed": false, "reason": install_token(&err), "detail": err.to_string(), "step": "0006-0008"});
    }
    if let Err(err) = target_installer::install_jsonb_document(&mut conn).await {
        return json!({"protocol": RESULT_PROTOCOL, "installed": false, "reason": install_token(&err), "detail": err.to_string(), "step": "0009"});
    }
    match target_installer::install_native_update(&mut conn).await {
        Ok(outcome) => match target_installer::install_native_partition(&mut conn).await {
        Ok(()) => match target_installer::install_native_partition_rebind(&mut conn).await {
        Ok(()) => match target_installer::install_native_interval(&mut conn).await {
        Ok(()) => match target_installer::install_native_olap_partition(&mut conn).await {
        Ok(()) => match target_installer::install_native_pg_transaction_group(&mut conn).await {
        Ok(()) => match target_installer::install_native_empty_catalog(&mut conn).await {
        Ok(()) => match target_installer::install_native_pg_group_receipt(&mut conn).await {
        Ok(()) => match target_installer::install_native_pg_group_prior(&mut conn).await {
        Ok(()) => match target_installer::install_native_pg_group_generation(&mut conn).await {
        Ok(()) => match target_installer::install_native_pg_single_transaction(&mut conn).await {
        Ok(()) => match target_installer::install_native_sqlserver_ct(&mut conn).await {
        Ok(()) => json!({
            "protocol": RESULT_PROTOCOL,
            "installed": true,
            "outcome": outcome.to_string(),
            "digest": target_installer::compiled_native_update_digest(),
            "partition_digest": target_installer::compiled_native_partition_digest(),
            "partition_rebind_digest": target_installer::compiled_native_partition_rebind_digest(),
            "interval_digest": target_installer::compiled_native_interval_digest(),
            "olap_partition_digest": target_installer::compiled_native_olap_partition_digest(),
            "olap_guard_digest": target_installer::compiled_native_olap_guard_digest()
            ,"olap_guard_signature_digest": target_installer::compiled_native_olap_guard_signature_digest()
            ,"pg_transaction_group_digest": target_installer::compiled_native_pg_transaction_group_digest()
            ,"native_empty_catalog_digest": target_installer::compiled_native_empty_catalog_digest()
            ,"pg_group_receipt_digest": target_installer::compiled_native_pg_group_receipt_digest()
            ,"pg_group_prior_digest": target_installer::compiled_native_pg_group_prior_digest()
            ,"pg_group_generation_digest": target_installer::compiled_native_pg_group_generation_digest()
            ,"pg_single_transaction_digest": target_installer::compiled_native_pg_single_transaction_digest()
            ,"sqlserver_ct_digest": target_installer::compiled_native_sqlserver_ct_digest()
        }),
        Err(err) => json!({"protocol": RESULT_PROTOCOL, "installed": false, "reason": install_token(&err), "detail": err.to_string(), "step": "0023"}),
        },
        Err(err) => json!({"protocol": RESULT_PROTOCOL, "installed": false, "reason": install_token(&err), "detail": err.to_string(), "step": "0022"}),
        },
        Err(err) => json!({"protocol": RESULT_PROTOCOL, "installed": false, "reason": install_token(&err), "detail": err.to_string(), "step": "0021"}),
        },
        Err(err) => json!({"protocol": RESULT_PROTOCOL, "installed": false, "reason": install_token(&err), "detail": err.to_string(), "step": "0020"}),
        },
        Err(err) => json!({"protocol": RESULT_PROTOCOL, "installed": false, "reason": install_token(&err), "detail": err.to_string(), "step": "0019"}),
        },
        Err(err) => json!({"protocol": RESULT_PROTOCOL, "installed": false, "reason": install_token(&err), "detail": err.to_string(), "step": "0018"}),
        },
        Err(err) => json!({"protocol": RESULT_PROTOCOL, "installed": false, "reason": install_token(&err), "detail": err.to_string(), "step": "0017"}),
        },
        Err(err) => json!({"protocol": RESULT_PROTOCOL, "installed": false, "reason": install_token(&err), "detail": err.to_string(), "step": "0016"}),
        },
        Err(err) => json!({"protocol": RESULT_PROTOCOL, "installed": false, "reason": install_token(&err), "detail": err.to_string(), "step": "0013"}),
        },
        Err(err) => json!({"protocol": RESULT_PROTOCOL, "installed": false, "reason": install_token(&err), "detail": err.to_string(), "step": "0012"}),
        },
        Err(err) => json!({"protocol": RESULT_PROTOCOL, "installed": false, "reason": install_token(&err), "detail": err.to_string(), "step": "0011"}),
        },
        Err(err) => json!({
            "protocol": RESULT_PROTOCOL,
            "installed": false,
            "reason": install_token(&err),
            "detail": err.to_string()
        }),
    }
}

fn install_token(err: &InstallError) -> &'static str {
    match err {
        InstallError::NotAdministrator => "not_administrator",
        InstallError::ChecksumMismatch { .. } => "OO001",
        InstallError::Refused { .. }
        | InstallError::MissingRelation { .. }
        | InstallError::Database { .. } => "install_failed",
    }
}

async fn execute(document: &Value) -> Result<Value, &'static str> {
    let plan = classify(document)?;
    if plan.transaction_scope.is_some() {
        return Ok(refused(document, "transaction_group_required"));
    }
    if plan.mirror_only {
        let mut conn = connect_target(&plan.target).await?;
        let status = mirror_only(&mut conn, &plan).await?;
        return Ok(mirror_result(&plan, &status));
    }
    if plan.lease_id.is_none() {
        return Ok(refused(document, "lease_required"));
    }
    if !plan.source_bound {
        return Ok(refused(document, "nonadmitted"));
    }
    let mut conn = connect_target(&plan.target).await?;
    let mut tx = conn.begin().await.map_err(|_| "not_ready")?;
    let lease_id = plan.lease_id.as_deref().ok_or("lease_required")?;
    let (lease_receipt, lease_call) =
        match lease_hold(&mut tx, lease_id, plan.lease_generation).await {
            Ok(held) => held,
            Err(token) => {
                let _ = tx.rollback().await;
                return Ok(refused(document, token));
            }
        };
    let existing = match read_batch(&mut tx, &plan).await {
        Ok(existing) => existing,
        Err(token) => {
            let _ = tx.rollback().await;
            return Err(token);
        }
    };
    if let Some(existing) = existing {
        if existing.payload_digest != plan.payload_digest {
            let _ = tx.rollback().await;
            return Ok(refused(document, "OO010"));
        }
        if let Some(scope) = plan.partition_scope.as_ref() {
            let stored_digest: Option<String> = sqlx::query_scalar(
                "SELECT checksum FROM _oo.schema_migrations WHERE version = $1"
            ).bind(target_installer::VERSION_NATIVE_PARTITION_REBIND)
             .fetch_optional(&mut *tx).await.map_err(sql_token)?;
            if stored_digest.as_deref() != Some(target_installer::compiled_native_partition_rebind_digest().as_str()) {
                let _ = tx.rollback().await;
                return Ok(refused(document, "partition_schema_not_ready"));
            }
            let scope_text = serde_json::to_string(scope).map_err(|_| "partition_scope_required")?;
            let status: String = sqlx::query_scalar(
                "SELECT _oo.native_partition_rebind($1,$2,$3,$4,$5::jsonb,$6::uuid,$7,$8,$9,$10::uuid,$11)"
            )
            .bind(&plan.source_id).bind(&plan.schema).bind(&plan.table)
            .bind(scope.get("partition_id").and_then(Value::as_str).ok_or("partition_scope_required")?)
            .bind(&scope_text).bind(&existing.run_id).bind(&plan.batch_id)
            .bind(&plan.payload_digest).bind(existing.target_generation)
            .bind(plan.lease_id.as_deref().ok_or("lease_required")?)
            .bind(plan.lease_generation.ok_or("lease_required")?)
            .fetch_one(&mut *tx).await.map_err(sql_token)?;
            if status == "acknowledged" {
                let _ = tx.rollback().await;
                return Ok(refused(document, "OO010"));
            }
        }
        tx.commit().await.map_err(|_| "not_ready")?;
        let mirror = if plan.mirror {
            Some(
                mirror_call(
                    &mut conn,
                    &plan.source_id,
                    &plan.batch_id,
                    &existing.physical_name,
                )
                .await?,
            )
        } else {
            None
        };
        let jobs = jobs_regclass(&mut conn).await;
        return Ok(replay_result(
            &plan,
            &existing,
            mirror,
            jobs,
            lease_receipt,
            lease_call,
        ));
    }
    if plan.partition_scope.is_none() && let Err(token) = position_ahead(&mut tx, &plan).await {
        let _ = tx.rollback().await;
        return Ok(refused(document, token));
    }
    let applied = match apply_tx(&mut tx, &plan).await {
        Ok(applied) => applied,
        Err(token) => {
            let _ = tx.rollback().await;
            let _ = release(&mut conn, &plan.shard).await;
            return Ok(refused(document, token));
        }
    };
    if let Err(token) = lease_hold(&mut tx, lease_id, plan.lease_generation).await {
        let _ = tx.rollback().await;
        let _ = release(&mut conn, &plan.shard).await;
        return Ok(refused(document, token));
    }
    if tx.commit().await.is_err() {
        let _ = release(&mut conn, &plan.shard).await;
        return Ok(refused(document, "not_sealed"));
    }
    let _ = release(&mut conn, &plan.shard).await;
    let mirror = if plan.mirror {
        match mirror_call(
            &mut conn,
            &plan.source_id,
            &plan.batch_id,
            &applied.physical_name,
        )
        .await
        {
            Ok(status) => Some(status),
            Err(token) => {
                let jobs = jobs_regclass(&mut conn).await;
                let mut result =
                    success_result(&plan, &applied, None, jobs, lease_receipt, lease_call);
                result["mirror"] = json!(token);
                result["mirror_committed"] = json!(false);
                return Ok(result);
            }
        }
    } else {
        None
    };
    let jobs = jobs_regclass(&mut conn).await;
    Ok(success_result(
        &plan,
        &applied,
        mirror,
        jobs,
        lease_receipt,
        lease_call,
    ))
}

fn admitted_plans(document: &Value) -> Result<Vec<UpdatePlan>, &'static str> {
    let object = document.as_object().ok_or("closed")?;
    if object.len() != 2 || document["protocol"] != TRANSACTION_PROTOCOL
        || object.keys().any(|key| key != "protocol" && key != "entries") { return Err("closed"); }
    let requests = document["entries"].as_array().filter(|items| (1..=32).contains(&items.len())).ok_or("closed")?;
    let vector_path = std::env::var("OO_NATIVE_TRANSACTION_REGISTRY_VECTOR").map_err(|_| "nonadmitted")?;
    let vector_sha = std::env::var("OO_NATIVE_TRANSACTION_REGISTRY_VECTOR_SHA256").map_err(|_| "nonadmitted")?;
    let admission_bin = std::env::var("OO_NATIVE_ADMISSION_BIN").map_err(|_| "nonadmitted")?;
    if !vector_path.starts_with('/') || !admission_bin.starts_with('/') || !hex64(&vector_sha) { return Err("nonadmitted"); }
    let bytes = fs::read(&vector_path).map_err(|_| "nonadmitted")?;
    if bytes.is_empty() || bytes.len() > 65_536 || hex_bytes(&Sha256::digest(&bytes)) != vector_sha { return Err("nonadmitted"); }
    let vector: Value = serde_json::from_slice(&bytes).map_err(|_| "nonadmitted")?;
    let vector_object = vector.as_object().ok_or("nonadmitted")?;
    if vector_object.len() != 2 || vector["protocol"] != "oo.pipeline.native-transaction-registry-vector/v1" { return Err("nonadmitted"); }
    let entries = vector["entries"].as_array().filter(|items| items.len() == requests.len()).ok_or("nonadmitted")?;
    let mut plans = Vec::with_capacity(requests.len());
    for (request, entry) in requests.iter().zip(entries) {
        let e = entry.as_object().ok_or("nonadmitted")?;
        if e.len() != 4 || !["logical_schema", "logical_table", "registry_path", "registry_sha256"].iter().all(|key| e.contains_key(*key))
            || request["logical_schema"] != entry["logical_schema"] || request["logical_table"] != entry["logical_table"] { return Err("nonadmitted"); }
        let path = entry["registry_path"].as_str().filter(|value| value.starts_with('/')).ok_or("nonadmitted")?;
        let sha = entry["registry_sha256"].as_str().filter(|value| hex64(value)).ok_or("nonadmitted")?;
        let plan = native_binding::with_operator_env(native_binding::OperatorEnv::Fixed {
            bin: Some(admission_bin.clone()), registry: Some(path.to_string()), registry_sha256: Some(sha.to_string()),
        }, || classify(request))?;
        plans.push(plan);
    }
    Ok(plans)
}

fn transaction_plans(document: &Value) -> Result<(Vec<UpdatePlan>, Value, String, u64), &'static str> {
    let plans = admitted_plans(document)?;
    let first = plans.first().ok_or("closed")?;
    let scope = first.transaction_scope.as_ref().ok_or("transaction_scope_required")?.clone();
    let scope_tables = scope["tables"].as_array().filter(|items| items.len() == plans.len()).ok_or("transaction_scope_required")?;
    let end_lsn = pg_lsn_value(scope["end_lsn"].as_str().ok_or("transaction_scope_required")?)?;
    let scope_digest = hex_bytes(&Sha256::digest(serde_json::to_vec(&scope).map_err(|_| "transaction_scope_required")?));
    let mut prior = None;
    for (plan, table) in plans.iter().zip(scope_tables) {
        let key = (&plan.schema, &plan.table);
        if prior.is_some_and(|old| old >= key) { return Err("transaction_scope_required"); }
        prior = Some(key);
        if !matches!(plan.source_engine.as_str(), "postgres" | "postgresql")
            || plan.capture_method != "c1_cdc" || plan.delete_detection != "native"
            || plan.consistency_grade != "T" || !plan.cross_table || plan.position_codec != "pg-commit-lsn/v1"
            || plan.mode != "incremental" || plan.partition_scope.is_some() || plan.mirror
            || !plan.source_bound || plan.lease_id.is_none() || plan.lease_generation.is_none()
            || plan.transaction_scope.as_ref() != Some(&scope)
            || plan.source_id != first.source_id || plan.target != first.target
            || plan.shard != first.shard || plan.owner != first.owner
            || plan.lease_id != first.lease_id || plan.lease_generation != first.lease_generation
            || plan.config_digest != first.config_digest
            || plan.applied_pos_request != end_lsn.to_string()
            || table["logical_schema"] != plan.schema || table["logical_table"] != plan.table
            || table["source_generation"] != plan.source_generation
            || table["parent_source_generation"] != json!(plan.parent_source_generation)
            || table["arrow_sha256"] != native_arrow::arrow_sha256(&plan.arrow)
            || table["row_count"].as_u64() != Some(if plan.arrow.is_empty() { 0 } else {
                decode_arrow_stream(&plan.arrow).map_err(arrow_token)?.rows as u64
            })
            || table["tombstones_sha256"] != hex_bytes(&Sha256::digest(serde_json::to_vec(&plan.tombstones).map_err(|_| "transaction_scope_required")?))
        { return Err("transaction_scope_required"); }
    }
    if scope["source_id"] != first.source_id || scope["complete"] != true
        || scope["xid"].as_i64().is_none_or(|xid| xid <= 0) { return Err("transaction_scope_required"); }
    Ok((plans, scope, scope_digest, end_lsn))
}

fn pg_lsn_value(value: &str) -> Result<u64, &'static str> {
    let (hi, lo) = value.split_once('/').ok_or("transaction_scope_required")?;
    if hi.is_empty() || hi.len() > 8 || lo.is_empty() || lo.len() > 8
        || !hi.bytes().chain(lo.bytes()).all(|byte| byte.is_ascii_hexdigit()) { return Err("transaction_scope_required"); }
    Ok((u64::from(u32::from_str_radix(hi, 16).map_err(|_| "transaction_scope_required")?) << 32)
        | u64::from(u32::from_str_radix(lo, 16).map_err(|_| "transaction_scope_required")?))
}

async fn completed_snapshot_vector(source: &str, target_database: &str) -> Result<Value, &'static str> {
    let descriptor = pinned_ack_target("OO_NATIVE_CONTROL_TARGET", "OO_NATIVE_CONTROL_TARGET_SHA256")?;
    let mut control = connect_target(&descriptor).await?;
    let raw: Option<String> = sqlx::query_scalar(
        "SELECT w.result::text FROM jw_native.source_progress p \
         JOIN jw_native.watermarks w ON w.binding_id=p.binding_id AND w.task_id=p.task_id \
         JOIN jobs.tasks t ON t.id=p.task_id \
         WHERE p.source_id=$1 AND p.target_database=$2 AND p.source_generation=1 \
         AND w.source_generation=1 AND w.pin=1 AND t.state='succeeded'"
    ).bind(source).bind(target_database).fetch_optional(&mut control).await.map_err(sql_token)?;
    let result: Value = serde_json::from_str(&raw.ok_or("OO030")?).map_err(|_| "OO030")?;
    if result["protocol"] != "oo.jobs.native-baseline-result/v1"
        || result["source_id"] != source || result["source_generation"] != 1
        || result["consistency_grade"] != "S1" || result["cross_table_consistent"] != false
        || result["load_state"] != "awaiting_publish" { return Err("OO030"); }
    let entries = result["entries"].as_array().filter(|entries| (1..=32).contains(&entries.len())).ok_or("OO030")?;
    if entries.iter().any(|entry| entry["source_id"] != source
        || entry["source_generation"] != 1 || entry["source_bound"] != true
        || entry["producer_trust"]["trusted"] != true) { return Err("OO030"); }
    Ok(result["entries"].clone())
}

async fn execute_transaction(document: &Value) -> Result<Value, &'static str> {
    if gtid_entries(document) {
        return execute_gtid_transaction(document).await;
    }
    let (plans, scope, scope_digest, end_lsn) = transaction_plans(document)?;
    let first = &plans[0];
    let xid = scope["xid"].as_i64().ok_or("transaction_scope_required")?;
    let legacy_group_generation = plans.iter().map(|plan| plan.source_generation).min().ok_or("transaction_scope_required")?;
    let job = first.lease_id.as_deref().ok_or("lease_required")?;
    let job_generation = first.lease_generation.ok_or("lease_required")?;
    let mut conn = connect_target(&first.target).await?;
    let shard = first.shard.clone();
    let result = async {
        let mut tx = conn.begin().await.map_err(sql_token)?;
        let (lease_receipt, _) = lease_hold(&mut tx, job, Some(job_generation)).await?;
        let attestation: Option<String> = sqlx::query_scalar(
            "SELECT _oo.native_transaction_read($1,$2,$3::text::numeric)::text"
        ).bind(&first.source_id).bind(xid).bind(end_lsn.to_string())
         .fetch_one(&mut *tx).await.map_err(sql_token)?;
        let (state, entries, target_generation, group_generation) = if let Some(raw) = attestation {
            let saved: Value = serde_json::from_str(&raw).map_err(|_| "not_sealed")?;
            if saved["scope_digest"] != scope_digest || saved["job_task_id"] != job
                || saved["source"] != first.source_id || saved["xid"] != xid
                || saved["end_lsn"].as_u64() != Some(end_lsn) {
                return Err("OO010");
            }
            let entries = saved["entries"].as_array().filter(|rows| rows.len() == plans.len()).ok_or("OO010")?.clone();
            for (plan, entry) in plans.iter().zip(&entries) {
                let existing = transaction_read_batch(&mut tx, plan).await?.ok_or("OO010")?;
                if entry["logical_schema"] != plan.schema || entry["logical_table"] != plan.table
                    || entry["source_generation"] != plan.source_generation
                    || entry["batch_id"] != plan.batch_id || entry["payload_digest"] != plan.payload_digest
                    || entry["run_id"] != existing.run_id || entry["physical_name"] != existing.physical_name
                    || entry["target_generation"] != existing.target_generation { return Err("OO010"); }
            }
            let group_generation = match saved.get("source_generation") {
                None | Some(Value::Null) => legacy_group_generation,
                Some(value) => value.as_i64().filter(|generation| *generation > 0).ok_or("OO010")?,
            };
            ("replay", entries, saved["target_generation"].as_i64().ok_or("OO010")?, group_generation)
        } else {
            let adopted_generation: i64 = sqlx::query_scalar(
                "SELECT _oo.adopt_target_generation($1,$2::uuid)"
            ).bind(&first.shard).bind(&first.owner)
             .fetch_one(&mut *tx).await.map_err(sql_token)?;
            sqlx::query("SELECT _oo.full_import_bind_session($1,$2::uuid,$3)")
                .bind(&first.shard).bind(&first.owner).bind(adopted_generation)
                .execute(&mut *tx).await.map_err(sql_token)?;
            let prior_generation: i64 = sqlx::query_scalar(
                "SELECT _oo.native_pg_group_prior($1,$2::uuid,$3,$4)"
            ).bind(&first.shard).bind(&first.owner).bind(adopted_generation)
             .bind(&first.source_id).fetch_one(&mut *tx).await.map_err(sql_token)?;
            let snapshot_prior: bool = sqlx::query_scalar(
                "SELECT _oo.native_pg_prior_is_snapshot($1,$2::uuid,$3,$4)"
            ).bind(&first.shard).bind(&first.owner).bind(adopted_generation)
             .bind(&first.source_id).fetch_one(&mut *tx).await.map_err(sql_token)?;
            if snapshot_prior {
                let database = text_field(&first.target, "database")?;
                let entries = completed_snapshot_vector(&first.source_id, database).await?;
                let complete: bool = sqlx::query_scalar(
                    "SELECT _oo.native_pg_baseline_receipts($1,$2::uuid,$3,$4,$5,$6::jsonb)"
                ).bind(&first.shard).bind(&first.owner).bind(adopted_generation)
                 .bind(&first.source_id).bind(&first.config_digest)
                 .bind(serde_json::to_string(&entries).map_err(|_| "OO030")?)
                 .fetch_one(&mut *tx).await.map_err(sql_token)?;
                if !complete { return Err("OO030"); }
            }
            let group_generation = prior_generation.checked_add(1).ok_or("stale_run")?;
            for plan in &plans {
                if transaction_read_batch(&mut tx, plan).await?.is_some() { return Err("OO010"); }
            }
            let mut order: Vec<usize> = (0..plans.len()).collect();
            // Table generations may differ; the full-source transaction advances
            // one global generation. Seal that minimum table generation last.
            order.sort_by_key(|index| std::cmp::Reverse(plans[*index].source_generation));
            let mut entries = vec![Value::Null; plans.len()];
            let mut target_generation = None;
            for index in order {
                let plan = &plans[index];
                position_ahead(&mut tx, plan).await?;
                let applied = apply_tx(&mut tx, plan).await?;
                if target_generation.is_some_and(|generation| generation != applied.target_generation) { return Err("OO010"); }
                target_generation = Some(applied.target_generation);
                entries[index] = json!({
                    "logical_schema":plan.schema,"logical_table":plan.table,
                    "source_generation":plan.source_generation,"target_generation":applied.target_generation,
                    "physical_name":applied.physical_name,"batch_id":plan.batch_id,
                    "payload_digest":plan.payload_digest,"run_id":applied.run_id
                });
            }
            let generation = target_generation.ok_or("not_sealed")?;
            if generation != adopted_generation { return Err("OO010"); }
            let entries_text = serde_json::to_string(&entries).map_err(|_| "not_sealed")?;
            sqlx::query("SELECT _oo.native_transaction_commit_global($1,$2::uuid,$3,$4,$5,$6::text::numeric,$7,$8::jsonb,$9::uuid,$10)")
                .bind(&first.shard).bind(&first.owner).bind(generation).bind(&first.source_id)
                .bind(xid).bind(end_lsn.to_string()).bind(&scope_digest).bind(&entries_text).bind(job).bind(group_generation)
                .execute(&mut *tx).await.map_err(sql_token)?;
            ("awaiting_publish", entries, generation, group_generation)
        };
        lease_hold(&mut tx, job, Some(job_generation)).await?;
        tx.commit().await.map_err(sql_token)?;
        Ok(json!({
            "protocol":TRANSACTION_RESULT_PROTOCOL,"load_state":state,"reason":null,
            "source_id":first.source_id,"xid":xid,"end_lsn":end_lsn.to_string(),
            "source_generation":group_generation,
            "scope_digest":scope_digest,"job_task_id":job,"job_generation":job_generation,
            "target_generation":target_generation,"entries":entries,
            "jobs_lease_receipt":lease_receipt,"source_bound":true
        }))
    }.await;
    let _ = release(&mut conn, &shard).await;
    result
}

fn gtid_entries(document: &Value) -> bool {
    document.get("entries").and_then(Value::as_array).is_some_and(|entries| {
        entries.iter().any(|entry| {
            matches!(
                entry.get("position_codec").and_then(Value::as_str),
                Some("mariadb-gtid/v1" | "mysql-gtid/v1")
            )
        })
    })
}

async fn execute_gtid_transaction(document: &Value) -> Result<Value, &'static str> {
    let identity = one_gtid(document)?;
    let plans = admitted_plans(document)?;
    gtid_plans_match(&plans, &identity)?;
    let first = &plans[0];
    let scope_digest = gtid_scope_digest(&first.source_id, &identity);
    let group_generation = plans
        .iter()
        .map(|plan| plan.source_generation)
        .min()
        .ok_or("transaction_scope_required")?;
    let job = first.lease_id.as_deref().ok_or("lease_required")?;
    let job_generation = first.lease_generation.ok_or("lease_required")?;
    let mut conn = connect_target(&first.target).await?;
    let shard = first.shard.clone();
    let result = async {
        let mut tx = conn.begin().await.map_err(sql_token)?;
        let (lease_receipt, _) = lease_hold(&mut tx, job, Some(job_generation)).await?;
        let attestation: Option<String> = sqlx::query_scalar(
            "SELECT _oo.native_gtid_transaction_read($1,$2,$3,$4,$5::text::numeric)::text",
        )
        .bind(&first.source_id)
        .bind(&identity.codec)
        .bind(&identity.lineage)
        .bind(identity.epoch)
        .bind(&identity.applied_pos)
        .fetch_one(&mut *tx)
        .await
        .map_err(sql_token)?;
        let (state, entries, target_generation) = if let Some(raw) = attestation {
            let saved: Value = serde_json::from_str(&raw).map_err(|_| "not_sealed")?;
            let saved_pos = match &saved["applied_pos"] {
                Value::String(text) => text.clone(),
                Value::Number(number) => number.to_string(),
                _ => return Err("OO010"),
            };
            if saved["scope_digest"] != scope_digest
                || saved["job_task_id"] != job
                || saved["source"] != first.source_id
                || saved["position_codec"] != identity.codec
                || saved["position_lineage"] != identity.lineage
                || saved["source_epoch"].as_i64() != Some(i64::from(identity.epoch))
                || saved_pos != identity.applied_pos
                || saved["txn_id"] != identity.txn_id
            {
                return Err("OO010");
            }
            let entries = saved["entries"]
                .as_array()
                .filter(|rows| rows.len() == plans.len())
                .ok_or("OO010")?
                .clone();
            for (plan, entry) in plans.iter().zip(&entries) {
                let existing = transaction_read_batch(&mut tx, plan).await?.ok_or("OO010")?;
                if entry["logical_schema"] != plan.schema
                    || entry["logical_table"] != plan.table
                    || entry["source_generation"] != plan.source_generation
                    || entry["batch_id"] != plan.batch_id
                    || entry["payload_digest"] != plan.payload_digest
                    || entry["run_id"] != existing.run_id
                    || entry["physical_name"] != existing.physical_name
                    || entry["target_generation"] != existing.target_generation
                {
                    return Err("OO010");
                }
            }
            (
                "replay",
                entries,
                saved["target_generation"].as_i64().ok_or("OO010")?,
            )
        } else {
            let adopted_generation: i64 = sqlx::query_scalar(
                "SELECT _oo.adopt_target_generation($1,$2::uuid)",
            )
            .bind(&first.shard)
            .bind(&first.owner)
            .fetch_one(&mut *tx)
            .await
            .map_err(sql_token)?;
            sqlx::query("SELECT _oo.full_import_bind_session($1,$2::uuid,$3)")
                .bind(&first.shard)
                .bind(&first.owner)
                .bind(adopted_generation)
                .execute(&mut *tx)
                .await
                .map_err(sql_token)?;
            for plan in &plans {
                if transaction_read_batch(&mut tx, plan).await?.is_some() {
                    return Err("OO010");
                }
            }
            let mut order: Vec<usize> = (0..plans.len()).collect();
            order.sort_by_key(|index| std::cmp::Reverse(plans[*index].source_generation));
            let mut entries = vec![Value::Null; plans.len()];
            let mut target_generation = None;
            for index in order {
                let plan = &plans[index];
                position_ahead(&mut tx, plan).await?;
                let applied = apply_tx(&mut tx, plan).await?;
                if target_generation.is_some_and(|generation| generation != applied.target_generation) {
                    return Err("OO010");
                }
                target_generation = Some(applied.target_generation);
                entries[index] = json!({
                    "logical_schema":plan.schema,"logical_table":plan.table,
                    "source_generation":plan.source_generation,"target_generation":applied.target_generation,
                    "physical_name":applied.physical_name,"batch_id":plan.batch_id,
                    "payload_digest":plan.payload_digest,"run_id":applied.run_id
                });
            }
            let generation = target_generation.ok_or("not_sealed")?;
            if generation != adopted_generation {
                return Err("OO010");
            }
            let entries_text = serde_json::to_string(&entries).map_err(|_| "not_sealed")?;
            sqlx::query(
                "SELECT _oo.native_gtid_transaction_commit($1,$2::uuid,$3,$4,$5,$6,$7,$8::text::numeric,$9,$10,$11::jsonb,$12::uuid,$13)",
            )
            .bind(&first.shard)
            .bind(&first.owner)
            .bind(generation)
            .bind(&first.source_id)
            .bind(&identity.codec)
            .bind(&identity.lineage)
            .bind(identity.epoch)
            .bind(&identity.applied_pos)
            .bind(&identity.txn_id)
            .bind(&scope_digest)
            .bind(&entries_text)
            .bind(job)
            .bind(group_generation)
            .execute(&mut *tx)
            .await
            .map_err(sql_token)?;
            ("awaiting_publish", entries, generation)
        };
        lease_hold(&mut tx, job, Some(job_generation)).await?;
        tx.commit().await.map_err(sql_token)?;
        let head = &document["entries"][0];
        Ok(json!({
            "protocol":TRANSACTION_RESULT_PROTOCOL,"load_state":state,"reason":null,
            "source_id":first.source_id,
            "position_codec":identity.codec,"position_lineage":identity.lineage,
            "source_epoch":identity.epoch,"applied_pos":identity.applied_pos,"txn_id":identity.txn_id,
            "source_generation":group_generation,
            "scope_digest":scope_digest,"job_task_id":job,"job_generation":job_generation,
            "target_generation":target_generation,"entries":entries,
            "jobs_lease_receipt":lease_receipt,"source_bound":true,
            "ack_allowed":false,
            "resume_file":head.get("resume_file").and_then(Value::as_str),
            "resume_pos":head.get("resume_pos").and_then(Value::as_str)
        }))
    }
    .await;
    let _ = release(&mut conn, &shard).await;
    result
}

async fn transaction_read_batch(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, plan: &UpdatePlan,
) -> Result<Option<StoredBatch>, &'static str> {
    let raw: Option<String> = sqlx::query_scalar("SELECT _oo.native_update_read_batch($1,$2,$3,$4)")
        .bind(&plan.source_id).bind(&plan.schema).bind(&plan.table).bind(&plan.batch_id)
        .fetch_one(&mut **tx).await.map_err(sql_token)?;
    let Some(raw) = raw else { return Ok(None); };
    let value: Value = serde_json::from_str(&raw).map_err(|_| "not_sealed")?;
    Ok(Some(StoredBatch {
        payload_digest: value["payload_digest"].as_str().ok_or("not_sealed")?.to_string(),
        run_id: value["run_id"].as_str().ok_or("not_sealed")?.to_string(),
        target_generation: value["target_generation"].as_i64().ok_or("not_sealed")?,
        physical_name: value["physical_name"].as_str().ok_or("not_sealed")?.to_string(),
        row_count: value["row_count"].as_i64().ok_or("not_sealed")?,
    }))
}

struct StoredBatch {
    payload_digest: String,
    run_id: String,
    target_generation: i64,
    physical_name: String,
    row_count: i64,
}

struct Applied {
    run_id: String,
    target_generation: i64,
    physical_name: String,
    row_count: i64,
    tombstone_count: usize,
    receipt: String,
    stage_select_granted: Option<bool>,
    stage_freshness_select: Option<bool>,
}

struct UpdatePlan {
    source_engine: String,
    source_id: String,
    source_generation: i64,
    parent_source_generation: Option<i64>,
    schema: String,
    table: String,
    batch_id: String,
    capture_method: String,
    delete_detection: String,
    consistency_grade: String,
    cross_table: bool,
    position_codec: String,
    position_lineage: String,
    applied_pos_request: String,
    ct_prior_position: Option<String>,
    config_digest: String,
    schema_digest: String,
    type_digest: String,
    input_digest: String,
    audit_digest: String,
    columns: Vec<CopyColumnDecl>,
    /// Source logical names before `mapping_ident`. `prepare_table` maps these
    /// itself. Passing the finished `q_` ident encodes it a second time, and
    /// COPY then names columns the shadow table does not have.
    column_sources: Vec<String>,
    column_idents: Vec<String>,
    arrow: Vec<u8>,
    tombstones: Vec<String>,
    truth: Option<Value>,
    shard: String,
    owner: String,
    target: Value,
    mirror: bool,
    mirror_only: bool,
    mirror_physical: String,
    payload_digest: String,
    source_bound: bool,
    producer_trust: Value,
    trusted_fields: Vec<native_binding::BoundField>,
    mode: &'static str,
    key_indexes: Vec<usize>,
    key_idents: Vec<String>,
    shape: &'static str,
    derived_semantics: Option<Value>,
    lease_id: Option<String>,
    lease_generation: Option<i64>,
    partition_scope: Option<Value>,
    transaction_scope: Option<Value>,
    previous_ack_exists: bool,
    previous_signature: Option<String>,
    previous_ack_auto: bool,
}

fn classify(document: &Value) -> Result<UpdatePlan, &'static str> {
    let protocol = document.get("protocol").and_then(|value| value.as_str());
    let partition_mode = protocol == Some(PARTITION_PROTOCOL);
    if protocol != Some(REQUEST_PROTOCOL) && !partition_mode {
        return Err("invalid_version");
    }
    if document
        .get("mirror_only")
        .and_then(|value| value.as_bool())
        == Some(true)
    {
        return classify_mirror(document);
    }
    let source_engine = token(document, "source_engine")?;
    let source_id = ident(document, "source_id")?;
    let source_generation = document
        .get("source_generation")
        .and_then(|value| value.as_i64())
        .filter(|value| *value >= 1)
        .ok_or("stale_run")?;
    let parent = match document.get("parent_source_generation") {
        None | Some(Value::Null) => None,
        Some(value) => Some(value.as_i64().ok_or("stale_run")?),
    };
    match parent {
        None if source_generation != 1 => return Err("stale_run"),
        Some(value) if value.checked_add(1) != Some(source_generation) => return Err("stale_run"),
        _ => {}
    }
    let derived_semantics = bind_oracle_contract(document)?;
    let lease_id = optional_lease_id(document)?;
    let lease_generation = optional_lease_generation(document)?;
    let full_snapshot = document
        .get("full_snapshot")
        .and_then(|value| value.as_bool());
    if source_engine == "starrocks" {
        let closed = match document.get("window_closed") {
            Some(Value::Bool(value)) => *value,
            _ => return Err("upsert_window_unsupported"),
        };
        let upsert = document
            .get("upsert_supported")
            .and_then(|value| value.as_bool())
            == Some(true);
        let snapshot = full_snapshot == Some(true);
        if !closed || !(upsert || snapshot || partition_mode) {
            return Err("upsert_window_unsupported");
        }
    }
    let audit_digest = hex_field(document, "audit_digest").map_err(|_| "no_audit")?;
    let config_digest = hex_field(document, "config_digest").map_err(digest_token)?;
    let schema_digest = hex_field(document, "schema_digest").map_err(digest_token)?;
    let type_digest = hex_field(document, "type_digest").map_err(digest_token)?;
    let input_digest = hex_field(document, "input_digest").map_err(digest_token)?;
    let capture_method = closed(
        document,
        "capture_method",
        &[
            "c1_cdc",
            "c2_change_feed",
            "c3_cursor",
            "c4_partition",
            "c5_diff",
        ],
    )?;
    let delete_detection = closed(
        document,
        "delete_detection",
        &["native", "partition_replace", "reconcile", "none"],
    )?;
    if !partition_mode && (capture_method == "c4_partition" || delete_detection == "partition_replace") {
        return Err("partition_scope_required");
    }
    if partition_mode && (!matches!(source_engine.as_str(), "clickhouse" | "doris" | "starrocks") || capture_method != "c4_partition"
        || delete_detection != "partition_replace") {
        return Err("partition_scope_required");
    }
    let consistency_grade = closed(document, "consistency_grade", &["T", "S1", "S0", "B"])?;
    let cross_table = document
        .get("cross_table_consistent")
        .and_then(|value| value.as_bool())
        .ok_or("consistency")?;
    if consistency_grade != "T" && cross_table {
        return Err("consistency");
    }
    let position_codec = closed(
        document,
        "position_codec",
        &[
            "pg-commit-lsn/v1",
            "mssql-ct-version/v1",
            "mysql-binlog-pos/v1",
            "mysql-gtid/v1",
            "mariadb-gtid/v1",
            "batch-cursor-us/v1",
            "snapshot-zero/v1",
            "partition-signature/v1",
        ],
    )?;
    let position_lineage = lineage(document)?;
    if partition_mode != (position_codec == "partition-signature/v1") {
        return Err("partition_scope_required");
    }
    let applied_pos_request = if partition_mode { "0".to_string() } else { digits(document, "applied_pos")? };
    if position_codec == "mariadb-gtid/v1" {
        if source_engine != "mariadb" {
            return Err("gtid_family");
        }
        if capture_method != "c1_cdc"
            || consistency_grade != "T"
            || !maria_stored(&position_lineage, &applied_pos_request)
        {
            return Err("gtid_identity");
        }
    }
    if position_codec == "mysql-gtid/v1" {
        if source_engine != "mysql" {
            return Err("gtid_family");
        }
        if capture_method != "c1_cdc"
            || consistency_grade != "T"
            || mysql_lineage(&position_lineage).is_none()
            || !mysql_gno(&applied_pos_request)
        {
            return Err("gtid_identity");
        }
    }
    if source_engine == "mariadb" && position_codec == "mysql-binlog-pos/v1" {
        return Err("binlog_pos_not_durable");
    }
    let batch_id = batch_token(document)?;
    let schema = mapped_name(text_field(document, "logical_schema")?)?;
    let table = mapped_name(text_field(document, "logical_table")?)?;
    let (columns, column_sources) = parse_columns(document)?;
    if columns.is_empty() {
        return Err("bad_type");
    }
    let (key_indexes, shape) = key_plan(
        document,
        &columns,
        &delete_detection,
        full_snapshot == Some(true),
    )?;
    if partition_mode && (shape != "partition" || full_snapshot == Some(true)) {
        return Err("partition_scope_required");
    }
    let arrow = read_arrow(document)?;
    if !arrow.is_empty() {
        check_arrow_declaration(&arrow, &columns)?;
        refuse_bare_document_double(&arrow, &columns)?;
    }
    let admission = bind_admission(document, &arrow)?;
    if source_engine == "sqlserver" || position_codec == "mssql-ct-version/v1" {
        if !admission.source_bound { return Err("nonadmitted"); }
        let registry: Value = serde_json::from_slice(&read_registry(document)?).map_err(|_| "nonadmitted")?;
        let binding = registry.pointer("/bindings/0").ok_or("nonadmitted")?;
        let capture_bytes = read_capped(Path::new(text_field(binding, "capture_receipt_path")?)).map_err(|_| "nonadmitted")?;
        if hex_bytes(&Sha256::digest(&capture_bytes)) != text_field(binding, "capture_receipt_sha256")? { return Err("digest_mismatch"); }
        let capture: Value = serde_json::from_slice(&capture_bytes).map_err(|_| "nonadmitted")?;
        validate_mssql_capture(document, &capture, &arrow)?;
    }
    let partition_scope = if partition_mode {
        let scope = admission.partition_scope.clone().ok_or("partition_scope_required")?;
        if scope["source_id"] != source_id || scope["source_generation"] != source_generation
            || scope["logical_schema"] != schema || scope["logical_table"] != table
            || scope["schema_digest"] != schema_digest || scope["type_digest"] != type_digest
            || scope["arrow_sha256"] != native_arrow::arrow_sha256(&arrow) {
            return Err("partition_scope_required");
        }
        Some(scope)
    } else { None };
    if arrow.is_empty() && parent.is_none() {
        return Err("empty");
    }
    let tombstones = text_list(document, "tombstones")?;
    if partition_mode && !tombstones.is_empty() {
        return Err("partition_scope_required");
    }
    let previous_ack_auto = partition_mode && matches!(source_engine.as_str(), "doris" | "starrocks");
    if partition_mode && !previous_ack_auto && document.get("row_identity").and_then(Value::as_str) != Some("unique") {
        return Err("partition_scope_required");
    }
    if previous_ack_auto {
        let scope = partition_scope.as_ref().ok_or("partition_scope_required")?;
        validate_olap_route(document, scope, &columns, &key_indexes, &arrow, &source_engine)?;
    }
    if !tombstones_fit(&tombstones, &columns, &key_indexes, shape) {
        return Err("bad_type");
    }
    refuse_upsert_conflict(&arrow, &tombstones, &key_indexes, shape)?;
    if !partition_mode && parent.is_some() && shape != "snapshot" && decimal_is_zero(&applied_pos_request) {
        return Err("expired");
    }
    let (consistency_grade, cross_table) =
        observed_grade(&admission, &consistency_grade, cross_table)?;
    let truth = document
        .get("truth")
        .cloned()
        .filter(|value| !value.is_null());
    if let Some(rows) = truth.as_ref()
        && !rows.is_array()
    {
        return Err("cell_mismatch");
    }
    let target = document.get("target").cloned().ok_or("not_ready")?;
    let shard = ident(&target, "shard")?;
    let owner = text_field(&target, "owner")?;
    if !is_uuid(owner) {
        return Err("not_ready");
    }
    let column_idents: Vec<String> = columns
        .iter()
        .map(|column| column.logical_name.clone())
        .collect();
    let key_idents: Vec<String> = key_indexes
        .iter()
        .map(|&index| column_idents[index].clone())
        .collect();
    let mode = if shape == "snapshot" || parent.is_none() {
        "full"
    } else {
        "incremental"
    };
    let previous_ack_exists = if previous_ack_auto {
        if document.get("previous_ack_exists").is_some() || document.get("previous_signature").is_some() {
            return Err("partition_scope_required");
        }
        false
    } else if partition_mode {
        document.get("previous_ack_exists").and_then(Value::as_bool).ok_or("partition_scope_required")?
    } else { false };
    let previous_signature = if previous_ack_auto { None } else if partition_mode {
        match document.get("previous_signature") {
            None | Some(Value::Null) => None,
            Some(Value::String(value)) if !value.is_empty() => Some(value.clone()),
            _ => return Err("partition_scope_required"),
        }
    } else { None };
    let mut payload_digest = payload_digest(
        &source_id,
        &schema,
        &table,
        source_generation,
        parent,
        &batch_id,
        &arrow,
        &tombstones,
        &column_idents,
        &config_digest,
        &schema_digest,
        &type_digest,
        &input_digest,
        &consistency_grade,
        &applied_pos_request,
        &audit_digest,
        truth.as_ref(),
    );
    if let Some(scope) = partition_scope.as_ref() {
        let scope_bytes = if previous_ack_auto {
            serde_json::to_vec(&(scope, "target-prior/v1")).map_err(|_| "partition_scope_required")?
        } else {
            serde_json::to_vec(&(scope, previous_ack_exists, &previous_signature)).map_err(|_| "partition_scope_required")?
        };
        let mut hashed = Sha256::new();
        hashed.update(payload_digest.as_bytes());
        hashed.update(scope_bytes);
        payload_digest = hex_bytes(&hashed.finalize());
    }
    Ok(UpdatePlan {
        source_engine,
        source_id,
        source_generation,
        parent_source_generation: parent,
        schema,
        table,
        batch_id,
        capture_method,
        delete_detection,
        consistency_grade,
        cross_table,
        position_codec: position_codec.clone(),
        position_lineage,
        ct_prior_position: if position_codec == "mssql-ct-version/v1" && mode == "incremental" {
            document["sqlserver_prior_position"]["tables"].as_array().and_then(|tables| tables.iter().find(|t| t["schema"] == document["logical_schema"] && t["name"] == document["logical_table"]))
                .and_then(|table| table["version"].as_i64()).map(|version| version.to_string())
        } else { None },
        applied_pos_request,
        config_digest,
        schema_digest,
        type_digest,
        input_digest,
        audit_digest,
        columns,
        column_sources,
        column_idents,
        arrow,
        tombstones,
        truth,
        shard,
        owner: owner.to_string(),
        target,
        mirror: document
            .get("mirror")
            .and_then(|value| value.as_bool())
            .unwrap_or(true),
        mirror_only: false,
        mirror_physical: String::new(),
        payload_digest,
        source_bound: admission.source_bound,
        producer_trust: admission.producer_trust,
        trusted_fields: admission.trusted_fields,
        mode,
        key_indexes,
        key_idents,
        shape,
        derived_semantics,
        lease_id,
        lease_generation,
        partition_scope,
        transaction_scope: admission.transaction_scope,
        previous_ack_exists,
        previous_signature,
        previous_ack_auto,
    })
}

fn classify_mirror(document: &Value) -> Result<UpdatePlan, &'static str> {
    let source_id = ident(document, "source_id")?;
    let batch_id = batch_token(document)?;
    let physical = text_field(document, "physical_name")?;
    if quote_physical_name(physical).is_err() || !physical.starts_with("pt_") {
        return Err("physical");
    }
    let target = document.get("target").cloned().ok_or("not_ready")?;
    let shard = ident(&target, "shard")?;
    let owner = text_field(&target, "owner")?.to_string();
    Ok(UpdatePlan {
        source_engine: String::new(),
        source_id,
        source_generation: 0,
        parent_source_generation: None,
        schema: String::new(),
        table: String::new(),
        batch_id,
        capture_method: String::new(),
        delete_detection: String::new(),
        consistency_grade: String::new(),
        cross_table: false,
        position_codec: String::new(),
        position_lineage: String::new(),
        applied_pos_request: String::new(),
        ct_prior_position: None,
        config_digest: String::new(),
        schema_digest: String::new(),
        type_digest: String::new(),
        input_digest: String::new(),
        audit_digest: String::new(),
        columns: Vec::new(),
        column_sources: Vec::new(),
        column_idents: Vec::new(),
        arrow: Vec::new(),
        tombstones: Vec::new(),
        truth: None,
        shard,
        owner,
        target,
        mirror: true,
        mirror_only: true,
        mirror_physical: physical.to_string(),
        payload_digest: String::new(),
        source_bound: false,
        producer_trust: producer_trust_absent(),
        trusted_fields: Vec::new(),
        mode: "incremental",
        key_indexes: Vec::new(),
        key_idents: Vec::new(),
        shape: "unique",
        derived_semantics: None,
        lease_id: None,
        lease_generation: None,
        partition_scope: None,
        transaction_scope: None,
        previous_ack_exists: false,
        previous_signature: None,
        previous_ack_auto: false,
    })
}

struct Admission {
    source_bound: bool,
    producer_trust: Value,
    trusted_fields: Vec<native_binding::BoundField>,
    partition_scope: Option<Value>,
    transaction_scope: Option<Value>,
    consistency: Option<String>,
    cross_table: Option<bool>,
}

fn unbound_admission() -> Admission {
    Admission {
        source_bound: false,
        producer_trust: producer_trust_absent(),
        trusted_fields: Vec::new(),
        partition_scope: None,
        transaction_scope: None,
        consistency: None,
        cross_table: None,
    }
}

fn observed_grade(
    admission: &Admission,
    client_grade: &str,
    client_cross: bool,
) -> Result<(String, bool), &'static str> {
    if admission.source_bound {
        let grade = admission.consistency.clone().ok_or("consistency")?;
        let cross = admission.cross_table.ok_or("consistency")?;
        if grade != client_grade || cross != client_cross {
            return Err("consistency");
        }
        return Ok((grade, cross));
    }
    if client_grade == "T" || client_cross {
        return Err("consistency");
    }
    Ok((client_grade.to_string(), client_cross))
}

fn decimal_is_zero(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte == b'0')
}

/// Binding of one load document to a producer capture receipt.
///
/// `source_bound` is true only when the server registry, catalog, driver,
/// audit, and Arrow bytes agree. A caller-written receipt is not that
/// agreement. `admission_expect` does not supply the identity.
#[derive(Clone, Debug)]
pub struct TrustedCapture {
    /// Producer file matched the load document.
    pub source_bound: bool,
    /// Where the receipt bytes were read, and that a self-equal digest was not accepted.
    pub producer_trust: Value,
}

/// Bind `document` to the producer receipt named by `admission_path`.
///
/// 74 calls this before `import-native-update`. A matching historical receipt
/// stays `source_bound` false. Mismatched engine, generation, or digests
/// return before any target write.
pub fn trusted_capture(document: &Value, arrow: &[u8]) -> Result<TrustedCapture, &'static str> {
    let admission = bind_admission(document, arrow)?;
    Ok(TrustedCapture {
        source_bound: admission.source_bound,
        producer_trust: admission.producer_trust,
    })
}

fn producer_trust_absent() -> Value {
    json!({
        "entry": "absent",
        "trusted": false,
        "self_equal_digest": false,
        "receipt_sha256": null,
        "document_bound": false
    })
}

fn bind_admission(document: &Value, arrow: &[u8]) -> Result<Admission, &'static str> {
    let Some(path) = document
        .get("admission_path")
        .and_then(|value| value.as_str())
    else {
        return Ok(unbound_admission());
    };
    expect_matches_document(document)?;
    let bytes = read_capped(Path::new(path)).map_err(|_| "digest_missing")?;
    let receipt_sha256 = hex_bytes(&Sha256::digest(&bytes));
    if receipt_sha256 == native_binding::HISTORICAL_ADMISSION_RECEIPT_SHA256 {
        return historical_admission(document, &bytes, &receipt_sha256);
    }
    let parsed: Value = serde_json::from_slice(&bytes).map_err(|_| "nonadmitted")?;
    let protocol = parsed
        .get("schema")
        .and_then(|value| value.as_str())
        .or_else(|| parsed.get("protocol").and_then(|value| value.as_str()))
        .unwrap_or("");
    if protocol != native_binding::CAPTURE_RECEIPT_PROTOCOL {
        if !protocol.is_empty()
            && parsed
                .get("source_engine")
                .and_then(|value| value.as_str())
                .is_some()
        {
            return bind_producer_catalog(document, arrow, &parsed, &receipt_sha256, protocol);
        }
        return Err("nonadmitted");
    }
    bind_document_to_producer(document, &parsed)?;
    let arrows = string_list(&parsed, "arrow_sha256")?;
    let file_hash = native_arrow::arrow_sha256(arrow);
    if !arrow.is_empty() && !arrows.iter().any(|item| item == &file_hash) {
        return Err("digest_mismatch");
    }
    if arrow.is_empty() {
        let _ = hex_ref(&parsed, "arrow_schema_sha256")?;
    } else {
        let schema_hash =
            native_arrow::arrow_schema_sha256(arrow).map_err(|_| "digest_mismatch")?;
        if hex_ref(&parsed, "arrow_schema_sha256")? != schema_hash {
            return Err("digest_mismatch");
        }
    }
    let declared = document
        .get("admission_expect")
        .and_then(|value| value.get("declared_receipt_sha256"))
        .and_then(|value| value.as_str());
    if declared_is_self(document, declared, &receipt_sha256) {
        return Err("digest_mismatch");
    }
    if declared.is_some_and(|value| value != receipt_sha256) {
        return Err("digest_mismatch");
    }
    require_server_registration(document, arrow, &bytes)
}

fn historical_admission(
    document: &Value,
    bytes: &[u8],
    receipt_sha256: &str,
) -> Result<Admission, &'static str> {
    let engine = text_field(document, "source_engine").map_err(|_| "cross_source")?;
    let source_id = text_field(document, "source_id").map_err(|_| "cross_source")?;
    let generation = document
        .get("source_generation")
        .and_then(|value| value.as_i64())
        .unwrap_or(0);
    let arrows = Vec::new();
    let capture = CaptureReceipt {
        source_engine: engine,
        source_id,
        source_generation: generation,
        snapshot_kind: "none",
        snapshot_value: "none",
        run_id: None,
        schema_digest: "",
        type_digest: "",
        config_digest: "",
        input_digest: "",
        catalog_sha256: "",
        registry_sha256: "",
        arrow_sha256: &arrows,
        driver_digest: "",
        arrow_schema_sha256: "",
        declared_receipt_sha256: None,
    };
    match native_binding::check_admission_receipt(bytes, &capture)? {
        native_binding::AdmissionReceiptKind::HistoricalReadonly => Ok(Admission {
            source_bound: false,
            trusted_fields: Vec::new(),
            partition_scope: None,
            transaction_scope: None,
            producer_trust: json!({
                "entry": "historical_readonly",
                "trusted": false,
                "self_equal_digest": false,
                "receipt_sha256": receipt_sha256,
                "document_bound": false
            }),
            consistency: None,
            cross_table: None,
        }),
        native_binding::AdmissionReceiptKind::CaptureMatched { .. } => Err("cross_source"),
    }
}

/// The load document's engine, id, generation, digests, and snapshot must
/// equal the producer file. `admission_expect` is not read here.
///
/// A public capture receipt does not repeat `audit_digest`. When that field
/// is absent, [`open_registration`] still requires the request hash to equal
/// the registry `capture_receipt_sha256`, and that file is hashed. A present
/// receipt value that differs is `digest_mismatch`.
fn bind_document_to_producer(document: &Value, receipt: &Value) -> Result<(), &'static str> {
    if receipt.get("schema").and_then(|value| value.as_str())
        != Some(native_binding::CAPTURE_RECEIPT_PROTOCOL)
    {
        return Err("nonadmitted");
    }
    let engine = text_field(document, "source_engine").map_err(|_| "cross_source")?;
    let source_id = text_field(document, "source_id").map_err(|_| "cross_source")?;
    if receipt
        .get("source_engine")
        .and_then(|value| value.as_str())
        != Some(engine)
        || receipt.get("source_id").and_then(|value| value.as_str()) != Some(source_id)
    {
        return Err("cross_source");
    }
    let generation = document
        .get("source_generation")
        .and_then(|value| value.as_i64())
        .ok_or("stale_run")?;
    if receipt
        .get("source_generation")
        .and_then(|value| value.as_i64())
        != Some(generation)
    {
        return Err("stale_run");
    }
    let kind = document
        .pointer("/snapshot/kind")
        .and_then(|value| value.as_str())
        .ok_or("cross_source")?;
    let snap = document
        .pointer("/snapshot/value")
        .and_then(|value| value.as_str())
        .ok_or("cross_source")?;
    if receipt
        .pointer("/snapshot/kind")
        .and_then(|value| value.as_str())
        != Some(kind)
        || receipt
            .pointer("/snapshot/value")
            .and_then(|value| value.as_str())
            != Some(snap)
    {
        return Err("cross_source");
    }
    for name in [
        "schema_digest",
        "type_digest",
        "config_digest",
        "input_digest",
    ] {
        let want = text_field(document, name).map_err(|_| "digest_mismatch")?;
        let got = receipt
            .get(name)
            .and_then(|value| value.as_str())
            .ok_or("digest_mismatch")?;
        if !hex64(want) || got != want {
            return Err("digest_mismatch");
        }
    }
    match receipt.get("audit_digest") {
        None | Some(Value::Null) => Ok(()),
        Some(Value::String(got)) => {
            let want = text_field(document, "audit_digest").map_err(|_| "digest_mismatch")?;
            if hex64(&want) && got == &want {
                Ok(())
            } else {
                Err("digest_mismatch")
            }
        }
        Some(_) => Err("digest_mismatch"),
    }
}

/// A declared hash that copies an identity digest is not the producer file hash.
fn declared_is_self(document: &Value, declared: Option<&str>, receipt_sha256: &str) -> bool {
    let Some(declared) = declared else {
        return false;
    };
    if declared == receipt_sha256 {
        return false;
    }
    [
        "schema_digest",
        "type_digest",
        "config_digest",
        "input_digest",
        "audit_digest",
    ]
    .iter()
    .any(|name| document.get(*name).and_then(|value| value.as_str()) == Some(declared))
}

/// SHA-256 of the `push_schema_table` / `push_schema_column` preimage.
///
/// This is the catalog schema digest. It is not the Arrow schema-message hash
/// and it is not a pasted historical digest. Storage caveat stays empty;
/// semantic notes are not sealed caveat tokens.
pub fn seal_schema_digest(
    logical_schema: &str,
    logical_table: &str,
    columns: &[CopyColumnDecl],
) -> Result<String, &'static str> {
    let mut preimage = String::new();
    oo_core::sealed_import::push_schema_table(&mut preimage, logical_schema, logical_table);
    for column in columns {
        let (oid, typmod) = column_storage(column)?;
        oo_core::sealed_import::push_schema_column(
            &mut preimage,
            &column.logical_name,
            oid,
            typmod,
            None,
        );
    }
    Ok(oo_core::api::payload_sha256(preimage.as_bytes()))
}

fn column_storage(column: &CopyColumnDecl) -> Result<(i64, i32), &'static str> {
    if column.document {
        if column.arrow_type != "utf8"
            || column.precision.is_some()
            || column.scale.is_some()
            || column.char_length.is_some()
        {
            return Err("bad_type");
        }
        return Ok((3802, -1));
    }
    let plain = column.precision.is_none() && column.scale.is_none();
    match column.arrow_type.as_str() {
        "int8" | "int16"
            if column.scale.is_none()
                && width_ok(column, if column.arrow_type == "int8" { 8 } else { 16 }) =>
        {
            Ok((21, -1))
        }
        "int32" if column.scale.is_none() && width_ok(column, 32) => Ok((23, -1)),
        "int64" if column.scale.is_none() && width_ok(column, 64) => Ok((20, -1)),
        "uint8" if column.scale.is_none() && width_ok(column, 8) => Ok((21, -1)),
        "uint64" if column.precision == Some(64) && column.scale.is_none() => {
            Ok((1700, native_arrow::numeric_typmod(20, 0)))
        }
        "date32" if plain => Ok((1082, -1)),
        "time64_micros" if plain => Ok((1083, -1)),
        "duration_micros" if plain => Ok((1186, -1)),
        "time64_nanos" if plain => Ok((20, -1)),
        kind if native_arrow::fixed_binary_width(kind).is_some() && plain => {
            if matches!(native_arrow::fixed_binary_width(kind), Some(12 | 16)) {
                Ok((17, -1))
            } else { Err("bad_type") }
        }
        kind if (kind.starts_with("struct(") || kind.starts_with("union(dense,")) && plain => {
            Ok((114, -1))
        }
        "utf8" => match column.char_length {
            Some(len) if (1..=63).contains(&len) && plain => Ok((1042, len + 4)),
            None if plain => Ok((1043, -1)),
            _ => Err("bad_type"),
        },
        "bool" | "float64" if plain => Ok((if column.arrow_type == "bool" { 16 } else { 701 }, -1)),
        "binary" | "dictionary_binary" | "dictionary_int32_binary" if plain => Ok((17, -1)),
        "list_int64" | "list_binary" | "list(utf8)" if plain => Ok((114, -1)),
        "timestamp_micros" | "timestamp_second_naive" | "timestamp_millis_naive" | "date64"
            if plain =>
        {
            Ok((1114, -1))
        }
        "timestamp_micros_utc" | "timestamp_second_utc" | "timestamp(ms,utc)" if plain => {
            Ok((1184, -1))
        }
        "decimal128" | "decimal256" => {
            let precision = column.precision.ok_or("precision")?;
            let scale = column.scale.ok_or("precision")?;
            let limit = if column.arrow_type == "decimal128" {
                38
            } else {
                76
            };
            if !(1..=limit).contains(&precision) || !(0..=precision).contains(&scale) {
                return Err("precision");
            }
            Ok((1700, native_arrow::numeric_typmod(precision, scale)))
        }
        _ => Err("bad_type"),
    }
}

fn width_ok(column: &CopyColumnDecl, width: i32) -> bool {
    match column.precision {
        None => true,
        Some(value) => value == width,
    }
}

/// Bind a real producer catalog. Identity comes from the file. `schema_digest`
/// must be [`seal_schema_digest`] of the load columns. A null producer digest
/// is filled by that preimage and is not a guessed constant.
fn bind_producer_catalog(
    document: &Value,
    arrow: &[u8],
    parsed: &Value,
    receipt_sha256: &str,
    _protocol: &str,
) -> Result<Admission, &'static str> {
    let engine = text_field(document, "source_engine").map_err(|_| "cross_source")?;
    let source_id = text_field(document, "source_id").map_err(|_| "cross_source")?;
    if parsed.get("source_engine").and_then(|value| value.as_str()) != Some(engine) {
        return Err("cross_source");
    }
    let table = producer_table(parsed, source_id)?;
    let generation = document
        .get("source_generation")
        .and_then(|value| value.as_i64())
        .ok_or("stale_run")?;
    let produced = table
        .get("generation")
        .or_else(|| table.get("source_generation"))
        .and_then(|value| value.as_i64())
        .ok_or("stale_run")?;
    if produced != generation {
        return Err("stale_run");
    }
    let snapshot = document
        .pointer("/snapshot/value")
        .and_then(|value| value.as_str())
        .ok_or("cross_source")?;
    match table.get("snapshot") {
        Some(value) if value.is_object() => {
            let kind = document
                .pointer("/snapshot/kind")
                .and_then(|item| item.as_str())
                .ok_or("cross_source")?;
            if value.pointer("/kind").and_then(|item| item.as_str()) != Some(kind)
                || value.pointer("/value").and_then(|item| item.as_str()) != Some(snapshot)
            {
                return Err("cross_source");
            }
        }
        _ => {
            let position = table
                .get("position")
                .and_then(|value| value.as_str())
                .filter(|text| !text.is_empty());
            if position != Some(snapshot) {
                return Err("cross_source");
            }
        }
    }
    let (columns, _) = parse_columns(document).map_err(|_| "bad_type")?;
    let schema = mapped_name(text_field(document, "logical_schema").map_err(|_| "bad_type")?)?;
    let logical_table =
        mapped_name(text_field(document, "logical_table").map_err(|_| "bad_type")?)?;
    let sealed = seal_schema_digest(&schema, &logical_table, &columns)?;
    if text_field(document, "schema_digest").map_err(|_| "digest_mismatch")? != sealed {
        return Err("digest_mismatch");
    }
    if !arrow.is_empty() {
        let schema_message =
            native_arrow::arrow_schema_sha256(arrow).map_err(|_| "digest_mismatch")?;
        if schema_message == sealed {
            return Err("digest_mismatch");
        }
        let file_hash = native_arrow::arrow_sha256(arrow);
        if text_field(document, "input_digest").map_err(|_| "digest_mismatch")? != file_hash {
            return Err("digest_mismatch");
        }
        let listed = producer_arrow_hashes(parsed, table);
        if !listed.is_empty() && !listed.iter().any(|item| item == &file_hash) {
            return Err("digest_mismatch");
        }
    }
    match table
        .get("schema_digest")
        .or_else(|| parsed.get("schema_digest"))
    {
        Some(Value::String(text)) if text == &sealed => {}
        Some(Value::String(_)) => return Err("digest_mismatch"),
        Some(Value::Null) | None => {}
        Some(_) => return Err("digest_mismatch"),
    }
    for name in [
        "type_digest",
        "config_digest",
        "audit_digest",
        "catalog_sha256",
        "registry_sha256",
        "driver_digest",
        "arrow_schema_sha256",
        "run_id",
    ] {
        match producer_field(parsed, table, name) {
            ProducerField::Absent => {}
            ProducerField::Hex(text) => {
                if document.get(name).and_then(|value| value.as_str()) != Some(text) {
                    return Err("digest_mismatch");
                }
            }
            ProducerField::Text(text) if name == "run_id" => {
                if document.get(name).and_then(|value| value.as_str()) != Some(text) {
                    return Err("cross_source");
                }
            }
            ProducerField::Text(_) | ProducerField::Other => return Err("digest_mismatch"),
        }
    }
    let declared = document
        .get("admission_expect")
        .and_then(|value| value.get("declared_receipt_sha256"))
        .and_then(|value| value.as_str());
    if declared_is_self(document, declared, receipt_sha256) || declared != Some(receipt_sha256) {
        return Err("digest_mismatch");
    }
    require_producer_registration(document, arrow, receipt_sha256)
}

fn read_registry(document: &Value) -> Result<Vec<u8>, &'static str> {
    let (path, bytes) = native_binding::read_operator_registry()?;
    if document.get("registry_path").and_then(Value::as_str).is_some_and(|requested| requested != path) {
        return Err("nonadmitted");
    }
    Ok(bytes)
}

fn registry_receipt_path(registry: &[u8]) -> Result<String, &'static str> {
    let document: Value = serde_json::from_slice(registry).map_err(|_| "nonadmitted")?;
    let path = document
        .pointer("/bindings/0/admission_receipt_path")
        .and_then(|value| value.as_str())
        .ok_or("nonadmitted")?;
    if path.starts_with('/') && !path.contains('\0') {
        Ok(path.to_string())
    } else {
        Err("nonadmitted")
    }
}

fn open_registration(
    document: &Value,
    arrow: &[u8],
    registry: &[u8],
    receipt: &[u8],
) -> Result<native_binding::ServerRegistration, &'static str> {
    let arrow_sha = native_arrow::arrow_sha256(arrow);
    let arrow_schema = if arrow.is_empty() {
        None
    } else {
        Some(native_arrow::arrow_schema_sha256(arrow).map_err(|_| "digest_mismatch")?)
    };
    let mut opened = native_binding::open_server_registration(
        registry,
        receipt,
        &arrow_sha,
        arrow_schema.as_deref(),
    )?;
    let audit = text_field(document, "audit_digest").map_err(|_| "digest_mismatch")?;
    if audit != opened.audit_sha256 {
        return Err("digest_mismatch");
    }
    // File/preimage equality is integrity only. Admission additionally needs
    // the process operator registry and reexecution of the frozen validator.
    let authorized = native_binding::resolve_sealed(
        text_field(document, "source_id")?,
        document.get("source_generation").and_then(Value::as_i64).ok_or("stale_run")?,
        document.pointer("/snapshot/kind").and_then(Value::as_str).ok_or("cross_source")?,
        document.pointer("/snapshot/value").and_then(Value::as_str).ok_or("cross_source")?,
        text_field(document, "config_digest")?,
        text_field(document, "schema_digest")?,
        text_field(document, "type_digest")?,
        text_field(document, "input_digest")?,
    )?;
    if !authorized.source_bound
        || authorized.receipt_sha256 != opened.receipt_sha256
        || authorized.source_engine != text_field(document, "source_engine")?
    {
        return Err("nonadmitted");
    }
    opened.fields = authorized.fields;
    opened.partition_scope = authorized.partition_scope;
    opened.transaction_scope = authorized.transaction_scope;
    Ok(opened)
}

fn canonical_ct_key(text: &str) -> bool {
    if text.is_empty() { return false; }
    if text.contains('\u{001f}') { return text.split('\u{001f}').all(canonical_ct_part); }
    canonical_ct_single(text)
}

fn canonical_ct_single(text: &str) -> bool {
    if let Some(hex) = text.strip_prefix("\\x") {
        return hex.len() % 2 == 0 && hex.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase());
    }
    canonical_i64(text) || !text.chars().any(|c| c.is_control())
}

fn canonical_ct_part(text: &str) -> bool {
    let Some((len, body)) = text.split_once(':') else { return false; };
    !len.is_empty() && len.bytes().all(|byte| byte.is_ascii_digit()) && !len.starts_with('0')
        && len == body.len().to_string() && canonical_ct_single(body)
}

// The operator registry already pins these captured bytes; caller CT fields cannot rebind them.
fn validate_mssql_capture(document: &Value, capture: &Value, arrow: &[u8]) -> Result<(), &'static str> {
    if document["source_engine"] != "sqlserver" || document["position_codec"] != "mssql-ct-version/v1"
        || document["capture_method"] != "c2_change_feed" || document["delete_detection"] != "native"
        || document["consistency_grade"] != "S1" || document["cross_table_consistent"] != false
        || capture["protocol"] != "oo.mssql-preview.capture/v1" || capture["tds_query_executed"] != true
        || capture["source_id"] != document["source_id"] || capture["source_generation"] != document["source_generation"]
        || capture["logical_schema"] != document["logical_schema"] || capture["logical_table"] != document["logical_table"]
        || capture["candidate_position"] != document["sqlserver_candidate_position"]
        || capture["prior_position"] != document["sqlserver_prior_position"]
        || capture["snapshot"] != document["snapshot"] || capture["tombstones"] != document["tombstones"]
        || capture["primary_key"] != document["key_columns"] {
        return Err("ct_scope");
    }
    let candidate = &capture["candidate_position"];
    if candidate["protocol"] != "oo.mssql-preview.ct-position/v1" || candidate["source_id"] != document["source_id"] {
        return Err("ct_scope");
    }
    let guid = text_field(candidate, "database_guid")?;
    let fork = text_field(candidate, "recovery_fork_guid")?;
    if !is_uuid(guid) || !is_uuid(fork) || document["position_lineage"] != format!("mssql.{guid}.{fork}") { return Err("ct_scope"); }
    let selected = |position: &Value| -> Result<Value, &'static str> {
        let rows = position["tables"].as_array().ok_or("ct_scope")?;
        let matching: Vec<_> = rows.iter().filter(|row| row["schema"] == document["logical_schema"] && row["name"] == document["logical_table"]).collect();
        if matching.len() != 1 { return Err("ct_scope"); }
        Ok(matching[0].clone())
    };
    let current = selected(candidate)?;
    let version = current["version"].as_i64().filter(|n| *n >= 0).ok_or("ct_scope")?;
    if current["object_id"] != capture["object_id"] || current["keys"] != capture["primary_key"]
        || current["object_id"].as_i64().is_none_or(|n| n <= 0)
        || !current["schema_sha256"].as_str().is_some_and(hex64)
        || document["snapshot"]["kind"] != "mssql_ct_version" || document["snapshot"]["value"] != version.to_string()
        || document["applied_pos"] != version.to_string() { return Err("ct_scope"); }
    let tombs = document["tombstones"].as_array().ok_or("ct_scope")?;
    for item in tombs {
        if !item.as_str().is_some_and(canonical_ct_key) { return Err("ct_scope"); }
    }
    let rows = decode_arrow_stream(arrow).map_err(arrow_token)?.rows;
    if capture["upserts"].as_u64() != Some(rows as u64) { return Err("ct_scope"); }
    let snapshot = capture["mode"] == "snapshot";
    if document["full_snapshot"] != snapshot { return Err("ct_scope"); }
    if !snapshot {
        let prior = &capture["prior_position"];
        for key in ["protocol", "source_id", "database_guid", "recovery_fork_guid", "database"] {
            if prior[key] != candidate[key] { return Err("ct_scope"); }
        }
        let old = selected(prior)?;
        for key in ["object_id", "schema_sha256", "keys"] { if old[key] != current[key] { return Err("ct_scope"); } }
        let before = old["version"].as_i64().ok_or("ct_scope")?;
        let minimum = capture["min_valid_version"].as_i64().ok_or("ct_scope")?;
        if before < minimum || before > version || (before == version && (rows != 0 || !document["tombstones"].as_array().is_some_and(Vec::is_empty))) {
            return Err("expired");
        }
    }
    Ok(())
}

fn registration_admission(opened: native_binding::ServerRegistration) -> Admission {
    Admission {
        source_bound: true,
        trusted_fields: opened.fields,
        partition_scope: opened.partition_scope,
        transaction_scope: opened.transaction_scope,
        consistency: Some(opened.consistency.clone()),
        cross_table: Some(opened.cross_table),
        producer_trust: json!({
            "entry": "verify_inputs",
            "trusted": true,
            "self_equal_digest": false,
            "receipt_sha256": opened.receipt_sha256,
            "registry_preimage_sha256": opened.preimage_sha256,
            "catalog_sha256": opened.catalog_sha256,
            "audit_sha256": opened.audit_sha256,
            "document_bound": true,
            "grade_source": "receipt",
            "note": "registry_sha256 on the receipt is the registry preimage; admission_receipt_sha256 on the registry is the receipt file hash"
        }),
    }
}

fn require_server_registration(
    document: &Value,
    arrow: &[u8],
    receipt: &[u8],
) -> Result<Admission, &'static str> {
    let registry = read_registry(document)?;
    let opened = open_registration(document, arrow, &registry, receipt)?;
    Ok(registration_admission(opened))
}

fn require_producer_registration(
    document: &Value,
    arrow: &[u8],
    catalog_sha: &str,
) -> Result<Admission, &'static str> {
    let registry = read_registry(document)?;
    let path = registry_receipt_path(&registry)?;
    let receipt = read_capped(Path::new(&path)).map_err(|_| "digest_missing")?;
    let opened = open_registration(document, arrow, &registry, &receipt)?;
    if opened.catalog_sha256 != catalog_sha {
        return Err("digest_mismatch");
    }
    let parsed: Value = serde_json::from_slice(&receipt).map_err(|_| "nonadmitted")?;
    bind_document_to_producer(document, &parsed)?;
    Ok(registration_admission(opened))
}

fn expect_matches_document(document: &Value) -> Result<(), &'static str> {
    let Some(expect) = document
        .get("admission_expect")
        .filter(|value| value.is_object())
    else {
        return Ok(());
    };
    if let Some(engine) = expect.get("source_engine").and_then(|value| value.as_str())
        && text_field(document, "source_engine").ok() != Some(engine)
    {
        return Err("cross_source");
    }
    if let Some(source) = expect.get("source_id").and_then(|value| value.as_str())
        && text_field(document, "source_id").ok() != Some(source)
    {
        return Err("cross_source");
    }
    if let Some(generation) = expect
        .get("source_generation")
        .and_then(|value| value.as_i64())
        && document.get("source_generation").and_then(|value| value.as_i64()) != Some(generation)
    {
        return Err("stale_run");
    }
    Ok(())
}

enum ProducerField<'a> {
    Absent,
    Hex(&'a str),
    Text(&'a str),
    Other,
}

fn producer_field<'a>(parsed: &'a Value, table: &'a Value, name: &str) -> ProducerField<'a> {
    let value = table.get(name).or_else(|| parsed.get(name));
    match value {
        None | Some(Value::Null) => ProducerField::Absent,
        Some(Value::String(text)) if hex64(text) => ProducerField::Hex(text),
        Some(Value::String(text)) if !text.is_empty() => ProducerField::Text(text),
        Some(_) => ProducerField::Other,
    }
}

fn producer_table<'a>(parsed: &'a Value, source_id: &str) -> Result<&'a Value, &'static str> {
    if let Some(tables) = parsed.get("tables").and_then(|value| value.as_array()) {
        return tables
            .iter()
            .find(|table| {
                table.get("source_id").and_then(|value| value.as_str()) == Some(source_id)
            })
            .ok_or("cross_source");
    }
    if parsed.get("source_id").and_then(|value| value.as_str()) == Some(source_id) {
        return Ok(parsed);
    }
    Err("cross_source")
}

fn producer_arrow_hashes(parsed: &Value, table: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |value: &Value| {
        if let Some(text) = value.as_str().filter(|text| hex64(text)) {
            out.push(text.to_string());
        }
    };
    if let Some(items) = parsed
        .get("arrow_sha256")
        .and_then(|value| value.as_array())
    {
        items.iter().for_each(&mut push);
    }
    if let Some(value) = table.get("arrow_sha256") {
        push(value);
    }
    if let Some(files) = parsed.get("files").and_then(|value| value.as_array()) {
        for file in files {
            if let Some(value) = file.get("file_sha256") {
                push(value);
            }
        }
    }
    out
}

fn hex_ref<'a>(value: &'a Value, name: &str) -> Result<&'a str, &'static str> {
    let text = value
        .get(name)
        .and_then(|item| item.as_str())
        .ok_or("digest_mismatch")?;
    if hex64(text) {
        Ok(text)
    } else {
        Err("digest_mismatch")
    }
}

fn check_arrow_declaration(bytes: &[u8], columns: &[CopyColumnDecl]) -> Result<(), &'static str> {
    let decoded = decode_arrow_stream(bytes).map_err(arrow_token)?;
    if decoded.fields.len() != columns.len() {
        return Err("bad_type");
    }
    for (field, decl) in decoded.fields.iter().zip(columns) {
        if field.name != decl.name
            || field.nullable != decl.nullable
            || (field.arrow_type != decl.arrow_type
                && !(field.arrow_type == "dictionary_binary"
                    && decl.arrow_type == "dictionary_int32_binary"))
        {
            return Err("bad_type");
        }
        if field.arrow_type == "decimal256" || field.arrow_type == "decimal128" {
            if field.precision != decl.precision || field.scale != decl.scale {
                return Err("precision");
            }
        } else if decl.precision.is_some()
            && field.precision.is_some()
            && decl.precision != field.precision
        {
            return Err("bad_type");
        }
    }
    let _ = decoded;
    Ok(())
}

fn arrow_token(fault: ArrowFault) -> &'static str {
    match fault {
        ArrowFault::Empty => "empty",
        ArrowFault::Truncated => "truncated",
        ArrowFault::Shape => "bad_type",
        ArrowFault::Precision => "precision",
    }
}

fn read_arrow(document: &Value) -> Result<Vec<u8>, &'static str> {
    if let Some(path) = document.get("arrow_path").and_then(|value| value.as_str()) {
        let budget = match document.get("max_arrow_bytes") {
            None => READ_BUDGET,
            Some(value) => value.as_u64().filter(|n| *n > 0 && *n <= 268_435_456)
                .and_then(|n| usize::try_from(n).ok()).ok_or("read_budget")?,
        };
        let meta = fs::metadata(path).map_err(|_| "digest_missing")?;
        if !meta.is_file() { return Err("digest_missing"); }
        if meta.len() > budget as u64 { return Err("read_budget"); }
        let bytes = fs::read(path).map_err(|_| "digest_missing")?;
        if bytes.len() > budget { return Err("read_budget"); }
        return Ok(bytes);
    }
    Ok(Vec::new())
}

enum ReadStop {
    Missing,
    TooLarge,
}

fn read_capped(path: &Path) -> Result<Vec<u8>, ReadStop> {
    let meta = fs::metadata(path).map_err(|_| ReadStop::Missing)?;
    if !meta.is_file() {
        return Err(ReadStop::Missing);
    }
    if meta.len() > READ_BUDGET as u64 {
        return Err(ReadStop::TooLarge);
    }
    let bytes = fs::read(path).map_err(|_| ReadStop::Missing)?;
    if bytes.len() > READ_BUDGET {
        return Err(ReadStop::TooLarge);
    }
    Ok(bytes)
}

async fn read_batch(
    conn: &mut sqlx::PgConnection,
    plan: &UpdatePlan,
) -> Result<Option<StoredBatch>, &'static str> {
    let row: Option<String> =
        sqlx::query_scalar("SELECT _oo.native_update_read_batch($1, $2, $3, $4)")
            .bind(&plan.source_id)
            .bind(&plan.schema)
            .bind(&plan.table)
            .bind(&plan.batch_id)
            .fetch_one(&mut *conn)
            .await
            .map_err(sql_token)?;
    let Some(text) = row else {
        return Ok(None);
    };
    let value: Value = serde_json::from_str(&text).map_err(|_| "not_sealed")?;
    Ok(Some(StoredBatch {
        payload_digest: value
            .get("payload_digest")
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .to_string(),
        run_id: value
            .get("run_id")
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .to_string(),
        target_generation: value
            .get("target_generation")
            .and_then(|item| item.as_i64())
            .unwrap_or(0),
        physical_name: value
            .get("physical_name")
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .to_string(),
        row_count: value
            .get("row_count")
            .and_then(|item| item.as_i64())
            .unwrap_or(0),
    }))
}

fn optional_lease_id(document: &Value) -> Result<Option<String>, &'static str> {
    match document.get("lease_id") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let text = value
                .as_str()
                .filter(|text| is_uuid(text))
                .ok_or("closed")?;
            Ok(Some(text.to_string()))
        }
    }
}

fn optional_lease_generation(document: &Value) -> Result<Option<i64>, &'static str> {
    match document.get("lease_generation") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let generation = value.as_i64().filter(|item| *item >= 1).ok_or("closed")?;
            Ok(Some(generation))
        }
    }
}

fn maria_stored(lineage: &str, pos: &str) -> bool {
    maria_lineage(lineage).is_some() && maria_seq(pos)
}

fn maria_lineage(lineage: &str) -> Option<(u32, u32)> {
    let rest = lineage.strip_prefix('d')?;
    let (domain, server) = rest.split_once(".s")?;
    let domain = canonical_u32(domain)?;
    let server = canonical_u32(server)?;
    if server == 0 {
        return None;
    }
    Some((domain, server))
}

fn maria_seq(pos: &str) -> bool {
    let bytes = pos.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 20
        && bytes.iter().all(|byte| byte.is_ascii_digit())
        && (bytes.len() == 1 || bytes[0] != b'0')
        && pos.parse::<u64>().is_ok()
}

fn canonical_u32(text: &str) -> Option<u32> {
    let bytes = text.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 10
        || !bytes.iter().all(|byte| byte.is_ascii_digit())
        || (bytes.len() > 1 && bytes[0] == b'0')
    {
        return None;
    }
    text.parse().ok()
}

fn maria_order(left_lineage: &str, left_pos: &str, right_lineage: &str, right_pos: &str) -> Result<std::cmp::Ordering, &'static str> {
    if !maria_stored(left_lineage, left_pos) || !maria_stored(right_lineage, right_pos) {
        return Err("gtid_identity");
    }
    if left_lineage != right_lineage {
        return Err("incomparable");
    }
    decimal_cmp(left_pos, right_pos)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GtidCommit {
    codec: String,
    lineage: String,
    epoch: i32,
    applied_pos: String,
    txn_id: String,
}

fn one_gtid(document: &Value) -> Result<GtidCommit, &'static str> {
    let object = document.as_object().ok_or("closed")?;
    if object.len() != 2
        || document["protocol"] != TRANSACTION_PROTOCOL
        || object.keys().any(|key| key != "protocol" && key != "entries")
    {
        return Err("closed");
    }
    let requests = document["entries"]
        .as_array()
        .filter(|items| (1..=32).contains(&items.len()))
        .ok_or("closed")?;
    let mut identity = None;
    for request in requests {
        let parsed = gtid_commit_identity(request)?;
        if let Some(prior) = &identity {
            if prior != &parsed {
                gtid_order(prior, &parsed)?;
                return Err("gtid_identity");
            }
        } else {
            identity = Some(parsed);
        }
    }
    identity.ok_or("closed")
}

fn gtid_plans_match(plans: &[UpdatePlan], identity: &GtidCommit) -> Result<(), &'static str> {
    let first = plans.first().ok_or("closed")?;
    if first.transaction_scope.is_some() {
        return Err("gtid_identity");
    }
    let mut prior = None;
    for plan in plans {
        let key = (&plan.schema, &plan.table);
        if prior.is_some_and(|old| old >= key) {
            return Err("transaction_scope_required");
        }
        prior = Some(key);
        let engine_ok = match identity.codec.as_str() {
            "mariadb-gtid/v1" => plan.source_engine == "mariadb",
            "mysql-gtid/v1" => plan.source_engine == "mysql",
            _ => false,
        };
        if !engine_ok
            || plan.capture_method != "c1_cdc"
            || plan.delete_detection != "native"
            || plan.consistency_grade != "T"
            || !plan.cross_table
            || plan.position_codec != identity.codec
            || plan.position_lineage != identity.lineage
            || plan.applied_pos_request != identity.applied_pos
            || plan.mode != "incremental"
            || plan.partition_scope.is_some()
            || plan.mirror
            || !plan.source_bound
            || plan.lease_id.is_none()
            || plan.lease_generation.is_none()
            || plan.source_id != first.source_id
            || plan.target != first.target
            || plan.shard != first.shard
            || plan.owner != first.owner
            || plan.lease_id != first.lease_id
            || plan.lease_generation != first.lease_generation
            || plan.config_digest != first.config_digest
        {
            return Err("transaction_scope_required");
        }
    }
    Ok(())
}

fn gtid_scope_digest(source: &str, identity: &GtidCommit) -> String {
    let canon = format!(
        "{source}\n{}\n{}\n{}\n{}\n{}",
        identity.codec, identity.lineage, identity.epoch, identity.applied_pos, identity.txn_id
    );
    hex_bytes(&Sha256::digest(canon.as_bytes()))
}

fn gtid_order(left: &GtidCommit, right: &GtidCommit) -> Result<std::cmp::Ordering, &'static str> {
    if left.codec != right.codec || left.lineage != right.lineage || left.epoch != right.epoch {
        return Err("incomparable");
    }
    decimal_cmp(&left.applied_pos, &right.applied_pos)
}

fn gtid_commit_identity(entry: &Value) -> Result<GtidCommit, &'static str> {
    let engine = entry.get("source_engine").and_then(Value::as_str).ok_or("gtid_family")?;
    let codec = entry.get("position_codec").and_then(Value::as_str).ok_or("gtid_identity")?;
    let lineage = entry.get("position_lineage").and_then(Value::as_str).ok_or("gtid_identity")?;
    let applied_pos = entry.get("applied_pos").and_then(Value::as_str).ok_or("gtid_identity")?;
    let txn_id = entry.get("txn_id").and_then(Value::as_str).ok_or("gtid_identity")?;
    if codec.contains(',') || lineage.contains(',') || applied_pos.contains(',') || txn_id.contains(',') {
        return Err("gtid_identity");
    }
    let epoch = match entry.get("source_epoch") {
        Some(Value::Number(number)) => number
            .as_i64()
            .and_then(|value| i32::try_from(value).ok())
            .filter(|value| *value >= 0)
            .ok_or("gtid_identity")?,
        _ => return Err("gtid_identity"),
    };
    let (codec, lineage, applied_pos, txn_id) = match (engine, codec) {
        ("mariadb", "mariadb-gtid/v1") => {
            let (domain, server) = maria_lineage(lineage).ok_or("gtid_identity")?;
            if !maria_seq(applied_pos) {
                return Err("gtid_identity");
            }
            let expected = format!("{domain}-{server}-{applied_pos}");
            if txn_id != expected {
                return Err("gtid_identity");
            }
            (
                "mariadb-gtid/v1".to_string(),
                lineage.to_string(),
                applied_pos.to_string(),
                txn_id.to_string(),
            )
        }
        ("mysql", "mysql-gtid/v1") => {
            let uuid = mysql_lineage(lineage).ok_or("gtid_identity")?;
            if !mysql_gno(applied_pos) {
                return Err("gtid_identity");
            }
            let expected = format!("{uuid}:{applied_pos}");
            if txn_id != expected {
                return Err("gtid_identity");
            }
            (
                "mysql-gtid/v1".to_string(),
                lineage.to_string(),
                applied_pos.to_string(),
                txn_id.to_string(),
            )
        }
        ("mariadb", "mysql-gtid/v1") | ("mysql", "mariadb-gtid/v1") => return Err("gtid_family"),
        _ => return Err("gtid_identity"),
    };
    assist_not_identity(entry, &txn_id, &applied_pos)?;
    Ok(GtidCommit {
        codec,
        lineage,
        epoch,
        applied_pos,
        txn_id,
    })
}

fn assist_not_identity(entry: &Value, txn_id: &str, applied_pos: &str) -> Result<(), &'static str> {
    let file = match entry.get("resume_file") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) if !text.is_empty() && text.len() <= 128 && !text.contains(',') => Some(text.as_str()),
        _ => return Err("gtid_identity"),
    };
    let pos = match entry.get("resume_pos") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) if maria_seq(text) => Some(text.as_str()),
        _ => return Err("gtid_identity"),
    };
    if file.is_some() != pos.is_some() {
        return Err("gtid_identity");
    }
    if let Some(file) = file
        && (txn_id == file || applied_pos == file || txn_id.contains(file))
    {
        return Err("gtid_identity");
    }
    if txn_id.contains('/') || applied_pos.contains('/') || applied_pos.contains(':') {
        return Err("gtid_identity");
    }
    Ok(())
}

fn mysql_lineage(lineage: &str) -> Option<&str> {
    let uuid = lineage.strip_prefix("mysql.")?;
    if mysql_uuid(uuid) { Some(uuid) } else { None }
}

fn mysql_uuid(text: &str) -> bool {
    let mut parts = text.split('-');
    for width in [8, 4, 4, 4, 12] {
        let Some(part) = parts.next() else { return false };
        if part.len() != width || !part.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
            return false;
        }
    }
    parts.next().is_none()
}

fn mysql_gno(pos: &str) -> bool {
    maria_seq(pos) && pos != "0"
}

fn decimal_add(text: &str, addend: u64) -> Result<String, &'static str> {
    if text.is_empty() || text.len() > 38 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("closed");
    }
    let mut digits: Vec<u8> = text.bytes().map(|byte| byte - b'0').collect();
    let mut carry = addend;
    let mut index = digits.len();
    while carry > 0 {
        if index == 0 {
            if digits.len() >= 38 {
                return Err("read_budget");
            }
            digits.insert(0, 0);
            index = 1;
        }
        index -= 1;
        let sum = u64::from(digits[index]) + carry;
        digits[index] = u8::try_from(sum % 10).map_err(|_| "read_budget")?;
        carry = sum / 10;
    }
    let start = digits
        .iter()
        .position(|digit| *digit != 0)
        .unwrap_or(digits.len() - 1);
    Ok(digits[start..]
        .iter()
        .map(|digit| char::from(digit + b'0'))
        .collect())
}

fn decimal_cmp(left: &str, right: &str) -> Result<std::cmp::Ordering, &'static str> {
    let left = integer_digits(left)?;
    let right = integer_digits(right)?;
    Ok(left.len().cmp(&right.len()).then(left.cmp(&right)))
}

fn integer_digits(text: &str) -> Result<String, &'static str> {
    let text = text.trim();
    let (body, fraction) = text.split_once('.').unwrap_or((text, ""));
    if body.is_empty()
        || !body.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte == b'0')
    {
        return Err("expired");
    }
    let trimmed = body.trim_start_matches('0');
    Ok(if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    })
}

/// Derive semantics only when both the sealed catalog and the source-bound
/// observation are present. A missing pair does not block the physical load.
/// One side without the other is incomplete. The engine name is not a refuse.
fn bind_oracle_contract(document: &Value) -> Result<Option<Value>, &'static str> {
    let observation = document
        .get("source_bound")
        .filter(|value| value.is_object());
    let catalog = document
        .get("sealed_catalog")
        .filter(|value| value.is_object());
    let (Some(bound), Some(catalog)) = (observation, catalog) else {
        if observation.is_some() || catalog.is_some() {
            return Err("source_bound_required");
        }
        return Ok(None);
    };
    if bound.get("engine").and_then(|value| value.as_str())
        != catalog.get("engine").and_then(|value| value.as_str())
        || catalog.get("source_id").and_then(|value| value.as_str())
            != document.get("source_id").and_then(|value| value.as_str())
        || catalog
            .get("source_generation")
            .and_then(|value| value.as_i64())
            != document
                .get("source_generation")
                .and_then(|value| value.as_i64())
    {
        return Err("cross_source");
    }
    let catalog_json = serde_json::to_string(catalog).map_err(|_| "source_bound_rejected")?;
    let bound_json = serde_json::to_string(bound).map_err(|_| "source_bound_rejected")?;
    match oo_core::source_bound::derive_source_bound(&catalog_json, &bound_json) {
        Ok(derived) => Ok(Some(derived_semantics_value(&derived))),
        Err(err) => Err(derive_token(&err.code)),
    }
}

fn derive_token(code: &str) -> &'static str {
    match code.rsplit('/').next().unwrap_or("") {
        "cross_source" | "cross_run" => "cross_source",
        "digest.mismatch" => "digest_mismatch",
        "column.mismatch" => "column_mismatch",
        "fidelity.contradiction" => "fidelity_contradiction",
        _ => "source_bound_rejected",
    }
}

fn derived_semantics_value(derived: &oo_core::source_bound::DerivedSourceBound) -> Value {
    json!({
        "protocol": oo_core::source_bound::DERIVED_PROTOCOL,
        "identity_hash": derived.identity_hash,
        "engine": derived.engine,
        "source_id": derived.source_id,
        "schema_digest": derived.schema_digest,
        "version": derived.version,
        "capture": derived.capture,
        "family": derived.family,
        "product": derived.product,
        "source_kind": derived.source_kind,
        "consistency_grade": derived.consistency_grade,
        "delete_detection": derived.delete_detection,
        "columns": derived.columns.iter().map(|column| json!({
            "dataset": column.dataset,
            "column": column.logical_name,
            "caveats": column.caveats,
            "native_note": column.native_note
        })).collect::<Vec<_>>()
    })
}

fn key_plan(
    document: &Value,
    columns: &[CopyColumnDecl],
    delete_detection: &str,
    full_snapshot: bool,
) -> Result<(Vec<usize>, &'static str), &'static str> {
    let identity = match document.get("row_identity") {
        None | Some(Value::Null) => "unique",
        Some(Value::String(text)) => text.as_str(),
        Some(_) => return Err("closed"),
    };
    if !matches!(identity, "unique" | "duplicate" | "aggregate" | "none") {
        return Err("closed");
    }
    let mut indexes = named_key_indexes(document, columns)?;
    if identity == "none" {
        indexes.clear();
    }
    for &index in &indexes {
        if !key_type_ok(&columns[index]) {
            return Err("bad_type");
        }
    }
    let shape = if full_snapshot {
        "snapshot"
    } else {
        match identity {
            "none" => "snapshot",
            "duplicate" | "aggregate" if delete_detection == "partition_replace" => "partition",
            "duplicate" | "aggregate" => "snapshot",
            "unique" if delete_detection == "partition_replace" => "partition",
            "unique" => "unique",
            _ => return Err("closed"),
        }
    };
    if shape != "snapshot" && indexes.is_empty() {
        return Err("bad_type");
    }
    Ok((indexes, shape))
}

fn named_key_indexes(
    document: &Value,
    columns: &[CopyColumnDecl],
) -> Result<Vec<usize>, &'static str> {
    let names = if let Some(list) = document.get("key_columns").filter(|value| !value.is_null()) {
        let items = list.as_array().ok_or("bad_type")?;
        let mut names = Vec::with_capacity(items.len());
        for item in items {
            names.push(
                item.as_str()
                    .filter(|text| !text.is_empty())
                    .ok_or("bad_type")?
                    .to_string(),
            );
        }
        names
    } else if let Some(name) = document.get("key_column").and_then(|value| value.as_str()) {
        if name.is_empty() {
            return Err("bad_type");
        }
        vec![name.to_string()]
    } else {
        Vec::new()
    };
    let mut indexes = Vec::with_capacity(names.len());
    for name in &names {
        let index = columns
            .iter()
            .position(|column| column.name == *name || column.logical_name == *name)
            .ok_or("bad_type")?;
        if indexes.contains(&index) {
            return Err("bad_type");
        }
        indexes.push(index);
    }
    Ok(indexes)
}

fn key_type_ok(column: &CopyColumnDecl) -> bool {
    if column.nullable || column.document {
        return false;
    }
    match column.arrow_type.as_str() {
        "int8" => column.scale.is_none() && width_ok(column, 8),
        "int16" => column.scale.is_none() && width_ok(column, 16),
        "int32" => column.scale.is_none() && width_ok(column, 32),
        "int64" => column.scale.is_none() && width_ok(column, 64),
        "uint8" => column.scale.is_none() && width_ok(column, 8),
        "uint64" => column.precision == Some(64) && column.scale.is_none(),
        "utf8" => column.precision.is_none() && column.scale.is_none(),
        "date32" => column.precision.is_none() && column.scale.is_none(),
        "binary" | "fixedsizebinary(12)" | "fixedsizebinary(16)" => column.precision.is_none() && column.scale.is_none(),
        _ => false,
    }
}

fn tombstones_fit(
    tombstones: &[String],
    columns: &[CopyColumnDecl],
    indexes: &[usize],
    _shape: &str,
) -> bool {
    if indexes.is_empty() {
        return tombstones.is_empty();
    }
    tombstones
        .iter()
        .all(|text| tombstone_text_fits(text, columns, indexes))
}

fn tombstone_text_fits(text: &str, columns: &[CopyColumnDecl], indexes: &[usize]) -> bool {
    if indexes.len() == 1 {
        return canonical_key_text(&columns[indexes[0]], text);
    }
    let parts: Vec<&str> = text.split('\u{001f}').collect();
    if parts.len() != indexes.len() {
        return false;
    }
    parts.iter().zip(indexes).all(|(part, &index)| {
        let Some((len_text, body)) = part.split_once(':') else {
            return false;
        };
        let Ok(len) = len_text.parse::<usize>() else {
            return false;
        };
        len == body.len() && canonical_length(len_text) && canonical_key_text(&columns[index], body)
    })
}

fn canonical_length(text: &str) -> bool {
    text == "0"
        || (!text.is_empty()
            && !text.starts_with('0')
            && text.bytes().all(|byte| byte.is_ascii_digit()))
}

fn canonical_key_text(column: &CopyColumnDecl, text: &str) -> bool {
    match column.arrow_type.as_str() {
        "int8" => canonical_signed(text, 8),
        "int16" => canonical_signed(text, 16),
        "int32" => canonical_signed(text, 32),
        "int64" => canonical_i64(text),
        "uint8" => canonical_unsigned(text, u128::from(u8::MAX)),
        "uint64" => canonical_u64(text),
        "utf8" => !text.is_empty(),
        "date32" => canonical_date(text),
        "binary" | "fixedsizebinary(12)" | "fixedsizebinary(16)" => {
            let Some(raw) = text.strip_prefix("\\x") else { return false; };
            raw.len() % 2 == 0 && raw.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                && native_arrow::fixed_binary_width(&column.arrow_type).is_none_or(|width| raw.len() == width as usize * 2)
        }
        _ => false,
    }
}

fn canonical_signed(text: &str, width: u32) -> bool {
    canonical_i64(text)
        && text.parse::<i64>().ok().is_some_and(|value| {
            let limit = 1i64 << (width - 1);
            value >= -limit && value < limit
        })
}

fn canonical_unsigned(text: &str, max: u128) -> bool {
    !text.is_empty()
        && text.bytes().all(|byte| byte.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'))
        && text.parse::<u128>().ok().is_some_and(|value| value <= max)
}

fn canonical_i64(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    !digits.is_empty()
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'))
        && text != "-0"
        && text.parse::<i64>().is_ok()
}

fn canonical_u64(text: &str) -> bool {
    !text.is_empty()
        && text.bytes().all(|byte| byte.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'))
        && text
            .parse::<u128>()
            .ok()
            .is_some_and(|value| value <= u128::from(u64::MAX))
}

fn canonical_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    if !bytes
        .iter()
        .enumerate()
        .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
    {
        return false;
    }
    let Ok(year) = text[0..4].parse::<i32>() else {
        return false;
    };
    let Ok(month) = text[5..7].parse::<u32>() else {
        return false;
    };
    let Ok(day) = text[8..10].parse::<u32>() else {
        return false;
    };
    let Some(days) = days_from_civil(year, month, day) else {
        return false;
    };
    date32_iso(days).as_deref() == Some(text)
}

fn date32_iso(days: i32) -> Option<String> {
    let (year, month, day) = civil_from_days(days)?;
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

fn civil_from_days(days: i32) -> Option<(i32, u32, u32)> {
    let z = i64::from(days) + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = u64::try_from(z - era * 146_097).ok()?;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = i64::try_from(yoe).ok()? + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    if month <= 2 {
        year += 1;
    }
    if !(1..=9999).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some((
        i32::try_from(year).ok()?,
        u32::try_from(month).ok()?,
        u32::try_from(day).ok()?,
    ))
}

fn days_from_civil(year: i32, month: u32, day: u32) -> Option<i32> {
    if !(1..=9999).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let y = i64::from(if month <= 2 { year - 1 } else { year });
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = u64::try_from(y - era * 400).ok()?;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * u64::from(mp) + 2) / 5 + u64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    i32::try_from(era * 146_097 + i64::try_from(doe).ok()? - 719_468).ok()
}

fn refuse_bare_document_double(
    arrow: &[u8],
    columns: &[CopyColumnDecl],
) -> Result<(), &'static str> {
    let decoded = decode_arrow_stream(arrow).map_err(arrow_token)?;
    for (index, column) in columns.iter().enumerate() {
        if !column.document {
            continue;
        }
        let cells = decoded.columns.get(index).ok_or("bad_type")?;
        for cell in cells {
            match cell {
                NativeCell::Null => {}
                NativeCell::Utf8(text) => {
                    let value: Value = serde_json::from_str(text).map_err(|_| "bad_type")?;
                    if json_has_bare_double(&value) {
                        return Err("jsonb_double_unencoded");
                    }
                }
                _ => return Err("bad_type"),
            }
        }
    }
    Ok(())
}

fn json_has_bare_double(value: &Value) -> bool {
    match value {
        Value::Number(number) => number.as_i64().is_none() && number.as_u64().is_none(),
        Value::Array(items) => items.iter().any(json_has_bare_double),
        Value::Object(map) => map.values().any(json_has_bare_double),
        _ => false,
    }
}

fn refuse_upsert_conflict(
    arrow: &[u8],
    tombstones: &[String],
    indexes: &[usize],
    shape: &str,
) -> Result<(), &'static str> {
    if shape == "snapshot" {
        if tombstones.is_empty() {
            return Ok(());
        }
        return Err("conflict");
    }
    if arrow.is_empty() {
        return Ok(());
    }
    let decoded = decode_arrow_stream(arrow).map_err(arrow_token)?;
    let keys = key_rows(&decoded, indexes)?;
    if overlaps(&keys, tombstones) {
        return Err("conflict");
    }
    if shape == "unique" && has_duplicate(&keys) {
        return Err("conflict");
    }
    Ok(())
}

fn key_rows(decoded: &DecodedStream, indexes: &[usize]) -> Result<Vec<String>, &'static str> {
    if indexes.is_empty() {
        return Ok(Vec::new());
    }
    let mut keys = Vec::with_capacity(decoded.rows);
    for row in 0..decoded.rows {
        keys.push(row_key_text(decoded, indexes, row)?);
    }
    Ok(keys)
}

fn row_key_text(
    decoded: &DecodedStream,
    indexes: &[usize],
    row: usize,
) -> Result<String, &'static str> {
    if indexes.len() == 1 {
        return cell_key_text(
            decoded
                .columns
                .get(indexes[0])
                .and_then(|column| column.get(row))
                .ok_or("bad_type")?,
        );
    }
    let mut out = String::new();
    for (pos, &index) in indexes.iter().enumerate() {
        if pos > 0 {
            out.push('\u{001f}');
        }
        let text = cell_key_text(
            decoded
                .columns
                .get(index)
                .and_then(|column| column.get(row))
                .ok_or("bad_type")?,
        )?;
        out.push_str(&text.len().to_string());
        out.push(':');
        out.push_str(&text);
    }
    Ok(out)
}

fn cell_key_text(cell: &NativeCell) -> Result<String, &'static str> {
    match cell {
        NativeCell::Binary(bytes) => Ok(format!("\\x{}", hex_bytes(bytes))),
        NativeCell::Int {
            bit_width: 64,
            value,
        } => {
            let text = value.to_string();
            if canonical_i64(&text) {
                Ok(text)
            } else {
                Err("bad_type")
            }
        }
        NativeCell::Int { bit_width, value } if matches!(bit_width, 8 | 16 | 32) => {
            let text = value.to_string();
            if canonical_signed(&text, u32::from(*bit_width)) {
                Ok(text)
            } else {
                Err("bad_type")
            }
        }
        NativeCell::UInt64(value) => Ok(value.to_string()),
        NativeCell::UInt8(value) => Ok(value.to_string()),
        NativeCell::Utf8(text) if !text.is_empty() => Ok(text.clone()),
        NativeCell::Date32(days) => date32_iso(*days).ok_or("bad_type"),
        _ => Err("bad_type"),
    }
}

fn validate_olap_route(
    document: &Value,
    scope: &Value,
    columns: &[CopyColumnDecl],
    key_indexes: &[usize],
    arrow: &[u8],
    source_engine: &str,
) -> Result<(), &'static str> {
    if scope["source_engine"] != source_engine || key_indexes.len() != 1 {
        return Err("partition_scope_required");
    }
    let route = scope["routing_column"].as_str().ok_or("partition_scope_required")?;
    let column = columns.get(key_indexes[0]).ok_or("partition_scope_required")?;
    if column.logical_name != route || column.arrow_type != "date32" || column.nullable {
        return Err("partition_scope_required");
    }
    let model = scope["key_model"].as_str().ok_or("partition_scope_required")?;
    let identity = document.get("row_identity").and_then(Value::as_str).ok_or("partition_scope_required")?;
    if !matches!((model, identity), ("duplicate", "duplicate") | ("unique", "unique") | ("primary", "unique")) {
        return Err("partition_scope_required");
    }
    let lower = scope["partition_range"]["lower"].as_str().ok_or("partition_scope_required")?;
    let upper = scope["partition_range"]["upper"].as_str().ok_or("partition_scope_required")?;
    // StarRocks reports a MINVALUE range bound as 0000-01-01. The target
    // copy-forward maps this exact token to -infinity; Arrow rows remain dates.
    if !(canonical_date(lower) || source_engine == "starrocks" && lower == "0000-01-01")
        || !canonical_date(upper) || lower >= upper {
        return Err("partition_scope_required");
    }
    let decoded = decode_arrow_stream(arrow).map_err(arrow_token)?;
    for row in 0..decoded.rows {
        let cell = decoded.columns.get(key_indexes[0]).and_then(|column| column.get(row)).ok_or("bad_type")?;
        let NativeCell::Date32(days) = cell else { return Err("bad_type"); };
        let date = date32_iso(*days).ok_or("bad_type")?;
        if date.as_str() < lower || date.as_str() >= upper {
            return Err("partition_scope_required");
        }
    }
    Ok(())
}

async fn apply_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    plan: &UpdatePlan,
) -> Result<Applied, &'static str> {
    if plan.columns.iter().any(|column| column.arrow_type == "duration_micros") {
        let stored_digest: Option<String> = sqlx::query_scalar(
            "SELECT checksum FROM _oo.schema_migrations WHERE version = $1"
        ).bind(target_installer::VERSION_NATIVE_INTERVAL)
         .fetch_optional(&mut **tx).await.map_err(sql_token)?;
        if stored_digest.as_deref() != Some(target_installer::compiled_native_interval_digest().as_str()) {
            return Err("interval_schema_not_ready");
        }
    }
    if plan.partition_scope.is_some() {
        let stored_digest: Option<String> = sqlx::query_scalar(
            "SELECT checksum FROM _oo.schema_migrations WHERE version = $1"
        ).bind(target_installer::VERSION_NATIVE_PARTITION)
         .fetch_optional(&mut **tx).await.map_err(sql_token)?;
        if stored_digest.as_deref() != Some(target_installer::compiled_native_partition_digest().as_str()) {
            return Err("partition_schema_not_ready");
        }
        if plan.previous_ack_auto {
            for (version, digest) in [
                (target_installer::VERSION_NATIVE_OLAP_PARTITION, target_installer::compiled_native_olap_partition_digest()),
                (target_installer::VERSION_NATIVE_OLAP_GUARD, target_installer::compiled_native_olap_guard_digest()),
                (target_installer::VERSION_NATIVE_OLAP_GUARD_SIGNATURE, target_installer::compiled_native_olap_guard_signature_digest()),
            ] {
                let stored_digest: Option<String> = sqlx::query_scalar("SELECT checksum FROM _oo.schema_migrations WHERE version = $1")
                    .bind(version).fetch_optional(&mut **tx).await.map_err(sql_token)?;
                if stored_digest.as_deref() != Some(digest.as_str()) {
                    return Err("partition_schema_not_ready");
                }
            }
        }
    }
    if plan.audit_digest.len() != 64 {
        return Err("no_audit");
    }
    let (upsert_keys, upsert_rows) = if plan.shape == "snapshot" {
        refuse_upsert_conflict(&plan.arrow, &plan.tombstones, &plan.key_indexes, plan.shape)?;
        if plan.arrow.is_empty() {
            (Vec::new(), 0usize)
        } else {
            let decoded = decode_arrow_stream(&plan.arrow).map_err(arrow_token)?;
            (Vec::new(), decoded.rows)
        }
    } else {
        refuse_upsert_conflict(&plan.arrow, &plan.tombstones, &plan.key_indexes, plan.shape)?;
        if plan.arrow.is_empty() {
            (Vec::new(), 0usize)
        } else {
            let decoded = decode_arrow_stream(&plan.arrow).map_err(arrow_token)?;
            let mut keys = key_rows(&decoded, &plan.key_indexes)?;
            if plan.shape == "partition" {
                keys.sort();
                if plan.source_engine == "clickhouse" && keys.windows(2).any(|pair| pair[0] == pair[1]) {
                    return Err("partition_duplicate_key");
                }
            }
            (keys, decoded.rows)
        }
    };
    register_mapping(
        tx,
        &plan.source_id,
        &plan.config_digest,
        plan.source_generation,
        "namespace",
        &plan.schema,
        "",
        "",
    )
    .await
    .map_err(|_| "physical")?;
    register_mapping(
        tx,
        &plan.source_id,
        &plan.config_digest,
        plan.source_generation,
        "table",
        &plan.schema,
        &plan.table,
        "",
    )
    .await
    .map_err(|_| "physical")?;
    for ident in &plan.column_idents {
        register_mapping(
            tx,
            &plan.source_id,
            &plan.config_digest,
            plan.source_generation,
            "column",
            &plan.schema,
            &plan.table,
            ident,
        )
        .await
        .map_err(|_| "physical")?;
    }
    let target_generation: i64 =
        sqlx::query_scalar("SELECT _oo.adopt_target_generation($1, $2::uuid)")
            .bind(&plan.shard)
            .bind(&plan.owner)
            .fetch_one(&mut **tx)
            .await
            .map_err(sql_token)?;
    sqlx::query("SELECT _oo.full_import_bind_session($1, $2::uuid, $3)")
        .bind(&plan.shard)
        .bind(&plan.owner)
        .bind(target_generation)
        .execute(&mut **tx)
        .await
        .map_err(sql_token)?;
    sqlx::query("SELECT _oo.assert_target_fence($1, $2::uuid, $3)")
        .bind(&plan.shard)
        .bind(&plan.owner)
        .bind(target_generation)
        .execute(&mut **tx)
        .await
        .map_err(sql_token)?;
    let input_bytes = decode_hex32(&plan.input_digest).map_err(|_| "digest_mismatch")?;
    let prepared = if plan.arrow.is_empty() {
        None
    } else {
        Some(
            prepare_table(
                &plan.arrow,
                &plan.source_id,
                &plan.schema,
                &plan.table,
                u64::try_from(plan.source_generation).map_err(|_| "stale_run")?,
                &plan.config_digest,
                input_bytes,
                &plan.batch_id,
                &columns_for_prepare(&plan.columns, &plan.column_sources)?,
                Some(&plan.trusted_fields),
            )
            .map_err(arrow_token)?,
        )
    };
    let physical = if let Some(prepared) = prepared.as_ref() {
        prepared.physical_name.clone()
    } else {
        planned_table(plan)?
    };
    let (oids, typmods, required) = if let Some(prepared) = prepared.as_ref() {
        (
            prepared
                .verified_columns
                .iter()
                .map(|column| column.oid)
                .collect::<Vec<_>>(),
            prepared
                .verified_columns
                .iter()
                .map(|column| column.typmod)
                .collect::<Vec<_>>(),
            prepared
                .verified_columns
                .iter()
                .map(|column| column.not_null)
                .collect::<Vec<_>>(),
        )
    } else {
        declared_types(&plan.columns)?
    };
    let run_id: String = sqlx::query_scalar(
        "SELECT _oo.full_import_open_run($1, $2::uuid, $3, $4, $5, $6, $7, $8, $9, $10::text[], $11::text[], $12::text[])::text",
    )
    .bind(&plan.shard)
    .bind(&plan.owner)
    .bind(target_generation)
    .bind(&plan.source_id)
    .bind(plan.source_generation)
    .bind(&plan.config_digest)
    .bind(&plan.type_digest)
    .bind(&plan.input_digest)
    .bind(&plan.schema_digest)
    .bind(vec![plan.schema.as_str()])
    .bind(vec![plan.table.as_str()])
    .bind(vec![physical.as_str()])
    .fetch_one(&mut **tx)
    .await
    .map_err(sql_token)?;
    let shadow: String = sqlx::query_scalar(
        "SELECT _oo.full_import_prepare_shadow($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8, $9, $10, $11::text[], $12::bigint[], $13::integer[], $14::boolean[])",
    )
    .bind(&plan.shard)
    .bind(&plan.owner)
    .bind(target_generation)
    .bind(&run_id)
    .bind(0_i32)
    .bind(&plan.source_id)
    .bind(&plan.config_digest)
    .bind(plan.source_generation)
    .bind(&plan.schema)
    .bind(&plan.table)
    .bind(&plan.column_idents)
    .bind(&oids)
    .bind(&typmods)
    .bind(&required)
    .fetch_one(&mut **tx)
    .await
    .map_err(sql_token)?;
    if shadow != physical {
        return Err("shadow");
    }
    let (previous_ack_exists, previous_signature) = if plan.previous_ack_auto {
        let scope = plan.partition_scope.as_ref().ok_or("partition_scope_required")?;
        let partition = scope["partition_id"].as_str().ok_or("partition_scope_required")?;
        let prior_text: String = sqlx::query_scalar(
            "SELECT _oo.native_partition_prior($1,$2::uuid,$3,$4,$5,$6,$7)::text"
        ).bind(&plan.shard).bind(&plan.owner).bind(target_generation)
         .bind(&plan.source_id).bind(&plan.schema).bind(&plan.table).bind(partition)
         .fetch_one(&mut **tx).await.map_err(sql_token)?;
        let prior: Value = serde_json::from_str(&prior_text).map_err(|_| "partition_prior_ack")?;
        let exists = prior["exists"].as_bool().ok_or("partition_prior_ack")?;
        let signature = prior["signature"].as_str().map(str::to_owned);
        (exists, signature)
    } else { (plan.previous_ack_exists, plan.previous_signature.clone()) };
    let replaced_keys: Vec<String> = if let Some(scope) = plan.partition_scope.as_ref() {
        let scope_text = serde_json::to_string(scope).map_err(|_| "partition_scope_required")?;
        sqlx::query_scalar("SELECT _oo.native_partition_guard($1, $2::uuid, $3, $4, $5, $6, $7, $8, $9::jsonb, $10, $11, $12::text[])")
            .bind(&plan.shard).bind(&plan.owner).bind(target_generation)
            .bind(&plan.source_id).bind(&plan.schema).bind(&plan.table)
            .bind(plan.source_generation).bind(plan.parent_source_generation)
            .bind(&scope_text).bind(previous_ack_exists)
            .bind(&previous_signature).bind(&upsert_keys)
            .fetch_one(&mut **tx).await.map_err(sql_token)?
    } else {
        plan.tombstones.clone()
    };
    let copied: i64 = if let Some(parent) = plan
        .parent_source_generation
        .filter(|_| plan.shape != "snapshot")
    {
        let scope = plan.partition_scope.as_ref().map(serde_json::to_string)
            .transpose().map_err(|_| "partition_scope_required")?;
        let statement = if plan.previous_ack_auto {
            "SELECT _oo.native_olap_copy_forward($1, $2::uuid, $3, $4, $5, $6, $7, $8, $9, $10::text[], $11::text[], $12::text[], $13::text[], $14::jsonb)"
        } else if plan.position_codec == "mssql-ct-version/v1" {
            "SELECT _oo.native_sqlserver_ct_copy_forward($1, $2::uuid, $3, $4, $5, $6, $7, $8, $9, $10::text[], $11::text[], $12::text[], $13::text[])"
        } else {
            "SELECT _oo.native_update_copy_forward($1, $2::uuid, $3, $4, $5, $6, $7, $8, $9, $10::text[], $11::text[], $12::text[], $13::text[])"
        };
        let query = sqlx::query_scalar(statement)
        .bind(&plan.shard)
        .bind(&plan.owner)
        .bind(target_generation)
        .bind(&plan.source_id)
        .bind(&plan.config_digest)
        .bind(&plan.schema)
        .bind(&plan.table)
        .bind(parent)
        .bind(plan.source_generation)
        .bind(&plan.column_idents)
        .bind(&replaced_keys)
        .bind(&upsert_keys)
        .bind(&plan.key_idents);
        let query = if plan.previous_ack_auto { query.bind(scope) } else { query };
        query.fetch_one(&mut **tx)
        .await
        .map_err(sql_token)?
    } else {
        0
    };
    if let Some(prepared) = prepared.as_ref() {
        let mut positions = Vec::with_capacity(upsert_rows);
        for _ in 0..upsert_rows {
            let pos = plan.applied_pos_request.clone();
            positions.push(ExactDecimal::new(false, pos, 0).map_err(|_| "encode")?);
        }
        let quoted = quote_physical_name(&physical).map_err(|_| "physical")?;
        let mut list = String::from("\"_oo_pos\"");
        for column in &prepared.verified_columns {
            list.push_str(", ");
            list.push_str(&quote_physical_name(&column.physical_name).map_err(|_| "physical")?);
        }
        let statement = format!("COPY _oo.{quoted} ({list}) FROM STDIN WITH (FORMAT binary)");
        let budgets = CopyBudgets {
            max_value_bytes: 1_048_576,
            max_row_bytes: 1_048_576,
            max_chunk_bytes: 1_048_576,
            max_rows_per_chunk: 1024,
        };
        let sent = copy_sink::send_batch(
            tx,
            &statement,
            &prepared.batch,
            &prepared.copy_columns,
            &positions,
            budgets,
        )
        .await
        .map_err(sink_token)?;
        if sent.rows != upsert_rows as u64 {
            return Err("rows");
        }
    }
    let expected = copied + i64::try_from(upsert_rows).unwrap_or(i64::MAX);
    let counted: i64 = sqlx::query_scalar("SELECT _oo.native_update_shadow_count($1)")
        .bind(&physical)
        .fetch_one(&mut **tx)
        .await
        .map_err(sql_token)?;
    if counted != expected {
        return Err("readback");
    }
    let reconcile = if plan.truth.is_some() {
        let cells: String =
            sqlx::query_scalar("SELECT _oo.native_update_read_cells($1, $2::text[])")
                .bind(&physical)
                .bind(&column_physical_names(plan, prepared.as_ref())?)
                .fetch_one(&mut **tx)
                .await
                .map_err(sql_token)?;
        let got: Value = serde_json::from_str(&cells).map_err(|_| "cell_mismatch")?;
        if !cells_match(&got, plan.truth.as_ref().ok_or("cell_mismatch")?) {
            return Err("cell_mismatch");
        }
        "ok"
    } else {
        "never"
    };
    // Row ordinals/counts are not source cursor positions, for any codec.
    let applied_pos = plan.applied_pos_request.clone();
    let hook = hook_digest();
    let wrote: String = sqlx::query_scalar(
        "SELECT _oo.full_import_write_batch($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15::numeric, $16, $17, $18::text[], $19::bigint[], $20::integer[], $21::bigint[], $22::text[])",
    )
    .bind(&plan.shard)
    .bind(&plan.owner)
    .bind(target_generation)
    .bind(&run_id)
    .bind(0_i32)
    .bind(&plan.source_id)
    .bind(&physical)
    .bind(&plan.batch_id)
    .bind(&plan.payload_digest)
    .bind(&plan.config_digest)
    .bind(&plan.type_digest)
    .bind(&hook)
    .bind(&plan.schema_digest)
    .bind(&plan.input_digest)
    .bind(&applied_pos)
    .bind(counted)
    .bind(0_i64)
    .bind(Vec::<String>::new())
    .bind(Vec::<i64>::new())
    .bind(Vec::<i32>::new())
    .bind(Vec::<i64>::new())
    .bind(Vec::<String>::new())
    .fetch_one(&mut **tx)
    .await
    .map_err(sql_token)?;
    if wrote != "inserted" {
        return Err("write");
    }
    let canonical = canonical_digest(&plan.payload_digest, counted);
    sqlx::query("SELECT _oo.full_import_note_table($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8)")
        .bind(&plan.shard)
        .bind(&plan.owner)
        .bind(target_generation)
        .bind(&run_id)
        .bind(0_i32)
        .bind(counted)
        .bind(&canonical)
        .bind(&plan.source_id)
        .execute(&mut **tx)
        .await
        .map_err(sql_token)?;
    let seal = if plan.position_codec == "mssql-ct-version/v1" {
        sqlx::query("SELECT _oo.native_sqlserver_ct_seal_quality($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8, $9, $10, $11, $12::numeric, $13, $14, $15, $16, $17, $18, $19::text[], $20, $21, $22, $23)")
    } else {
        sqlx::query("SELECT _oo.native_update_seal_quality($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8, $9, $10, $11, $12::numeric, $13, $14, $15, $16, $17, $18, $19::text[], $20, $21, $22, $23)")
    };
    seal
    .bind(&plan.shard)
    .bind(&plan.owner)
    .bind(target_generation)
    .bind(&run_id)
    .bind(&plan.source_id)
    .bind(plan.source_generation)
    .bind(&plan.schema)
    .bind(&plan.table)
    .bind(&physical)
    .bind(&plan.batch_id)
    .bind(&plan.payload_digest)
    .bind(&applied_pos)
    .bind(if plan.partition_scope.is_some() { "snapshot-zero/v1" } else { &plan.position_codec })
    .bind(&plan.position_lineage)
    .bind(&plan.capture_method)
    .bind(&plan.delete_detection)
    .bind(&plan.consistency_grade)
    .bind(plan.cross_table)
    .bind(&replaced_keys)
    .bind(counted)
    .bind(reconcile)
    .bind(0_i64)
    .bind(plan.mode)
    .execute(&mut **tx)
    .await
    .map_err(sql_token)?;
    if let Some(scope) = plan.partition_scope.as_ref() {
        let scope_text = serde_json::to_string(scope).map_err(|_| "partition_scope_required")?;
        sqlx::query("SELECT _oo.native_partition_commit($1, $2::uuid, $3, $4, $5, $6, $7::jsonb, $8::text[], $9::uuid, $10, $11, $12, $13, $14, $15, $16::uuid, $17)")
            .bind(&plan.shard).bind(&plan.owner).bind(target_generation)
            .bind(&plan.source_id).bind(&plan.schema).bind(&plan.table)
            .bind(&scope_text).bind(&upsert_keys).bind(&run_id)
            .bind(&plan.batch_id).bind(&plan.payload_digest)
            .bind(previous_ack_exists).bind(&previous_signature)
            .bind(&plan.audit_digest).bind(&plan.consistency_grade)
            .bind(plan.lease_id.as_deref().ok_or("lease_required")?)
            .bind(plan.lease_generation.ok_or("lease_required")?)
            .execute(&mut **tx).await.map_err(sql_token)?;
    }
    sqlx::raw_sql(sqlx::AssertSqlSafe("SET CONSTRAINTS ALL IMMEDIATE"))
        .execute(&mut **tx)
        .await
        .map_err(|_| "seal")?;
    let sealed: String = sqlx::query_scalar(
        "SELECT _oo.full_import_seal_run($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8, $9, $10, $11::text[], $12::text[])",
    )
    .bind(&plan.shard)
    .bind(&plan.owner)
    .bind(target_generation)
    .bind(&run_id)
    .bind(&plan.source_id)
    .bind(plan.source_generation)
    .bind(&plan.config_digest)
    .bind(&plan.type_digest)
    .bind(&plan.input_digest)
    .bind(&plan.schema_digest)
    .bind(vec![plan.schema.as_str()])
    .bind(vec![plan.table.as_str()])
    .fetch_one(&mut **tx)
    .await
    .map_err(sql_token)?;
    if sealed != "awaiting_publish" {
        return Err("seal");
    }
    let stage = stage_probe(tx, &physical).await?;
    Ok(Applied {
        run_id,
        target_generation,
        physical_name: physical,
        row_count: counted,
        tombstone_count: replaced_keys.len(),
        receipt: wrote,
        stage_select_granted: stage.as_ref().map(|flags| flags.table_select),
        stage_freshness_select: stage.as_ref().map(|flags| flags.freshness_select),
    })
}

fn column_physical_names(
    plan: &UpdatePlan,
    prepared: Option<&native_arrow::PreparedTable>,
) -> Result<Vec<String>, &'static str> {
    if let Some(prepared) = prepared {
        return Ok(prepared
            .verified_columns
            .iter()
            .map(|column| column.physical_name.clone())
            .collect());
    }
    plan.column_idents
        .iter()
        .map(|ident| planned_column(plan, ident))
        .collect()
}

fn planned_table(plan: &UpdatePlan) -> Result<String, &'static str> {
    use crate::physical_mapping::{Level, MappingRequest, plan_physical_mapping};
    let planned = plan_physical_mapping(&MappingRequest {
        source: plan.source_id.clone(),
        config_digest: plan.config_digest.clone(),
        run_generation: plan.source_generation,
        level: Level::Table,
        logical_namespace: plan.schema.clone(),
        logical_table: plan.table.clone(),
        logical_column: String::new(),
    })
    .map_err(|_| "physical")?;
    Ok(planned.physical_name)
}

fn planned_column(plan: &UpdatePlan, ident: &str) -> Result<String, &'static str> {
    use crate::physical_mapping::{Level, MappingRequest, plan_physical_mapping};
    let planned = plan_physical_mapping(&MappingRequest {
        source: plan.source_id.clone(),
        config_digest: plan.config_digest.clone(),
        run_generation: plan.source_generation,
        level: Level::Column,
        logical_namespace: plan.schema.clone(),
        logical_table: plan.table.clone(),
        logical_column: ident.to_string(),
    })
    .map_err(|_| "physical")?;
    Ok(planned.physical_name)
}

type DeclaredTypes = (Vec<i64>, Vec<i32>, Vec<bool>);

fn declared_types(columns: &[CopyColumnDecl]) -> Result<DeclaredTypes, &'static str> {
    let mut oids = Vec::new();
    let mut typmods = Vec::new();
    let mut required = Vec::new();
    for column in columns {
        let (oid, typmod) = column_storage(column)?;
        oids.push(oid);
        typmods.push(typmod);
        required.push(!column.nullable);
    }
    Ok((oids, typmods, required))
}

fn overlaps(left: &[String], right: &[String]) -> bool {
    left.iter()
        .any(|key| right.iter().any(|other| other == key))
}

fn has_duplicate(keys: &[String]) -> bool {
    let mut seen = keys.to_vec();
    let before = seen.len();
    seen.sort();
    seen.dedup();
    seen.len() != before
}

fn cells_match(got: &Value, truth: &Value) -> bool {
    let Some(got_rows) = got.as_array() else {
        return false;
    };
    let Some(truth_rows) = truth.as_array() else {
        return false;
    };
    if got_rows.len() != truth_rows.len() {
        return false;
    }
    let mut got_sorted = got_rows.clone();
    let mut truth_sorted = truth_rows.clone();
    got_sorted.sort_by_key(row_text);
    truth_sorted.sort_by_key(row_text);
    got_sorted == truth_sorted
}

fn row_text(row: &Value) -> String {
    row.to_string()
}

struct StageFlags {
    table_select: bool,
    freshness_select: bool,
}

async fn stage_probe(
    conn: &mut sqlx::PgConnection,
    physical: &str,
) -> Result<Option<StageFlags>, &'static str> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_stage_probe')",
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(|_| "not_sealed")?;
    if !exists {
        return Ok(None);
    }
    let relation = format!("_oo.{physical}");
    let table_select: bool =
        sqlx::query_scalar("SELECT has_table_privilege('oo_stage_probe', $1, 'SELECT')")
            .bind(&relation)
            .fetch_one(&mut *conn)
            .await
            .map_err(|_| "not_sealed")?;
    let freshness_select: bool = sqlx::query_scalar(
        "SELECT has_table_privilege('oo_stage_probe', '_oo.table_freshness', 'SELECT')",
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(|_| "not_sealed")?;
    Ok(Some(StageFlags {
        table_select,
        freshness_select,
    }))
}

async fn release(conn: &mut sqlx::PgConnection, shard: &str) -> Result<(), &'static str> {
    sqlx::query("SELECT _oo.native_update_release_fence($1)")
        .bind(shard)
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(sql_token)
}

async fn mirror_call(
    conn: &mut sqlx::PgConnection,
    source: &str,
    batch: &str,
    physical: &str,
) -> Result<String, &'static str> {
    let mut tx = conn.begin().await.map_err(|_| "not_ready")?;
    let status: String = match sqlx::query_scalar("SELECT _oo.native_update_mirror($1, $2, $3)")
        .bind(source)
        .bind(batch)
        .bind(physical)
        .fetch_one(&mut *tx)
        .await
    {
        Ok(status) => status,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(sql_token(err));
        }
    };
    tx.commit().await.map_err(|_| "not_sealed")?;
    Ok(status)
}

async fn mirror_only(
    conn: &mut sqlx::PgConnection,
    plan: &UpdatePlan,
) -> Result<String, &'static str> {
    mirror_call(conn, &plan.source_id, &plan.batch_id, &plan.mirror_physical).await
}

async fn jobs_regclass(conn: &mut sqlx::PgConnection) -> Option<String> {
    sqlx::query_scalar("SELECT to_regclass('jobs.tasks')::text")
        .fetch_one(&mut *conn)
        .await
        .ok()
        .flatten()
}

fn jobs_observation(regclass: Option<String>, call: &str) -> Value {
    json!({
        "rust_claim_api": "tools/job-worker",
        "rust_source": "tools/job-worker/CONTRACT.md",
        "sql_file": "crates/oo-jobs/migrations/0003_job_lease_operations_expand.sql",
        "sql_lease_functions": [
            "jobs.enqueue",
            "jobs.claim",
            "jobs.heartbeat",
            "jobs.complete_success",
            "jobs.complete_failure",
            "jobs.retry_failure",
            "jobs.cancel"
        ],
        "target_fence_certified": false,
        "call": call,
        "loader_interface": "oo_pipeline::native_update::trusted_capture",
        "lease_interface": "oo_pipeline::native_update::jobs_lease_interface",
        "import_command": "oo-pipeline import-native-update",
        "tasks_regclass": regclass,
        "leases_regclass": null,
        "control_schema": if regclass.is_some() { "present" } else { "control_schema_absent" },
        "root": JOBS_ROOT
    })
}

/// Read-only description of the lease surface for 74.
///
/// `jobs.claim(uuid, text[], text)` is the writer of a live `jobs.tasks`
/// generation. This loader does not call it again, does not insert
/// `jobs.leases`, and does not set `target_fence_certified`. An import with
/// no `lease_id` is refused before connect. A presented task id is locked in
/// the seal transaction and must match that generation.
pub fn jobs_lease_interface() -> Value {
    let mut value = jobs_observation(None, "not_invoked");
    if let Some(object) = value.as_object_mut() {
        object.insert("jobs_lease_receipt".to_string(), Value::Null);
    }
    value
}

async fn lease_hold(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    lease_id: &str,
    lease_generation: Option<i64>,
) -> Result<(Value, &'static str), &'static str> {
    let regclass: Option<String> = sqlx::query_scalar("SELECT to_regclass('jobs.tasks')::text")
        .fetch_one(&mut **tx)
        .await
        .map_err(lease_sql_token)?;
    if regclass.as_deref() != Some("jobs.tasks") {
        return Err("lease_required");
    }
    type ClaimRow = (String, String, String, i64, Option<String>, bool);
    let row: Option<ClaimRow> = sqlx::query_as(
        "SELECT id, project_id, state, generation, lease_owner, live \
         FROM jobs.loader_hold($1::uuid, $2::bigint)",
    )
    .bind(lease_id)
    .bind(lease_generation)
    .fetch_optional(&mut **tx)
    .await
    .map_err(lease_sql_token)?;
    let Some((id, project, state, generation, owner, live)) = row else {
        return Err("lease_required");
    };
    if state != "running" || owner.is_none() || generation < 1 || !live {
        return Err("fence");
    }
    if lease_generation.is_some_and(|want| want != generation) {
        return Err("fence");
    }
    Ok((
        json!({
            "id": id,
            "project_id": project,
            "state": state,
            "owner": owner,
            "generation": generation,
            "claim": "jobs.claim",
            "target_fence_certified": false,
            "inserted": false
        }),
        "locked",
    ))
}

fn lease_sql_token(err: sqlx::Error) -> &'static str {
    let sqlstate = err
        .as_database_error()
        .and_then(|item| item.code().map(|code| code.to_string()))
        .unwrap_or_default();
    match sqlstate.as_str() {
        "42501" | "28000" => "privilege",
        "22P02" | "22023" => "wrong_data",
        _ => "lease_required",
    }
}

async fn position_ahead(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    plan: &UpdatePlan,
) -> Result<(), &'static str> {
    if plan.parent_source_generation.is_none() || plan.shape == "snapshot" {
        return Ok(());
    }
    if plan.position_codec == "mssql-ct-version/v1" {
        let stored: Option<(String, String, String)> = sqlx::query_as(
            "SELECT applied_pos::text, position_codec, position_lineage FROM _oo.source_watermark WHERE source=$1"
        ).bind(&plan.source_id).fetch_optional(&mut **tx).await.map_err(sql_token)?;
        let (position, codec, lineage) = stored.ok_or("expired")?;
        if codec != plan.position_codec || lineage != plan.position_lineage || plan.ct_prior_position.as_deref() != Some(position.as_str()) { return Err("expired"); }
        let ordering = decimal_cmp(&plan.applied_pos_request, &position)?;
        let empty = decode_arrow_stream(&plan.arrow).map_err(arrow_token)?.rows == 0 && plan.tombstones.is_empty();
        return if ordering == std::cmp::Ordering::Greater || ordering == std::cmp::Ordering::Equal && empty { Ok(()) } else { Err("expired") };
    }
    if plan.position_codec == "mariadb-gtid/v1" || plan.position_codec == "mysql-gtid/v1" {
        let stored: Option<(String, String, String)> = sqlx::query_as(
            "SELECT applied_pos::text, position_codec, position_lineage FROM _oo.source_watermark WHERE source=$1"
        ).bind(&plan.source_id).fetch_optional(&mut **tx).await.map_err(sql_token)?;
        let Some((position, codec, lineage)) = stored else {
            return Ok(());
        };
        if codec != plan.position_codec || lineage != plan.position_lineage {
            return Err("incomparable");
        }
        return if decimal_cmp(&plan.applied_pos_request, &position)? == std::cmp::Ordering::Greater {
            Ok(())
        } else {
            Err("expired")
        };
    }
    if plan.source_engine == "postgresql" || plan.source_engine == "postgres" {
        if plan.position_codec != "pg-commit-lsn/v1" { return Err("expired"); }
        let ahead: bool = sqlx::query_scalar(
            "SELECT _oo.native_pg_position_ahead($1, $2, $3, $4::numeric)"
        ).bind(&plan.source_id).bind(&plan.schema).bind(&plan.table)
         .bind(&plan.applied_pos_request).fetch_one(&mut **tx).await.map_err(sql_token)?;
        return if ahead { Ok(()) } else { Err("expired") };
    }
    let stored: Option<String> =
        sqlx::query_scalar("SELECT applied_pos::text FROM _oo.source_watermark WHERE source = $1")
            .bind(&plan.source_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(sql_token)?;
    let Some(stored) = stored else {
        return Ok(());
    };
    if decimal_cmp(&plan.applied_pos_request, &stored)? != std::cmp::Ordering::Greater {
        return Err("expired");
    }
    Ok(())
}

fn success_result(
    plan: &UpdatePlan,
    applied: &Applied,
    mirror: Option<String>,
    jobs: Option<String>,
    lease_receipt: Value,
    lease_call: &str,
) -> Value {
    json!({
        "protocol": RESULT_PROTOCOL,
        "load_state": "awaiting_publish",
        "freshness_state": "awaiting_publish",
        "reason": null,
        "loader_invoked": true,
        "receipt": applied.receipt,
        "run_id": applied.run_id,
        "target_generation": applied.target_generation,
        "source_generation": plan.source_generation,
        "physical_name": applied.physical_name,
        "row_count": applied.row_count,
        "tombstone_count": applied.tombstone_count,
        "payload_digest": plan.payload_digest,
        "jobs_lease_receipt": lease_receipt,
        "jobs_observation": jobs_observation(jobs, lease_call),
        "source_bound": plan.source_bound,
        "producer_trust": plan.producer_trust.clone(),
        "source_id": plan.source_id,
        "source_engine": plan.source_engine,
        "logical_schema": plan.schema,
        "logical_table": plan.table,
        "mirror": mirror,
        "mirror_committed": mirror.is_some(),
        "stage_select_granted": applied.stage_select_granted,
        "stage_freshness_select": applied.stage_freshness_select,
        "watermark_state": "lagging",
        "row_shape": plan.shape,
        "partition_target_receipt": partition_target_receipt(plan),
        "derived_semantics": plan.derived_semantics.clone().unwrap_or(Value::Null)
    })
}

fn partition_target_receipt(plan: &UpdatePlan) -> Value {
    let Some(scope) = plan.partition_scope.as_ref() else { return Value::Null; };
    let mut receipt = json!({
        "partition_scope": scope,
        "payload_digest": plan.payload_digest,
        "batch_id": plan.batch_id,
        "job_task_id": plan.lease_id,
        "job_generation": plan.lease_generation,
        "acknowledged": false
    });
    if plan.previous_ack_auto {
        receipt["previous_ack_binding"] = json!("target-sidecar/v1");
    } else {
        receipt["previous_ack_exists"] = json!(plan.previous_ack_exists);
        receipt["previous_signature"] = json!(plan.previous_signature);
    }
    receipt
}

fn replay_result(
    plan: &UpdatePlan,
    existing: &StoredBatch,
    mirror: Option<String>,
    jobs: Option<String>,
    lease_receipt: Value,
    lease_call: &str,
) -> Value {
    json!({
        "protocol": RESULT_PROTOCOL,
        "load_state": "replay",
        "freshness_state": "awaiting_publish",
        "reason": null,
        "loader_invoked": false,
        "receipt": "replay",
        "run_id": existing.run_id,
        "target_generation": existing.target_generation,
        "source_generation": plan.source_generation,
        "physical_name": existing.physical_name,
        "row_count": existing.row_count,
        "tombstone_count": if plan.partition_scope.is_some() { Value::Null } else { json!(plan.tombstones.len()) },
        "payload_digest": existing.payload_digest,
        "jobs_lease_receipt": lease_receipt,
        "jobs_observation": jobs_observation(jobs, lease_call),
        "source_bound": plan.source_bound,
        "producer_trust": plan.producer_trust.clone(),
        "source_id": plan.source_id,
        "logical_schema": plan.schema,
        "logical_table": plan.table,
        "mirror": mirror,
        "stage_select_granted": null,
        "watermark_state": "lagging",
        "partition_target_receipt": partition_target_receipt(plan)
    })
}

fn mirror_result(plan: &UpdatePlan, status: &str) -> Value {
    json!({
        "protocol": RESULT_PROTOCOL,
        "load_state": if status == "inserted" || status == "replay" { "replay" } else { "not_sealed" },
        "reason": if status == "inserted" || status == "replay" { Value::Null } else { json!(status) },
        "loader_invoked": false,
        "receipt": status,
        "mirror": status,
        "physical_name": plan.mirror_physical,
        "jobs_lease_receipt": null,
        "source_id": plan.source_id,
        "batch_id": plan.batch_id
    })
}

fn sql_token(err: sqlx::Error) -> &'static str {
    let sqlstate = err
        .as_database_error()
        .and_then(|item| item.code().map(|code| code.to_string()))
        .unwrap_or_default();
    eprintln!("oo-pipeline: sqlstate={sqlstate} {err}");
    match sqlstate.as_str() {
        "OO010" => "OO010",
        "OO013" => "OO013",
        "OO011" => "OO011",
        "OO017" => "OO017",
        "OO024" => "parent_absent",
        "OO030" => "OO030",
        "OO031" => "OO031",
        "42P01" => "not_ready",
        "23514" => "closed",
        "22023" => "closed",
        "22003" => "read_budget",
        _ => "not_sealed",
    }
}

fn sink_token(err: SinkError) -> &'static str {
    match err {
        SinkError::Encode(_) => "encode",
        SinkError::Driver(_) => "copy",
    }
}

async fn connect_target(target: &Value) -> Result<sqlx::PgConnection, &'static str> {
    let host = text_field(target, "host")?;
    let port = target
        .get("port")
        .and_then(|value| value.as_u64())
        .and_then(|value| u16::try_from(value).ok())
        .ok_or("not_ready")?;
    let database = text_field(target, "database")?;
    let username = text_field(target, "runtime_principal")?;
    let password_file = text_field(target, "password_file")?;
    let password = fs::read_to_string(password_file).map_err(|_| "not_ready")?;
    let ssl = target
        .get("ssl")
        .and_then(|value| value.as_str())
        .unwrap_or("verify-full");
    let mut options = PgConnectOptions::from_explicit()
        .host(host)
        .port(port)
        .username(username)
        .password(password.trim())
        .database(database);
    if ssl == "disable" {
        if host != "127.0.0.1" {
            return Err("not_ready");
        }
        options = options.ssl_mode(PgSslMode::Disable);
    } else {
        let ca_file = text_field(target, "ca_file")?;
        let ca = fs::read(ca_file).map_err(|_| "not_ready")?;
        options = options
            .ssl_mode(PgSslMode::VerifyFull)
            .ssl_root_cert_from_pem(ca);
    }
    options = options.disable_statement_logging();
    let mut conn = tokio::time::timeout(Duration::from_secs(30), options.connect())
        .await
        .map_err(|_| "not_ready")?
        .map_err(|_| "not_ready")?;
    sqlx::query("SET statement_timeout = '60s'")
        .execute(&mut conn)
        .await
        .map_err(|_| "not_ready")?;
    Ok(conn)
}

#[allow(clippy::too_many_arguments)]
fn payload_digest(
    source: &str,
    schema: &str,
    table: &str,
    generation: i64,
    parent: Option<i64>,
    batch: &str,
    arrow: &[u8],
    tombstones: &[String],
    columns: &[String],
    config: &str,
    schema_digest: &str,
    type_digest: &str,
    input: &str,
    grade: &str,
    applied_pos: &str,
    audit_digest: &str,
    truth: Option<&Value>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"native-update/v1\n");
    hasher.update(source.as_bytes());
    hasher.update(schema.as_bytes());
    hasher.update(table.as_bytes());
    hasher.update(generation.to_string().as_bytes());
    hasher.update(parent.unwrap_or(0).to_string().as_bytes());
    hasher.update(batch.as_bytes());
    hasher.update(native_arrow::arrow_sha256(arrow).as_bytes());
    for key in tombstones {
        hasher.update(key.as_bytes());
        hasher.update([0]);
    }
    for column in columns {
        hasher.update(column.as_bytes());
        hasher.update([0]);
    }
    hasher.update(config.as_bytes());
    hasher.update(schema_digest.as_bytes());
    hasher.update(type_digest.as_bytes());
    hasher.update(input.as_bytes());
    hasher.update(grade.as_bytes());
    hasher.update([0]);
    hasher.update(applied_pos.as_bytes());
    hasher.update([0]);
    hasher.update(audit_digest.as_bytes());
    hasher.update([0]);
    if let Some(truth) = truth {
        hasher.update(truth.to_string().as_bytes());
    }
    hex_bytes(&hasher.finalize())
}

fn canonical_digest(payload: &str, rows: i64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"native-update/v1 canonical\n");
    hasher.update(payload.as_bytes());
    hasher.update(rows.to_string().as_bytes());
    hex_bytes(&hasher.finalize())
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn decode_hex32(text: &str) -> Result<[u8; 32], ()> {
    if text.len() != 64 {
        return Err(());
    }
    let mut out = [0u8; 32];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).map_err(|_| ())?;
    }
    Ok(out)
}

fn columns_for_prepare(
    columns: &[CopyColumnDecl],
    sources: &[String],
) -> Result<Vec<CopyColumnDecl>, &'static str> {
    if columns.len() != sources.len() {
        return Err("physical");
    }
    let mut out = columns.to_vec();
    for (decl, source) in out.iter_mut().zip(sources) {
        if mapping_ident(source).map_err(|_| "physical")? != decl.logical_name {
            return Err("physical");
        }
        decl.logical_name = source.clone();
    }
    Ok(out)
}

fn parse_columns(document: &Value) -> Result<(Vec<CopyColumnDecl>, Vec<String>), &'static str> {
    let items = document
        .get("columns")
        .and_then(|value| value.as_array())
        .ok_or("bad_type")?;
    let mut columns = Vec::with_capacity(items.len());
    let mut sources = Vec::with_capacity(items.len());
    for item in items {
        let name = text_field(item, "name")?;
        let logical = item
            .get("logical_name")
            .and_then(|value| value.as_str())
            .unwrap_or(name);
        sources.push(logical.to_string());
        let logical_name = mapping_ident(logical).map_err(|_| "bad_type")?;
        columns.push(CopyColumnDecl {
            name: name.to_string(),
            logical_name,
            arrow_type: text_field(item, "arrow_type")?.to_string(),
            nullable: item
                .get("nullable")
                .and_then(|value| value.as_bool())
                .ok_or("bad_type")?,
            precision: item
                .get("precision")
                .and_then(|value| value.as_i64())
                .and_then(|value| i32::try_from(value).ok()),
            scale: item
                .get("scale")
                .and_then(|value| value.as_i64())
                .and_then(|value| i32::try_from(value).ok()),
            char_length: item
                .get("char_length")
                .and_then(|value| value.as_i64())
                .and_then(|value| i32::try_from(value).ok()),
            document: item
                .get("document")
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
        });
    }
    Ok((columns, sources))
}

fn text_list(document: &Value, name: &str) -> Result<Vec<String>, &'static str> {
    match document.get(name) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(value) => {
            let items = value.as_array().ok_or("bad_type")?;
            items
                .iter()
                .map(|item| {
                    item.as_str()
                        .or_else(|| item.as_i64().map(|_| ""))
                        .map(|_| json_text(item))
                        .ok_or("bad_type")
                })
                .collect()
        }
    }
}

fn json_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => String::new(),
    }
}

fn string_list(document: &Value, name: &str) -> Result<Vec<String>, &'static str> {
    let items = document
        .get(name)
        .and_then(|value| value.as_array())
        .ok_or("digest_mismatch")?;
    items
        .iter()
        .map(|item| {
            item.as_str()
                .filter(|text| hex64(text))
                .map(str::to_string)
                .ok_or("digest_mismatch")
        })
        .collect()
}

fn token(document: &Value, name: &str) -> Result<String, &'static str> {
    let text = text_field(document, name)?;
    if ident_text(text) {
        Ok(text.to_string())
    } else {
        Err("closed")
    }
}

fn ident(document: &Value, name: &str) -> Result<String, &'static str> {
    token(document, name)
}

fn ident_text(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn text_field<'a>(value: &'a Value, name: &str) -> Result<&'a str, &'static str> {
    value
        .get(name)
        .and_then(|item| item.as_str())
        .filter(|item| !item.is_empty())
        .ok_or("not_ready")
}

fn hex_field(document: &Value, name: &str) -> Result<String, ()> {
    let text = document
        .get(name)
        .and_then(|value| value.as_str())
        .ok_or(())?;
    if hex64(text) {
        Ok(text.to_string())
    } else {
        Err(())
    }
}

fn hex64(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 64
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn digest_token(_: ()) -> &'static str {
    "digest_missing"
}

fn closed(document: &Value, name: &str, allowed: &[&str]) -> Result<String, &'static str> {
    let text = document
        .get(name)
        .and_then(|value| value.as_str())
        .ok_or("closed")?;
    if allowed.contains(&text) {
        Ok(text.to_string())
    } else {
        Err("closed")
    }
}

fn lineage(document: &Value) -> Result<String, &'static str> {
    let text = text_field(document, "position_lineage")?;
    let bytes = text.as_bytes();
    if (1..=128).contains(&bytes.len())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-'))
    {
        Ok(text.to_string())
    } else {
        Err("closed")
    }
}

fn digits(document: &Value, name: &str) -> Result<String, &'static str> {
    let text = text_field(document, name)?;
    if !text.is_empty() && text.len() <= 38 && text.bytes().all(|byte| byte.is_ascii_digit()) {
        Ok(text.to_string())
    } else {
        Err("closed")
    }
}

fn batch_token(document: &Value) -> Result<String, &'static str> {
    let text = text_field(document, "batch_id")?;
    let bytes = text.as_bytes();
    if (1..=128).contains(&bytes.len())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-'))
    {
        Ok(text.to_string())
    } else {
        Err("closed")
    }
}

fn mapped_name(original: &str) -> Result<String, &'static str> {
    mapping_ident(original).map_err(|_| "physical")
}

fn is_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

/// Closed int64 and utf8 file the fixture and the live CLI share.
pub enum FixtureKind {
    /// Signed 64-bit integer.
    Int64,
    /// UTF-8 text.
    Utf8,
}

/// One cell in [`encode_fixture_arrow`].
pub enum FixtureCell {
    /// int64 value.
    Int(i64),
    /// utf8 value.
    Text(String),
    /// SQL NULL. The column must be nullable.
    Null,
}

/// One fixture column.
pub struct FixtureColumn {
    /// Arrow field name.
    pub name: String,
    /// Nulls are legal.
    pub nullable: bool,
    /// Physical kind.
    pub kind: FixtureKind,
}

/// Encode one Arrow IPC file with an int64 and utf8 schema. This is not a general Arrow writer.
pub fn encode_fixture_arrow(
    columns: &[FixtureColumn],
    rows: &[Vec<FixtureCell>],
) -> Result<Vec<u8>, &'static str> {
    if columns.is_empty() || rows.is_empty() {
        return Err("empty");
    }
    if rows.iter().any(|row| row.len() != columns.len()) {
        return Err("bad_type");
    }
    let body = encode_body(columns, rows)?;
    let schema = schema_meta(columns)?;
    let batch = batch_meta(columns, rows, body.len())?;
    Ok(wrap_file(&schema, &batch, &body))
}

struct Buf {
    data: Vec<u8>,
}

impl Buf {
    fn new() -> Self {
        Self { data: Vec::new() }
    }

    fn len(&self) -> usize {
        self.data.len()
    }

    fn align(&mut self, n: usize) {
        while !self.len().is_multiple_of(n) {
            self.data.push(0);
        }
    }

    fn u8(&mut self, value: u8) {
        self.data.push(value);
    }

    fn u16(&mut self, value: u16) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }

    fn i32(&mut self, value: i32) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }

    fn i64(&mut self, value: i64) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }

    fn patch(&mut self, at: usize, target: usize) -> Result<(), &'static str> {
        let rel =
            u32::try_from(target.checked_sub(at).ok_or("bad_type")?).map_err(|_| "bad_type")?;
        self.data[at..at + 4].copy_from_slice(&rel.to_le_bytes());
        Ok(())
    }
}

fn schema_meta(columns: &[FixtureColumn]) -> Result<Vec<u8>, &'static str> {
    let mut buf = Buf::new();
    buf.u32(0);
    buf.align(8);
    if buf.len() % 8 != 4 {
        buf.u32(0);
    }
    let vtable = buf.len();
    buf.u16(12);
    buf.u16(24);
    buf.u16(0);
    buf.u16(4);
    buf.u16(8);
    buf.u16(16);
    let table = buf.len();
    buf.i32(i32::try_from(table - vtable).map_err(|_| "bad_type")?);
    buf.u8(1);
    buf.u8(0);
    buf.u8(0);
    buf.u8(0);
    let header_slot = buf.len();
    buf.u32(0);
    buf.u32(0);
    buf.i64(0);

    let schema_vtable = buf.len();
    buf.u16(8);
    buf.u16(8);
    buf.u16(0);
    buf.u16(4);
    let schema_table = buf.len();
    buf.i32(i32::try_from(schema_table - schema_vtable).map_err(|_| "bad_type")?);
    let fields_slot = buf.len();
    buf.u32(0);

    buf.align(4);
    let vector_at = buf.len();
    buf.u32(u32::try_from(columns.len()).map_err(|_| "bad_type")?);
    let mut field_slots = Vec::new();
    for _ in columns {
        field_slots.push(buf.len());
        buf.u32(0);
    }
    buf.patch(fields_slot, vector_at)?;

    let mut name_slots = Vec::new();
    let mut type_slots = Vec::new();
    let mut field_tables = Vec::new();
    for column in columns {
        buf.align(4);
        let field_vtable = buf.len();
        buf.u16(12);
        buf.u16(16);
        buf.u16(4);
        buf.u16(8);
        buf.u16(9);
        buf.u16(12);
        let field_table = buf.len();
        buf.i32(i32::try_from(field_table - field_vtable).map_err(|_| "bad_type")?);
        name_slots.push(buf.len());
        buf.u32(0);
        buf.u8(u8::from(column.nullable));
        buf.u8(match column.kind {
            FixtureKind::Int64 => 2,
            FixtureKind::Utf8 => 5,
        });
        buf.u8(0);
        buf.u8(0);
        type_slots.push(buf.len());
        buf.u32(0);
        field_tables.push(field_table);
    }
    for (slot, start) in field_slots.iter().zip(&field_tables) {
        buf.patch(*slot, *start)?;
    }
    for (index, column) in columns.iter().enumerate() {
        buf.align(4);
        let string_at = buf.len();
        buf.u32(u32::try_from(column.name.len()).map_err(|_| "bad_type")?);
        buf.data.extend(column.name.as_bytes());
        buf.patch(name_slots[index], string_at)?;
        buf.align(4);
        match column.kind {
            FixtureKind::Utf8 => {
                let type_vtable = buf.len();
                buf.u16(4);
                buf.u16(4);
                let type_table = buf.len();
                buf.i32(i32::try_from(type_table - type_vtable).map_err(|_| "bad_type")?);
                buf.patch(type_slots[index], type_table)?;
            }
            FixtureKind::Int64 => {
                let type_vtable = buf.len();
                buf.u16(8);
                buf.u16(12);
                buf.u16(4);
                buf.u16(8);
                let type_table = buf.len();
                buf.i32(i32::try_from(type_table - type_vtable).map_err(|_| "bad_type")?);
                buf.i32(64);
                buf.u8(1);
                buf.u8(0);
                buf.u8(0);
                buf.u8(0);
                buf.patch(type_slots[index], type_table)?;
            }
        }
    }
    buf.patch(header_slot, schema_table)?;
    buf.patch(0, table)?;
    Ok(buf.data)
}

fn batch_meta(
    columns: &[FixtureColumn],
    rows: &[Vec<FixtureCell>],
    body_len: usize,
) -> Result<Vec<u8>, &'static str> {
    let mut buf = Buf::new();
    buf.u32(0);
    buf.align(8);
    if buf.len() % 8 != 4 {
        buf.u32(0);
    }
    let vtable = buf.len();
    buf.u16(12);
    buf.u16(24);
    buf.u16(0);
    buf.u16(4);
    buf.u16(8);
    buf.u16(16);
    let table = buf.len();
    buf.i32(i32::try_from(table - vtable).map_err(|_| "bad_type")?);
    buf.u8(3);
    buf.u8(0);
    buf.u8(0);
    buf.u8(0);
    let header_slot = buf.len();
    buf.u32(0);
    buf.u32(0);
    buf.i64(i64::try_from(body_len).map_err(|_| "read_budget")?);

    buf.align(2);
    let batch_vtable = buf.len();
    buf.u16(10);
    buf.u16(24);
    buf.u16(8);
    buf.u16(16);
    buf.u16(20);
    while !buf.len().is_multiple_of(8) {
        buf.u8(0);
    }
    let batch_table = buf.len();
    buf.i32(i32::try_from(batch_table - batch_vtable).map_err(|_| "bad_type")?);
    buf.u32(0);
    buf.i64(i64::try_from(rows.len()).map_err(|_| "bad_type")?);
    let nodes_slot = buf.len();
    buf.u32(0);
    let buffers_slot = buf.len();
    buf.u32(0);

    while buf.len() % 8 != 4 {
        buf.u8(0);
    }
    let nodes_at = buf.len();
    buf.u32(u32::try_from(columns.len()).map_err(|_| "bad_type")?);
    for (index, _column) in columns.iter().enumerate() {
        let nulls = null_count(columns, index, rows)?;
        buf.i64(i64::try_from(rows.len()).map_err(|_| "bad_type")?);
        buf.i64(nulls);
    }
    while buf.len() % 8 != 4 {
        buf.u8(0);
    }
    let buffers_at = buf.len();
    let buffer_count = columns
        .iter()
        .map(|column| match column.kind {
            FixtureKind::Int64 => 2,
            FixtureKind::Utf8 => 3,
        })
        .sum::<usize>();
    buf.u32(u32::try_from(buffer_count).map_err(|_| "bad_type")?);
    let specs = buffer_specs(columns, rows)?;
    for (offset, length) in specs {
        buf.i64(offset);
        buf.i64(length);
    }
    buf.patch(nodes_slot, nodes_at)?;
    buf.patch(buffers_slot, buffers_at)?;
    buf.patch(header_slot, batch_table)?;
    buf.patch(0, table)?;
    Ok(buf.data)
}

fn null_count(
    columns: &[FixtureColumn],
    index: usize,
    rows: &[Vec<FixtureCell>],
) -> Result<i64, &'static str> {
    let column = columns.get(index).ok_or("bad_type")?;
    let mut nulls = 0i64;
    for row in rows {
        let cell = row.get(index).ok_or("bad_type")?;
        if matches!(cell, FixtureCell::Null) {
            if !column.nullable {
                return Err("bad_type");
            }
            nulls += 1;
        }
    }
    Ok(nulls)
}

fn encode_body(
    columns: &[FixtureColumn],
    rows: &[Vec<FixtureCell>],
) -> Result<Vec<u8>, &'static str> {
    let mut body = Vec::new();
    for (index, column) in columns.iter().enumerate() {
        let nulls = rows
            .iter()
            .filter(|row| matches!(row[index], FixtureCell::Null))
            .count();
        if nulls > 0 {
            align_body(&mut body, 8);
            let mut bits = vec![0u8; rows.len().div_ceil(8)];
            for (row_index, row) in rows.iter().enumerate() {
                if !matches!(row[index], FixtureCell::Null) {
                    bits[row_index / 8] |= 1 << (row_index % 8);
                }
            }
            body.extend(bits);
        }
        match column.kind {
            FixtureKind::Int64 => {
                align_body(&mut body, 8);
                for row in rows {
                    let value = match &row[index] {
                        FixtureCell::Int(value) => *value,
                        FixtureCell::Null => 0,
                        FixtureCell::Text(_) => return Err("bad_type"),
                    };
                    body.extend_from_slice(&value.to_le_bytes());
                }
            }
            FixtureKind::Utf8 => {
                align_body(&mut body, 8);
                let mut data: Vec<u8> = Vec::new();
                let mut offsets = Vec::with_capacity(rows.len() + 1);
                offsets.push(0i32);
                for row in rows {
                    match &row[index] {
                        FixtureCell::Text(text) => data.extend(text.as_bytes()),
                        FixtureCell::Null => {}
                        FixtureCell::Int(_) => return Err("bad_type"),
                    }
                    offsets.push(i32::try_from(data.len()).map_err(|_| "read_budget")?);
                }
                for offset in offsets {
                    body.extend_from_slice(&offset.to_le_bytes());
                }
                align_body(&mut body, 8);
                body.extend(data);
            }
        }
    }
    Ok(body)
}

fn buffer_specs(
    columns: &[FixtureColumn],
    rows: &[Vec<FixtureCell>],
) -> Result<Vec<(i64, i64)>, &'static str> {
    let mut cursor = 0usize;
    let mut specs = Vec::new();
    for (index, column) in columns.iter().enumerate() {
        let nulls = rows
            .iter()
            .filter(|row| matches!(row[index], FixtureCell::Null))
            .count();
        if nulls == 0 {
            specs.push((0, 0));
        } else {
            cursor = align_cursor(cursor, 8);
            let length = rows.len().div_ceil(8);
            specs.push((
                i64::try_from(cursor).map_err(|_| "read_budget")?,
                i64::try_from(length).map_err(|_| "read_budget")?,
            ));
            cursor += length;
        }
        match column.kind {
            FixtureKind::Int64 => {
                cursor = align_cursor(cursor, 8);
                let length = rows.len() * 8;
                specs.push((
                    i64::try_from(cursor).map_err(|_| "read_budget")?,
                    i64::try_from(length).map_err(|_| "read_budget")?,
                ));
                cursor += length;
            }
            FixtureKind::Utf8 => {
                cursor = align_cursor(cursor, 8);
                let offset_len = (rows.len() + 1) * 4;
                specs.push((
                    i64::try_from(cursor).map_err(|_| "read_budget")?,
                    i64::try_from(offset_len).map_err(|_| "read_budget")?,
                ));
                cursor += offset_len;
                cursor = align_cursor(cursor, 8);
                let data_len: usize = rows
                    .iter()
                    .map(|row| match &row[index] {
                        FixtureCell::Text(text) => text.len(),
                        _ => 0,
                    })
                    .sum();
                specs.push((
                    i64::try_from(cursor).map_err(|_| "read_budget")?,
                    i64::try_from(data_len).map_err(|_| "read_budget")?,
                ));
                cursor += data_len;
            }
        }
    }
    let _ = cursor;
    Ok(specs)
}

fn align_body(body: &mut Vec<u8>, align: usize) {
    while !body.len().is_multiple_of(align) {
        body.push(0);
    }
}

fn align_cursor(cursor: usize, align: usize) -> usize {
    cursor.saturating_add(align - 1) & !(align - 1)
}

fn wrap_file(schema: &[u8], batch: &[u8], body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"ARROW1");
    out.extend_from_slice(&[0, 0]);
    push_message(&mut out, schema, &[]);
    push_message(&mut out, batch, body);
    out.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
    out.extend_from_slice(&8i32.to_le_bytes());
    out.extend_from_slice(b"ARROW1");
    out
}

fn push_message(out: &mut Vec<u8>, meta: &[u8], body: &[u8]) {
    out.extend_from_slice(&(-1i32).to_le_bytes());
    out.extend_from_slice(&(i32::try_from(meta.len()).unwrap_or(i32::MAX)).to_le_bytes());
    out.extend_from_slice(meta);
    while !out.len().is_multiple_of(8) {
        out.push(0);
    }
    out.extend_from_slice(body);
    while !out.len().is_multiple_of(8) {
        out.push(0);
    }
}

impl std::fmt::Display for target_installer::NativeUpdateInstallOutcome {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Installed => "installed",
            Self::Replay => "replay",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ct_batch_anchor_no_change_and_scope_are_checked() {
        let columns = [FixtureColumn { name: "id".into(), nullable: false, kind: FixtureKind::Int64 }];
        let none: &[Vec<FixtureCell>] = &[];
        let body = encode_body(&columns, none).unwrap();
        let empty = wrap_file(&schema_meta(&columns).unwrap(), &batch_meta(&columns, none, body.len()).unwrap(), &body);
        assert_eq!(decode_arrow_stream(&empty).map(|decoded| decoded.rows), Ok(0));
        let one = encode_fixture_arrow(&columns, &[vec![FixtureCell::Int(1)]]).unwrap();
        let candidate = json!({"protocol":"oo.mssql-preview.ct-position/v1","source_id":"sql1","database":"fixture",
            "database_guid":"11111111-1111-4111-8111-111111111111","recovery_fork_guid":"22222222-2222-4222-8222-222222222222",
            "tables":[{"schema":"dbo","name":"t","object_id":7,"keys":["id"],"schema_sha256":"ab".repeat(32),"version":5}]});
        let capture = json!({"protocol":"oo.mssql-preview.capture/v1","tds_query_executed":true,"source_id":"sql1","source_generation":2,
            "logical_schema":"dbo","logical_table":"t","candidate_position":candidate,"prior_position":candidate,
            "snapshot":{"kind":"mssql_ct_version","value":"5"},"tombstones":[],"primary_key":["id"],"object_id":7,
            "mode":"changes","upserts":0,"min_valid_version":1});
        let request = json!({"source_engine":"sqlserver","position_codec":"mssql-ct-version/v1","capture_method":"c2_change_feed",
            "delete_detection":"native","consistency_grade":"S1","cross_table_consistent":false,"source_id":"sql1","source_generation":2,
            "logical_schema":"dbo","logical_table":"t","sqlserver_candidate_position":candidate,"sqlserver_prior_position":candidate,
            "snapshot":capture["snapshot"],"tombstones":[],"key_columns":["id"],"full_snapshot":false,"applied_pos":"5",
            "position_lineage":"mssql.11111111-1111-4111-8111-111111111111.22222222-2222-4222-8222-222222222222"});
        assert_eq!(validate_mssql_capture(&request, &capture, &empty), Ok(()));
        let mut wrong=request.clone();wrong["applied_pos"]=json!("6");
        assert_eq!(validate_mssql_capture(&wrong,&capture,&empty),Err("ct_scope"));
        wrong=request.clone();wrong["tombstones"]=json!(["1"]);
        assert_eq!(validate_mssql_capture(&wrong,&capture,&empty),Err("ct_scope"));
        let echoed = "\\x000000FF";
        wrong=request.clone();wrong["tombstones"]=json!([echoed]);wrong["applied_pos"]=json!("6");
        wrong["sqlserver_candidate_position"]["tables"][0]["version"]=json!(6);
        wrong["snapshot"]=json!({"kind":"mssql_ct_version","value":"6"});
        let mut echoed_capture=capture.clone();
        echoed_capture["tombstones"]=json!([echoed]);
        echoed_capture["candidate_position"]=wrong["sqlserver_candidate_position"].clone();
        echoed_capture["snapshot"]=wrong["snapshot"].clone();
        assert_eq!(validate_mssql_capture(&wrong,&echoed_capture,&empty),Err("ct_scope"));
        let key = cell_key_text(&NativeCell::Binary(vec![0,0,0,255])).unwrap();
        assert_eq!(key, "\\x000000ff");
        wrong["tombstones"]=json!([key]);
        echoed_capture["tombstones"]=wrong["tombstones"].clone();
        assert_eq!(validate_mssql_capture(&wrong,&echoed_capture,&empty),Ok(()));
        let mut changed=capture.clone();changed["upserts"]=json!(1);changed["candidate_position"]["tables"][0]["version"]=json!(6);
        let mut advanced=request.clone();advanced["applied_pos"]=json!("6");
        advanced["sqlserver_candidate_position"]=changed["candidate_position"].clone();
        advanced["snapshot"]=json!({"kind":"mssql_ct_version","value":"6"});
        changed["snapshot"]=advanced["snapshot"].clone();
        assert_eq!(validate_mssql_capture(&advanced,&changed,&empty),Err("ct_scope"));
        changed["upserts"]=json!(1);
        assert_eq!(validate_mssql_capture(&request,&changed,&one),Err("ct_scope"));
        changed=capture.clone();changed["upserts"]=json!(1);
        assert_eq!(validate_mssql_capture(&request,&changed,&one),Err("expired"));
    }

    fn digests() -> Value {
        let digest = "ab".repeat(32);
        json!({
            "config_digest": digest,
            "schema_digest": digest,
            "type_digest": digest,
            "input_digest": digest,
            "audit_digest": digest
        })
    }

    fn base_request() -> Value {
        let mut request = json!({
            "protocol": REQUEST_PROTOCOL,
            "source_engine": "clickhouse",
            "source_id": "clickhouse_268",
            "source_generation": 1,
            "parent_source_generation": null,
            "logical_schema": "meter",
            "logical_table": "readings",
            "key_column": "reading_id",
            "batch_id": "gen1",
            "capture_method": "c5_diff",
            "delete_detection": "reconcile",
            "consistency_grade": "S0",
            "cross_table_consistent": false,
            "position_codec": "snapshot-zero/v1",
            "position_lineage": "native-update.batch",
            "applied_pos": "0",
            "columns": [
                {"name": "reading_id", "logical_name": "reading_id", "arrow_type": "int64", "nullable": false},
                {"name": "amount", "logical_name": "amount", "arrow_type": "int64", "nullable": false},
                {"name": "note", "logical_name": "note", "arrow_type": "utf8", "nullable": false}
            ],
            "tombstones": [],
            "target": {
                "host": "127.0.0.1",
                "port": 55432,
                "database": "postgres",
                "runtime_principal": "oo_u06_runtime",
                "password_file": "pw",
                "ssl": "disable",
                "shard": "shard_u06",
                "owner": "11111111-1111-1111-1111-111111111111"
            }
        });
        if let Some(object) = request.as_object_mut() {
            for (key, value) in digests().as_object().unwrap().clone() {
                object.insert(key, value);
            }
        }
        request
    }

    #[test]
    fn fixture_arrow_roundtrip_int64_and_utf8() {
        let columns = vec![
            FixtureColumn {
                name: "reading_id".to_string(),
                nullable: false,
                kind: FixtureKind::Int64,
            },
            FixtureColumn {
                name: "amount".to_string(),
                nullable: false,
                kind: FixtureKind::Int64,
            },
            FixtureColumn {
                name: "note".to_string(),
                nullable: false,
                kind: FixtureKind::Utf8,
            },
        ];
        let rows = vec![
            vec![
                FixtureCell::Int(101),
                FixtureCell::Int(10),
                FixtureCell::Text("a".to_string()),
            ],
            vec![
                FixtureCell::Int(102),
                FixtureCell::Int(20),
                FixtureCell::Text("b".to_string()),
            ],
            vec![
                FixtureCell::Int(103),
                FixtureCell::Int(30),
                FixtureCell::Text("c".to_string()),
            ],
        ];
        let bytes = encode_fixture_arrow(&columns, &rows).expect("encode");
        let gen2 = encode_fixture_arrow(
            &columns,
            &[
                vec![
                    FixtureCell::Int(101),
                    FixtureCell::Int(11),
                    FixtureCell::Text("a2".to_string()),
                ],
                vec![
                    FixtureCell::Int(104),
                    FixtureCell::Int(40),
                    FixtureCell::Text("d".to_string()),
                ],
            ],
        )
        .expect("encode gen2");
        let dir = std::env::temp_dir().join("oo-u06-fixtures");
        fs::create_dir_all(&dir).unwrap();
        let conflict = encode_fixture_arrow(
            &columns,
            &[
                vec![
                    FixtureCell::Int(101),
                    FixtureCell::Int(11),
                    FixtureCell::Text("ZZ".to_string()),
                ],
                vec![
                    FixtureCell::Int(104),
                    FixtureCell::Int(40),
                    FixtureCell::Text("d".to_string()),
                ],
            ],
        )
        .expect("encode conflict");
        fs::write(dir.join("gen1.arrow"), &bytes).unwrap();
        fs::write(dir.join("gen2.arrow"), &gen2).unwrap();
        fs::write(dir.join("gen2-conflict.arrow"), &conflict).unwrap();
        let decoded = decode_arrow_stream(&bytes).expect("decode");
        assert_eq!(decoded.rows, 3);
        assert_eq!(decoded.fields[0].arrow_type, "int64");
        assert_eq!(decoded.fields[2].arrow_type, "utf8");
        assert_eq!(
            decoded.columns[0][0],
            NativeCell::Int {
                bit_width: 64,
                value: 101
            }
        );
        assert_eq!(decoded.columns[2][2], NativeCell::Utf8("c".to_string()));
    }

    #[test]
    fn preflight_refuses_before_a_write() {
        let mut request = base_request();
        request.as_object_mut().unwrap().remove("audit_digest");
        assert_eq!(classify(&request).err(), Some("no_audit"));

        let mut request = base_request();
        request["parent_source_generation"] = json!(4);
        request["source_generation"] = json!(2);
        assert_eq!(classify(&request).err(), Some("stale_run"));

        let mut request = base_request();
        with_parent(&mut request);
        request["source_engine"] = json!("oracle");
        let plan = classify(&request).expect("oracle physical without a sealed pair");
        assert!(plan.derived_semantics.is_none());
        request["sealed_catalog"] = json!({"engine": "oracle"});
        assert_eq!(classify(&request).err(), Some("source_bound_required"));

        let mut request = base_request();
        request["source_engine"] = json!("starrocks");
        request["window_closed"] = json!(false);
        request["upsert_supported"] = json!(false);
        assert_eq!(classify(&request).err(), Some("upsert_window_unsupported"));

        let mut request = base_request();
        with_parent(&mut request);
        request["source_engine"] = json!("mongodb");
        request["columns"][0]["name"] = json!("weight");
        assert!(classify(&request).is_ok());

        let mut request = base_request();
        request.as_object_mut().unwrap().remove("config_digest");
        assert_eq!(classify(&request).err(), Some("digest_missing"));

        let mut request = base_request();
        with_parent(&mut request);
        request["columns"][0]["arrow_type"] = json!("utf8");
        assert!(classify(&request).is_ok());

        let mut request = base_request();
        request["columns"][0]["arrow_type"] = json!("float64");
        assert_eq!(classify(&request).err(), Some("bad_type"));

        let mut request = base_request();
        request["consistency_grade"] = json!("S0");
        request["cross_table_consistent"] = json!(true);
        assert_eq!(classify(&request).err(), Some("consistency"));
    }

    #[test]
    fn upsert_key_shared_with_a_tombstone_is_conflict() {
        let columns = vec![
            FixtureColumn {
                name: "reading_id".to_string(),
                nullable: false,
                kind: FixtureKind::Int64,
            },
            FixtureColumn {
                name: "amount".to_string(),
                nullable: false,
                kind: FixtureKind::Int64,
            },
            FixtureColumn {
                name: "note".to_string(),
                nullable: false,
                kind: FixtureKind::Utf8,
            },
        ];
        let rows = vec![vec![
            FixtureCell::Int(101),
            FixtureCell::Int(11),
            FixtureCell::Text("a2".to_string()),
        ]];
        let bytes = encode_fixture_arrow(&columns, &rows).expect("encode");
        let dir = std::env::temp_dir().join("oo-native-update-conflict");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("one.arrow");
        fs::write(&path, bytes).unwrap();
        let mut request = base_request();
        request["source_generation"] = json!(2);
        request["parent_source_generation"] = json!(1);
        request["batch_id"] = json!("gen2");
        request["tombstones"] = json!(["101"]);
        request["arrow_path"] = json!(path.to_string_lossy().to_string());
        assert_eq!(classify(&request).err(), Some("conflict"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn oversized_arrow_is_read_budget() {
        let dir = std::env::temp_dir().join("oo-native-update-budget");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("big.arrow");
        fs::write(&path, vec![0u8; READ_BUDGET + 1]).unwrap();
        let mut request = base_request();
        request["arrow_path"] = json!(path.to_string_lossy().to_string());
        assert_eq!(classify(&request).err(), Some("read_budget"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn declared_utf8_against_int64_arrow_is_bad_type() {
        let columns = vec![FixtureColumn {
            name: "reading_id".to_string(),
            nullable: false,
            kind: FixtureKind::Int64,
        }];
        let rows = vec![vec![FixtureCell::Int(1)]];
        let bytes = encode_fixture_arrow(&columns, &rows).unwrap();
        let dir = std::env::temp_dir().join("oo-native-update-bad-type");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("one.arrow");
        fs::write(&path, bytes).unwrap();
        let mut request = base_request();
        request["columns"] = json!([
            {"name": "reading_id", "logical_name": "reading_id", "arrow_type": "utf8", "nullable": false}
        ]);
        request["arrow_path"] = json!(path.to_string_lossy().to_string());
        assert_eq!(classify(&request).err(), Some("bad_type"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn historical_receipt_on_oracle_is_cross_source() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../evidence/2ecaf08e-eb23-4b7a-a720-e07459e6f2a6/DL01/budget-report/native-readonly-completion/receipt.json",
        );
        if !path.is_file() {
            return;
        }
        let digest = "ab".repeat(32);
        let mut request = base_request();
        request["source_engine"] = json!("doris");
        request["admission_path"] = json!(path.to_string_lossy().to_string());
        request["admission_expect"] = json!({
            "source_engine": "doris",
            "source_id": "clickhouse_268",
            "source_generation": 1,
            "snapshot": {"kind": "none", "value": "none"},
            "schema_digest": digest,
            "type_digest": digest,
            "config_digest": digest,
            "input_digest": digest,
            "catalog_sha256": digest,
            "registry_sha256": digest,
            "driver_digest": digest,
            "arrow_schema_sha256": digest,
            "arrow_sha256": [digest]
        });
        assert_eq!(classify(&request).err(), Some("cross_source"));
    }

    #[test]
    fn producer_receipt_binds_the_document_not_the_expect() {
        let dir = std::env::temp_dir().join("oo-native-update-trust");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let columns = vec![FixtureColumn {
            name: "reading_id".to_string(),
            nullable: false,
            kind: FixtureKind::Int64,
        }];
        let rows = vec![vec![FixtureCell::Int(101)]];
        let arrow = encode_fixture_arrow(&columns, &rows).expect("arrow");
        let arrow_path = dir.join("one.arrow");
        fs::write(&arrow_path, &arrow).unwrap();
        let arrow_sha = native_arrow::arrow_sha256(&arrow);
        let schema_sha = native_arrow::arrow_schema_sha256(&arrow).expect("schema");
        let mut document = base_request();
        document["snapshot"] = json!({"kind": "registry", "value": "snap-u06"});
        document["columns"] = json!([
            {"name": "reading_id", "logical_name": "reading_id", "arrow_type": "int64", "nullable": false}
        ]);
        document["arrow_path"] = json!(arrow_path.to_string_lossy().to_string());
        let body = json!({
            "schema": crate::native_binding::CAPTURE_RECEIPT_PROTOCOL,
            "source_engine": document["source_engine"],
            "source_id": document["source_id"],
            "source_generation": document["source_generation"],
            "snapshot": {"kind": "registry", "value": "snap-u06"},
            "run_id": "run-u06",
            "schema_digest": document["schema_digest"],
            "type_digest": document["type_digest"],
            "config_digest": document["config_digest"],
            "input_digest": document["input_digest"],
            "audit_digest": document["audit_digest"],
            "catalog_sha256": document["schema_digest"],
            "registry_sha256": document["schema_digest"],
            "driver_digest": document["schema_digest"],
            "arrow_sha256": [arrow_sha],
            "arrow_schema_sha256": schema_sha
        });
        let bytes = serde_json::to_vec(&body).unwrap();
        let receipt = dir.join("receipt.json");
        fs::write(&receipt, &bytes).unwrap();
        let receipt_sha = hex_bytes(&Sha256::digest(&bytes));
        let path = receipt.to_string_lossy().to_string();
        document["admission_path"] = json!(path);
        document["admission_expect"] = json!({
            "source_engine": "doris",
            "source_id": "oo_u06_fix",
            "source_generation": 9,
            "declared_receipt_sha256": receipt_sha
        });
        assert_eq!(classify(&document).err(), Some("cross_source"));
        assert!(trusted_capture(&document, &arrow).is_err());

        let mut aligned = document.clone();
        aligned["admission_expect"] = json!({
            "source_engine": "clickhouse",
            "source_id": "clickhouse_268",
            "source_generation": 1,
            "declared_receipt_sha256": receipt_sha
        });
        assert_eq!(classify(&aligned).err(), Some("nonadmitted"));
        assert!(trusted_capture(&aligned, &arrow).is_err());
        let lease = jobs_lease_interface();
        assert!(lease["jobs_lease_receipt"].is_null());
        assert_eq!(lease["rust_claim_api"], json!("tools/job-worker"));
        assert_eq!(
            lease["import_command"],
            json!("oo-pipeline import-native-update")
        );
        assert_eq!(lease["target_fence_certified"], json!(false));
        assert_eq!(lease["call"], json!("not_invoked"));

        let mut swapped = aligned.clone();
        swapped["source_id"] = json!("oo_u06_fix");
        swapped["admission_expect"]["source_id"] = json!("oo_u06_fix");
        assert_eq!(classify(&swapped).err(), Some("cross_source"));
        assert!(trusted_capture(&swapped, &arrow).is_err());

        let mut generation = aligned.clone();
        generation["source_generation"] = json!(2);
        generation["parent_source_generation"] = json!(1);
        generation["applied_pos"] = json!("1");
        generation["admission_expect"]["source_generation"] = json!(2);
        assert_eq!(classify(&generation).err(), Some("stale_run"));

        let mut tampered = aligned.clone();
        tampered["schema_digest"] = json!("cd".repeat(32));
        assert_eq!(classify(&tampered).err(), Some("digest_mismatch"));

        let mut echoed = aligned.clone();
        echoed["admission_expect"]["declared_receipt_sha256"] = document["schema_digest"].clone();
        assert_eq!(classify(&echoed).err(), Some("digest_mismatch"));
        assert!(trusted_capture(&echoed, &arrow).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    fn splice_stamp(bytes: &[u8], from: &str, to: &str) -> Vec<u8> {
        let position = bytes
            .windows(from.len())
            .position(|window| window == from.as_bytes())
            .expect("stamp");
        let mut out = bytes[..position].to_vec();
        out.extend_from_slice(to.as_bytes());
        out.extend_from_slice(&bytes[position + from.len()..]);
        out
    }

    #[test]
    fn server_registration_preimage_cannot_self_authorize() {
        let dir = std::env::var("OO_U06_REG_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir().join("oo-native-update-registration"));
        let keep = std::env::var_os("OO_U06_KEEP_REGISTRATION").is_some();
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let columns = vec![FixtureColumn {
            name: "reading_id".to_string(),
            nullable: false,
            kind: FixtureKind::Int64,
        }];
        let arrow = encode_fixture_arrow(&columns, &[vec![FixtureCell::Int(101)]]).expect("arrow");
        let arrow_path = dir.join("batch.arrow");
        fs::write(&arrow_path, &arrow).unwrap();
        let arrow_sha = native_arrow::arrow_sha256(&arrow);
        let arrow_schema = native_arrow::arrow_schema_sha256(&arrow).expect("schema");
        let schema_digest = "ab".repeat(32);
        assert_ne!(schema_digest, arrow_schema);
        let type_digest = "cd".repeat(32);
        let config_digest = "ef".repeat(32);
        let driver = b"driver-u06\n";
        let audit = b"audit-u06\n";
        let driver_path = dir.join("driver.bin");
        let audit_path = dir.join("audit.json");
        fs::write(&driver_path, driver).unwrap();
        fs::write(&audit_path, audit).unwrap();
        let driver_sha = hex_bytes(&Sha256::digest(driver));
        let audit_sha = hex_bytes(&Sha256::digest(audit));
        let catalog = json!({
            "protocol": native_binding::CATALOG_PROTOCOL,
            "engine": "clickhouse",
            "source_id": "clickhouse_268",
            "source_generation": 1,
            "schema_digest": schema_digest,
            "type_digest": type_digest,
            "config_digest": config_digest,
            "input_digest": arrow_sha
        });
        let catalog_path = dir.join("catalog.json");
        let catalog_bytes = serde_json::to_vec(&catalog).unwrap();
        fs::write(&catalog_path, &catalog_bytes).unwrap();
        let catalog_sha = hex_bytes(&Sha256::digest(&catalog_bytes));
        let receipt_path = dir.join("receipt.json");
        let registry_path = dir.join("registry.json");
        let blank = native_binding::REGISTRY_STAMP_BLANK;
        let registry_blank = serde_json::to_vec(&json!({
            "protocol": native_binding::BINDING_REGISTRY_PROTOCOL,
            "bindings": [{
                "protocol": native_binding::CAPTURE_BINDING_PROTOCOL,
                "source_engine": "clickhouse",
                "source_id": "clickhouse_268",
                "source_generation": 1,
                "admission_receipt_path": receipt_path.to_string_lossy(),
                "admission_receipt_sha256": blank,
                "catalog_path": catalog_path.to_string_lossy(),
                "catalog_sha256": catalog_sha,
                "driver_path": driver_path.to_string_lossy(),
                "driver_sha256": driver_sha,
                "driver_digest": driver_sha,
                "capture_receipt_path": audit_path.to_string_lossy(),
                "capture_receipt_sha256": audit_sha,
                "schema_digest": schema_digest,
                "type_digest": type_digest,
                "config_digest": config_digest,
                "input_digest": arrow_sha,
                "arrow_schema_sha256": arrow_schema,
                "objects": [{"arrow_sha256": arrow_sha}]
            }]
        }))
        .unwrap();
        assert_eq!(
            registry_blank
                .windows(blank.len())
                .filter(|window| *window == blank.as_bytes())
                .count(),
            1
        );
        let preimage = hex_bytes(&Sha256::digest(&registry_blank));
        let receipt = serde_json::to_vec(&json!({
            "schema": native_binding::CAPTURE_RECEIPT_PROTOCOL,
            "source_engine": "clickhouse",
            "source_id": "clickhouse_268",
            "source_generation": 1,
            "snapshot": {"kind": "registry", "value": "snap-u06"},
            "schema_digest": schema_digest,
            "type_digest": type_digest,
            "config_digest": config_digest,
            "input_digest": arrow_sha,
            "audit_digest": audit_sha,
            "catalog_sha256": catalog_sha,
            "registry_sha256": preimage,
            "driver_digest": driver_sha,
            "arrow_schema_sha256": arrow_schema,
            "arrow_sha256": [arrow_sha],
            "consistency": "T",
            "cross_table_consistency": "T"
        }))
        .unwrap();
        let receipt_sha = hex_bytes(&Sha256::digest(&receipt));
        let registry = splice_stamp(&registry_blank, blank, &receipt_sha);
        fs::write(&receipt_path, &receipt).unwrap();
        fs::write(&registry_path, &registry).unwrap();
        let mut opened = native_binding::open_server_registration(
            &registry,
            &receipt,
            &arrow_sha,
            Some(arrow_schema.as_str()),
        )
        .expect("open");
        assert_eq!(opened.preimage_sha256, preimage);
        assert_eq!(opened.receipt_sha256, receipt_sha);
        assert_eq!(opened.consistency, "T");
        assert!(opened.cross_table);
        assert_ne!(hex_bytes(&Sha256::digest(&registry)), preimage);

        let mut document = base_request();
        document["snapshot"] = json!({"kind": "registry", "value": "snap-u06"});
        document["consistency_grade"] = json!("T");
        document["cross_table_consistent"] = json!(true);
        document["columns"] = json!([
            {"name": "reading_id", "logical_name": "reading_id", "arrow_type": "int64", "nullable": false}
        ]);
        document["arrow_path"] = json!(arrow_path.to_string_lossy().to_string());
        document["admission_path"] = json!(receipt_path.to_string_lossy().to_string());
        document["registry_path"] = json!(registry_path.to_string_lossy().to_string());
        document["schema_digest"] = json!(schema_digest);
        document["type_digest"] = json!(type_digest);
        document["config_digest"] = json!(config_digest);
        document["input_digest"] = json!(arrow_sha);
        document["audit_digest"] = json!(audit_sha);
        native_binding::with_operator_env(native_binding::OperatorEnv::Absent, || {
            assert_eq!(classify(&document).err(), Some("nonadmitted"));
            assert_eq!(trusted_capture(&document, &arrow).err(), Some("nonadmitted"));
        });
        if keep {
            let good_receipt = dir.join("receipt-good.json");
            let good_registry = dir.join("registry-good.json");
            fs::copy(&receipt_path, &good_receipt).unwrap();
            fs::copy(&registry_path, &good_registry).unwrap();
            let mut saved = document.clone();
            saved["admission_path"] = json!(good_receipt.to_string_lossy().to_string());
            saved["registry_path"] = json!(good_registry.to_string_lossy().to_string());
            saved["mirror"] = json!(false);
            fs::write(
                dir.join("request-good.json"),
                serde_json::to_vec_pretty(&saved).unwrap(),
            )
            .unwrap();
        }

        let mut client_grade = document.clone();
        client_grade["consistency_grade"] = json!("S0");
        client_grade["cross_table_consistent"] = json!(false);
        assert_eq!(classify(&client_grade).err(), Some("nonadmitted"));

        let full_registry = hex_bytes(&Sha256::digest(&registry));
        let circular_receipt = serde_json::to_vec(&json!({
            "schema": native_binding::CAPTURE_RECEIPT_PROTOCOL,
            "source_engine": "clickhouse",
            "source_id": "clickhouse_268",
            "source_generation": 1,
            "snapshot": {"kind": "registry", "value": "snap-u06"},
            "schema_digest": schema_digest,
            "type_digest": type_digest,
            "config_digest": config_digest,
            "input_digest": arrow_sha,
            "audit_digest": audit_sha,
            "catalog_sha256": catalog_sha,
            "registry_sha256": full_registry,
            "driver_digest": driver_sha,
            "arrow_schema_sha256": arrow_schema,
            "arrow_sha256": [arrow_sha],
            "consistency": "T",
            "cross_table_consistency": "T"
        }))
        .unwrap();
        let circular_sha = hex_bytes(&Sha256::digest(&circular_receipt));
        let circular_registry = splice_stamp(&registry_blank, blank, &circular_sha);
        fs::write(&receipt_path, &circular_receipt).unwrap();
        fs::write(&registry_path, &circular_registry).unwrap();
        assert_eq!(classify(&document).err(), Some("nonadmitted"));
        if keep {
            fs::write(
                dir.join("request-circular.json"),
                serde_json::to_vec_pretty(&document).unwrap(),
            )
            .unwrap();
        } else {
            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn parented_zero_position_expires_and_a_snapshot_does_not() {
        let mut request = base_request();
        request["source_generation"] = json!(2);
        request["parent_source_generation"] = json!(1);
        request["applied_pos"] = json!("0");
        assert_eq!(classify(&request).err(), Some("expired"));
        request["full_snapshot"] = json!(true);
        let plan = classify(&request).expect("snapshot");
        assert_eq!(plan.shape, "snapshot");
        request["full_snapshot"] = json!(false);
        request["applied_pos"] = json!("1");
        assert!(classify(&request).is_ok());
        assert_eq!(decimal_add("9", 1).unwrap(), "10");
        assert_eq!(decimal_add("1", 0).unwrap(), "1");
        assert!(decimal_add(&"9".repeat(38), 1).is_err());
    }

    #[test]
    fn mariadb_gtid_is_one_domain_server_sequence() {
        assert!(maria_stored("d0.s1", "6"));
        assert!(!maria_stored("d0.s1", "0-1-6"));
        assert!(!maria_stored("0-1-6", "6"));
        assert!(!maria_stored("d0.s1,d1.s1", "6"));
        assert!(!maria_stored("d0.s0", "6"));
        assert!(!maria_stored("d0.s1", "06"));
        assert!(!maria_stored("d00.s1", "6"));
        assert_eq!(maria_order("d0.s1", "6", "d0.s1", "7").unwrap(), std::cmp::Ordering::Less);
        assert_eq!(maria_order("d0.s1", "10", "d0.s1", "6").unwrap(), std::cmp::Ordering::Greater);
        assert_eq!(maria_order("d0.s1", "6", "d1.s1", "100"), Err("incomparable"));
        assert_eq!(maria_order("d0.s1", "6", "d0.s2", "7"), Err("incomparable"));
        assert_eq!(maria_order("d0.s1", "0-1-6", "d0.s1", "7"), Err("gtid_identity"));

        let mut request = base_request();
        request["source_engine"] = json!("mariadb");
        request["position_codec"] = json!("mysql-binlog-pos/v1");
        request["applied_pos"] = json!("328");
        assert_eq!(classify(&request).err(), Some("binlog_pos_not_durable"));

        request["position_codec"] = json!("mariadb-gtid/v1");
        request["position_lineage"] = json!("0-1-6");
        request["applied_pos"] = json!("6");
        request["capture_method"] = json!("c1_cdc");
        request["delete_detection"] = json!("native");
        request["consistency_grade"] = json!("T");
        request["cross_table_consistent"] = json!(true);
        assert_eq!(classify(&request).err(), Some("gtid_identity"));

        request["source_engine"] = json!("mysql");
        request["position_lineage"] = json!("d0.s1");
        assert_eq!(classify(&request).err(), Some("gtid_family"));

        request["source_engine"] = json!("mariadb");
        request["source_generation"] = json!(2);
        request["parent_source_generation"] = json!(1);
        assert_eq!(classify(&request).err(), Some("consistency"));

        let mut batch = base_request();
        batch["source_engine"] = json!("mariadb");
        assert_eq!(classify(&batch).err(), Some("empty"));
    }

    #[test]
    fn mysql_gtid_grammar_is_one_uuid_and_gno() {
        let mut request = base_request();
        request["source_engine"] = json!("mysql");
        request["position_codec"] = json!("mysql-gtid/v1");
        request["position_lineage"] = json!("mysql.not-a-uuid");
        request["applied_pos"] = json!("7");
        request["capture_method"] = json!("c1_cdc");
        request["delete_detection"] = json!("native");
        request["consistency_grade"] = json!("T");
        request["cross_table_consistent"] = json!(true);
        assert_eq!(classify(&request).err(), Some("gtid_identity"));
        request["position_lineage"] = json!("mysql.aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
        request["source_generation"] = json!(2);
        request["parent_source_generation"] = json!(1);
        assert_eq!(classify(&request).err(), Some("consistency"));
        request["source_engine"] = json!("mariadb");
        assert_eq!(classify(&request).err(), Some("gtid_family"));
    }

    #[test]
    fn gtid_commit_identity_uses_the_captured_spool_and_refuses_sets() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testing/dl01-driver-preflight/mariadb/fixtures/c1-gtid-0-1-7");
        let spool_bytes = fs::read(dir.join("spool.ndjson")).expect("spool");
        let signature: Value = serde_json::from_slice(&fs::read(dir.join("signature.json")).expect("signature")).unwrap();
        assert_eq!(signature["spool_sha256"], hex_bytes(&Sha256::digest(&spool_bytes)));
        assert_eq!(signature["ack_allowed"], false);
        let lines: Vec<Value> = std::str::from_utf8(&spool_bytes)
            .unwrap()
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let ops: Vec<&str> = lines.iter().filter_map(|line| line["op"].as_str()).collect();
        assert_eq!(ops, ["c", "u", "d"]);
        assert_eq!(lines.last().unwrap()["txn_id"], "0-1-7");
        assert!(lines.last().unwrap().get("source_epoch").is_none());
        assert_eq!(gtid_commit_identity(lines.last().unwrap()).err(), Some("gtid_identity"));
        let positive: Value = serde_json::from_slice(&fs::read(dir.join("positive-maria.json")).unwrap()).unwrap();
        let parsed = gtid_commit_identity(&positive).expect("maria");
        assert_eq!(parsed.codec, "mariadb-gtid/v1");
        assert_eq!(parsed.lineage, "d0.s1");
        assert_eq!(parsed.applied_pos, "7");
        assert_eq!(parsed.txn_id, "0-1-7");
        assert_eq!(parsed.epoch, 1);
        assert_ne!(parsed.txn_id, positive["resume_file"]);
        assert_ne!(parsed.applied_pos, positive["resume_pos"]);
        let mut without_resume = positive.clone();
        without_resume.as_object_mut().unwrap().remove("resume_file");
        without_resume.as_object_mut().unwrap().remove("resume_pos");
        assert_eq!(gtid_commit_identity(&without_resume).unwrap(), parsed);
        assert_eq!(
            gtid_scope_digest("logistics", &parsed),
            gtid_scope_digest("logistics", &gtid_commit_identity(&without_resume).unwrap())
        );
        let mysql: Value = serde_json::from_slice(&fs::read(dir.join("positive-mysql.json")).unwrap()).unwrap();
        let mysql_id = gtid_commit_identity(&mysql).expect("mysql");
        assert_eq!(mysql_id.codec, "mysql-gtid/v1");
        assert_eq!(mysql_id.applied_pos, "7");
        assert!(mysql_id.txn_id.ends_with(":7"));
        assert!(!mysql_id.txn_id.contains("binlog"));
        assert_ne!(mysql_id.applied_pos, mysql["resume_pos"]);
        let mut other_epoch = mysql.clone();
        other_epoch["source_epoch"] = json!(2);
        let other = gtid_commit_identity(&other_epoch).unwrap();
        assert_eq!(gtid_order(&mysql_id, &other), Err("incomparable"));
        let negatives: Vec<Value> = serde_json::from_slice(&fs::read(dir.join("negative.json")).unwrap()).unwrap();
        assert!(negatives.len() >= 8);
        for case in &negatives {
            assert_eq!(
                gtid_commit_identity(&case["entry"]).err().map(str::to_string),
                case["error"].as_str().map(str::to_string),
                "{}",
                case["name"]
            );
        }
        let lease: Value = serde_json::from_slice(&fs::read(dir.join("lease.json")).unwrap()).unwrap();
        assert_eq!(lease["ack_allowed"], false);
        assert!(lease["lease_id"].is_null());
        let rules: Value = serde_json::from_slice(&fs::read(dir.join("source-rules.json")).unwrap()).unwrap();
        let gold: Value = serde_json::from_slice(&fs::read(dir.join("source-money-gold.json")).unwrap()).unwrap();
        let mut money = Vec::new();
        for column in rules["columns"].as_array().unwrap() {
            if column["semantic_type"] == "identifier" {
                assert_eq!(column["pii"], "unknown");
            }
            if column["semantic_type"] == "money" {
                assert_eq!(column["unit"], "unspecified");
                assert_eq!(column["pii"], "unknown");
                assert_ne!(column["unit"], "USD");
                money.push(column["column"].as_str().unwrap().to_string());
            }
        }
        money.sort();
        assert_eq!(
            money,
            vec!["cust_east.k_note.money".to_string(), "logistics.shipments.cost_amount".to_string()]
        );
        assert_eq!(gold["observed"]["currency_columns"], 0);
        assert_eq!(gold["observed"]["hub_9000000000001"], 0);
        assert_eq!(gold["s5"], "not_called");
    }

    #[test]
    fn payload_digest_covers_truth_grade_position_and_audit() {
        let digest = "ab".repeat(32);
        let base = payload_digest(
            "clickhouse_268",
            "meter",
            "readings",
            1,
            None,
            "gen1",
            &[],
            &[],
            &[],
            &digest,
            &digest,
            &digest,
            &digest,
            "S0",
            "1",
            &digest,
            None,
        );
        let truth = payload_digest(
            "clickhouse_268",
            "meter",
            "readings",
            1,
            None,
            "gen1",
            &[],
            &[],
            &[],
            &digest,
            &digest,
            &digest,
            &digest,
            "S0",
            "1",
            &digest,
            Some(&json!([{"note": "changed"}])),
        );
        let grade = payload_digest(
            "clickhouse_268",
            "meter",
            "readings",
            1,
            None,
            "gen1",
            &[],
            &[],
            &[],
            &digest,
            &digest,
            &digest,
            &digest,
            "S1",
            "1",
            &digest,
            None,
        );
        let position = payload_digest(
            "clickhouse_268",
            "meter",
            "readings",
            1,
            None,
            "gen1",
            &[],
            &[],
            &[],
            &digest,
            &digest,
            &digest,
            &digest,
            "S0",
            "2",
            &digest,
            None,
        );
        let audit = payload_digest(
            "clickhouse_268",
            "meter",
            "readings",
            1,
            None,
            "gen1",
            &[],
            &[],
            &[],
            &digest,
            &digest,
            &digest,
            &digest,
            "S0",
            "1",
            &"cd".repeat(32),
            None,
        );
        assert_ne!(base, truth);
        assert_ne!(base, grade);
        assert_ne!(base, position);
        assert_ne!(base, audit);
        let mut request = base_request();
        with_parent(&mut request);
        request["consistency_grade"] = json!("T");
        assert_eq!(classify(&request).err(), Some("consistency"));
    }

    #[tokio::test]
    async fn empty_lease_is_refused_before_connect() {
        let mut request = base_request();
        with_parent(&mut request);
        let result = execute(&request).await.expect("refusal");
        assert_eq!(result["load_state"], json!("not_sealed"));
        assert_eq!(result["reason"], json!("lease_required"));
        assert!(result["jobs_lease_receipt"].is_null());
        assert_eq!(
            result["jobs_observation"]["target_fence_certified"],
            json!(false)
        );
        assert_eq!(result["source_bound"], json!(false));
    }

    #[test]
    fn oracle_producer_catalog_seals_int16_and_refuses_a_crossed_source() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/c5-capture",
        );
        let arrow_path = root.join("run/arrow/t_wide-gen-1.arrow");
        let catalog_path = root.join("run/producer-catalog.json");
        if !arrow_path.is_file() || !catalog_path.is_file() {
            return;
        }
        for (name, rows, kind) in [
            ("t_batch-gen-1.arrow", 5usize, "D:timestamp_micros"),
            ("t_batch-gen-3.arrow", 6, "N1:int8"),
            ("t_batch-gen-8.arrow", 5, "C5_NOTE:utf8"),
            ("t_ts-gen-1.arrow", 2, "TS:utf8"),
            ("t_wide-gen-1.arrow", 4, "N_FREE:utf8"),
        ] {
            let bytes = fs::read(root.join("run/arrow").join(name)).unwrap();
            let stream = decode_arrow_stream(&bytes).unwrap_or_else(|err| panic!("{name} {err:?}"));
            assert_eq!(stream.rows, rows, "{name}");
            assert_eq!(stream.fields[0].arrow_type, "int16", "{name}");
            let kinds = stream
                .fields
                .iter()
                .map(|field| format!("{}:{}", field.name, field.arrow_type))
                .collect::<Vec<_>>()
                .join(",");
            assert!(kinds.contains(kind), "{name} {kinds}");
        }
        let arrow = fs::read(&arrow_path).unwrap();
        let decoded = decode_arrow_stream(&arrow).expect("wide");
        assert_eq!(decoded.fields[0].arrow_type, "int16");
        assert_eq!(decoded.fields[1].arrow_type, "utf8");
        let mut columns = Vec::new();
        for field in &decoded.fields {
            columns.push(json!({
                "name": field.name,
                "logical_name": field.name,
                "arrow_type": field.arrow_type,
                "nullable": field.nullable
            }));
        }
        let decls = columns
            .iter()
            .map(|item| CopyColumnDecl {
                name: item["name"].as_str().unwrap().to_string(),
                logical_name: mapping_ident(item["name"].as_str().unwrap()).unwrap(),
                arrow_type: item["arrow_type"].as_str().unwrap().to_string(),
                nullable: item["nullable"].as_bool().unwrap(),
                precision: None,
                scale: None,
                char_length: None,
                document: false,
            })
            .collect::<Vec<_>>();
        let schema = mapping_ident("oo04").unwrap();
        let table = mapping_ident("t_wide").unwrap();
        let sealed = seal_schema_digest(&schema, &table, &decls).expect("seal");
        assert_eq!(decls[0].logical_name, "q_4944");
        assert_eq!(decls[1].logical_name, "q_4e5f46524545");
        assert_eq!(
            sealed,
            "be89ad85dde881bcda1e24907db7d26637c4b6bad4900575fe88bffe81a89ed5"
        );
        let batch_decls = [
            ("ID", "int16", None, None, 21, -1),
            ("N16", "decimal128", Some(16), Some(6), 1700, 1_048_586),
            ("N1", "int8", Some(8), None, 21, -1),
            ("D", "timestamp_micros", None, None, 1114, -1),
            ("C6", "utf8", None, None, 1043, -1),
            ("V2", "utf8", None, None, 1043, -1),
            ("R", "binary", None, None, 17, -1),
            ("C5_NOTE", "utf8", None, None, 1043, -1),
        ];
        let batch_columns = batch_decls
            .iter()
            .map(|(name, kind, precision, scale, _, _)| CopyColumnDecl {
                name: (*name).to_string(),
                logical_name: mapping_ident(name).unwrap(),
                arrow_type: (*kind).to_string(),
                nullable: *name != "ID",
                precision: *precision,
                scale: *scale,
                char_length: None,
                document: false,
            })
            .collect::<Vec<_>>();
        let batch_sealed = seal_schema_digest("oo04", "t_batch", &batch_columns).unwrap();
        assert_eq!(
            batch_sealed,
            "03895ecdebef73710637d800db3c11ded7e490194a036b25b7f5601e1c5e7500"
        );
        let ts_columns = ["ID", "TS", "TSTZ"]
            .iter()
            .map(|name| CopyColumnDecl {
                name: (*name).to_string(),
                logical_name: mapping_ident(name).unwrap(),
                arrow_type: if *name == "ID" { "int16" } else { "utf8" }.to_string(),
                nullable: *name != "ID",
                precision: if *name == "ID" { Some(16) } else { None },
                scale: None,
                char_length: None,
                document: false,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            seal_schema_digest("oo04", "t_ts", &ts_columns).unwrap(),
            "17302acc7bc81d4bfa7d3286572a62fe0f8364c155ea2e5de1d4dc8d5dbaec0e"
        );
        for pasted in [
            "59a1d30cfedb66d93fa8277124851ed6fcb3504c697688ddcf92dcadfcc289dc",
            "71eba817dac14915743a455dc02d55305d1cde2294a0391541bafc1fd347978c",
            "89ab99d0ae23b23f05435bbbd3818f736c8fb6ff30ab0ff16dbc3da59b8a4a04",
        ] {
            assert_ne!(sealed, pasted);
        }
        let schema_message = native_arrow::arrow_schema_sha256(&arrow).expect("schema message");
        assert_ne!(sealed, schema_message);
        let file_hash = native_arrow::arrow_sha256(&arrow);
        let catalog_bytes = fs::read(&catalog_path).unwrap();
        let catalog_sha = hex_bytes(&Sha256::digest(&catalog_bytes));
        let mut request = base_request();
        request["source_engine"] = json!("oracle");
        request["source_id"] = json!("oo04_t_wide_text");
        request["source_generation"] = json!(7);
        request["parent_source_generation"] = json!(6);
        request["logical_schema"] = json!("oo04");
        request["logical_table"] = json!("t_wide");
        request["capture_method"] = json!("c5_diff");
        request["delete_detection"] = json!("reconcile");
        request["consistency_grade"] = json!("S0");
        request["key_column"] = json!("ID");
        request["columns"] = json!(columns);
        request["arrow_path"] = json!(arrow_path.to_string_lossy().to_string());
        request["schema_digest"] = json!(sealed);
        request["input_digest"] = json!(file_hash);
        request["snapshot"] = json!({"kind": "token", "value": "oo-dl01w1-04-scn-6463953"});
        request["admission_path"] = json!(catalog_path.to_string_lossy().to_string());
        request["admission_expect"] = json!({"declared_receipt_sha256": catalog_sha});
        let mut physical = request.clone();
        physical.as_object_mut().unwrap().remove("admission_path");
        physical.as_object_mut().unwrap().remove("admission_expect");
        physical["source_generation"] = json!(1);
        physical["parent_source_generation"] = Value::Null;
        physical["applied_pos"] = json!("0");
        let plan = classify(&physical).expect("unbound physical");
        assert!(!plan.source_bound);
        assert_eq!(classify(&request).err(), Some("nonadmitted"));
        assert!(trusted_capture(&request, &arrow).is_err());
        assert_eq!(plan.shape, "unique");
        assert_eq!(plan.columns[0].arrow_type, "int16");
        let keys = key_rows(&decoded, &plan.key_indexes).expect("keys");
        assert_eq!(keys[0], "71");
        assert!(plan.derived_semantics.is_none());
        assert_eq!(plan.column_sources[0], "ID");
        assert_eq!(plan.columns[0].logical_name, "q_4944");
        let prepare_columns =
            columns_for_prepare(&plan.columns, &plan.column_sources).expect("sources");
        let input = decode_hex32(&plan.input_digest).expect("input");
        let prepared = prepare_table(
            &arrow,
            &plan.source_id,
            &plan.schema,
            &plan.table,
            u64::try_from(plan.source_generation).unwrap(),
            &plan.config_digest,
            input,
            &plan.batch_id,
            &prepare_columns,
            None,
        )
        .expect("prepare once");
        let doubled = prepare_table(
            &arrow,
            &plan.source_id,
            &plan.schema,
            &plan.table,
            u64::try_from(plan.source_generation).unwrap(),
            &plan.config_digest,
            input,
            &plan.batch_id,
            &plan.columns,
            None,
        )
        .expect("prepare twice");
        assert_ne!(
            prepared.verified_columns[0].physical_name,
            doubled.verified_columns[0].physical_name
        );
        for (verified, ident) in prepared.verified_columns.iter().zip(&plan.column_idents) {
            let planned = crate::physical_mapping::plan_physical_mapping(
                &crate::physical_mapping::MappingRequest {
                    source: plan.source_id.clone(),
                    config_digest: plan.config_digest.clone(),
                    run_generation: plan.source_generation,
                    level: crate::physical_mapping::Level::Column,
                    logical_namespace: plan.schema.clone(),
                    logical_table: plan.table.clone(),
                    logical_column: ident.clone(),
                },
            )
            .expect("map");
            assert_eq!(verified.physical_name, planned.physical_name);
        }
        let mut crossed = request.clone();
        crossed["source_id"] = json!("oo04_missing");
        assert_eq!(classify(&crossed).err(), Some("cross_source"));

        let mut stale = request.clone();
        stale["source_id"] = json!("oo04_t_batch");
        stale["source_generation"] = json!(7);
        stale["parent_source_generation"] = json!(6);
        assert_eq!(classify(&stale).err(), Some("stale_run"));

        let mut echoed = request.clone();
        echoed["admission_expect"]["declared_receipt_sha256"] = json!(sealed);
        assert_eq!(classify(&echoed).err(), Some("digest_mismatch"));
    }

    #[test]
    fn narrowed_decimal256_is_precision() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/native/clickhouse-attempt06.arrow");
        let bytes = fs::read(&path).expect("fixture");
        let decoded = decode_arrow_stream(&bytes).expect("decode");
        let amount = decoded
            .fields
            .iter()
            .find(|field| field.name == "amount")
            .expect("amount");
        assert_eq!(amount.precision, Some(76));
        assert_eq!(amount.scale, Some(10));
        let mut columns = Vec::new();
        for field in &decoded.fields {
            columns.push(CopyColumnDecl {
                name: field.name.clone(),
                logical_name: field.name.clone(),
                arrow_type: field.arrow_type.clone(),
                nullable: field.nullable,
                precision: if field.name == "amount" {
                    Some(38)
                } else {
                    field.precision
                },
                scale: field.scale,
                char_length: None,
                document: false,
            });
        }
        let err =
            native_arrow::copy_digest(&bytes, "clickhouse_268", "meter", "readings", 1, &columns);
        assert!(matches!(err, Err(ArrowFault::Precision)));
    }

    #[test]
    fn probe_fixed_product_arrows() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evidence");
        let mut files = Vec::new();
        let ch = root.join("4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/product-run-14-c4/arrows");
        let doris =
            root.join("5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product/capture-live");
        if let Ok(entries) = fs::read_dir(&ch) {
            for entry in entries.flatten() {
                if entry.path().extension().and_then(|ext| ext.to_str()) == Some("arrow") {
                    files.push(entry.path());
                }
            }
        }
        if let Ok(entries) = fs::read_dir(&doris) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("b2-") && name.ends_with(".arrow") {
                    files.push(entry.path());
                }
            }
        }
        files.sort();
        let mut rows = Vec::new();
        for path in &files {
            let bytes = fs::read(path).unwrap_or_default();
            let rel = path.to_string_lossy().to_string();
            match decode_arrow_stream(&bytes) {
                Ok(decoded) => {
                    let fields: Vec<Value> = decoded
                        .fields
                        .iter()
                        .map(|field| {
                            json!({
                                "name": field.name,
                                "arrow_type": field.arrow_type,
                                "nullable": field.nullable,
                                "precision": field.precision,
                                "scale": field.scale
                            })
                        })
                        .collect();
                    let key = decoded
                        .fields
                        .first()
                        .map(|field| field.arrow_type.as_str())
                        .unwrap_or("");
                    rows.push(json!({
                        "path": rel,
                        "bytes": bytes.len(),
                        "sha256": native_arrow::arrow_sha256(&bytes),
                        "rows": decoded.rows,
                        "key_arrow_type": key,
                        "int64_key": key == "int64",
                        "fields": fields,
                        "fault": null
                    }));
                }
                Err(fault) => rows.push(json!({
                    "path": rel,
                    "bytes": bytes.len(),
                    "sha256": native_arrow::arrow_sha256(&bytes),
                    "fault": arrow_token(fault)
                })),
            }
        }
        let out = std::env::temp_dir().join("oo-u06-arrow-probe.json");
        fs::write(&out, serde_json::to_vec_pretty(&rows).unwrap()).unwrap();
        if ch.is_dir() {
            assert!(!rows.is_empty());
        }
    }

    fn with_parent(request: &mut Value) {
        request["source_generation"] = json!(1);
        request["parent_source_generation"] = json!(0);
        request["applied_pos"] = json!("1");
    }

    #[test]
    fn date32_epoch_and_uint64_date_utf8_keys_classify() {
        assert_eq!(date32_iso(0).as_deref(), Some("1970-01-01"));
        assert_eq!(date32_iso(1).as_deref(), Some("1970-01-02"));
        assert_eq!(date32_iso(-1).as_deref(), Some("1969-12-31"));
        let days = days_from_civil(2026, 10, 12).expect("days");
        assert_eq!(date32_iso(days).as_deref(), Some("2026-10-12"));

        let mut request = base_request();
        with_parent(&mut request);
        request["columns"][0]["arrow_type"] = json!("uint64");
        request["columns"][0]["precision"] = json!(64);
        let plan = classify(&request).expect("uint64");
        assert_eq!(plan.shape, "unique");
        assert_eq!(plan.key_indexes, vec![0]);
        request["tombstones"] = json!(["18446744073709551615"]);
        assert!(classify(&request).is_ok());
        request["tombstones"] = json!(["01"]);
        assert_eq!(classify(&request).err(), Some("bad_type"));

        let mut request = base_request();
        with_parent(&mut request);
        request["columns"][0]["arrow_type"] = json!("date32");
        request["tombstones"] = json!(["1970-01-01"]);
        assert!(classify(&request).is_ok());
        request["tombstones"] = json!(["1970-1-1"]);
        assert_eq!(classify(&request).err(), Some("bad_type"));

        let mut request = base_request();
        with_parent(&mut request);
        request["key_columns"] = json!(["reading_id", "note"]);
        request.as_object_mut().unwrap().remove("key_column");
        let plan = classify(&request).expect("composite");
        assert_eq!(plan.key_indexes, vec![0, 2]);
        assert_eq!(
            plan.key_idents,
            vec!["reading_id".to_string(), "note".to_string()]
        );
        request["tombstones"] = json!(["3:101\u{001f}1:a"]);
        assert!(classify(&request).is_ok());
        request["tombstones"] = json!(["101"]);
        assert_eq!(classify(&request).err(), Some("bad_type"));
    }

    #[test]
    fn duplicate_rows_snapshot_keeps_multiplicity_and_unique_conflicts() {
        let columns = vec![
            FixtureColumn {
                name: "reading_id".to_string(),
                nullable: false,
                kind: FixtureKind::Int64,
            },
            FixtureColumn {
                name: "amount".to_string(),
                nullable: false,
                kind: FixtureKind::Int64,
            },
            FixtureColumn {
                name: "note".to_string(),
                nullable: false,
                kind: FixtureKind::Utf8,
            },
        ];
        let rows = vec![
            vec![
                FixtureCell::Int(7),
                FixtureCell::Int(1),
                FixtureCell::Text("a".to_string()),
            ],
            vec![
                FixtureCell::Int(7),
                FixtureCell::Int(2),
                FixtureCell::Text("b".to_string()),
            ],
        ];
        let bytes = encode_fixture_arrow(&columns, &rows).expect("encode");
        let dir = std::env::temp_dir().join("oo-native-update-dup");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("dup.arrow");
        fs::write(&path, bytes).unwrap();
        let mut request = base_request();
        request["arrow_path"] = json!(path.to_string_lossy().to_string());
        assert_eq!(classify(&request).err(), Some("conflict"));
        request["row_identity"] = json!("duplicate");
        let plan = classify(&request).expect("snapshot");
        assert_eq!(plan.shape, "snapshot");
        assert_eq!(plan.mode, "full");
        request["tombstones"] = json!(["7"]);
        assert_eq!(classify(&request).err(), Some("conflict"));
        request["row_identity"] = json!("duplicate");
        request["delete_detection"] = json!("partition_replace");
        request["tombstones"] = json!([]);
        assert_eq!(classify(&request).err(), Some("partition_scope_required"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn canonical_extended_json_passes_and_a_bare_double_does_not() {
        let columns = vec![
            FixtureColumn {
                name: "reading_id".to_string(),
                nullable: false,
                kind: FixtureKind::Int64,
            },
            FixtureColumn {
                name: "doc".to_string(),
                nullable: false,
                kind: FixtureKind::Utf8,
            },
        ];
        let canonical = encode_fixture_arrow(
            &columns,
            &[vec![
                FixtureCell::Int(1),
                FixtureCell::Text(r#"{"weight":{"$numberDouble":"41.25"}}"#.to_string()),
            ]],
        )
        .expect("canonical");
        let bare = encode_fixture_arrow(
            &columns,
            &[vec![
                FixtureCell::Int(1),
                FixtureCell::Text(r#"{"weight":41.25}"#.to_string()),
            ]],
        )
        .expect("bare");
        let dir = std::env::temp_dir().join("oo-native-update-double");
        let _ = fs::create_dir_all(&dir);
        let canonical_path = dir.join("canonical.arrow");
        let bare_path = dir.join("bare.arrow");
        fs::write(&canonical_path, canonical).unwrap();
        fs::write(&bare_path, bare).unwrap();
        let mut request = base_request();
        request["columns"] = json!([
            {"name": "reading_id", "logical_name": "reading_id", "arrow_type": "int64", "nullable": false},
            {"name": "doc", "logical_name": "doc", "arrow_type": "utf8", "nullable": false, "document": true}
        ]);
        request["arrow_path"] = json!(canonical_path.to_string_lossy().to_string());
        let plan = classify(&request).expect("canonical");
        assert!(!plan.source_bound);
        assert!(plan.derived_semantics.is_none());
        request["arrow_path"] = json!(bare_path.to_string_lossy().to_string());
        assert_eq!(classify(&request).err(), Some("jsonb_double_unencoded"));
        request.as_object_mut().unwrap().remove("arrow_path");
        with_parent(&mut request);
        request["columns"] = json!([
            {"name": "reading_id", "logical_name": "reading_id", "arrow_type": "int64", "nullable": false},
            {"name": "weight", "logical_name": "weight", "arrow_type": "float64", "nullable": false}
        ]);
        assert!(classify(&request).is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn starrocks_window_comes_from_the_request_and_clickhouse_does_not() {
        let mut clickhouse = base_request();
        with_parent(&mut clickhouse);
        assert!(classify(&clickhouse).is_ok());
        clickhouse["capture_method"] = json!("c4_partition");
        assert_eq!(classify(&clickhouse).err(), Some("partition_scope_required"));
        let mut request = base_request();
        request["source_engine"] = json!("starrocks");
        assert_eq!(classify(&request).err(), Some("upsert_window_unsupported"));
        request["window_closed"] = json!(true);
        request["upsert_supported"] = json!(false);
        request["full_snapshot"] = json!(false);
        assert_eq!(classify(&request).err(), Some("upsert_window_unsupported"));
        with_parent(&mut request);
        request["full_snapshot"] = json!(true);
        let plan = classify(&request).expect("snapshot");
        assert_eq!(plan.shape, "snapshot");
        request["full_snapshot"] = json!(false);
        request["upsert_supported"] = json!(true);
        let plan = classify(&request).expect("upsert");
        assert_eq!(plan.shape, "unique");
    }

    fn capture_request(path: &Path, key: &str, identity: &str, delete_detection: &str) -> Value {
        let bytes = fs::read(path).expect("arrow");
        let decoded = decode_arrow_stream(&bytes).expect("decode");
        let mut columns = Vec::new();
        for field in &decoded.fields {
            let mut column = json!({
                "name": field.name,
                "logical_name": field.name,
                "arrow_type": field.arrow_type,
                "nullable": field.nullable
            });
            if let Some(precision) = field.precision {
                column["precision"] = json!(precision);
            }
            if let Some(scale) = field.scale {
                column["scale"] = json!(scale);
            }
            columns.push(column);
        }
        let mut request = base_request();
        with_parent(&mut request);
        request["source_engine"] = json!("doris");
        request["source_id"] = json!("dorispreview");
        request["logical_schema"] = json!("live");
        request["logical_table"] = json!("captured");
        request["key_column"] = json!(key);
        request["row_identity"] = json!(identity);
        request["delete_detection"] = json!(delete_detection);
        request["columns"] = Value::Array(columns);
        request["arrow_path"] = json!(path.to_string_lossy().to_string());
        request
    }

    #[test]
    fn doris_b2_and_clickhouse_c4_classify_from_the_capture_key_model() {
        let doris = Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product/capture-live",
        );
        let tick: Value =
            serde_json::from_str(&fs::read_to_string(doris.join("b2.json")).expect("b2"))
                .expect("json");
        let tables = tick["tables"].as_array().expect("tables");
        let model = |name: &str| -> String {
            tables
                .iter()
                .find(|table| table["name"] == name)
                .and_then(|table| table["key_model"].as_str())
                .unwrap()
                .to_string()
        };
        let delete = |name: &str| -> String {
            tables
                .iter()
                .find(|table| table["name"] == name)
                .and_then(|table| table["delete_mode"].as_str())
                .unwrap()
                .to_string()
        };
        assert_eq!(model("dup_events"), "DUPLICATE");
        assert_eq!(model("nz_obs"), "DUPLICATE");
        assert_eq!(model("dec_exact"), "DUPLICATE");
        assert_eq!(model("str_key"), "DUPLICATE");
        assert_eq!(model("agg_qty"), "AGGREGATE");
        assert_eq!(model("uniq_acct"), "UNIQUE");
        assert_eq!(model("part_day"), "DUPLICATE");
        assert_eq!(delete("part_day"), "partition_replace");
        let dup = capture_request(
            &doris.join("b2-dup_events.arrow"),
            "event_id",
            "unique",
            "reconcile",
        );
        assert_eq!(classify(&dup).err(), Some("conflict"));
        let dup = capture_request(
            &doris.join("b2-dup_events.arrow"),
            "event_id",
            "duplicate",
            "reconcile",
        );
        assert_eq!(classify(&dup).expect("dup").shape, "snapshot");
        let nz = capture_request(
            &doris.join("b2-nz_obs.arrow"),
            "id",
            "duplicate",
            "reconcile",
        );
        assert_eq!(classify(&nz).expect("nz").shape, "snapshot");
        let exact = capture_request(
            &doris.join("b2-dec_exact.arrow"),
            "id",
            "duplicate",
            "reconcile",
        );
        assert_eq!(classify(&exact).expect("dec").shape, "snapshot");
        let text = capture_request(
            &doris.join("b2-str_key.arrow"),
            "k",
            "duplicate",
            "reconcile",
        );
        assert_eq!(classify(&text).expect("str").shape, "snapshot");
        let agg = capture_request(
            &doris.join("b2-agg_qty.arrow"),
            "sku",
            "aggregate",
            "reconcile",
        );
        assert_eq!(classify(&agg).expect("agg").shape, "snapshot");
        let uniq = capture_request(
            &doris.join("b2-uniq_acct.arrow"),
            "id",
            "unique",
            "reconcile",
        );
        assert_eq!(classify(&uniq).expect("uniq").shape, "unique");
        let part = capture_request(
            &doris.join("b2-part_day.arrow"),
            "day",
            "duplicate",
            "partition_replace",
        );
        assert_eq!(classify(&part).err(), Some("partition_scope_required"));
        let ch = Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/product-run-14-c4/arrows/oo-dl01w1-40-c4b-a18-all.arrow",
        );
        if ch.is_file() {
            let mut request = capture_request(&ch, "reading_id", "unique", "reconcile");
            request["source_engine"] = json!("clickhouse");
            request["source_id"] = json!("clickhouse_268");
            let plan = classify(&request).expect("clickhouse");
            assert_eq!(plan.shape, "unique");
            assert_eq!(plan.columns[0].arrow_type, "uint64");
            assert_eq!(plan.columns[2].arrow_type, "decimal256");
            assert_eq!(plan.columns[2].precision, Some(76));
            assert_eq!(plan.columns[2].scale, Some(10));
        }
    }

    #[test]
    fn oracle_wide_derives_unconstrained_number_without_forging_a_grade() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/live-chain",
        );
        let catalog_path = root.join("catalog-wide/catalog.json");
        let audit_path = root.join("identity-67/wide/audit/readonly-audit.json");
        if !catalog_path.is_file() || !audit_path.is_file() {
            return;
        }
        let catalog: Value =
            serde_json::from_str(&fs::read_to_string(catalog_path).unwrap()).unwrap();
        let audit: Value = serde_json::from_str(&fs::read_to_string(audit_path).unwrap()).unwrap();
        assert!(audit.get("consistency_grade").is_none());
        assert!(audit.get("delete_detection").is_none());
        let bound = json!({
            "protocol": oo_core::source_bound::SOURCE_BOUND_PROTOCOL,
            "engine": catalog["engine"],
            "source_id": catalog["source_id"],
            "source_generation": catalog["source_generation"],
            "target_generation": catalog["target_generation"],
            "run_id": catalog["run_id"],
            "schema_digest": catalog["schema_digest"],
            "type_digest": catalog["type_digest"],
            "config_digest": catalog["config_digest"],
            "input_digest": catalog["input_digest"],
            "banner": audit["banner"],
            "scn": audit["scn"],
            "registry_snapshot": audit["registry_snapshot"],
            "published_scn": audit["published_scn"],
            "dictionary": {
                "protocol": oo_core::source_bound::TYPE_DICTIONARY_PROTOCOL,
                "columns": audit["dictionary_columns"]
            },
            "bindings": [
                {"sealed_column": "id", "dictionary_column": "ID"},
                {"sealed_column": "n_free_text", "dictionary_column": "N_FREE"}
            ]
        });
        let mut request = base_request();
        request["source_engine"] = json!("oracle");
        request["source_id"] = catalog["source_id"].clone();
        request["source_generation"] = catalog["source_generation"].clone();
        request["parent_source_generation"] =
            json!(catalog["source_generation"].as_i64().unwrap() - 1);
        request["applied_pos"] = json!("1");
        request["sealed_catalog"] = catalog;
        request["source_bound"] = bound;
        let plan = classify(&request).expect("derive");
        assert!(!plan.source_bound);
        let derived = plan.derived_semantics.expect("semantics");
        assert_eq!(derived["consistency_grade"], Value::Null);
        assert_eq!(derived["delete_detection"], Value::Null);
        let notes: Vec<&str> = derived["columns"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|column| column["native_note"].as_str())
            .collect();
        assert_eq!(notes, vec!["unconstrained_number"]);
        assert!(derived["columns"].as_array().unwrap().iter().all(|column| {
            column["caveats"]
                .as_array()
                .map(|items| items.is_empty())
                .unwrap_or(false)
                || column["column"] != "n_free_text"
        }));
        let n_free = derived["columns"]
            .as_array()
            .unwrap()
            .iter()
            .find(|column| column["column"] == "n_free_text")
            .unwrap();
        assert!(n_free["caveats"].as_array().unwrap().is_empty());
    }

    #[test]
    fn fixture_and_product_arrows_decode_for_the_loader() {
        let columns = vec![
            FixtureColumn {
                name: "reading_id".to_string(),
                nullable: false,
                kind: FixtureKind::Int64,
            },
            FixtureColumn {
                name: "amount".to_string(),
                nullable: false,
                kind: FixtureKind::Int64,
            },
            FixtureColumn {
                name: "note".to_string(),
                nullable: false,
                kind: FixtureKind::Utf8,
            },
        ];
        let gen1 = encode_fixture_arrow(
            &columns,
            &[
                vec![
                    FixtureCell::Int(101),
                    FixtureCell::Int(10),
                    FixtureCell::Text("a".to_string()),
                ],
                vec![
                    FixtureCell::Int(102),
                    FixtureCell::Int(20),
                    FixtureCell::Text("b".to_string()),
                ],
                vec![
                    FixtureCell::Int(103),
                    FixtureCell::Int(30),
                    FixtureCell::Text("c".to_string()),
                ],
            ],
        )
        .expect("gen1");
        let gen2 = encode_fixture_arrow(
            &columns,
            &[
                vec![
                    FixtureCell::Int(101),
                    FixtureCell::Int(11),
                    FixtureCell::Text("a2".to_string()),
                ],
                vec![
                    FixtureCell::Int(104),
                    FixtureCell::Int(40),
                    FixtureCell::Text("d".to_string()),
                ],
            ],
        )
        .expect("gen2");
        let conflict = encode_fixture_arrow(
            &columns,
            &[
                vec![
                    FixtureCell::Int(101),
                    FixtureCell::Int(11),
                    FixtureCell::Text("ZZ".to_string()),
                ],
                vec![
                    FixtureCell::Int(104),
                    FixtureCell::Int(40),
                    FixtureCell::Text("d".to_string()),
                ],
            ],
        )
        .expect("conflict");
        let dup = encode_fixture_arrow(
            &columns,
            &[
                vec![
                    FixtureCell::Int(7),
                    FixtureCell::Int(1),
                    FixtureCell::Text("a".to_string()),
                ],
                vec![
                    FixtureCell::Int(7),
                    FixtureCell::Int(2),
                    FixtureCell::Text("b".to_string()),
                ],
            ],
        )
        .expect("dup");
        let doc_columns = vec![
            FixtureColumn {
                name: "reading_id".to_string(),
                nullable: false,
                kind: FixtureKind::Int64,
            },
            FixtureColumn {
                name: "doc".to_string(),
                nullable: false,
                kind: FixtureKind::Utf8,
            },
        ];
        let canonical = encode_fixture_arrow(
            &doc_columns,
            &[vec![
                FixtureCell::Int(1),
                FixtureCell::Text(r#"{"weight":{"$numberDouble":"41.25"}}"#.to_string()),
            ]],
        )
        .expect("canonical");
        let bare = encode_fixture_arrow(
            &doc_columns,
            &[vec![
                FixtureCell::Int(1),
                FixtureCell::Text(r#"{"weight":41.25}"#.to_string()),
            ]],
        )
        .expect("bare");
        assert_eq!(decode_arrow_stream(&gen1).expect("decode gen1").rows, 3);
        assert_eq!(decode_arrow_stream(&canonical).expect("decode doc").rows, 1);
        let Ok(dir) = std::env::var("OO_U06_PACK_DUMP") else {
            return;
        };
        let dump = std::path::PathBuf::from(dir);
        fs::create_dir_all(&dump).unwrap();
        fs::write(dump.join("gen1.arrow"), &gen1).unwrap();
        fs::write(dump.join("gen2.arrow"), &gen2).unwrap();
        fs::write(dump.join("gen2-conflict.arrow"), &conflict).unwrap();
        fs::write(dump.join("dup.arrow"), &dup).unwrap();
        fs::write(dump.join("canonical.arrow"), &canonical).unwrap();
        fs::write(dump.join("bare.arrow"), &bare).unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evidence");
        let mut files = vec![
            root.join("4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/product-run-14-c4/arrows/oo-dl01w1-40-c4b-a18-all.arrow"),
        ];
        let doris =
            root.join("5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product/capture-live");
        if let Ok(entries) = fs::read_dir(&doris) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if (name.starts_with("a1-") || name.starts_with("b2-")) && name.ends_with(".arrow")
                {
                    files.push(entry.path());
                }
            }
        }
        let mut index = Vec::new();
        for path in files {
            if !path.is_file() {
                index.push(json!({"path": path.display().to_string(), "fault": "missing"}));
                continue;
            }
            let bytes = fs::read(&path).unwrap_or_default();
            if bytes.len() > READ_BUDGET {
                index.push(json!({"path": path.display().to_string(), "bytes": bytes.len(), "fault": "read_budget"}));
                continue;
            }
            match decode_arrow_stream(&bytes) {
                Ok(decoded) => {
                    let cells = arrow_cell_text(&decoded);
                    let out_name =
                        path.file_name().unwrap().to_string_lossy().to_string() + ".cells.json";
                    fs::write(
                        dump.join(&out_name),
                        serde_json::to_vec_pretty(&cells).unwrap(),
                    )
                    .unwrap();
                    index.push(json!({
                        "path": path.display().to_string(),
                        "bytes": bytes.len(),
                        "rows": decoded.rows,
                        "cells": out_name
                    }));
                }
                Err(fault) => index.push(json!({
                    "path": path.display().to_string(),
                    "bytes": bytes.len(),
                    "fault": arrow_token(fault)
                })),
            }
        }
        fs::write(
            dump.join("arrow-index.json"),
            serde_json::to_vec_pretty(&index).unwrap(),
        )
        .unwrap();
        assert!(!index.is_empty());
    }

    fn arrow_cell_text(decoded: &DecodedStream) -> Value {
        let mut rows = Vec::new();
        for row_index in 0..decoded.rows {
            let mut row = Vec::new();
            for column in &decoded.columns {
                row.push(native_cell_text(&column[row_index]));
            }
            rows.push(row);
        }
        let fields: Vec<Value> = decoded
            .fields
            .iter()
            .map(|field| {
                json!({
                    "name": field.name,
                    "arrow_type": field.arrow_type,
                    "nullable": field.nullable,
                    "precision": field.precision,
                    "scale": field.scale
                })
            })
            .collect();
        json!({"fields": fields, "rows": rows})
    }

    fn native_cell_text(cell: &NativeCell) -> Value {
        match cell {
            NativeCell::Null => Value::Null,
            NativeCell::Int { value, .. } => json!(value.to_string()),
            NativeCell::UInt64(value) => json!(value.to_string()),
            NativeCell::UInt8(value) => json!(value.to_string()),
            NativeCell::Bool(value) => json!(if *value { "true" } else { "false" }),
            NativeCell::Utf8(value) | NativeCell::Json(value) => json!(value),
            NativeCell::Date32(days) => json!(date32_iso(*days)),
            NativeCell::Binary(bytes) => json!(format!("\\x{}", hex_bytes(bytes))),
            NativeCell::Decimal {
                scale, coefficient, ..
            } => json!(scaled_i128_text(*coefficient, *scale)),
            NativeCell::Decimal256 {
                scale, coefficient, ..
            } => json!(decimal256_text(coefficient, *scale)),
            NativeCell::TimestampNanos { value, .. } | NativeCell::TimestampMicros(value) => {
                json!(value.to_string())
            }
            NativeCell::TimestampMicrosUtc { value, .. } => json!(value.to_string()),
            NativeCell::Float64(bits) => json!(f64::from_bits(*bits).to_string()),
        }
    }

    fn scaled_i128_text(coefficient: i128, scale: i8) -> String {
        let negative = coefficient < 0;
        let mut digits = coefficient.unsigned_abs().to_string().into_bytes();
        let text = place_scale(&mut digits, scale);
        if negative { format!("-{text}") } else { text }
    }

    fn decimal256_text(coefficient: &[u8; 32], scale: i8) -> String {
        let negative = coefficient[31] & 0x80 != 0;
        let mut magnitude = *coefficient;
        if negative {
            let mut carry = 1u16;
            for byte in magnitude.iter_mut() {
                let wide = u16::from(!*byte) + carry;
                *byte = (wide & 0xff) as u8;
                carry = wide >> 8;
            }
        }
        let mut digits = Vec::new();
        if magnitude.iter().all(|byte| *byte == 0) {
            digits.push(b'0');
        } else {
            while magnitude.iter().any(|byte| *byte != 0) {
                let mut remainder = 0u16;
                for byte in magnitude.iter_mut().rev() {
                    let current = (remainder << 8) | u16::from(*byte);
                    *byte = u8::try_from(current / 10).unwrap_or(0);
                    remainder = current % 10;
                }
                digits.push(b'0' + u8::try_from(remainder).unwrap_or(0));
            }
            digits.reverse();
        }
        let text = place_scale(&mut digits, scale);
        if negative { format!("-{text}") } else { text }
    }

    fn place_scale(digits: &mut Vec<u8>, scale: i8) -> String {
        let scale = usize::try_from(scale).unwrap_or(0);
        if scale == 0 {
            return String::from_utf8(digits.clone()).unwrap_or_default();
        }
        if digits.len() <= scale {
            let mut padded = vec![b'0'; scale - digits.len() + 1];
            padded.extend_from_slice(digits);
            *digits = padded;
        }
        let split = digits.len() - scale;
        let mut out = String::from_utf8(digits[..split].to_vec()).unwrap_or_default();
        out.push('.');
        out.push_str(std::str::from_utf8(&digits[split..]).unwrap_or(""));
        out
    }

}
