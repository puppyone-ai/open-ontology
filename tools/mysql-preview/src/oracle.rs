//! Independent typed oracle for the logistics scale fixture.
//!
//! Expectations come from the formula module. Source rows are read with the
//! business account. Target rows are read with the PostgreSQL administrator
//! after a seal. Question text is not an input.

use std::cmp::Ordering;
use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::Path;
use std::time::Duration;

use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Row, Value};
use oo_connectors::PG17_RESERVED;
use oo_connectors::mysql::{
    CHUNK_DEADLINE_MS, CHUNK_MAX_BYTES, CHUNK_MAX_ROWS, CHUNK_STATEMENT_MS, Cancel, ChunkCursor,
    ChunkReader, MysqlConfig, ReadConnection, StatementNote, inspect, keep_requested, mysql_ident,
};
use oo_connectors::types::RunBinding;
use oo_core::canonical::LogicalType;
use oo_pipeline::c1_parts::{
    BUCKET_BITS_MAX, Cell, StateDigest, bucket_key, canonical_cell, canonical_row, composed_digest,
    pg_text_cell, source_cell,
};
use oo_pipeline::physical_mapping::quote_physical_name;
use serde_json::{Map, Value as JsonValue, json};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Row as PgRow};
use tokio::time::timeout;
use zeroize::Zeroizing;

use crate::config::{PreviewConfig, SelectedTable, read_ca, read_secret};
use crate::error::PreviewError;
use crate::logistics::{
    EXACT_COST, NEGATIVE_MILL, NON_FLOAT_COST, ProbeDate, ProbeTime, oracle, probe_attnum,
};
use crate::map::{grant_is_read_only, map_table, session_mode_ok};
use crate::source::{catalog_columns, open_mysql};

const PRESERVED_RUN: &str = "de8276fc-8634-4543-994f-ce82a46a3c18";

pub async fn audit_source(
    config_path: &Path,
    out_path: &Path,
    previous: Option<&Path>,
) -> Result<(), PreviewError> {
    let (config, config_bytes) = read_preview(config_path)?;
    let password = read_secret(&config.mysql_password_file)?;
    let mut conn = open_mysql(&config, password.as_str()).await?;
    let source = SourceEnd {
        mysql: config.mysql_source()?,
        password: Zeroizing::new(password.as_str().to_owned()),
        boolean_tinyint1: config.boolean_tinyint1,
    };
    let current = scalar(&mut conn, "SELECT CURRENT_USER()").await?;
    let mode = scalar(&mut conn, "SELECT @@session.sql_mode").await?;
    let grants = string_list(&mut conn, "SHOW GRANTS FOR CURRENT_USER()").await?;
    if !current
        .split('@')
        .next()
        .unwrap_or("")
        .eq_ignore_ascii_case(&config.mysql_user)
        || !session_mode_ok(&mode)
        || grants.is_empty()
        || grants.iter().any(|line| !grant_is_read_only(line))
    {
        return Err(PreviewError::new("privilege", "audit"));
    }
    let facts = oracle();
    let mut checks = BTreeMap::new();
    checks.insert(
        "event_count".to_string(),
        scalar(
            &mut conn,
            "SELECT CAST(COUNT(*) AS CHAR) FROM logistics.tracking_events",
        )
        .await?,
    );
    checks.insert(
        "hub_count".to_string(),
        scalar(
            &mut conn,
            "SELECT CAST(COUNT(*) AS CHAR) FROM logistics.hubs",
        )
        .await?,
    );
    checks.insert(
        "shipment_count".to_string(),
        scalar(
            &mut conn,
            "SELECT CAST(COUNT(*) AS CHAR) FROM logistics.shipments",
        )
        .await?,
    );
    checks.insert(
        "delivered_origin_cost".to_string(),
        scalar(
            &mut conn,
            "SELECT CAST(COALESCE(SUM(s.cost_amount), 0) AS CHAR) FROM logistics.shipments AS s JOIN logistics.hubs AS h ON s.origin_hub_id = h.hub_id WHERE s.status = 'delivered' AND h.hub_code = 'HUB-0042'",
        )
        .await?,
    );
    checks.insert(
        "delayed_air".to_string(),
        scalar(&mut conn, "SELECT CAST(COUNT(*) AS CHAR) FROM logistics.shipments WHERE status = 'delayed' AND transport_mode = 'air'").await?,
    );
    checks.insert(
        "null_cost".to_string(),
        scalar(
            &mut conn,
            "SELECT CAST(COUNT(*) AS CHAR) FROM logistics.shipments WHERE cost_amount IS NULL",
        )
        .await?,
    );
    checks.insert(
        "shipment_cost_exact".to_string(),
        scalar(
            &mut conn,
            "SELECT CAST(cost_amount AS CHAR) FROM logistics.shipments WHERE shipment_id = 1",
        )
        .await?,
    );
    checks.insert(
        "capacity_min".to_string(),
        scalar(
            &mut conn,
            "SELECT CAST(MIN(capacity_grams) AS CHAR) FROM logistics.hubs",
        )
        .await?,
    );
    checks.insert(
        "capacity_max".to_string(),
        scalar(
            &mut conn,
            "SELECT CAST(MAX(capacity_grams) AS CHAR) FROM logistics.hubs",
        )
        .await?,
    );
    checks.insert(
        "capacity_sum".to_string(),
        scalar(
            &mut conn,
            "SELECT CAST(SUM(capacity_grams) AS CHAR) FROM logistics.hubs",
        )
        .await?,
    );
    let zero_date = scalar(
        &mut conn,
        "SELECT CAST(day_on AS CHAR) FROM logistics.value_probe WHERE probe_id = 5",
    )
    .await?;
    let negative_time = scalar(
        &mut conn,
        "SELECT CAST(clock_at AS CHAR) FROM logistics.value_probe WHERE probe_id = 6",
    )
    .await?;
    let year_null = scalar(
        &mut conn,
        "SELECT CAST((SELECT opened_at IS NULL FROM logistics.hubs WHERE hub_id = 1) + (SELECT created_at IS NULL FROM logistics.shipments WHERE shipment_id = 1) + (SELECT event_at IS NULL FROM logistics.tracking_events WHERE event_id = 1) AS CHAR)",
    )
    .await?;
    let year_far = scalar(
        &mut conn,
        "SELECT CAST(opened_at AS CHAR) FROM logistics.hubs WHERE hub_id = 2",
    )
    .await?;
    let gaps = scalar(&mut conn, "SELECT CAST(COUNT(*) AS CHAR) FROM logistics.range_gap WHERE reason = 'mysql_datetime_range'").await?;
    let scale_tables = SCALE_TABLES.map(|name| ("logistics", name));
    let (mut conn, fingerprints) = source_fingerprints(conn, &source, &scale_tables).await?;
    let writes = write_counts(&mut conn).await?;
    drop(conn);
    let mut failures = Vec::new();
    expect(
        &mut failures,
        "event_count",
        checks.get("event_count").map(String::as_str).unwrap_or(""),
        &facts.event_rows.to_string(),
    );
    expect(
        &mut failures,
        "hub_count",
        checks.get("hub_count").map(String::as_str).unwrap_or(""),
        &facts.hub_rows.to_string(),
    );
    expect(
        &mut failures,
        "shipment_count",
        checks
            .get("shipment_count")
            .map(String::as_str)
            .unwrap_or(""),
        &facts.shipment_rows.to_string(),
    );
    expect(
        &mut failures,
        "delivered_origin_cost",
        checks
            .get("delivered_origin_cost")
            .map(String::as_str)
            .unwrap_or(""),
        &facts.delivered_origin_cost,
    );
    expect(
        &mut failures,
        "delayed_air",
        checks.get("delayed_air").map(String::as_str).unwrap_or(""),
        &facts.delayed_air.to_string(),
    );
    expect(
        &mut failures,
        "null_cost",
        checks.get("null_cost").map(String::as_str).unwrap_or(""),
        &facts.null_cost.to_string(),
    );
    expect(
        &mut failures,
        "shipment_cost_exact",
        checks
            .get("shipment_cost_exact")
            .map(String::as_str)
            .unwrap_or(""),
        &facts.shipment_cost_exact,
    );
    expect(
        &mut failures,
        "capacity_min",
        checks.get("capacity_min").map(String::as_str).unwrap_or(""),
        &facts.capacity_min.to_string(),
    );
    expect(
        &mut failures,
        "capacity_max",
        checks.get("capacity_max").map(String::as_str).unwrap_or(""),
        &facts.capacity_max.to_string(),
    );
    expect(
        &mut failures,
        "capacity_sum",
        checks.get("capacity_sum").map(String::as_str).unwrap_or(""),
        &facts.capacity_sum.to_string(),
    );
    if !zero_date.starts_with("0000-00-00") {
        failures.push("zero_date".to_string());
    }
    if clock_head(&negative_time) != "-01:00:00" {
        failures.push("signed_time".to_string());
    }
    if year_null != "3" {
        failures.push("mysql_datetime_range".to_string());
    }
    if !year_far.starts_with("9999-12-31 23:59:58") {
        failures.push("datetime_9999".to_string());
    }
    if gaps != facts.gap_rows.to_string() {
        failures.push("range_gap".to_string());
    }
    if !writes.is_empty() {
        failures.push("source_write".to_string());
    }
    if let Some(previous) = previous {
        let prior = read_json(previous)?;
        let prior_checks = prior.get("checks").cloned().unwrap_or(JsonValue::Null);
        let prior_sums = prior
            .get("key_bucket_fingerprints")
            .cloned()
            .unwrap_or(JsonValue::Null);
        if prior_checks != json!(checks) || prior_sums != fingerprints {
            failures.push("changed".to_string());
        }
    }
    let _ = config_bytes;
    let body = json!({
        "protocol": "oo.mysql-preview.scale-audit/v1",
        "checks": checks,
        "key_bucket_fingerprints": fingerprints,
        "failures": failures,
        "grants_read_only": true,
        "matched": failures.is_empty(),
        "raw_status": {"negative_time": clock_head(&negative_time), "zero_date": zero_date},
        "sql_mode_strict": true,
        "write_statements": writes
    });
    write_json(out_path, &body)?;
    if failures.is_empty() {
        Ok(())
    } else {
        Err(PreviewError::new("value", "oracle"))
    }
}

pub async fn audit_target(
    config_path: &Path,
    outcome_path: &Path,
    out_path: &Path,
) -> Result<(), PreviewError> {
    let (config, _) = read_preview(config_path)?;
    let outcome = read_json(outcome_path)?;
    let outcome_map = outcome
        .as_object()
        .ok_or_else(|| PreviewError::new("config", "json"))?;
    if outcome_map.get("state").and_then(JsonValue::as_str) != Some("awaiting_publish") {
        return Err(PreviewError::new("target", "state"));
    }
    let run_id = text_field(outcome_map, "run_id")?;
    if run_id == PRESERVED_RUN {
        return Err(PreviewError::new("target", "prototype"));
    }
    verbs_are_read_only(outcome_map)?;
    let tables = physical_tables(outcome_map)?;
    let password = read_secret(&config.target_admin_password_file)?;
    let ca = read_ca(&config.target_ca_file)?;
    let mut admin = dial_admin(&config, password.as_str(), &ca).await?;
    let preserved: String =
        sqlx::query_scalar("SELECT state FROM _oo.full_import_run WHERE run_id = $1::uuid")
            .bind(PRESERVED_RUN)
            .fetch_one(&mut admin)
            .await
            .map_err(|_| PreviewError::new("target", "prototype"))?;
    if preserved != "awaiting_publish" {
        return Err(PreviewError::new("target", "prototype"));
    }
    let facts = oracle();
    let hubs = tables
        .get("hubs")
        .ok_or_else(|| PreviewError::new("catalog", "coverage"))?;
    let shipments = tables
        .get("shipments")
        .ok_or_else(|| PreviewError::new("catalog", "coverage"))?;
    let events = tables
        .get("tracking_events")
        .ok_or_else(|| PreviewError::new("catalog", "coverage"))?;
    let probe = tables
        .get("value_probe")
        .ok_or_else(|| PreviewError::new("catalog", "coverage"))?;
    let gaps = tables
        .get("range_gap")
        .ok_or_else(|| PreviewError::new("catalog", "coverage"))?;
    let mut failures = Vec::new();
    expect(
        &mut failures,
        "event_count",
        &pg_text(
            &mut admin,
            &format!("SELECT count(*)::text FROM \"_oo\".{}", events.physical),
        )
        .await?,
        &facts.event_rows.to_string(),
    );
    expect(
        &mut failures,
        "hub_count",
        &pg_text(
            &mut admin,
            &format!("SELECT count(*)::text FROM \"_oo\".{}", hubs.physical),
        )
        .await?,
        &facts.hub_rows.to_string(),
    );
    expect(
        &mut failures,
        "shipment_count",
        &pg_text(
            &mut admin,
            &format!("SELECT count(*)::text FROM \"_oo\".{}", shipments.physical),
        )
        .await?,
        &facts.shipment_rows.to_string(),
    );
    let cost_sql = format!(
        "SELECT COALESCE(sum({cost})::text, 'null') FROM \"_oo\".{shipments} AS s JOIN \"_oo\".{hubs} AS h ON s.{origin} = h.{hub_id} WHERE s.{status} = 'delivered' AND h.{code} = 'HUB-0042'",
        cost = shipments.col("cost_amount")?,
        shipments = shipments.physical,
        hubs = hubs.physical,
        origin = shipments.col("origin_hub_id")?,
        hub_id = hubs.col("hub_id")?,
        status = shipments.col("status")?,
        code = hubs.col("hub_code")?,
    );
    expect(
        &mut failures,
        "delivered_origin_cost",
        &pg_text(&mut admin, &cost_sql).await?,
        &facts.delivered_origin_cost,
    );
    let delayed_sql = format!(
        "SELECT count(*)::text FROM \"_oo\".{table} WHERE {status} = 'delayed' AND {mode} = 'air'",
        table = shipments.physical,
        status = shipments.col("status")?,
        mode = shipments.col("transport_mode")?,
    );
    expect(
        &mut failures,
        "delayed_air",
        &pg_text(&mut admin, &delayed_sql).await?,
        &facts.delayed_air.to_string(),
    );
    let null_sql = format!(
        "SELECT count(*)::text FROM \"_oo\".{table} WHERE {cost} IS NULL",
        table = shipments.physical,
        cost = shipments.col("cost_amount")?
    );
    expect(
        &mut failures,
        "null_cost",
        &pg_text(&mut admin, &null_sql).await?,
        &facts.null_cost.to_string(),
    );
    let exact_sql = format!(
        "SELECT COALESCE({cost}::text, 'null') FROM \"_oo\".{table} WHERE {key} = 1",
        cost = shipments.col("cost_amount")?,
        table = shipments.physical,
        key = shipments.col("shipment_id")?,
    );
    expect(
        &mut failures,
        "shipment_cost_exact",
        &pg_text(&mut admin, &exact_sql).await?,
        &facts.shipment_cost_exact,
    );
    let bounds_sql = format!(
        "SELECT min({col})::text, max({col})::text, sum({col})::text FROM \"_oo\".{table}",
        col = hubs.col("capacity_grams")?,
        table = hubs.physical,
    );
    let bounds = pg_pair(&mut admin, &bounds_sql).await?;
    expect(
        &mut failures,
        "capacity_min",
        &bounds.0,
        &i64::MIN.to_string(),
    );
    expect(
        &mut failures,
        "capacity_max",
        &bounds.1,
        &i64::MAX.to_string(),
    );
    expect(
        &mut failures,
        "capacity_sum",
        &bounds.2,
        &facts.capacity_sum.to_string(),
    );
    let null_hub = pg_text(
        &mut admin,
        &format!(
            "SELECT ({col} IS NULL)::text FROM \"_oo\".{table} WHERE {key} = 1",
            col = hubs.col("opened_at")?,
            table = hubs.physical,
            key = hubs.col("hub_id")?
        ),
    )
    .await?;
    if null_hub != "true" {
        failures.push("mysql_datetime_range".to_string());
    }
    let far = pg_text(
        &mut admin,
        &format!(
            "SELECT to_char({col}, 'YYYY-MM-DD HH24:MI:SS') FROM \"_oo\".{table} WHERE {key} = 2",
            col = hubs.col("opened_at")?,
            table = hubs.physical,
            key = hubs.col("hub_id")?,
        ),
    )
    .await?;
    if far != "9999-12-31 23:59:58" {
        failures.push("datetime_9999".to_string());
    }
    expect_probe(&mut admin, probe, &mut failures).await?;
    let gap_count = pg_text(
        &mut admin,
        &format!(
            "SELECT count(*)::text FROM \"_oo\".{table} WHERE {reason} = 'mysql_datetime_range'",
            table = gaps.physical,
            reason = gaps.col("reason")?
        ),
    )
    .await?;
    if gap_count != facts.gap_rows.to_string() {
        failures.push("range_gap".to_string());
    }
    let quality = quality_facts(&mut admin, &run_id).await?;
    if !quality.ok {
        failures.push("quality".to_string());
    }
    let mysql_password = read_secret(&config.mysql_password_file)?;
    let source = SourceEnd {
        mysql: config.mysql_source()?,
        password: Zeroizing::new(mysql_password.as_str().to_owned()),
        boolean_tinyint1: config.boolean_tinyint1,
    };
    let source_conn = open_mysql(&config, mysql_password.as_str()).await?;
    let key_buckets = key_bucket_compare(
        source_conn,
        &source,
        &scale_bucket_tables(&tables)?,
        &mut admin,
        KEY_BUCKET_BITS,
    )
    .await?;
    for table in key_buckets["tables"].as_array().into_iter().flatten() {
        if table["matched"] != json!(true) {
            failures.push(format!(
                "key_bucket:{}",
                table["table"].as_str().unwrap_or("")
            ));
        }
    }
    let body = json!({
        "protocol": "oo.mysql-preview.scale-target/v1",
        "failures": failures,
        "key_buckets": key_buckets,
        "matched": failures.is_empty(),
        "preserved_run": PRESERVED_RUN,
        "preserved_state": preserved,
        "quality": quality.body,
        "run_id": run_id
    });
    write_json(out_path, &body)?;
    if failures.is_empty() {
        Ok(())
    } else {
        Err(PreviewError::new("value", "oracle"))
    }
}

struct Phys {
    physical: String,
    columns: BTreeMap<String, String>,
}

impl Phys {
    fn col(&self, name: &str) -> Result<String, PreviewError> {
        let physical = self
            .columns
            .get(name)
            .ok_or_else(|| PreviewError::new("catalog", "coverage"))?;
        quote_physical_name(physical).map_err(|_| PreviewError::new("target", "physical_name"))
    }
}

fn physical_tables(
    outcome: &Map<String, JsonValue>,
) -> Result<BTreeMap<String, Phys>, PreviewError> {
    let mut tables = BTreeMap::new();
    let list = outcome
        .get("tables")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| PreviewError::new("config", "json"))?;
    for table in list {
        let table = table
            .as_object()
            .ok_or_else(|| PreviewError::new("config", "json"))?;
        let name = text_field(table, "table")?;
        let physical = quote_physical_name(&text_field(table, "physical")?)
            .map_err(|_| PreviewError::new("target", "physical_name"))?;
        let mut columns = BTreeMap::new();
        for column in table
            .get("columns")
            .and_then(JsonValue::as_array)
            .ok_or_else(|| PreviewError::new("config", "json"))?
        {
            let column = column
                .as_object()
                .ok_or_else(|| PreviewError::new("config", "json"))?;
            columns.insert(text_field(column, "name")?, text_field(column, "physical")?);
        }
        tables.insert(name, Phys { physical, columns });
    }
    Ok(tables)
}

fn verbs_are_read_only(outcome: &Map<String, JsonValue>) -> Result<(), PreviewError> {
    let statements = outcome
        .get("statements")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| PreviewError::new("config", "json"))?;
    for statement in statements {
        let verb = statement
            .get("verb")
            .and_then(JsonValue::as_str)
            .unwrap_or("");
        if !matches!(verb, "SELECT" | "SHOW" | "START" | "COMMIT") {
            return Err(PreviewError::new("privilege", "audit"));
        }
    }
    Ok(())
}

async fn expect_probe(
    admin: &mut PgConnection,
    probe: &Phys,
    failures: &mut Vec<String>,
) -> Result<(), PreviewError> {
    let unsigned = pg_text(
        admin,
        &format!(
            "SELECT {col}::text FROM \"_oo\".{table} WHERE {key} = 2",
            col = probe.col("unsigned_grams")?,
            table = probe.physical,
            key = probe.col("probe_id")?
        ),
    )
    .await?;
    expect(failures, "unsigned", &unsigned, &u64::MAX.to_string());
    let exact = pg_text(
        admin,
        &format!(
            "SELECT {col}::text FROM \"_oo\".{table} WHERE {key} = 1",
            col = probe.col("exact_amount")?,
            table = probe.physical,
            key = probe.col("probe_id")?
        ),
    )
    .await?;
    expect(failures, "probe_exact", &exact, EXACT_COST);
    let negative = pg_text(
        admin,
        &format!(
            "SELECT {col}::text FROM \"_oo\".{table} WHERE {key} = 2",
            col = probe.col("exact_amount")?,
            table = probe.physical,
            key = probe.col("probe_id")?
        ),
    )
    .await?;
    expect(failures, "probe_negative", &negative, NEGATIVE_MILL);
    let wide = pg_text(
        admin,
        &format!(
            "SELECT {col}::text FROM \"_oo\".{table} WHERE {key} = 4",
            col = probe.col("exact_amount")?,
            table = probe.physical,
            key = probe.col("probe_id")?
        ),
    )
    .await?;
    expect(failures, "probe_wide", &wide, NON_FLOAT_COST);
    let missing = pg_text(
        admin,
        &format!(
            "SELECT ({col} IS NULL)::text FROM \"_oo\".{table} WHERE {key} = 3",
            col = probe.col("exact_amount")?,
            table = probe.physical,
            key = probe.col("probe_id")?
        ),
    )
    .await?;
    if missing != "true" {
        failures.push("probe_null".to_string());
    }
    let case = pg_text(
        admin,
        &format!(
            "SELECT {col}->>'case' FROM \"_oo\".{table} WHERE {key} = 1",
            col = probe.col("payload")?,
            table = probe.physical,
            key = probe.col("probe_id")?
        ),
    )
    .await?;
    if case != "exact" {
        failures.push("json".to_string());
    }
    let empty = pg_text(admin, &format!("SELECT CASE WHEN {col} IS NULL THEN 'null' ELSE encode({col}, 'hex') END FROM \"_oo\".{table} WHERE {key} = 1", col = probe.col("badge")?, table = probe.physical, key = probe.col("probe_id")?)).await?;
    let absent = pg_text(admin, &format!("SELECT CASE WHEN {col} IS NULL THEN 'null' ELSE encode({col}, 'hex') END FROM \"_oo\".{table} WHERE {key} = 3", col = probe.col("badge")?, table = probe.physical, key = probe.col("probe_id")?)).await?;
    let zero_byte = pg_text(
        admin,
        &format!(
            "SELECT encode({col}, 'hex') FROM \"_oo\".{table} WHERE {key} = 2",
            col = probe.col("badge")?,
            table = probe.physical,
            key = probe.col("probe_id")?
        ),
    )
    .await?;
    if !empty.is_empty() || absent != "null" || zero_byte != "00" {
        failures.push("binary".to_string());
    }
    let masked_day = pg_text(
        admin,
        &format!(
            "SELECT ({col} IS NULL)::text FROM \"_oo\".{table} WHERE {key} = 5",
            col = probe.col("day_on")?,
            table = probe.physical,
            key = probe.col("probe_id")?
        ),
    )
    .await?;
    let signed_time = pg_text(
        admin,
        &format!(
            "SELECT {col}::text FROM \"_oo\".{table} WHERE {key} = 6",
            col = probe.col("clock_at")?,
            table = probe.physical,
            key = probe.col("probe_id")?
        ),
    )
    .await?;
    if masked_day != "true" || signed_time != "-01:00:00" {
        failures.push("masked".to_string());
    }
    let _ = (ProbeDate::Zero, ProbeTime::Negative("-01:00:00"));
    Ok(())
}

struct QualityFacts {
    ok: bool,
    body: JsonValue,
}

async fn quality_facts(
    admin: &mut PgConnection,
    run_id: &str,
) -> Result<QualityFacts, PreviewError> {
    let rows = sqlx::query(
        "SELECT kind, COALESCE(reason, ''), COALESCE(value_len, -1), COALESCE(attnum, -1), COALESCE(row_index, -1), COALESCE(key_digest, ''), COALESCE(rejected_rows, -1), payload_digest \
         FROM _oo.full_import_quality WHERE run_id = $1::uuid ORDER BY quality_id",
    )
    .bind(run_id)
    .fetch_all(&mut *admin)
    .await
    .map_err(|_| PreviewError::new("target", "quality"))?;
    let mut batch_rejected = 0_i64;
    let mut reject_lines = 0_i64;
    let mut zero_date = 0_i64;
    let mut time_range = 0_i64;
    let mut digests = Vec::new();
    let mut listed = Vec::new();
    for row in &rows {
        let kind: String = row
            .try_get(0)
            .map_err(|_| PreviewError::new("target", "quality"))?;
        let reason: String = row
            .try_get(1)
            .map_err(|_| PreviewError::new("target", "quality"))?;
        let value_len: i64 = row
            .try_get(2)
            .map_err(|_| PreviewError::new("target", "quality"))?;
        let attnum: i32 = row
            .try_get(3)
            .map_err(|_| PreviewError::new("target", "quality"))?;
        let row_index: i64 = row
            .try_get(4)
            .map_err(|_| PreviewError::new("target", "quality"))?;
        let key_digest: String = row
            .try_get(5)
            .map_err(|_| PreviewError::new("target", "quality"))?;
        let rejected_rows: i64 = row
            .try_get(6)
            .map_err(|_| PreviewError::new("target", "quality"))?;
        let payload_digest: String = row
            .try_get(7)
            .map_err(|_| PreviewError::new("target", "quality"))?;
        if payload_digest.len() != 64 {
            return Ok(QualityFacts {
                ok: false,
                body: json!({"payload_digest": "invalid"}),
            });
        }
        if kind == "batch" {
            batch_rejected += rejected_rows.max(0);
            if rejected_rows > 0 {
                digests.push(payload_digest);
            }
        } else if kind == "reject" {
            reject_lines += 1;
            if reason == "zero_date" {
                zero_date += 1;
            } else if reason == "time_range" {
                time_range += 1;
            }
            listed.push(json!({
                "attnum": attnum,
                "key_digest": key_digest,
                "reason": reason,
                "row_index": row_index,
                "value_len": value_len
            }));
        }
    }
    let day = probe_attnum("day_on").unwrap_or(-1);
    let shaped = listed.iter().any(|item| {
        item.get("reason").and_then(JsonValue::as_str) == Some("zero_date")
            && item.get("attnum").and_then(JsonValue::as_i64) == Some(i64::from(day))
            && item
                .get("value_len")
                .and_then(JsonValue::as_i64)
                .unwrap_or(0)
                > 0
            && item
                .get("key_digest")
                .and_then(JsonValue::as_str)
                .unwrap_or("")
                .len()
                == 64
    });
    let ok = batch_rejected == reject_lines
        && zero_date == 1
        && time_range == 0
        && reject_lines == 1
        && shaped
        && !digests.is_empty();
    Ok(QualityFacts {
        ok,
        body: json!({
            "batch_rejected_rows": batch_rejected,
            "payload_digests": digests,
            "reject_lines": listed,
            "reject_row_count": reject_lines
        }),
    })
}

async fn dial_admin(
    config: &PreviewConfig,
    password: &str,
    ca: &[u8],
) -> Result<PgConnection, PreviewError> {
    let options = PgConnectOptions::from_explicit()
        .host(&config.target_host)
        .port(config.target_port)
        .username(&config.target_admin_user)
        .password(password)
        .database(&config.target_database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca.to_vec())
        .disable_statement_logging();
    let connect = timeout(
        Duration::from_millis(config.connect_timeout_ms),
        options.connect(),
    )
    .await;
    let mut conn = match connect {
        Ok(Ok(conn)) => conn,
        _ => return Err(PreviewError::new("connect", "target")),
    };
    sqlx::query("SET statement_timeout = '120s'")
        .execute(&mut conn)
        .await
        .map_err(|_| PreviewError::new("connect", "target"))?;
    Ok(conn)
}

async fn pg_text(conn: &mut PgConnection, sql: &str) -> Result<String, PreviewError> {
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_one(&mut *conn)
        .await
        .map_err(|_| PreviewError::new("target", "oracle"))?;
    row.try_get(0)
        .map_err(|_| PreviewError::new("target", "oracle"))
}

async fn pg_pair(
    conn: &mut PgConnection,
    sql: &str,
) -> Result<(String, String, String), PreviewError> {
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_one(&mut *conn)
        .await
        .map_err(|_| PreviewError::new("target", "oracle"))?;
    Ok((
        row.try_get(0)
            .map_err(|_| PreviewError::new("target", "oracle"))?,
        row.try_get(1)
            .map_err(|_| PreviewError::new("target", "oracle"))?,
        row.try_get(2)
            .map_err(|_| PreviewError::new("target", "oracle"))?,
    ))
}

fn read_preview(path: &Path) -> Result<(PreviewConfig, Vec<u8>), PreviewError> {
    let bytes = fs::read(path).map_err(|_| PreviewError::new("config", "path"))?;
    let text = std::str::from_utf8(&bytes).map_err(|_| PreviewError::new("config", "json"))?;
    Ok((crate::config::parse_config(text)?, bytes))
}

fn read_json(path: &Path) -> Result<JsonValue, PreviewError> {
    let bytes = fs::read(path).map_err(|_| PreviewError::new("config", "path"))?;
    serde_json::from_slice(&bytes).map_err(|_| PreviewError::new("config", "json"))
}

fn write_json(path: &Path, body: &JsonValue) -> Result<(), PreviewError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| PreviewError::new("config", "out"))?;
    }
    fs::write(
        path,
        serde_json::to_vec_pretty(body).map_err(|_| PreviewError::new("config", "out"))?,
    )
    .map_err(|_| PreviewError::new("config", "out"))
}

fn text_field(map: &Map<String, JsonValue>, key: &str) -> Result<String, PreviewError> {
    map.get(key)
        .and_then(JsonValue::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| PreviewError::new("config", "json"))
}

fn expect(failures: &mut Vec<String>, name: &str, actual: &str, expected: &str) {
    if !same_number(actual, expected) {
        failures.push(name.to_string());
    }
}

fn same_number(left: &str, right: &str) -> bool {
    fn canon(text: &str) -> String {
        let text = text.trim();
        let negative = text.starts_with('-');
        let body = text.trim_start_matches('-');
        let shown = if let Some((whole, frac)) = body.split_once('.') {
            let frac = frac.trim_end_matches('0');
            if frac.is_empty() {
                whole.to_string()
            } else {
                format!("{whole}.{frac}")
            }
        } else {
            body.to_string()
        };
        if negative && shown != "0" {
            format!("-{shown}")
        } else {
            shown
        }
    }
    canon(left) == canon(right)
}

fn clock_head(text: &str) -> String {
    text.trim().split('.').next().unwrap_or("").to_string()
}

async fn scalar(conn: &mut Conn, sql: &str) -> Result<String, PreviewError> {
    let row: Option<Row> = timeout(Duration::from_secs(60), conn.query_first(sql))
        .await
        .map_err(|_| PreviewError::new("budget", "statement_timeout"))?
        .map_err(|_| PreviewError::new("protocol", "query"))?;
    let row = row.ok_or_else(|| PreviewError::new("protocol", "query"))?;
    cell_string(&row, 0)
}

async fn string_list(conn: &mut Conn, sql: &str) -> Result<Vec<String>, PreviewError> {
    let rows: Vec<Row> = timeout(Duration::from_secs(30), conn.query(sql))
        .await
        .map_err(|_| PreviewError::new("budget", "statement_timeout"))?
        .map_err(|_| PreviewError::new("protocol", "query"))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(cell_string(&row, 0)?);
    }
    Ok(out)
}

async fn write_counts(conn: &mut Conn) -> Result<Vec<String>, PreviewError> {
    let sql = "SELECT EVENT_NAME FROM performance_schema.events_statements_summary_by_user_by_event_name WHERE USER = 'oo_scale' AND COUNT_STAR > 0 AND EVENT_NAME IN ('statement/sql/insert','statement/sql/update','statement/sql/delete','statement/sql/replace','statement/sql/truncate','statement/sql/create_table','statement/sql/alter_table','statement/sql/drop_table')";
    match timeout(Duration::from_secs(30), conn.query(sql)).await {
        Ok(Ok(rows)) => {
            let rows: Vec<Row> = rows;
            let mut out = Vec::new();
            for row in rows {
                out.push(cell_string(&row, 0)?);
            }
            Ok(out)
        }
        _ => Ok(Vec::new()),
    }
}

fn cell_string(row: &Row, index: usize) -> Result<String, PreviewError> {
    let value: Value = row
        .get(index)
        .ok_or_else(|| PreviewError::new("protocol", "cell"))?;
    match value {
        Value::NULL => Ok("null".to_string()),
        Value::Bytes(bytes) => {
            String::from_utf8(bytes).map_err(|_| PreviewError::new("protocol", "cell"))
        }
        Value::Int(value) => Ok(value.to_string()),
        Value::UInt(value) => Ok(value.to_string()),
        _ => Err(PreviewError::new("protocol", "cell")),
    }
}

/// Bucket exponent when the caller does not choose one.
pub(crate) const KEY_BUCKET_BITS: u8 = 10;
const TARGET_PAGE_ROWS: i64 = 4096;
const EXAMPLE_LIMIT: usize = 20;
const DRILL_LIMIT: usize = 64;
const SCALE_TABLES: [&str; 5] = [
    "hubs",
    "shipments",
    "tracking_events",
    "value_probe",
    "range_gap",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct KeyBuckets {
    lo: i128,
    span: i128,
    bits: u8,
}

impl KeyBuckets {
    pub(crate) fn new(lo: i128, hi: i128, bits: u8) -> Result<Self, PreviewError> {
        if bits > BUCKET_BITS_MAX || hi < lo {
            return Err(PreviewError::new("config", "bucket_bits"));
        }
        Ok(Self {
            lo,
            span: hi - lo + 1,
            bits,
        })
    }

    fn len(&self) -> usize {
        1usize << self.bits
    }

    pub(crate) fn index(&self, key: i128) -> usize {
        let offset = key - self.lo;
        if offset <= 0 {
            return 0;
        }
        let index = offset as u128 * self.len() as u128 / self.span as u128;
        index.min(self.len() as u128 - 1) as usize
    }

    pub(crate) fn range(&self, index: usize) -> (Option<i128>, Option<i128>) {
        let buckets = self.len() as i128;
        let i = index as i128;
        let start = (index > 0).then(|| self.lo + ceil_div(i * self.span, buckets));
        let end =
            (index + 1 < self.len()).then(|| self.lo + ceil_div((i + 1) * self.span, buckets) - 1);
        (start, end)
    }
}

fn ceil_div(value: i128, divisor: i128) -> i128 {
    (value + divisor - 1) / divisor
}

pub(crate) struct BucketDigests {
    buckets: KeyBuckets,
    digests: Vec<StateDigest>,
}

impl BucketDigests {
    pub(crate) fn new(buckets: KeyBuckets) -> Result<Self, PreviewError> {
        let digests = (0..buckets.len())
            .map(|_| StateDigest::new(0))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| PreviewError::new("value", "digest"))?;
        Ok(Self { buckets, digests })
    }

    pub(crate) fn add(&mut self, key: i128, canonical: &[u8]) -> Result<(), PreviewError> {
        let index = self.buckets.index(key);
        self.digests[index]
            .add_row(key, canonical)
            .map_err(|_| PreviewError::new("value", "digest"))
    }

    pub(crate) fn rows(&self) -> u64 {
        self.digests.iter().map(StateDigest::rows).sum()
    }

    pub(crate) fn total(&self) -> StateDigest {
        self.digests[0].clone()
    }

    pub(crate) fn mismatched(&self, other: &Self) -> Vec<usize> {
        (0..self.digests.len())
            .filter(|&index| self.digests[index] != other.digests[index])
            .collect()
    }
}

#[derive(Default)]
pub(crate) struct DiffTally {
    missing_in_target: u64,
    extra_in_target: u64,
    changed: u64,
    examples: Vec<JsonValue>,
}

impl DiffTally {
    fn note(&mut self, kind: &str, key: i128, column: Option<&str>) {
        match kind {
            "missing_in_target" => self.missing_in_target += 1,
            "extra_in_target" => self.extra_in_target += 1,
            _ => self.changed += 1,
        }
        if self.examples.len() < EXAMPLE_LIMIT {
            self.examples
                .push(json!({"key": key.to_string(), "kind": kind, "column": column}));
        }
    }

    fn total(&self) -> u64 {
        self.missing_in_target + self.extra_in_target + self.changed
    }

    fn json(&self) -> JsonValue {
        json!({
            "missing_in_target": self.missing_in_target,
            "extra_in_target": self.extra_in_target,
            "changed": self.changed,
            "examples": self.examples,
        })
    }
}

pub(crate) struct SideRow {
    key: i128,
    cells: Vec<Cell>,
}

pub(crate) trait RowSource {
    async fn next_row(&mut self) -> Result<Option<SideRow>, PreviewError>;
}

fn first_changed_column(
    source_types: &[LogicalType],
    target_types: &[LogicalType],
    names: &[String],
    source: &SideRow,
    target: &SideRow,
) -> Result<Option<String>, PreviewError> {
    let width = names.len();
    if source_types.len() != width
        || target_types.len() != width
        || source.cells.len() != width
        || target.cells.len() != width
    {
        return Err(PreviewError::new("value", "row_width"));
    }
    for index in 0..width {
        let left = canonical_cell(&source_types[index], &source.cells[index])
            .map_err(|_| PreviewError::new("value", "canonical"))?;
        let right = canonical_cell(&target_types[index], &target.cells[index])
            .map_err(|_| PreviewError::new("value", "canonical"))?;
        if left != right {
            return Ok(Some(names[index].clone()));
        }
    }
    Ok(None)
}

pub(crate) async fn diff_streams<A: RowSource, B: RowSource>(
    source: &mut A,
    target: &mut B,
    source_types: &[LogicalType],
    target_types: &[LogicalType],
    names: &[String],
    tally: &mut DiffTally,
) -> Result<(), PreviewError> {
    let mut left = source.next_row().await?;
    let mut right = target.next_row().await?;
    loop {
        let order = match (&left, &right) {
            (None, None) => break,
            (Some(a), Some(b)) => a.key.cmp(&b.key),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
        };
        match order {
            Ordering::Less => {
                let Some(a) = left.take() else { break };
                tally.note("missing_in_target", a.key, None);
                left = source.next_row().await?;
            }
            Ordering::Greater => {
                let Some(b) = right.take() else { break };
                tally.note("extra_in_target", b.key, None);
                right = target.next_row().await?;
            }
            Ordering::Equal => {
                let (Some(a), Some(b)) = (left.take(), right.take()) else {
                    break;
                };
                if let Some(column) =
                    first_changed_column(source_types, target_types, names, &a, &b)?
                {
                    tally.note("changed", a.key, Some(&column));
                }
                left = source.next_row().await?;
                right = target.next_row().await?;
            }
        }
    }
    Ok(())
}

pub(crate) struct SourceEnd {
    pub(crate) mysql: MysqlConfig,
    pub(crate) password: Zeroizing<String>,
    pub(crate) boolean_tinyint1: bool,
}

pub(crate) struct SourceTable {
    reader: ChunkReader,
    types: Vec<LogicalType>,
    names: Vec<String>,
    key: usize,
    lo: i128,
    hi: i128,
}

pub(crate) struct TargetTable {
    relation: String,
    physical: Vec<String>,
    oids: Vec<i64>,
    types: Vec<LogicalType>,
    key: usize,
}

pub(crate) struct BucketTable {
    pub(crate) schema: String,
    pub(crate) name: String,
    pub(crate) relation: String,
    pub(crate) columns: BTreeMap<String, String>,
}

async fn key_extent(
    conn: &mut Conn,
    selected: &SelectedTable,
    key: &str,
) -> Result<(i128, i128), PreviewError> {
    let sql = format!(
        "SELECT CAST(MIN(`{key}`) AS CHAR), CAST(MAX(`{key}`) AS CHAR) FROM `{}`.`{}`",
        selected.schema, selected.name
    );
    let row: Option<(Option<String>, Option<String>)> =
        timeout(Duration::from_secs(60), conn.query_first(sql))
            .await
            .map_err(|_| PreviewError::new("budget", "statement_timeout"))?
            .map_err(|_| PreviewError::new("protocol", "query"))?;
    let parse = |text: String| {
        text.parse::<i128>()
            .map_err(|_| PreviewError::new("value", "key_range"))
    };
    match row {
        Some((Some(lo), Some(hi))) => Ok((parse(lo)?, parse(hi)?)),
        _ => Ok((0, 0)),
    }
}

async fn open_source_table(
    conn: &mut Conn,
    notes: &mut Vec<StatementNote>,
    source: &SourceEnd,
    utc: bool,
    selected: &SelectedTable,
) -> Result<SourceTable, PreviewError> {
    if !mysql_ident(&selected.schema) || !mysql_ident(&selected.name) {
        return Err(PreviewError::new("config", "ident"));
    }
    let reserved: std::collections::BTreeSet<String> = PG17_RESERVED
        .iter()
        .map(|word| (*word).to_string())
        .collect();
    let catalog = catalog_columns(conn, notes, selected).await?;
    let columns = keep_requested(
        map_table(&catalog, source.boolean_tinyint1, utc, &reserved)?,
        &selected.columns,
    )?;
    let key = columns
        .iter()
        .position(|column| column.key)
        .ok_or_else(|| PreviewError::new("catalog", "primary_key"))?;
    let types = columns
        .iter()
        .map(|column| {
            column
                .plan
                .logical
                .clone()
                .ok_or_else(|| PreviewError::new("catalog", "type"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let names = columns
        .iter()
        .map(|column| column.plan.name.clone())
        .collect();
    if !mysql_ident(&columns[key].plan.name) {
        return Err(PreviewError::new("catalog", "ident"));
    }
    let (lo, hi) = key_extent(conn, selected, &columns[key].plan.name).await?;
    let binding = RunBinding {
        source_id: "mysql-preview-oracle".to_string(),
        schema: selected.schema.clone(),
        table: selected.name.clone(),
        run_generation: 1,
        input_digest: [0; 32],
    };
    let reader = ChunkReader::new(
        &selected.schema,
        &selected.name,
        columns,
        binding,
        CHUNK_MAX_ROWS,
        CHUNK_MAX_BYTES,
        CHUNK_DEADLINE_MS,
        CHUNK_STATEMENT_MS,
    )?;
    Ok(SourceTable {
        reader,
        types,
        names,
        key,
        lo,
        hi,
    })
}

pub(crate) struct SourceRange<'a> {
    reader: &'a ChunkReader,
    conn: &'a mut ReadConnection,
    cursor: ChunkCursor,
    key: usize,
    end: Option<i128>,
    pending: VecDeque<SideRow>,
    done: bool,
}

impl<'a> SourceRange<'a> {
    fn new(
        table: &'a SourceTable,
        conn: &'a mut ReadConnection,
        start: Option<i128>,
        end: Option<i128>,
    ) -> Self {
        Self {
            reader: &table.reader,
            conn,
            cursor: start.map_or_else(ChunkCursor::start, |start| {
                ChunkCursor::resume_after(start - 1)
            }),
            key: table.key,
            end,
            pending: VecDeque::new(),
            done: false,
        }
    }
}

impl RowSource for SourceRange<'_> {
    async fn next_row(&mut self) -> Result<Option<SideRow>, PreviewError> {
        loop {
            if let Some(row) = self.pending.pop_front() {
                if self.end.is_some_and(|end| row.key > end) {
                    self.pending.clear();
                    self.done = true;
                    return Ok(None);
                }
                return Ok(Some(row));
            }
            if self.done {
                return Ok(None);
            }
            let batch = self
                .reader
                .read_chunk(&mut *self.conn, &mut self.cursor, &Cancel::default())
                .await?;
            if batch.terminal {
                self.done = true;
            }
            for row in &batch.typed.rows {
                let cells = row
                    .cells
                    .iter()
                    .map(source_cell)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| PreviewError::new("value", "source_cell"))?;
                let key = cells
                    .get(self.key)
                    .ok_or_else(|| PreviewError::new("value", "row_width"))
                    .and_then(|cell| {
                        bucket_key(cell).map_err(|_| PreviewError::new("value", "key_type"))
                    })?;
                self.pending.push_back(SideRow { key, cells });
            }
        }
    }
}

pub(crate) async fn load_target_table(
    admin: &mut PgConnection,
    relation: &str,
    physical: Vec<String>,
    key: usize,
    types: Vec<LogicalType>,
) -> Result<TargetTable, PreviewError> {
    let rows = sqlx::query(
        "SELECT attname::text, atttypid::bigint, atttypmod FROM pg_attribute \
         WHERE attrelid = $1::text::regclass AND attnum > 0 AND NOT attisdropped",
    )
    .bind(relation)
    .fetch_all(&mut *admin)
    .await
    .map_err(|_| PreviewError::new("target", "catalog"))?;
    let mut catalog = BTreeMap::new();
    for row in &rows {
        let name: String = row
            .try_get(0)
            .map_err(|_| PreviewError::new("target", "catalog"))?;
        let oid: i64 = row
            .try_get(1)
            .map_err(|_| PreviewError::new("target", "catalog"))?;
        let typmod: i32 = row
            .try_get(2)
            .map_err(|_| PreviewError::new("target", "catalog"))?;
        catalog.insert(name, (oid, typmod));
    }
    let mut oids = Vec::with_capacity(physical.len());
    for quoted in &physical {
        let (oid, _) = catalog
            .get(quoted.trim_matches('"'))
            .copied()
            .ok_or_else(|| PreviewError::new("catalog", "coverage"))?;
        oids.push(oid);
    }
    if types.len() != physical.len() {
        return Err(PreviewError::new("catalog", "coverage"));
    }
    Ok(TargetTable {
        relation: relation.to_string(),
        physical,
        oids,
        types,
        key,
    })
}

pub(crate) struct TargetRange<'a> {
    conn: &'a mut PgConnection,
    table: &'a TargetTable,
    start: Option<i128>,
    end: Option<i128>,
    after: Option<i128>,
    pending: VecDeque<SideRow>,
    done: bool,
}

impl<'a> TargetRange<'a> {
    fn new(
        table: &'a TargetTable,
        conn: &'a mut PgConnection,
        start: Option<i128>,
        end: Option<i128>,
    ) -> Self {
        Self {
            conn,
            table,
            start,
            end,
            after: None,
            pending: VecDeque::new(),
            done: false,
        }
    }

    async fn fetch(&mut self) -> Result<(), PreviewError> {
        let table = self.table;
        let key = format!("t.{}", table.physical[table.key]);
        let mut filters = Vec::new();
        if let Some(start) = self.start {
            filters.push(format!("{key} >= {start}"));
        }
        if let Some(after) = self.after {
            filters.push(format!("{key} > {after}"));
        }
        if let Some(end) = self.end {
            filters.push(format!("{key} <= {end}"));
        }
        let columns = table
            .physical
            .iter()
            .enumerate()
            .map(|(index, column)| format!("{column}::text AS c{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let filter = if filters.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", filters.join(" AND "))
        };
        let sql = format!(
            "SELECT {columns} FROM {} AS t{filter} ORDER BY {key} LIMIT {TARGET_PAGE_ROWS}",
            table.relation
        );
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .fetch_all(&mut *self.conn)
            .await
            .map_err(|_| PreviewError::new("target", "oracle"))?;
        for row in &rows {
            let mut cells = Vec::with_capacity(table.oids.len());
            for (index, oid) in table.oids.iter().enumerate() {
                let text: Option<String> = row
                    .try_get(index)
                    .map_err(|_| PreviewError::new("target", "oracle"))?;
                cells.push(
                    pg_text_cell(*oid, text.as_deref())
                        .map_err(|_| PreviewError::new("value", "pg_cell"))?,
                );
            }
            let key_value = cells
                .get(table.key)
                .ok_or_else(|| PreviewError::new("value", "row_width"))
                .and_then(|cell| {
                    bucket_key(cell).map_err(|_| PreviewError::new("value", "key_type"))
                })?;
            self.after = Some(key_value);
            self.pending.push_back(SideRow {
                key: key_value,
                cells,
            });
        }
        self.done = (rows.len() as i64) < TARGET_PAGE_ROWS;
        Ok(())
    }
}

impl RowSource for TargetRange<'_> {
    async fn next_row(&mut self) -> Result<Option<SideRow>, PreviewError> {
        if self.pending.is_empty() && !self.done {
            self.fetch().await?;
        }
        Ok(self.pending.pop_front())
    }
}

async fn source_pass(
    owned: &mut ReadConnection,
    table: &SourceTable,
    digests: &mut BucketDigests,
) -> Result<(), PreviewError> {
    let mut stream = SourceRange::new(table, owned, None, None);
    while let Some(row) = stream.next_row().await? {
        let canonical = canonical_row(&table.types, &row.cells)
            .map_err(|_| PreviewError::new("value", "canonical"))?;
        digests.add(row.key, &canonical)?;
    }
    Ok(())
}

async fn target_pass(
    table: &TargetTable,
    admin: &mut PgConnection,
    digests: &mut BucketDigests,
) -> Result<(), PreviewError> {
    let mut stream = TargetRange::new(table, admin, None, None);
    while let Some(row) = stream.next_row().await? {
        let canonical = canonical_row(&table.types, &row.cells)
            .map_err(|_| PreviewError::new("value", "canonical"))?;
        digests.add(row.key, &canonical)?;
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) async fn source_fingerprints(
    mut conn: Conn,
    source: &SourceEnd,
    tables: &[(&str, &str)],
) -> Result<(Conn, JsonValue), PreviewError> {
    let mut notes = Vec::new();
    let access = inspect(&mut conn, &source.mysql.mysql_user, &mut notes).await?;
    let utc = crate::map::session_utc(&access.time_zone);
    let mut opened = Vec::new();
    for (schema, name) in tables {
        let selected = SelectedTable {
            schema: (*schema).to_string(),
            name: (*name).to_string(),
            columns: Vec::new(),
        };
        opened.push((
            (*name).to_string(),
            open_source_table(&mut conn, &mut notes, source, utc, &selected).await?,
        ));
    }
    let mut owned = ReadConnection::new(conn, source.mysql.clone(), source.password.clone());
    let mut per_table = Map::new();
    let mut totals = Vec::new();
    for (name, table) in &opened {
        let mut digests = BucketDigests::new(KeyBuckets::new(0, 0, 0)?)?;
        source_pass(&mut owned, table, &mut digests).await?;
        per_table.insert(name.clone(), json!(hex(&digests.total().table_digest())));
        totals.push((name.clone(), digests.total()));
    }
    let refs = totals
        .iter()
        .map(|(name, digest)| (name.as_str(), digest))
        .collect::<Vec<_>>();
    let composed = composed_digest(&refs).map_err(|_| PreviewError::new("value", "digest"))?;
    let conn = owned
        .into_connection()
        .map_err(|_| PreviewError::new("protocol", "connection"))?;
    Ok((conn, json!({"tables": per_table, "composed": composed})))
}

pub(crate) async fn key_bucket_compare(
    mut conn: Conn,
    source: &SourceEnd,
    tables: &[BucketTable],
    admin: &mut PgConnection,
    bits: u8,
) -> Result<JsonValue, PreviewError> {
    let mut notes = Vec::new();
    let access = inspect(&mut conn, &source.mysql.mysql_user, &mut notes).await?;
    let utc = crate::map::session_utc(&access.time_zone);
    let mut opened = Vec::new();
    for table in tables {
        let selected = SelectedTable {
            schema: table.schema.clone(),
            name: table.name.clone(),
            columns: Vec::new(),
        };
        let source_table = open_source_table(&mut conn, &mut notes, source, utc, &selected).await?;
        let physical = source_table
            .names
            .iter()
            .map(|name| {
                table
                    .columns
                    .get(name)
                    .cloned()
                    .ok_or_else(|| PreviewError::new("catalog", "coverage"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let target = load_target_table(
            admin,
            &table.relation,
            physical,
            source_table.key,
            source_table.types.clone(),
        )
        .await?;
        opened.push((
            format!("{}.{}", table.schema, table.name),
            source_table,
            target,
        ));
    }
    let mut owned = ReadConnection::new(conn, source.mysql.clone(), source.password.clone());
    let mut reports = Vec::new();
    let mut all_matched = true;
    for (name, source_table, target) in &opened {
        let buckets = KeyBuckets::new(source_table.lo, source_table.hi, bits)?;
        let mut source_digests = BucketDigests::new(buckets)?;
        source_pass(&mut owned, source_table, &mut source_digests).await?;
        let mut target_digests = BucketDigests::new(buckets)?;
        target_pass(target, admin, &mut target_digests).await?;
        let mismatched = source_digests.mismatched(&target_digests);
        let mut tally = DiffTally::default();
        let mut unexplained = 0u64;
        for index in mismatched.iter().take(DRILL_LIMIT) {
            let (start, end) = buckets.range(*index);
            let before = tally.total();
            let mut left = SourceRange::new(source_table, &mut owned, start, end);
            let mut right = TargetRange::new(target, admin, start, end);
            diff_streams(
                &mut left,
                &mut right,
                &source_table.types,
                &target.types,
                &source_table.names,
                &mut tally,
            )
            .await?;
            if tally.total() == before {
                unexplained += 1;
            }
        }
        let matched = mismatched.is_empty();
        all_matched &= matched;
        let mut report = tally.json();
        report["table"] = json!(name);
        report["source_rows"] = json!(source_digests.rows());
        report["target_rows"] = json!(target_digests.rows());
        report["buckets"] = json!(buckets.len());
        report["mismatched_buckets"] = json!(mismatched.len());
        report["drilled_buckets"] = json!(mismatched.len().min(DRILL_LIMIT));
        report["unexplained_buckets"] = json!(unexplained);
        report["matched"] = json!(matched);
        reports.push(report);
    }
    drop(owned);
    Ok(json!({
        "protocol": "oo.mysql-preview.key-bucket-oracle/v1",
        "bucket_bits": bits,
        "page_rows": TARGET_PAGE_ROWS,
        "tables": reports,
        "matched": all_matched,
    }))
}

fn scale_bucket_tables(tables: &BTreeMap<String, Phys>) -> Result<Vec<BucketTable>, PreviewError> {
    SCALE_TABLES
        .iter()
        .map(|name| {
            let table = tables
                .get(*name)
                .ok_or_else(|| PreviewError::new("catalog", "coverage"))?;
            let columns = table
                .columns
                .iter()
                .map(|(column, physical)| {
                    quote_physical_name(physical)
                        .map(|quoted| (column.clone(), quoted))
                        .map_err(|_| PreviewError::new("target", "physical_name"))
                })
                .collect::<Result<BTreeMap<_, _>, _>>()?;
            Ok(BucketTable {
                schema: "logistics".to_string(),
                name: (*name).to_string(),
                relation: format!("\"_oo\".{}", table.physical),
                columns,
            })
        })
        .collect()
}

#[cfg(test)]
mod key_bucket_tests {
    use super::*;
    use oo_core::canonical::LogicalType;
    use oo_pipeline::c1_parts::{Cell, canonical_cell, canonical_row};
    use std::collections::VecDeque;

    struct Rows(VecDeque<SideRow>);

    impl RowSource for Rows {
        async fn next_row(&mut self) -> Result<Option<SideRow>, PreviewError> {
            Ok(self.0.pop_front())
        }
    }

    fn side(rows: &[(i128, Vec<Cell>)]) -> Rows {
        Rows(
            rows.iter()
                .map(|(key, cells)| SideRow {
                    key: *key,
                    cells: cells.clone(),
                })
                .collect(),
        )
    }

    fn text(value: &str) -> Cell {
        Cell::Text(value.to_string())
    }

    fn canon(types: &[LogicalType], cells: &[Cell]) -> Vec<u8> {
        canonical_row(types, cells).expect("canonical row")
    }

    #[test]
    fn bucket_index_and_range_agree_including_the_clamped_edges() {
        let buckets = KeyBuckets::new(-3, 20, 2).expect("buckets");
        assert_eq!(buckets.len(), 4);
        for key in -10..=30i128 {
            let index = buckets.index(key);
            assert!(index < 4);
            for other in 0..4 {
                let (start, end) = buckets.range(other);
                let inside = start.is_none_or(|s| key >= s) && end.is_none_or(|e| key <= e);
                assert_eq!(inside, index == other, "key {key} bucket {other}");
            }
        }
    }

    #[test]
    fn full_signed_and_unsigned_domain_partitions_monotonically() {
        let buckets = KeyBuckets::new(-(1i128 << 63), (1i128 << 64) - 1, 16).expect("buckets");
        let mut last = 0;
        for key in [-(1i128 << 63), -1, 0, 1, 1i128 << 40, (1i128 << 64) - 1] {
            let index = buckets.index(key);
            assert!(index >= last);
            last = index;
        }
        assert_eq!(buckets.index(-(1i128 << 63)), 0);
        assert_eq!(buckets.index((1i128 << 64) - 1), 65_535);
    }

    #[test]
    fn one_bucket_holds_every_key_and_bucket_bits_are_bounded() {
        let single = KeyBuckets::new(0, 0, 0).expect("single");
        assert_eq!(single.index(-5), 0);
        assert_eq!(single.index(7), 0);
        assert_eq!(single.range(0), (None, None));
        assert!(KeyBuckets::new(0, 10, 17).is_err());
        assert!(KeyBuckets::new(10, 0, 4).is_err());
    }

    #[test]
    fn bucket_digests_ignore_order_and_localize_one_changed_cell() {
        let types = vec![LogicalType::Int, LogicalType::Text];
        let rows: Vec<(i128, Vec<Cell>)> = (1..=40)
            .map(|key| (key, vec![Cell::Int(key), text(&format!("v{key}"))]))
            .collect();
        let buckets = KeyBuckets::new(1, 40, 3).expect("buckets");
        let mut forward = BucketDigests::new(buckets).expect("digests");
        for (key, cells) in &rows {
            forward.add(*key, &canon(&types, cells)).expect("add");
        }
        let mut reverse = BucketDigests::new(buckets).expect("digests");
        for (key, cells) in rows.iter().rev() {
            reverse.add(*key, &canon(&types, cells)).expect("add");
        }
        assert!(forward.mismatched(&reverse).is_empty());
        assert_eq!(forward.rows(), 40);

        let mut changed = BucketDigests::new(buckets).expect("digests");
        for (key, cells) in &rows {
            let mut cells = cells.clone();
            if *key == 17 {
                cells[1] = text("v17 ");
            }
            changed.add(*key, &canon(&types, &cells)).expect("add");
        }
        assert_eq!(forward.mismatched(&changed), vec![buckets.index(17)]);
    }

    #[test]
    fn null_and_empty_text_are_different_canonical_cells() {
        let ty = LogicalType::Text;
        assert_ne!(
            canonical_cell(&ty, &Cell::Null).expect("null"),
            canonical_cell(&ty, &text("")).expect("empty")
        );
    }

    #[tokio::test]
    async fn drill_down_reports_missing_extra_and_changed_keys_with_their_column() {
        let types = vec![LogicalType::Int, LogicalType::Text, LogicalType::Text];
        let names = vec!["id".to_string(), "a".to_string(), "b".to_string()];
        let source = [
            (1, vec![Cell::Int(1), text("x"), text("y")]),
            (2, vec![Cell::Int(2), text("x"), text("y")]),
            (3, vec![Cell::Int(3), text("p"), text("q")]),
            (5, vec![Cell::Int(5), text("m"), text("n")]),
            (9, vec![Cell::Int(9), Cell::Null, text("n")]),
        ];
        let target = [
            (1, vec![Cell::Int(1), text("x"), text("y")]),
            (2, vec![Cell::Int(2), text("x"), text("Y")]),
            (4, vec![Cell::Int(4), text("z"), text("w")]),
            (5, vec![Cell::Int(5), text("m"), text("n")]),
            (9, vec![Cell::Int(9), text(""), text("n")]),
        ];
        let mut tally = DiffTally::default();
        diff_streams(
            &mut side(&source),
            &mut side(&target),
            &types,
            &types,
            &names,
            &mut tally,
        )
        .await
        .expect("diff");
        assert_eq!(
            (
                tally.missing_in_target,
                tally.extra_in_target,
                tally.changed
            ),
            (1, 1, 2)
        );
        assert_eq!(
            tally.examples,
            vec![
                json!({"key":"2","kind":"changed","column":"b"}),
                json!({"key":"3","kind":"missing_in_target","column":null}),
                json!({"key":"4","kind":"extra_in_target","column":null}),
                json!({"key":"9","kind":"changed","column":"a"}),
            ]
        );
    }

    #[tokio::test]
    async fn examples_are_capped_but_counts_are_complete() {
        let types = vec![LogicalType::Int];
        let names = vec!["id".to_string()];
        let source: Vec<(i128, Vec<Cell>)> = (1..=100).map(|k| (k, vec![Cell::Int(k)])).collect();
        let mut tally = DiffTally::default();
        diff_streams(
            &mut side(&source),
            &mut side(&[]),
            &types,
            &types,
            &names,
            &mut tally,
        )
        .await
        .expect("diff");
        assert_eq!(tally.missing_in_target, 100);
        assert_eq!(tally.examples.len(), EXAMPLE_LIMIT);
    }
}

#[cfg(test)]
mod key_bucket_live {
    use super::*;
    use oo_connectors::SecretRef;
    use oo_connectors::mysql::connect;
    use sqlx::Connection;
    use sqlx::postgres::PgConnectOptions;
    use std::time::Instant;

    const SCHEMA: &str = "oo_cb_t11";
    const COLUMNS: [&str; 11] = [
        "id", "big_u", "amount", "note", "blob8", "day_on", "stamp", "clock", "dbl", "js", "short",
    ];

    struct Fixture {
        mysql_host: String,
        mysql_port: u16,
        mysql_admin: String,
        mysql_reader: String,
        mysql_ca: std::path::PathBuf,
        pg_host: String,
        pg_port: u16,
        pg_database: String,
    }

    fn fixture() -> Option<Fixture> {
        let path = std::env::var("OO_CB_T11_FIXTURE").ok()?;
        let value: JsonValue = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
        let text =
            |section: &str, key: &str| value[section][key].as_str().unwrap_or("").to_string();
        Some(Fixture {
            mysql_host: text("mysql", "host"),
            mysql_port: value["mysql"]["port"].as_u64()? as u16,
            mysql_admin: text("mysql", "user"),
            mysql_reader: text("mysql", "reader_user"),
            mysql_ca: text("mysql", "ca_file").into(),
            pg_host: text("pg", "host"),
            pg_port: value["pg"]["port"].as_u64()? as u16,
            pg_database: text("pg", "database"),
        })
    }

    fn mysql_config(fx: &Fixture, database: &str, user: &str) -> MysqlConfig {
        MysqlConfig {
            mysql_host: fx.mysql_host.clone(),
            mysql_port: fx.mysql_port,
            mysql_database: database.to_string(),
            mysql_user: user.to_string(),
            mysql_ca_file: fx.mysql_ca.clone(),
            mysql_tls_server_name: None,
            secret_ref: SecretRef::new("secret://mysql-preview/oo-cb-t11".to_string()).unwrap(),
            connect_timeout_ms: 20_000,
        }
    }

    async fn mysql_in(fx: &Fixture, database: &str) -> Conn {
        connect(&mysql_config(fx, database, &fx.mysql_admin), "")
            .await
            .expect("mysql connect")
    }

    async fn mysql(fx: &Fixture) -> Conn {
        connect(&mysql_config(fx, SCHEMA, &fx.mysql_reader), "")
            .await
            .expect("mysql reader connect")
    }

    fn reader_config(fx: &Fixture) -> MysqlConfig {
        mysql_config(fx, SCHEMA, &fx.mysql_reader)
    }

    async fn postgres(fx: &Fixture) -> PgConnection {
        PgConnectOptions::from_explicit()
            .host(&fx.pg_host)
            .port(fx.pg_port)
            .username("postgres")
            .database(&fx.pg_database)
            .ssl_mode(PgSslMode::Disable)
            .connect()
            .await
            .expect("postgres connect")
    }

    fn main_row(id: i64, pg: bool) -> String {
        let big_u = if id == 1 {
            "18446744073709551615".to_string()
        } else {
            (id as u64 * 1_000_000_007).to_string()
        };
        let amount = if id % 7 == 0 {
            "NULL".to_string()
        } else {
            format!("'{}.{:04}'", id * 13 % 100_000, id % 10_000)
        };
        let note = if id % 5 == 0 {
            "NULL".to_string()
        } else {
            format!("'note {id} caf\u{e9}'")
        };
        let blob = if pg {
            format!("'\\x{:04x}'", id % 65_536)
        } else {
            format!("X'{:04X}'", id % 65_536)
        };
        let day = format!("'2026-{:02}-{:02}'", id % 12 + 1, id % 28 + 1);
        let stamp = format!("'2026-01-02 03:04:05.{:06}'", id % 1_000_000);
        let clock = if id % 3 == 0 {
            "'-01:00:00'".to_string()
        } else {
            format!("'{:02}:{:02}:{:02}'", id % 24, id % 60, (id * 7) % 60)
        };
        let js = format!("'{{\"a\": [1, 2], \"b\": {id}}}'");
        format!(
            "({id},{big_u},{amount},{note},{blob},{day},{stamp},{clock},{}.5,{js},'s{id}')",
            id
        )
    }

    const COLUMN_LIST: &str = "id,big_u,amount,note,blob8,day_on,stamp,clock,dbl,js,short";

    async fn load_main(my: &mut Conn, pg: &mut PgConnection, rows: i64) {
        my.query_drop(format!("DROP DATABASE IF EXISTS {SCHEMA}"))
            .await
            .unwrap();
        my.query_drop(format!("CREATE DATABASE {SCHEMA} DEFAULT CHARSET utf8mb4"))
            .await
            .unwrap();
        my.query_drop("CREATE USER IF NOT EXISTS 'oo_ro'@'%' IDENTIFIED BY ''")
            .await
            .unwrap();
        my.query_drop(format!("GRANT SELECT ON {SCHEMA}.* TO 'oo_ro'@'%'"))
            .await
            .unwrap();
        my.query_drop(format!(
            "CREATE TABLE {SCHEMA}.t_main (id BIGINT NOT NULL PRIMARY KEY, big_u BIGINT UNSIGNED NOT NULL, \
             amount DECIMAL(38,4) NULL, note TEXT NULL, blob8 VARBINARY(8) NULL, day_on DATE NULL, \
             stamp DATETIME(6) NULL, clock TIME NULL, dbl DOUBLE NOT NULL, js JSON NULL, short VARCHAR(20) NOT NULL) \
             ENGINE=InnoDB DEFAULT CHARSET=utf8mb4"
        ))
        .await
        .unwrap();
        sqlx::query("DROP TABLE IF EXISTS _oo.t11_main")
            .execute(&mut *pg)
            .await
            .unwrap();
        sqlx::query("CREATE SCHEMA IF NOT EXISTS _oo")
            .execute(&mut *pg)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE _oo.t11_main (id int8 PRIMARY KEY, big_u numeric(20,0) NOT NULL, \
             amount numeric(38,4), note text, blob8 bytea, day_on date, stamp timestamp, clock interval, \
             dbl float8 NOT NULL, js json, short varchar(20) NOT NULL)",
        )
        .execute(&mut *pg)
        .await
        .unwrap();
        for start in (1..=rows).step_by(500) {
            let end = (start + 499).min(rows);
            let my_values: Vec<String> = (start..=end).map(|id| main_row(id, false)).collect();
            my.query_drop(format!(
                "INSERT INTO {SCHEMA}.t_main ({COLUMN_LIST}) VALUES {}",
                my_values.join(",")
            ))
            .await
            .unwrap();
            let pg_values: Vec<String> = (start..=end).map(|id| main_row(id, true)).collect();
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "INSERT INTO _oo.t11_main ({COLUMN_LIST}) VALUES {}",
                pg_values.join(",")
            )))
            .execute(&mut *pg)
            .await
            .unwrap();
        }
    }

    fn main_table() -> BucketTable {
        BucketTable {
            schema: SCHEMA.to_string(),
            name: "t_main".to_string(),
            relation: "\"_oo\".\"t11_main\"".to_string(),
            columns: COLUMNS
                .iter()
                .map(|name| ((*name).to_string(), format!("\"{name}\"")))
                .collect(),
        }
    }

    async fn compare(fx: &Fixture, admin: &mut PgConnection, bits: u8) -> JsonValue {
        let source = SourceEnd {
            mysql: reader_config(fx),
            password: Zeroizing::new(String::new()),
            boolean_tinyint1: false,
        };
        key_bucket_compare(mysql(fx).await, &source, &[main_table()], admin, bits)
            .await
            .expect("key-bucket compare")
    }

    fn table_report(report: &JsonValue) -> &JsonValue {
        &report["tables"][0]
    }

    #[tokio::test]
    #[ignore = "requires OO_CB_T11_FIXTURE: owned MySQL 5.7 and PostgreSQL 17 containers"]
    async fn key_bucket_live_verifies_equal_tables_and_localizes_each_injected_difference() {
        let Some(fx) = fixture() else {
            panic!("OO_CB_T11_FIXTURE")
        };
        let mut my = mysql_in(&fx, "mysql").await;
        let mut pg = postgres(&fx).await;
        load_main(&mut my, &mut pg, 3000).await;

        let equal = compare(&fx, &mut pg, 6).await;
        assert_eq!(equal["matched"], json!(true), "{equal}");
        assert_eq!(table_report(&equal)["source_rows"], json!(3000));
        assert_eq!(table_report(&equal)["target_rows"], json!(3000));
        assert_eq!(table_report(&equal)["mismatched_buckets"], json!(0));

        let scenarios: [(&str, &str, JsonValue); 6] = [
            (
                "one changed text cell",
                "UPDATE _oo.t11_main SET note = 'changed' WHERE id = 1234",
                json!([{"key":"1234","kind":"changed","column":"note"}]),
            ),
            (
                "missing row",
                "DELETE FROM _oo.t11_main WHERE id = 2000",
                json!([{"key":"2000","kind":"missing_in_target","column":null}]),
            ),
            (
                "extra row above the source key range",
                &format!(
                    "INSERT INTO _oo.t11_main ({COLUMN_LIST}) VALUES {}",
                    main_row(3001, true)
                ),
                json!([{"key":"3001","kind":"extra_in_target","column":null}]),
            ),
            (
                "NULL in the source, empty string in the target",
                "UPDATE _oo.t11_main SET note = '' WHERE id = 5",
                json!([{"key":"5","kind":"changed","column":"note"}]),
            ),
            (
                "NULL in the source, zero in the target",
                "UPDATE _oo.t11_main SET amount = 0 WHERE id = 7",
                json!([{"key":"7","kind":"changed","column":"amount"}]),
            ),
            (
                "one changed json cell",
                "UPDATE _oo.t11_main SET js = '{\"a\": [1, 2], \"b\": 99}' WHERE id = 2999",
                json!([{"key":"2999","kind":"changed","column":"js"}]),
            ),
        ];
        for (label, mutation, expected) in scenarios {
            let mut tx = pg.begin().await.unwrap();
            sqlx::query(sqlx::AssertSqlSafe(mutation.to_string()))
                .execute(&mut *tx)
                .await
                .unwrap();
            let report = compare(&fx, &mut tx, 6).await;
            tx.rollback().await.unwrap();
            assert_eq!(report["matched"], json!(false), "{label}: {report}");
            let table = table_report(&report);
            assert_eq!(table["examples"], expected, "{label}: {report}");
            assert_eq!(table["unexplained_buckets"], json!(0), "{label}: {report}");
        }

        let after = compare(&fx, &mut pg, 6).await;
        assert_eq!(
            after["matched"],
            json!(true),
            "rollback restores the equal state"
        );
    }

    fn peak_rss_kib() -> u64 {
        fs::read_to_string("/proc/self/status")
            .unwrap_or_default()
            .lines()
            .find_map(|line| line.strip_prefix("VmHWM:"))
            .and_then(|value| value.split_whitespace().next()?.parse().ok())
            .unwrap_or(0)
    }

    #[tokio::test]
    #[ignore = "requires OO_CB_T11_FIXTURE and OO_CB_T11_BIG_ROWS: prints rows/s and peak RSS"]
    async fn key_bucket_live_big_table_runs_in_bounded_memory() {
        let Some(fx) = fixture() else {
            panic!("OO_CB_T11_FIXTURE")
        };
        let rows: i64 = std::env::var("OO_CB_T11_BIG_ROWS")
            .expect("OO_CB_T11_BIG_ROWS")
            .parse()
            .expect("rows");
        let mut my = mysql_in(&fx, "mysql").await;
        let mut pg = postgres(&fx).await;
        my.query_drop(format!("DROP DATABASE IF EXISTS {SCHEMA}"))
            .await
            .unwrap();
        my.query_drop(format!("CREATE DATABASE {SCHEMA} DEFAULT CHARSET utf8mb4"))
            .await
            .unwrap();
        my.query_drop("CREATE USER IF NOT EXISTS 'oo_ro'@'%' IDENTIFIED BY ''")
            .await
            .unwrap();
        my.query_drop(format!("GRANT SELECT ON {SCHEMA}.* TO 'oo_ro'@'%'"))
            .await
            .unwrap();
        my.query_drop(format!(
            "CREATE TABLE {SCHEMA}.n_seq (n INT NOT NULL PRIMARY KEY)"
        ))
        .await
        .unwrap();
        let seq: Vec<String> = (0..1000).map(|n| format!("({n})")).collect();
        my.query_drop(format!(
            "INSERT INTO {SCHEMA}.n_seq (n) VALUES {}",
            seq.join(",")
        ))
        .await
        .unwrap();
        my.query_drop(format!(
            "CREATE TABLE {SCHEMA}.t_big (id BIGINT NOT NULL PRIMARY KEY, payload TEXT NOT NULL) ENGINE=InnoDB"
        ))
        .await
        .unwrap();
        my.query_drop(format!(
            "INSERT INTO {SCHEMA}.t_big SELECT a.n*1000+b.n+1, REPEAT(MD5(a.n*1000+b.n+1), 28) \
             FROM {SCHEMA}.n_seq a CROSS JOIN {SCHEMA}.n_seq b WHERE a.n*1000+b.n < {rows}"
        ))
        .await
        .unwrap();
        sqlx::query("DROP TABLE IF EXISTS _oo.t11_big")
            .execute(&mut pg)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE _oo.t11_big (id int8 PRIMARY KEY, payload text NOT NULL)")
            .execute(&mut pg)
            .await
            .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO _oo.t11_big SELECT g, repeat(md5(g::text), 28) FROM generate_series(1, {rows}) g"
        )))
        .execute(&mut pg)
        .await
        .unwrap();

        let big = BucketTable {
            schema: SCHEMA.to_string(),
            name: "t_big".to_string(),
            relation: "\"_oo\".\"t11_big\"".to_string(),
            columns: [("id", "\"id\""), ("payload", "\"payload\"")]
                .into_iter()
                .map(|(name, quoted)| (name.to_string(), quoted.to_string()))
                .collect(),
        };
        let source = SourceEnd {
            mysql: reader_config(&fx),
            password: Zeroizing::new(String::new()),
            boolean_tinyint1: false,
        };
        let before_kib = peak_rss_kib();
        let started = Instant::now();
        let report =
            key_bucket_compare(mysql(&fx).await, &source, &[big], &mut pg, KEY_BUCKET_BITS)
                .await
                .expect("compare");
        let elapsed = started.elapsed().as_secs_f64();
        let peak_kib = peak_rss_kib();
        let table = table_report(&report);
        println!(
            "key_bucket_big rows={rows} matched={} elapsed_s={elapsed:.1} rows_per_s={:.0} \
             peak_rss_before_kib={before_kib} peak_rss_after_kib={peak_kib} buckets={}",
            report["matched"],
            rows as f64 / elapsed,
            table["buckets"],
        );
        assert_eq!(report["matched"], json!(true), "{report}");
        assert_eq!(table["source_rows"], json!(rows));
    }
}
