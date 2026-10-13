//! Doris 3.x capture choice. Unpublished binlog is refused.
//! `part_day` is C4 partition refresh. Every other approved table is explicit C5.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value as Json};
use sha2::{Digest, Sha256};

use crate::audit::Audit;
use crate::cell::{self, Cell, Kind};
use crate::fail::Fail;
use crate::ipc::{self, Column as ArrowColumn};
use crate::session::{self, Endpoint, Link};
use crate::sqltext;

const DB: &str = "oo_dl01w1_43";

pub struct ObjectRoute {
    pub name: &'static str,
    pub key_model: &'static str,
    pub route: &'static str,
    pub delete_mode: &'static str,
    pub reason: &'static str,
}

const ROUTES: &[ObjectRoute] = &[
    ObjectRoute {
        name: "part_day",
        key_model: "DUPLICATE",
        route: "C4",
        delete_mode: "partition_replace",
        reason: "range partitions exist; duplicate key is not a unique row identity; unpublished binlog is not used",
    },
    ObjectRoute {
        name: "dup_events",
        key_model: "DUPLICATE",
        route: "C5",
        delete_mode: "reconcile",
        reason: "no partition metadata and no reliable cursor; duplicate key is not a unique row identity",
    },
    ObjectRoute {
        name: "agg_qty",
        key_model: "AGGREGATE",
        route: "C5",
        delete_mode: "reconcile",
        reason: "aggregate key merges values; no cursor column and no published change stream",
    },
    ObjectRoute {
        name: "uniq_acct",
        key_model: "UNIQUE",
        route: "C5",
        delete_mode: "reconcile",
        reason: "unique key can diff rows, but no commit-ordered cursor column is declared",
    },
    ObjectRoute {
        name: "dec_exact",
        key_model: "DUPLICATE",
        route: "C5",
        delete_mode: "reconcile",
        reason: "no partition metadata and no reliable cursor",
    },
    ObjectRoute {
        name: "str_key",
        key_model: "DUPLICATE",
        route: "C5",
        delete_mode: "reconcile",
        reason: "no partition metadata and no reliable cursor",
    },
    ObjectRoute {
        name: "nz_obs",
        key_model: "DUPLICATE",
        route: "C5",
        delete_mode: "reconcile",
        reason: "no partition metadata and no reliable cursor",
    },
];

struct TableRead {
    name: &'static str,
    columns_sql: &'static str,
    columns: &'static [(&'static str, Kind, bool)],
}

const DUP_COLUMNS: &[(&str, Kind, bool)] = &[
    ("event_id", Kind::Int64, false),
    ("kind", Kind::Utf8, false),
    ("qty", Kind::Decimal { precision: 10, scale: 2 }, true),
];
const AGG_COLUMNS: &[(&str, Kind, bool)] = &[
    ("sku", Kind::Utf8, false),
    ("qty", Kind::Decimal { precision: 10, scale: 2 }, true),
    ("note", Kind::Utf8, true),
];
const UNIQ_COLUMNS: &[(&str, Kind, bool)] = &[
    ("id", Kind::Int64, false),
    ("balance", Kind::Decimal { precision: 18, scale: 4 }, true),
    ("label", Kind::Utf8, true),
];
const DEC_COLUMNS: &[(&str, Kind, bool)] = &[
    ("id", Kind::Int64, false),
    ("a18", Kind::Decimal { precision: 18, scale: 4 }, true),
    ("b18", Kind::Decimal { precision: 18, scale: 4 }, true),
    ("d38", Kind::Decimal { precision: 38, scale: 18 }, true),
];
const PART_COLUMNS: &[(&str, Kind, bool)] = &[
    ("day", Kind::Date, false),
    ("region", Kind::Utf8, false),
    ("amount", Kind::Decimal { precision: 12, scale: 2 }, true),
];
const STR_COLUMNS: &[(&str, Kind, bool)] = &[
    ("k", Kind::Utf8, false),
    ("note", Kind::Utf8, true),
];
const NZ_COLUMNS: &[(&str, Kind, bool)] = &[
    ("id", Kind::Int64, false),
    ("qty", Kind::Decimal { precision: 10, scale: 2 }, true),
];

const TABLES: &[TableRead] = &[
    TableRead { name: "dup_events", columns_sql: "event_id, kind, qty", columns: DUP_COLUMNS },
    TableRead { name: "agg_qty", columns_sql: "sku, qty, note", columns: AGG_COLUMNS },
    TableRead { name: "uniq_acct", columns_sql: "id, balance, label", columns: UNIQ_COLUMNS },
    TableRead { name: "dec_exact", columns_sql: "id, a18, b18, d38", columns: DEC_COLUMNS },
    TableRead { name: "part_day", columns_sql: "day, region, amount", columns: PART_COLUMNS },
    TableRead { name: "str_key", columns_sql: "k, note", columns: STR_COLUMNS },
    TableRead { name: "nz_obs", columns_sql: "id, qty", columns: NZ_COLUMNS },
];

pub fn quality_for(route: &str) -> &'static str {
    match route {
        "C5" => "DELETES_LAGGED",
        "C4" => "partition_metadata_recorded",
        _ => "unknown",
    }
}

pub fn refuse_binlog(requested: bool) -> Result<(), Fail> {
    if requested {
        Err(Fail::new(
            "binlog",
            "Doris 2.x/3.x row binlog is unpublished and is not a capture source",
        ))
    } else {
        Ok(())
    }
}

pub fn route_named(name: &str) -> Option<&'static ObjectRoute> {
    ROUTES.iter().find(|item| item.name == name)
}

pub fn reader_statement_kind(sql: &str) -> Result<&'static str, Fail> {
    let folded = sql.split_whitespace().next().unwrap_or("").to_ascii_uppercase();
    match folded.as_str() {
        "SELECT" | "SHOW" => Ok("read"),
        "USE" => Ok("session"),
        "INSERT" | "UPDATE" | "DELETE" | "ALTER" | "DROP" | "CREATE" | "TRUNCATE" | "GRANT" | "KILL" | "SET" => {
            Err(Fail::new("reader_write", folded))
        }
        _ => Err(Fail::new("reader_sql", folded)),
    }
}

pub fn tick_allowed(previous_ms: Option<u64>, now_ms: u64, every_ms: u64) -> bool {
    match previous_ms {
        None => every_ms >= 1000,
        Some(previous) => now_ms.saturating_sub(previous) >= every_ms,
    }
}

pub fn canonical(rows: &[Vec<Option<String>>]) -> Vec<String> {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|cell| cell.clone().unwrap_or_else(|| "\\N".to_string()))
                .collect::<Vec<_>>()
                .join("\u{1f}")
        })
        .collect()
}

pub fn diff_lines(before: &[String], after: &[String]) -> (Vec<String>, Vec<String>) {
    let left = counts(before);
    let right = counts(after);
    let mut removed = Vec::new();
    let mut added = Vec::new();
    for (line, count) in &left {
        let have = right.get(line).copied().unwrap_or(0);
        for _ in 0..count.saturating_sub(have) {
            removed.push(line.clone());
        }
    }
    for (line, count) in &right {
        let have = left.get(line).copied().unwrap_or(0);
        for _ in 0..count.saturating_sub(have) {
            added.push(line.clone());
        }
    }
    removed.sort();
    added.sort();
    (removed, added)
}

fn counts(lines: &[String]) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for line in lines {
        *out.entry(line.clone()).or_insert(0) += 1;
    }
    out
}

pub fn count_reader_writes(audit_text: &str) -> usize {
    let mut writes = 0;
    for line in audit_text.lines() {
        let Ok(body) = serde_json::from_str::<Json>(line) else {
            continue;
        };
        if body.get("user").and_then(Json::as_str) != Some("oo_reader") {
            continue;
        }
        let Some(sql) = body.get("sql").and_then(Json::as_str) else {
            continue;
        };
        if reader_statement_kind(sql).is_err() {
            writes += 1;
        }
    }
    writes
}

pub fn cycle_statements(file_name: &str) -> Result<&'static [&'static str], Fail> {
    match file_name {
        "capture-cycle.sql" => Ok(CYCLE_1),
        "capture-cycle-2.sql" => Ok(CYCLE_2),
        _ => Err(Fail::new("dba_sql", "cycle file is not one of the two fixture scripts")),
    }
}

const CYCLE_1: &[&str] = &[
    "USE oo_dl01w1_43",
    "INSERT INTO part_day VALUES ('2026-03-20', 'east', CAST('1.50' AS DECIMAL(12,2)))",
    "UPDATE uniq_acct SET balance = CAST('3.5000' AS DECIMAL(18,4)), label = 'cycle' WHERE id = 2",
    "INSERT INTO uniq_acct VALUES (5, CAST('4.0000' AS DECIMAL(18,4)), 'new')",
    "DELETE FROM nz_obs WHERE id = 3",
];

const CYCLE_2: &[&str] = &[
    "USE oo_dl01w1_43",
    "UPDATE uniq_acct SET label = 'cycle2' WHERE id = 2",
];

pub async fn capture_tick(
    evidence: PathBuf,
    secrets: &str,
    endpoint: Endpoint,
    every_ms: u64,
    label: &str,
) -> Result<(), Fail> {
    capture_tick_impl(evidence, secrets, endpoint, every_ms, label, None).await
}

pub async fn capture_tick_budget(
    evidence: PathBuf,
    secrets: &str,
    endpoint: Endpoint,
    every_ms: u64,
    label: &str,
    max_rows: usize,
) -> Result<(), Fail> {
    if !(1..=1_000_000).contains(&max_rows) {
        return Err(Fail::new("budget", "max_rows outside 1..1000000"));
    }
    capture_tick_impl(evidence, secrets, endpoint, every_ms, label, Some(max_rows)).await
}

/// Read one observed Doris range partition. The metadata check brackets the
/// bounded read so a changed partition cannot be published as a stable view.
pub async fn capture_partition_budget(
    evidence: PathBuf,
    secrets: &str,
    endpoint: Endpoint,
    every_ms: u64,
    label: &str,
    partition: &str,
    max_rows: usize,
) -> Result<(), Fail> {
    validate_label(label)?;
    if !(1000..=86_400_000).contains(&every_ms) || !(1..=1_000_000).contains(&max_rows) {
        return Err(Fail::new("budget", "period or row cap"));
    }
    if !partition.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        || !(partition.starts_with('p') || partition.strip_prefix("cycle").is_some_and(|n| n.parse::<u64>().is_ok_and(|v| v > 0))) {
        return Err(Fail::new("partition", "unsafe partition name"));
    }
    let state_path = evidence.join(format!("schedule-{partition}-{every_ms}.json"));
    let previous = read_finished_ms(&state_path)?;
    if !tick_allowed(previous, unix_ms()?, every_ms) {
        return Err(Fail::new("schedule", "partition tick overlapped"));
    }
    let password = session::load_secret(&Path::new(secrets).join("reader.password"), false)?;
    let audit_path = evidence.join(format!("reader-audit-{label}.jsonl"));
    let audit = Audit::create(&audit_path, vec![password.to_string()])?;
    let mut link = session::connect_retry(
        &endpoint, "oo_reader", password.as_str(), &audit, 12,
        &evidence.join(format!("reader-connect-{label}.log")),
    ).await?;
    let result = read_partition(&mut link, &audit, partition, max_rows).await;
    link.close(&audit).await;
    let (mut body, arrow) = result?;
    let audit_text = std::fs::read_to_string(&audit_path).map_err(|err| Fail::new("audit", err.to_string()))?;
    if count_reader_writes(&audit_text) != 0 {
        return Err(Fail::new("reader_write", "partition reader audit"));
    }
    let arrow_name = format!("{label}-part_day-{partition}.arrow");
    std::fs::write(evidence.join(&arrow_name), arrow).map_err(|err| Fail::new("write", err.to_string()))?;
    let finished = unix_ms()?;
    body["tables"][0]["arrow"] = json!(arrow_name);
    body["every_ms"] = json!(every_ms);
    body["finished_unix_ms"] = json!(finished);
    body["label"] = json!(label);
    body["previous_finished_unix_ms"] = json!(previous);
    body["product_reader_write_attempts"] = json!(0);
    session::write_json(&evidence.join(format!("{label}.json")), &body)?;
    session::write_json(&state_path, &json!({"every_ms":every_ms,"finished_unix_ms":finished,"label":label}))?;
    Ok(())
}

async fn read_partition(
    link: &mut Link, audit: &Audit, partition: &str, max_rows: usize,
) -> Result<(Json, Vec<u8>), Fail> {
    let create_before = named_query(link, audit, "SHOW CREATE TABLE part_day").await?;
    let status_before = named_query(link, audit, "SHOW TABLE STATUS LIKE 'part_day'").await?;
    let identity_before = named_query(link, audit, "SELECT CATALOG_ID, DATABASE_ID, TABLE_ID FROM information_schema.metadata_name_ids WHERE DATABASE_NAME = 'oo_dl01w1_43' AND TABLE_NAME = 'part_day'").await?;
    let jobs_before = partition_jobs(link, audit).await?;
    let parts_before = named_query(link, audit, "SHOW PARTITIONS FROM part_day").await?;
    let selected = select_partition(&parts_before, partition)?;
    let part = stable_partition(&parts_before, &selected)?;
    let (lower, upper) = partition_range(part)?;
    let sql = format!(
        "SELECT day, region, amount FROM part_day PARTITION({selected}) ORDER BY day, region, amount LIMIT {}",
        max_rows + 1,
    );
    let (rows, cells) = checked_cells(link, audit, &sql, PART_COLUMNS, Some(max_rows)).await?;
    let (rows_again, _) = checked_cells(link, audit, &sql, PART_COLUMNS, Some(max_rows)).await?;
    if rows != rows_again {
        return Err(Fail::new("partition", "two partition reads disagree"));
    }
    if rows.iter().any(|row| row.first().and_then(Option::as_deref).is_none_or(|day| day < lower.as_str() || day >= upper.as_str())) {
        return Err(Fail::new("partition", "row outside observed range"));
    }
    let parts_after = named_query(link, audit, "SHOW PARTITIONS FROM part_day").await?;
    let jobs_after = partition_jobs(link, audit).await?;
    let identity_after = named_query(link, audit, "SELECT CATALOG_ID, DATABASE_ID, TABLE_ID FROM information_schema.metadata_name_ids WHERE DATABASE_NAME = 'oo_dl01w1_43' AND TABLE_NAME = 'part_day'").await?;
    let status_after = named_query(link, audit, "SHOW TABLE STATUS LIKE 'part_day'").await?;
    let create_after = named_query(link, audit, "SHOW CREATE TABLE part_day").await?;
    if parts_before != parts_after || status_before != status_after || create_before != create_after
        || identity_before != identity_after || jobs_before != jobs_after {
        return Err(Fail::new("partition", "source metadata changed during read"));
    }
    if part.get("RowCount").and_then(Json::as_str).and_then(|n| n.parse::<usize>().ok()) != Some(rows.len()) {
        return Err(Fail::new("partition", "partition row count not stable"));
    }
    let status = status_before.first().ok_or_else(|| Fail::new("partition", "table status missing"))?;
    let created = status.get("Create_time").and_then(Json::as_str)
        .filter(|s| !s.is_empty()).ok_or_else(|| Fail::new("partition", "table creation epoch missing"))?;
    if identity_before.len() != 1 || create_before.len() != 1 {
        return Err(Fail::new("partition", "table identity missing"));
    }
    let mut ids: Vec<String> = parts_before.iter().map(|p| {
        p.get("PartitionName").and_then(Json::as_str).unwrap_or("").to_string()
    }).collect();
    if ids.iter().any(String::is_empty) || ids.len() > 1024 {
        return Err(Fail::new("partition", "partition identity set"));
    }
    ids.sort();
    if ids.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(Fail::new("partition", "duplicate partition name"));
    }
    let native_id = part.get("PartitionId").and_then(Json::as_str)
        .ok_or_else(|| Fail::new("partition", "native partition id"))?;
    let signature = format!("{native_id}|{}", digest_text(&partition_signature(part)?));
    let mut epoch_partitions: Vec<(&str, &str, &str)> = parts_before.iter().map(|entry| {
        (
            entry.get("PartitionName").and_then(Json::as_str).unwrap_or(""),
            entry.get("PartitionId").and_then(Json::as_str).unwrap_or(""),
            entry.get("Range").and_then(Json::as_str).unwrap_or(""),
        )
    }).collect();
    epoch_partitions.sort();
    if epoch_partitions.iter().any(|(name, id, range)| name.is_empty() || id.is_empty() || range.is_empty()) {
        return Err(Fail::new("partition", "epoch partition set incomplete"));
    }
    let table_epoch = digest_text(&format!("{}|{created}|{}|{:?}", identity_before[0], create_before[0], epoch_partitions));
    let schema_fingerprint = digest_text(&create_before[0].to_string());
    let native_version = part.get("VisibleVersion").and_then(Json::as_str)
        .ok_or_else(|| Fail::new("partition", "native partition version"))?;
    let spec = TABLES.iter().find(|t| t.name == "part_day").ok_or_else(|| Fail::new("partition", "table"))?;
    let arrow = ipc::write_file(&arrow_columns(spec), &cells, arrow_metadata(spec.name, "DUPLICATE", &sql))?;
    Ok((json!({
        "protocol":"oo.doris.capture-tick/v1",
        "database":DB,
        "source_engine":"doris",
        "cross_table_consistency":"B",
        "snapshot_token":false,
        "table_status":status_before,
        "source_table_identity":identity_before,
        "mutation_jobs":jobs_before,
        "create_table":create_before,
        "partitions":parts_before,
        "partition_scope":{
            "partition_id":selected,
            "observed_partition_ids":ids,
            "complete":true,
            "table_epoch":table_epoch,
            "schema_fingerprint":schema_fingerprint,
            "source_engine":"doris",
            "key_model":"duplicate",
            "routing_column":"day",
            "partition_range":{"lower":lower,"upper":upper},
            "native_partition_id":native_id,
            "native_partition_version":native_version,
            "row_count":rows.len(),
            "current_signature":signature,
            "mutations_pending":false,
        },
        "tables":[{"name":"part_day","route":"C4","delete_mode":"partition_replace",
                    "consistency":"S0","key_model":"DUPLICATE","cursor_safety":"no_cursor",
                    "rows":canonical(&rows)}],
        "budget":{"unit":"Rows","max_rows_per_table":max_rows,"enforced":true,"overflow_probe_rows":1},
    }), arrow))
}

async fn partition_jobs(link: &mut Link, audit: &Audit) -> Result<Json, Fail> {
    let statements = [
        "SHOW DELETE FROM oo_dl01w1_43",
        "SHOW ALTER TABLE COLUMN FROM oo_dl01w1_43",
        "SHOW ALTER TABLE ROLLUP FROM oo_dl01w1_43",
        "SHOW LOAD FROM oo_dl01w1_43",
    ];
    let mut observed = serde_json::Map::new();
    for sql in statements {
        let rows = named_query(link, audit, sql).await?;
        for row in &rows {
            let table = row.get("TableName").and_then(Json::as_str);
            if table.is_some_and(|name| name != "part_day") { continue; }
            let state = row.get("State").and_then(Json::as_str);
            if !sql.starts_with("SHOW DELETE") && state.is_none() {
                return Err(Fail::new("partition", "source mutation job state missing"));
            }
            if state.is_some_and(|state| state != "FINISHED" && state != "CANCELLED") {
                return Err(Fail::new("partition", "source mutation job pending"));
            }
        }
        observed.insert(sql.to_string(), Json::Array(rows));
    }
    Ok(Json::Object(observed))
}

fn digest_text(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn stable_partition<'a>(parts: &'a [Json], name: &str) -> Result<&'a Json, Fail> {
    let part = parts.iter().find(|p| p.get("PartitionName").and_then(Json::as_str) == Some(name))
        .ok_or_else(|| Fail::new("partition", "partition missing"))?;
    if part.get("State").and_then(Json::as_str) != Some("NORMAL")
        || part.get("CommittedVersion") != part.get("VisibleVersion")
        || part.get("PartitionId").and_then(Json::as_str).is_none()
    {
        return Err(Fail::new("partition", "partition not stable"));
    }
    Ok(part)
}

fn select_partition(parts: &[Json], selector: &str) -> Result<String, Fail> {
    if let Some(value) = selector.strip_prefix("cycle") {
        let pin: usize = value.parse().map_err(|_| Fail::new("partition", "cycle pin"))?;
        let mut names: Vec<&str> = parts.iter()
            .filter_map(|part| part.get("PartitionName").and_then(Json::as_str)).collect();
        names.sort();
        if pin == 0 || names.is_empty() || names.len() != parts.len() || names.windows(2).any(|p| p[0] == p[1]) {
            return Err(Fail::new("partition", "cycle partition set"));
        }
        return Ok(names[(pin - 1) % names.len()].to_string());
    }
    Ok(selector.to_string())
}

fn partition_range(part: &Json) -> Result<(String, String), Fail> {
    let range = part.get("Range").and_then(Json::as_str).ok_or_else(|| Fail::new("partition", "range"))?;
    let dates: Vec<&str> = range.split("keys: [").skip(1).filter_map(|s| s.split(']').next()).collect();
    if dates.len() != 2 || dates.iter().any(|s| s.len() != 10 || !s.bytes().all(|b| b.is_ascii_digit() || b == b'-'))
        || dates[0] >= dates[1] {
        return Err(Fail::new("partition", "unsupported DATEV2 range"));
    }
    Ok((dates[0].to_string(), dates[1].to_string()))
}

fn partition_signature(part: &Json) -> Result<String, Fail> {
    let fields = ["PartitionName", "PartitionId", "Range", "CommittedVersion", "VisibleVersion", "VisibleVersionTime", "RowCount"];
    let values: Option<Vec<&str>> = fields.iter().map(|key| part.get(*key).and_then(Json::as_str)).collect();
    let values = values.ok_or_else(|| Fail::new("partition", "signature fields"))?;
    if values.iter().any(|value| value.is_empty()) { return Err(Fail::new("partition", "signature empty")); }
    Ok(values.join("|"))
}

async fn capture_tick_impl(
    evidence: PathBuf,
    secrets: &str,
    endpoint: Endpoint,
    every_ms: u64,
    label: &str,
    max_rows: Option<usize>,
) -> Result<(), Fail> {
    refuse_binlog(false)?;
    validate_label(label)?;
    if !(1000..=86_400_000).contains(&every_ms) {
        return Err(Fail::new("schedule", "every_ms out of range"));
    }
    let now = unix_ms()?;
    let state_path = evidence.join(format!("schedule-{every_ms}.json"));
    let previous = read_finished_ms(&state_path)?;
    if !tick_allowed(previous, now, every_ms) {
        return Err(Fail::new(
            "schedule",
            format!("tick {label} overlapped every {every_ms}ms"),
        ));
    }
    let password = session::load_secret(&Path::new(secrets).join("reader.password"), false)?;
    let audit = Audit::create(
        &evidence.join(format!("reader-audit-{label}.jsonl")),
        vec![password.to_string()],
    )?;
    let mut link = session::connect_retry(
        &endpoint,
        "oo_reader",
        password.as_str(),
        &audit,
        12,
        &evidence.join(format!("reader-connect-{label}.log")),
    )
    .await?;
    let snapshot = match read_snapshot(&mut link, &audit, max_rows).await {
        Ok(body) => body,
        Err(err) => {
            link.close(&audit).await;
            if err.code == "budget" {
                session::write_json(&evidence.join(format!("budget-rejection-{label}.json")),
                    &json!({"unit":"Rows", "max_rows_per_table":max_rows,
                            "reason":"max_rows", "snapshot_emitted":false,
                            "arrow_emitted":false, "complete":false}))?;
            }
            return Err(err);
        }
    };
    link.close(&audit).await;
    let finished = unix_ms()?;
    let audit_text = std::fs::read_to_string(evidence.join(format!("reader-audit-{label}.jsonl")))
        .map_err(|err| Fail::new("audit", err.to_string()))?;
    let writes = count_reader_writes(&audit_text);
    if writes != 0 {
        return Err(Fail::new("reader_write", format!("{writes}")));
    }
    for (name, bytes) in &snapshot.arrows {
        let path = evidence.join(format!("{label}-{name}.arrow"));
        std::fs::write(&path, bytes).map_err(|err| Fail::new("write", err.to_string()))?;
    }
    let mut body = snapshot.body;
    if let Some(tables) = body.get_mut("tables").and_then(Json::as_array_mut) {
        for table in tables {
            let Some(name) = table.get("name").and_then(Json::as_str).map(str::to_string) else {
                continue;
            };
            table["arrow"] = json!(format!("{label}-{name}.arrow"));
        }
    }
    body["every_ms"] = json!(every_ms);
    body["finished_unix_ms"] = json!(finished);
    body["label"] = json!(label);
    body["previous_finished_unix_ms"] = json!(previous);
    body["product_reader_write_attempts"] = json!(0);
    session::write_json(&evidence.join(format!("{label}.json")), &body)?;
    session::write_json(
        &state_path,
        &json!({
            "every_ms": every_ms,
            "finished_unix_ms": finished,
            "label": label,
        }),
    )?;
    Ok(())
}

pub async fn capture_dba(evidence: PathBuf, secrets: &str, endpoint: Endpoint, sql_file: &Path) -> Result<(), Fail> {
    refuse_binlog(false)?;
    let file_name = sql_file
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| Fail::new("dba_sql", "file name"))?;
    let expected = cycle_statements(file_name)?;
    let sql = std::fs::read_to_string(sql_file).map_err(|err| Fail::new("dba_sql", err.to_string()))?;
    let statements = sqltext::split(&sql);
    let got: Vec<&str> = statements.iter().map(String::as_str).collect();
    if got != expected {
        return Err(Fail::new("dba_sql", "script does not match the fixture allowlist"));
    }
    let password = session::load_secret(&Path::new(secrets).join("dba.password"), false)?;
    let audit = Audit::create(&evidence.join(format!("dba-audit-{file_name}.jsonl")), vec![password.to_string()])?;
    let mut link = session::connect_retry(
        &endpoint,
        "oo_dba",
        password.as_str(),
        &audit,
        12,
        &evidence.join(format!("dba-connect-{file_name}.log")),
    )
    .await?;
    for statement in &statements {
        if let Err(err) = link.exec(statement, &audit).await {
            link.close(&audit).await;
            return Err(err);
        }
    }
    link.close(&audit).await;
    session::write_json(
        &evidence.join(format!("dba-{file_name}.json")),
        &json!({
            "account": "oo_dba",
            "file": file_name,
            "statements": statements,
            "product_reader_write_attempts": 0,
        }),
    )?;
    Ok(())
}

struct Snapshot {
    body: Json,
    arrows: Vec<(&'static str, Vec<u8>)>,
}

async fn read_snapshot(link: &mut Link, audit: &Audit, max_rows: Option<usize>) -> Result<Snapshot, Fail> {
    let select_version = one_cell(link, audit, "SELECT VERSION()").await?;
    let backends = checked_query(link, audit, "SHOW BACKENDS").await?;
    let backend_version = backends
        .iter()
        .flatten()
        .flatten()
        .find(|cell| cell.contains("doris-3.") || cell.contains("3.1."))
        .cloned()
        .ok_or_else(|| Fail::new("version", "SHOW BACKENDS has no Doris 3.x version"))?;
    let mut tables = Vec::new();
    let mut arrows = Vec::new();
    for spec in TABLES {
        let route = route_named(spec.name).ok_or_else(|| Fail::new("route", spec.name))?;
        let mut sql = format!("SELECT {} FROM {} ORDER BY {}", spec.columns_sql, spec.name, spec.columns_sql);
        if let Some(limit) = max_rows {
            // One bounded sentinel detects exhaustion, instead of silently
            // treating a truncated table as a complete deletion snapshot.
            sql.push_str(&format!(" LIMIT {}", limit + 1));
        }
        let (rows, cells) = checked_cells(link, audit, &sql, spec.columns, max_rows).await?;
        let bytes = ipc::write_file(&arrow_columns(spec), &cells, arrow_metadata(spec.name, route.key_model, &sql))?;
        arrows.push((spec.name, bytes));
        tables.push(json!({
            "consistency": "S0",
            "cursor_safety": "no_cursor",
            "delete_mode": route.delete_mode,
            "key_model": route.key_model,
            "name": spec.name,
            "out_of_partition_deletes": if route.route == "C4" {
                "undetected_when_partition_metadata_is_unchanged"
            } else {
                "not_partitioned"
            },
            "quality": quality_for(route.route),
            "reason": route.reason,
            "route": route.route,
            "rows": canonical(&rows),
        }));
    }
    let partitions = named_query(link, audit, "SHOW PARTITIONS FROM part_day").await?;
    Ok(Snapshot {
        arrows,
        body: json!({
            "backend_version": backend_version,
            "binlog": "refused_unpublished",
            "change_stream": false,
            "cross_table_consistency": "B",
            "database": DB,
            "equal_cursor": "not_applicable_no_cursor",
            "long_transaction": "not_observable_no_snapshot_token",
            "partitions": partitions,
            "position_expiry": "not_applicable_no_change_position",
            "protocol": "oo.doris.capture-tick/v1",
            "budget": {"unit":"Rows", "max_rows_per_table":max_rows,
                       "enforced":max_rows.is_some(), "overflow_probe_rows":1},
            "select_version": select_version,
            "select_version_is_product_identity": false,
            "snapshot_token": false,
            "source_acceptance": false,
            "tables": tables,
        }),
    })
}

fn arrow_columns(spec: &TableRead) -> Vec<ArrowColumn> {
    spec.columns
        .iter()
        .map(|(name, kind, nullable)| ArrowColumn {
            name,
            kind: *kind,
            nullable: *nullable,
        })
        .collect()
}

fn arrow_metadata(name: &str, model: &str, statement: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert("oo.doris.capture".to_string(), "oo.doris.capture-tick/v1".to_string());
    map.insert("oo.doris.driver".to_string(), "mysql_async 0.36.1".to_string());
    map.insert("oo.doris.lossless".to_string(), "true".to_string());
    map.insert("oo.doris.model".to_string(), model.to_string());
    map.insert("oo.doris.statement".to_string(), statement.to_string());
    map.insert("oo.doris.table".to_string(), name.to_string());
    map
}

async fn one_cell(link: &mut Link, audit: &Audit, sql: &str) -> Result<String, Fail> {
    let rows = checked_query(link, audit, sql).await?;
    rows.first()
        .and_then(|row| row.first())
        .and_then(|cell| cell.clone())
        .ok_or_else(|| Fail::new("version", "empty"))
}

async fn checked_query(link: &mut Link, audit: &Audit, sql: &str) -> Result<Vec<Vec<Option<String>>>, Fail> {
    reader_statement_kind(sql)?;
    let rows = link.query(sql, audit).await?;
    session::rows_texts(&rows)
}

async fn checked_cells(
    link: &mut Link,
    audit: &Audit,
    sql: &str,
    columns: &[(&str, Kind, bool)],
    max_rows: Option<usize>,
) -> Result<(Vec<Vec<Option<String>>>, Vec<Vec<Cell>>), Fail> {
    reader_statement_kind(sql)?;
    let rows = if let Some(limit) = max_rows {
        let (rows, reason, _) = link.query_bound(sql, audit, Some(limit), None).await?;
        if reason.is_some() {
            return Err(Fail::new("budget", "max_rows exhausted before snapshot emission"));
        }
        rows
    } else {
        link.query(sql, audit).await?
    };
    let texts = session::rows_texts(&rows)?;
    let mut cells = Vec::with_capacity(rows.len());
    for row in &rows {
        if row.len() != columns.len() {
            return Err(Fail::new("row_width", sql));
        }
        let mut line = Vec::with_capacity(columns.len());
        for (index, column) in columns.iter().enumerate() {
            let value = row.as_ref(index).ok_or_else(|| Fail::new("cell", column.0))?;
            line.push(cell::from_value(value, column.1)?);
        }
        cells.push(line);
    }
    Ok((texts, cells))
}

async fn named_query(link: &mut Link, audit: &Audit, sql: &str) -> Result<Vec<Json>, Fail> {
    reader_statement_kind(sql)?;
    let rows = link.query(sql, audit).await?;
    let mut out = Vec::new();
    for row in &rows {
        let texts = session::row_texts(row)?;
        let mut object = serde_json::Map::new();
        for (column, cell) in row.columns_ref().iter().zip(texts) {
            object.insert(
                column.name_str().to_string(),
                match cell {
                    Some(text) => Json::String(text),
                    None => Json::Null,
                },
            );
        }
        out.push(Json::Object(object));
    }
    Ok(out)
}

fn read_finished_ms(path: &Path) -> Result<Option<u64>, Fail> {
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path).map_err(|err| Fail::new("schedule", err.to_string()))?;
    let body: Json = serde_json::from_str(&text).map_err(|err| Fail::new("schedule", err.to_string()))?;
    let finished = body
        .get("finished_unix_ms")
        .and_then(Json::as_u64)
        .ok_or_else(|| Fail::new("schedule", "finished_unix_ms"))?;
    Ok(Some(finished))
}

fn validate_label(label: &str) -> Result<(), Fail> {
    let ok = (1..32).contains(&label.len())
        && label
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-');
    if ok {
        Ok(())
    } else {
        Err(Fail::new("label", label))
    }
}

fn unix_ms() -> Result<u64, Fail> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Fail::new("clock", "before epoch"))?;
    u64::try_from(duration.as_millis()).map_err(|_| Fail::new("clock", "millis"))
}

#[cfg(test)]
mod tests {
    use super::{
        canonical, count_reader_writes, cycle_statements, diff_lines, partition_range, quality_for, refuse_binlog, route_named,
        select_partition,
        reader_statement_kind, tick_allowed, CYCLE_1, CYCLE_2,
    };
    use crate::sqltext::split;
    use serde_json::json;

    #[test]
    fn partition_range_and_duplicate_bag() {
        let part = json!({"Range":"[types: [DATEV2]; keys: [2026-03-01]; ..types: [DATEV2]; keys: [2026-04-01]; )"});
        assert_eq!(partition_range(&part).unwrap(), ("2026-03-01".into(), "2026-04-01".into()));
        assert!(partition_range(&json!({"Range":"unbounded"})).is_err());
        let rows = vec![vec![Some("2026-03-20".into()), Some("east".into())]; 2];
        let lines = canonical(&rows);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], lines[1]);
        let parts = vec![json!({"PartitionName":"p202604"}), json!({"PartitionName":"p202603"})];
        assert_eq!(select_partition(&parts, "cycle1").unwrap(), "p202603");
        assert_eq!(select_partition(&parts, "cycle2").unwrap(), "p202604");
        assert_eq!(select_partition(&parts, "cycle3").unwrap(), "p202603");
    }

    #[test]
    fn part_day_is_c4_and_other_tables_are_explicit_c5() {
        let part = route_named("part_day").unwrap();
        assert_eq!(part.route, "C4");
        assert_eq!(part.delete_mode, "partition_replace");
        for name in ["dup_events", "agg_qty", "uniq_acct", "dec_exact", "str_key", "nz_obs"] {
            let route = route_named(name).unwrap();
            assert_eq!(route.route, "C5");
            assert_ne!(route.reason, "");
        }
        assert!(route_named("uniq_acct").unwrap().reason.contains("no commit-ordered cursor"));
        assert_eq!(quality_for("C5"), "DELETES_LAGGED");
        assert_eq!(quality_for("C4"), "partition_metadata_recorded");
        assert_eq!(quality_for("C1"), "unknown");
    }

    #[test]
    fn unpublished_binlog_is_refused_before_a_read() {
        assert!(refuse_binlog(true).is_err());
        assert!(refuse_binlog(false).is_ok());
    }

    #[test]
    fn reader_rejects_dml_and_the_two_cycles_match_their_files() {
        assert!(reader_statement_kind("DELETE FROM nz_obs WHERE id = 3").is_err());
        assert!(reader_statement_kind("SELECT id FROM nz_obs").is_ok());
        let first = split(include_str!("../../sql/capture-cycle.sql"));
        let second = split(include_str!("../../sql/capture-cycle-2.sql"));
        assert_eq!(first.iter().map(String::as_str).collect::<Vec<_>>(), CYCLE_1);
        assert_eq!(second.iter().map(String::as_str).collect::<Vec<_>>(), CYCLE_2);
        assert_eq!(cycle_statements("capture-cycle.sql").unwrap(), CYCLE_1);
        assert!(cycle_statements("mutate.sql").is_err());
    }

    #[test]
    fn ticks_do_not_overlap_and_diffs_keep_duplicate_rows() {
        assert!(tick_allowed(None, 1_000, 3_000));
        assert!(!tick_allowed(Some(5_000), 7_000, 3_000));
        assert!(tick_allowed(Some(5_000), 8_000, 3_000));
        let before = canonical(&[
            vec![Some("3".into()), Some("1.00".into())],
            vec![Some("3".into()), Some("1.00".into())],
        ]);
        let after = canonical(&[
            vec![Some("3".into()), Some("1.00".into())],
            vec![Some("9".into()), None],
        ]);
        let (removed, added) = diff_lines(&before, &after);
        assert_eq!(removed, vec!["3\u{1f}1.00".to_string()]);
        assert_eq!(added, vec!["9\u{1f}\\N".to_string()]);
    }

    #[test]
    fn reader_audit_counts_a_failed_write() {
        let text = "{\"user\":\"oo_reader\",\"sql\":\"DELETE FROM nz_obs WHERE id = 3\",\"ok\":false}\n\
                    {\"user\":\"oo_dba\",\"sql\":\"DELETE FROM nz_obs WHERE id = 3\",\"ok\":true}\n\
                    {\"user\":\"oo_reader\",\"sql\":\"SELECT id FROM nz_obs\",\"ok\":true}\n";
        assert_eq!(count_reader_writes(text), 1);
    }
}
