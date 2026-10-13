use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mysql_async::prelude::Queryable;
use mysql_async::{Conn, OptsBuilder, Row, Value};
use serde_json::{json, Value as Json};
use zeroize::Zeroize;

use crate::audit::{Audit, SqlMode};
use crate::civil::{naive_seconds, ymd_to_days};
use crate::decimal;
use crate::encode::{self, Cell, ColumnOut};
use crate::fail::Fail;
use crate::identity;
use crate::plan::{self, ArrowKind, ColumnContract, TableContract, TypePlan};
use crate::{DATABASE, EXPECTED_BUILD, PROTOCOL, READER_USER};

const BATCH_ROWS: usize = 3;
const PRESENCE_SQL: &str = "SELECT LOWER(table_name) FROM information_schema.tables WHERE LOWER(table_schema) = 'information_schema' AND LOWER(table_name) IN ('be_tablets', 'tables_config') ORDER BY 1";
const CONFIG_SQL: &str = "SELECT TABLE_NAME, TABLE_MODEL, IFNULL(PRIMARY_KEY, ''), IFNULL(PARTITION_KEY, ''), IFNULL(DISTRIBUTE_KEY, ''), IFNULL(SORT_KEY, '') FROM information_schema.tables_config WHERE TABLE_SCHEMA = 'oo_w45_sr'";
const COLUMNS_SQL: &str = "SELECT TABLE_NAME, COLUMN_NAME, DATA_TYPE, COLUMN_TYPE, IS_NULLABLE, ORDINAL_POSITION FROM information_schema.columns WHERE TABLE_SCHEMA = 'oo_w45_sr' ORDER BY TABLE_NAME, ORDINAL_POSITION";
const GRANTS_SQL: &str = "SHOW GRANTS FOR 'oo_w45_reader'@'%'";
// StarRocks 3.5.21 has no information_schema.processlist. SHOW FULL PROCESSLIST
// on this image is ServerName, Id, User, Host, Db, Command, ConnectionStartTime, Time, State, Info, ...
const PROCESS_SQL: &str = "SHOW FULL PROCESSLIST";
const PROCESS_ID: usize = 1;
const PROCESS_USER: usize = 2;
const PROCESS_TIME: usize = 7;
const PROCESS_INFO: usize = 9;
const CANCEL_MARKER: &str = "oo-dl01w1-45-cancel";
const DEADLINE_MARKER: &str = "oo-dl01w1-45-deadline";

#[derive(Clone, Copy, Debug)]
pub enum Command {
    Read,
    Block,
    CapsRow,
    CapsByte,
    Deadline,
    Cancel,
}

pub struct Request {
    pub command: Command,
    pub host: String,
    pub port: u16,
    pub password_file: PathBuf,
    pub out: PathBuf,
    pub image_digest: String,
    pub max_rows: u64,
    pub max_bytes: u64,
    pub deadline: Duration,
}

pub async fn execute(request: Request) -> Result<(), Fail> {
    fs::create_dir_all(&request.out).map_err(|err| Fail::new("out", err.to_string()))?;
    match request.command {
        Command::Read => read(&request).await,
        Command::Block => block(&request).await,
        Command::CapsRow | Command::CapsByte => caps(&request).await,
        Command::Deadline => deadline(&request).await,
        Command::Cancel => cancel(&request).await,
    }
}

async fn read(request: &Request) -> Result<(), Fail> {
    let audit = Audit::new();
    let outcome = read_inner(request, &audit).await;
    finish_io(&request.out, &audit, &outcome)
}

async fn read_inner(request: &Request, audit: &Audit) -> Result<(), Fail> {
    let mut conn = connect(request, audit, "business").await?;
    let loaded = match load_world(request, &mut conn, audit, SqlMode::ReadOnly).await {
        Ok(loaded) => loaded,
        Err(err) => {
            disconnect(conn, audit, "business").await;
            return Err(err);
        }
    };
    let mut encoded = Vec::new();
    let tables = plan::base_tables();
    let mut planned = Vec::new();
    planned.extend(tables.iter().cloned());
    planned.push(plan::partition_table());
    for table in &planned {
        if Instant::now() > loaded.deadline {
            disconnect(conn, audit, "business").await;
            discard_published(&request.out);
            return Err(Fail::new("deadline", table.name));
        }
        let rows = match fetch_table(&mut conn, audit, table, request.max_rows, request.max_bytes, loaded.deadline).await {
            Ok(rows) => rows,
            Err(err) => {
                disconnect(conn, audit, "business").await;
                discard_published(&request.out);
                return Err(err);
            }
        };
        let columns = match column_outs(table, &loaded) {
            Ok(columns) => columns,
            Err(err) => {
                disconnect(conn, audit, "business").await;
                discard_published(&request.out);
                return Err(err);
            }
        };
        let bytes = match encode::write_ipc(&columns, &rows, schema_meta(table, &loaded.version), BATCH_ROWS) {
            Ok(bytes) => bytes,
            Err(err) => {
                disconnect(conn, audit, "business").await;
                discard_published(&request.out);
                return Err(err);
            }
        };
        encoded.push((table.name, bytes, rows.len()));
    }
    disconnect(conn, audit, "business").await;
    for (name, bytes, _rows) in &encoded {
        publish_arrow(&request.out, name, bytes)?;
    }
    write_json(
        &request.out.join("catalog.json"),
        &catalog_json(&loaded, 1, 0, &request.image_digest),
    )?;
    write_json(
        &request.out.join("ready.json"),
        &json!({
            "protocol": PROTOCOL,
            "ready": true,
            "command": "read",
            "business_connections": 1,
            "control_connections": 0,
            "tables": encoded.iter().map(|(name, bytes, rows)| json!({"name": name, "rows": rows, "ipc_bytes": bytes.len()})).collect::<Vec<_>>(),
        }),
    )?;
    Ok(())
}

async fn block(request: &Request) -> Result<(), Fail> {
    let audit = Audit::new();
    let mut conn = connect(request, &audit, "business").await?;
    let loaded = match load_world(request, &mut conn, &audit, SqlMode::ReadOnly).await {
        Ok(loaded) => loaded,
        Err(err) => {
            disconnect(conn, &audit, "business").await;
            let _ = write_audit(&request.out, &audit);
            return Err(err);
        }
    };
    disconnect(conn, &audit, "business").await;
    let note = TypePlan::Arrow(ArrowKind::Utf8);
    let bitmap = TypePlan::Excluded { native: "bitmap".into() };
    let blocks = vec![
        named("cursor_note", plan::cursor_decision(plan::KeyModel::Duplicate, "note", &note)),
        named("cursor_bitmap", plan::cursor_decision(plan::KeyModel::Aggregate, "bits", &bitmap)),
        named("duplicate_enforced_key", plan::key_decision(plan::KeyModel::Duplicate, true)),
        named("float_cast", plan::cast_decision("CAST(balance AS DOUBLE)")),
    ];
    let all_blocked = blocks.iter().all(|item| item["blocked"] == json!(true));
    write_json(
        &request.out.join("catalog.json"),
        &catalog_json(&loaded, 1, 0, &request.image_digest),
    )?;
    write_json(
        &request.out.join("block.json"),
        &json!({
            "protocol": PROTOCOL,
            "ready": false,
            "blocked": all_blocked,
            "cases": blocks,
            "sketch_table": loaded.sketch_present,
        }),
    )?;
    write_audit(&request.out, &audit)?;
    if all_blocked {
        Ok(())
    } else {
        Err(Fail::new("block", "a key, cursor, or cast was accepted"))
    }
}

async fn caps(request: &Request) -> Result<(), Fail> {
    let audit = Audit::new();
    let mut conn = connect(request, &audit, "business").await?;
    let version = match identity_of(&mut conn, &audit).await {
        Ok(version) => version,
        Err(err) => {
            disconnect(conn, &audit, "business").await;
            let _ = write_audit(&request.out, &audit);
            return Err(err);
        }
    };
    if let Err(err) = exec_sql(&mut conn, &audit, "business", "USE oo_w45_sr", SqlMode::ReadOnly).await {
        disconnect(conn, &audit, "business").await;
        let _ = write_audit(&request.out, &audit);
        return Err(err);
    }
    let table = &plan::base_tables()[0];
    let deadline = Instant::now() + request.deadline;
    let fetched = fetch_table(&mut conn, &audit, table, request.max_rows, request.max_bytes, deadline).await;
    disconnect(conn, &audit, "business").await;
    discard_published(&request.out);
    let code = match fetched {
        Err(err) if err.code == "row_cap" || err.code == "byte_cap" => err.code,
        Err(err) => {
            let _ = write_audit(&request.out, &audit);
            return Err(err);
        }
        Ok(_) => {
            let _ = write_audit(&request.out, &audit);
            return Err(Fail::new("cap_missed", "the projected table fitted inside the cap"));
        }
    };
    write_json(
        &request.out.join("ready.json"),
        &json!({
            "protocol": PROTOCOL,
            "ready": false,
            "command": "caps",
            "code": code,
            "product_version": version,
            "half_ready": false,
        }),
    )?;
    write_audit(&request.out, &audit)?;
    Ok(())
}

async fn deadline(request: &Request) -> Result<(), Fail> {
    let audit = Audit::new();
    let started = Instant::now();
    let mut control = connect(request, &audit, "control").await?;
    if let Err(err) = prime_control(&mut control, &audit).await {
        disconnect(control, &audit, "control").await;
        let _ = write_audit(&request.out, &audit);
        return Err(err);
    }
    let mut business = connect(request, &audit, "business").await?;
    let business_id = business.id();
    if let Err(err) = prepare_business(&mut business, &audit, "SET query_timeout = 180").await {
        disconnect(business, &audit, "business").await;
        disconnect(control, &audit, "control").await;
        let _ = write_audit(&request.out, &audit);
        return Err(err);
    }
    let sleep_sql = format!("SELECT '{DEADLINE_MARKER}', sleep(40)");
    let audit_sleep = audit.clone();
    let (early_tx, mut early_rx) = tokio::sync::oneshot::channel();
    let sleep_task = tokio::spawn(async move {
        let end = sleep_query(business, audit_sleep, sleep_sql).await;
        let _ = early_tx.send(());
        end
    });
    let finished_early = tokio::time::timeout(Duration::from_secs(2), &mut early_rx).await.is_ok();
    if finished_early {
        let _ = sleep_task.await;
        disconnect(control, &audit, "control").await;
        let _ = write_audit(&request.out, &audit);
        return Err(Fail::new("deadline", "sleep finished before the client deadline"));
    }
    let cleanup = kill_and_join(&mut control, &audit, u64::from(business_id), sleep_task).await;
    let elapsed_ms = started.elapsed().as_millis();
    disconnect(control, &audit, "control").await;
    write_json(
        &request.out.join("deadline.json"),
        &json!({
            "protocol": PROTOCOL,
            "state": "deadline_cap",
            "ready": false,
            "client_timeout_ms": 2000,
            "timed_out_before_row": true,
            "elapsed_ms": elapsed_ms,
            "cleanup_kill": cleanup,
            "not_a_cancel_proof": true,
            "business_connection_id": business_id,
        }),
    )?;
    write_audit(&request.out, &audit)?;
    if cleanup.get("kill").and_then(Json::as_str) != Some("ok") {
        return Err(Fail::new("deadline_cleanup", cleanup.to_string()));
    }
    if elapsed_ms < 15_000 {
        Ok(())
    } else {
        Err(Fail::new("deadline", format!("elapsed_ms={elapsed_ms}")))
    }
}

async fn cancel(request: &Request) -> Result<(), Fail> {
    let audit = Audit::new();
    let primed = Instant::now();
    let mut control = connect(request, &audit, "control").await?;
    if let Err(err) = prime_control(&mut control, &audit).await {
        disconnect(control, &audit, "control").await;
        let _ = write_audit(&request.out, &audit);
        return Err(err);
    }
    let control_id = control.id();
    let prime_ms = primed.elapsed().as_millis();
    let mut business = connect(request, &audit, "business").await?;
    let business_id = business.id();
    if let Err(err) = prepare_business(&mut business, &audit, "SET query_timeout = 180").await {
        disconnect(business, &audit, "business").await;
        disconnect(control, &audit, "control").await;
        let _ = write_audit(&request.out, &audit);
        return Err(err);
    }
    let started = Instant::now();
    let sleep_sql = format!("SELECT '{CANCEL_MARKER}', sleep(90)");
    let audit_sleep = audit.clone();
    let sleep_task = tokio::spawn(async move { sleep_query(business, audit_sleep, sleep_sql).await });
    // A KILL in the same millisecond the statement starts is accepted and does not cancel it.
    // The approved setup lists the query only after it has already been running.
    let mut seen = None;
    let mut target_id = None;
    let mut last = String::new();
    for _ in 0..40 {
        match process_rows(&mut control, &audit).await {
            Ok(rows) => {
                last = format!("{rows:?}");
                if let Some(row) = rows
                    .iter()
                    .find(|row| row.is_reader() && row.info.contains(CANCEL_MARKER) && row.time_s >= 1)
                {
                    target_id = Some(row.id);
                    seen = Some(rows);
                    break;
                }
            }
            Err(err) => {
                last = err.fact.clone();
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let Some(rows) = seen else {
        let _ = kill_query(&mut control, &audit, u64::from(business_id)).await;
        disconnect(control, &audit, "control").await;
        let _ = sleep_task.await;
        write_audit(&request.out, &audit)?;
        return Err(Fail::new("cancel", format!("marker not visible: {last}")));
    };
    let processlist_id = target_id.ok_or_else(|| Fail::new("cancel", "processlist id missing"))?;
    let business_connections = rows.iter().filter(|row| row.is_reader() && row.info.contains(CANCEL_MARKER)).count();
    let control_connections = rows.iter().filter(|row| row.is_reader() && !row.info.contains(CANCEL_MARKER)).count();
    let dba_rows = rows.iter().filter(|row| row.user.contains("oo_w45_dba")).count();
    let by_user = user_counts(&rows);
    let kill = kill_query(&mut control, &audit, processlist_id).await;
    let sleep_end = match tokio::time::timeout(Duration::from_secs(20), sleep_task).await {
        Ok(Ok(end)) => end,
        Ok(Err(err)) => SleepEnd::Failed(err.to_string()),
        Err(_) => SleepEnd::Failed("business task timed out waiting after kill".to_string()),
    };
    let after = process_rows(&mut control, &audit).await.unwrap_or_default();
    let still = after.iter().any(|row| row.info.contains(CANCEL_MARKER));
    disconnect(control, &audit, "control").await;
    let elapsed_ms = started.elapsed().as_millis();
    let kill_text = match &kill {
        Ok(()) => "ok".to_string(),
        Err(err) => format!("{}: {}", err.code, err.fact),
    };
    let sleep_text = match &sleep_end {
        SleepEnd::Finished(text) => text.clone(),
        SleepEnd::Failed(text) => text.clone(),
    };
    let proved = kill.is_ok()
        && matches!(sleep_end, SleepEnd::Failed(ref text) if is_cancel_text(text))
        && !still
        && business_connections == 1
        && control_connections >= 1
        && dba_rows == 0
        && elapsed_ms < 30_000;
    let pending = kill.as_ref().err().is_some_and(|err| err.code == "gate_pending");
    let state = if proved {
        "cancel_proven"
    } else if pending {
        "gate_pending"
    } else {
        "cancel_failed"
    };
    write_json(
        &request.out.join("cancel.json"),
        &json!({
            "protocol": PROTOCOL,
            "state": state,
            "ready": false,
            "control_primed_before_business": true,
            "control_prime_ms": prime_ms,
            "control_connection_id": control_id,
            "business_connection_id": business_id,
            "processlist_id": processlist_id,
            "business_connections": business_connections,
            "control_connections": control_connections,
            "dba_rows": dba_rows,
            "by_user": by_user,
            "elapsed_ms": elapsed_ms,
            "marker_cleared": !still,
            "kill": kill_text,
            "business_result": sleep_text,
            "dba_kill_used": false,
            "old_dba_cancel_reused": false,
            "setup_cancel_3_128s_reused": false,
        }),
    )?;
    write_audit(&request.out, &audit)?;
    if proved {
        Ok(())
    } else if pending {
        Err(Fail::new("gate_pending", kill_text))
    } else {
        Err(Fail::new("cancel", format!("kill={kill_text} business={sleep_text} elapsed_ms={elapsed_ms}")))
    }
}

fn finish_io(out: &Path, audit: &Audit, outcome: &Result<(), Fail>) -> Result<(), Fail> {
    write_audit(out, audit)?;
    match outcome {
        Ok(()) => Ok(()),
        Err(err) => {
            discard_published(out);
            write_json(
                &out.join("failure.json"),
                &json!({"protocol": PROTOCOL, "ready": false, "code": err.code, "fact": err.fact}),
            )?;
            Err(err.clone())
        }
    }
}

struct Loaded {
    version: String,
    comment: String,
    protocol_version: String,
    grants: String,
    tables: Vec<ServerTable>,
    columns: Vec<ServerColumn>,
    excluded: Vec<Json>,
    sketch_present: bool,
    deadline: Instant,
}

struct ServerTable {
    name: String,
    model_raw: String,
    model: plan::KeyModel,
    primary_key: String,
    partition_key: String,
    distribute_key: String,
    sort_key: String,
}

struct ServerColumn {
    table: String,
    name: String,
    data_type: String,
    column_type: String,
    nullable: bool,
}

async fn load_world(request: &Request, conn: &mut Conn, audit: &Audit, mode: SqlMode) -> Result<Loaded, Fail> {
    let version = identity_of(conn, audit).await?;
    let comment = scalar_text(conn, audit, "business", "SELECT @@version_comment", mode).await?;
    let protocol_version = scalar_text(conn, audit, "business", "SELECT @@version", mode).await?;
    let presence = query_texts(conn, audit, "business", PRESENCE_SQL, mode).await?;
    let tables_present = presence.into_iter().filter_map(|row| row.into_iter().next()).collect::<Vec<_>>();
    let decision = identity::decide(&version, &comment, &protocol_version, &tables_present, Some(EXPECTED_BUILD));
    if !decision.ok {
        return Err(Fail::new("product_identity", decision.reason));
    }
    exec_sql(conn, audit, "business", "SET query_timeout = 120", mode).await?;
    exec_sql(conn, audit, "business", "USE oo_w45_sr", mode).await?;
    let grants = query_texts(conn, audit, "business", GRANTS_SQL, mode).await?;
    let grant_text = grants.iter().flatten().cloned().collect::<Vec<_>>().join("\n");
    select_only(&grant_text)?;
    let config_rows = query_texts(conn, audit, "business", CONFIG_SQL, mode).await?;
    let column_rows = query_texts(conn, audit, "business", COLUMNS_SQL, mode).await?;
    let mut tables = Vec::new();
    for row in config_rows {
        if row.len() < 6 {
            return Err(Fail::new("catalog", "tables_config width"));
        }
        let model = plan::parse_key_model(&row[1])?;
        tables.push(ServerTable {
            name: row[0].clone(),
            model_raw: row[1].clone(),
            model,
            primary_key: row[2].clone(),
            partition_key: row[3].clone(),
            distribute_key: row[4].clone(),
            sort_key: row[5].clone(),
        });
    }
    let mut columns = Vec::new();
    for row in column_rows {
        if row.len() < 6 {
            return Err(Fail::new("catalog", "columns width"));
        }
        columns.push(ServerColumn {
            table: row[0].clone(),
            name: row[1].clone(),
            data_type: row[2].clone(),
            column_type: row[3].clone(),
            nullable: row[4].eq_ignore_ascii_case("YES"),
        });
    }
    verify_contracts(&tables, &columns)?;
    let excluded = excluded_objects(&columns);
    let sketch_present = tables.iter().any(|table| table.name.eq_ignore_ascii_case("sketch_hold"));
    if sketch_present && !excluded.iter().any(|item| item["table"] == "sketch_hold") {
        return Err(Fail::new("excluded_type", "sketch_hold is readable"));
    }
    Ok(Loaded {
        version,
        comment,
        protocol_version,
        grants: grant_text,
        tables,
        columns,
        excluded,
        sketch_present,
        deadline: Instant::now() + request.deadline,
    })
}

fn verify_contracts(tables: &[ServerTable], columns: &[ServerColumn]) -> Result<(), Fail> {
    for contract in plan::base_tables() {
        let server = tables
            .iter()
            .find(|table| table.name.eq_ignore_ascii_case(contract.name))
            .ok_or_else(|| Fail::new("catalog", contract.name))?;
        if server.model != contract.model || plan::trust_for(server.model) != contract.trust {
            return Err(Fail::new(
                "key_model",
                format!("{} server {} expected {}", contract.name, server.model_raw, contract.model.as_str()),
            ));
        }
        if contract.name == "dup_events" && !server.partition_key.to_ascii_lowercase().contains("bucket") {
            return Err(Fail::new("partition", server.partition_key.clone()));
        }
        for column in contract.columns {
            let found = columns.iter().find(|item| {
                item.table.eq_ignore_ascii_case(contract.name) && item.name.eq_ignore_ascii_case(column.name)
            });
            let Some(found) = found else {
                return Err(Fail::new("catalog", format!("{}.{}", contract.name, column.name)));
            };
            if found.nullable != column.nullable {
                return Err(Fail::new(
                    "nullability",
                    format!("{}.{} server nullable {}", contract.name, column.name, found.nullable),
                ));
            }
            let classified = plan::classify_type(&found.data_type, &found.column_type);
            if !kind_matches(column.kind, &classified) {
                return Err(Fail::new(
                    "native_type",
                    format!("{}.{} {} {}", contract.name, column.name, found.data_type, found.column_type),
                ));
            }
        }
    }
    Ok(())
}

fn kind_matches(kind: ArrowKind, plan: &TypePlan) -> bool {
    match (kind, plan) {
        (ArrowKind::Int64, TypePlan::Arrow(ArrowKind::Int64)) => true,
        (
            ArrowKind::Decimal { precision, scale },
            TypePlan::Arrow(ArrowKind::Decimal { precision: got_p, scale: got_s }),
        ) => precision == *got_p && scale == *got_s,
        (ArrowKind::Date32, TypePlan::Arrow(ArrowKind::Date32)) => true,
        (ArrowKind::NaiveTimestampSecond, TypePlan::Arrow(ArrowKind::NaiveTimestampSecond)) => true,
        (ArrowKind::Utf8, TypePlan::Arrow(ArrowKind::Utf8)) => true,
        _ => false,
    }
}

fn excluded_objects(columns: &[ServerColumn]) -> Vec<Json> {
    let mut objects = Vec::new();
    for column in columns {
        let plan = plan::classify_type(&column.data_type, &column.column_type);
        if let TypePlan::Excluded { native } = plan {
            objects.push(json!({
                "table": column.table,
                "column": column.name,
                "native_type": native,
                "read": "excluded",
            }));
        }
    }
    objects
}

fn column_outs(table: &TableContract, loaded: &Loaded) -> Result<Vec<ColumnOut>, Fail> {
    table
        .columns
        .iter()
        .map(|column| {
            let native = if table.name == "dup_partition_sums" {
                native_for(column.kind)
            } else {
                loaded
                    .columns
                    .iter()
                    .find(|item| item.table.eq_ignore_ascii_case(table.name) && item.name.eq_ignore_ascii_case(column.name))
                    .map(|item| item.column_type.clone())
                    .ok_or_else(|| Fail::new("catalog", column.name))?
            };
            Ok(ColumnOut {
                name: column.name.to_string(),
                nullable: column.nullable,
                kind: column.kind,
                native_type: native,
            })
        })
        .collect()
}

fn native_for(kind: ArrowKind) -> String {
    match kind {
        ArrowKind::Int64 => "bigint".to_string(),
        ArrowKind::Decimal { precision, scale } => format!("decimal({precision},{scale})"),
        ArrowKind::Date32 => "date".to_string(),
        ArrowKind::NaiveTimestampSecond => "datetime".to_string(),
        ArrowKind::Utf8 => "varchar".to_string(),
    }
}

fn schema_meta(table: &TableContract, version: &str) -> std::collections::HashMap<String, String> {
    let mut meta = std::collections::HashMap::new();
    meta.insert("oo.source.protocol".to_string(), PROTOCOL.to_string());
    meta.insert("oo.starrocks.product_version".to_string(), version.to_string());
    meta.insert("oo.starrocks.table".to_string(), table.name.to_string());
    meta.insert("oo.starrocks.key_model".to_string(), table.model.as_str().to_string());
    meta.insert("oo.starrocks.constraint_trust".to_string(), table.trust.as_str().to_string());
    meta.insert("oo.starrocks.database".to_string(), DATABASE.to_string());
    meta
}

fn catalog_json(loaded: &Loaded, business: u64, control: u64, digest: &str) -> Json {
    let objects = plan::base_tables()
        .iter()
        .map(|contract| {
            let server = loaded.tables.iter().find(|table| table.name.eq_ignore_ascii_case(contract.name));
            json!({
                "name": contract.name,
                "key_model": contract.model.as_str(),
                "constraint_trust": contract.trust.as_str(),
                "server_model": server.map(|table| table.model_raw.as_str()).unwrap_or(""),
                "physical_hints": {
                    "primary_key": server.map(|table| table.primary_key.as_str()).unwrap_or(""),
                    "partition_key": server.map(|table| table.partition_key.as_str()).unwrap_or(""),
                    "distribute_key": server.map(|table| table.distribute_key.as_str()).unwrap_or(""),
                    "sort_key": server.map(|table| table.sort_key.as_str()).unwrap_or(""),
                },
                "columns": contract.columns.iter().map(|column| json!({
                    "name": column.name,
                    "nullable": column.nullable,
                    "arrow": arrow_name(column.kind),
                })).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "protocol": PROTOCOL,
        "product": "StarRocks",
        "product_version": loaded.version,
        "version_comment": loaded.comment,
        "wire_protocol_version": loaded.protocol_version,
        "wire_is_not_product": true,
        "rejected_equivalents": ["MySQL", "MariaDB", "Apache Doris"],
        "image_digest_asserted_by_caller": digest,
        "consistency_grade": "S0",
        "capture_method": "C4",
        "cdc": "absent",
        "final_semantics": "not_applicable",
        "semantic_caveats": [
            {"code": "empty_string_is_null", "applies": false, "evidence": "fixture keeps NULL and empty string distinct"},
            {"code": "eventual_consistency", "applies": true, "scope": "primary_and_unique_update_delete"},
            {"code": "append_only_source", "applies": true, "scope": "duplicate_key"},
            {"code": "precision_loss", "applies": false, "blocked_for": ["key", "cursor", "float_cast"]},
            {"code": "synthetic_binlog", "applies": false},
            {"code": "lossy_type_fallback", "applies": false},
            {"code": "offset_lost", "applies": false, "temporal": "naive_timestamp_second"}
        ],
        "delete_detection": {
            "dup_events": "partition_replace",
            "pk_accounts": "observed_visibility_not_cdc",
            "uniq_dim": "observed_visibility_not_cdc"
        },
        "excluded_aggregate_types": ["BITMAP", "HLL", "QUANTILE_STATE"],
        "excluded_columns": loaded.excluded,
        "native_columns": loaded.columns.iter().map(|column| json!({
            "table": column.table,
            "name": column.name,
            "data_type": column.data_type,
            "column_type": column.column_type,
            "nullable": column.nullable,
            "approved": plan::base_tables().iter().any(|table| table.name.eq_ignore_ascii_case(&column.table)
                && table.columns.iter().any(|field| field.name.eq_ignore_ascii_case(&column.name))),
        })).collect::<Vec<_>>(),
        "objects": objects,
        "reader_grants": loaded.grants,
        "reader_user": READER_USER,
        "business_connections": business,
        "control_connections": control,
    })
}

fn arrow_name(kind: ArrowKind) -> String {
    match kind {
        ArrowKind::Int64 => "int64".to_string(),
        ArrowKind::Decimal { precision, scale } => format!("decimal128({precision},{scale})"),
        ArrowKind::Date32 => "date32".to_string(),
        ArrowKind::NaiveTimestampSecond => "timestamp_second_naive".to_string(),
        ArrowKind::Utf8 => "utf8".to_string(),
    }
}

async fn fetch_table(
    conn: &mut Conn,
    audit: &Audit,
    table: &TableContract,
    max_rows: u64,
    max_bytes: u64,
    deadline: Instant,
) -> Result<Vec<Vec<Cell>>, Fail> {
    audit.sql("business", table.sql, SqlMode::ReadOnly)?;
    let mut result = conn.query_iter(table.sql).await.map_err(map_server)?;
    let mut rows = Vec::new();
    let mut bytes = 0u64;
    loop {
        if Instant::now() > deadline {
            return Err(Fail::new("deadline", table.name));
        }
        let next = result.next().await.map_err(map_server)?;
        let Some(row) = next else { break };
        let values = take_row(row)?;
        if values.len() != table.columns.len() {
            return Err(Fail::new("row_width", table.name));
        }
        let cells = values
            .iter()
            .zip(table.columns.iter())
            .map(|(value, column)| cell_from_value(value, column))
            .collect::<Result<Vec<_>, Fail>>()?;
        let width = encode::estimate_row_bytes(&cells);
        if rows.len() as u64 >= max_rows {
            return Err(Fail::new("row_cap", format!("{} rows>{max_rows}", table.name)));
        }
        if bytes + width > max_bytes {
            return Err(Fail::new("byte_cap", format!("{} bytes>{max_bytes}", table.name)));
        }
        bytes += width;
        rows.push(cells);
    }
    let _ = result.drop_result().await;
    if rows.is_empty() {
        return Err(Fail::new("empty_batch", table.name));
    }
    Ok(rows)
}

fn cell_from_value(value: &Value, column: &ColumnContract) -> Result<Cell, Fail> {
    if matches!(value, Value::Float(_) | Value::Double(_)) {
        return Err(Fail::new("unsafe_cast", column.name));
    }
    if matches!(value, Value::NULL) {
        if column.nullable {
            return Ok(Cell::Null);
        }
        return Err(Fail::new("nullability", column.name));
    }
    match column.kind {
        ArrowKind::Int64 => Ok(Cell::I64(value_i64(value)?)),
        ArrowKind::Decimal { scale, .. } => {
            let text = value_text(value)?.ok_or_else(|| Fail::new("decimal", column.name))?;
            Ok(Cell::Decimal(decimal::parse_unscaled(&text, scale)?))
        }
        ArrowKind::Date32 => Ok(Cell::Date(value_date(value)?)),
        ArrowKind::NaiveTimestampSecond => Ok(Cell::NaiveSecond(value_naive(value)?)),
        ArrowKind::Utf8 => Ok(Cell::Text(value_text(value)?.unwrap_or_default())),
    }
}

fn value_i64(value: &Value) -> Result<i64, Fail> {
    match value {
        Value::Int(number) => Ok(*number),
        Value::UInt(number) => i64::try_from(*number).map_err(|_| Fail::new("int_range", number.to_string())),
        Value::Bytes(bytes) => {
            let text = std::str::from_utf8(bytes).map_err(|_| Fail::new("utf8", "int"))?;
            text.trim().parse::<i64>().map_err(|_| Fail::new("int_range", text.to_string()))
        }
        _ => Err(Fail::new("value_shape", "int")),
    }
}

fn value_text(value: &Value) -> Result<Option<String>, Fail> {
    match value {
        Value::NULL => Ok(None),
        Value::Bytes(bytes) => {
            let text = std::str::from_utf8(bytes).map_err(|_| Fail::new("utf8", "text"))?;
            Ok(Some(text.to_string()))
        }
        Value::Int(number) => Ok(Some(number.to_string())),
        Value::UInt(number) => Ok(Some(number.to_string())),
        Value::Float(_) | Value::Double(_) => Err(Fail::new("unsafe_cast", "float")),
        Value::Date(year, month, day, hour, minute, second, micros) => {
            if *micros != 0 {
                return Err(Fail::new("precision_loss", "date micros"));
            }
            if *hour == 0 && *minute == 0 && *second == 0 {
                Ok(Some(format!("{year:04}-{month:02}-{day:02}")))
            } else {
                Ok(Some(format!(
                    "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}"
                )))
            }
        }
        Value::Time(..) => Err(Fail::new("unsupported_type", "time")),
    }
}

fn value_date(value: &Value) -> Result<i32, Fail> {
    let text = value_text(value)?.ok_or_else(|| Fail::new("date", "null"))?;
    let date = text.get(..10).ok_or_else(|| Fail::new("date", text.clone()))?;
    if text.len() > 10 && !text.ends_with("00:00:00") {
        return Err(Fail::new("date", text));
    }
    let mut parts = date.split('-');
    let year: i32 = parts.next().ok_or_else(|| Fail::new("date", text.clone()))?.parse().map_err(|_| Fail::new("date", text.clone()))?;
    let month: u32 = parts.next().ok_or_else(|| Fail::new("date", text.clone()))?.parse().map_err(|_| Fail::new("date", text.clone()))?;
    let day: u32 = parts.next().ok_or_else(|| Fail::new("date", text.clone()))?.parse().map_err(|_| Fail::new("date", text.clone()))?;
    ymd_to_days(year, month, day)
}

fn value_naive(value: &Value) -> Result<i64, Fail> {
    match value {
        Value::Date(year, month, day, hour, minute, second, micros) => {
            if *micros != 0 {
                return Err(Fail::new("precision_loss", "datetime micros"));
            }
            naive_seconds(i32::from(*year), u32::from(*month), u32::from(*day), u32::from(*hour), u32::from(*minute), u32::from(*second))
        }
        _ => {
            let text = value_text(value)?.ok_or_else(|| Fail::new("datetime", "null"))?;
            parse_naive_text(&text)
        }
    }
}

fn parse_naive_text(text: &str) -> Result<i64, Fail> {
    let normalized = text.trim().replace('T', " ");
    if normalized.contains('+') || normalized.ends_with('Z') {
        return Err(Fail::new("offset_lost", normalized));
    }
    let (date, time) = normalized
        .split_once(' ')
        .ok_or_else(|| Fail::new("datetime", normalized.clone()))?;
    let mut dates = date.split('-');
    let year: i32 = dates.next().ok_or_else(|| Fail::new("datetime", normalized.clone()))?.parse().map_err(|_| Fail::new("datetime", normalized.clone()))?;
    let month: u32 = dates.next().ok_or_else(|| Fail::new("datetime", normalized.clone()))?.parse().map_err(|_| Fail::new("datetime", normalized.clone()))?;
    let day: u32 = dates.next().ok_or_else(|| Fail::new("datetime", normalized.clone()))?.parse().map_err(|_| Fail::new("datetime", normalized.clone()))?;
    let mut times = time.split(':');
    let hour: u32 = times.next().ok_or_else(|| Fail::new("datetime", normalized.clone()))?.parse().map_err(|_| Fail::new("datetime", normalized.clone()))?;
    let minute: u32 = times.next().ok_or_else(|| Fail::new("datetime", normalized.clone()))?.parse().map_err(|_| Fail::new("datetime", normalized.clone()))?;
    let second_text = times.next().ok_or_else(|| Fail::new("datetime", normalized.clone()))?;
    if second_text.contains('.') {
        return Err(Fail::new("precision_loss", normalized));
    }
    let second: u32 = second_text.parse().map_err(|_| Fail::new("datetime", normalized.clone()))?;
    naive_seconds(year, month, day, hour, minute, second)
}

async fn identity_of(conn: &mut Conn, audit: &Audit) -> Result<String, Fail> {
    scalar_text(conn, audit, "business", "SELECT current_version()", SqlMode::ReadOnly).await
}

async fn prime_control(conn: &mut Conn, audit: &Audit) -> Result<(), Fail> {
    let session = scalar_u64(conn, audit, "control", "SELECT CONNECTION_ID()").await?;
    if session != u64::from(conn.id()) {
        return Err(Fail::new("connection_id", format!("handshake={} session={session}", conn.id())));
    }
    Ok(())
}

async fn prepare_business(conn: &mut Conn, audit: &Audit, timeout_sql: &str) -> Result<(), Fail> {
    let session = scalar_u64(conn, audit, "business", "SELECT CONNECTION_ID()").await?;
    if session != u64::from(conn.id()) {
        return Err(Fail::new("connection_id", format!("handshake={} session={session}", conn.id())));
    }
    exec_sql(conn, audit, "business", timeout_sql, SqlMode::ReadOnly).await?;
    exec_sql(conn, audit, "business", "USE oo_w45_sr", SqlMode::ReadOnly).await?;
    Ok(())
}

async fn sleep_query(mut conn: Conn, audit: Audit, sql: String) -> SleepEnd {
    let end = match audit.sql("business", &sql, SqlMode::ReadOnly) {
        Err(err) => SleepEnd::Failed(err.to_string()),
        Ok(_) => match conn.query_iter(sql).await {
            Err(err) => SleepEnd::Failed(err.to_string()),
            Ok(mut result) => match result.next().await {
                Ok(Some(_row)) => SleepEnd::Finished("row".to_string()),
                Ok(None) => SleepEnd::Failed("sleep returned no row".to_string()),
                Err(err) => SleepEnd::Failed(err.to_string()),
            },
        },
    };
    disconnect(conn, &audit, "business").await;
    end
}

enum SleepEnd {
    Finished(String),
    Failed(String),
}

#[derive(Debug)]
struct ProcessRow {
    id: u64,
    user: String,
    info: String,
    time_s: u64,
}

impl ProcessRow {
    fn is_reader(&self) -> bool {
        self.user == READER_USER || self.user.starts_with(&format!("{READER_USER}@"))
    }
}

async fn process_rows(conn: &mut Conn, audit: &Audit) -> Result<Vec<ProcessRow>, Fail> {
    let rows = query_texts(conn, audit, "control", PROCESS_SQL, SqlMode::Cancel).await?;
    let mut out = Vec::new();
    for row in rows {
        if row.len() <= PROCESS_INFO {
            return Err(Fail::new("processlist", format!("width {}", row.len())));
        }
        let id = row[PROCESS_ID]
            .parse::<u64>()
            .map_err(|_| Fail::new("processlist", format!("id {}", row[PROCESS_ID])))?;
        let time_s = row[PROCESS_TIME].parse::<u64>().unwrap_or(0);
        out.push(ProcessRow {
            id,
            user: row[PROCESS_USER].clone(),
            info: row[PROCESS_INFO].clone(),
            time_s,
        });
    }
    Ok(out)
}

async fn kill_query(conn: &mut Conn, audit: &Audit, id: u64) -> Result<(), Fail> {
    let sql = format!("KILL QUERY {id}");
    audit.sql("control", &sql, SqlMode::Cancel)?;
    conn.query_drop(sql).await.map_err(map_kill)
}

async fn kill_and_join(
    conn: &mut Conn,
    audit: &Audit,
    id: u64,
    sleep_task: tokio::task::JoinHandle<SleepEnd>,
) -> Json {
    let kill = kill_query(conn, audit, id).await;
    let joined = tokio::time::timeout(Duration::from_secs(20), sleep_task).await;
    json!({
        "kill": match &kill { Ok(()) => "ok".to_string(), Err(err) => err.to_string() },
        "business": match joined {
            Ok(Ok(SleepEnd::Failed(text))) => text,
            Ok(Ok(SleepEnd::Finished(text))) => text,
            Ok(Err(err)) => err.to_string(),
            Err(_) => "join timeout".to_string(),
        },
    })
}

fn is_cancel_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    (lower.contains("kill") || lower.contains("cancel")) && !lower.contains("timeout")
}

fn user_counts(rows: &[ProcessRow]) -> Json {
    let mut counts = serde_json::Map::new();
    for row in rows {
        let entry = counts.entry(row.user.clone()).or_insert(json!(0));
        if let Some(value) = entry.as_u64() {
            *entry = json!(value + 1);
        }
    }
    Json::Object(counts)
}

fn select_only(grants: &str) -> Result<(), Fail> {
    let upper = grants.to_ascii_uppercase();
    if !upper.contains("SELECT") {
        return Err(Fail::new("grants", "SELECT is not visible"));
    }
    for word in ["INSERT", "UPDATE", "DELETE", "CREATE", "DROP", "ALTER", "OPERATE", "ADMIN"] {
        if upper.split(|item: char| !item.is_ascii_alphanumeric()).any(|part| part == word) {
            return Err(Fail::new("grants", format!("reader grant contains {word}")));
        }
    }
    Ok(())
}

fn named(name: &str, result: Result<(), plan::Block>) -> Json {
    match result {
        Ok(()) => json!({"name": name, "blocked": false}),
        Err(block) => json!({"name": name, "blocked": true, "code": block.code, "fact": block.fact}),
    }
}

async fn scalar_text(conn: &mut Conn, audit: &Audit, role: &str, sql: &str, mode: SqlMode) -> Result<String, Fail> {
    let rows = query_texts(conn, audit, role, sql, mode).await?;
    rows.into_iter().next().and_then(|row| row.into_iter().next()).ok_or_else(|| Fail::new("scalar", sql))
}

async fn scalar_u64(conn: &mut Conn, audit: &Audit, role: &str, sql: &str) -> Result<u64, Fail> {
    let text = scalar_text(conn, audit, role, sql, SqlMode::ReadOnly).await?;
    text.trim().parse::<u64>().map_err(|_| Fail::new("scalar", text))
}

async fn query_texts(conn: &mut Conn, audit: &Audit, role: &str, sql: &str, mode: SqlMode) -> Result<Vec<Vec<String>>, Fail> {
    audit.sql(role, sql, mode)?;
    let mut result = conn.query_iter(sql).await.map_err(map_server)?;
    let mut rows = Vec::new();
    while let Some(row) = result.next().await.map_err(map_server)? {
        let values = take_row(row)?;
        let mut texts = Vec::new();
        for value in values {
            texts.push(value_text(&value)?.unwrap_or_default());
        }
        rows.push(texts);
    }
    let _ = result.drop_result().await;
    Ok(rows)
}

async fn exec_sql(conn: &mut Conn, audit: &Audit, role: &str, sql: &str, mode: SqlMode) -> Result<(), Fail> {
    audit.sql(role, sql, mode)?;
    conn.query_drop(sql).await.map_err(map_server)
}

async fn connect(request: &Request, audit: &Audit, role: &str) -> Result<Conn, Fail> {
    let mut password = read_password(&request.password_file)?;
    let opts = OptsBuilder::default()
        .ip_or_hostname(request.host.clone())
        .tcp_port(request.port)
        .user(Some(READER_USER.to_string()))
        .pass(Some(password.clone()))
        .db_name(Some(DATABASE.to_string()))
        .prefer_socket(false)
        .stmt_cache_size(0);
    let connected = tokio::time::timeout(Duration::from_secs(20), Conn::new(opts)).await;
    let result = match connected {
        Err(_) => Err(Fail::new("connect_timeout", request.host.clone())),
        Ok(Ok(conn)) => Ok(conn),
        Ok(Err(err)) => Err(Fail::new("connect", scrub(&err.to_string(), &password))),
    };
    password.zeroize();
    let conn = result?;
    audit.handshake(role, conn.id());
    Ok(conn)
}

async fn disconnect(conn: Conn, audit: &Audit, role: &str) {
    let id = conn.id();
    let _ = conn.disconnect().await;
    audit.cleanup(role, id);
}

fn read_password(path: &Path) -> Result<String, Fail> {
    use std::os::unix::fs::PermissionsExt;
    let meta = fs::metadata(path).map_err(|err| Fail::new("password_file", err.to_string()))?;
    if meta.permissions().mode() & 0o077 != 0 {
        return Err(Fail::new("secret_mode", format!("{:o}", meta.permissions().mode())));
    }
    if meta.len() > 256 {
        return Err(Fail::new("password_file", "too large"));
    }
    let mut text = fs::read_to_string(path).map_err(|err| Fail::new("password_file", err.to_string()))?;
    if text.ends_with('\n') {
        text.pop();
    }
    if text.is_empty() || text.chars().any(|item| item.is_whitespace() || item == '\'' || item == '\\') {
        text.zeroize();
        return Err(Fail::new("password_file", "rejected"));
    }
    Ok(text)
}

fn take_row(mut row: Row) -> Result<Vec<Value>, Fail> {
    let width = row.len();
    let mut values = Vec::with_capacity(width);
    for index in 0..width {
        values.push(row.take(index).ok_or_else(|| Fail::new("row", index.to_string()))?);
    }
    Ok(values)
}

fn map_server(err: mysql_async::Error) -> Fail {
    Fail::new("server", err.to_string())
}

fn map_kill(err: mysql_async::Error) -> Fail {
    let text = err.to_string();
    let lower = text.to_ascii_lowercase();
    if lower.contains("operate") || lower.contains("access denied") || lower.contains("privilege") {
        Fail::new("gate_pending", text)
    } else {
        Fail::new("cancel", text)
    }
}

fn scrub(text: &str, secret: &str) -> String {
    if secret.is_empty() {
        text.to_string()
    } else {
        text.replace(secret, "[redacted]")
    }
}

fn publish_arrow(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), Fail> {
    let partial = dir.join(format!("{name}.arrow.partial"));
    let final_path = dir.join(format!("{name}.arrow"));
    fs::write(&partial, bytes).map_err(|err| Fail::new("ipc", err.to_string()))?;
    fs::rename(&partial, &final_path).map_err(|err| Fail::new("ipc", err.to_string()))?;
    Ok(())
}

fn discard_published(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".arrow") || name.ends_with(".arrow.partial") || name == "ready.json" {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn write_audit(dir: &Path, audit: &Audit) -> Result<(), Fail> {
    fs::write(dir.join("audit.jsonl"), audit.to_jsonl()).map_err(|err| Fail::new("audit", err.to_string()))?;
    write_json(&dir.join("audit-counts.json"), &audit.counts())
}

fn write_json(path: &Path, value: &Json) -> Result<(), Fail> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|err| Fail::new("json", err.to_string()))?;
    bytes.push(b'\n');
    fs::write(path, bytes).map_err(|err| Fail::new("json", err.to_string()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn fixture_values_are_not_the_setup_gold() {
        let text = include_str!("../../fixtures/expected.json");
        assert!(!text.contains("12345678901234567890123456789.123456789"));
        assert!(!text.contains("99999999999999.9999"));
        assert!(text.contains("22345678901234567890123456789.123456789"));
    }
}
