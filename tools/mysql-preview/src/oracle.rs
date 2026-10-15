//! Independent typed oracle for the logistics scale fixture.
//!
//! Expectations come from the formula module. Source rows are read with the
//! business account. Target rows are read with the PostgreSQL administrator
//! after a seal. Question text is not an input.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Row, Value};
use oo_pipeline::physical_mapping::quote_physical_name;
use serde_json::{Map, Value as JsonValue, json};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Row as PgRow};
use tokio::time::timeout;

use crate::config::{PreviewConfig, read_ca, read_secret};
use crate::error::PreviewError;
use crate::logistics::{
    EXACT_COST, NEGATIVE_MILL, NON_FLOAT_COST, ProbeDate, ProbeTime, oracle, probe_attnum,
};
use crate::map::{grant_is_read_only, session_mode_ok};
use crate::source::open_mysql;

const PRESERVED_RUN: &str = "de8276fc-8634-4543-994f-ce82a46a3c18";

pub async fn audit_source(
    config_path: &Path,
    out_path: &Path,
    previous: Option<&Path>,
) -> Result<(), PreviewError> {
    let (config, config_bytes) = read_preview(config_path)?;
    let password = read_secret(&config.mysql_password_file)?;
    let mut conn = open_mysql(&config, password.as_str()).await?;
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
    let checksums = checksums(&mut conn).await?;
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
        let prior_sums = prior.get("checksums").cloned().unwrap_or(JsonValue::Null);
        if prior_checks != json!(checks) || prior_sums != json!(checksums) {
            failures.push("changed".to_string());
        }
    }
    let _ = config_bytes;
    let body = json!({
        "protocol": "oo.mysql-preview.scale-audit/v1",
        "checks": checks,
        "checksums": checksums,
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
    let body = json!({
        "protocol": "oo.mysql-preview.scale-target/v1",
        "failures": failures,
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

async fn checksums(conn: &mut Conn) -> Result<BTreeMap<String, String>, PreviewError> {
    let rows: Vec<Row> = timeout(
        Duration::from_secs(120),
        conn.query("CHECKSUM TABLE logistics.hubs, logistics.shipments, logistics.tracking_events, logistics.value_probe, logistics.range_gap"),
    )
    .await
    .map_err(|_| PreviewError::new("budget", "statement_timeout"))?
    .map_err(|_| PreviewError::new("protocol", "query"))?;
    let mut out = BTreeMap::new();
    for row in rows {
        out.insert(cell_string(&row, 0)?, cell_string(&row, 1)?);
    }
    if out.len() != 5 {
        return Err(PreviewError::new("value", "checksum"));
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
