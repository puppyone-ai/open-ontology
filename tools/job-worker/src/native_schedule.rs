//! Native producer scheduling with a source-and-target generation gate.
use crate::{
    api::LeaseStatus,
    config::Config,
    error::{Error, Result},
    lease, session,
    sha256::sha256_hex,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command,
};
const KIND: &str = "native_capture";
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Executable {
    pub path: String,
    pub sha256: String,
    pub args: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registration {
    pub admission_binary: String,
    pub admission_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transaction_registry_vector_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transaction_registry_vector_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_target_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_target_sha256: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub protocol: String,
    pub binding: String,
    pub project_id: String,
    pub source_id: String,
    pub method: String,
    pub cycle: String,
    pub period_ms: i64,
    pub producer: Executable,
    pub loader: Executable,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registration: Option<Registration>,
}
fn executable_check(e: &Executable) -> Result<()> {
    if !Path::new(&e.path).is_absolute()
        || e.path.contains("..")
        || e.sha256.len() != 64
        || !e.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        || e.args
            .iter()
            .any(|a| a.contains('\0') || a.contains("secret://") || a.contains("password="))
    {
        return Err(Error::new("executable"));
    }
    if sha256_hex(&std::fs::read(&e.path).map_err(|_| Error::new("executable"))?) != e.sha256 {
        return Err(Error::new("hash"));
    }
    Ok(())
}
// Template identity comes only from the immutable occurrence, never producer output.
fn vector_path(path: &str, sha: Option<&str>, group: bool) -> Result<()> {
    if !Path::new(path).is_absolute() || path.contains("..") || path.contains('\0') {
        return Err(Error::new("registration"));
    }
    let template = path.contains('{') || path.contains('}');
    if template {
        let residual = path.replace("{binding}", "").replace("{pin}", "");
        if !group
            || sha.is_some()
            || !path.contains("{binding}")
            || !path.contains("{pin}")
            || residual.contains(['{', '}'])
        {
            return Err(Error::new("registration"));
        }
    } else if !sha.is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())) {
        return Err(Error::new("registration"));
    }
    Ok(())
}
fn vector_bytes_sha(bytes: &[u8], pinned: Option<&str>, prior: Option<&[u8]>) -> Result<String> {
    let sha = sha256_hex(bytes);
    if bytes.len() > 1024 * 1024 || pinned.is_some_and(|pinned| pinned != sha) {
        return Err(Error::new("registration"));
    }
    if prior.is_some_and(|prior| prior != bytes) {
        return Err(Error::new("registration_changed"));
    }
    Ok(sha)
}
fn resolve_vector_path(path: &str, binding: &str, pin: i64) -> Result<String> {
    if !crate::api::is_uuid(binding) || pin < 1 {
        return Err(Error::new("registration"));
    }
    Ok(path
        .replace("{binding}", binding)
        .replace("{pin}", &pin.to_string()))
}
impl Manifest {
    fn shape(&self) -> Result<()> {
        if self.protocol != "oo.jobs.native-schedule/v1"
            || !crate::api::is_uuid(&self.binding)
            || !crate::api::is_uuid(&self.project_id)
            || self.source_id.is_empty()
            || !["C1", "C2", "C3", "C4", "C5"].contains(&self.method.as_str())
            || !["detect", "pull", "reconcile"].contains(&self.cycle.as_str())
            || !(1000..=86400000).contains(&self.period_ms)
            || self.loader.args != ["import-native-update"]
                && self.loader.args != ["import-native-transaction"]
                && self.loader.args != ["import-native-c1-baseline"]
            || self.loader.args == ["import-native-transaction"] && self.method != "C1"
            || self.loader.args == ["import-native-c1-baseline"] && self.method != "C1"
        {
            return Err(Error::new("manifest"));
        }
        for e in [&self.producer, &self.loader] {
            if !Path::new(&e.path).is_absolute()
                || e.path.contains("..")
                || e.sha256.len() != 64
                || !e.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(Error::new("manifest"));
            }
        }
        if let Some(r) = &self.registration {
            let group = self.loader.args == ["import-native-transaction"];
            let baseline = self.method == "C1"
                && self.loader.args == ["import-native-update"]
                && r.transaction_registry_vector_path.is_some();
            let cdc_baseline = self.loader.args == ["import-native-c1-baseline"];
            if group || baseline {
                let path = r
                    .transaction_registry_vector_path
                    .as_deref()
                    .ok_or_else(|| Error::new("registration"))?;
                vector_path(path, r.transaction_registry_vector_sha256.as_deref(), group)?;
                if r.registry_path.is_some() {
                    return Err(Error::new("registration"));
                }
            } else if cdc_baseline {
                if r.registry_path.is_some()
                    || r.transaction_registry_vector_path.is_some()
                    || r.transaction_registry_vector_sha256.is_some()
                {
                    return Err(Error::new("registration"));
                }
            } else if !r.registry_path.as_ref().is_some_and(|path| {
                Path::new(path).is_absolute() && !path.contains("..") && !path.contains('\0')
            }) || r.transaction_registry_vector_path.is_some()
                || r.transaction_registry_vector_sha256.is_some()
            {
                return Err(Error::new("registration"));
            }
            if r.control_target_path.is_some() != r.control_target_sha256.is_some() {
                return Err(Error::new("registration"));
            }
            if let (Some(path), Some(sha)) = (&r.control_target_path, &r.control_target_sha256)
                && (!group
                    || !Path::new(path).is_absolute()
                    || path.contains("..")
                    || path.contains('\0')
                    || sha.len() != 64
                    || !sha.bytes().all(|b| b.is_ascii_hexdigit()))
            {
                return Err(Error::new("registration"));
            }
        } else if self.loader.args == ["import-native-transaction"]
            || self.loader.args == ["import-native-c1-baseline"]
        {
            return Err(Error::new("registration"));
        }
        Ok(())
    }
    fn check(&self) -> Result<()> {
        self.shape()?;
        executable_check(&self.producer)?;
        executable_check(&self.loader)?;
        if let Some(r) = &self.registration {
            executable_check(&Executable {
                path: r.admission_binary.clone(),
                sha256: r.admission_sha256.clone(),
                args: vec![],
            })?;
            if let (Some(path), Some(sha)) = (
                &r.transaction_registry_vector_path,
                &r.transaction_registry_vector_sha256,
            ) && sha256_hex(&std::fs::read(path).map_err(|_| Error::new("registration"))?)
                != *sha
            {
                return Err(Error::new("registration"));
            }
            if let (Some(path), Some(sha)) = (&r.control_target_path, &r.control_target_sha256)
                && sha256_hex(&std::fs::read(path).map_err(|_| Error::new("registration"))?) != *sha
            {
                return Err(Error::new("registration"));
            }
        }
        Ok(())
    }
}

fn manifest_read(path: &str) -> Result<Manifest> {
    let b = std::fs::read(path).map_err(|_| Error::new("manifest"))?;
    let m: Manifest = serde_json::from_slice(&b).map_err(|_| Error::new("manifest"))?;
    m.check()?;
    Ok(m)
}
fn canonical(m: &Manifest) -> Result<(String, String)> {
    let s = serde_json::to_string(m).map_err(|_| Error::new("manifest"))?;
    let h = sha256_hex(s.as_bytes());
    Ok((s, h))
}
fn target_database(config: &Config, method: &str) -> Result<String> {
    let database = target_database_for(
        &config.database,
        config.target_database.as_deref(),
        config.c1_target_database.as_deref(),
        method,
    )?;
    if database.is_empty() || database.contains('\0') {
        return Err(Error::new("target_database"));
    }
    Ok(database.to_owned())
}
fn target_database_for<'a>(
    control: &'a str,
    c4: Option<&'a str>,
    c1: Option<&'a str>,
    method: &str,
) -> Result<&'a str> {
    match method {
        "C4" => c4.ok_or_else(|| Error::new("target_database")),
        "C1" => Ok(c1.unwrap_or(control)),
        _ => Ok(control),
    }
}
async fn target_owner(config: &Config, database: &str) -> Result<tokio_postgres::Client> {
    if config.target_database.as_deref() == Some(database)
        || config.c1_target_database.as_deref() == Some(database)
    {
        session::target_runtime(config).await
    } else if config.database == database {
        session::admin_db(config, database).await
    } else {
        Err(Error::new("target_database"))
    }
}
async fn confirmed_generation(
    config: &Config,
    control: &tokio_postgres::Client,
    source: &str,
    database: &str,
    task: &str,
    stored: &Option<String>,
) -> Result<i64> {
    let row=control.query_one("SELECT source_generation,task_id::text,active_task_id::text FROM jw_native.source_progress WHERE source_id=$1 AND target_database=$2",&[&source,&database]).await.map_err(Error::sql)?;
    if row.get::<_, Option<String>>(2).as_deref() != Some(task) {
        return Err(Error::new("source_active"));
    }
    let target = target_owner(config, database).await?;
    let applied = target
        .query_opt(
            "SELECT source_generation FROM _oo.source_watermark WHERE source=$1",
            &[&source],
        )
        .await
        .map_err(Error::sql)?
        .map(|r| r.get::<_, i64>(0))
        .unwrap_or(0);
    let mut confirmed: Option<i64> = row.get(0);
    let replay_generation = stored
        .as_ref()
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .and_then(|v| v.get("source_generation").and_then(Value::as_i64));
    if confirmed.is_none() && applied > 0 && replay_generation != Some(applied) {
        let historical=control.query_opt("SELECT w.source_generation,w.task_id::text,w.binding_id::text,b.method,w.result::text FROM jw_native.watermarks w JOIN jw_native.bindings b ON b.id=w.binding_id JOIN jobs.tasks t ON t.id=w.task_id LEFT JOIN jw_native.pending_acks a ON a.task_id=w.task_id WHERE b.source_id=$1 AND b.target_database=$2 AND w.source_generation=$3 AND t.state='succeeded' AND (b.method<>'C4' OR a.acknowledged_at IS NOT NULL) ORDER BY w.advanced_at DESC LIMIT 1",&[&source,&database,&applied]).await.map_err(Error::sql)?.ok_or_else(||Error::new("unconfirmed_baseline"))?;
        let method: String = historical.get(3);
        let result_text: String = historical.get(4);
        if method == "C4" {
            let result: Value = serde_json::from_str(&result_text)
                .map_err(|_| Error::new("unconfirmed_baseline"))?;
            let ack = ack_request(
                &result,
                &historical.get::<_, String>(1),
                result
                    .pointer("/partition_target_receipt/job_generation")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| Error::new("unconfirmed_baseline"))?,
            )?;
            let exact=target.query_opt("SELECT acknowledged FROM _oo.native_partition_state WHERE source=$1 AND schema_name=$2 AND table_name=$3 AND partition_id=$4 AND run_id=$5::text::uuid AND batch_id=$6 AND payload_digest=$7 AND job_task_id=$8::text::uuid AND job_generation=$9",&[&source,&ack["logical_schema"].as_str(),&ack["logical_table"].as_str(),&ack["partition_id"].as_str(),&ack["run_id"].as_str(),&ack["batch_id"].as_str(),&ack["payload_digest"].as_str(),&ack["job_task_id"].as_str(),&ack["job_generation"].as_i64()]).await.map_err(Error::sql)?;
            if !exact.is_some_and(|r| r.get::<_, bool>(0)) {
                return Err(Error::new("unconfirmed_baseline"));
            }
        }
        let historical_task: String = historical.get(1);
        let historical_binding: String = historical.get(2);
        control.execute("UPDATE jw_native.source_progress SET source_generation=$3,task_id=$4::text::uuid,binding_id=$5::text::uuid,confirmed_at=clock_timestamp() WHERE source_id=$1 AND target_database=$2 AND source_generation IS NULL AND active_task_id=$6::text::uuid",&[&source,&database,&applied,&historical_task,&historical_binding,&task]).await.map_err(Error::sql)?;
        confirmed = Some(applied);
    }
    let base = confirmed.unwrap_or(0);
    if let Some(previous_task) = row.get::<_, Option<String>>(1) {
        let prior=control.query_opt("SELECT t.state,b.method,w.result::text,a.acknowledged_at IS NOT NULL FROM jobs.tasks t JOIN jw_native.watermarks w ON w.task_id=t.id JOIN jw_native.bindings b ON b.id=w.binding_id LEFT JOIN jw_native.pending_acks a ON a.task_id=t.id WHERE t.id=$1::text::uuid AND b.source_id=$2 AND b.target_database=$3 AND w.source_generation=$4",&[&previous_task,&source,&database,&base]).await.map_err(Error::sql)?.ok_or_else(||Error::new("unconfirmed_baseline"))?;
        if prior.get::<_, String>(0) != "succeeded" {
            return Err(Error::new("unconfirmed_baseline"));
        }
        if prior.get::<_, String>(1) == "C4" {
            if !prior.get::<_, bool>(3) {
                return Err(Error::new("unconfirmed_baseline"));
            }
            let result: Value = serde_json::from_str(&prior.get::<_, String>(2))
                .map_err(|_| Error::new("unconfirmed_baseline"))?;
            let receipt = result
                .get("partition_target_receipt")
                .ok_or_else(|| Error::new("unconfirmed_baseline"))?;
            let scope = receipt
                .get("partition_scope")
                .ok_or_else(|| Error::new("unconfirmed_baseline"))?;
            let exact=target.query_opt("SELECT acknowledged FROM _oo.native_partition_state WHERE source=$1 AND schema_name=$2 AND table_name=$3 AND partition_id=$4 AND run_id=$5::text::uuid AND batch_id=$6 AND payload_digest=$7 AND job_task_id=$8::text::uuid AND job_generation=$9",&[&source,&scope["logical_schema"].as_str(),&scope["logical_table"].as_str(),&scope["partition_id"].as_str(),&result["run_id"].as_str(),&receipt["batch_id"].as_str(),&receipt["payload_digest"].as_str(),&previous_task,&receipt["job_generation"].as_i64()]).await.map_err(Error::sql)?;
            if !exact.is_some_and(|r| r.get::<_, bool>(0)) {
                return Err(Error::new("unconfirmed_baseline"));
            }
        }
    }
    if applied != base && (applied != replay_generation.unwrap_or(-1) || applied != base + 1) {
        return Err(Error::new("target_ahead"));
    }
    Ok(base)
}
// C1 generations belong to a table binding. The source watermark is a
// cross-table position barrier and can be many generations ahead of a table.
async fn confirmed_c1_generation(
    config: &Config,
    control: &tokio_postgres::Client,
    source: &str,
    database: &str,
    binding: &str,
    task: &str,
    stored: &Option<String>,
) -> Result<i64> {
    let active=control.query_opt("SELECT active_task_id::text FROM jw_native.source_progress WHERE source_id=$1 AND target_database=$2",&[&source,&database]).await.map_err(Error::sql)?;
    if active
        .and_then(|r| r.get::<_, Option<String>>(0))
        .as_deref()
        != Some(task)
    {
        return Err(Error::new("source_active"));
    }
    let prior=control.query_opt("SELECT w.source_generation,w.applied_pos,t.state,o.request::text,w.result::text FROM jw_native.watermarks w JOIN jobs.tasks t ON t.id=w.task_id JOIN jw_native.occurrences o ON o.task_id=w.task_id WHERE w.binding_id=$1::text::uuid",&[&binding]).await.map_err(Error::sql)?;
    let replay: Option<Value> = stored
        .as_ref()
        .map(|s| serde_json::from_str(s).map_err(|_| Error::new("request")))
        .transpose()?;
    let base = prior.as_ref().map(|r| r.get::<_, i64>(0)).unwrap_or(0);
    let lineage = prior
        .as_ref()
        .map(|r| {
            serde_json::from_str::<Value>(&r.get::<_, String>(3))
                .map_err(|_| Error::new("unconfirmed_baseline"))
        })
        .transpose()?;
    if prior
        .as_ref()
        .is_some_and(|r| r.get::<_, String>(2) != "succeeded")
    {
        return Err(Error::new("unconfirmed_baseline"));
    }
    let identity = replay.as_ref().or(lineage.as_ref());
    if let Some(request) = identity {
        let schema = request
            .get("logical_schema")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("unconfirmed_baseline"))?;
        let table = request
            .get("logical_table")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("unconfirmed_baseline"))?;
        if request.get("source_id").and_then(Value::as_str) != Some(source)
            || lineage.as_ref().is_some_and(|p| {
                p.get("logical_schema") != request.get("logical_schema")
                    || p.get("logical_table") != request.get("logical_table")
            })
        {
            return Err(Error::new("unconfirmed_baseline"));
        }
        let target = target_owner(config, database).await?;
        if let Some(prior) = prior.as_ref() {
            let old = lineage
                .as_ref()
                .ok_or_else(|| Error::new("unconfirmed_baseline"))?;
            let result: Value = serde_json::from_str(&prior.get::<_, String>(4))
                .map_err(|_| Error::new("unconfirmed_baseline"))?;
            let run = result
                .get("run_id")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::new("unconfirmed_baseline"))?;
            let physical = result
                .get("physical_name")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::new("unconfirmed_baseline"))?;
            let target_generation = result
                .get("target_generation")
                .and_then(Value::as_i64)
                .ok_or_else(|| Error::new("unconfirmed_baseline"))?;
            let digest = result
                .get("payload_digest")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::new("unconfirmed_baseline"))?;
            let batch = old
                .get("batch_id")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::new("unconfirmed_baseline"))?;
            let previous: String = prior.get(1);
            let sealed:bool=target.query_one("SELECT EXISTS(SELECT 1 FROM _oo.full_import_run r JOIN _oo.full_import_table ft ON ft.run_id=r.run_id JOIN _oo.apply_receipts ar ON ar.source=r.source_id AND ar.table_generation=ft.physical_name WHERE r.run_id=$1::text::uuid AND r.state='awaiting_publish' AND r.source_id=$2 AND r.source_generation=$3 AND r.target_generation=$4 AND ft.schema_ident=$5 AND ft.table_ident=$6 AND ft.physical_name=$7 AND ft.exhausted AND ar.txn_or_batch_id=$8 AND ar.payload_digest=$9 AND ar.applied_pos=$10::text::numeric AND ar.txn_complete)",&[&run,&source,&base,&target_generation,&schema,&table,&physical,&batch,&digest,&previous]).await.map_err(Error::sql)?.get(0);
            if !sealed {
                return Err(Error::new("unconfirmed_baseline"));
            }
        }
        let row=target.query_opt("SELECT source_generation,last_pos::text FROM _oo.table_freshness WHERE source=$1 AND schema_name=$2 AND table_name=$3",&[&source,&schema,&table]).await.map_err(Error::sql)?;
        let next = replay
            .as_ref()
            .and_then(|v| v.get("source_generation"))
            .and_then(Value::as_i64);
        match (prior.as_ref(), row) {
            (Some(prior), Some(row)) => {
                let applied: i64 = row.get(0);
                let pos: Option<String> = row.get(1);
                let previous: String = prior.get(1);
                if !(applied == base && pos.as_deref() == Some(previous.as_str())
                    || next == Some(base + 1) && applied == base + 1)
                {
                    return Err(Error::new("unconfirmed_baseline"));
                }
            }
            (None, None) => {}
            (None, Some(row)) if next == Some(1) && row.get::<_, i64>(0) == 1 => {}
            _ => return Err(Error::new("unconfirmed_baseline")),
        }
    }
    Ok(base)
}
async fn verify_applied(
    config: &Config,
    source: &str,
    database: &str,
    generation: i64,
) -> Result<()> {
    let target = target_owner(config, database).await?;
    let row = target
        .query_opt(
            "SELECT source_generation FROM _oo.source_watermark WHERE source=$1",
            &[&source],
        )
        .await
        .map_err(Error::sql)?;
    if row.map(|r| r.get::<_, i64>(0)) != Some(generation) {
        return Err(Error::new("target_generation"));
    }
    Ok(())
}
async fn verify_c1_applied(
    config: &Config,
    source: &str,
    database: &str,
    request: &Value,
    result: &Value,
) -> Result<()> {
    let target = target_owner(config, database).await?;
    let schema = request
        .get("logical_schema")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("position"))?;
    let table = request
        .get("logical_table")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("position"))?;
    let generation = request
        .get("source_generation")
        .and_then(Value::as_i64)
        .ok_or_else(|| Error::new("position"))?;
    let pos = request
        .get("applied_pos")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("position"))?;
    let kind = c1_codec_closed(request)?;
    let codec = request
        .get("position_codec")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("position"))?;
    let run = result
        .get("run_id")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("target_generation"))?;
    let physical = result
        .get("physical_name")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("target_generation"))?;
    let target_generation = result
        .get("target_generation")
        .and_then(Value::as_i64)
        .ok_or_else(|| Error::new("target_generation"))?;
    let digest = result
        .get("payload_digest")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("target_generation"))?;
    let batch = request
        .get("batch_id")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("target_generation"))?;
    let valid: bool = if kind == "legacy" {
        target.query_one("SELECT EXISTS(SELECT 1 FROM _oo.table_freshness f JOIN _oo.source_watermark w ON w.source=f.source JOIN _oo.full_import_run r ON r.run_id=$7::text::uuid JOIN _oo.full_import_table ft ON ft.run_id=r.run_id JOIN _oo.apply_receipts ar ON ar.source=r.source_id AND ar.table_generation=ft.physical_name WHERE f.source=$1 AND f.schema_name=$2 AND f.table_name=$3 AND f.source_generation=$4 AND f.last_pos=$5::text::numeric AND f.physical_name=$8 AND w.position_codec=$6 AND w.applied_pos>=$5::text::numeric AND r.state='awaiting_publish' AND r.source_id=$1 AND r.source_generation=$4 AND r.target_generation=$9 AND ft.schema_ident=$2 AND ft.table_ident=$3 AND ft.physical_name=$8 AND ft.exhausted AND ar.txn_or_batch_id=$10 AND ar.payload_digest=$11 AND ar.applied_pos=$5::text::numeric AND ar.txn_complete)",&[&source,&schema,&table,&generation,&pos,&codec,&run,&physical,&target_generation,&batch,&digest]).await.map_err(Error::sql)?.get(0)
    } else {
        let lineage = request
            .get("position_lineage")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("position"))?;
        let epoch = i32::try_from(
            request
                .get("source_epoch")
                .and_then(Value::as_i64)
                .ok_or_else(|| Error::new("position"))?,
        )
        .map_err(|_| Error::new("position"))?;
        let grade = request
            .get("consistency_grade")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("position"))?;
        if kind == "gtid" {
            let attestation: Option<String> = target
                .query_one(
                    "SELECT _oo.native_gtid_transaction_read($1,$2,$3,$4,$5::text::numeric)::text",
                    &[&source, &codec, &lineage, &epoch, &pos],
                )
                .await
                .map_err(Error::sql)?
                .get(0);
            if attestation.is_none() {
                return Err(Error::new("target_generation"));
            }
        }
        target.query_one("SELECT EXISTS(SELECT 1 FROM _oo.table_freshness f JOIN _oo.source_watermark w ON w.source=f.source JOIN _oo.source_instance i ON i.source=w.source AND i.source_epoch=w.source_epoch AND i.measured_id=w.position_lineage JOIN _oo.full_import_run r ON r.run_id=$7::text::uuid JOIN _oo.full_import_table ft ON ft.run_id=r.run_id JOIN _oo.apply_receipts ar ON ar.source=r.source_id AND ar.table_generation=ft.physical_name WHERE f.source=$1 AND f.schema_name=$2 AND f.table_name=$3 AND f.source_generation=$4 AND f.last_pos=$5::text::numeric AND f.physical_name=$8 AND w.position_codec=$6 AND w.position_lineage=$12 AND w.source_epoch=$13 AND w.applied_pos=$5::text::numeric AND w.consistency_grade=$14 AND (($15 AND w.cross_table_consistent) OR (NOT $15 AND NOT w.cross_table_consistent)) AND r.state='awaiting_publish' AND r.source_id=$1 AND r.source_generation=$4 AND r.target_generation=$9 AND ft.schema_ident=$2 AND ft.table_ident=$3 AND ft.physical_name=$8 AND ft.exhausted AND ar.txn_or_batch_id=$10 AND ar.payload_digest=$11 AND ar.applied_pos=$5::text::numeric AND ar.txn_complete)",&[&source,&schema,&table,&generation,&pos,&codec,&run,&physical,&target_generation,&batch,&digest,&lineage,&epoch,&grade,&(kind=="gtid")]).await.map_err(Error::sql)?.get(0)
    };
    if !valid {
        return Err(Error::new("target_generation"));
    }
    Ok(())
}
async fn verify_group_applied(
    config: &Config,
    source: &str,
    database: &str,
    request: &Value,
    result: &Value,
) -> Result<()> {
    let scope = request
        .get("transaction_scope")
        .ok_or_else(|| Error::new("transaction_scope"))?;
    let end = pg_lsn_decimal(
        scope
            .get("end_lsn")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("transaction_scope"))?,
    )?;
    let generation = request
        .get("source_generation")
        .and_then(Value::as_i64)
        .ok_or_else(|| Error::new("source_generation"))?;
    let target_generation = result
        .get("target_generation")
        .and_then(Value::as_i64)
        .ok_or_else(|| Error::new("target_generation"))?;
    let entries = request
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::new("transaction_request"))?;
    let receipts = result
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::new("target_generation"))?;
    if result.get("end_lsn").and_then(Value::as_str) != Some(end.as_str())
        || result.get("xid") != scope.get("xid")
        || entries.len() != receipts.len()
    {
        return Err(Error::new("target_generation"));
    }
    let target = target_owner(config, database).await?;
    let xid = scope
        .get("xid")
        .and_then(Value::as_i64)
        .ok_or_else(|| Error::new("transaction_scope"))?;
    let attestation = target
        .query_one(
            "SELECT _oo.native_transaction_read($1,$2,$3::text::numeric)::text",
            &[&source, &xid, &end],
        )
        .await
        .map_err(Error::sql)?
        .get::<_, Option<String>>(0)
        .ok_or_else(|| Error::new("transaction_attestation"))?;
    let attestation: Value =
        serde_json::from_str(&attestation).map_err(|_| Error::new("transaction_attestation"))?;
    if attestation.get("source").and_then(Value::as_str) != Some(source)
        || attestation.get("xid").and_then(Value::as_i64) != Some(xid)
        || attestation.get("end_lsn").and_then(Value::as_u64) != end.parse::<u64>().ok()
        || attestation.get("scope_digest") != result.get("scope_digest")
        || attestation.get("job_task_id") != result.get("job_task_id")
        || attestation.get("target_generation").and_then(Value::as_i64) != Some(target_generation)
        || attestation.get("entries") != result.get("entries")
    {
        return Err(Error::new("transaction_attestation"));
    }
    let watermark:bool=target.query_one("SELECT EXISTS(SELECT 1 FROM _oo.source_watermark WHERE source=$1 AND source_generation=$2 AND applied_pos=$3::text::numeric AND target_generation=$4 AND position_codec='pg-commit-lsn/v1' AND consistency='transactional' AND consistency_grade='T' AND cross_table_consistent)",&[&source,&generation,&end,&target_generation]).await.map_err(Error::sql)?.get(0);
    if !watermark {
        return Err(Error::new("target_generation"));
    }
    for (entry, receipt) in entries.iter().zip(receipts) {
        let schema = entry
            .get("logical_schema")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("transaction_request"))?;
        let table = entry
            .get("logical_table")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("transaction_request"))?;
        let table_generation = entry
            .get("source_generation")
            .and_then(Value::as_i64)
            .ok_or_else(|| Error::new("transaction_request"))?;
        let batch = entry
            .get("batch_id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("transaction_request"))?;
        let physical = receipt
            .get("physical_name")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("target_generation"))?;
        let run = receipt
            .get("run_id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("target_generation"))?;
        let digest = receipt
            .get("payload_digest")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("target_generation"))?;
        if receipt.get("logical_schema").and_then(Value::as_str) != Some(schema)
            || receipt.get("logical_table").and_then(Value::as_str) != Some(table)
            || receipt.get("source_generation").and_then(Value::as_i64) != Some(table_generation)
            || receipt.get("target_generation").and_then(Value::as_i64) != Some(target_generation)
            || receipt.get("batch_id").and_then(Value::as_str) != Some(batch)
        {
            return Err(Error::new("target_generation"));
        }
        let applied:bool=target.query_one("SELECT EXISTS(SELECT 1 FROM _oo.table_freshness f JOIN _oo.full_import_run r ON r.run_id=$6::text::uuid JOIN _oo.full_import_table ft ON ft.run_id=r.run_id JOIN _oo.apply_receipts ar ON ar.source=r.source_id AND ar.table_generation=ft.physical_name WHERE f.source=$1 AND f.schema_name=$2 AND f.table_name=$3 AND f.source_generation=$4 AND f.last_pos=$5::text::numeric AND f.physical_name=$7 AND r.state='awaiting_publish' AND r.source_id=$1 AND r.source_generation=$4 AND r.target_generation=$8 AND ft.schema_ident=$2 AND ft.table_ident=$3 AND ft.physical_name=$7 AND ft.exhausted AND ar.txn_or_batch_id=$9 AND ar.payload_digest=$10 AND ar.applied_pos=$5::text::numeric AND ar.txn_complete)",&[&source,&schema,&table,&table_generation,&end,&run,&physical,&target_generation,&batch,&digest]).await.map_err(Error::sql)?.get(0);
        if !applied {
            return Err(Error::new("target_generation"));
        }
    }
    Ok(())
}
async fn adopt_claim(
    control: &tokio_postgres::Client,
    source: &str,
    database: &str,
    project: &str,
    task: &str,
    worker: &str,
    generation: i64,
) -> Result<()> {
    // Upgrade old queued occurrences only after a real jobs.claim. This records
    // the active fence, never a source generation, ACK, or substitute request.
    let n=control.execute("INSERT INTO jw_native.source_progress(source_id,target_database,active_task_id) SELECT $1,$2,t.id FROM jobs.tasks t WHERE t.id=$3::text::uuid AND t.project_id=$4::text::uuid AND t.state='running' AND t.lease_owner=$5 AND t.generation=$6 AND t.lease_until>clock_timestamp() ON CONFLICT(source_id,target_database) DO UPDATE SET active_task_id=EXCLUDED.active_task_id WHERE jw_native.source_progress.active_task_id IS NULL OR jw_native.source_progress.active_task_id=EXCLUDED.active_task_id",&[&source,&database,&task,&project,&worker,&generation]).await.map_err(Error::sql)?;
    if n != 1 {
        return Err(Error::new("source_active"));
    }
    Ok(())
}
async fn confirm_terminal_c1_base(
    config: &Config,
    control: &tokio_postgres::Client,
    source: &str,
    database: &str,
    binding: &str,
    task: &str,
    stored: &Option<String>,
) -> Result<i64> {
    let active=control.query_opt("SELECT 1 FROM jw_native.source_progress WHERE source_id=$1 AND target_database=$2 AND active_task_id=$3::text::uuid AND binding_id=$4::text::uuid",&[&source,&database,&task,&binding]).await.map_err(Error::sql)?;
    if active.is_none() {
        return Err(Error::new("source_active"));
    }
    let request: Value = stored
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|_| Error::new("request"))?
        .ok_or_else(|| Error::new("request"))?;
    if request.get("source_id").and_then(Value::as_str) != Some(source)
        || c1_codec_closed(&request).is_err()
    {
        return Err(Error::new("request"));
    }
    let source_generation = request
        .get("source_generation")
        .and_then(Value::as_i64)
        .filter(|value| *value > 0)
        .ok_or_else(|| Error::new("source_generation"))?;
    let codec = request
        .get("position_codec")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("position"))?;
    let lineage = request.get("position_lineage").and_then(Value::as_str);
    let epoch = request
        .get("source_epoch")
        .and_then(Value::as_i64)
        .map(i32::try_from)
        .transpose()
        .map_err(|_| Error::new("position"))?;
    let pos = request
        .get("applied_pos")
        .and_then(Value::as_str)
        .filter(|value| gtid_digits(value, true))
        .ok_or_else(|| Error::new("position"))?;
    let target = target_owner(config, database).await?;
    let watermark=target.query_opt("SELECT source_generation FROM _oo.source_watermark WHERE source=$1 AND position_codec=$2 AND position_lineage IS NOT DISTINCT FROM $3 AND source_epoch IS NOT DISTINCT FROM $4 AND applied_pos < $5::text::numeric",&[&source,&codec,&lineage,&epoch,&pos]).await.map_err(Error::sql)?;
    let Some(watermark) = watermark else {
        return Err(Error::new("target_ahead_or_incomparable"));
    };
    let base: i64 = watermark.get(0);
    if base >= source_generation {
        return Err(Error::new("target_ahead"));
    }
    let admin = session::admin_db(config, database).await?;
    let higher:bool=admin.query_one("SELECT EXISTS(SELECT 1 FROM _oo.full_import_run WHERE source_id=$1 AND source_generation>$2)",&[&source,&base]).await.map_err(Error::sql)?.get(0);
    if higher {
        return Err(Error::new("target_ahead"));
    }
    Ok(base)
}
async fn release_terminal_native(config: &Config, project: &str) -> Result<()> {
    let control = session::runtime(config).await?;
    let rows=control.query("SELECT p.source_id,p.target_database,p.active_task_id::text,o.request::text,b.method,b.id::text FROM jw_native.source_progress p JOIN jobs.tasks t ON t.id=p.active_task_id JOIN jw_native.occurrences o ON o.task_id=t.id JOIN jw_native.bindings b ON b.id=o.binding_id WHERE b.project_id=$1::text::uuid AND b.method IN ('C1','C2','C4','C5') AND t.state IN ('failed','cancelled') AND NOT EXISTS(SELECT 1 FROM jw_native.pending_acks a WHERE a.task_id=t.id)",&[&project]).await.map_err(Error::sql)?;
    for row in rows {
        let source: String = row.get(0);
        let database: String = row.get(1);
        let task: String = row.get(2);
        let stored: Option<String> = row.get(3);
        let method: String = row.get(4);
        let binding: String = row.get(5);
        if database != target_database(config, &method)? {
            continue;
        }
        if method=="C1" && stored.is_none() {
            // A rejected operator selection can terminate before capture created a request.
            let prior=control.query_opt("SELECT w.result::text FROM jw_native.source_progress p JOIN jw_native.watermarks w ON w.binding_id=p.binding_id AND w.task_id=p.task_id JOIN jw_native.pending_acks a ON a.task_id=w.task_id JOIN jobs.tasks t ON t.id=w.task_id JOIN jw_native.occurrences o ON o.task_id=p.active_task_id WHERE p.source_id=$1 AND p.target_database=$2 AND p.binding_id=$3::text::uuid AND p.active_task_id=$4::text::uuid AND a.acknowledged_at IS NOT NULL AND t.state='succeeded' AND o.manifest->>'method'='C1' AND o.manifest#>>'{loader,args,0}'='import-native-c1-baseline' AND jsonb_array_length(o.manifest#>'{loader,args}')=1 AND NOT EXISTS(SELECT 1 FROM jw_native.process_attempts x WHERE x.task_id=p.active_task_id AND x.stage='c1_baseline_stream')",&[&source,&database,&binding,&task]).await.map_err(Error::sql)?;
            if let Some(prior)=prior {
                let result:Value=serde_json::from_str(&prior.get::<_,String>(0)).map_err(|_|Error::new("ack_receipt"))?;
                if result["protocol"]!="oo.pipeline.native-c1-baseline-result/v1" || result["source_id"]!=source {continue;}
                let admin=session::admin_db(config,&database).await?;
                let exact:bool=admin.query_one("SELECT EXISTS(SELECT 1 FROM _oo.source_watermark w WHERE w.source=$1 AND w.source_generation=$2 AND w.target_generation=$3 AND w.source_epoch=$4 AND w.applied_pos=$5::text::numeric AND w.position_codec='mysql-gtid/v1' AND w.position_lineage=$6) AND NOT EXISTS(SELECT 1 FROM _oo.native_c1_baseline_run WHERE source=$1 AND task_id=$7::text::uuid) AND NOT EXISTS(SELECT 1 FROM _oo.full_import_run WHERE source_id=$1 AND source_generation>$2)",&[&source,&result["source_generation"].as_i64(),&result["target_generation"].as_i64(),&result["source_epoch"].as_i64().map(|n|n as i32),&result["applied_pos"].as_str(),&result["source_instance"]["measured_id"].as_str(),&task]).await.map_err(Error::sql)?.get(0);
                if exact {
                    let n=control.execute("UPDATE jw_native.source_progress SET active_task_id=NULL WHERE source_id=$1 AND target_database=$2 AND binding_id=$3::text::uuid AND active_task_id=$4::text::uuid AND EXISTS(SELECT 1 FROM jobs.tasks WHERE id=$4::text::uuid AND state IN ('failed','cancelled')) AND NOT EXISTS(SELECT 1 FROM jw_native.pending_acks WHERE task_id=$4::text::uuid)",&[&source,&database,&binding,&task]).await.map_err(Error::sql)?;
                    if n==1 {println!("source_active=released_terminal_pre_capture task={task} source={source} acknowledged_predecessor_unchanged=true");}
                }
            }
            continue;
        }
        let base = match if method == "C1" {
            confirm_terminal_c1_base(
                config, &control, &source, &database, &binding, &task, &stored,
            )
            .await
        } else {
            confirmed_generation(config, &control, &source, &database, &task, &stored).await
        } {
            Ok(base) => base,
            Err(e) => {
                eprintln!("terminal_release=refused task={task} reason={}", e.kind);
                continue;
            }
        };
        let target = match target_owner(config, &database).await {
            Ok(target) => target,
            Err(e) => {
                eprintln!("terminal_release=refused task={task} reason={}", e.kind);
                continue;
            }
        };
        let applied = match target
            .query_opt(
                "SELECT source_generation FROM _oo.source_watermark WHERE source=$1",
                &[&source],
            )
            .await
        {
            Ok(row) => row.map(|r| r.get::<_, i64>(0)).unwrap_or(0),
            Err(e) => {
                eprintln!(
                    "terminal_release=refused task={task} reason={}",
                    Error::sql(e).kind
                );
                continue;
            }
        };
        // Pipeline runtime cannot select full_import_run; 0006 exposes it only through
        // security-definer functions. The operator admin already installs this database.
        let sealed: bool = if method == "C4" {
            match target.query_one("SELECT EXISTS(SELECT 1 FROM _oo.native_partition_state WHERE job_task_id=$1::text::uuid)",&[&task]).await {
    Ok(row)=>row.get(0),Err(e)=>{eprintln!("terminal_release=refused task={task} reason={}",Error::sql(e).kind);continue}
   }
        } else {
            let admin = match session::admin_db(config, &database).await {
                Ok(admin) => admin,
                Err(e) => {
                    eprintln!("terminal_release=refused task={task} reason={}", e.kind);
                    continue;
                }
            };
            let higher=match admin.query_one("SELECT EXISTS(SELECT 1 FROM _oo.full_import_run WHERE source_id=$1 AND source_generation>$2)",&[&source,&base]).await {
    Ok(row)=>row.get(0),Err(e)=>{eprintln!("terminal_release=refused task={task} reason={}",Error::sql(e).kind);continue}
   };
            if method == "C1" {
                let request: Value = match stored
                    .as_deref()
                    .and_then(|raw| serde_json::from_str(raw).ok())
                {
                    Some(value) => value,
                    None => {
                        eprintln!("terminal_release=refused task={task} reason=request");
                        continue;
                    }
                };
                let codec = request
                    .get("position_codec")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let lineage = request
                    .get("position_lineage")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let epoch = request
                    .get("source_epoch")
                    .and_then(Value::as_i64)
                    .unwrap_or(-1);
                let pos = request
                    .get("applied_pos")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let incomparable:bool=match admin.query_one("SELECT EXISTS(SELECT 1 FROM _oo.source_watermark WHERE source=$1 AND (position_codec IS DISTINCT FROM $2 OR position_lineage IS DISTINCT FROM $3 OR COALESCE(source_epoch,-1) IS DISTINCT FROM $4 OR applied_pos >= $5::text::numeric))",&[&source,&codec,&lineage,&epoch,&pos]).await {
     Ok(row)=>row.get(0),Err(e)=>{eprintln!("terminal_release=refused task={task} reason={}",Error::sql(e).kind);continue}
    };
                if applied > base || higher || incomparable {
                    continue;
                }
                false
            } else {
                higher
            }
        };
        if applied != base || sealed {
            continue;
        }
        let n=control.execute("UPDATE jw_native.source_progress p SET active_task_id=NULL WHERE p.source_id=$1 AND p.target_database=$2 AND p.active_task_id=$3::text::uuid AND p.binding_id=$4::text::uuid AND EXISTS(SELECT 1 FROM jobs.tasks t WHERE t.id=p.active_task_id AND t.state IN ('failed','cancelled')) AND NOT EXISTS(SELECT 1 FROM jw_native.pending_acks a WHERE a.task_id=$3::text::uuid)",&[&source,&database,&task,&binding]).await.map_err(Error::sql)?;
        if n == 1 {
            println!("source_active=released_terminal task={task} source={source}")
        }
    }
    Ok(())
}
pub async fn cancel_native(
    config: &Config,
    project: &str,
    task: &str,
    worker: &str,
    generation: i64,
) -> Result<()> {
    if !crate::api::is_uuid(project) || !crate::api::is_uuid(task) || generation < 0 {
        return Err(Error::new("input"));
    }
    let control = session::runtime(config).await?;
    let row=control.query_opt("SELECT b.source_id,b.target_database,o.request::text,b.method FROM jw_native.occurrences o JOIN jw_native.bindings b ON b.id=o.binding_id JOIN jobs.tasks t ON t.id=o.task_id JOIN jw_native.source_progress p ON p.source_id=b.source_id AND p.target_database=b.target_database AND p.active_task_id=t.id WHERE t.id=$1::text::uuid AND t.project_id=$2::text::uuid AND (b.method IN ('C2','C4') AND t.state IN ('queued','running') OR b.method='C5' AND t.state='queued') AND t.generation=$3 AND NOT EXISTS(SELECT 1 FROM jw_native.pending_acks a WHERE a.task_id=t.id)",&[&task,&project,&generation]).await.map_err(Error::sql)?.ok_or_else(||Error::new("cancel_identity"))?;
    let source: String = row.get(0);
    let database: String = row.get(1);
    let stored: Option<String> = row.get(2);
    let method: String = row.get(3);
    if database != target_database(config, &method)? {
        return Err(Error::new("target_database"));
    }
    let base = confirmed_generation(config, &control, &source, &database, task, &stored).await?;
    let target = target_owner(config, &database).await?;
    let applied = target
        .query_opt(
            "SELECT source_generation FROM _oo.source_watermark WHERE source=$1",
            &[&source],
        )
        .await
        .map_err(Error::sql)?
        .map(|r| r.get::<_, i64>(0))
        .unwrap_or(0);
    let sealed: bool = if method == "C4" {
        target.query_one("SELECT EXISTS(SELECT 1 FROM _oo.native_partition_state WHERE job_task_id=$1::text::uuid)",&[&task]).await.map_err(Error::sql)?.get(0)
    } else {
        let admin = session::admin_db(config, &database).await?;
        admin.query_one("SELECT EXISTS(SELECT 1 FROM _oo.full_import_run WHERE source_id=$1 AND source_generation>$2)",&[&source,&base]).await.map_err(Error::sql)?.get(0)
    };
    if applied != base || sealed {
        return Err(Error::new("cancel_target"));
    }
    let cancelled = lease::cancel(&control, project, task, worker, generation).await?;
    if cancelled.status != LeaseStatus::Cancelled {
        return Err(Error::new("cancel_lease"));
    }
    release_terminal_native(config, project).await?;
    println!("native_cancel=cancelled task={task} generation={generation}");
    Ok(())
}
pub async fn install(config: &Config) -> Result<()> {
    let c = session::admin_db(config, &config.database).await?;
    c.batch_execute(include_str!("../native_schedule.sql"))
        .await
        .map_err(Error::sql)?;
    println!(
        "native_schedule_schema=installed database={}",
        config.database
    );
    Ok(())
}
pub async fn put(config: &Config, path: &str) -> Result<()> {
    let m = manifest_read(path)?;
    let (text, hash) = canonical(&m)?;
    let c = session::runtime(config).await?;
    let target_database = target_database(config, &m.method)?;
    let changed=c.execute("INSERT INTO jw_native.bindings(id,project_id,source_id,method,cycle,period_ms,manifest_sha,manifest,target_database) VALUES($1::text::uuid,$2::text::uuid,$3,$4,$5,$6,$7,$8::text::jsonb,$9) ON CONFLICT(id) DO UPDATE SET period_ms=EXCLUDED.period_ms,enabled=true,manifest_sha=EXCLUDED.manifest_sha,manifest=EXCLUDED.manifest,target_database=EXCLUDED.target_database WHERE jw_native.bindings.project_id=EXCLUDED.project_id AND jw_native.bindings.source_id=EXCLUDED.source_id AND jw_native.bindings.cycle=EXCLUDED.cycle AND COALESCE(jw_native.bindings.target_database,EXCLUDED.target_database)=EXCLUDED.target_database", &[&m.binding,&m.project_id,&m.source_id,&m.method,&m.cycle,&m.period_ms,&hash,&text,&target_database]).await.map_err(Error::sql)?;
    if changed != 1 {
        return Err(Error::new("binding_identity"));
    }
    println!(
        "schedule=registered binding={} method={} cycle={} period_ms={} manifest_sha={}",
        m.binding, m.method, m.cycle, m.period_ms, hash
    );
    Ok(())
}
pub async fn enabled(config: &Config, binding: &str, value: bool) -> Result<()> {
    if !crate::api::is_uuid(binding) {
        return Err(Error::new("binding"));
    }
    let c = session::runtime(config).await?;
    let n = c
        .execute(
            "UPDATE jw_native.bindings SET enabled=$2 WHERE id=$1::text::uuid",
            &[&binding, &value],
        )
        .await
        .map_err(Error::sql)?;
    println!("schedule_enabled={value} rows={n}");
    Ok(())
}
pub async fn fire(config: &Config, project: &str) -> Result<()> {
    release_terminal_native(config, project).await?;
    let mut c = session::runtime(config).await?;
    let tx = c.transaction().await.map_err(Error::sql)?;
    let rows=tx.query("SELECT id::text FROM jw_native.bindings WHERE project_id=$1::text::uuid AND enabled AND target_database IS NOT NULL AND next_run_at<=clock_timestamp() ORDER BY next_run_at,id",&[&project]).await.map_err(Error::sql)?;
    for row in rows {
        let binding: String = row.get(0);
        let Some(row)=tx.query_opt("SELECT pin,manifest_sha,period_ms,next_run_at::text,manifest::text,source_id,target_database FROM jw_native.bindings WHERE id=$1::text::uuid AND project_id=$2::text::uuid AND enabled AND next_run_at<=clock_timestamp() FOR UPDATE SKIP LOCKED",&[&binding,&project]).await.map_err(Error::sql)? else {continue};
        let pin: i64 = row.get::<_, i64>(0) + 1;
        let hash: String = row.get(1);
        let period: i64 = row.get(2);
        let at: String = row.get(3);
        let manifest: String = row.get(4);
        let source: String = row.get(5);
        let target_database: String = row.get(6);
        tx.execute("INSERT INTO jw_native.source_progress(source_id,target_database) VALUES($1,$2) ON CONFLICT DO NOTHING",&[&source,&target_database]).await.map_err(Error::sql)?;
        let gate=tx.query_opt("SELECT active_task_id::text FROM jw_native.source_progress WHERE source_id=$1 AND target_database=$2 FOR UPDATE SKIP LOCKED",&[&source,&target_database]).await.map_err(Error::sql)?;
        let Some(gate) = gate else { continue };
        if gate.get::<_, Option<String>>(0).is_some() {
            continue;
        }
        let older=tx.query_one("SELECT EXISTS(SELECT 1 FROM jw_native.bindings b JOIN jw_native.occurrences o ON o.binding_id=b.id JOIN jobs.tasks t ON t.id=o.task_id WHERE b.source_id=$1 AND b.target_database=$2 AND (t.state IN ('queued','running') OR EXISTS(SELECT 1 FROM jw_native.pending_acks a WHERE a.task_id=t.id AND a.acknowledged_at IS NULL)))",&[&source,&target_database]).await.map_err(Error::sql)?;
        if older.get::<_, bool>(0) {
            continue;
        }
        let payload = crate::capture_plan::CapturePayload::text(&binding, pin);
        let digest = sha256_hex(payload.as_bytes());
        let dedupe = format!("native/{binding}/{pin}");
        let r = lease::enqueue(
            &tx,
            lease::Enqueue {
                project_id: project,
                kind: KIND,
                payload_version: 1,
                payload: &payload,
                config_version: "jw-native-1",
                digest: &digest,
                dedupe_key: &dedupe,
                run_delay_seconds: 0.,
                max_attempts: 8,
            },
        )
        .await?;
        let task = r.task_id.ok_or_else(|| Error::new("enqueue"))?;
        tx.execute("INSERT INTO jw_native.occurrences(binding_id,pin,task_id,manifest_sha,scheduled_at,manifest) VALUES($1::text::uuid,$2,$3::text::uuid,$4,$5::text::timestamptz,$6::text::jsonb)",&[&binding,&pin,&task,&hash,&at,&manifest]).await.map_err(Error::sql)?;
        tx.execute("UPDATE jw_native.source_progress SET active_task_id=$3::text::uuid WHERE source_id=$1 AND target_database=$2 AND active_task_id IS NULL",&[&source,&target_database,&task]).await.map_err(Error::sql)?;
        tx.execute("UPDATE jw_native.bindings SET pin=$2,next_run_at=clock_timestamp()+$3::bigint*interval '1 millisecond' WHERE id=$1::text::uuid",&[&binding,&pin,&period]).await.map_err(Error::sql)?;
        println!("fire=enqueued binding={binding} pin={pin} task={task} period_ms={period}");
    }
    tx.commit().await.map_err(Error::sql)?;
    Ok(())
}
// Drain pipes concurrently: large producer/loader outputs must never deadlock heartbeat.
#[allow(clippy::too_many_arguments)]
async fn exec(
    config: &Config,
    e: &Executable,
    stage: &str,
    input: Vec<u8>,
    registration: Option<&Registration>,
    registry_override: Option<(&str, &str)>,
    binding: &str,
    pin: i64,
    project: &str,
    task: &str,
    worker: &str,
    generation: i64,
    target: Option<&tokio_postgres::Client>,
) -> Result<Value> {
    executable_check(e)?;
    let mut command = Command::new(&e.path);
    command
        .args(&e.args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(r) = registration {
        executable_check(&Executable {
            path: r.admission_binary.clone(),
            sha256: r.admission_sha256.clone(),
            args: vec![],
        })?;
        command.env("OO_NATIVE_ADMISSION_BIN", &r.admission_binary);
        if let Some(path) = &r.registry_path {
            let registry = path
                .replace("{binding}", binding)
                .replace("{pin}", &pin.to_string());
            if !Path::new(&registry).is_absolute() || registry.contains("..") {
                return Err(Error::new("registration"));
            }
            let bytes = std::fs::read(&registry).map_err(|_| Error::new("registration"))?;
            let c = session::runtime(config).await?;
            let prior=c.query_opt("SELECT stdout FROM jw_native.process_attempts WHERE task_id=$1::text::uuid AND stage='source_registry' ORDER BY generation LIMIT 1",&[&task]).await.map_err(Error::sql)?;
            let prior_bytes = prior.as_ref().map(|row| row.get::<_, Vec<u8>>(0));
            let sha = vector_bytes_sha(&bytes, None, prior_bytes.as_deref())?;
            let n=c.execute("INSERT INTO jw_native.process_attempts(task_id,generation,stage,executable_sha256,exit_code,stdout,stderr) SELECT id,generation,'source_registry',$4,0,$5,''::bytea FROM jobs.tasks WHERE id=$1::text::uuid AND generation=$2 AND lease_owner=$3 AND state='running' AND lease_until>clock_timestamp() ON CONFLICT DO NOTHING",&[&task,&generation,&worker,&r.admission_sha256,&bytes]).await.map_err(Error::sql)?;
            if n == 0 && prior.is_none() {
                return Err(Error::new("lost_lease"));
            }
            command
                .env("OO_NATIVE_BINDING_REGISTRY", &registry)
                .env("OO_NATIVE_BINDING_REGISTRY_SHA256", sha);
        }
        if let Some(path) = &r.transaction_registry_vector_path {
            let path = resolve_vector_path(path, binding, pin)?;
            let bytes = std::fs::read(&path).map_err(|_| Error::new("registration"))?;
            // Freeze the operator-selected vector across every attempt of this UUID.
            let c = session::runtime(config).await?;
            let prior=c.query_opt("SELECT stdout FROM jw_native.process_attempts WHERE task_id=$1::text::uuid AND stage='registry_vector' ORDER BY generation LIMIT 1",&[&task]).await.map_err(Error::sql)?;
            let prior_bytes = prior.as_ref().map(|row| row.get::<_, Vec<u8>>(0));
            let sha = vector_bytes_sha(
                &bytes,
                r.transaction_registry_vector_sha256.as_deref(),
                prior_bytes.as_deref(),
            )?;
            let n=c.execute("INSERT INTO jw_native.process_attempts(task_id,generation,stage,executable_sha256,exit_code,stdout,stderr) SELECT id,generation,'registry_vector',$4,0,$5,''::bytea FROM jobs.tasks WHERE id=$1::text::uuid AND generation=$2 AND lease_owner=$3 AND state='running' AND lease_until>clock_timestamp() ON CONFLICT DO NOTHING",&[&task,&generation,&worker,&r.admission_sha256,&bytes]).await.map_err(Error::sql)?;
            if n == 0 && prior.is_none() {
                return Err(Error::new("lost_lease"));
            }
            command
                .env("OO_NATIVE_TRANSACTION_REGISTRY_VECTOR", &path)
                .env("OO_NATIVE_TRANSACTION_REGISTRY_VECTOR_SHA256", &sha);
            println!(
                "registry_vector_sha={sha} binding={binding} pin={pin} source_request_rewritten=false"
            );
        }
        if let (Some(path), Some(sha)) = (&r.control_target_path, &r.control_target_sha256) {
            let bytes = std::fs::read(path).map_err(|_| Error::new("registration"))?;
            if bytes.len() > 65536 || sha256_hex(&bytes) != *sha {
                return Err(Error::new("registration"));
            }
            command
                .env("OO_NATIVE_CONTROL_TARGET", path)
                .env("OO_NATIVE_CONTROL_TARGET_SHA256", sha);
        }
    }
    if let Some((path, sha)) = registry_override {
        let bytes = std::fs::read(path).map_err(|_| Error::new("registration"))?;
        if !Path::new(path).is_absolute()
            || path.contains("..")
            || bytes.len() > 1024 * 1024
            || sha256_hex(&bytes) != sha
        {
            return Err(Error::new("registration"));
        }
        command
            .env("OO_NATIVE_BINDING_REGISTRY", path)
            .env("OO_NATIVE_BINDING_REGISTRY_SHA256", sha);
    }
    let mut child = command.spawn().map_err(|_| Error::new("spawn"))?;
    let mut stdin = child.stdin.take().ok_or_else(|| Error::new("spawn"))?;
    let writer = tokio::spawn(async move {
        stdin.write_all(&input).await?;
        stdin.shutdown().await
    });
    let stdout = child.stdout.take().ok_or_else(|| Error::new("spawn"))?;
    let stderr = child.stderr.take().ok_or_else(|| Error::new("spawn"))?;
    let out = tokio::spawn(async move {
        let mut b = Vec::new();
        stdout
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut b)
            .await
            .map(|_| b)
    });
    let err = tokio::spawn(async move {
        let mut b = Vec::new();
        stderr.take(65537).read_to_end(&mut b).await.map(|_| b)
    });
    let control = session::runtime(config).await?;
    let status = loop {
        tokio::select! {
         r=child.wait()=>break r.map_err(|_|Error::new("spawn"))?,
         _=tokio::time::sleep(Duration::from_secs(10))=>{
          let r=lease::heartbeat(&control,project,task,worker,generation).await?;
          if r.status!=LeaseStatus::Extended {let _=child.kill().await;return Err(Error::new("lost_lease"))}
          if let Some(target)=target {
           let r=lease::heartbeat(target,project,task,worker,generation).await?;
           if r.status!=LeaseStatus::Extended {let _=child.kill().await;return Err(Error::new("target_lease"))}
          }
         }
        }
    };
    writer
        .await
        .map_err(|_| Error::new("spawn"))?
        .map_err(|_| Error::new("spawn"))?;
    let bytes = out
        .await
        .map_err(|_| Error::new("spawn"))?
        .map_err(|_| Error::new("spawn"))?;
    let errors = err
        .await
        .map_err(|_| Error::new("spawn"))?
        .map_err(|_| Error::new("spawn"))?;
    let code = status.code().unwrap_or(-1);
    control.execute("INSERT INTO jw_native.process_attempts(task_id,generation,stage,executable_sha256,exit_code,stdout,stderr) VALUES($1::text::uuid,$2,$3,$4,$5,$6,$7) ON CONFLICT DO NOTHING",&[&task,&generation,&stage,&e.sha256,&code,&bytes,&errors]).await.map_err(Error::sql)?;
    println!(
        "process_stage={stage} task={task} generation={generation} exit_code={code} stdout_bytes={} stderr_bytes={} raw_receipt=jw_native.process_attempts",
        bytes.len(),
        errors.len()
    );
    if !status.success() || bytes.len() > 4 * 1024 * 1024 || errors.len() > 65536 {
        return Err(Error::new("process"));
    }
    serde_json::from_slice(&bytes).map_err(|_| Error::new("protocol"))
}
pub async fn step(config: &Config, project: &str, worker: &str) -> Result<()> {
    release_terminal_native(config, project).await?;
    let mut c = session::runtime(config).await?;
    let pending=c.query("SELECT a.task_id::text FROM jw_native.pending_acks a JOIN jw_native.bindings b ON b.id=a.binding_id WHERE b.project_id=$1::text::uuid AND a.acknowledged_at IS NULL ORDER BY a.task_id",&[&project]).await.map_err(Error::sql)?;
    let mut ack_error = None;
    let mut acknowledged = false;
    for row in pending {
        let pending_task: String = row.get(0);
        match drain_ack(config, &mut c, project, Some(&pending_task)).await {
            Ok(done) => acknowledged |= done,
            Err(e) => {
                eprintln!("ack_retry_error={} task={pending_task}", e.kind);
                ack_error = Some(e)
            }
        }
    }
    if acknowledged {
        return Ok(());
    }
    let selected_install=c.query_opt("SELECT checksum,filename FROM jobs.schema_migrations WHERE version='jobs-claim-selected/1'",&[]).await.map_err(Error::sql)?;
    if !selected_install.is_some_and(|r| {
        r.get::<_, String>(0) == "d2daaedcdfec509334df7eda09e5ac6fa277ae2291b16d12293e1deb4270b9d0"
            && r.get::<_, String>(1) == "0009_claim_selected_expand.sql"
    }) {
        return Err(Error::new("claim_selected_install"));
    }
    // Select the immutable occurrence before claiming it. An older retry may
    // have a later run_at than its sibling; generic jobs.claim could steal the
    // sibling while the source's active fence still belongs to the retry.
    let candidate=c.query_opt("WITH waiting AS (SELECT t.id,t.project_id,t.run_at,t.state,t.lease_until,p.active_task_id,ROW_NUMBER() OVER (PARTITION BY b.source_id,b.target_database ORDER BY CASE WHEN b.method='C1' THEN (o.request->>'source_generation')::bigint END NULLS LAST,o.scheduled_at,t.id) AS source_order FROM jw_native.occurrences o JOIN jw_native.bindings b ON b.id=o.binding_id JOIN jobs.tasks t ON t.id=o.task_id LEFT JOIN jw_native.source_progress p ON p.source_id=b.source_id AND p.target_database=b.target_database WHERE t.kind='native_capture' AND t.state IN ('queued','running') AND b.target_database IS NOT NULL AND b.target_database=CASE WHEN b.method='C4' THEN $2 WHEN b.method='C1' THEN COALESCE($3,$4) ELSE $4 END) SELECT id::text FROM waiting WHERE project_id=$1::text::uuid AND (active_task_id=id OR active_task_id IS NULL AND source_order=1) AND (state='queued' AND run_at<=clock_timestamp() OR state='running' AND lease_until<=clock_timestamp()) ORDER BY run_at,id LIMIT 1",&[&project,&config.target_database,&config.c1_target_database,&config.database]).await.map_err(Error::sql)?;
    let Some(candidate) = candidate else {
        println!("claim=none");
        return ack_error.map_or(Ok(()), Err);
    };
    let selected: String = candidate.get(0);
    let r = lease::claim_selected(&c, project, &selected, &[KIND.into()], worker).await?;
    if r.status == LeaseStatus::None {
        println!("claim=none");
        return ack_error.map_or(Ok(()), Err);
    }
    if r.status != LeaseStatus::Claimed {
        return Err(Error::new("claim"));
    }
    let task = r.task_id.ok_or_else(|| Error::new("claim"))?;
    let generation = r.generation.ok_or_else(|| Error::new("claim"))?;
    if task != selected {
        return Err(Error::new("claim_identity"));
    }
    println!("claim=claimed task={task} generation={generation} worker={worker}");
    match apply(config, &mut c, project, worker, &task, generation).await {
        Ok(()) => Ok(()),
        Err(e) => {
            let r = lease::retry_failure(&c, project, &task, worker, generation, "worker_failed")
                .await?;
            println!(
                "apply=refused reason={} retry={}",
                e.kind,
                r.status.as_str()
            );
            Err(e)
        }
    }
}
async fn apply(
    config: &Config,
    c: &mut tokio_postgres::Client,
    project: &str,
    worker: &str,
    task: &str,
    generation: i64,
) -> Result<()> {
    let row=c.query_one("SELECT o.manifest::text,o.manifest_sha,o.request::text,o.pin,b.id::text,t.payload::text,t.artifact_digest,(SELECT pin FROM jw_native.watermarks WHERE binding_id=b.id),b.target_database,o.producer_input::text FROM jw_native.occurrences o JOIN jw_native.bindings b ON b.id=o.binding_id JOIN jobs.tasks t ON t.id=o.task_id WHERE t.id=$1::text::uuid AND b.project_id=$2::text::uuid",&[&task,&project]).await.map_err(Error::sql)?;
    let text: String = row.get(0);
    let pinned: String = row.get(1);
    let stored: Option<String> = row.get(2);
    let pin: i64 = row.get(3);
    let binding: String = row.get(4);
    let payload: String = row.get(5);
    let m: Manifest = serde_json::from_str(&text).map_err(|_| Error::new("manifest"))?;
    m.shape()?;
    let group = m.loader.args == ["import-native-transaction"];
    let cdc_baseline = m.loader.args == ["import-native-c1-baseline"];
    let baseline = m.method == "C1"
        && m.registration
            .as_ref()
            .is_some_and(|r| r.transaction_registry_vector_path.is_some())
        && !group;
    let database: String = row
        .get::<_, Option<String>>(8)
        .ok_or_else(|| Error::new("target_database"))?;
    if database != target_database(config, &m.method)? {
        return Err(Error::new("target_database"));
    }
    let parsed = crate::capture_plan::CapturePayload::parse(&payload)?;
    let artifact: String = row.get(6);
    if canonical(&m)?.1 != pinned
        || parsed.binding != binding
        || parsed.pin != pin
        || artifact
            != sha256_hex(crate::capture_plan::CapturePayload::text(&binding, pin).as_bytes())
    {
        return Err(Error::new("manifest_changed"));
    }
    adopt_claim(
        c,
        &m.source_id,
        &database,
        project,
        task,
        worker,
        generation,
    )
    .await?;
    let override_row=c.query_opt("SELECT operator_manifest::text,operator_manifest_sha FROM jw_native.loader_overrides WHERE binding_id=$1::text::uuid",&[&binding]).await.map_err(Error::sql)?;
    let effective = match override_row {
        Some(row) => {
            let text: String = row.get(0);
            let sha: String = row.get(1);
            let selected: Manifest =
                serde_json::from_str(&text).map_err(|_| Error::new("loader_override"))?;
            selected.shape()?;
            if canonical(&selected)?.1 != sha
                || selected.binding != m.binding
                || selected.project_id != m.project_id
                || selected.source_id != m.source_id
                || selected.method != m.method
                || selected.cycle != m.cycle
                || (baseline || cdc_baseline)
                    && (selected.registration != m.registration
                        || selected.loader.args != m.loader.args)
            {
                return Err(Error::new("loader_override"));
            }
            if selected.producer != m.producer {
                if !cdc_baseline || selected.producer.args != m.producer.args {
                    return Err(Error::new("baseline_producer_override"));
                }
                let path=selected.producer.args.get(2).ok_or_else(||Error::new("baseline_config"))?;
                let digest=sha256_hex(&std::fs::read(path).map_err(|_|Error::new("baseline_config"))?);
                let previous=c.query_opt("SELECT a.stdout FROM jw_native.process_attempts a JOIN jw_native.occurrences o ON o.task_id=a.task_id WHERE a.stage='c1_baseline_stream' AND o.binding_id=$2::text::uuid AND (a.task_id=$1::text::uuid OR EXISTS(SELECT 1 FROM jw_native.watermarks w JOIN jw_native.pending_acks k ON k.task_id=w.task_id JOIN jobs.tasks t ON t.id=w.task_id WHERE w.binding_id=o.binding_id AND w.task_id=a.task_id AND t.state='succeeded' AND k.acknowledged_at IS NOT NULL)) ORDER BY (a.task_id=$1::text::uuid) DESC,a.generation LIMIT 1",&[&task,&binding]).await.map_err(Error::sql)?
                    .ok_or_else(||Error::new("baseline_original_config"))?;
                let raw:Vec<u8>=previous.get(0);
                let open:Value=serde_json::from_slice(raw.split(|b|*b==b'\n').next().ok_or_else(||Error::new("baseline_original_config"))?).map_err(|_|Error::new("baseline_original_config"))?;
                if open["kind"]!="open" || open["connection_config_sha256"]!=digest {
                    return Err(Error::new("baseline_original_config"));
                }
            }
            println!(
                "loader_operator_override={sha} original_occurrence_manifest={pinned} source_request_rewritten=false"
            );
            selected
        }
        None => m.clone(),
    };
    let base = if m.method == "C1" && !group && !baseline && !cdc_baseline {
        confirmed_c1_generation(config, c, &m.source_id, &database, &binding, task, &stored).await?
    } else {
        confirmed_generation(config, c, &m.source_id, &database, task, &stored).await?
    };
    let mut request = match stored {
        Some(s) => serde_json::from_str(&s).map_err(|_| Error::new("request"))?,
        None => {
            let acknowledged_pin: Option<i64> = row.get(7);
            let input = match row.get::<_, Option<String>>(9) {
                Some(saved) => serde_json::from_str::<Value>(&saved)
                    .map_err(|_| Error::new("producer_input"))?,
                None => {
                    let fresh = json!({"protocol":"oo.jobs.native-producer/v1","binding":binding,"pin":pin,"source_id":m.source_id,"method":m.method,"cycle":m.cycle,"period_ms":m.period_ms,"acknowledged_source_generation":base,"acknowledged_pin":acknowledged_pin});
                    let n=c.execute("UPDATE jw_native.occurrences o SET producer_input=$2::text::jsonb FROM jobs.tasks t WHERE o.task_id=t.id AND t.id=$1::text::uuid AND t.state='running' AND t.lease_owner=$3 AND t.generation=$4 AND t.lease_until>clock_timestamp() AND o.producer_input IS NULL",&[&task,&fresh.to_string(),&worker,&generation]).await.map_err(Error::sql)?;
                    if n != 1 {
                        return Err(Error::new("lost_lease"));
                    }
                    fresh
                }
            };
            if input.get("protocol").and_then(Value::as_str) != Some("oo.jobs.native-producer/v1")
                || input.get("binding").and_then(Value::as_str) != Some(&binding)
                || input.get("pin").and_then(Value::as_i64) != Some(pin)
                || input.get("source_id").and_then(Value::as_str) != Some(&m.source_id)
                || input.get("method").and_then(Value::as_str) != Some(&m.method)
                || input
                    .get("acknowledged_source_generation")
                    .and_then(Value::as_i64)
                    != Some(base)
            {
                return Err(Error::new("producer_input"));
            }
            let value = if cdc_baseline {
                let staged = run_cdc_baseline_stream(
                    config,
                    c,
                    project,
                    worker,
                    task,
                    generation,
                    &binding,
                    pin,
                    &m.source_id,
                    &database,
                    &effective.producer,
                    &effective.loader,
                    base,
                    effective.registration.as_ref().ok_or_else(|| Error::new("registration"))?,
                )
                .await?;
                if staged.pointer("/request/protocol").and_then(Value::as_str)!=Some("oo.jobs.mysql-c1-nochange/v1") {
                println!(
                    "baseline=cdc-first-staged task={task} source_epoch={} frames={} stdout_sha256={}",
                    staged
                        .pointer("/source_instance/source_epoch")
                        .and_then(Value::as_i64)
                        .unwrap_or_default(),
                    staged["frames"].as_u64().unwrap_or_default(),
                    staged["stdout_sha256"].as_str().unwrap_or("")
                );
                }
                staged.get("request").cloned().ok_or_else(|| Error::new("baseline_materialization"))?
            } else {
                exec(
                    config,
                    &m.producer,
                    "producer",
                    serde_json::to_vec(&input).unwrap(),
                    None,
                    None,
                    &binding,
                    pin,
                    project,
                    task,
                    worker,
                    generation,
                    None,
                )
                .await?
            };
            let c1_quiet=cdc_baseline && value["protocol"]=="oo.jobs.mysql-c1-nochange/v1";
            let nochange = m.method == "C5"
                && value.get("protocol").and_then(Value::as_str)
                    == Some("oo.jobs.native-c5-nochange/v1");
            let protocol = if cdc_baseline {
                "oo.pipeline.native-c1-baseline/v1"
            } else if group {
                "oo.jobs.native-transaction-producer/v1"
            } else if baseline {
                "oo.jobs.native-baseline-producer/v1"
            } else {
                request_protocol(&m.method)
            };
            if nochange {
                c5_nochange_shape(&value, &binding, pin, &m.source_id, base)?;
            } else if !c1_quiet && (value.get("protocol").and_then(Value::as_str) != Some(protocol)
                || value.get("source_id").and_then(Value::as_str) != Some(&m.source_id)
                || value.get("capture_method").and_then(Value::as_str)
                    != Some(method_token(&m.method)?))
            {
                return Err(Error::new("source_binding"));
            }
            let s = serde_json::to_string(&value).unwrap();
            let n=c.execute("UPDATE jw_native.occurrences o SET request=$2::text::jsonb FROM jobs.tasks t WHERE o.task_id=t.id AND t.id=$1::text::uuid AND t.state='running' AND t.lease_owner=$3 AND t.generation=$4 AND t.lease_until>clock_timestamp() AND o.request IS NULL",&[&task,&s,&worker,&generation]).await.map_err(Error::sql)?;
            if n != 1 {
                return Err(Error::new("lost_lease"));
            }
            value
        }
    };
    if cdc_baseline && request["protocol"]=="oo.jobs.mysql-c1-nochange/v1" {
        return finish_c1_quiet(c,project,worker,task,generation,&binding,pin,&m.source_id,&database,base,&request).await;
    }
    if m.method == "C5"
        && request.get("protocol").and_then(Value::as_str) == Some("oo.jobs.native-c5-nochange/v1")
    {
        return finish_c5_nochange(
            c,
            project,
            worker,
            task,
            generation,
            &binding,
            pin,
            &m.source_id,
            &database,
            base,
            &request,
        )
        .await;
    }
    let protocol = if cdc_baseline {
        "oo.pipeline.native-c1-baseline/v1"
    } else if group {
        "oo.jobs.native-transaction-producer/v1"
    } else if baseline {
        "oo.jobs.native-baseline-producer/v1"
    } else {
        request_protocol(&m.method)
    };
    if request.get("protocol").and_then(Value::as_str) != Some(protocol)
        || request.get("source_id").and_then(Value::as_str) != Some(&m.source_id)
        || (!cdc_baseline
            && request.get("capture_method").and_then(Value::as_str)
                != Some(method_token(&m.method)?))
    {
        return Err(Error::new("source_binding"));
    }
    if request.get("source_generation").and_then(Value::as_i64) != Some(base + 1) {
        return Err(Error::new("source_generation"));
    }
    let target_database = if m.method == "C4" {
        config
            .target_database
            .as_deref()
            .ok_or_else(|| Error::new("target_database"))?
    } else {
        database.as_str()
    };
    let loader_request = if cdc_baseline {
        validate_cdc_baseline_request(
            &request,
            &m.source_id,
            &binding,
            pin,
            base,
            target_database,
            task,
            generation,
        )?
    } else if baseline {
        request.clone()
    } else if group {
        let value = group_loader_request(
            &request,
            &binding,
            pin,
            &m.source_id,
            base,
            task,
            generation,
        )?;
        if value["entries"].as_array().is_none_or(|entries| {
            entries.iter().any(|entry| {
                entry.pointer("/target/database").and_then(Value::as_str) != Some(target_database)
            })
        }) {
            return Err(Error::new("target_database"));
        }
        value
    } else {
        if request.pointer("/target/database").and_then(Value::as_str) != Some(target_database) {
            return Err(Error::new("target_database"));
        }
        crate::capture_source::attach_observed_lease(&mut request, task, generation)
            .map_err(|_| Error::new("lease"))?;
        request.clone()
    };
    if cdc_baseline {
        return apply_cdc_baseline(
            config,
            c,
            project,
            worker,
            task,
            generation,
            &binding,
            pin,
            &m,
            &effective,
            &database,
            base,
            &loader_request,
        )
        .await;
    }
    if baseline {
        return apply_baseline(
            config, c, project, worker, task, generation, &binding, pin, &m, &effective, &database,
            base, &request,
        )
        .await;
    }
    // mirror is an invocation-only policy, excluded from the native source payload digest.
    // Preserve original stored request; require actual Loader mirror recovery before CAS.
    let mut loader_request = loader_request;
    if !group {
        invocation_mirror(&mut loader_request, &m.method)?
    }
    println!(
        "invocation_mirror={} source_request_rewritten=false",
        !group && m.method != "C4"
    );
    let target = if m.method == "C4" {
        Some(
            mirror_claim(
                config,
                c,
                project,
                task,
                worker,
                generation,
                &effective.loader,
            )
            .await?,
        )
    } else {
        None
    };
    let result = exec(
        config,
        &effective.loader,
        "loader",
        serde_json::to_vec(&loader_request).unwrap(),
        effective.registration.as_ref(),
        None,
        &binding,
        pin,
        project,
        task,
        worker,
        generation,
        target.as_ref(),
    )
    .await?;
    let lease_id = result
        .pointer("/jobs_lease_receipt/id")
        .and_then(Value::as_str);
    let lease_gen = result
        .pointer("/jobs_lease_receipt/generation")
        .and_then(Value::as_i64);
    let lease_owner = result
        .pointer("/jobs_lease_receipt/owner")
        .and_then(Value::as_str);
    if lease_id != Some(task) || lease_gen != Some(generation) || lease_owner != Some(worker) {
        return Err(Error::new("lease_result"));
    }
    let c4_ready = m.method == "C4"
        && result.get("source_id").and_then(Value::as_str) == Some(&m.source_id)
        && result.get("source_bound").and_then(Value::as_bool) == Some(true)
        && crate::capture_source::producer_trust_authoritative(&result)
        && crate::capture_source::fence_held(&result)
        && matches!(
            result.get("load_state").and_then(Value::as_str),
            Some("awaiting_publish" | "replay")
        )
        && (result.get("mirror_committed").and_then(Value::as_bool) == Some(false)
            || result.get("load_state").and_then(Value::as_str) == Some("replay")
                && result.get("receipt").and_then(Value::as_str) == Some("replay")
                && result.get("mirror_committed").is_none())
        && result
            .pointer("/partition_target_receipt/acknowledged")
            .and_then(Value::as_bool)
            == Some(false);
    if m.method == "C4" && !c4_ready {
        return Err(Error::new("loader_refused"));
    }
    if group {
        if result.get("protocol").and_then(Value::as_str)
            != Some("oo.pipeline.native-transaction-result/v1")
            || result.get("source_id").and_then(Value::as_str) != Some(&m.source_id)
            || result.get("source_generation").and_then(Value::as_i64)
                != request.get("source_generation").and_then(Value::as_i64)
            || result.get("source_bound").and_then(Value::as_bool) != Some(true)
            || result.get("job_task_id").and_then(Value::as_str) != Some(task)
            || result.get("job_generation").and_then(Value::as_i64) != Some(generation)
            || !matches!(
                result.get("load_state").and_then(Value::as_str),
                Some("awaiting_publish" | "replay")
            )
        {
            return Err(Error::new("loader_refused"));
        }
    } else if m.method != "C4"
        && !matches!(
            crate::capture_source::native_decision(&result),
            crate::capture_source::NativeDecision::Advance
                | crate::capture_source::NativeDecision::Replay
        )
    {
        return Err(Error::new("loader_refused"));
    }
    let source_generation = request
        .get("source_generation")
        .and_then(Value::as_i64)
        .ok_or_else(|| Error::new("position"))?;
    let raw_pos = if group {
        result.get("end_lsn")
    } else if m.method == "C4" {
        result.pointer("/partition_target_receipt/partition_scope/partition_id")
    } else {
        request.get("applied_pos")
    }
    .ok_or_else(|| Error::new("position"))?;
    let pos = raw_pos
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| raw_pos.to_string());
    let state = result
        .get("load_state")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let result_text = result.to_string();
    if group {
        verify_group_applied(config, &m.source_id, &database, &request, &result).await?
    } else if m.method == "C1" {
        verify_c1_applied(config, &m.source_id, &database, &request, &result).await?
    } else {
        verify_applied(config, &m.source_id, &database, source_generation).await?
    }
    let tx = c.transaction().await.map_err(Error::sql)?;
    if m.method == "C4" {
        let ack = ack_request(&result, task, generation)?;
        tx.execute("INSERT INTO jw_native.pending_acks(task_id,binding_id,pin,generation,ack_request,loader_path,loader_sha256) VALUES($1::text::uuid,$2::text::uuid,$3,$4,$5::text::jsonb,$6,$7)",&[&task,&binding,&pin,&generation,&ack.to_string(),&effective.loader.path,&effective.loader.sha256]).await.map_err(Error::sql)?;
        tx.execute(
            "UPDATE jw_native.occurrences SET result=$2::text::jsonb WHERE task_id=$1::text::uuid",
            &[&task, &result_text],
        )
        .await
        .map_err(Error::sql)?;
        let done = lease::complete_success(&tx, project, task, worker, generation).await?;
        if done.status != LeaseStatus::Succeeded {
            return Err(Error::new("lost_lease"));
        }
        tx.commit().await.map_err(Error::sql)?;
        println!(
            "complete=cas_committed_ack_pending binding={binding} pin={pin} task={task} generation={generation}"
        );
        return drain_ack(config, c, project, Some(task))
            .await
            .and_then(|ok| {
                if ok {
                    Ok(())
                } else {
                    Err(Error::new("ack_pending"))
                }
            });
    }
    let done = lease::complete_success(&tx, project, task, worker, generation).await?;
    if done.status != LeaseStatus::Succeeded {
        return Err(Error::new("lost_lease"));
    }
    tx.execute("INSERT INTO jw_native.watermarks(binding_id,pin,task_id,generation,source_generation,applied_pos,load_state,result) VALUES($1::text::uuid,$2,$3::text::uuid,$4,$5,$6,$7,$8::text::jsonb) ON CONFLICT(binding_id) DO UPDATE SET pin=EXCLUDED.pin,task_id=EXCLUDED.task_id,generation=EXCLUDED.generation,source_generation=EXCLUDED.source_generation,applied_pos=EXCLUDED.applied_pos,load_state=EXCLUDED.load_state,result=EXCLUDED.result,advanced_at=clock_timestamp() WHERE jw_native.watermarks.pin<EXCLUDED.pin",&[&binding,&pin,&task,&generation,&source_generation,&pos,&state,&result_text]).await.map_err(Error::sql)?;
    tx.execute(
        "UPDATE jw_native.occurrences SET result=$2::text::jsonb WHERE task_id=$1::text::uuid",
        &[&task, &result_text],
    )
    .await
    .map_err(Error::sql)?;
    let n = if m.method == "C1" && !group {
        tx.execute("UPDATE jw_native.source_progress SET active_task_id=NULL WHERE source_id=$1 AND target_database=$2 AND active_task_id=$3::text::uuid",&[&m.source_id,&database,&task]).await.map_err(Error::sql)?
    } else {
        tx.execute("UPDATE jw_native.source_progress SET source_generation=$3,task_id=$4::text::uuid,binding_id=$5::text::uuid,active_task_id=NULL,confirmed_at=clock_timestamp() WHERE source_id=$1 AND target_database=$2 AND COALESCE(source_generation,0)=$6 AND active_task_id=$4::text::uuid",&[&m.source_id,&database,&source_generation,&task,&binding,&base]).await.map_err(Error::sql)?
    };
    if n != 1 {
        return Err(Error::new("source_active"));
    }
    tx.commit().await.map_err(Error::sql)?;
    println!(
        "complete=succeeded binding={binding} pin={pin} task={task} generation={generation} load_state={state} applied_pos={pos}"
    );
    Ok(())
}
#[allow(clippy::too_many_arguments)]
async fn apply_baseline(
    config: &Config,
    c: &mut tokio_postgres::Client,
    project: &str,
    worker: &str,
    task: &str,
    generation: i64,
    binding: &str,
    pin: i64,
    m: &Manifest,
    effective: &Manifest,
    database: &str,
    base: i64,
    request: &Value,
) -> Result<()> {
    let registration = effective
        .registration
        .as_ref()
        .ok_or_else(|| Error::new("baseline_registry"))?;
    let path = registration
        .transaction_registry_vector_path
        .as_deref()
        .ok_or_else(|| Error::new("baseline_registry"))?;
    let sha = registration
        .transaction_registry_vector_sha256
        .as_deref()
        .ok_or_else(|| Error::new("baseline_registry"))?;
    let bytes = std::fs::read(path).map_err(|_| Error::new("baseline_registry"))?;
    if bytes.len() > 1024 * 1024 || sha256_hex(&bytes) != sha {
        return Err(Error::new("baseline_registry"));
    }
    let vector: Value =
        serde_json::from_slice(&bytes).map_err(|_| Error::new("baseline_registry"))?;
    let entries = baseline_entries(
        request,
        &vector,
        binding,
        pin,
        &m.source_id,
        base,
        database,
        task,
        generation,
    )?;
    let mut receipts = Vec::with_capacity(entries.len());
    for (index, (entry, registry, registry_sha)) in entries.iter().enumerate() {
        let stage = format!("loader_{index}");
        let result = exec(
            config,
            &effective.loader,
            &stage,
            serde_json::to_vec(entry).map_err(|_| Error::new("baseline_request"))?,
            Some(registration),
            Some((registry, registry_sha)),
            binding,
            pin,
            project,
            task,
            worker,
            generation,
            None,
        )
        .await?;
        if result.get("source_id").and_then(Value::as_str) != Some(&m.source_id)
            || result.get("logical_schema") != entry.get("logical_schema")
            || result.get("logical_table") != entry.get("logical_table")
            || result.get("source_generation").and_then(Value::as_i64) != Some(base + 1)
            || result.get("source_bound").and_then(Value::as_bool) != Some(true)
            || !(result.get("mirror_committed").and_then(Value::as_bool) == Some(true)
                || result.get("load_state").and_then(Value::as_str) == Some("replay")
                    && result.get("receipt").and_then(Value::as_str) == Some("replay")
                    && result.get("mirror").and_then(Value::as_str) == Some("replay"))
            || result
                .pointer("/jobs_lease_receipt/id")
                .and_then(Value::as_str)
                != Some(task)
            || result
                .pointer("/jobs_lease_receipt/generation")
                .and_then(Value::as_i64)
                != Some(generation)
            || result
                .pointer("/jobs_lease_receipt/owner")
                .and_then(Value::as_str)
                != Some(worker)
            || !matches!(
                crate::capture_source::native_decision(&result),
                crate::capture_source::NativeDecision::Advance
                    | crate::capture_source::NativeDecision::Replay
            )
        {
            return Err(Error::new("loader_refused"));
        }
        verify_c1_applied(config, &m.source_id, database, entry, &result).await?;
        receipts.push(result);
    }
    let target = target_owner(config, database).await?;
    let last_generation = receipts
        .last()
        .and_then(|r| r.get("target_generation"))
        .and_then(Value::as_i64)
        .ok_or_else(|| Error::new("target_generation"))?;
    let state:bool=target.query_one("SELECT EXISTS(SELECT 1 FROM _oo.source_watermark WHERE source=$1 AND source_generation=$2 AND target_generation=$3 AND consistency='per_table' AND consistency_grade='S1' AND NOT cross_table_consistent AND position_codec='snapshot-zero/v1' AND applied_pos=0) AND (SELECT count(*) FROM _oo.table_freshness WHERE source=$1)=$4",&[&m.source_id,&(base+1),&last_generation,&(receipts.len() as i64)]).await.map_err(Error::sql)?.get(0);
    if !state {
        return Err(Error::new("target_generation"));
    }
    let result = json!({"protocol":"oo.jobs.native-baseline-result/v1","source_id":m.source_id,"source_generation":base+1,"consistency_grade":"S1","cross_table_consistent":false,"load_state":"awaiting_publish","entries":receipts});
    let tx = c.transaction().await.map_err(Error::sql)?;
    let done = lease::complete_success(&tx, project, task, worker, generation).await?;
    if done.status != LeaseStatus::Succeeded {
        return Err(Error::new("lost_lease"));
    }
    tx.execute("INSERT INTO jw_native.watermarks(binding_id,pin,task_id,generation,source_generation,applied_pos,load_state,result) VALUES($1::text::uuid,$2,$3::text::uuid,$4,$5,'0','awaiting_publish',$6::text::jsonb) ON CONFLICT(binding_id) DO UPDATE SET pin=EXCLUDED.pin,task_id=EXCLUDED.task_id,generation=EXCLUDED.generation,source_generation=EXCLUDED.source_generation,applied_pos=EXCLUDED.applied_pos,load_state=EXCLUDED.load_state,result=EXCLUDED.result,advanced_at=clock_timestamp() WHERE jw_native.watermarks.pin<EXCLUDED.pin",&[&binding,&pin,&task,&generation,&(base+1),&result.to_string()]).await.map_err(Error::sql)?;
    tx.execute(
        "UPDATE jw_native.occurrences SET result=$2::text::jsonb WHERE task_id=$1::text::uuid",
        &[&task, &result.to_string()],
    )
    .await
    .map_err(Error::sql)?;
    let n=tx.execute("UPDATE jw_native.source_progress SET source_generation=$3,task_id=$4::text::uuid,binding_id=$5::text::uuid,active_task_id=NULL,confirmed_at=clock_timestamp() WHERE source_id=$1 AND target_database=$2 AND COALESCE(source_generation,0)=$6 AND active_task_id=$4::text::uuid",&[&m.source_id,&database,&(base+1),&task,&binding,&base]).await.map_err(Error::sql)?;
    if n != 1 {
        return Err(Error::new("source_active"));
    }
    tx.commit().await.map_err(Error::sql)?;
    println!(
        "complete=succeeded baseline=S1 binding={binding} pin={pin} task={task} generation={generation} tables={}",
        entries.len()
    );
    Ok(())
}
#[allow(clippy::too_many_arguments)]
fn validate_cdc_baseline_request(
    request: &Value,
    source: &str,
    binding: &str,
    pin: i64,
    base: i64,
    database: &str,
    task: &str,
    generation: i64,
) -> Result<Value> {
    request
        .as_object()
        .ok_or_else(|| Error::new("baseline_request"))?;
    if request.get("protocol").and_then(Value::as_str) != Some("oo.pipeline.native-c1-baseline/v1")
        || request.get("source_id").and_then(Value::as_str) != Some(source)
        || request.get("binding").and_then(Value::as_str) != Some(binding)
        || request.get("pin").and_then(Value::as_i64) != Some(pin)
        || request.get("capture_method").and_then(Value::as_str) != Some("c1_cdc")
        || request.get("source_generation").and_then(Value::as_i64) != Some(base + 1)
        || request.get("target_database").and_then(Value::as_str) != Some(database)
        || request.get("lease_id").is_some()
        || request.get("lease_generation").is_some()
    {
        return Err(Error::new("baseline_request"));
    }
    let scope = request
        .get("baseline_scope")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::new("baseline_scope"))?;
    let scope_keys = [
        "protocol",
        "engine",
        "lineage",
        "l0",
        "l_end",
        "complete",
        "read_fence_digest",
        "config_digest",
        "schema_digest",
        "capture_digest",
        "consistency_grade",
        "cross_table_consistent",
        "source_identity",
        "tables",
    ];
    if scope.len() != scope_keys.len()
        || scope_keys.iter().any(|key| !scope.contains_key(*key))
        || scope.get("protocol").and_then(Value::as_str)
            != Some("oo.source-admission.c1-baseline-scope/v1")
        || !matches!(
            scope.get("consistency_grade").and_then(Value::as_str),
            Some("T" | "S1" | "S0" | "B")
        )
        || scope.get("cross_table_consistent").and_then(Value::as_bool)
            != scope
                .get("consistency_grade")
                .and_then(Value::as_str)
                .map(|grade| grade == "T")
        || !scope
            .get("read_fence_digest")
            .and_then(Value::as_str)
            .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(Error::new("baseline_scope"));
    }
    if scope.get("complete").and_then(Value::as_bool) != Some(true)
        || ["l0", "l_end"].iter().any(|key| {
            scope
                .get(*key)
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
        })
        || ["config_digest", "schema_digest", "capture_digest"]
            .iter()
            .any(|key| {
                !scope
                    .get(*key)
                    .and_then(Value::as_str)
                    .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            })
    {
        return Err(Error::new("baseline_scope"));
    }
    let envelope_path = request
        .get("capture_envelope_path")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("capture_envelope"))?;
    let envelope_bytes =
        std::fs::read(envelope_path).map_err(|_| Error::new("capture_envelope"))?;
    let envelope_sha = sha256_hex(&envelope_bytes);
    if !Path::new(envelope_path).is_absolute()
        || envelope_path.contains("..")
        || envelope_bytes.len() > 16 * 1024 * 1024
        || request
            .get("capture_envelope_sha256")
            .and_then(Value::as_str)
            != Some(envelope_sha.as_str())
    {
        return Err(Error::new("capture_envelope"));
    }
    let mut envelope: Value =
        serde_json::from_slice(&envelope_bytes).map_err(|_| Error::new("capture_envelope"))?;
    let entries = request
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::new("baseline_entries"))?;
    validate_c1_envelope_entries(&envelope, entries)?;
    let mut scope_projection = Value::Object(scope.clone());
    if let Some(map) = scope_projection.as_object_mut() {
        map.remove("capture_digest");
    } else {
        return Err(Error::new("baseline_scope"));
    }
    if envelope.get("baseline_scope") != Some(&scope_projection) {
        return Err(Error::new("capture_envelope"));
    }
    let envelope_digest = scope
        .get("capture_digest")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("capture_envelope"))?;
    if c1_capture_digest(&mut envelope)? != envelope_digest {
        return Err(Error::new("capture_envelope"));
    }
    let identity = request
        .get("source_identity")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::new("source_identity"))?;
    if !matches!(
        identity.get("engine").and_then(Value::as_str),
        Some("mariadb" | "mysql" | "mongodb")
    ) || identity.get("engine") != scope.get("engine")
        || identity.get("measured_id") != scope.get("lineage")
        || request.get("source_identity") != scope.get("source_identity")
        || identity
            .get("measured_id")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        || identity
            .get("shard_id")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        || identity
            .get("owner_token")
            .and_then(Value::as_str)
            .is_none_or(|s| !crate::api::is_uuid(s))
        || identity.contains_key("source_epoch")
        || identity.contains_key("lease_id")
    {
        return Err(Error::new("source_identity"));
    }
    let tables = scope
        .get("tables")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::new("baseline_scope"))?;
    if entries.is_empty() || entries.len() > 32 || tables.len() != entries.len() {
        return Err(Error::new("baseline_entries"));
    }
    let mut prior: Option<(&str, &str)> = None;
    for entry in entries {
        let schema = entry
            .get("schema")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("baseline_entry"))?;
        let table = entry
            .get("table")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("baseline_entry"))?;
        let scope_table = tables
            .iter()
            .find(|item| {
                item.get("schema").and_then(Value::as_str) == Some(schema)
                    && item.get("table").and_then(Value::as_str) == Some(table)
            })
            .ok_or_else(|| Error::new("baseline_entry"))?;
        if prior.is_some_and(|old| old >= (schema, table))
            || entry.get("source_generation").and_then(Value::as_i64) != Some(base + 1)
            || !entry
                .get("schema_sha256")
                .and_then(Value::as_str)
                .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            || entry.get("schema_sha256") != scope_table.get("schema_sha256")
        {
            return Err(Error::new("baseline_entry"));
        }
        let snap = entry
            .get("snapshot_arrow")
            .and_then(Value::as_object)
            .ok_or_else(|| Error::new("baseline_arrow"))?;
        let path = snap
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("baseline_arrow"))?;
        let sha = snap
            .get("sha256")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("baseline_arrow"))?;
        verify_artifact(path, sha)?;
        let registry = entry
            .get("admission_registry_path")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("baseline_registry"))?;
        let registry_sha = entry
            .get("admission_registry_sha256")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("baseline_registry"))?;
        verify_artifact(registry, registry_sha)?;
        let registry_bytes =
            std::fs::read(registry).map_err(|_| Error::new("baseline_registry"))?;
        let registry_value: Value =
            serde_json::from_slice(&registry_bytes).map_err(|_| Error::new("baseline_registry"))?;
        let admitted = registry_value
            .get("bindings")
            .and_then(Value::as_array)
            .filter(|items| items.len() == 1)
            .and_then(|items| items.first())
            .ok_or_else(|| Error::new("baseline_registry"))?;
        let admitted_objects = admitted
            .get("objects")
            .and_then(Value::as_array)
            .filter(|items| items.len() == 1)
            .and_then(|items| items.first())
            .ok_or_else(|| Error::new("baseline_registry"))?;
        if registry_value.get("protocol").and_then(Value::as_str)
            != Some("oo.source-admission.binding-registry/v1")
            || admitted.get("source_id").and_then(Value::as_str) != Some(source)
            || admitted.get("source_engine") != scope.get("engine")
            || admitted.get("config_digest") != scope.get("config_digest")
            || admitted.get("schema_digest") != entry.get("schema_sha256")
            || admitted.get("baseline_scope") != Some(&Value::Object(scope.clone()))
            || admitted_objects
                .get("logical_schema")
                .and_then(Value::as_str)
                != Some(schema)
            || admitted_objects
                .get("logical_table")
                .and_then(Value::as_str)
                != Some(table)
            || admitted_objects.get("row_count").and_then(Value::as_u64)
                != scope_table.get("row_count").and_then(Value::as_u64)
            || admitted_objects.get("arrow_sha256").and_then(Value::as_str) != Some(sha)
            || admitted_objects.get("arrow_sha256").and_then(Value::as_str)
                != scope_table.get("arrow_sha256").and_then(Value::as_str)
        {
            return Err(Error::new("baseline_registry"));
        }
        let txns = entry
            .get("transactions")
            .and_then(Value::as_array)
            .ok_or_else(|| Error::new("baseline_transactions"))?;
        for txn in txns {
            if txn
                .get("position_codec")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
                || txn
                    .get("lineage")
                    .and_then(Value::as_str)
                    .is_none_or(str::is_empty)
                || txn
                    .get("txn_id")
                    .and_then(Value::as_str)
                    .is_none_or(str::is_empty)
                || txn
                    .get("applied_pos")
                    .and_then(Value::as_str)
                    .is_none_or(str::is_empty)
            {
                return Err(Error::new("baseline_transaction"));
            }
            let events = txn
                .get("events")
                .and_then(Value::as_array)
                .ok_or_else(|| Error::new("baseline_events"))?;
            if events.is_empty() {
                return Err(Error::new("baseline_events"));
            }
            for event in events {
                if !matches!(
                    event.get("op").and_then(Value::as_str),
                    Some("insert" | "update" | "delete")
                ) || event.get("primary_key").is_none_or(Value::is_null)
                    || event.get("tombstone").and_then(Value::as_bool).is_none()
                {
                    return Err(Error::new("baseline_event"));
                }
                if let Some(arrow) = event.get("arrow").and_then(Value::as_object) {
                    let p = arrow
                        .get("path")
                        .and_then(Value::as_str)
                        .ok_or_else(|| Error::new("baseline_arrow"))?;
                    let h = arrow
                        .get("sha256")
                        .and_then(Value::as_str)
                        .ok_or_else(|| Error::new("baseline_arrow"))?;
                    verify_artifact(p, h)?;
                } else if event.get("tombstone").and_then(Value::as_bool) != Some(true) {
                    return Err(Error::new("baseline_arrow"));
                }
            }
        }
        prior = Some((schema, table));
    }
    let mut invoked = request.clone();
    crate::capture_source::attach_observed_lease(&mut invoked, task, generation)
        .map_err(|_| Error::new("lease"))?;
    Ok(invoked)
}
fn verify_artifact(path: &str, sha: &str) -> Result<()> {
    if !Path::new(path).is_absolute()
        || path.contains("..")
        || sha.len() != 64
        || !sha.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(Error::new("baseline_artifact"));
    }
    let bytes = std::fs::read(path).map_err(|_| Error::new("baseline_artifact"))?;
    if bytes.len() > 512 * 1024 * 1024 || sha256_hex(&bytes) != sha {
        return Err(Error::new("baseline_artifact"));
    }
    Ok(())
}
// Registry references are checked separately; including them here would make admission self-referential.
fn c1_capture_digest(envelope: &mut Value) -> Result<String> {
    envelope
        .as_object_mut()
        .ok_or_else(|| Error::new("capture_envelope"))?
        .insert("capture_digest".into(), Value::String("0".repeat(64)));
    if let Some(entries) = envelope.get_mut("entries") {
        for entry in entries
            .as_array_mut()
            .ok_or_else(|| Error::new("capture_envelope"))?
        {
            let object = entry
                .as_object_mut()
                .ok_or_else(|| Error::new("capture_envelope"))?;
            object.remove("admission_registry_path");
            object.remove("admission_registry_sha256");
        }
    }
    Ok(sha256_hex(
        &serde_json::to_vec(envelope).map_err(|_| Error::new("capture_envelope"))?,
    ))
}
fn validate_c1_envelope_entries(envelope: &Value, entries: &[Value]) -> Result<()> {
    if envelope.get("protocol").and_then(Value::as_str)
        != Some("oo.source-admission.c1-capture-envelope/v1")
        || envelope.get("entries") != Some(&Value::Array(entries.to_vec()))
    {
        return Err(Error::new("capture_envelope"));
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
async fn apply_cdc_baseline(
    config: &Config,
    c: &mut tokio_postgres::Client,
    project: &str,
    worker: &str,
    task: &str,
    generation: i64,
    binding: &str,
    pin: i64,
    m: &Manifest,
    effective: &Manifest,
    database: &str,
    base: i64,
    request: &Value,
) -> Result<()> {
    let result = exec(
        config,
        &effective.loader,
        "loader_cdc_baseline",
        serde_json::to_vec(request).map_err(|_| Error::new("baseline_request"))?,
        effective.registration.as_ref(),
        None,
        binding,
        pin,
        project,
        task,
        worker,
        generation,
        None,
    )
    .await?;
    let receipt = result
        .get("source_instance")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::new("source_instance"))?;
    let scope = request
        .get("baseline_scope")
        .ok_or_else(|| Error::new("baseline_scope"))?;
    let identity = request
        .get("source_identity")
        .ok_or_else(|| Error::new("source_identity"))?;
    let epoch = receipt
        .get("source_epoch")
        .and_then(Value::as_i64)
        .filter(|v| *v > 0)
        .ok_or_else(|| Error::new("source_epoch"))?;
    if result.get("protocol").and_then(Value::as_str)
        != Some("oo.pipeline.native-c1-baseline-result/v1")
        || result.get("source_id").and_then(Value::as_str) != Some(&m.source_id)
        || result.get("source_generation").and_then(Value::as_i64) != Some(base + 1)
        || result.get("load_state").and_then(Value::as_str) != Some("awaiting_publish")
        || result.get("acknowledged").and_then(Value::as_bool) != Some(false)
        || result.get("l0") != scope.get("l0")
        || result.get("l_end") != scope.get("l_end")
        || receipt.get("protocol").and_then(Value::as_str)
            != Some("oo.pipeline.native-source-instance/v1")
        || receipt.get("source").and_then(Value::as_str) != Some(&m.source_id)
        || receipt.get("engine") != identity.get("engine")
        || receipt.get("measured_id") != identity.get("measured_id")
        || receipt.get("shard_id") != identity.get("shard_id")
        || receipt.get("owner_token") != identity.get("owner_token")
        || receipt.get("config_digest") != scope.get("config_digest")
        || receipt.get("schema_digest") != scope.get("schema_digest")
        || receipt.get("lease_id").and_then(Value::as_str) != Some(task)
        || result.get("source_epoch").and_then(Value::as_i64) != Some(epoch)
    {
        return Err(Error::new("loader_refused"));
    }
    let entries = result
        .get("entries")
        .and_then(Value::as_array)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| Error::new("baseline_receipts"))?;
    let requested = request
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::new("baseline_entries"))?;
    if entries.len() != requested.len()
        || entries.iter().zip(requested).any(|(receipt, entry)| {
            receipt.get("schema").and_then(Value::as_str)
                != entry.get("schema").and_then(Value::as_str)
                || receipt.get("table").and_then(Value::as_str)
                    != entry.get("table").and_then(Value::as_str)
                || receipt
                    .get("physical_id")
                    .and_then(Value::as_str)
                    .is_none_or(str::is_empty)
                || !receipt
                    .get("sha256")
                    .and_then(Value::as_str)
                    .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        })
    {
        return Err(Error::new("baseline_receipts"));
    }
    let target = target_owner(config, database).await?;
    let target_generation = result
        .get("target_generation")
        .and_then(Value::as_i64)
        .filter(|v| *v > 0)
        .ok_or_else(|| Error::new("target_generation"))?;
    let pos = result
        .get("applied_pos")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .ok_or_else(|| Error::new("applied_pos"))?;
    let engine = identity
        .get("engine")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("source_identity"))?;
    let measured = identity
        .get("measured_id")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("source_identity"))?;
    let shard = identity
        .get("shard_id")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("source_identity"))?;
    let owner = identity
        .get("owner_token")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("source_identity"))?;
    let config_digest = scope
        .get("config_digest")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("baseline_scope"))?;
    let schema_digest = scope
        .get("schema_digest")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("baseline_scope"))?;
    let instance_ok:bool=target.query_one("SELECT EXISTS(SELECT 1 FROM _oo.source_instance WHERE source=$1 AND source_epoch::bigint=$2 AND engine=$3 AND measured_id=$4 AND config_digest=$5 AND schema_digest=$6 AND shard_id=$7 AND owner_token=$8::text::uuid AND lease_id=$9::text::uuid)",&[&m.source_id,&epoch,&engine,&measured,&config_digest,&schema_digest,&shard,&owner,&task]).await.map_err(Error::sql)?.get(0);
    if !instance_ok {
        return Err(Error::new("source_instance"));
    }
    let grade = scope
        .get("consistency_grade")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("baseline_scope"))?;
    let cross = scope
        .get("cross_table_consistent")
        .and_then(Value::as_bool)
        .ok_or_else(|| Error::new("baseline_scope"))?;
    if cross != (grade == "T") {
        return Err(Error::new("baseline_scope"));
    }
    let source_state:bool=target.query_one("SELECT EXISTS(SELECT 1 FROM _oo.source_watermark WHERE source=$1 AND source_generation=$2 AND target_generation=$3 AND source_epoch::bigint=$4 AND applied_pos=$5::text::numeric AND consistency_grade=$6 AND cross_table_consistent=$7 AND position_codec IN ('mariadb-gtid/v1','mysql-gtid/v1','mongo-cluster-time/v1'))",&[&m.source_id,&(base+1),&target_generation,&epoch,&pos,&grade,&cross]).await.map_err(Error::sql)?.get(0);
    if !source_state {
        return Err(Error::new("target_generation"));
    }
    let result_text = result.to_string();
    let request_digest=result["request_digest"].as_str().filter(|s|s.len()==64).ok_or_else(||Error::new("baseline_receipts"))?;
    let ack=json!({"protocol":"oo.pipeline.native-c1-baseline-ack/v1","source_id":m.source_id,"task_id":task,"request_digest":request_digest});
    let tx=c.transaction().await.map_err(Error::sql)?;
    tx.execute("INSERT INTO jw_native.pending_acks(task_id,binding_id,pin,generation,ack_request,loader_path,loader_sha256) VALUES($1::text::uuid,$2::text::uuid,$3,$4,$5::text::jsonb,$6,$7)",&[&task,&binding,&pin,&generation,&ack.to_string(),&effective.loader.path,&effective.loader.sha256]).await.map_err(Error::sql)?;
    tx.execute("UPDATE jw_native.occurrences SET result=$2::text::jsonb WHERE task_id=$1::text::uuid",&[&task,&result_text]).await.map_err(Error::sql)?;
    let done=lease::complete_success(&tx,project,task,worker,generation).await?;
    if done.status!=LeaseStatus::Succeeded {return Err(Error::new("lost_lease"));}
    tx.commit().await.map_err(Error::sql)?;
    println!("complete=cas_committed_ack_pending baseline=cdc-first epoch={epoch} tables={} binding={binding} pin={pin} task={task} generation={generation}",entries.len());
    if drain_ack(config,c,project,Some(task)).await? {Ok(())} else {Err(Error::new("ack_pending"))}

}

/// Run the MySQL C1 reader as a leased, bidirectional stream. The reader has
/// source credentials only; each reply follows a committed target stage call.
#[allow(clippy::too_many_arguments)]
async fn baseline_stage_rows(target:&tokio_postgres::Client,source:&str,task:&str)->Result<Value> {
    // Construct each bounded row separately; jsonb_agg duplicates the entire
    // baseline inside one PostgreSQL aggregate allocation.
    let rows=target.query("SELECT jsonb_build_object('schema',logical_schema,'table',logical_table,'key',row_key,'value',row_value,'deleted',deleted,'applied_pos',applied_pos::text)::text FROM _oo.native_c1_baseline_row WHERE source=$1 AND task_id=$2::text::uuid ORDER BY logical_schema,logical_table,row_key",&[&source,&task]).await.map_err(Error::sql)?;
    let values=rows.into_iter().map(|row|serde_json::from_str::<Value>(&row.get::<_,String>(0)).map_err(|_|Error::new("baseline_stage_export"))).collect::<Result<Vec<_>>>()?;
    Ok(Value::Array(values))
}

async fn run_cdc_baseline_stream(
    config: &Config,
    control: &tokio_postgres::Client,
    project: &str,
    worker: &str,
    task: &str,
    generation: i64,
    binding: &str,
    pin: i64,
    source: &str,
    database: &str,
    producer: &Executable,
    loader: &Executable,
    base: i64,
    registration: &Registration,
) -> Result<Value> {
    executable_check(producer)?;
    if producer.args.len() != 5 || producer.args[0] != "c1-baseline-stream"
        || producer.args[1] != "--config" || producer.args[3] != "--out" {
        return Err(Error::new("baseline_producer"));
    }
    let config_path = &producer.args[2];
    if !Path::new(config_path).is_absolute() || config_path.contains("..") {
        return Err(Error::new("baseline_config"));
    }
    let config_bytes = std::fs::read(config_path).map_err(|_| Error::new("baseline_config"))?;
    let configured_out = producer.args[4]
        .replace("{task}", task)
        .replace("{generation}", &generation.to_string());
    if !Path::new(&configured_out).is_absolute() || configured_out.contains("..") {
        return Err(Error::new("baseline_out"));
    }

    let admin = session::admin_db(config, database).await?;
    let owner:String=admin.query_opt("SELECT owner_token::text FROM _oo.full_import_run WHERE source_id=$1 ORDER BY target_generation DESC LIMIT 1",&[&source])
  .await.map_err(Error::sql)?.map(|row|row.get(0)).unwrap_or_else(||binding.to_owned());
    if !crate::api::is_uuid(&owner) {
        return Err(Error::new("baseline_owner"));
    }
    let target = target_owner(config, database).await?;
    let ca_file = config
        .ca_file
        .as_ref()
        .ok_or_else(|| Error::new("baseline_target"))?;
    let target_desc = json!({
     "host":config.host,"port":config.port,"database":database,
     "runtime_principal":config.runtime_user,"password_file":config.runtime_password_file,
     "ca_file":ca_file,"shard":source,"owner":owner
    });

    let previous=if base>0 {
        let row=admin.query_opt("SELECT jsonb_build_object('task_id',s.task_id,'l_end',substring(s.measured_id from 7)||':1-'||_oo.native_pos_text(s.l_end),'measured_id',s.measured_id,'config_digest',s.config_digest,'schema_digest',s.schema_digest,'source_epoch',s.source_epoch,'source_generation',$2::bigint)::text FROM _oo.native_c1_baseline_run s JOIN _oo.native_c1_baseline_typed t USING(source,task_id) JOIN _oo.source_watermark w ON w.source=s.source JOIN jobs.tasks j ON j.id=s.task_id WHERE s.source=$1 AND (t.receipt->>'source_generation')::bigint=$2 AND t.acknowledged_at IS NOT NULL AND j.state='succeeded' AND w.source_generation=$2 AND w.applied_pos=s.l_end AND w.source_epoch=s.source_epoch",&[&source,&base]).await.map_err(Error::sql)?.ok_or_else(||Error::new("unconfirmed_baseline"))?;
        serde_json::from_str::<Value>(&row.get::<_,String>(0)).map_err(|_|Error::new("baseline_previous"))?
    } else {Value::Null};
    let resume_raw:Option<String>=target.query_one("SELECT _oo.native_c1_baseline_rebind($1,$2::text::uuid,$3)::text",&[&source,&task,&generation]).await.map_err(Error::sql)?.get(0);
    let mut resume:Value=resume_raw.as_deref().map(serde_json::from_str).transpose().map_err(|_|Error::new("baseline_resume"))?.unwrap_or(Value::Null);
    if resume["state"]=="awaiting_publish" {
        resume["rows"]=baseline_stage_rows(&target,source,task).await?;
    }
    let mut command = Command::new(&producer.path);
    let args = producer
        .args
        .iter()
        .map(|arg| {
            arg.replace("{task}", task)
                .replace("{generation}", &generation.to_string())
        })
        .collect::<Vec<_>>();
    command
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|_| Error::new("baseline_spawn"))?;
    let mut child_in = child
        .stdin
        .take()
        .ok_or_else(|| Error::new("baseline_spawn"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::new("baseline_spawn"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Error::new("baseline_spawn"))?;
    let (tx, mut rx) = tokio::sync::mpsc::channel::<std::io::Result<Vec<u8>>>(2);
    let stdout_reader = tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = Vec::new();
            let n = reader.read_until(b'\n', &mut line).await?;
            if n == 0 {
                break;
            }
            if tx.send(Ok(line)).await.is_err() {
                break;
            }
        }
        Ok::<(), std::io::Error>(())
    });
    let stderr_reader = tokio::spawn(async move {
        let mut r = stderr.take(65537);
        let mut bytes = Vec::new();
        r.read_to_end(&mut bytes).await.map(|_| bytes)
    });
    executable_check(&Executable { path: registration.admission_binary.clone(), sha256: registration.admission_sha256.clone(), args: vec![] })?;
    let bootstrap = json!({"protocol":"oo.jobs.mysql-c1-bootstrap/v1","source_id":source,"binding":binding,"pin":pin,
        "source_generation":base+1,"target":target_desc,"resume":resume,"previous":previous,"admission_binary":registration.admission_binary,
        "admission_sha256":registration.admission_sha256});
    child_in.write_all(&serde_json::to_vec(&bootstrap).map_err(|_| Error::new("baseline_request"))?).await.map_err(|_| Error::new("baseline_reply"))?;
    child_in.write_all(b"\n").await.map_err(|_| Error::new("baseline_reply"))?;
    child_in.flush().await.map_err(|_| Error::new("baseline_reply"))?;
    let mut quiet = None;
    let mut materialized = None;
    let mut raw = Vec::new();
    let mut frame_count = 0u64;
    let mut source_instance = None;
    let mut epoch = None;
    let mut sealed = None;
    let mut schema_tables = std::collections::BTreeSet::new();
    let mut opened = false;
    let mut stream_started = false;
    let mut snap_started = false;
    let mut last_pos = 0u64;
    let stage = Executable {
        path: loader.path.clone(),
        sha256: loader.sha256.clone(),
        args: vec!["native-c1-baseline-stage".into()],
    };
    let mut heartbeat=tokio::time::interval_at(tokio::time::Instant::now()+Duration::from_secs(10),Duration::from_secs(10));
    let mut raw_file:Option<std::fs::File>=None;
    loop {
        let line = tokio::select! {
         item=rx.recv()=>match item {Some(Ok(bytes))=>bytes,Some(Err(_))=>return Err(Error::new("baseline_stdout")),None=>break},
         _=heartbeat.tick()=>{
          if lease::heartbeat(control,project,task,worker,generation).await?.status!=LeaseStatus::Extended {let _=child.kill().await;return Err(Error::new("lost_lease"))}
          if lease::heartbeat(&target,project,task,worker,generation).await?.status!=LeaseStatus::Extended {let _=child.kill().await;return Err(Error::new("target_lease"))}
          continue
         }
        };
        if raw.len().saturating_add(line.len()) > 256 * 1024 * 1024 {
            let _ = child.kill().await;
            return Err(Error::new("baseline_output_budget"));
        }
        if raw_file.is_none() {
            use std::os::unix::fs::OpenOptionsExt;
            raw_file=Some(std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600)
                .open(Path::new(&configured_out).join("worker-stream.ndjson")).map_err(|_|Error::new("baseline_raw_evidence"))?);
        }
        std::io::Write::write_all(raw_file.as_mut().ok_or_else(||Error::new("baseline_raw_evidence"))?,&line)
            .map_err(|_|Error::new("baseline_raw_evidence"))?;
        raw.extend_from_slice(&line);
        frame_count += 1;
        let frame: Value =
            serde_json::from_slice(&line).map_err(|_| Error::new("baseline_frame"))?;
        let kind = frame
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("baseline_frame"))?;
        let reply = match kind {
            "quiet" => {
                let request=frame.get("request").ok_or_else(||Error::new("baseline_quiet"))?;
                if frame_count!=1 || previous.is_null() || !resume.is_null()
                    || request["protocol"]!="oo.jobs.mysql-c1-nochange/v1" || request["source_id"]!=source
                    || request["source_generation"]!=base || request["previous"]!=previous
                    || request["observed_gtid"]!=previous["l_end"]
                    || request["connection_config_sha256"]!=sha256_hex(&config_bytes) {return Err(Error::new("baseline_quiet"));}
                quiet=Some(request.clone());break;
            }
            "open" => {
                if opened
                    || frame.get("connection_config_sha256").and_then(Value::as_str)
                        != Some(sha256_hex(&config_bytes).as_str())
                {
                    return Err(Error::new("baseline_open"));
                }
                let uuid = frame
                    .get("source_uuid")
                    .and_then(Value::as_str)
                    .filter(|s| crate::api::is_uuid(s))
                    .ok_or_else(|| Error::new("baseline_identity"))?;
                let measured = format!("mysql.{uuid}");
                let config_digest = frame
                    .get("config_digest")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::new("baseline_identity"))?;
                let schema_digest = frame
                    .get("schema_digest")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::new("baseline_identity"))?;
                let admitted_config=frame.get("admission_config").ok_or_else(|| Error::new("baseline_identity"))?;
                if admitted_config["protocol"] != "oo.source-admission.config/v1" || admitted_config["source_id"] != source
                    || sha256_hex(&serde_json::to_vec(admitted_config).map_err(|_| Error::new("baseline_identity"))?) != config_digest {
                    return Err(Error::new("baseline_identity"));
                }
                if config_digest.len() != 64
                    || schema_digest.len() != 64
                    || !config_digest.bytes().all(|b| b.is_ascii_hexdigit())
                    || !schema_digest.bytes().all(|b| b.is_ascii_hexdigit())
                {
                    return Err(Error::new("baseline_identity"));
                }
                let tables = frame
                    .get("table_schemas")
                    .and_then(Value::as_array)
                    .filter(|a| !a.is_empty() && a.len() <= 32)
                    .ok_or_else(|| Error::new("baseline_catalog"))?;
                let expected = sha256_hex(
                    &serde_json::to_vec(frame.get("schema_preimage").ok_or_else(|| Error::new("baseline_catalog"))?).map_err(|_| Error::new("baseline_catalog"))?,
                );
                if expected != schema_digest {
                    return Err(Error::new("baseline_catalog"));
                }
                for table in tables {
                    let schema = table
                        .get("schema")
                        .and_then(Value::as_str)
                        .ok_or_else(|| Error::new("baseline_catalog"))?;
                    let name = table
                        .get("table")
                        .and_then(Value::as_str)
                        .ok_or_else(|| Error::new("baseline_catalog"))?;
                    let columns = table
                        .get("columns")
                        .and_then(Value::as_array)
                        .filter(|a| !a.is_empty() && a.len() <= 256)
                        .ok_or_else(|| Error::new("baseline_catalog"))?;
                    let key = (schema.to_owned(), name.to_owned());
                    if !schema_tables.insert(format!("{}.{}", key.0, key.1)) {
                        return Err(Error::new("baseline_catalog"));
                    }
                    if columns.iter().any(|c| {
                        c.get("ordinal")
                            .and_then(Value::as_u64)
                            .is_none_or(|n| n == 0)
                            || c.get("name")
                                .and_then(Value::as_str)
                                .is_none_or(str::is_empty)
                            || c.get("data_type")
                                .and_then(Value::as_str)
                                .is_none_or(str::is_empty)
                    }) {
                        return Err(Error::new("baseline_catalog"));
                    }
                }
                let l0 = frame
                    .get("l0")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| Error::new("baseline_l0"))?;
                let l0_pos = frame
                    .get("l0_pos")
                    .and_then(Value::as_u64)
                    .filter(|p| *p > 0)
                    .ok_or_else(|| Error::new("baseline_l0"))?;
                if l0 != format!("{uuid}:1-{l0_pos}") {
                    return Err(Error::new("baseline_l0"));
                }
                if !resume.is_null() && (resume["l0"]!=l0 || resume["schema_digest"]!=schema_digest || resume["config_digest"]!=config_digest) {
                    return Err(Error::new("baseline_resume_identity"));
                }
                let source_instance_text:String=target.query_one("SELECT _oo.native_source_instance_register($1,$2::text::uuid,$3,$4,$5,$6,$7,$8,$9::text::uuid)::text",
     &[&source,&owner,&generation,&source,&"mysql",&measured,&config_digest,&schema_digest,&task]).await.map_err(Error::sql)?.get(0);
                let instance: Value = serde_json::from_str(&source_instance_text)
                    .map_err(|_| Error::new("source_instance"))?;
                let source_epoch = instance
                    .get("source_epoch")
                    .and_then(Value::as_i64)
                    .filter(|e| *e > 0)
                    .ok_or_else(|| Error::new("source_epoch"))?;
                let mut request = json!({"protocol":"oo.pipeline.native-c1-baseline-stage/v1","operation":"open","source_id":source,
     "lease_id":task,"lease_generation":generation,"source_epoch":source_epoch,"measured_id":measured,
     "config_digest":config_digest,"schema_digest":schema_digest,"l0":l0,"l0_pos":l0_pos.to_string(),"target":target_desc});
                if !previous.is_null() {
                    if previous["l_end"]!=l0 || previous["source_epoch"]!=source_epoch
                        || previous["config_digest"]!=config_digest || previous["schema_digest"]!=schema_digest
                        || previous["measured_id"]!=measured {return Err(Error::new("baseline_previous"));}
                    request["operation"]=json!("ack_seed");request["previous_task"]=previous["task_id"].clone();
                    request["previous_source_generation"]=json!(base);
                }
                let opened_receipt = exec(
                    config,
                    &stage,
                    "loader_c1_stage_open",
                    serde_json::to_vec(&request).map_err(|_| Error::new("baseline_request"))?,
                    None,
                    None,
                    binding,
                    pin,
                    project,
                    task,
                    worker,
                    generation,
                    Some(&target),
                )
                .await?;
                if !matches!(opened_receipt.get("outcome").and_then(Value::as_str),Some("open"|"replay")) {
                    return Err(Error::new("baseline_target_open"));
                }
                epoch = Some(source_epoch);
                source_instance = Some(instance);
                last_pos = l0_pos;
                opened = true;
                if !previous.is_null() {snap_started=true;}
                if resume["state"]=="awaiting_publish" {sealed=Some(resume.clone());stream_started=true;snap_started=true;}
                json!({"protocol":"oo.jobs.mysql-c1-baseline-ack/v1","stage":"stage_open","durable":true,"source_epoch":source_epoch,
                    "sealed":if resume["state"]=="awaiting_publish" {resume.clone()} else {Value::Null}})
            }
            "stream_started" => {
                if !opened || stream_started || frame.get("l0").and_then(Value::as_str).is_none() {
                    return Err(Error::new("baseline_stream"));
                }
                stream_started = true;
                json!({"protocol":"oo.jobs.mysql-c1-baseline-ack/v1","stage":"stream_started","durable":true})
            }
            "snapshot" => {
                if !stream_started {
                    return Err(Error::new("baseline_snapshot_order"));
                }
                snap_started = true;
                let rows = frame
                    .get("rows")
                    .and_then(Value::as_array)
                    .ok_or_else(|| Error::new("baseline_snapshot"))?;
                for row in rows {
                    let table = format!(
                        "{}.{}",
                        row.get("schema").and_then(Value::as_str).unwrap_or(""),
                        row.get("table").and_then(Value::as_str).unwrap_or("")
                    );
                    if !schema_tables.contains(&table) {
                        return Err(Error::new("baseline_snapshot"));
                    }
                }
                let request = json!({"protocol":"oo.pipeline.native-c1-baseline-stage/v1","operation":"snapshot_fill","source_id":source,
     "lease_id":task,"lease_generation":generation,"source_epoch":epoch.ok_or_else(||Error::new("source_epoch"))?,"rows":rows,"target":target_desc});
                let receipt = exec(
                    config,
                    &stage,
                    "loader_c1_stage_snapshot",
                    serde_json::to_vec(&request).map_err(|_| Error::new("baseline_request"))?,
                    None,
                    None,
                    binding,
                    pin,
                    project,
                    task,
                    worker,
                    generation,
                    Some(&target),
                )
                .await?;
                if receipt.get("outcome").and_then(Value::as_str) != Some("filled") {
                    return Err(Error::new("baseline_snapshot_fill"));
                }
                json!({"protocol":"oo.jobs.mysql-c1-baseline-ack/v1","stage":"snapshot_fill","durable":true})
            }
            "transaction" => {
                if !stream_started {
                    return Err(Error::new("baseline_transaction_order"));
                }
                let pos = frame
                    .get("applied_pos")
                    .and_then(Value::as_u64)
                    .filter(|p| *p > last_pos)
                    .ok_or_else(|| Error::new("baseline_position"))?;
                let txn = frame
                    .get("txn_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::new("baseline_transaction"))?;
                let rows = frame
                    .get("rows")
                    .and_then(Value::as_array)
                    .ok_or_else(|| Error::new("baseline_transaction"))?;
                for row in rows {
                    let table = format!(
                        "{}.{}",
                        row.get("schema").and_then(Value::as_str).unwrap_or(""),
                        row.get("table").and_then(Value::as_str).unwrap_or("")
                    );
                    if !schema_tables.contains(&table) {
                        return Err(Error::new("baseline_transaction"));
                    }
                }
                let payload = sha256_hex(
                    &serde_json::to_vec(rows).map_err(|_| Error::new("baseline_transaction"))?,
                );
                let request = json!({"protocol":"oo.pipeline.native-c1-baseline-stage/v1","operation":"transaction","source_id":source,
     "lease_id":task,"lease_generation":generation,"source_epoch":epoch.ok_or_else(||Error::new("source_epoch"))?,"applied_pos":pos.to_string(),
     "txn_id":txn,"rows":rows,"payload_sha256":payload,"source_commit_timestamp":frame.get("source_commit_timestamp"),"target":target_desc});
                let receipt = exec(
                    config,
                    &stage,
                    "loader_c1_stage_transaction",
                    serde_json::to_vec(&request).map_err(|_| Error::new("baseline_request"))?,
                    None,
                    None,
                    binding,
                    pin,
                    project,
                    task,
                    worker,
                    generation,
                    Some(&target),
                )
                .await?;
                if !matches!(
                    receipt.get("outcome").and_then(Value::as_str),
                    Some("applied" | "replay")
                ) || receipt.get("applied_pos").and_then(Value::as_i64) != Some(pos as i64)
                {
                    return Err(Error::new("baseline_transaction_apply"));
                }
                last_pos = pos;
                json!({"protocol":"oo.jobs.mysql-c1-baseline-ack/v1","stage":"transaction","durable":true})
            }
            "end" => {
                if !opened || !stream_started || !snap_started {
                    return Err(Error::new("baseline_end_order"));
                }
                let l_end = frame
                    .get("l_end")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::new("baseline_l_end"))?;
                let l_end_pos = frame
                    .get("l_end_pos")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| Error::new("baseline_l_end"))?;
                if l_end_pos != last_pos {
                    return Err(Error::new("baseline_l_end"));
                }
                let request = json!({"protocol":"oo.pipeline.native-c1-baseline-stage/v1","operation":"seal","source_id":source,
     "lease_id":task,"lease_generation":generation,"source_epoch":epoch.ok_or_else(||Error::new("source_epoch"))?,
     "l_end":l_end,"l_end_pos":l_end_pos.to_string(),"target":target_desc});
                let receipt = exec(
                    config,
                    &stage,
                    "loader_c1_stage_seal",
                    serde_json::to_vec(&request).map_err(|_| Error::new("baseline_request"))?,
                    None,
                    None,
                    binding,
                    pin,
                    project,
                    task,
                    worker,
                    generation,
                    Some(&target),
                )
                .await?;
                if receipt.get("outcome").and_then(Value::as_str) != Some("sealed") {
                    return Err(Error::new("baseline_seal"));
                }
                sealed = Some(receipt.clone());
                let rows=baseline_stage_rows(&target,source,task).await?;
                json!({"protocol":"oo.jobs.mysql-c1-baseline-ack/v1","stage":"end","durable":true,"rows":rows})
            }
            "materialized" => {
                if sealed.is_none() || materialized.is_some() { return Err(Error::new("baseline_materialization")); }
                materialized = Some(frame.get("request").cloned().ok_or_else(|| Error::new("baseline_materialization"))?);
                break;
            }
            _ => return Err(Error::new("baseline_frame_kind")),
        };
        child_in
            .write_all(&serde_json::to_vec(&reply).map_err(|_| Error::new("baseline_reply"))?)
            .await
            .map_err(|_| Error::new("baseline_reply"))?;
        child_in
            .write_all(b"\n")
            .await
            .map_err(|_| Error::new("baseline_reply"))?;
        child_in
            .flush()
            .await
            .map_err(|_| Error::new("baseline_reply"))?;
    }
    drop(child_in);
    let status = child
        .wait()
        .await
        .map_err(|_| Error::new("baseline_wait"))?;
    stdout_reader
        .await
        .map_err(|_| Error::new("baseline_stdout"))?
        .map_err(|_| Error::new("baseline_stdout"))?;
    let stderr = stderr_reader
        .await
        .map_err(|_| Error::new("baseline_stderr"))?
        .map_err(|_| Error::new("baseline_stderr"))?;
    let code = status.code().unwrap_or(-1);
    control.execute("INSERT INTO jw_native.process_attempts(task_id,generation,stage,executable_sha256,exit_code,stdout,stderr) VALUES($1::text::uuid,$2,'c1_baseline_stream',$3,$4,$5,$6) ON CONFLICT DO NOTHING",&[&task,&generation,&producer.sha256,&code,&raw,&stderr]).await.map_err(Error::sql)?;
    println!(
        "process_stage=c1_baseline_stream task={task} generation={generation} exit_code={code} frames={frame_count} stdout_sha256={} raw_receipt=jw_native.process_attempts",
        sha256_hex(&raw)
    );
    if let Some(request)=quiet {
        if !status.success() {return Err(Error::new("baseline_process"));}
        return Ok(json!({"request":request,"frames":frame_count,"stdout_sha256":sha256_hex(&raw)}));
    }
    if !status.success() || !opened || !stream_started || !snap_started || sealed.is_none() {
        return Err(Error::new("baseline_process"));
    }
    Ok(
        json!({"protocol":"oo.pipeline.native-c1-baseline-stream-result/v1","source_instance":source_instance.ok_or_else(||Error::new("source_instance"))?,"sealed":sealed.ok_or_else(||Error::new("baseline_seal"))?,"request":materialized.ok_or_else(||Error::new("baseline_materialization"))?,"frames":frame_count,"stdout_sha256":sha256_hex(&raw),"stderr_sha256":sha256_hex(&stderr)}),
    )
}
async fn mirror_claim(
    config: &Config,
    control: &tokio_postgres::Client,
    project: &str,
    task: &str,
    worker: &str,
    generation: i64,
    loader: &Executable,
) -> Result<tokio_postgres::Client> {
    let row=control.query_opt("SELECT row_to_json(t)::text FROM jobs.tasks t WHERE id=$1::text::uuid AND project_id=$2::text::uuid AND state='running' AND lease_owner=$3 AND generation=$4 AND lease_until>clock_timestamp()",&[&task,&project,&worker,&generation]).await.map_err(Error::sql)?.ok_or_else(||Error::new("lost_lease"))?;
    let task_json: String = row.get(0);
    let expected: Value =
        serde_json::from_str(&task_json).map_err(|_| Error::new("target_lease"))?;
    let attempt = expected
        .get("attempt")
        .and_then(Value::as_i64)
        .ok_or_else(|| Error::new("target_lease"))?;
    let target = session::target_runtime(config).await?;
    let markers=target.query("SELECT version,checksum,filename FROM jobs.schema_migrations WHERE version IN ('jobs-mirror-rebind/1','jobs-mirror-retry-run-at/1','jobs-mirror-sync/1','jobs-mirror-read/1')",&[]).await.map_err(Error::sql)?;
    if markers.len() != 4
        || !markers.iter().any(|r| {
            r.get::<_, String>(0) == "jobs-mirror-rebind/1"
                && r.get::<_, String>(1)
                    == "1b4b7ddf2c3539913764ea4f680d3e1740b6be9fb562c104ac165dae2d20ad37"
                && r.get::<_, String>(2) == "0005_mirror_rebind_expand.sql"
        })
        || !markers.iter().any(|r| {
            r.get::<_, String>(0) == "jobs-mirror-retry-run-at/1"
                && r.get::<_, String>(1)
                    == "1b4a31486956019305f9f563798141e4764156db87aaad4b6b3f5fadb8fdb6dc"
                && r.get::<_, String>(2) == "0006_mirror_retry_run_at_expand.sql"
        })
        || !markers.iter().any(|r| {
            r.get::<_, String>(0) == "jobs-mirror-sync/1"
                && r.get::<_, String>(1)
                    == "d391d7c30fc086afeac8d95edc967828eb3e1d4fbe8b67eff295f76945bbd368"
                && r.get::<_, String>(2) == "0007_verified_mirror_sync_expand.sql"
        })
        || !markers.iter().any(|r| {
            r.get::<_, String>(0) == "jobs-mirror-read/1"
                && r.get::<_, String>(1)
                    == "e4ff243fabaa3bf7ba9947026db7b0af62a4a036b79002b8e4075cea3257442c"
                && r.get::<_, String>(2) == "0008_native_mirror_read_grants_expand.sql"
        })
    {
        return Err(Error::new("mirror_install"));
    }
    for second in 0..40 {
        let old=target.query_opt("SELECT state,generation,attempt,lease_until<=clock_timestamp() FROM jobs.tasks WHERE id=$1::text::uuid",&[&task]).await.map_err(Error::sql)?;
        let Some(old) = old else { break };
        let old_state: String = old.get(0);
        let old_gen: i64 = old.get(1);
        let old_attempt: i32 = old.get(2);
        let expired: Option<bool> = old.get(3);
        if old_gen == generation
            || old_state == "running"
                && old_gen < generation
                && i64::from(old_attempt) < attempt
                && expired == Some(true)
        {
            break;
        }
        if old_state != "running" || old_gen >= generation || i64::from(old_attempt) >= attempt {
            return Err(Error::new("target_lease"));
        }
        if second % 8 == 0
            && lease::heartbeat(control, project, task, worker, generation)
                .await?
                .status
                != LeaseStatus::Extended
        {
            return Err(Error::new("lost_lease"));
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    sync_mirror(control, loader, project, task, worker, generation).await?;
    let matched:bool=target.query_one("SELECT EXISTS(SELECT 1 FROM jobs.tasks t WHERE id=$1::text::uuid AND state='running' AND lease_until>clock_timestamp() AND to_jsonb(t)-'updated_at'-'lease_until'=$2::text::jsonb-'updated_at'-'lease_until')",&[&task,&task_json]).await.map_err(Error::sql)?.get(0);
    if !matched {
        return Err(Error::new("target_lease"));
    }
    let beat = lease::heartbeat(&target, project, task, worker, generation).await?;
    if beat.status != LeaseStatus::Extended {
        return Err(Error::new("target_lease"));
    }
    println!("target_claim=mirrored task={task} generation={generation}");
    Ok(target)
}
async fn sync_mirror(
    control: &tokio_postgres::Client,
    loader: &Executable,
    project: &str,
    task: &str,
    worker: &str,
    generation: i64,
) -> Result<()> {
    executable_check(loader)?;
    let body = json!({"protocol":"oo.pipeline.native-task-mirror/v1","job_task_id":task});
    let mut command = Command::new(&loader.path);
    command
        .arg("sync-native-task-mirror")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for (path_var, sha_var) in [
        (
            "OO_NATIVE_CONTROL_TARGET",
            "OO_NATIVE_CONTROL_TARGET_SHA256",
        ),
        ("OO_NATIVE_MIRROR_TARGET", "OO_NATIVE_MIRROR_TARGET_SHA256"),
    ] {
        let path = std::env::var(path_var).map_err(|_| Error::new("mirror_target"))?;
        let digest = std::env::var(sha_var).map_err(|_| Error::new("mirror_target"))?;
        if !Path::new(&path).is_absolute()
            || path.contains("..")
            || digest.len() != 64
            || !digest.bytes().all(|b| b.is_ascii_hexdigit())
            || sha256_hex(&std::fs::read(&path).map_err(|_| Error::new("mirror_target"))?) != digest
        {
            return Err(Error::new("mirror_target"));
        }
        command.env(path_var, path).env(sha_var, digest);
    }
    let mut child = command.spawn().map_err(|_| Error::new("spawn"))?;
    let mut stdin = child.stdin.take().ok_or_else(|| Error::new("spawn"))?;
    stdin
        .write_all(body.to_string().as_bytes())
        .await
        .map_err(|_| Error::new("spawn"))?;
    stdin.shutdown().await.map_err(|_| Error::new("spawn"))?;
    drop(stdin);
    if lease::heartbeat(control, project, task, worker, generation)
        .await?
        .status
        != LeaseStatus::Extended
    {
        return Err(Error::new("lost_lease"));
    }
    let output = tokio::time::timeout(Duration::from_secs(20), child.wait_with_output())
        .await
        .map_err(|_| Error::new("mirror_timeout"))?
        .map_err(|_| Error::new("spawn"))?;
    let response: Value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    println!(
        "process_stage=mirror_sync task={task} generation={generation} exit_code={} stdout_bytes={} stderr_bytes={}",
        output.status.code().unwrap_or(-1),
        output.stdout.len(),
        output.stderr.len()
    );
    if !output.status.success()
        || response.get("protocol").and_then(Value::as_str)
            != Some("oo.pipeline.native-task-mirror-result/v1")
        || response.get("mirrored").and_then(Value::as_bool) != Some(true)
        || response.get("job_generation").and_then(Value::as_i64) != Some(generation)
    {
        return Err(Error::new("mirror_refused"));
    }
    Ok(())
}
fn ack_request(result: &Value, task: &str, generation: i64) -> Result<Value> {
    let receipt = result
        .get("partition_target_receipt")
        .ok_or_else(|| Error::new("ack_receipt"))?;
    let scope = receipt
        .get("partition_scope")
        .ok_or_else(|| Error::new("ack_receipt"))?;
    let string = |value: Option<&Value>| {
        value
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| Error::new("ack_receipt"))
    };
    if receipt.get("acknowledged").and_then(Value::as_bool) != Some(false)
        || receipt.get("job_task_id").and_then(Value::as_str) != Some(task)
        || receipt.get("job_generation").and_then(Value::as_i64) != Some(generation)
        || scope.get("source_id").and_then(Value::as_str)
            != result.get("source_id").and_then(Value::as_str)
        || scope.get("logical_schema") != result.get("logical_schema")
        || scope.get("logical_table") != result.get("logical_table")
        || receipt.get("payload_digest") != result.get("payload_digest")
        || result
            .pointer("/jobs_lease_receipt/state")
            .and_then(Value::as_str)
            != Some("running")
    {
        return Err(Error::new("ack_receipt"));
    }
    Ok(json!({"protocol":"oo.pipeline.native-partition-ack/v1",
  "source_id":string(result.get("source_id"))?,
  "logical_schema":string(scope.get("logical_schema"))?,
  "logical_table":string(scope.get("logical_table"))?,
  "partition_id":string(scope.get("partition_id"))?,
  "run_id":string(result.get("run_id"))?,
  "batch_id":string(receipt.get("batch_id"))?,
  "payload_digest":string(receipt.get("payload_digest"))?,
  "job_task_id":task,"job_generation":generation}))
}
async fn drain_ack(
    config: &Config,
    c: &mut tokio_postgres::Client,
    project: &str,
    only_task: Option<&str>,
) -> Result<bool> {
    let tx = c.transaction().await.map_err(Error::sql)?;
    let row=tx.query_opt("SELECT a.task_id::text,a.binding_id::text,a.pin,a.generation,a.ack_request::text,a.loader_path,a.loader_sha256,o.request::text,o.result::text,t.state,t.generation,b.source_id,b.target_database FROM jw_native.pending_acks a JOIN jw_native.bindings b ON b.id=a.binding_id JOIN jw_native.occurrences o ON o.task_id=a.task_id JOIN jobs.tasks t ON t.id=a.task_id WHERE b.project_id=$1::text::uuid AND a.acknowledged_at IS NULL AND ($2::text IS NULL OR a.task_id=$2::text::uuid) ORDER BY o.scheduled_at FOR UPDATE OF a SKIP LOCKED LIMIT 1",&[&project,&only_task]).await.map_err(Error::sql)?;
    let Some(row) = row else { return Ok(false) };
    let task: String = row.get(0);
    let binding: String = row.get(1);
    let pin: i64 = row.get(2);
    let generation: i64 = row.get(3);
    let ack_text: String = row.get(4);
    let path: String = row.get(5);
    let sha: String = row.get(6);
    let request_text: String = row.get(7);
    let result_text: String = row.get(8);
    let state: String = row.get(9);
    let current_generation: i64 = row.get(10);
    let source: String = row.get(11);
    let database: String = row
        .get::<_, Option<String>>(12)
        .unwrap_or_else(|| config.target_database.clone().unwrap_or_default());
    let ack:Value=serde_json::from_str(&ack_text).map_err(|_|Error::new("ack_receipt"))?;
    if ack["protocol"]=="oo.pipeline.native-c1-baseline-ack/v1" {
        if database!=target_database(config,"C1")? || state!="succeeded" || current_generation!=generation {return Err(Error::new("jobs_cas_absent"));}
        let request:Value=serde_json::from_str(&request_text).map_err(|_|Error::new("request"))?;
        let result:Value=serde_json::from_str(&result_text).map_err(|_|Error::new("ack_receipt"))?;
        let expected=json!({"protocol":"oo.pipeline.native-c1-baseline-ack/v1","source_id":source,"task_id":task,"request_digest":result["request_digest"]});
        if ack!=expected || result["protocol"]!="oo.pipeline.native-c1-baseline-result/v1" || result["source_id"]!=source
            || request["protocol"]!="oo.pipeline.native-c1-baseline/v1" {return Err(Error::new("ack_receipt"));}
        executable_check(&Executable{path,sha256:sha,args:vec![]})?;
        let digest=ack["request_digest"].as_str().ok_or_else(||Error::new("ack_receipt"))?;
        let target=target_owner(config,&database).await?;
        let raw:String=target.query_one("SELECT _oo.native_c1_baseline_ack($1,$2::text::uuid,$3)::text",&[&source,&task,&digest]).await.map_err(Error::sql)?.get(0);
        let response:Value=serde_json::from_str(&raw).map_err(|_|Error::new("ack_receipt"))?;
        if response["acknowledged"]!=true {return Err(Error::new("ack_refused"));}
        let source_generation=result["source_generation"].as_i64().ok_or_else(||Error::new("position"))?;
        let pos=result["applied_pos"].as_str().ok_or_else(||Error::new("position"))?;
        tx.execute("INSERT INTO jw_native.watermarks(binding_id,pin,task_id,generation,source_generation,applied_pos,load_state,result) VALUES($1::text::uuid,$2,$3::text::uuid,$4,$5,$6::text::numeric,'awaiting_publish',$7::text::jsonb) ON CONFLICT(binding_id) DO UPDATE SET pin=EXCLUDED.pin,task_id=EXCLUDED.task_id,generation=EXCLUDED.generation,source_generation=EXCLUDED.source_generation,applied_pos=EXCLUDED.applied_pos,load_state=EXCLUDED.load_state,result=EXCLUDED.result,advanced_at=clock_timestamp() WHERE jw_native.watermarks.pin<EXCLUDED.pin",&[&binding,&pin,&task,&generation,&source_generation,&pos,&result_text]).await.map_err(Error::sql)?;
        tx.execute("UPDATE jw_native.pending_acks SET acknowledged_at=clock_timestamp() WHERE task_id=$1::text::uuid",&[&task]).await.map_err(Error::sql)?;
        let n=tx.execute("UPDATE jw_native.source_progress SET source_generation=$3,task_id=$4::text::uuid,binding_id=$5::text::uuid,active_task_id=NULL,confirmed_at=clock_timestamp() WHERE source_id=$1 AND target_database=$2 AND COALESCE(source_generation,0)=$6 AND active_task_id=$4::text::uuid",&[&source,&database,&source_generation,&task,&binding,&(source_generation-1)]).await.map_err(Error::sql)?;
        if n!=1 {return Err(Error::new("source_active"));}
        tx.commit().await.map_err(Error::sql)?;
        println!("complete=acknowledged baseline=cdc-first binding={binding} pin={pin} task={task} generation={generation}");
        return Ok(true);
    }
    if database != target_database(config, "C4")? {
        return Err(Error::new("target_database"));
    }
    if state != "succeeded" || current_generation != generation {
        return Err(Error::new("jobs_cas_absent"));
    }
    let request: Value = serde_json::from_str(&request_text).map_err(|_| Error::new("request"))?;
    let result: Value =
        serde_json::from_str(&result_text).map_err(|_| Error::new("ack_receipt"))?;
    let expected = ack_request(&result, &task, generation)?;
    let ack: Value = serde_json::from_str(&ack_text).map_err(|_| Error::new("ack_receipt"))?;
    if ack != expected {
        return Err(Error::new("ack_receipt"));
    }
    let executable = Executable {
        path,
        sha256: sha,
        args: vec!["ack-native-partition".into()],
    };
    executable_check(&executable)?;
    let mut command = Command::new(&executable.path);
    command
        .arg("ack-native-partition")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for (path_var, sha_var) in [
        (
            "OO_NATIVE_CONTROL_TARGET",
            "OO_NATIVE_CONTROL_TARGET_SHA256",
        ),
        ("OO_NATIVE_ACK_TARGET", "OO_NATIVE_ACK_TARGET_SHA256"),
    ] {
        let path = std::env::var(path_var).map_err(|_| Error::new("ack_target"))?;
        let digest = std::env::var(sha_var).map_err(|_| Error::new("ack_target"))?;
        if !Path::new(&path).is_absolute()
            || path.contains("..")
            || digest.len() != 64
            || !digest.bytes().all(|b| b.is_ascii_hexdigit())
            || sha256_hex(&std::fs::read(&path).map_err(|_| Error::new("ack_target"))?) != digest
        {
            return Err(Error::new("ack_target"));
        }
        command.env(path_var, path).env(sha_var, digest);
    }
    let mut child = command.spawn().map_err(|_| Error::new("spawn"))?;
    let mut stdin = child.stdin.take().ok_or_else(|| Error::new("spawn"))?;
    stdin
        .write_all(ack_text.as_bytes())
        .await
        .map_err(|_| Error::new("spawn"))?;
    stdin.shutdown().await.map_err(|_| Error::new("spawn"))?;
    drop(stdin);
    let output = tokio::time::timeout(Duration::from_secs(60), child.wait_with_output())
        .await
        .map_err(|_| Error::new("ack_timeout"))?
        .map_err(|_| Error::new("spawn"))?;
    let code = output.status.code().unwrap_or(-1);
    tx.execute("INSERT INTO jw_native.process_attempts(task_id,generation,stage,executable_sha256,exit_code,stdout,stderr) VALUES($1::text::uuid,$2,'ack',$3,$4,$5,$6) ON CONFLICT(task_id,generation,stage) DO UPDATE SET exit_code=EXCLUDED.exit_code,stdout=EXCLUDED.stdout,stderr=EXCLUDED.stderr,observed_at=clock_timestamp()",&[&task,&generation,&executable.sha256,&code,&output.stdout,&output.stderr]).await.map_err(Error::sql)?;
    println!(
        "process_stage=ack task={task} generation={generation} exit_code={code} stdout_bytes={} stderr_bytes={}",
        output.stdout.len(),
        output.stderr.len()
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    if !output.status.success()
        || response.as_object().is_none_or(|o| o.len() != 2)
        || response.get("protocol").and_then(Value::as_str)
            != Some("oo.pipeline.native-partition-ack-result/v1")
        || response.get("acknowledged").and_then(Value::as_bool) != Some(true)
    {
        tx.commit().await.map_err(Error::sql)?;
        return Err(Error::new("ack_refused"));
    }
    let source_generation = request
        .get("source_generation")
        .and_then(Value::as_i64)
        .ok_or_else(|| Error::new("position"))?;
    verify_applied(config, &source, &database, source_generation).await?;
    let raw_pos = result
        .pointer("/partition_target_receipt/partition_scope/partition_id")
        .ok_or_else(|| Error::new("position"))?;
    let pos = raw_pos
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| raw_pos.to_string());
    let load_state = result
        .get("load_state")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("ack_receipt"))?;
    tx.execute("INSERT INTO jw_native.watermarks(binding_id,pin,task_id,generation,source_generation,applied_pos,load_state,result) VALUES($1::text::uuid,$2,$3::text::uuid,$4,$5,$6,$7,$8::text::jsonb) ON CONFLICT(binding_id) DO UPDATE SET pin=EXCLUDED.pin,task_id=EXCLUDED.task_id,generation=EXCLUDED.generation,source_generation=EXCLUDED.source_generation,applied_pos=EXCLUDED.applied_pos,load_state=EXCLUDED.load_state,result=EXCLUDED.result,advanced_at=clock_timestamp() WHERE jw_native.watermarks.pin<EXCLUDED.pin",&[&binding,&pin,&task,&generation,&source_generation,&pos,&load_state,&result_text]).await.map_err(Error::sql)?;
    tx.execute("UPDATE jw_native.pending_acks SET acknowledged_at=clock_timestamp() WHERE task_id=$1::text::uuid",&[&task]).await.map_err(Error::sql)?;
    let n=tx.execute("INSERT INTO jw_native.source_progress(source_id,target_database,source_generation,task_id,binding_id,confirmed_at) VALUES($1,$2,$3,$4::text::uuid,$5::text::uuid,clock_timestamp()) ON CONFLICT(source_id,target_database) DO UPDATE SET source_generation=EXCLUDED.source_generation,task_id=EXCLUDED.task_id,binding_id=EXCLUDED.binding_id,active_task_id=NULL,confirmed_at=clock_timestamp() WHERE COALESCE(jw_native.source_progress.source_generation,0)=$3-1 AND (jw_native.source_progress.active_task_id=$4::text::uuid OR jw_native.source_progress.active_task_id IS NULL)",&[&source,&database,&source_generation,&task,&binding]).await.map_err(Error::sql)?;
    if n != 1 {
        return Err(Error::new("source_active"));
    }
    tx.commit().await.map_err(Error::sql)?;
    println!(
        "complete=acknowledged binding={binding} pin={pin} task={task} generation={generation}"
    );
    Ok(true)
}
pub async fn serve(config: &Config, project: &str, worker: &str, seconds: u64) -> Result<()> {
    if !crate::api::is_uuid(project) || seconds == 0 {
        return Err(Error::new("input"));
    }
    let start = tokio::time::Instant::now();
    loop {
        fire(config, project).await?;
        if let Err(e) = step(config, project, worker).await {
            eprintln!("cycle_error={}", e.kind)
        }
        if start.elapsed() >= Duration::from_secs(seconds) {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

fn c1_codec_closed(request: &Value) -> Result<&'static str> {
    let codec = request
        .get("position_codec")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("position"))?;
    let grade = request
        .get("consistency_grade")
        .and_then(Value::as_str)
        .unwrap_or("");
    match codec {
        "pg-commit-lsn/v1" | "snapshot-zero/v1" => Ok("legacy"),
        "mariadb-gtid/v1" | "mysql-gtid/v1" => {
            if grade != "T" || !gtid_request_closed(codec, request) {
                return Err(Error::new("position"));
            }
            Ok("gtid")
        }
        "mongo-cluster-time/v1" => {
            if (grade != "S0" && grade != "B")
                || request
                    .get("cross_table_consistent")
                    .and_then(Value::as_bool)
                    == Some(true)
                || !mongo_request_closed(request)
            {
                return Err(Error::new("position"));
            }
            Ok("mongo")
        }
        _ => Err(Error::new("position")),
    }
}
fn gtid_request_closed(codec: &str, request: &Value) -> bool {
    let lineage = request
        .get("position_lineage")
        .and_then(Value::as_str)
        .unwrap_or("");
    let pos = request
        .get("applied_pos")
        .and_then(Value::as_str)
        .unwrap_or("");
    let txn = request.get("txn_id").and_then(Value::as_str).unwrap_or("");
    let epoch = request.get("source_epoch").and_then(Value::as_i64);
    epoch.is_some_and(|value| (0..=i64::from(i32::MAX)).contains(&value))
        && !lineage.contains(',')
        && !pos.contains(',')
        && !txn.contains(',')
        && !txn.contains('/')
        && !pos.contains('/')
        && !pos.contains(':')
        && match codec {
            "mariadb-gtid/v1" => maria_pair(lineage).is_some_and(|(domain, server)| {
                gtid_digits(pos, true) && txn == format!("{domain}-{server}-{pos}")
            }),
            "mysql-gtid/v1" => lineage.strip_prefix("mysql.").is_some_and(|uuid| {
                mysql_uuid_text(uuid) && gtid_digits(pos, false) && txn == format!("{uuid}:{pos}")
            }),
            _ => false,
        }
}
#[cfg(test)]
fn gtid_family_order(left: &Value, right: &Value) -> std::result::Result<std::cmp::Ordering, ()> {
    if !gtid_request_closed(
        left.get("position_codec")
            .and_then(Value::as_str)
            .unwrap_or(""),
        left,
    ) || left.get("position_codec") != right.get("position_codec")
        || left.get("position_lineage") != right.get("position_lineage")
        || left.get("source_epoch") != right.get("source_epoch")
    {
        return Err(());
    }
    let left_pos = left.get("applied_pos").and_then(Value::as_str).ok_or(())?;
    let right_pos = right.get("applied_pos").and_then(Value::as_str).ok_or(())?;
    Ok(left_pos
        .len()
        .cmp(&right_pos.len())
        .then(left_pos.cmp(right_pos)))
}
fn mongo_request_closed(request: &Value) -> bool {
    let lineage = request
        .get("position_lineage")
        .and_then(Value::as_str)
        .unwrap_or("");
    let pos = request
        .get("applied_pos")
        .and_then(Value::as_str)
        .unwrap_or("");
    let epoch = request.get("source_epoch").and_then(Value::as_i64);
    let bytes = lineage.as_bytes();
    epoch.is_some_and(|value| (0..=i64::from(i32::MAX)).contains(&value))
        && !bytes.is_empty()
        && bytes.len() <= 128
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'_' | b'.' | b':' | b'-')
        })
        && !lineage.contains(',')
        && (1..=38).contains(&pos.len())
        && pos.bytes().all(|byte| byte.is_ascii_digit())
        && (pos.len() == 1 || pos.as_bytes()[0] != b'0')
        && pos != "0"
}
fn maria_pair(lineage: &str) -> Option<(u32, u32)> {
    let rest = lineage.strip_prefix('d')?;
    let (domain, server) = rest.split_once(".s")?;
    let domain = canonical_u32_text(domain)?;
    let server = canonical_u32_text(server).filter(|value| *value != 0)?;
    Some((domain, server))
}
fn canonical_u32_text(text: &str) -> Option<u32> {
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
fn gtid_digits(pos: &str, zero_ok: bool) -> bool {
    let bytes = pos.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 20
        && bytes.iter().all(|byte| byte.is_ascii_digit())
        && (bytes.len() == 1 || bytes[0] != b'0')
        && pos.parse::<u64>().is_ok()
        && (zero_ok || pos != "0")
}
fn mysql_uuid_text(text: &str) -> bool {
    let mut parts = text.split('-');
    for width in [8, 4, 4, 4, 12] {
        let Some(part) = parts.next() else {
            return false;
        };
        if part.len() != width
            || !part
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return false;
        }
    }
    parts.next().is_none()
}
fn c5_nochange_shape(
    request: &Value,
    binding: &str,
    pin: i64,
    source: &str,
    base: i64,
) -> Result<String> {
    if request.get("protocol").and_then(Value::as_str) != Some("oo.jobs.native-c5-nochange/v1")
        || request.get("binding").and_then(Value::as_str) != Some(binding)
        || request.get("pin").and_then(Value::as_i64) != Some(pin)
        || request.get("source_id").and_then(Value::as_str) != Some(source)
        || request.get("method").and_then(Value::as_str) != Some("C5")
        || request.get("capture_method").and_then(Value::as_str) != Some("c5_diff")
    {
        return Err(Error::new("source_binding"));
    }
    if request
        .get("acknowledged_source_generation")
        .and_then(Value::as_i64)
        != Some(base)
        || request.get("source_generation").and_then(Value::as_i64) != Some(base)
    {
        return Err(Error::new("source_generation"));
    }
    if request.get("empty_diff").and_then(Value::as_bool) != Some(true)
        || request.get("data_gen").and_then(Value::as_bool) != Some(false)
        || request.get("apply").and_then(Value::as_bool) != Some(false)
        || request.get("ack").and_then(Value::as_bool) != Some(false)
    {
        return Err(Error::new("source_binding"));
    }
    if let Some(codec) = request.get("position_codec") {
        if codec.as_str() != Some("snapshot-zero/v1") {
            return Err(Error::new("position"));
        }
    }
    let source_digest = request
        .get("source_digest")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("source_binding"))?;
    let parent = request
        .get("parent_digest")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("parent_digest"))?;
    if !is_hex64(source_digest)
        || !is_hex64(parent)
        || request.pointer("/snapshot/kind").and_then(Value::as_str) != Some("c5_cycle")
        || request.pointer("/snapshot/value").and_then(Value::as_str) != Some(source_digest)
    {
        return Err(Error::new("parent_digest"));
    }
    Ok(parent.to_string())
}
fn is_hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
#[allow(clippy::too_many_arguments)]
async fn finish_c1_quiet(c:&mut tokio_postgres::Client,project:&str,worker:&str,task:&str,generation:i64,
 binding:&str,pin:i64,source:&str,database:&str,base:i64,request:&Value)->Result<()> {
    let prior=request.pointer("/previous/task_id").and_then(Value::as_str).ok_or_else(||Error::new("baseline_quiet"))?;
    let exact:bool=c.query_one("SELECT EXISTS(SELECT 1 FROM jw_native.source_progress p JOIN jw_native.pending_acks a ON a.task_id=p.task_id JOIN jw_native.watermarks w ON w.task_id=p.task_id JOIN jobs.tasks t ON t.id=p.task_id WHERE p.source_id=$1 AND p.target_database=$2 AND p.task_id=$3::text::uuid AND p.source_generation=$4 AND p.active_task_id=$5::text::uuid AND a.acknowledged_at IS NOT NULL AND t.state='succeeded' AND w.applied_pos=split_part($6,':1-',2))",&[&source,&database,&prior,&base,&task,&request["observed_gtid"].as_str().ok_or_else(||Error::new("baseline_quiet"))?]).await.map_err(Error::sql)?.get(0);
    if !exact || request["source_id"]!=source || request["source_generation"]!=base || request["observed_gtid"]!=request["previous"]["l_end"] {return Err(Error::new("baseline_quiet"));}
    let tx=c.transaction().await.map_err(Error::sql)?;
    if lease::complete_success(&tx,project,task,worker,generation).await?.status!=LeaseStatus::Succeeded {return Err(Error::new("lost_lease"));}
    let result=json!({"protocol":"oo.jobs.mysql-c1-nochange/v1","source_id":source,"source_generation":base,"observed_gtid":request["observed_gtid"],"data_gen":false,"apply":false,"ack":false});
    tx.execute("UPDATE jw_native.occurrences SET result=$2::text::jsonb WHERE task_id=$1::text::uuid",&[&task,&result.to_string()]).await.map_err(Error::sql)?;
    let n=tx.execute("UPDATE jw_native.source_progress SET active_task_id=NULL WHERE source_id=$1 AND target_database=$2 AND active_task_id=$3::text::uuid AND source_generation=$4",&[&source,&database,&task,&base]).await.map_err(Error::sql)?;
    if n!=1 {return Err(Error::new("source_active"));}tx.commit().await.map_err(Error::sql)?;
    println!("complete=nochange method=C1 binding={binding} pin={pin} task={task} generation={generation} source_generation={base} apply=false ack=false");Ok(())
}
async fn finish_c5_nochange(
    c: &mut tokio_postgres::Client,
    project: &str,
    worker: &str,
    task: &str,
    generation: i64,
    binding: &str,
    pin: i64,
    source: &str,
    database: &str,
    base: i64,
    request: &Value,
) -> Result<()> {
    let parent = c5_nochange_shape(request, binding, pin, source, base)?;
    let stored:Option<String>=c.query_opt("SELECT COALESCE(o.request->>'input_digest', o.request #>> '{snapshot,value}') FROM jw_native.watermarks w JOIN jw_native.occurrences o ON o.task_id=w.task_id WHERE w.binding_id=$1::text::uuid AND w.source_generation=$2",&[&binding,&base]).await.map_err(Error::sql)?.and_then(|row| row.get(0));
    if stored.as_deref() != Some(parent.as_str()) {
        return Err(Error::new("parent_digest"));
    }
    let tx = c.transaction().await.map_err(Error::sql)?;
    let done = lease::complete_success(&tx, project, task, worker, generation).await?;
    if done.status != LeaseStatus::Succeeded {
        return Err(Error::new("lost_lease"));
    }
    let result = json!({"protocol":"oo.jobs.native-c5-nochange/v1","source_id":source,"source_generation":base,"empty_diff":true,"data_gen":false,"apply":false,"ack":false});
    tx.execute(
        "UPDATE jw_native.occurrences SET result=$2::text::jsonb WHERE task_id=$1::text::uuid",
        &[&task, &result.to_string()],
    )
    .await
    .map_err(Error::sql)?;
    let n=tx.execute("UPDATE jw_native.source_progress SET active_task_id=NULL WHERE source_id=$1 AND target_database=$2 AND active_task_id=$3::text::uuid AND COALESCE(source_generation,0)=$4",&[&source,&database,&task,&base]).await.map_err(Error::sql)?;
    if n != 1 {
        return Err(Error::new("source_active"));
    }
    tx.commit().await.map_err(Error::sql)?;
    println!(
        "complete=nochange binding={binding} pin={pin} task={task} generation={generation} source_generation={base}"
    );
    Ok(())
}
fn method_token(method: &str) -> Result<&'static str> {
    match method {
        "C1" => Ok("c1_cdc"),
        "C2" => Ok("c2_change_feed"),
        "C3" => Ok("c3_cursor"),
        "C4" => Ok("c4_partition"),
        "C5" => Ok("c5_diff"),
        _ => Err(Error::new("method")),
    }
}
fn request_protocol(method: &str) -> &'static str {
    if method == "C4" {
        "oo.pipeline.native-partition/v1"
    } else {
        "oo.pipeline.native-update/v1"
    }
}

fn pg_lsn_decimal(value: &str) -> Result<String> {
    let (high, low) = value
        .split_once('/')
        .ok_or_else(|| Error::new("transaction_scope"))?;
    let high = u32::from_str_radix(high, 16).map_err(|_| Error::new("transaction_scope"))?;
    let low = u32::from_str_radix(low, 16).map_err(|_| Error::new("transaction_scope"))?;
    Ok(((u64::from(high) << 32) | u64::from(low)).to_string())
}

// One source snapshot has one global generation, even when it contains
// several tables. The operator pins each sealed registry independently.
#[allow(clippy::too_many_arguments)]
fn baseline_entries(
    request: &Value,
    vector: &Value,
    binding: &str,
    pin: i64,
    source: &str,
    base: i64,
    database: &str,
    task: &str,
    generation: i64,
) -> Result<Vec<(Value, String, String)>> {
    let object = request
        .as_object()
        .ok_or_else(|| Error::new("baseline_request"))?;
    if object.len() != 7
        || request.get("protocol").and_then(Value::as_str)
            != Some("oo.jobs.native-baseline-producer/v1")
        || request.get("binding").and_then(Value::as_str) != Some(binding)
        || request.get("pin").and_then(Value::as_i64) != Some(pin)
        || request.get("source_id").and_then(Value::as_str) != Some(source)
        || request.get("capture_method").and_then(Value::as_str) != Some("c1_cdc")
        || request.get("source_generation").and_then(Value::as_i64) != Some(base + 1)
    {
        return Err(Error::new("baseline_request"));
    }
    let entries = request
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::new("baseline_request"))?;
    let pins = vector
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::new("baseline_registry"))?;
    if vector.as_object().is_none_or(|o| o.len() != 2)
        || vector.get("protocol").and_then(Value::as_str)
            != Some("oo.pipeline.native-transaction-registry-vector/v1")
        || entries.is_empty()
        || entries.len() > 16
        || entries.len() != pins.len()
    {
        return Err(Error::new("baseline_registry"));
    }
    let mut checked = Vec::with_capacity(entries.len());
    let mut previous = None;
    let mut config_digest = None;
    for (entry, pin_entry) in entries.iter().zip(pins) {
        let schema = entry
            .get("logical_schema")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("baseline_request"))?;
        let table = entry
            .get("logical_table")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("baseline_request"))?;
        let key = (schema, table);
        if previous.is_some_and(|old| old >= key)
            || pin_entry.as_object().is_none_or(|o| o.len() != 4)
            || pin_entry.get("logical_schema").and_then(Value::as_str) != Some(schema)
            || pin_entry.get("logical_table").and_then(Value::as_str) != Some(table)
            || entry.get("protocol").and_then(Value::as_str) != Some("oo.pipeline.native-update/v1")
            || entry.get("source_id").and_then(Value::as_str) != Some(source)
            || entry.get("capture_method").and_then(Value::as_str) != Some("c1_cdc")
            || entry.get("source_generation").and_then(Value::as_i64) != Some(base + 1)
            || entry.get("position_codec").and_then(Value::as_str) != Some("snapshot-zero/v1")
            || entry.get("applied_pos").and_then(Value::as_str) != Some("0")
            || entry.get("consistency_grade").and_then(Value::as_str) != Some("S1")
            || entry.get("cross_table_consistent").and_then(Value::as_bool) != Some(false)
            || entry.pointer("/target/database").and_then(Value::as_str) != Some(database)
            || entry.get("mirror").and_then(Value::as_bool) != Some(false)
        {
            return Err(Error::new("baseline_request"));
        }
        let digest = entry
            .get("config_digest")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("baseline_request"))?;
        if config_digest.is_some_and(|old| old != digest) {
            return Err(Error::new("baseline_request"));
        }
        config_digest = Some(digest);
        let path = pin_entry
            .get("registry_path")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("baseline_registry"))?;
        let sha = pin_entry
            .get("registry_sha256")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("baseline_registry"))?;
        if !Path::new(path).is_absolute()
            || path.contains("..")
            || sha.len() != 64
            || !sha.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::new("baseline_registry"));
        }
        let mut invoked = entry.clone();
        crate::capture_source::attach_observed_lease(&mut invoked, task, generation)
            .map_err(|_| Error::new("lease"))?;
        invocation_mirror(&mut invoked, "C1")?;
        checked.push((invoked, path.to_owned(), sha.to_owned()));
        previous = Some(key);
    }
    Ok(checked)
}

fn group_loader_request(
    request: &Value,
    binding: &str,
    pin: i64,
    source: &str,
    base: i64,
    task: &str,
    generation: i64,
) -> Result<Value> {
    let object = request
        .as_object()
        .ok_or_else(|| Error::new("transaction_request"))?;
    if object.len() != 8
        || request.get("protocol").and_then(Value::as_str)
            != Some("oo.jobs.native-transaction-producer/v1")
        || request.get("binding").and_then(Value::as_str) != Some(binding)
        || request.get("pin").and_then(Value::as_i64) != Some(pin)
        || request.get("source_id").and_then(Value::as_str) != Some(source)
        || request.get("capture_method").and_then(Value::as_str) != Some("c1_cdc")
        || request.get("source_generation").and_then(Value::as_i64) != Some(base + 1)
    {
        return Err(Error::new("transaction_request"));
    }
    let scope = request
        .get("transaction_scope")
        .ok_or_else(|| Error::new("transaction_scope"))?;
    if scope.get("complete").and_then(Value::as_bool) != Some(true)
        || scope.get("source_id").and_then(Value::as_str) != Some(source)
    {
        return Err(Error::new("transaction_scope"));
    }
    let end = pg_lsn_decimal(
        scope
            .get("end_lsn")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("transaction_scope"))?,
    )?;
    let tables = scope
        .get("tables")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::new("transaction_scope"))?;
    let entries = request
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::new("transaction_request"))?;
    if entries.is_empty() || entries.len() != tables.len() {
        return Err(Error::new("transaction_request"));
    }
    let mut checked = Vec::with_capacity(entries.len());
    let mut previous = None;
    for (entry, table) in entries.iter().zip(tables) {
        let schema = entry
            .get("logical_schema")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("transaction_request"))?;
        let name = entry
            .get("logical_table")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("transaction_request"))?;
        let key = (schema, name);
        if previous.is_some_and(|old| old >= key)
            || table.get("logical_schema").and_then(Value::as_str) != Some(schema)
            || table.get("logical_table").and_then(Value::as_str) != Some(name)
            || entry.get("protocol").and_then(Value::as_str) != Some("oo.pipeline.native-update/v1")
            || entry.get("source_id").and_then(Value::as_str) != Some(source)
            || entry.get("capture_method").and_then(Value::as_str) != Some("c1_cdc")
            || entry.get("consistency_grade").and_then(Value::as_str) != Some("T")
            || entry.get("cross_table_consistent").and_then(Value::as_bool) != Some(true)
            || entry.get("position_codec").and_then(Value::as_str) != Some("pg-commit-lsn/v1")
            || entry.get("applied_pos").and_then(Value::as_str) != Some(end.as_str())
            || entry.get("input_digest") != table.get("arrow_sha256")
            || entry.get("source_generation") != table.get("source_generation")
            || entry.get("parent_source_generation") != table.get("parent_source_generation")
            || entry.get("source_generation").and_then(Value::as_i64)
                != entry
                    .get("parent_source_generation")
                    .and_then(Value::as_i64)
                    .map(|g| g + 1)
        {
            return Err(Error::new("transaction_request"));
        }
        previous = Some(key);
        let mut invoked = entry.clone();
        crate::capture_source::attach_observed_lease(&mut invoked, task, generation)
            .map_err(|_| Error::new("lease"))?;
        checked.push(invoked);
    }
    Ok(json!({"protocol":"oo.pipeline.native-transaction/v1","entries":checked}))
}

fn invocation_mirror(request: &mut Value, method: &str) -> Result<()> {
    let object = request
        .as_object_mut()
        .ok_or_else(|| Error::new("request"))?;
    if method == "C4" {
        if object
            .get("mirror")
            .is_some_and(|v| v.as_bool() != Some(false))
        {
            return Err(Error::new("c4_mirror"));
        }
        object.insert("mirror".into(), Value::Bool(false));
    } else {
        object.insert("mirror".into(), Value::Bool(true));
    }
    Ok(())
}

#[cfg(test)]
mod baseline_tests {
    use super::*;
    fn fixture() -> (Value, Value) {
        let entry = |table: &str| json!({"protocol":"oo.pipeline.native-update/v1","source_id":"s","capture_method":"c1_cdc","source_generation":1,"logical_schema":"public","logical_table":table,"position_codec":"snapshot-zero/v1","applied_pos":"0","consistency_grade":"S1","cross_table_consistent":false,"config_digest":"same","mirror":false,"target":{"database":"db"}});
        (
            json!({"protocol":"oo.jobs.native-baseline-producer/v1","binding":"b","pin":1,"source_id":"s","capture_method":"c1_cdc","source_generation":1,"entries":[entry("a"),entry("b")]}),
            json!({"protocol":"oo.pipeline.native-transaction-registry-vector/v1","entries":[{"logical_schema":"public","logical_table":"a","registry_path":"/r/a","registry_sha256":"a".repeat(64)},{"logical_schema":"public","logical_table":"b","registry_path":"/r/b","registry_sha256":"b".repeat(64)}]}),
        )
    }
    #[test]
    fn baseline_snapshot_vector_is_closed_and_keeps_one_real_lease() {
        let (mut request, vector) = fixture();
        let accepted = baseline_entries(
            &request,
            &vector,
            "b",
            1,
            "s",
            0,
            "db",
            "d101fd4a-3a89-41f6-b7fe-94b4001d5512",
            2,
        )
        .unwrap();
        assert_eq!(accepted.len(), 2);
        assert_eq!(accepted[0].0["lease_generation"], 2);
        assert_eq!(accepted[1].0["mirror"], true);
        request["entries"][1]["logical_table"] = json!("a");
        assert!(
            baseline_entries(
                &request,
                &vector,
                "b",
                1,
                "s",
                0,
                "db",
                "d101fd4a-3a89-41f6-b7fe-94b4001d5512",
                2
            )
            .is_err()
        );
        let (mut request, vector) = fixture();
        request["entries"][1]["config_digest"] = json!("other");
        assert!(
            baseline_entries(
                &request,
                &vector,
                "b",
                1,
                "s",
                0,
                "db",
                "d101fd4a-3a89-41f6-b7fe-94b4001d5512",
                2
            )
            .is_err()
        );
        let (request, mut vector) = fixture();
        vector["entries"][1]["registry_sha256"] = json!("tampered");
        assert!(
            baseline_entries(
                &request,
                &vector,
                "b",
                1,
                "s",
                0,
                "db",
                "d101fd4a-3a89-41f6-b7fe-94b4001d5512",
                2
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod control_pin_tests {
    use super::*;
    #[test]
    fn vector_bytes_are_bounded_pinned_and_frozen_per_uuid() {
        let first = b"sealed vector";
        let hash = sha256_hex(first);
        assert_eq!(
            vector_bytes_sha(first, Some(&hash), Some(first)).unwrap(),
            hash
        );
        assert!(vector_bytes_sha(first, Some(&"a".repeat(64)), None).is_err());
        assert!(vector_bytes_sha(b"different vector", None, Some(first)).is_err());
        assert!(vector_bytes_sha(&vec![0; 1024 * 1024 + 1], None, None).is_err());
        assert!(vector_bytes_sha(b"next pin", None, None).is_ok());
    }
    #[test]
    fn vector_template_is_operator_scoped_and_static_hashes_stay_required() {
        assert!(vector_path("/owned/{binding}/{pin}/vector.json", None, true).is_ok());
        for path in [
            "relative/{binding}/{pin}",
            "/owned/../{binding}/{pin}",
            "/owned/{binding}/{other}",
            "/owned/{pin}",
            "/owned/{binding}",
        ] {
            assert!(vector_path(path, None, true).is_err())
        }
        assert!(vector_path("/owned/{binding}/{pin}", Some(&"a".repeat(64)), true).is_err());
        assert!(vector_path("/owned/{binding}/{pin}", None, false).is_err());
        assert!(vector_path("/static", None, true).is_err());
        assert!(vector_path("/static", Some(&"a".repeat(64)), true).is_ok());
        assert!(resolve_vector_path("/owned/{binding}/{pin}", "../escape", 1).is_err());
        assert!(
            resolve_vector_path(
                "/owned/{binding}/{pin}",
                "d101fd4a-3a89-41f6-b7fe-94b4001d5512",
                0
            )
            .is_err()
        );
        assert_eq!(
            resolve_vector_path(
                "/owned/{binding}/{pin}",
                "d101fd4a-3a89-41f6-b7fe-94b4001d5512",
                2
            )
            .unwrap(),
            "/owned/d101fd4a-3a89-41f6-b7fe-94b4001d5512/2"
        );
    }
    #[test]
    fn group_control_descriptor_is_operator_pinned_and_closed() {
        let mut manifest:Manifest=serde_json::from_value(json!({"protocol":"oo.jobs.native-schedule/v1","binding":"d101fd4a-3a89-41f6-b7fe-94b4001d5512","project_id":"00000000-0000-4000-8000-000000000074","source_id":"s","method":"C1","cycle":"pull","period_ms":1000,"producer":{"path":"/producer","sha256":"a".repeat(64),"args":[]},"loader":{"path":"/loader","sha256":"b".repeat(64),"args":["import-native-transaction"]},"registration":{"admission_binary":"/admission","admission_sha256":"c".repeat(64),"transaction_registry_vector_path":"/vector","transaction_registry_vector_sha256":"d".repeat(64),"control_target_path":"/private/control.json","control_target_sha256":"e".repeat(64)}})).unwrap();
        assert!(manifest.shape().is_ok());
        manifest
            .registration
            .as_mut()
            .unwrap()
            .control_target_sha256 = None;
        assert!(manifest.shape().is_err());
        manifest
            .registration
            .as_mut()
            .unwrap()
            .control_target_sha256 = Some("e".repeat(64));
        manifest.loader.args = vec!["import-native-update".into()];
        assert!(manifest.shape().is_err());
    }
}

#[cfg(test)]
mod invocation_tests {
    use super::*;
    #[test]
    fn c1_digest_has_no_registry_cycle_and_still_binds_capture() {
        let capture = json!({"capture_digest":"0".repeat(64),"entries":[{"snapshot_arrow":{"sha256":"a".repeat(64)},"transactions":[{"txn_id":"g:1"}]}]});
        let expected = c1_capture_digest(&mut capture.clone()).unwrap();
        assert_eq!(
            expected,
            "033c12a3f8a18bd6f35598682992b0a53d4a0f193fceb2efa99bde84ee6bcc6c"
        );
        let mut sealed = capture.clone();
        sealed["entries"][0]["admission_registry_path"] = json!("/sealed/registry.json");
        sealed["entries"][0]["admission_registry_sha256"] = json!("b".repeat(64));
        assert_eq!(c1_capture_digest(&mut sealed.clone()).unwrap(), expected);
        assert!(
            validate_c1_envelope_entries(&capture, sealed["entries"].as_array().unwrap()).is_err()
        );
        sealed["entries"][0]["snapshot_arrow"]["sha256"] = json!("c".repeat(64));
        assert_ne!(c1_capture_digest(&mut sealed).unwrap(), expected);
    }
    #[test]
    fn c1_envelope_binds_exact_per_table_registry_and_event_refs() {
        let entries = vec![
            json!({"schema":"s","table":"t","admission_registry_path":"/sealed/r.json","admission_registry_sha256":"a".repeat(32),"transactions":[{"txn_id":"g:1","applied_pos":"1"}]}),
        ];
        let envelope =
            json!({"protocol":"oo.source-admission.c1-capture-envelope/v1","entries":entries});
        assert!(
            validate_c1_envelope_entries(&envelope, envelope["entries"].as_array().unwrap())
                .is_ok()
        );
        let forged = vec![
            json!({"schema":"s","table":"t","admission_registry_path":"/mutable/r.json","admission_registry_sha256":"b".repeat(32),"transactions":[]}),
        ];
        assert!(validate_c1_envelope_entries(&envelope, &forged).is_err());
        let mut wrong_protocol = envelope;
        wrong_protocol["protocol"] = json!("oo.pipeline.native-c1-baseline/v1");
        assert!(validate_c1_envelope_entries(&wrong_protocol, &entries).is_err());
    }
    #[test]
    fn separate_c1_target_does_not_redirect_c5_or_c4() {
        assert_eq!(
            target_database_for("control", Some("c4"), Some("c1"), "C1").unwrap(),
            "c1"
        );
        assert_eq!(
            target_database_for("control", Some("c4"), Some("c1"), "C4").unwrap(),
            "c4"
        );
        assert_eq!(
            target_database_for("control", Some("c4"), Some("c1"), "C5").unwrap(),
            "control"
        );
        assert_eq!(
            target_database_for("control", None, None, "C1").unwrap(),
            "control"
        );
    }

    #[test]
    fn frozen_c4_without_mirror_is_invoked_without_target_cas() {
        let frozen = json!({"protocol":"oo.pipeline.native-partition/v1","source_generation":7});
        let mut invoked = frozen.clone();
        invocation_mirror(&mut invoked, "C4").expect("invocation");
        assert!(frozen.get("mirror").is_none());
        assert_eq!(invoked["mirror"], false);
        assert!(invocation_mirror(&mut json!({"mirror":true}), "C4").is_err());
    }
    #[test]
    fn transaction_group_keeps_table_generations_and_freezes_one_lease() {
        let scope = json!({"complete":true,"source_id":"pg","end_lsn":"0/2D8FAF0","tables":[
   {"logical_schema":"a","logical_table":"one","arrow_sha256":"abc","parent_source_generation":3,"source_generation":4},
   {"logical_schema":"b","logical_table":"two","arrow_sha256":"def","parent_source_generation":4,"source_generation":5}]});
        let entries=[("a","one","abc",3,4),("b","two","def",4,5)].into_iter().map(|(schema,table,digest,parent,generation)|json!({
   "protocol":"oo.pipeline.native-update/v1","source_id":"pg","capture_method":"c1_cdc",
   "logical_schema":schema,"logical_table":table,"input_digest":digest,
   "parent_source_generation":parent,"source_generation":generation,"consistency_grade":"T",
   "cross_table_consistent":true,"position_codec":"pg-commit-lsn/v1","applied_pos":"47774448"
  })).collect::<Vec<_>>();
        let frozen = json!({"protocol":"oo.jobs.native-transaction-producer/v1","binding":"binding",
   "pin":1,"source_id":"pg","capture_method":"c1_cdc","source_generation":4,
   "transaction_scope":scope,"entries":entries});
        let task = "00000000-0000-4000-8000-000000000001";
        let invoked = group_loader_request(&frozen, "binding", 1, "pg", 3, task, 7).expect("group");
        assert_eq!(invoked["entries"][0]["source_generation"], 4);
        assert_eq!(invoked["entries"][1]["source_generation"], 5);
        assert_eq!(invoked["entries"][0]["lease_generation"], 7);
        assert_eq!(invoked["entries"][1]["lease_id"], task);
        assert!(frozen["entries"][0].get("lease_id").is_none());
        let mut changed = frozen.clone();
        changed["entries"][1]["applied_pos"] = json!("47774449");
        assert!(group_loader_request(&changed, "binding", 1, "pg", 3, task, 7).is_err());
    }
    #[test]
    fn gtid_codecs_and_mongo_weak_grade_do_not_cross_families() {
        let maria = json!({"position_codec":"mariadb-gtid/v1","position_lineage":"d0.s1","source_epoch":1,"applied_pos":"7","txn_id":"0-1-7","consistency_grade":"T"});
        let mysql = json!({"position_codec":"mysql-gtid/v1","position_lineage":"mysql.1bc0749a-c3f5-11f1-9a89-6e5583e808c4","source_epoch":1,"applied_pos":"17","txn_id":"1bc0749a-c3f5-11f1-9a89-6e5583e808c4:17","consistency_grade":"T"});
        assert_eq!(c1_codec_closed(&maria).unwrap(), "gtid");
        assert_eq!(c1_codec_closed(&mysql).unwrap(), "gtid");
        assert!(c1_codec_closed(&json!({"position_codec":"mariadb-gtid/v1","position_lineage":"d0.s1","source_epoch":1,"applied_pos":"7","txn_id":"0-1-7,0-1-8","consistency_grade":"T"})).is_err());
        let mut other = maria.clone();
        other["position_lineage"] = json!("d0.s2");
        other["txn_id"] = json!("0-2-7");
        assert!(gtid_family_order(&maria, &other).is_err());
        assert!(gtid_family_order(&maria, &mysql).is_err());
        let mut later = maria.clone();
        later["applied_pos"] = json!("8");
        later["txn_id"] = json!("0-1-8");
        assert_eq!(
            gtid_family_order(&maria, &later).unwrap(),
            std::cmp::Ordering::Less
        );
        let mongo = json!({"position_codec":"mongo-cluster-time/v1","position_lineage":"rs0","source_epoch":2,"applied_pos":"15","consistency_grade":"S0","cross_table_consistent":false});
        assert_eq!(c1_codec_closed(&mongo).unwrap(), "mongo");
        let mut weak = mongo.clone();
        weak["consistency_grade"] = json!("B");
        assert_eq!(c1_codec_closed(&weak).unwrap(), "mongo");
        weak["consistency_grade"] = json!("T");
        assert!(c1_codec_closed(&weak).is_err());
        assert!(
            c1_codec_closed(
                &json!({"position_codec":"mongo-resume-token/v1","consistency_grade":"S0"})
            )
            .is_err()
        );
        assert!(c1_codec_closed(&json!({"position_codec":"snapshot-zero/v1","consistency_grade":"S0","applied_pos":"0"})).is_ok());
    }
    #[test]
    fn c5_nochange_binds_parent_and_refuses_a_fake_advance() {
        let digest = "ab".repeat(32);
        let parent = "cd".repeat(32);
        let request = json!({"protocol":"oo.jobs.native-c5-nochange/v1","binding":"b","pin":3,"source_id":"oo_dl01w1_44","method":"C5","capture_method":"c5_diff","acknowledged_source_generation":12,"source_generation":12,"empty_diff":true,"snapshot":{"kind":"c5_cycle","value":digest},"source_digest":digest,"parent_digest":parent,"data_gen":false,"apply":false,"ack":false});
        assert_eq!(
            c5_nochange_shape(&request, "b", 3, "oo_dl01w1_44", 12).unwrap(),
            parent
        );
        let mut fake = request.clone();
        fake["source_generation"] = json!(13);
        assert!(c5_nochange_shape(&fake, "b", 3, "oo_dl01w1_44", 12).is_err());
        let mut wrong_source = request.clone();
        wrong_source["source_id"] = json!("other");
        assert!(c5_nochange_shape(&wrong_source, "b", 3, "oo_dl01w1_44", 12).is_err());
        let mut codec = request.clone();
        codec["position_codec"] = json!("mariadb-gtid/v1");
        assert!(c5_nochange_shape(&codec, "b", 3, "oo_dl01w1_44", 12).is_err());
        let mut parent_bad = request.clone();
        parent_bad["parent_digest"] = json!("zz".repeat(32));
        assert!(c5_nochange_shape(&parent_bad, "b", 3, "oo_dl01w1_44", 12).is_err());
    }
}

/// Read only the last successfully CAS-committed occurrence, never a capture candidate.
pub async fn state(config: &Config, binding: &str) -> Result<()> {
    if !crate::api::is_uuid(binding) {
        return Err(Error::new("binding"));
    }
    let c = session::runtime(config).await?;
    let row=c.query_opt("SELECT jsonb_build_object('protocol','oo.jobs.native-state/v1','binding',b.id,'source_id',b.source_id,'method',b.method,'cycle',b.cycle,'period_ms',b.period_ms,'enabled',b.enabled,'applied',w.task_id IS NOT NULL,'pin',w.pin,'task_id',w.task_id,'lease_generation',w.generation,'source_generation',w.source_generation,'applied_pos',w.applied_pos,'load_state',w.load_state,'advanced_at',w.advanced_at,'position_codec',o.request->'position_codec','position_lineage',o.request->'position_lineage','sqlserver_candidate_position',o.request->'sqlserver_candidate_position')::text FROM jw_native.bindings b LEFT JOIN jw_native.watermarks w ON w.binding_id=b.id LEFT JOIN jw_native.occurrences o ON o.task_id=w.task_id LEFT JOIN jobs.tasks t ON t.id=w.task_id WHERE b.id=$1::text::uuid AND (w.task_id IS NULL OR t.state='succeeded')",&[&binding]).await.map_err(Error::sql)?;
    let row = row.ok_or_else(|| Error::new("binding"))?;
    let text: String = row.get(0);
    println!("{text}");
    Ok(())
}

/// Export the exact acknowledged cut for downstream catalog and Pack maintenance.
pub async fn artifacts(config: &Config, binding: &str, out: &str) -> Result<()> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    if !crate::api::is_uuid(binding) { return Err(Error::new("binding")); }
    let control=session::runtime(config).await?;
    let row=control.query_opt("SELECT b.source_id,b.method,w.task_id::text,w.source_generation,o.request::text,w.result::text FROM jw_native.bindings b JOIN jw_native.watermarks w ON w.binding_id=b.id JOIN jw_native.occurrences o ON o.task_id=w.task_id JOIN jobs.tasks j ON j.id=w.task_id WHERE b.id=$1::text::uuid AND j.state='succeeded'",&[&binding]).await.map_err(Error::sql)?.ok_or_else(||Error::new("unconfirmed_baseline"))?;
    let source:String=row.get(0);let method:String=row.get(1);let task:String=row.get(2);let generation:i64=row.get(3);
    let request:Value=serde_json::from_str(&row.get::<_,String>(4)).map_err(|_|Error::new("request"))?;
    let load:Value=serde_json::from_str(&row.get::<_,String>(5)).map_err(|_|Error::new("request"))?;
    if method!="C1" || request["protocol"]!="oo.pipeline.native-c1-baseline/v1"
        || request["source_id"]!=source || request["source_generation"]!=generation
        || load["source_id"]!=source || load["source_generation"]!=generation {
        return Err(Error::new("unconfirmed_baseline"));
    }
    let database=target_database(config,&method)?;
    let target=session::admin_db(config,&database).await?;
    let ack:Option<String>=target.query_opt("SELECT jsonb_build_object('protocol','oo.mysql.c1-ack-state/v1','acknowledged',true,'task_state',j.state,'request_digest',b.request_digest,'receipt',b.receipt,'source_id',w.source,'source_generation',w.source_generation,'target_generation',w.target_generation,'applied_pos',_oo.native_pos_text(w.applied_pos),'source_epoch',w.source_epoch)::text FROM _oo.native_c1_baseline_typed b JOIN jobs.tasks j ON j.id=b.task_id JOIN _oo.source_watermark w ON w.source=b.source AND w.source_generation=(b.receipt->>'source_generation')::bigint AND w.target_generation=(b.receipt->>'target_generation')::bigint AND w.source_epoch=(b.receipt->>'source_epoch')::integer AND w.applied_pos=(b.receipt->>'applied_pos')::numeric WHERE b.source=$1 AND b.task_id=$2::text::uuid AND b.acknowledged_at IS NOT NULL AND j.state='succeeded'",&[&source,&task]).await.map_err(Error::sql)?.map(|r|r.get(0));
    let ack:Value=serde_json::from_str(&ack.ok_or_else(||Error::new("unconfirmed_baseline"))?).map_err(|_|Error::new("request"))?;
    if ack["receipt"]!=load {return Err(Error::new("unconfirmed_baseline"));}
    let root=Path::new(out);
    std::fs::create_dir(root).map_err(|_|Error::new("artifact_output"))?;
    std::fs::set_permissions(root,std::fs::Permissions::from_mode(0o700)).map_err(|_|Error::new("artifact_output"))?;
    let mut hashes=serde_json::Map::new();
    for (name,value) in [("request",request),("load",load),("ack",ack)] {
        let bytes=serde_json::to_vec(&value).map_err(|_|Error::new("request"))?;
        let mut file=std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(root.join(format!("{name}.json"))).map_err(|_|Error::new("artifact_output"))?;
        std::io::Write::write_all(&mut file,&bytes).map_err(|_|Error::new("artifact_output"))?;
        file.sync_all().map_err(|_|Error::new("artifact_output"))?;
        hashes.insert(name.to_owned(),json!(sha256_hex(&bytes)));
    }
    println!("{}",json!({"protocol":"oo.jobs.native-artifacts/v1","binding":binding,"source_id":source,"task_id":task,"source_generation":generation,"sha256":hashes}));
    Ok(())
}

/// Explicit operator replacement of only the loader/registration used by a binding.
/// Original occurrence manifest and source request remain immutable evidence.
pub async fn loader_put(config: &Config, path: &str) -> Result<()> {
    let m = manifest_read(path)?;
    let (text, sha) = canonical(&m)?;
    let c = session::runtime(config).await?;
    let changed=c.execute("INSERT INTO jw_native.loader_overrides(binding_id,operator_manifest,operator_manifest_sha) SELECT id,$2::text::jsonb,$3 FROM jw_native.bindings WHERE id=$1::text::uuid AND project_id=$4::text::uuid AND source_id=$5 AND method=$6 AND cycle=$7 ON CONFLICT(binding_id) DO UPDATE SET operator_manifest=EXCLUDED.operator_manifest,operator_manifest_sha=EXCLUDED.operator_manifest_sha,approved_by=current_user,approved_at=clock_timestamp()",&[&m.binding,&text,&sha,&m.project_id,&m.source_id,&m.method,&m.cycle]).await.map_err(Error::sql)?;
    if changed != 1 {
        return Err(Error::new("binding_identity"));
    }
    println!(
        "loader_override=registered binding={} manifest_sha={sha} source_request_rewritten=false",
        m.binding
    );
    Ok(())
}

#[cfg(test)]
mod ack_tests {
    use super::*;
    #[test]
    fn c4_ack_is_derived_from_loader_receipt() {
        let task = "d3b1aa26-1505-40be-b0bb-47c53996a8bf";
        let result = json!({"source_id":"clickhouse_268","logical_schema":"oo_dl01w40","logical_table":"readings","run_id":"ebcc9bdc-df0c-427b-9133-771875c0afb1","payload_digest":"4dab995f958e28b8da4c244cbe78b225c0dd50caffaf834d5bec763aedf44c9d","jobs_lease_receipt":{"state":"running"},"partition_target_receipt":{"acknowledged":false,"job_task_id":task,"job_generation":1,"batch_id":"t1.b1","payload_digest":"4dab995f958e28b8da4c244cbe78b225c0dd50caffaf834d5bec763aedf44c9d","partition_scope":{"source_id":"clickhouse_268","logical_schema":"oo_dl01w40","logical_table":"readings","partition_id":"202401"}}});
        let ack = ack_request(&result, task, 1).unwrap();
        assert_eq!(ack["protocol"], "oo.pipeline.native-partition-ack/v1");
        assert_eq!(ack["partition_id"], "202401");
        assert!(ack_request(&result, task, 2).is_err());
    }
}
