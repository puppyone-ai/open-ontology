//! DBA setup for the synthetic logistics schema.
//!
//! This is not the business read path. It opens the setup account, creates
//! `logistics`, and loads formula rows. It never opens `erp` except to count
//! the preserved prototype tables.

use std::fs;
use std::path::Path;
use std::time::Duration;

use mysql_async::prelude::Queryable;
use mysql_async::{Conn, OptsBuilder, Row, SslOpts, Value};
use serde_json::{json, Map, Value as JsonValue};
use tokio::time::timeout;

use crate::config::{mysql_ident, read_ca, read_secret};
use crate::error::{redact, PreviewError};
use crate::logistics::{
    badge, capacity_grams, cost_text, created_at, dest_hub, event_at, event_comment, event_hub, event_shipment,
    event_status, hub_code, hub_name, hub_note, latitude, longitude, mysql_wall, opened_at, oracle, origin_hub,
    payload, probe_rows, promised_at, questions_document, range_gaps, recorded_weight, region_code, seal,
    shipment_comment, shipment_status, transport_mode, weight_grams, ProbeDate, ProbeRow, ProbeTime, HUB_ROWS,
    SHIPMENT_ROWS, EVENT_ROWS,
};

const SCHEMA: &str = "logistics";
const BUSINESS_USER: &str = "oo_scale";

pub async fn load_logistics(config_path: &Path, out_dir: &Path) -> Result<(), PreviewError> {
    let config_bytes = fs::read(config_path).map_err(|_| PreviewError::new("config", "path"))?;
    let text = std::str::from_utf8(&config_bytes).map_err(|_| PreviewError::new("config", "json"))?;
    let config = parse_dba(text)?;
    let password = read_secret(&config.password_file)?;
    let business_password = read_secret(&config.business_password_file)?;
    if !business_password.bytes().all(|byte| byte.is_ascii_hexdigit()) || business_password.len() < 16 {
        return Err(PreviewError::new("config", "password_file"));
    }
    fs::create_dir_all(out_dir).map_err(|_| PreviewError::new("config", "out"))?;
    let mut conn = open_dba(&config, password.as_str()).await?;
    let before = prototype_counts(&mut conn).await?;
    exec(&mut conn, "CREATE DATABASE IF NOT EXISTS logistics CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_ai_ci").await?;
    exec(&mut conn, "USE logistics").await?;
    for statement in DROP_STATEMENTS {
        exec(&mut conn, statement).await?;
    }
    for statement in CREATE_STATEMENTS {
        exec(&mut conn, statement).await?;
    }
    ensure_business_user(&mut conn, &business_password).await?;
    exec(&mut conn, "SET SESSION time_zone = '+00:00'").await?;
    exec(&mut conn, STRICT_MODE).await?;
    insert_hubs(&mut conn).await?;
    insert_shipments(&mut conn).await?;
    insert_events(&mut conn).await?;
    insert_probe(&mut conn).await?;
    insert_gaps(&mut conn).await?;
    exec(&mut conn, STRICT_MODE).await?;
    exec(&mut conn, "GRANT SELECT ON logistics.* TO 'oo_scale'@'%'").await?;
    let counts = table_counts(&mut conn).await?;
    let raw = raw_probe(&mut conn).await?;
    let after = prototype_counts(&mut conn).await?;
    if before != after || after.parcel != 5 || after.empty_box != 0 {
        return Err(PreviewError::new("target", "prototype"));
    }
    let facts = oracle();
    let logistics_rows = counts.hubs.saturating_add(counts.shipments).saturating_add(counts.events);
    if counts.hubs != facts.hub_rows
        || counts.shipments != facts.shipment_rows
        || counts.events != facts.event_rows
        || counts.probe != facts.probe_rows
        || counts.gaps != facts.gap_rows
        || logistics_rows != facts.logistics_rows
        || facts.shipment_cost_exact != crate::logistics::EXACT_COST
    {
        return Err(PreviewError::new("value", "count"));
    }
    if raw.zero_date != "0000-00-00" || raw.time_range != "-01:00:00" {
        return Err(PreviewError::new("value", "raw"));
    }
    let body = json!({
        "protocol": "oo.mysql-preview.dba-load/v1",
        "database": SCHEMA,
        "business_user": BUSINESS_USER,
        "counts": counts.json(),
        "prototype_parcel": after.parcel,
        "prototype_empty_box": after.empty_box,
        "raw_status": {"zero_date": raw.zero_date, "time_range": raw.time_range},
        "range_reason": crate::logistics::RANGE_REASON,
        "questions_separated": true
    });
    fs::write(out_dir.join("load.json"), serde_json::to_vec_pretty(&body).map_err(|_| PreviewError::new("config", "out"))?)
        .map_err(|_| PreviewError::new("config", "out"))?;
    fs::write(
        out_dir.join("questions.json"),
        serde_json::to_vec_pretty(&questions_document()).map_err(|_| PreviewError::new("config", "out"))?,
    )
    .map_err(|_| PreviewError::new("config", "out"))?;
    drop(conn);
    let _ = password;
    let _ = business_password;
    Ok(())
}

pub async fn guard_source(config_path: &Path, out_path: &Path) -> Result<(), PreviewError> {
    let config_bytes = fs::read(config_path).map_err(|_| PreviewError::new("config", "path"))?;
    let text = std::str::from_utf8(&config_bytes).map_err(|_| PreviewError::new("config", "json"))?;
    let config = parse_dba(text)?;
    let password = read_secret(&config.password_file)?;
    let mut conn = open_dba(&config, password.as_str()).await?;
    let prototype = prototype_counts(&mut conn).await?;
    let counts = table_counts(&mut conn).await?;
    let retired = scalar(&mut conn, "SELECT COUNT(*) FROM mysql.user WHERE user IN ('oo_read', 'oo_write')").await?;
    let grants = strings(&mut conn, "SHOW GRANTS FOR 'oo_scale'@'%'").await?;
    let read_only = !grants.is_empty()
        && grants.iter().all(|line| {
            let upper = line.to_ascii_uppercase();
            !upper.contains("INSERT") && !upper.contains("UPDATE") && !upper.contains("DELETE") && !upper.contains(" ALL ") && !upper.contains("ERP")
        })
        && grants.iter().any(|line| line.to_ascii_uppercase().contains("SELECT"));
    if prototype.parcel != 5 || prototype.empty_box != 0 || retired != "0" || !read_only {
        return Err(PreviewError::new("privilege", "guard"));
    }
    let body = json!({
        "protocol": "oo.mysql-preview.scale-guard/v1",
        "prototype_parcel": prototype.parcel,
        "prototype_empty_box": prototype.empty_box,
        "retired_source_users": retired,
        "business_grants_read_only": read_only,
        "counts": counts.json()
    });
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).map_err(|_| PreviewError::new("config", "out"))?;
    }
    fs::write(out_path, serde_json::to_vec_pretty(&body).map_err(|_| PreviewError::new("config", "out"))?)
        .map_err(|_| PreviewError::new("config", "out"))?;
    Ok(())
}

struct DbaConfig {
    host: String,
    port: u16,
    password_file: std::path::PathBuf,
    ca_file: std::path::PathBuf,
    business_password_file: std::path::PathBuf,
    connect_timeout_ms: u64,
}

struct PrototypeCounts {
    parcel: i64,
    empty_box: i64,
}

impl PartialEq for PrototypeCounts {
    fn eq(&self, other: &Self) -> bool {
        self.parcel == other.parcel && self.empty_box == other.empty_box
    }
}

struct TableCounts {
    hubs: i64,
    shipments: i64,
    events: i64,
    probe: i64,
    gaps: i64,
}

impl TableCounts {
    fn json(&self) -> JsonValue {
        json!({
            "hubs": self.hubs,
            "shipments": self.shipments,
            "tracking_events": self.events,
            "value_probe": self.probe,
            "range_gap": self.gaps
        })
    }
}

struct RawProbe {
    zero_date: String,
    time_range: String,
}



fn parse_dba(text: &str) -> Result<DbaConfig, PreviewError> {
    let value: JsonValue = serde_json::from_str(text).map_err(|_| PreviewError::new("config", "json"))?;
    reject_secret_keys(&value)?;
    let map = value.as_object().ok_or_else(|| PreviewError::new("config", "json"))?;
    exact(map, &["business_password_file", "ca_file", "connect_timeout_ms", "database", "host", "password_file", "port", "user", "version"])?;
    if map.get("version").and_then(JsonValue::as_str) != Some("oo.mysql-preview.dba-load/v1") {
        return Err(PreviewError::new("config", "version"));
    }
    if map.get("database").and_then(JsonValue::as_str) != Some(SCHEMA) {
        return Err(PreviewError::new("config", "database"));
    }
    if map.get("user").and_then(JsonValue::as_str) != Some("root") {
        return Err(PreviewError::new("config", "user"));
    }
    let host = map.get("host").and_then(JsonValue::as_str).ok_or_else(|| PreviewError::new("config", "host"))?;
    if host.is_empty() || host.len() > 253 || !host.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-') {
        return Err(PreviewError::new("config", "host"));
    }
    let port = map.get("port").and_then(JsonValue::as_u64).and_then(|value| u16::try_from(value).ok()).filter(|port| *port != 0).ok_or_else(|| PreviewError::new("config", "port"))?;
    let timeout_ms = map.get("connect_timeout_ms").and_then(JsonValue::as_u64).filter(|value| (1..=30_000).contains(value)).ok_or_else(|| PreviewError::new("budget", "range"))?;
    Ok(DbaConfig {
        host: host.to_string(),
        port,
        password_file: absolute(map, "password_file")?,
        ca_file: absolute(map, "ca_file")?,
        business_password_file: absolute(map, "business_password_file")?,
        connect_timeout_ms: timeout_ms,
    })
}

fn absolute(map: &Map<String, JsonValue>, key: &str) -> Result<std::path::PathBuf, PreviewError> {
    let text = map.get(key).and_then(JsonValue::as_str).ok_or_else(|| PreviewError::new("config", "path"))?;
    let path = std::path::PathBuf::from(text);
    if path.is_absolute() { Ok(path) } else { Err(PreviewError::new("config", "path")) }
}

fn exact(map: &Map<String, JsonValue>, allowed: &[&str]) -> Result<(), PreviewError> {
    if map.len() != allowed.len() || allowed.iter().any(|key| !map.contains_key(*key)) {
        Err(PreviewError::new("config", "keys"))
    } else {
        Ok(())
    }
}

fn reject_secret_keys(value: &JsonValue) -> Result<(), PreviewError> {
    match value {
        JsonValue::Object(map) => {
            for (key, child) in map {
                if matches!(key.as_str(), "password" | "passwd" | "secret" | "token" | "private_key" | "ca_pem") {
                    return Err(PreviewError::new("config", "secret_key"));
                }
                reject_secret_keys(child)?;
            }
            Ok(())
        }
        JsonValue::Array(items) => {
            for item in items {
                reject_secret_keys(item)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

async fn open_dba(config: &DbaConfig, password: &str) -> Result<Conn, PreviewError> {
    let ca = read_ca(&config.ca_file)?;
    let _ = ca;
    let ssl = SslOpts::default()
        .with_root_certs(vec![config.ca_file.clone().into()])
        .with_danger_accept_invalid_certs(false)
        .with_danger_skip_domain_validation(false)
        .with_disable_built_in_roots(true);
    let opts = OptsBuilder::default()
        .ip_or_hostname(config.host.clone())
        .tcp_port(config.port)
        .user(Some("root".to_string()))
        .pass(Some(password.to_string()))
        .prefer_socket(false)
        .ssl_opts(ssl);
    let connect = timeout(Duration::from_millis(config.connect_timeout_ms), Conn::new(opts)).await;
    match connect {
        Ok(Ok(conn)) => Ok(conn),
        Ok(Err(err)) => {
            if redact(&err.to_string(), password) {
                Err(PreviewError::new("connect", "redacted"))
            } else {
                Err(PreviewError::new("connect", "dba"))
            }
        }
        Err(_) => Err(PreviewError::new("connect", "connect_timeout")),
    }
}

async fn ensure_business_user(conn: &mut Conn, password: &str) -> Result<(), PreviewError> {
    if !mysql_ident(BUSINESS_USER) {
        return Err(PreviewError::new("ident", "user"));
    }
    let quoted = sql_text(password)?;
    exec(conn, &format!("CREATE USER IF NOT EXISTS 'oo_scale'@'%' IDENTIFIED BY {quoted}")).await?;
    exec(conn, &format!("ALTER USER 'oo_scale'@'%' IDENTIFIED BY {quoted}")).await?;
    exec(conn, "REVOKE ALL PRIVILEGES, GRANT OPTION FROM 'oo_scale'@'%'").await?;
    Ok(())
}

async fn insert_hubs(conn: &mut Conn) -> Result<(), PreviewError> {
    let mut rows = Vec::new();
    for id in 1..=HUB_ROWS {
        let opened = opened_at(id).as_ref().and_then(|stamp| mysql_wall(stamp));
        let note = hub_note(id);
        let lat = latitude(id);
        let lon = longitude(id);
        let bytes = badge(id);
        rows.push(format!(
            "({},{},{},{},{},{},{},{},{},{})",
            id,
            sql_text(&hub_code(id))?,
            sql_text(&hub_name(id))?,
            sql_opt(region_code(id))?,
            sql_decimal(lat.as_deref())?,
            sql_decimal(lon.as_deref())?,
            sql_i64(capacity_grams(id)),
            sql_opt(opened.as_deref())?,
            sql_bytes(bytes.as_deref()),
            sql_opt(note.as_deref())?,
        ));
        if rows.len() == 200 {
            flush(conn, "hubs", "(hub_id, hub_code, hub_name, region_code, latitude, longitude, capacity_grams, opened_at, badge, note)", &rows).await?;
            rows.clear();
        }
    }
    flush(conn, "hubs", "(hub_id, hub_code, hub_name, region_code, latitude, longitude, capacity_grams, opened_at, badge, note)", &rows).await
}

async fn insert_shipments(conn: &mut Conn) -> Result<(), PreviewError> {
    let mut rows = Vec::new();
    for id in 1..=SHIPMENT_ROWS {
        let weight = weight_grams(id);
        let cost = cost_text(id);
        let created = mysql_wall(&created_at(id));
        let promised = promised_at(id).and_then(mysql_wall);
        let bytes = seal(id);
        let comment = shipment_comment(id);
        rows.push(format!(
            "({},{},{},{},{},{},{},{},{},{},{})",
            id,
            origin_hub(id),
            dest_hub(id),
            sql_text(shipment_status(id))?,
            sql_text(transport_mode(id))?,
            sql_opt_i64(weight),
            sql_decimal(cost.as_deref())?,
            sql_opt(created.as_deref())?,
            sql_opt(promised.as_deref())?,
            sql_bytes(bytes.as_deref()),
            sql_opt(comment.as_deref())?,
        ));
        if rows.len() == 200 {
            flush(conn, "shipments", "(shipment_id, origin_hub_id, dest_hub_id, status, transport_mode, weight_grams, cost_amount, created_at, promised_at, seal, comment)", &rows).await?;
            rows.clear();
        }
    }
    flush(conn, "shipments", "(shipment_id, origin_hub_id, dest_hub_id, status, transport_mode, weight_grams, cost_amount, created_at, promised_at, seal, comment)", &rows).await
}

async fn insert_events(conn: &mut Conn) -> Result<(), PreviewError> {
    let mut rows = Vec::new();
    for id in 1..=EVENT_ROWS {
        let at = event_at(id).as_ref().and_then(|stamp| mysql_wall(stamp));
        let weight = recorded_weight(id);
        let bytes = payload(id);
        let comment = event_comment(id);
        let hub = event_hub(id);
        rows.push(format!(
            "({},{},{},{},{},{},{},{})",
            id,
            event_shipment(id),
            sql_opt_i64(hub),
            sql_text(event_status(id))?,
            sql_opt(at.as_deref())?,
            sql_opt_i64(weight),
            sql_bytes(bytes.as_deref()),
            sql_opt(comment.as_deref())?,
        ));
        if rows.len() == 200 {
            flush(conn, "tracking_events", "(event_id, shipment_id, hub_id, status, event_at, recorded_weight_grams, payload, comment)", &rows).await?;
            rows.clear();
        }
    }
    flush(conn, "tracking_events", "(event_id, shipment_id, hub_id, status, event_at, recorded_weight_grams, payload, comment)", &rows).await
}

async fn insert_probe(conn: &mut Conn) -> Result<(), PreviewError> {
    let rows = probe_rows();
    let strict: Vec<&ProbeRow> = rows.iter().filter(|row| row.day_on != ProbeDate::Zero).collect();
    let zero: Vec<&ProbeRow> = rows.iter().filter(|row| row.day_on == ProbeDate::Zero).collect();
    flush_probe(conn, &strict).await?;
    exec(conn, LOOSE_MODE).await?;
    flush_probe(conn, &zero).await?;
    exec(conn, STRICT_MODE).await
}

async fn flush_probe(conn: &mut Conn, rows: &[&ProbeRow]) -> Result<(), PreviewError> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut values = Vec::new();
    for row in rows {
        values.push(format!(
            "({},{},{},{},{},{},{},{})",
            row.probe_id,
            row.unsigned_grams,
            sql_decimal(row.exact_amount)?,
            sql_opt(row.payload)?,
            sql_bytes(row.badge),
            probe_date(row.day_on)?,
            probe_time(row.clock_at)?,
            sql_opt(row.note)?,
        ));
    }
    flush(conn, "value_probe", "(probe_id, unsigned_grams, exact_amount, payload, badge, day_on, clock_at, note)", &values).await
}

async fn insert_gaps(conn: &mut Conn) -> Result<(), PreviewError> {
    let gaps = range_gaps();
    let mut values = Vec::new();
    for gap in &gaps {
        values.push(format!(
            "({},{},{},{},{},{})",
            gap.gap_id,
            sql_text(gap.table_name)?,
            sql_text(gap.column_name)?,
            gap.row_key,
            sql_text(gap.reason)?,
            sql_text(&gap.canonical_text)?,
        ));
    }
    flush(conn, "range_gap", "(gap_id, table_name, column_name, row_key, reason, canonical_text)", &values).await
}

async fn flush(conn: &mut Conn, table: &str, columns: &str, rows: &[String]) -> Result<(), PreviewError> {
    if rows.is_empty() {
        return Ok(());
    }
    if !mysql_ident(table) {
        return Err(PreviewError::new("ident", "ident"));
    }
    let sql = format!("INSERT INTO `{table}` {columns} VALUES {}", rows.join(","));
    exec(conn, &sql).await
}

fn probe_date(value: ProbeDate) -> Result<String, PreviewError> {
    match value {
        ProbeDate::Null => Ok("NULL".to_string()),
        ProbeDate::Zero => Ok("'0000-00-00'".to_string()),
        ProbeDate::Day(text) => {
            if text.len() == 10 && text.as_bytes()[4] == b'-' && text.as_bytes()[7] == b'-' && text.bytes().all(|byte| byte.is_ascii_digit() || byte == b'-') {
                Ok(format!("'{text}'"))
            } else {
                Err(PreviewError::new("value", "date"))
            }
        }
    }
}

fn probe_time(value: ProbeTime) -> Result<String, PreviewError> {
    match value {
        ProbeTime::Null => Ok("NULL".to_string()),
        ProbeTime::Clock(text) | ProbeTime::Negative(text) => {
            let body = text.strip_prefix('-').unwrap_or(text);
            if body.len() == 8 && body.as_bytes()[2] == b':' && body.as_bytes()[5] == b':' && body.bytes().all(|byte| byte.is_ascii_digit() || byte == b':') {
                Ok(format!("'{text}'"))
            } else {
                Err(PreviewError::new("value", "time_range"))
            }
        }
    }
}

fn sql_text(value: &str) -> Result<String, PreviewError> {
    if value.contains('\0') {
        return Err(PreviewError::new("value", "text_nul"));
    }
    let mut out = String::from("'");
    for char in value.chars() {
        match char {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            _ => out.push(char),
        }
    }
    out.push('\'');
    Ok(out)
}

fn sql_opt(value: Option<&str>) -> Result<String, PreviewError> {
    match value {
        None => Ok("NULL".to_string()),
        Some(text) => sql_text(text),
    }
}

fn sql_decimal(value: Option<&str>) -> Result<String, PreviewError> {
    let Some(text) = value else {
        return Ok("NULL".to_string());
    };
    if text.is_empty() || text.len() > 80 || !text.bytes().all(|byte| byte.is_ascii_digit() || byte == b'.' || byte == b'-') {
        return Err(PreviewError::new("value", "decimal"));
    }
    Ok(text.to_string())
}

fn sql_i64(value: i64) -> String {
    if value == i64::MIN {
        "-9223372036854775807-1".to_string()
    } else {
        value.to_string()
    }
}

fn sql_opt_i64(value: Option<i64>) -> String {
    match value {
        None => "NULL".to_string(),
        Some(number) => sql_i64(number),
    }
}

fn sql_bytes(value: Option<&[u8]>) -> String {
    match value {
        None => "NULL".to_string(),
        Some(bytes) => {
            let mut out = String::from("x'");
            for byte in bytes {
                out.push_str(&format!("{byte:02x}"));
            }
            out.push('\'');
            out
        }
    }
}

async fn prototype_counts(conn: &mut Conn) -> Result<PrototypeCounts, PreviewError> {
    let parcel = scalar(conn, "SELECT COUNT(*) FROM erp.parcel").await?;
    let empty_box = scalar(conn, "SELECT COUNT(*) FROM erp.empty_box").await?;
    Ok(PrototypeCounts {
        parcel: parcel.parse().map_err(|_| PreviewError::new("value", "count"))?,
        empty_box: empty_box.parse().map_err(|_| PreviewError::new("value", "count"))?,
    })
}

async fn table_counts(conn: &mut Conn) -> Result<TableCounts, PreviewError> {
    Ok(TableCounts {
        hubs: scalar(conn, "SELECT COUNT(*) FROM logistics.hubs").await?.parse().map_err(|_| PreviewError::new("value", "count"))?,
        shipments: scalar(conn, "SELECT COUNT(*) FROM logistics.shipments").await?.parse().map_err(|_| PreviewError::new("value", "count"))?,
        events: scalar(conn, "SELECT COUNT(*) FROM logistics.tracking_events").await?.parse().map_err(|_| PreviewError::new("value", "count"))?,
        probe: scalar(conn, "SELECT COUNT(*) FROM logistics.value_probe").await?.parse().map_err(|_| PreviewError::new("value", "count"))?,
        gaps: scalar(conn, "SELECT COUNT(*) FROM logistics.range_gap").await?.parse().map_err(|_| PreviewError::new("value", "count"))?,
    })
}

async fn raw_probe(conn: &mut Conn) -> Result<RawProbe, PreviewError> {
    let zero = scalar(conn, "SELECT CAST(day_on AS CHAR) FROM logistics.value_probe WHERE probe_id = 5").await?;
    let time = scalar(conn, "SELECT CAST(clock_at AS CHAR) FROM logistics.value_probe WHERE probe_id = 6").await?;
    Ok(RawProbe { zero_date: zero, time_range: time })
}

async fn exec(conn: &mut Conn, sql: &str) -> Result<(), PreviewError> {
    timeout(Duration::from_secs(120), conn.query_drop(sql))
        .await
        .map_err(|_| PreviewError::new("budget", "statement_timeout"))?
        .map_err(|_| PreviewError::new("target", "dba"))?;
    Ok(())
}

async fn scalar(conn: &mut Conn, sql: &str) -> Result<String, PreviewError> {
    let row: Option<Row> = timeout(Duration::from_secs(60), conn.query_first(sql))
        .await
        .map_err(|_| PreviewError::new("budget", "statement_timeout"))?
        .map_err(|_| PreviewError::new("target", "dba"))?;
    let row = row.ok_or_else(|| PreviewError::new("target", "dba"))?;
    cell_string(&row, 0)
}

async fn strings(conn: &mut Conn, sql: &str) -> Result<Vec<String>, PreviewError> {
    let rows: Vec<Row> = timeout(Duration::from_secs(30), conn.query(sql))
        .await
        .map_err(|_| PreviewError::new("budget", "statement_timeout"))?
        .map_err(|_| PreviewError::new("target", "dba"))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(cell_string(&row, 0)?);
    }
    Ok(out)
}

fn cell_string(row: &Row, index: usize) -> Result<String, PreviewError> {
    let value: Value = row.get(index).ok_or_else(|| PreviewError::new("protocol", "cell"))?;
    match value {
        Value::Bytes(bytes) => String::from_utf8(bytes).map_err(|_| PreviewError::new("protocol", "cell")),
        Value::Int(value) => Ok(value.to_string()),
        Value::UInt(value) => Ok(value.to_string()),
        _ => Err(PreviewError::new("protocol", "cell")),
    }
}

const STRICT_MODE: &str = "SET SESSION sql_mode = 'STRICT_TRANS_TABLES,NO_ZERO_IN_DATE,NO_ZERO_DATE,ERROR_FOR_DIVISION_BY_ZERO,NO_ENGINE_SUBSTITUTION'";
const LOOSE_MODE: &str = "SET SESSION sql_mode = 'STRICT_TRANS_TABLES,ERROR_FOR_DIVISION_BY_ZERO,NO_ENGINE_SUBSTITUTION'";

const DROP_STATEMENTS: [&str; 5] = [
    "DROP TABLE IF EXISTS logistics.tracking_events",
    "DROP TABLE IF EXISTS logistics.shipments",
    "DROP TABLE IF EXISTS logistics.hubs",
    "DROP TABLE IF EXISTS logistics.value_probe",
    "DROP TABLE IF EXISTS logistics.range_gap",
];

const CREATE_STATEMENTS: [&str; 5] = [
    "CREATE TABLE logistics.hubs (
        hub_id BIGINT NOT NULL,
        hub_code TEXT NOT NULL,
        hub_name TEXT NOT NULL,
        region_code TEXT NULL,
        latitude DECIMAL(38,4) NULL,
        longitude DECIMAL(38,4) NULL,
        capacity_grams BIGINT NOT NULL,
        opened_at DATETIME NULL,
        badge VARBINARY(8) NULL,
        note TEXT NULL,
        PRIMARY KEY (hub_id)
    ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci",
    "CREATE TABLE logistics.shipments (
        shipment_id BIGINT NOT NULL,
        origin_hub_id BIGINT NOT NULL,
        dest_hub_id BIGINT NOT NULL,
        status TEXT NOT NULL,
        transport_mode TEXT NOT NULL,
        weight_grams BIGINT NULL,
        cost_amount DECIMAL(38,4) NULL,
        created_at DATETIME NULL,
        promised_at DATETIME NULL,
        seal VARBINARY(8) NULL,
        comment TEXT NULL,
        PRIMARY KEY (shipment_id),
        CONSTRAINT shipments_origin_fk FOREIGN KEY (origin_hub_id) REFERENCES logistics.hubs (hub_id),
        CONSTRAINT shipments_dest_fk FOREIGN KEY (dest_hub_id) REFERENCES logistics.hubs (hub_id),
        CONSTRAINT shipments_status_ck CHECK (status IN ('delivered', 'in_transit', 'delayed')),
        CONSTRAINT shipments_mode_ck CHECK (transport_mode IN ('road', 'rail', 'air', 'sea'))
    ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci",
    "CREATE TABLE logistics.tracking_events (
        event_id BIGINT NOT NULL,
        shipment_id BIGINT NOT NULL,
        hub_id BIGINT NULL,
        status TEXT NOT NULL,
        event_at DATETIME NULL,
        recorded_weight_grams BIGINT NULL,
        payload VARBINARY(8) NULL,
        comment TEXT NULL,
        PRIMARY KEY (event_id),
        CONSTRAINT events_shipment_fk FOREIGN KEY (shipment_id) REFERENCES logistics.shipments (shipment_id),
        CONSTRAINT events_hub_fk FOREIGN KEY (hub_id) REFERENCES logistics.hubs (hub_id),
        CONSTRAINT tracking_status_ck CHECK (status IN ('delivered', 'in_transit', 'delayed'))
    ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci",
    "CREATE TABLE logistics.value_probe (
        probe_id BIGINT NOT NULL,
        unsigned_grams BIGINT UNSIGNED NOT NULL,
        exact_amount DECIMAL(38,4) NULL,
        payload JSON NULL,
        badge VARBINARY(8) NULL,
        day_on DATE NULL,
        clock_at TIME NULL,
        note TEXT NULL,
        PRIMARY KEY (probe_id)
    ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci",
    "CREATE TABLE logistics.range_gap (
        gap_id BIGINT NOT NULL,
        table_name TEXT NOT NULL,
        column_name TEXT NOT NULL,
        row_key BIGINT NOT NULL,
        reason TEXT NOT NULL,
        canonical_text TEXT NOT NULL,
        PRIMARY KEY (gap_id)
    ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci",
];
