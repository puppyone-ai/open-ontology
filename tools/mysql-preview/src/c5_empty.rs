use super::{
    Result, c5plus, catalog_fingerprint, mysql, number, selected_projection, sha256_hex, text,
};
use mysql_async::{Conn, prelude::Queryable};
use serde_json::{Value, json};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const AUTHORITY: &str = "oo.pipeline.native-c5-empty-authority/v1";
pub const OBSERVATION: &str = "oo.c5.empty-observation/v1";
pub const SCHEMA_CODEC: &str = "mysql-empty-catalog-keys/v1";

pub(super) fn selected(authority: &Value) -> Result<bool> {
    if authority["protocol"] == AUTHORITY {
        Ok(true)
    } else if authority["empty_only"] == true {
        Err("C5_EMPTY_AUTHORITY_REQUIRED".into())
    } else {
        Ok(false)
    }
}

fn hex64(value: &Value) -> bool {
    value.as_str().is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

fn uuid(value: &Value) -> bool {
    value.as_str().is_some_and(|s| {
        s.len() == 36
            && s.bytes().enumerate().all(|(i, b)| {
                if matches!(i, 8 | 13 | 18 | 23) {
                    b == b'-'
                } else {
                    b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
                }
            })
    })
}

fn now_ms() -> Result<u64> {
    Ok(u64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
    )?)
}

fn validate(config: &Value, authority: &Value) -> Result<()> {
    let context = &authority["empty_observation"];
    let baseline = &authority["baseline_receipt"];
    let high = number(authority, "source_generation")?;
    let base = number(authority, "table_source_generation")?;
    let projection = selected_projection(config, authority)?;
    let canonical = super::canonical_projection(&projection)?;
    if authority["protocol"] != AUTHORITY
        || authority["empty_only"] != true
        || ["source_id", "schema", "table"]
            .iter()
            .any(|key| authority[*key] != config[*key])
        || authority["chunk_topology_revision"] != 1
        || authority["chunks"] != json!([])
        || authority["key_columns"] != json!([])
        || authority["next_chunk_index"] != 0
        || [
            "scan_id",
            "expected_chunks",
            "scan_layers",
            "scan_observed_at_ms",
        ]
        .iter()
        .any(|key| authority.get(*key) != Some(&Value::Null))
        || base == 0
        || base > high
        || !baseline.is_object()
        || baseline["source_generation"] != base
        || baseline["row_count"] != 0
        || baseline["input_digest"] != authority["input_digest"]
        || baseline["type_digest"] != authority["type_digest"]
        || authority["type_policy_version"] != "type-policy/v1"
        || baseline["type_policy_version"] != authority["type_policy_version"]
        || !hex64(&authority["input_digest"])
        || !hex64(&authority["schema_fingerprint"])
        || !hex64(&authority["type_digest"])
        || authority["projection_sha256"] != sha256_hex(&serde_json::to_vec(&canonical)?)
        || !authority["columns"].as_array().is_some_and(|columns| {
            columns.len() == projection.len()
                && columns
                    .iter()
                    .zip(&canonical)
                    .all(|(column, name)| column["name"] == *name)
        })
        || !uuid(&context["id"])
        || !uuid(&context["lease_id"])
        || context["lease_generation"].as_u64().is_none_or(|n| n == 0)
        || context["source_generation"] != high
        || context["issued_at_ms"].as_u64().is_none()
        || config
            .get("schema_fingerprint_codec")
            .is_some_and(|codec| codec != SCHEMA_CODEC)
    {
        return Err("C5_EMPTY_AUTHORITY_INVALID".into());
    }
    Ok(())
}

fn source_query(schema: &str, table: &str) -> Result<String> {
    if [schema, table]
        .iter()
        .any(|s| s.is_empty() || s.contains('\0') || s.chars().count() > 64)
    {
        return Err("empty query identifier".into());
    }
    Ok(format!(
        "SELECT 1 FROM `{}`.`{}` LIMIT 1",
        schema.replace('`', "``"),
        table.replace('`', "``")
    ))
}

type KeyFact = (
    String,
    u64,
    u64,
    String,
    Option<u64>,
    Option<String>,
    String,
    String,
    Option<String>,
    String,
);

fn fingerprint(schema: &str, table: &str, catalog: &str, keys: &[KeyFact]) -> Result<String> {
    Ok(sha256_hex(&serde_json::to_vec(&json!({
        "codec":SCHEMA_CODEC,"schema":schema,"table":table,"catalog_sha256":catalog,"declared_keys":keys,
    }))?))
}

/// Empty-only schema identity includes declared indexes without range-key type filtering.
/// Pin this value in the target initializer; never replace a target pin at observation time.
pub async fn schema_fingerprint(conn: &mut Conn, schema: &str, table: &str) -> Result<String> {
    let engine: Option<String> = conn.exec_first(
        "SELECT ENGINE FROM information_schema.TABLES WHERE TABLE_SCHEMA=? AND TABLE_NAME=? AND TABLE_TYPE='BASE TABLE'",
        (schema, table),
    ).await?;
    if engine.as_deref() != Some("InnoDB") {
        return Err("snapshot engine".into());
    }
    let catalog = mysql::catalog_columns(
        conn,
        &mut vec![],
        &mysql::SelectedTable {
            schema: schema.into(),
            name: table.into(),
            columns: vec![],
        },
    )
    .await?;
    let keys: Vec<KeyFact> = conn.exec(
        "SELECT s.INDEX_NAME,s.NON_UNIQUE,s.SEQ_IN_INDEX,s.COLUMN_NAME,s.SUB_PART,s.COLLATION,s.INDEX_TYPE,c.COLUMN_TYPE,c.COLLATION_NAME,c.IS_NULLABLE \
         FROM information_schema.STATISTICS s JOIN information_schema.COLUMNS c \
         ON c.TABLE_SCHEMA=s.TABLE_SCHEMA AND c.TABLE_NAME=s.TABLE_NAME AND c.COLUMN_NAME=s.COLUMN_NAME \
         WHERE s.TABLE_SCHEMA=? AND s.TABLE_NAME=? ORDER BY BINARY s.INDEX_NAME,s.SEQ_IN_INDEX LIMIT 1025",
        (schema, table),
    ).await?;
    if catalog.len() > 1024 || keys.len() > 1024 {
        return Err("empty schema budget".into());
    }
    fingerprint(schema, table, &catalog_fingerprint(&catalog)?, &keys)
}

fn require_schema(actual: &str, authority: &Value) -> Result<()> {
    if authority["schema_fingerprint"] != actual {
        return Err("SCHEMA_DRIFT".into());
    }
    Ok(())
}

async fn snapshot(
    conn: &mut Conn,
    config: &Value,
    authority: &Value,
) -> Result<(u64, u64, String)> {
    let schema = text(config, "schema")?;
    let table = text(config, "table")?;
    let query = source_query(schema, table)?;
    let started = now_ms()?;
    if started < number(&authority["empty_observation"], "issued_at_ms")? {
        return Err("C5_EMPTY_CONTEXT_NOT_ISSUED".into());
    }
    conn.query_drop("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .await?;
    conn.query_drop("START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY")
        .await?;
    let grants: Vec<String> = conn.query("SHOW GRANTS FOR CURRENT_USER()").await?;
    mysql::full_read_grants(&grants)?;
    require_schema(&schema_fingerprint(conn, schema, table).await?, authority)?;
    let rows: Vec<u8> = conn.query(&query).await?;
    require_schema(&schema_fingerprint(conn, schema, table).await?, authority)?;
    if !rows.is_empty() {
        return Err("C5_EMPTY_BASELINE_NONEMPTY".into());
    }
    conn.query_drop("COMMIT").await?;
    let checked = now_ms()?;
    if checked < started {
        return Err("empty observation clock".into());
    }
    Ok((started, checked, query))
}

pub(super) fn validate_replay(config: &Value, authority: &Value, request: &Value) -> Result<()> {
    validate(config, authority)?;
    let observation = &request["c5_observation"];
    let context = &authority["empty_observation"];
    if request["protocol"] != "oo.jobs.native-c5-nochange/v1"
        || request["source_id"] != authority["source_id"]
        || request["source_generation"] != authority["source_generation"]
        || request["parent_digest"] != authority["input_digest"]
        || observation["protocol"] != OBSERVATION
        || observation["observation_id"] != context["id"]
        || observation["lease_id"] != context["lease_id"]
        || observation["lease_generation"] != context["lease_generation"]
        || observation["schema_fingerprint"] != authority["schema_fingerprint"]
        || observation["baseline_receipt"] != authority["baseline_receipt"]
    {
        return Err("C5_EMPTY_OBSERVATION_CONTEXT_CHANGED".into());
    }
    Ok(())
}

pub(super) async fn produce(
    mut conn: Conn,
    config: &Value,
    input: &Value,
    authority: &Value,
    timeout: Duration,
) -> Result<Value> {
    validate(config, authority)?;
    let _: [bool; 4] = serde_json::from_value(input["c5_due"].clone())?;
    let policy = &input["c5_layered"]["policy"];
    let window = policy["execution_window_utc"]
        .as_array()
        .filter(|v| v.len() == 2)
        .ok_or("window")?;
    let start = window[0].as_u64().filter(|v| *v < 1440).ok_or("window")?;
    let end = window[1].as_u64().filter(|v| *v <= 1440).ok_or("window")? % 1440;
    let window = c5plus::Window::parse(&format!(
        "{:02}:{:02}-{:02}:{:02}",
        start / 60,
        start % 60,
        end / 60,
        end % 60
    ))?;
    if !window.contains(((now_ms()? / 60_000) % 1440) as u16) {
        return Err("outside execution window".into());
    }
    let elapsed = std::time::Instant::now();
    let result = tokio::time::timeout(timeout, snapshot(&mut conn, config, authority)).await;
    // Disconnect also rolls back incomplete or timed-out snapshots.
    let disconnected = tokio::time::timeout(Duration::from_secs(5), conn.disconnect()).await;
    let (started, checked, query) = result.map_err(|_| "empty snapshot deadline")??;
    disconnected.map_err(|_| "empty disconnect deadline")??;
    let duty = number(policy, "duty_per_mille")? as f64;
    tokio::time::sleep(elapsed.elapsed().mul_f64((1000.0 - duty) / duty)).await;
    let context = &authority["empty_observation"];
    let observation = json!({
        "protocol":OBSERVATION,"source_id":authority["source_id"],"schema":authority["schema"],"table":authority["table"],
        "source_generation":authority["source_generation"],"table_source_generation":authority["table_source_generation"],
        "schema_fingerprint":authority["schema_fingerprint"],"baseline_receipt":authority["baseline_receipt"],
        "observation_id":context["id"],"lease_id":context["lease_id"],"lease_generation":context["lease_generation"],
        "started_at_ms":started,"checked_at_ms":checked,"source_query":query,"rows_returned":0,"row_limit":1,
        "snapshot_complete":true,"read_only":true,"schema_checked_before":true,"schema_checked_after":true,
    });
    let digest = sha256_hex(&serde_json::to_vec(&observation)?);
    Ok(json!({
        "protocol":"oo.jobs.native-c5-nochange/v1","binding":input["binding"],"pin":input["pin"],
        "source_id":authority["source_id"],"method":"C5","capture_method":"c5_diff",
        "acknowledged_source_generation":authority["source_generation"],"source_generation":authority["source_generation"],
        "empty_diff":true,"data_gen":false,"apply":false,"ack":false,"position_codec":"snapshot-zero/v1",
        "parent_digest":authority["input_digest"],"source_digest":digest,"snapshot":{"kind":"c5_cycle","value":digest},
        "c5_completed_layers":[false,false,false,false],"c5_observation":observation,
    }))
}

#[cfg(test)]
#[path = "c5_empty_tests.rs"]
mod tests;
