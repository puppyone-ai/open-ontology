use crate::c5plus::{self, Result};
#[cfg(test)]
use arrow_ipc::writer::StreamWriter;
use mysql_async::{Conn, Params, Value as Cell, prelude::Queryable};
use oo_connectors::{
    mysql::{
        self,
        chunk::{self, ChunkBound, KeyValue},
    },
    types::RunBinding,
};
use oo_pipeline::target_installer::sha256_hex;
use serde_json::{Value, json};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::{fs, path::Path, time::Duration};

#[path = "c5_float_sql.rs"]
mod c5_float_sql;
#[path = "c5_digest.rs"]
pub mod digest;
#[path = "c5_range_parts.rs"]
mod range_parts;
#[path = "c5_empty.rs"]
pub mod empty;
#[cfg(test)]
#[path = "c5_layered_tests.rs"]
mod tests;

// Approved source-data/Arrow budget, independent of executable artifact limits.
const MAX_DATA_BYTES: u64 = 33_554_432;

pub fn encode_bound(values: Option<&[KeyValue]>) -> Value {
    values
        .map(|values| {
            Value::Array(
                values
                    .iter()
                    .map(|v| match v {
                        KeyValue::Int(n) => json!({"kind":"int","value":*n}),
                        KeyValue::Text(b) => json!({"kind":"text","value":hex(b)}),
                        KeyValue::Bytes(b) => json!({"kind":"bytes","value":hex(b)}),
                    })
                    .collect(),
            )
        })
        .unwrap_or(Value::Null)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn decode_bound(value: &Value) -> Result<Option<Vec<KeyValue>>> {
    if value.is_null() {
        return Ok(None);
    }
    let items = value
        .as_array()
        .filter(|a| !a.is_empty())
        .ok_or("boundary")?;
    Ok(Some(
        items
            .iter()
            .map(|item| {
                if item.as_object().is_none_or(|o| o.len() != 2) {
                    return Err("boundary".into());
                }
                Ok(match item["kind"].as_str() {
                    Some("int") => KeyValue::Int(
                        item["value"]
                            .as_i64()
                            .map(i128::from)
                            .or_else(|| item["value"].as_u64().map(i128::from))
                            .ok_or("integer")?,
                    ),
                    Some(kind @ ("text" | "bytes")) => {
                        let text = item["value"].as_str().ok_or("hex")?;
                        if text.len() % 2 != 0
                            || !text
                                .bytes()
                                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                        {
                            return Err("hex".into());
                        }
                        let bytes = (0..text.len())
                            .step_by(2)
                            .map(|i| u8::from_str_radix(&text[i..i + 2], 16))
                            .collect::<std::result::Result<Vec<_>, _>>()?;
                        if kind == "text" {
                            KeyValue::Text(bytes)
                        } else {
                            KeyValue::Bytes(bytes)
                        }
                    }
                    _ => return Err("boundary".into()),
                })
            })
            .collect::<Result<Vec<_>>>()?,
    ))
}
fn params(values: Vec<KeyValue>) -> Result<Params> {
    Ok(Params::Positional(
        values
            .into_iter()
            .map(|v| {
                Ok(match v {
                    KeyValue::Int(n) if n < 0 => Cell::Int(i64::try_from(n)?),
                    KeyValue::Int(n) => Cell::UInt(u64::try_from(n)?),
                    KeyValue::Text(b) | KeyValue::Bytes(b) => Cell::Bytes(b),
                })
            })
            .collect::<Result<_>>()?,
    ))
}
fn text<'a>(v: &'a Value, name: &str) -> Result<&'a str> {
    v[name]
        .as_str()
        .ok_or_else(|| format!("missing {name}").into())
}
fn number(v: &Value, name: &str) -> Result<u64> {
    v[name]
        .as_u64()
        .ok_or_else(|| format!("missing {name}").into())
}

fn incremental_only(config: &Value, input: &Value) -> Result<bool> {
    let parse = |value: &Value| -> Result<bool> {
        Ok(value
            .get("incremental_only")
            .map(|v| v.as_bool().ok_or("incremental_only must be boolean"))
            .transpose()?
            .unwrap_or(false))
    };
    let configured = parse(config)?;
    if configured != parse(&input["c5_layered"])? {
        return Err("C5_INCREMENTAL_ONLY_PIN_MISMATCH".into());
    }
    Ok(configured)
}

fn selected_projection(config: &Value, authority: &Value) -> Result<Vec<String>> {
    let parse = |value: &Value, source_names: bool| -> Result<Vec<String>> {
        let names = value.as_array().ok_or("selected columns")?;
        let mut selected = Vec::with_capacity(names.len());
        for name in names {
            let name = name.as_str().ok_or("selected column")?;
            let valid = if source_names { mysql::mysql_ident(name) } else {
                !name.is_empty() && name.len() <= 130
                    && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            };
            if !valid || selected.iter().any(|n| n == name) {
                return Err("selected column".into());
            }
            selected.push(name.to_owned());
        }
        Ok(selected)
    };
    let configured = config
        .get("columns")
        .map(|value| parse(value, true))
        .transpose()?
        .unwrap_or_default();
    let pinned = authority.get("selected_columns").map(|value| parse(value, false)).transpose()?;
    if let Some(pinned) = pinned {
        if pinned.is_empty() || (!configured.is_empty() && canonical_projection(&configured)? != pinned) {
            return Err("PROJECTION_DRIFT".into());
        }
        if !configured.is_empty() { return Ok(configured); }
        if canonical_projection(&pinned)? != pinned {
            return Err("C5_SOURCE_PROJECTION_REQUIRED".into());
        }
        return Ok(pinned);
    }
    // An all-column fallback is safe only before there is a published baseline.
    if number(authority, "source_generation")? > 0 {
        return Err("C5_PROJECTION_REQUIRED".into());
    }
    Ok(configured)
}

pub(crate) fn canonical_projection(names: &[String]) -> Result<Vec<String>> {
    let reserved = oo_connectors::PG17_RESERVED.iter().map(|name| name.to_string()).collect();
    let mapped = names.iter().map(|name| oo_connectors::stable_ident(name, &reserved))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if mapped.iter().collect::<std::collections::HashSet<_>>().len() != mapped.len() {
        return Err("projection identifier collision".into());
    }
    Ok(mapped)
}

fn projection_digest(columns: &[mysql::map::MappedColumn]) -> Result<String> {
    Ok(sha256_hex(&serde_json::to_vec(
        &canonical_projection(&columns.iter().map(|c| c.plan.name.clone()).collect::<Vec<_>>())?,
    )?))
}

fn scan_cursor(authority: &Value, chunks: &[Value]) -> Result<usize> {
    let next = match authority.get("next_chunk_index") {
        None => 0,
        Some(value) => usize::try_from(value.as_u64().ok_or("authority scan cursor")?)?,
    };
    if next > chunks.len() {
        return Err("authority scan cursor".into());
    }
    Ok(next)
}

fn scan_time(authority: &Value, due: &[bool], next: usize, now: u64) -> Result<u64> {
    if next == 0 && authority["scan_id"].is_null() {
        return Ok(now);
    }
    if text(authority, "scan_id")?.is_empty() {
        return Err("C5_SCAN_CONTEXT_MISMATCH".into());
    }
    let started = number(authority, "scan_observed_at_ms")?;
    if authority["scan_layers"] != json!(due) || started > now {
        return Err("C5_SCAN_CONTEXT_MISMATCH".into());
    }
    Ok(started)
}

fn digest_matches(entry: &Value, current: &Value, codec: &str, projection: &str) -> bool {
    let hex_is = |value: &Value, width: usize| {
        value.as_str().is_some_and(|s| {
            s.len() == width
                && s.bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        })
    };
    entry["digest"].as_array().is_some_and(|d| d.len() == 3)
        && entry["digest"][0].as_u64().is_some()
        && entry["digest"][0] == entry["row_count"]
        && hex_is(&entry["digest"][1], 16)
        && hex_is(&entry["digest"][2], 16)
        && hex_is(&entry["arrow_sha256"], 64)
        && entry["membership_revision"].as_u64().is_some_and(|n| n > 0)
        && entry["digest_codec"] == codec
        && entry["projection_sha256"] == projection
        && entry["digest"] == *current
}

fn needs_capture(
    entry: &Value,
    current: &Value,
    projection: &str,
    force: bool,
    codec: &str,
) -> bool {
    force || !digest_matches(entry, current, codec, projection)
}

fn full_digest_scan_required(
    incremental_only: bool,
    due: &[bool],
    authority: &Value,
    planned: &[Value],
    codec: &str,
    projection: &str,
    policy: mysql::values::TypePolicy,
) -> Result<bool> {
    // Hydration seeds are not source observations. Only target-confirmed scan
    // receipts can end the initial scan; overdue maintenance alone cannot repeat it.
    Ok(due[2]
        || (!incremental_only && due[3])
        || scan_cursor(authority, planned)? > 0
        || authority["schema_fingerprint"].is_null()
        || planned.iter().any(|entry| {
            !digest::policy_matches(entry, policy)
                || !digest_matches(entry, &entry["digest"], codec, projection)
                || (incremental_only
                    && entry["last_digest_scan_id"]
                        .as_str()
                        .is_none_or(str::is_empty))
        }))
}

fn bootstrap_refusal(
    input: &Value,
    authority: &Value,
    chunk: Option<&Value>,
    reason: &str,
) -> Value {
    json!({
        "protocol":"oo.jobs.native-c5-needs-bootstrap/v1","status":"needs_bootstrap",
        "reason":reason,"binding":input["binding"],"pin":input["pin"],
        "source_id":authority["source_id"],"schema":authority["schema"],"table":authority["table"],
        "source_generation":authority["source_generation"],
        "chunk_id":chunk.map(|c| &c["chunk_id"]),
        "data_gen":false,"apply":false,"ack":false,"full_refresh_fallback":false,
        "work":{"full_reconcile_deferred":input["c5_due"][3] == true},
    })
}

fn normalization_reference(
    out: &Path,
    binding: &RunBinding,
    columns: &[mysql::map::MappedColumn],
    batch: &mysql::Batch,
    arrow_hash: &str,
    policy: mysql::values::TypePolicy,
) -> Result<Option<Value>> {
    if batch.rejects.is_empty() {
        return Ok(None);
    }
    let mut records = Vec::new();
    let mut total_bytes = 2usize;
    for (row, column, note) in &batch.rejects {
        let column = columns.get(*column).ok_or("normalization column")?;
        if !digest::normalization_allowed(column, note)
            || (note.reason == "zero_date" && !policy.zero_date_to_null)
            || (note.reason == "nul_byte" && !policy.strip_text_nul)
        {
            return Err("typed rejects".into());
        }
        let record = json!({
            "protocol":oo_connectors::types::TYPE_CONVERSION_PROTOCOL,
            "source_id":binding.source_id,"schema":binding.schema,"table":binding.table,
            "column":column.plan.name,"attnum":column.plan.attnum,"path":column.plan.name,"row_index":row,
            "key_digest":batch.typed.rows.get(*row).and_then(|r|r.key_digest.as_ref()).map(|key|hex(key)),
            "reason":note.reason,"value_len":note.value_len,"run_generation":binding.run_generation,
            "input_digest":arrow_hash,"type_policy_version":oo_core::TYPE_POLICY_V1,
            "raw_value_recorded":false,
        });
        let size = serde_json::to_vec(&record)?.len();
        total_bytes += size + 1;
        if size > 1024 || total_bytes > 33_554_432 || records.len() == 100_000 {
            return Err("normalization notes budget".into());
        }
        records.push(record);
    }
    let value = json!(records);
    let bytes = serde_json::to_vec(&value)?;
    let hash = sha256_hex(&bytes);
    let path = out.join(format!("normalization-{hash}.json"));
    if path.exists() {
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_file() || metadata.len() != bytes.len() as u64 || fs::read(&path)? != bytes
        {
            return Err("normalization evidence changed".into());
        }
    } else {
        c5plus::save(&path, &value)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(Some(json!({"path":path,"sha256":hash})))
}

fn observed_chunk(entry: &Value, current: Value, codec: &str, projection: &str, now: u64) -> Value {
    let mut updated = entry.clone();
    updated["digest"] = current;
    updated["digest_codec"] = json!(codec);
    updated["projection_sha256"] = json!(projection);
    updated["scanned_at_ms"] = json!(now);
    updated
}

fn captured_chunk(
    mut updated: Value,
    digest: Value,
    hash: &str,
    rows: u64,
) -> Result<(Value, bool)> {
    if digest[0].as_u64() != Some(rows) {
        return Err("capture digest rows".into());
    }
    updated["digest"] = digest;
    updated["row_count"] = json!(rows);
    let revision = number(&updated, "membership_revision")?;
    let unchanged = revision > 0 && updated["arrow_sha256"] == hash;
    if !unchanged {
        updated["arrow_sha256"] = json!(hash);
        updated["membership_revision"] =
            json!(revision.checked_add(1).ok_or("membership revision")?);
    }
    Ok((updated, unchanged))
}

fn observation(
    authority: &Value,
    columns: &[mysql::map::MappedColumn],
    fingerprint: &str,
    chunks: &[Value],
    next: usize,
    now: u64,
) -> Value {
    json!({
        "source_id":authority["source_id"],"schema":authority["schema"],"table":authority["table"],
        "parent_generation":authority["source_generation"],
        "scan_id":authority["scan_id"],"expected_chunks":authority["expected_chunks"],
        "chunk_topology_revision":authority["chunk_topology_revision"],
        "schema_fingerprint":fingerprint,
        "selected_columns":columns.iter().map(|c| &c.plan.name).collect::<Vec<_>>(),
        "checked_at_ms":now,"chunks":chunks,"next_chunk_index":next,
        "full_digest_scan_completed":false,
    })
}

fn initial_full_read_qualification(
    authority: &Value,
    planned: &[Value],
    proofs: &[Value],
) -> Option<Value> {
    if proofs.is_empty() {
        return None;
    }
    let expected = planned
        .iter()
        .map(|c| c["chunk_id"].clone())
        .collect::<Vec<_>>();
    let complete = expected.len() == proofs.len()
        && expected
            .iter()
            .all(|id| proofs.iter().filter(|p| p["chunk_id"] == *id).count() == 1);
    Some(json!({
        "protocol":"oo.mysql.c5-initial-full-read-qualification/v1",
        "scan_id":authority["scan_id"],"source_id":authority["source_id"],
        "schema":authority["schema"],"table":authority["table"],
        "baseline_source_generation":authority["baseline"]["source_generation"],
        "expected_chunks":expected,"parts":proofs,"complete":complete,
        "uses_sealed_baseline_time":true,"current_full_read_completed":false,
    }))
}

#[derive(Default)]
struct ScanWork {
    full_reconcile_deferred: bool,
    digest_blocks: u64,
    digest_rows: u64,
    recent_probe_blocks: u64,
    recent_probe_rows: u64,
    captured_blocks: u64,
    captured_rows: u64,
    captured_bytes: u64,
    bootstrap_blocks: u64,
    bootstrap_rows: u64,
    bootstrap_bytes: u64,
    arrow_equal_blocks: u64,
    baseline_digest_blocks: u64,
    baseline_digest_rows: u64,
    baseline_digest_bytes: u64,
    baseline_digest_skips: u64,
}

impl ScanWork {
    fn json(&self) -> Value {
        let mode = if self.bootstrap_blocks > 0 {
            "bounded_bootstrap"
        } else if self.captured_blocks > 0 {
            "changed_ranges"
        } else if self.baseline_digest_blocks > 0 {
            "local_baseline_digest"
        } else if self.recent_probe_blocks > 0 {
            "recent_probe"
        } else if self.digest_blocks > 0 {
            "digest_only"
        } else {
            "metadata_only"
        };
        json!({
            "digest_blocks":self.digest_blocks,"digest_rows":self.digest_rows,
            "recent_probe_blocks":self.recent_probe_blocks,"recent_probe_rows":self.recent_probe_rows,
            "captured_blocks":self.captured_blocks,"captured_rows":self.captured_rows,
            "captured_bytes":self.captured_bytes,"bootstrap_blocks":self.bootstrap_blocks,
            "bootstrap_rows":self.bootstrap_rows,"bootstrap_bytes":self.bootstrap_bytes,
            "arrow_equal_blocks":self.arrow_equal_blocks,
            "baseline_digest_blocks":self.baseline_digest_blocks,"baseline_digest_rows":self.baseline_digest_rows,
            "baseline_digest_bytes":self.baseline_digest_bytes,"baseline_digest_skips":self.baseline_digest_skips,
            "payload_mode":mode,"full_refresh_fallback":false,"baseline_digest_shortcut":self.baseline_digest_skips>0,
            "full_reconcile_deferred":self.full_reconcile_deferred,
            "byte_accounting":"decoded_source_cells_plus_row_hash_and_keyset_cursor",
        })
    }

    fn captured(&mut self, result: &Value, bootstrap: bool) -> Result<()> {
        let rows = number(&result["c5_capture_work"], "rows")?;
        let bytes = number(&result["c5_capture_work"], "bytes")?;
        self.captured_blocks += 1;
        self.captured_rows += rows;
        self.captured_bytes += bytes;
        if bootstrap {
            self.bootstrap_blocks += 1;
            self.bootstrap_rows += rows;
            self.bootstrap_bytes += bytes;
        }
        if result["unchanged"] == true {
            self.arrow_equal_blocks += 1;
        }
        Ok(())
    }
}

pub(crate) fn catalog_fingerprint(catalog: &[mysql::map::MysqlColumn]) -> Result<String> {
    // Exact full.rs chunk-binding schema_sha256 encoding, before projection or policy.
    let preimage = catalog.iter().map(|c| json!({
        "name":c.name,"ordinal":c.ordinal,"data_type":c.data_type,"column_type":c.column_type,
        "nullable":c.nullable,"char_len":c.char_len,"precision":c.precision,"scale":c.scale,
        "charset":c.charset,"column_key":c.column_key,
    })).collect::<Vec<_>>();
    Ok(sha256_hex(&serde_json::to_vec(&preimage)?))
}

fn full_catalog_fingerprint(config: &Value) -> Result<bool> {
    match config["schema_fingerprint_codec"].as_str() {
        None if config.get("schema_fingerprint_codec").is_none() => Ok(false),
        Some("mysql-layered-columns-keys/v1") => Ok(false),
        Some("mysql-full-catalog/v1") => {
            let keys: Vec<chunk::KeyColumn> =
                serde_json::from_value(config["baseline_keys"].clone())?;
            if keys.is_empty() || json!(keys) != config["baseline_keys"] {
                return Err("baseline_keys binding".into());
            }
            Ok(true)
        }
        _ => Err("schema_fingerprint_codec".into()),
    }
}

fn check_baseline_keys(config: &Value, keys: &[chunk::KeyColumn]) -> Result<()> {
    if full_catalog_fingerprint(config)? && config["baseline_keys"] != json!(keys) {
        return Err("KEY_METADATA_DRIFT".into());
    }
    Ok(())
}

async fn full_schema_fingerprint(conn: &mut Conn, table: &mysql::SelectedTable) -> Result<String> {
    catalog_fingerprint(&mysql::catalog_columns(conn, &mut vec![], table).await?)
}

async fn schema_fingerprint(
    conn: &mut Conn,
    table: &mysql::SelectedTable,
    keys: &[chunk::KeyColumn],
) -> Result<String> {
    let columns:Vec<(String,String,Option<String>,String)>=conn.exec("SELECT COLUMN_NAME,COLUMN_TYPE,COLLATION_NAME,IS_NULLABLE FROM information_schema.COLUMNS WHERE TABLE_SCHEMA=? AND TABLE_NAME=? ORDER BY ORDINAL_POSITION",(&table.schema,&table.name)).await?;
    Ok(sha256_hex(&serde_json::to_vec(
        &json!({"columns":columns,"keys":keys}),
    )?))
}

async fn capture_schema_fingerprint(
    config: &Value,
    conn: &mut Conn,
    table: &mysql::SelectedTable,
    keys: &[chunk::KeyColumn],
) -> Result<String> {
    check_baseline_keys(config, keys)?;
    if full_catalog_fingerprint(config)? {
        full_schema_fingerprint(conn, table).await
    } else {
        schema_fingerprint(conn, table, keys).await
    }
}

pub async fn produce(
    mut conn: Conn,
    config: &Value,
    input: &Value,
    authority: &Value,
    out: &Path,
) -> Result<Value> {
    let produce_started = std::time::Instant::now();
    if input["protocol"] != "oo.jobs.native-producer/v1"
        || input["method"] != "C5"
        || input["c5_mode"] != "layered"
        || input["source_id"] != config["source_id"]
        || authority["source_generation"] != input["acknowledged_source_generation"]
        || authority["source_id"] != config["source_id"]
        || authority["schema"] != config["schema"]
        || authority["table"] != config["table"]
    {
        return Err("occurrence/authority mismatch".into());
    }
    if authority["chunk_topology_revision"]
        .as_u64()
        .is_none_or(|n| n == 0)
    {
        return Err("topology revision".into());
    }
    let incremental_only = incremental_only(config, input)?;
    let boolean_tinyint1 = config
        .get("boolean_tinyint1")
        .map(|v| v.as_bool().ok_or("boolean_tinyint1 must be boolean"))
        .transpose()?
        .unwrap_or(false);
    let type_policy = digest::type_policy(config)?;
    let empty_only = empty::selected(authority)?;
    let historical_fingerprint = if empty_only {
        false
    } else {
        full_catalog_fingerprint(config)?
    };
    if config.get("range_max_rows").is_some() {
        return Err(
            "C5_RANGE_REPARTITION_REQUIRED: range_max_rows would change sealed admission budget"
                .into(),
        );
    }
    let schema = text(config, "schema")?;
    let table = text(config, "table")?;
    if !mysql::mysql_ident(schema) || !mysql::mysql_ident(table) {
        return Err("identifier".into());
    }
    let selected = mysql::SelectedTable {
        schema: schema.into(),
        name: table.into(),
        columns: selected_projection(config, authority)?,
    };
    let layer = &input["c5_layered"];
    let policy = &layer["policy"];
    if layer["execution_timezone"] != "UTC" {
        return Err("timezone".into());
    }
    let duty = number(policy, "duty_per_mille")?;
    if !(1..=1000).contains(&duty)
        || number(policy, "threads_running_max")? == 0
        || !(1..=30000).contains(&number(layer, "server_timeout_ms")?)
        || number(layer, "client_timeout_ms")? <= number(layer, "server_timeout_ms")?
        || !(1..=100_000).contains(&number(config, "max_rows")?)
        || !(1..=MAX_DATA_BYTES).contains(&number(config, "max_bytes")?)
    {
        return Err("budget".into());
    }
    let timeout = Duration::from_millis(number(layer, "client_timeout_ms")?);
    conn.query_drop("SET time_zone = '+00:00'").await?;
    conn.query_drop(format!(
        "SET SESSION max_execution_time={}",
        number(layer, "server_timeout_ms")?
    ))
    .await?;
    let facts: Option<(u8, String)> = conn.query_first("SELECT @@log_bin,VERSION()").await?;
    validate_c5_source_facts(facts)?;
    loop {
        let load: Option<(String, String)> = tokio::time::timeout(
            timeout,
            conn.query_first("SHOW GLOBAL STATUS LIKE 'Threads_running'"),
        )
        .await??;
        if load.ok_or("load")?.1.parse::<u64>()? <= number(policy, "threads_running_max")? {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let grants: Vec<String> = conn.query("SHOW GRANTS FOR CURRENT_USER()").await?;
    mysql::full_read_grants(&grants)?;
    if empty_only {
        return empty::produce(conn, config, input, authority, timeout).await;
    }
    if input["c5_due"][0] == true {
        c5plus::table_hints(&mut conn, schema).await?;
    }
    if let Some(recent) = layer["recent_column"].as_str() {
        let indexed:Option<u64>=conn.exec_first("SELECT COUNT(*) FROM information_schema.STATISTICS WHERE TABLE_SCHEMA=? AND TABLE_NAME=? AND COLUMN_NAME=? AND SEQ_IN_INDEX=1",(schema,table,recent)).await?;
        if indexed.unwrap_or(0) == 0 {
            return Err("indexed recent column required".into());
        }
    }
    let keys = chunk::key_columns(&mut conn, schema, table).await?;
    if keys.is_empty() {
        return Err("stable key required".into());
    }
    check_baseline_keys(config, &keys)?;
    let source_key_columns = json!(keys.iter().map(|key| &key.name).collect::<Vec<_>>());
    if authority["baseline"].get("key_columns").is_some()
        && authority["baseline"]["key_columns"] != source_key_columns
    {
        return Err("KEY_ORDER_DRIFT".into());
    }
    let mut catalog = mysql::catalog_columns(&mut conn, &mut vec![], &selected).await?;
    let fingerprint = if historical_fingerprint {
        catalog_fingerprint(&catalog)?
    } else {
        schema_fingerprint(&mut conn, &selected, &keys).await?
    };
    // catalog_columns returns the whole catalog, regardless of SelectedTable.columns.
    if !selected.columns.is_empty() {
        catalog = selected
            .columns
            .iter()
            .map(|name| {
                catalog
                    .iter()
                    .find(|c| c.name == *name)
                    .cloned()
                    .ok_or_else(|| "PROJECTION_DRIFT".into())
            })
            .collect::<Result<Vec<_>>>()?;
    }
    if keys
        .iter()
        .any(|key| !catalog.iter().any(|c| c.name == key.name))
    {
        return Err("projection must include every key".into());
    }
    let reserved = oo_connectors::PG17_RESERVED
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut columns = catalog
        .iter()
        .map(|c| mysql::map::map_capture_column(c, boolean_tinyint1, true, &reserved))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for column in &mut columns {
        column.key = keys.iter().any(|k| k.name == column.plan.name);
        column.plan.key = column.key;
    }
    digest::apply_type_policy(&mut columns, type_policy);
    let codec = digest::codec(&columns);
    let projection = projection_digest(&columns)?;
    if authority["schema_fingerprint"]
        .as_str()
        .is_some_and(|old| old != fingerprint)
    {
        return Err("SCHEMA_DRIFT".into());
    }
    if incremental_only && codec != digest::ARROW_SHA256 {
        conn.disconnect().await?;
        return Ok(bootstrap_refusal(
            input,
            authority,
            None,
            "unsupported_columns",
        ));
    }
    let max_rows = number(config, "max_rows")?;
    let mut planned = authority["chunks"]
        .as_array()
        .ok_or("authority chunks")?
        .clone();
    if !authority["scan_id"].is_null()
        && authority["expected_chunks"]
            != json!(planned.iter().map(|c| &c["chunk_id"]).collect::<Vec<_>>())
    {
        return Err("C5_SCAN_CONTEXT_MISMATCH".into());
    }
    for (index, chunk) in planned.iter().enumerate() {
        if chunk["chunk_id"]
            .as_str()
            .and_then(|id| id.parse::<usize>().ok())
            != Some(index)
            || chunk["hi"].is_null() && index + 1 != planned.len()
            || (index == 0 && !chunk["lo"].is_null())
            || (index > 0 && chunk["lo"] != planned[index - 1]["hi"])
        {
            return Err("authority topology".into());
        }
    }
    let lo = planned
        .last()
        .map(|c| decode_bound(&c["hi"]))
        .transpose()?
        .flatten();
    let boundary_started = std::time::Instant::now();
    let hi = if planned.last().is_some_and(|chunk| chunk["hi"].is_null()) {
        None
    } else {
        tokio::time::timeout(
            timeout,
            chunk::next_boundary(&mut conn, schema, table, &keys, lo.clone(), max_rows),
        )
        .await??
    };
    tokio::time::sleep(
        boundary_started
            .elapsed()
            .mul_f64((1000.0 - duty as f64) / duty as f64),
    )
    .await;
    if let Some(hi) = hi {
        let id = planned.len().to_string();
        planned.push(json!({"chunk_id":id,"lo":encode_bound(lo.as_deref()),"hi":encode_bound(Some(&hi)),"membership_revision":0}));
    }
    let mut budget = c5plus::Budget::default();
    budget.sleep_factor = (1000.0 - number(policy, "duty_per_mille")? as f64)
        / number(policy, "duty_per_mille")? as f64;
    budget.max_threads = number(policy, "threads_running_max")?;
    budget.client_timeout = timeout;
    budget.max_execution_ms = u32::try_from(number(layer, "server_timeout_ms")?)?;
    let w = &policy["execution_window_utc"];
    let start = w[0].as_u64().ok_or("window")?;
    let end = w[1].as_u64().ok_or("window")?;
    let window = c5plus::Window::parse(&format!(
        "{:02}:{:02}-{:02}:{:02}",
        start / 60,
        start % 60,
        (end % 1440) / 60,
        end % 60
    ))?;
    let mut scanner = c5plus::Scanner::new(conn, budget, c5plus::Window::parse("00:00-00:00")?);
    let source_schema = scanner.schema(schema, table).await?;
    let due: Vec<bool> = input["c5_due"]
        .as_array()
        .ok_or("due")?
        .iter()
        .map(|v| v.as_bool().ok_or("due"))
        .collect::<std::result::Result<_, _>>()?;
    if due.len() != 4 {
        return Err("due".into());
    }
    let now = number(input, "c5_observed_at_ms")?;
    let next = scan_cursor(authority, &planned)?;
    let scan_observed_at = scan_time(authority, &due, next, now)?;
    let recent_window = if due[1] && !layer["recent_column"].is_null() {
        let column = text(layer, "recent_column")?;
        if !mysql::mysql_ident(column) {
            return Err("recent column".into());
        }
        let lower = scan_observed_at.saturating_sub(
            number(layer, "window_ms")?
                .checked_add(number(layer, "overlap_ms")?)
                .ok_or("window")?,
        );
        Some((
            column,
            lower as f64 / 1000.0,
            scan_observed_at as f64 / 1000.0,
        ))
    } else {
        None
    };
    let full_scan = full_digest_scan_required(
        incremental_only,
        &due,
        authority,
        &planned,
        codec,
        &projection,
        type_policy,
    )?;
    scanner.window = if full_scan {
        window
    } else {
        c5plus::Window::parse("00:00-00:00")?
    };
    let key_names = keys.iter().map(|k| k.name.as_str()).collect::<Vec<_>>();
    if planned[..next]
        .iter()
        .any(|entry| !digest_matches(entry, &entry["digest"], codec, &projection))
    {
        return Err("C5_SCAN_RECEIPT_REQUIRED".into());
    }
    // Legacy exports omit policy: rehash that prefix, never reread its old archive.
    let scan_start = planned[..next]
        .iter()
        .position(|entry| !digest::policy_matches(entry, type_policy))
        .unwrap_or(next);
    let mut observed = planned[..scan_start].to_vec();
    let mut work = ScanWork {
        full_reconcile_deferred: incremental_only && due[3],
        ..ScanWork::default()
    };
    let mut baseline_proofs = Vec::new();
    let mut initial_full_read_proofs = Vec::new();
    if !full_scan && recent_window.is_none() {
        observed = planned.clone();
    }

    for (index, entry) in planned
        .iter()
        .enumerate()
        .skip(scan_start)
        .filter(|_| full_scan || recent_window.is_some())
    {
        scanner.check_schema(schema, table, &source_schema).await?;
        let bound = ChunkBound {
            lo: decode_bound(&entry["lo"])?,
            hi: decode_bound(&entry["hi"])?,
        };
        let (where_sql, values) = digest::source_predicate(&keys, &bound)?;
        let predicate = where_sql.strip_prefix(" WHERE ").unwrap_or("1");
        if !full_scan
            && digest::policy_matches(entry, type_policy)
            && digest_matches(entry, &entry["digest"], codec, &projection)
        {
            let Some((column, lower, upper)) = recent_window else {
                observed.push(entry.clone());
                continue;
            };
            let Params::Positional(mut near) = params(values.clone())? else {
                return Err("params".into());
            };
            near.extend([Cell::Double(lower), Cell::Double(upper)]);
            let hits=scanner.rows(schema,table,&key_names,&format!("({predicate}) AND `{column}` >= FROM_UNIXTIME(?) AND `{column}` <= FROM_UNIXTIME(?)"),Params::Positional(near)).await?;
            work.recent_probe_blocks += 1;
            work.recent_probe_rows += hits.len() as u64;
            if hits.is_empty() {
                observed.push(entry.clone());
                continue;
            }
        }
        // Receipt validity is independent of policy. Only a fresh normalized
        // source digest comparison below can turn this cache into nochange.
        let trusted = digest_matches(entry, &entry["digest"], codec, &projection);
        let baseline = if trusted {
            None
        } else {
            match digest::baseline_digest_proof(config, authority, entry, &columns) {
                Ok(proof) => proof,
                Err(_) if incremental_only => {
                    scanner.into_connection().disconnect().await?;
                    return Ok(bootstrap_refusal(
                        input,
                        authority,
                        Some(entry),
                        "baseline_arrow_invalid",
                    ));
                }
                Err(error) => return Err(error),
            }
        };
        if !trusted && baseline.is_none() && incremental_only {
            scanner.into_connection().disconnect().await?;
            return Ok(bootstrap_refusal(
                input,
                authority,
                Some(entry),
                "baseline_arrow_required",
            ));
        }
        if let Some(proof) = &baseline {
            work.baseline_digest_blocks += 1;
            work.baseline_digest_rows += number(proof, "row_count")?;
            work.baseline_digest_bytes += number(proof, "archive_bytes")?;
            baseline_proofs.push(proof.clone());
        }
        // Only maintenance jobs can qualify or execute a typed L4 reconciliation.
        let initial_full_read = !incremental_only
            && due[3]
            && entry["digest"].is_null()
            && baseline
                .as_ref()
                .is_some_and(|p| p["membership_revision"] == 1);
        if initial_full_read {
            initial_full_read_proofs.push(baseline.as_ref().ok_or("baseline proof")?.clone());
        }
        let sql = digest::aggregate_sql(
            schema,
            table,
            &keys,
            &columns,
            predicate,
            u32::try_from(number(layer, "server_timeout_ms")?)?,
            codec,
        )?;
        let digest = scanner.digest(&sql, params(values)?).await?.0;
        let digest = json!([
            digest[0],
            format!("{:016x}", digest[1]),
            format!("{:016x}", digest[2])
        ]);
        work.digest_blocks += 1;
        work.digest_rows += digest[0].as_u64().ok_or("digest rows")?;
        let mut updated = observed_chunk(entry, digest.clone(), codec, &projection, now);
        updated["type_policy"] = json!(type_policy);
        updated
            .as_object_mut()
            .ok_or("observation")?
            .remove("normalization_rejects");
        let baseline_equal = baseline
            .as_ref()
            .is_some_and(|proof| proof["digest"] == digest);
        let force_capture = !incremental_only && due[3] && !initial_full_read;
        if (trusted && !needs_capture(entry, &digest, &projection, force_capture, codec))
            || (baseline_equal && !force_capture)
        {
            if baseline_equal {
                work.baseline_digest_skips += 1;
            }
            observed.push(updated);
            continue;
        }
        let generation = number(authority, "source_generation")?
            .checked_add(1)
            .ok_or("generation")?;
        let binding = RunBinding {
            source_id: text(config, "source_id")?.into(),
            schema: schema.into(),
            table: table.into(),
            run_generation: generation,
            input_digest: [0; 32],
        };
        let conn = scanner.into_connection();
        let (conn, result) = read_request(
            config,
            input,
            authority,
            out,
            RequestRead {
                conn,
                binding: &binding,
                keys: &keys,
                columns: &columns,
                bound: &bound,
                updated,
                fingerprint: fingerprint.clone(),
                deadline: timeout,
            },
        )
        .await?;
        work.captured(&result, !trusted && baseline.is_none())?;
        observed.push(result["c5_observation"]["chunk"].clone());
        if result["unchanged"] == true {
            let mut budget = c5plus::Budget::default();
            budget.sleep_factor = (1000.0 - number(policy, "duty_per_mille")? as f64)
                / number(policy, "duty_per_mille")? as f64;
            budget.max_threads = number(policy, "threads_running_max")?;
            budget.client_timeout = timeout;
            budget.max_execution_ms = u32::try_from(number(layer, "server_timeout_ms")?)?;
            scanner = c5plus::Scanner::new(
                conn,
                budget,
                if full_scan {
                    window
                } else {
                    c5plus::Window::parse("00:00-00:00")?
                },
            );
            continue;
        }
        conn.disconnect().await?;
        let mut result = result;
        result
            .as_object_mut()
            .ok_or("range result")?
            .remove("c5_capture_work");
        let mut receipt = observation(authority, &columns, &fingerprint, &observed, index + 1, now);
        receipt["chunk"] = result["c5_observation"]["chunk"].clone();
        receipt["work"] = work.json();
        receipt["type_policy"] = json!(type_policy);
        receipt["key_columns"] = source_key_columns.clone();
        receipt["work"]["elapsed_ms"] = json!(produce_started.elapsed().as_millis());
        receipt["baseline_digest_proofs"] = json!(baseline_proofs);
        if let Some(qualification) =
            initial_full_read_qualification(authority, &planned, &initial_full_read_proofs)
        {
            receipt["baseline_full_read_qualification"] = qualification;
        }
        receipt["scan_layers"] = json!(due);
        receipt["scan_observed_at_ms"] = json!(scan_observed_at);
        result["c5_observation"] = receipt;
        return Ok(result);
    }
    scanner.into_connection().disconnect().await?;
    let digest = sha256_hex(&serde_json::to_vec(&observed.iter().map(|c|json!({"chunk_id":c["chunk_id"],"lo":c["lo"],"hi":c["hi"],"digest":c["digest"],"digest_codec":c["digest_codec"],"projection_sha256":c["projection_sha256"]})).collect::<Vec<_>>())?);
    let parent = authority["input_digest"].as_str().unwrap_or(&digest);
    let mut receipt = observation(authority, &columns, &fingerprint, &observed, 0, now);
    receipt["work"] = work.json();
    receipt["type_policy"] = json!(type_policy);
    receipt["key_columns"] = source_key_columns.clone();
    receipt["work"]["elapsed_ms"] = json!(produce_started.elapsed().as_millis());
    receipt["baseline_digest_proofs"] = json!(baseline_proofs);
    if let Some(qualification) =
        initial_full_read_qualification(authority, &planned, &initial_full_read_proofs)
    {
        receipt["baseline_full_read_qualification"] = qualification;
    }
    receipt["scan_layers"] = json!(due);
    receipt["scan_observed_at_ms"] = json!(scan_observed_at);
    receipt["full_digest_scan_completed"] = json!(full_scan);
    if !full_scan && work.digest_blocks == 0 {
        // A deferred L4 tick must not turn old receipts into fresh observations.
        receipt["chunks"] = json!([]);
    }
    receipt["recent_window_watermark_ms"] = if due[1] && recent_window.is_some() {
        json!(scan_observed_at)
    } else {
        Value::Null
    };
    let mut completed = due.clone();
    // A resumed scan needs worker-verified coverage across occurrences.
    completed[3] = !incremental_only
        && due[3]
        && next == 0
        && work.captured_blocks == planned.len() as u64
        && initial_full_read_proofs.is_empty();
    Ok(
        json!({"protocol":"oo.jobs.native-c5-nochange/v1","binding":input["binding"],"pin":input["pin"],"source_id":config["source_id"],"method":"C5","capture_method":"c5_diff",
        "acknowledged_source_generation":authority["source_generation"],"source_generation":authority["source_generation"],"empty_diff":true,"data_gen":false,"apply":false,"ack":false,
        "position_codec":"snapshot-zero/v1","source_digest":digest,"parent_digest":parent,"snapshot":{"kind":"c5_cycle","value":digest},"c5_completed_layers":completed,"c5_observation":receipt,
        "key_columns":source_key_columns,"type_policy":type_policy}),
    )
}

struct RequestRead<'a> {
    conn: Conn,
    binding: &'a RunBinding,
    keys: &'a [chunk::KeyColumn],
    columns: &'a [mysql::map::MappedColumn],
    bound: &'a ChunkBound,
    updated: Value,
    fingerprint: String,
    deadline: Duration,
}

async fn read_request(
    config: &Value,
    input: &Value,
    authority: &Value,
    out: &Path,
    read: RequestRead<'_>,
) -> Result<(Conn, Value)> {
    range_parts::read_request(config, input, authority, out, read).await
}

fn save_full_preimage(path: &Path, value: &Value) -> Result<()> {
    // Match full.rs::write byte-for-byte: pretty JSON, without a trailing newline.
    let bytes = serde_json::to_vec_pretty(value)?;
    let temporary = path.with_extension("json.pending");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    use std::io::Write;
    file.write_all(&bytes)?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

fn write_preimages(out: &Path, config: &Value, columns: &[mysql::map::MappedColumn]) -> Result<()> {
    let mut fields = Vec::new();
    let mut types = Vec::new();
    for c in columns {
        let (kind, precision, scale) =
            crate::capture::arrow_description(c.plan.arrow.as_ref().ok_or("range type")?)?;
        let field = json!({"name":c.plan.name,"arrow_type":kind,"nullable":c.plan.mapping.nullable,"precision":precision,"scale":scale,"key":c.key,"document":c.oid==114,"char_length":if c.oid==1042 {Some(c.typmod-4)} else {None}});
        fields.push(field.clone());
        let mut typ = field;
        typ["table"] = config["table"].clone();
        typ["source_type"] = json!(c.plan.mapping.source_type);
        typ["target_oid"] = json!(c.oid);
        typ["target_typmod"] = json!(c.typmod);
        typ["caveat"] = json!(c.caveat);
        typ.as_object_mut().ok_or("range type")?.remove("nullable");
        typ.as_object_mut().ok_or("range type")?.remove("key");
        types.push(typ);
    }
    let save_preimage = if full_catalog_fingerprint(config)? {
        save_full_preimage
    } else {
        c5plus::save
    };
    save_preimage(
        &out.join("schema-preimage.json"),
        &json!([{"table":config["table"],"columns":fields}]),
    )?;
    save_preimage(&out.join("type-preimage.json"), &json!(types))?;
    let driver = std::env::current_exe()?;
    c5plus::save(
        &out.join("driver.json"),
        &json!({"protocol":"oo.mysql.capture-driver/v1","executable_path":driver,"executable_sha256":chunk::file_sha256(&driver)?}),
    )?;
    for name in ["schema-preimage.json", "type-preimage.json", "driver.json"] {
        fs::set_permissions(out.join(name), fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
fn write_capture(
    out: &Path,
    config: &Value,
    request: &Value,
    columns: &[mysql::map::MappedColumn],
    audit: &Value,
    run_id: &str,
) -> Result<()> {
    let mut scope = request.clone();
    for key in [
        "input_digest",
        "arrow_path",
        "consistency_grade",
        "cross_table_consistent",
        "target",
        "c5_completed_layers",
        "c5_observation",
        "binding",
        "pin",
        "key_columns",
        "type_policy",
        "normalization_rejects",
    ] {
        scope.as_object_mut().ok_or("range scope")?.remove(key);
    }
    oo_pipeline::native_c5_range::RangeScope::parse(&scope)?;
    write_preimages(out, config, columns)?;
    c5plus::save(
        &out.join("capturer.json"),
        &json!({"protocol":"oo.mysql.full-capture/v1","source_engine":"mysql","database":config["schema"],"source_id":config["source_id"],"source_generation":request["source_generation"],"run_id":run_id,"label":run_id,"snapshot":{"kind":"mysql_consistent_snapshot","value":run_id},"snapshot_token":false,"snapshot_complete":true,"cross_table_consistency":"B","product_reader_write_attempts":0,"source_audit":audit,"observed_privileges":audit["privileges"].as_array().cloned().unwrap_or_else(||vec![json!("SELECT")]),"max_rows":config["max_rows"],"max_bytes":config["max_bytes"],"config_sha256":sha256_hex(&serde_json::to_vec(config)?),"range_scope":scope,"key_columns":request["key_columns"],"type_policy":request["type_policy"],"normalization_rejects":request["normalization_rejects"],"tables":[{"name":config["table"],"schema":config["schema"],"arrow":Path::new(text(request,"arrow_path")?).file_name().and_then(|s|s.to_str()).ok_or("Arrow path")?,"arrow_path":request["arrow_path"],"arrow_sha256":request["arrow_sha256"],"row_count":request["row_count"],"route":"C5","consistency":"B","delete_mode":"reconcile","cursor_safety":"no_cursor","quality":"RANGE_SNAPSHOT"}]}),
    )?;
    Ok(())
}

pub async fn connect(config: &Value) -> Result<Conn> {
    let c = &config["connection"];
    let password = crate::source_sql::secret_file(Path::new(text(c, "secret_file")?))?;
    let mut conn = if c["tls"] == "disabled" {
        if c["plaintext_authorized"] != true {
            return Err("plaintext authorization".into());
        }
        tokio::time::timeout(
            Duration::from_secs(10),
            Conn::new(
                mysql_async::OptsBuilder::default()
                    .ip_or_hostname(text(c, "host")?)
                    .tcp_port(u16::try_from(number(c, "port")?)?)
                    .user(Some(text(c, "user")?))
                    .pass(Some(password.to_string()))
                    .db_name(Some(text(config, "schema")?))
                    .prefer_socket(false)
                    .ssl_opts(None::<mysql_async::SslOpts>),
            ),
        )
        .await??
    } else {
        let source = mysql::MysqlConfig {
            mysql_host: text(c, "host")?.into(),
            mysql_port: u16::try_from(number(c, "port")?)?,
            mysql_database: text(config, "schema")?.into(),
            mysql_user: text(c, "user")?.into(),
            mysql_ca_file: text(c, "ca_file")?.into(),
            mysql_tls_server_name: None,
            secret_ref: oo_connectors::SecretRef::new(text(config, "secret_ref")?)?,
            connect_timeout_ms: 10000,
        };
        mysql::connect(&source, &password).await?
    };
    conn.query_drop("SET time_zone = '+00:00'").await?;
    if c["tls"] == "disabled" {
        let ssl: Option<String> = conn.query_first("SELECT @@have_ssl").await?;
        if ssl.as_deref() != Some("DISABLED") {
            return Err("plaintext source facts".into());
        }
    }
    Ok(conn)
}

pub(crate) fn validate_c5_source_facts(facts: Option<(u8, String)>) -> Result<()> {
    let (log_bin, version) = facts.ok_or("source facts")?;
    crate::version::ServerVersion::parse(&version)?;
    // Enabling logs must not interrupt an explicitly selected read-only C5
    // service while its separately qualified C1 handoff is still pending.
    if log_bin > 1 {
        return Err("source facts".into());
    }
    Ok(())
}

#[cfg(test)]
mod source_mode_tests {
    use super::*;
    #[test]
    fn enabling_binlog_does_not_disable_explicit_c5() {
        for log_bin in [0, 1] {
            validate_c5_source_facts(Some((log_bin, "5.7.28-log".into()))).unwrap();
        }
        assert!(validate_c5_source_facts(None).is_err());
        assert!(validate_c5_source_facts(Some((2, "5.7.28-log".into()))).is_err());
        assert!(validate_c5_source_facts(Some((1, "5.7.28-MariaDB".into()))).is_err());
    }
}

pub async fn run_stdio(config_path: &Path) -> Result<()> {
    use std::io::{Read, Write};
    let config_bytes = fs::read(config_path)?;
    let config: Value = serde_json::from_slice(&config_bytes)?;
    let mut bytes = Vec::new();
    std::io::stdin().take(1024 * 1024).read_to_end(&mut bytes)?;
    let input: Value = serde_json::from_slice(&bytes)?;
    incremental_only(&config, &input)?;
    let root = Path::new(text(&config, "state_dir")?);
    if !root.is_absolute() {
        return Err("absolute state directory required".into());
    }
    let binding = text(&input, "binding")?;
    if binding.len() != 36 || !binding.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return Err("binding".into());
    }
    let pin = number(&input, "pin")?;
    let out = root.join(format!("{binding}_{pin}"));
    fs::create_dir_all(&out)?;
    fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
    fs::set_permissions(&out, fs::Permissions::from_mode(0o700))?;
    let stopped = root.join(format!("{binding}.schema-drift.json"));
    if stopped.exists() {
        return Err("SCHEMA_DRIFT".into());
    }
    let request = out.join("request.json");
    let config_hash = sha256_hex(&config_bytes);
    let pinned_config = out.join("config.sha256");
    if pinned_config.exists() {
        if fs::read_to_string(&pinned_config)? != config_hash {
            return Err("config changed".into());
        }
    } else {
        fs::write(&pinned_config, &config_hash)?;
    }
    let input_path = out.join("input.json");
    if input_path.exists() {
        if serde_json::from_slice::<Value>(&fs::read(&input_path)?)? != input {
            return Err("pin changed".into());
        }
    } else {
        c5plus::save(&input_path, &input)?;
    }
    let value = if request.exists() {
        let value: Value = serde_json::from_slice(&fs::read(&request)?)?;
        if value["c5_observation"]["protocol"] == empty::OBSERVATION {
            let authority: Value =
                serde_json::from_slice(&fs::read(text(&config, "authority_file")?)?)?;
            empty::validate_replay(&config, &authority, &value)?;
        }
        value
    } else {
        let authority: Value =
            serde_json::from_slice(&fs::read(text(&config, "authority_file")?)?)?;
        let conn = connect(&config).await?;
        let result = match produce(conn, &config, &input, &authority, &out).await {
            Ok(result) => result,
            Err(error) => {
                if error.to_string() == "SCHEMA_DRIFT" {
                    c5plus::save(
                        &stopped,
                        &json!({"code":"SCHEMA_DRIFT","source_id":config["source_id"],"schema":config["schema"],"table":config["table"]}),
                    )?;
                }
                return Err(error);
            }
        };
        if result["status"] == "needs_bootstrap" {
            std::io::stdout().write_all(&serde_json::to_vec(&result)?)?;
            return Err("needs_bootstrap".into());
        }
        c5plus::save(&request, &result)?;
        result
    };
    std::io::stdout().write_all(&serde_json::to_vec(&value)?)?;
    Ok(())
}
