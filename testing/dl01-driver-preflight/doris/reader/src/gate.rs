use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use mysql_async::Row;
use serde_json::{json, Value as Json};
use tokio::time::{sleep, timeout};

use crate::cell::{self, Cell, Kind};
use crate::fail::Fail;
use crate::guard::{business_statement, CastRequest};
use crate::ipc::{self, Column as ArrowColumn};
use crate::session::{self, Endpoint, Link};
use crate::audit::Audit;

const DRIVER: &str = "mysql_async 0.36.1";
const DUP_SQL: &str = "SELECT event_id, kind, qty FROM dup_events ORDER BY event_id, kind";
const AGG_SQL: &str = "SELECT sku, qty, note FROM agg_qty ORDER BY sku";
const UNIQ_SQL: &str = "SELECT id, balance, label FROM uniq_acct ORDER BY id";
const DEC_SQL: &str = "SELECT id, a18, b18, d38 FROM dec_exact ORDER BY id";
const PART_SQL: &str = "SELECT day, region, amount FROM part_day ORDER BY day, region";
const STR_SQL: &str = "SELECT k, note FROM str_key ORDER BY k";
const NZ_SQL: &str = "SELECT id, qty FROM nz_obs ORDER BY id";
const SAT_SQL: &str = "SELECT stored, literal_text FROM sat_cast";
const CANCEL_SQL: &str = "/* oo-dl01w1-43-cancel */ SELECT SLEEP(30)";
const DEADLINE_SQL: &str = "SELECT SLEEP(30) /* oo-dl01w1-43-deadline */";

struct Spec {
    name: &'static str,
    model: &'static str,
    sql: &'static str,
    columns: &'static [(&'static str, Kind, bool)],
}

pub async fn run(evidence: PathBuf, secrets: &str, endpoint: Endpoint) -> Result<(), Fail> {
    if let Err(err) = refuse_unapproved(&evidence) {
        let _ = session::write_json(
            &evidence.join("failure.json"),
            &json!({
                "code": err.code,
                "fact": err.fact,
                "ready": false,
                "source_acceptance": false
            }),
        );
        return Err(err);
    }
    let reader = session::load_secret(&std::path::Path::new(secrets).join("reader.password"), false)?;
    let audit = Arc::new(Audit::create(
        &evidence.join("client-audit.jsonl"),
        vec![reader.to_string()],
    )?);
    match run_inner(&evidence, &endpoint, reader.as_str(), &audit).await {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = session::write_json(
                &evidence.join("failure.json"),
                &json!({
                    "code": err.code,
                    "fact": err.fact,
                    "ready": false,
                    "source_acceptance": false
                }),
            );
            let _ = std::fs::remove_file(evidence.join("ready.json"));
            Err(err)
        }
    }
}

async fn run_inner(evidence: &std::path::Path, endpoint: &Endpoint, password: &str, audit: &Arc<Audit>) -> Result<(), Fail> {
    prove_guards(evidence)?;
    let mut link = open(endpoint, password, audit, evidence).await?;
    let catalog = catalog(&mut link, audit, &endpoint.db).await?;
    session::write_json(&evidence.join("catalog.json"), &catalog)?;
    for spec in specs() {
        let statement = business_statement("snapshot", None, spec.sql)?;
        let (rows, reason, _) = link.query_bound(&statement, audit, None, None).await?;
        if reason.is_some() {
            return Err(Fail::new("bound", spec.name));
        }
        let cells = convert_rows(&rows, spec)?;
        let bytes = ipc::write_file(&arrow_columns(spec), &cells, metadata(spec, &statement, true))?;
        publish(evidence, spec.name, &bytes)?;
    }
    write_cap(evidence, &mut link, audit, "row-cap", Some(2), None).await?;
    write_cap(evidence, &mut link, audit, "byte-cap", None, Some(32)).await?;
    saturation(evidence, &mut link, audit).await?;
    let product_reader = std::env::var("OO_DORIS_PRODUCT_READER").ok().as_deref() == Some("1");
    if product_reader {
        session::write_json(
            &evidence.join("denied.json"),
            &json!({
                "product_reader": true,
                "trial_writes": false,
                "statements": []
            }),
        )?;
    } else {
        denied(evidence, &mut link, audit).await?;
    }
    link.close(audit).await;
    let mut confirm = open(endpoint, password, audit, evidence).await?;
    confirm_counts(&mut confirm, audit, evidence).await?;
    confirm.close(audit).await;
    let deadline_result = deadline(evidence, endpoint, password, audit).await;
    let cancel_body = cancel(evidence, endpoint, password, audit).await?;
    deadline_result?;
    if cancel_body["status"] == "failed" {
        return Err(Fail::new("cancel", "sleep finished"));
    }
    session::write_json(
        &evidence.join("ready.json"),
        &json!({
            "protocol": "oo.doris.reader-run/v0",
            "source_acceptance": false,
            "author_complete_review_deferred": true,
            "engine": "apache-doris",
            "driver": DRIVER,
            "select_version_is_product_identity": false,
            "final_keyword_required": false,
            "database": endpoint.db,
            "product_reader": product_reader,
            "trial_ddl": !product_reader,
            "cancel": cancel_body,
            "tables": specs().iter().map(|spec| spec.name).collect::<Vec<_>>(),
            "ready": false,
            "ready_reason": "fe_audit_coverage_is_outside_the_reader"
        }),
    )?;
    Ok(())
}

fn prove_guards(evidence: &std::path::Path) -> Result<(), Fail> {
    let narrow = CastRequest {
        source_precision: 38,
        source_scale: 18,
        target_precision: 10,
        target_scale: 2,
    };
    let cast_blocked = matches!(
        business_statement("snapshot", Some(narrow), DEC_SQL),
        Err(err) if err.code == "UNSAFE_CAST"
    );
    let cursors_blocked = ["binlog", "gtid:1", "stream", "change"].iter().all(|cursor| {
        matches!(business_statement(cursor, None, DUP_SQL), Err(err) if err.code == "CURSOR_UNSUPPORTED")
    });
    let final_blocked = matches!(
        business_statement("snapshot", None, "SELECT FINAL"),
        Err(err) if err.code == "final_keyword"
    );
    let allowed = !business_statement("snapshot", None, DUP_SQL)?.contains("FINAL");
    session::write_json(
        &evidence.join("blocks.json"),
        &json!({
            "unsafe_cast_blocked": cast_blocked,
            "cursors_blocked": cursors_blocked,
            "final_keyword_blocked": final_blocked,
            "snapshot_select_allowed": allowed
        }),
    )?;
    if cast_blocked && cursors_blocked && final_blocked && allowed {
        Ok(())
    } else {
        Err(Fail::new("guard", "pre-connection block failed"))
    }
}

fn specs() -> &'static [Spec] {
    const DUP: &[(&str, Kind, bool)] = &[
        ("event_id", Kind::Int64, false),
        ("kind", Kind::Utf8, false),
        ("qty", Kind::Decimal { precision: 10, scale: 2 }, true),
    ];
    const AGG: &[(&str, Kind, bool)] = &[
        ("sku", Kind::Utf8, false),
        ("qty", Kind::Decimal { precision: 10, scale: 2 }, true),
        ("note", Kind::Utf8, true),
    ];
    const UNIQ: &[(&str, Kind, bool)] = &[
        ("id", Kind::Int64, false),
        ("balance", Kind::Decimal { precision: 18, scale: 4 }, true),
        ("label", Kind::Utf8, true),
    ];
    const DEC: &[(&str, Kind, bool)] = &[
        ("id", Kind::Int64, false),
        ("a18", Kind::Decimal { precision: 18, scale: 4 }, true),
        ("b18", Kind::Decimal { precision: 18, scale: 4 }, true),
        ("d38", Kind::Decimal { precision: 38, scale: 18 }, true),
    ];
    const PART: &[(&str, Kind, bool)] = &[
        ("day", Kind::Date, false),
        ("region", Kind::Utf8, false),
        ("amount", Kind::Decimal { precision: 12, scale: 2 }, true),
    ];
    const STR: &[(&str, Kind, bool)] = &[
        ("k", Kind::Utf8, false),
        ("note", Kind::Utf8, true),
    ];
    const NZ: &[(&str, Kind, bool)] = &[
        ("id", Kind::Int64, false),
        ("qty", Kind::Decimal { precision: 10, scale: 2 }, true),
    ];
    const SPECS: &[Spec] = &[
        Spec { name: "dup_events", model: "DUPLICATE", sql: DUP_SQL, columns: DUP },
        Spec { name: "agg_qty", model: "AGGREGATE", sql: AGG_SQL, columns: AGG },
        Spec { name: "uniq_acct", model: "UNIQUE", sql: UNIQ_SQL, columns: UNIQ },
        Spec { name: "dec_exact", model: "DUPLICATE", sql: DEC_SQL, columns: DEC },
        Spec { name: "part_day", model: "DUPLICATE", sql: PART_SQL, columns: PART },
        Spec { name: "str_key", model: "DUPLICATE", sql: STR_SQL, columns: STR },
        Spec { name: "nz_obs", model: "DUPLICATE", sql: NZ_SQL, columns: NZ },
    ];
    SPECS
}

fn arrow_columns(spec: &Spec) -> Vec<ArrowColumn> {
    spec.columns
        .iter()
        .map(|(name, kind, nullable)| ArrowColumn {
            name,
            kind: *kind,
            nullable: *nullable,
        })
        .collect()
}

fn metadata(spec: &Spec, statement: &str, lossless: bool) -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert("oo.doris.driver".to_string(), DRIVER.to_string());
    map.insert("oo.doris.model".to_string(), spec.model.to_string());
    map.insert("oo.doris.final_keyword_required".to_string(), "false".to_string());
    map.insert("oo.doris.lossless".to_string(), if lossless { "true" } else { "false" }.to_string());
    map.insert("oo.doris.table".to_string(), spec.name.to_string());
    map.insert("oo.doris.statement".to_string(), statement.to_string());
    map
}

fn convert_rows(rows: &[Row], spec: &Spec) -> Result<Vec<Vec<Cell>>, Fail> {
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        if row.len() != spec.columns.len() {
            return Err(Fail::new("row_width", spec.name));
        }
        let mut cells = Vec::with_capacity(spec.columns.len());
        for (index, (_, kind, _)) in spec.columns.iter().enumerate() {
            let value = row.as_ref(index).ok_or_else(|| Fail::new("cell", spec.name))?;
            cells.push(cell::from_value(value, *kind)?);
        }
        out.push(cells);
    }
    Ok(out)
}

fn publish(dir: &std::path::Path, name: &str, bytes: &[u8]) -> Result<(), Fail> {
    let partial = dir.join(format!("{name}.arrow.partial"));
    let final_path = dir.join(format!("{name}.arrow"));
    std::fs::write(&partial, bytes).map_err(|err| Fail::new("ipc", err.to_string()))?;
    std::fs::rename(&partial, &final_path).map_err(|err| Fail::new("ipc", err.to_string()))?;
    Ok(())
}

async fn open(endpoint: &Endpoint, password: &str, audit: &Audit, evidence: &std::path::Path) -> Result<Link, Fail> {
    session::connect_retry(endpoint, "oo_reader", password, audit, 48, &evidence.join("reader-connect.log")).await
}

async fn catalog(link: &mut Link, audit: &Audit, db: &str) -> Result<Json, Fail> {
    let version_rows = link.query("SELECT VERSION()", audit).await?;
    let select_version = version_rows
        .first()
        .ok_or_else(|| Fail::new("version", "empty"))
        .and_then(|row| session::text_at(row, 0))?;
    let frontends = session::rows_texts(&link.query("SHOW FRONTENDS", audit).await?)?;
    let backends = session::rows_texts(&link.query("SHOW BACKENDS", audit).await?)?;
    let tables_sql = format!(
        "SELECT table_schema, table_name, table_type FROM information_schema.tables WHERE table_schema = '{db}'"
    );
    let columns_sql = format!(
        "SELECT table_schema, table_name, column_name, data_type, column_type, is_nullable, column_key, ordinal_position FROM information_schema.columns WHERE table_schema = '{db}' ORDER BY table_name, ordinal_position"
    );
    let tables = session::rows_texts(&link.query(&tables_sql, audit).await?)?;
    let columns = session::rows_texts(&link.query(&columns_sql, audit).await?)?;
    let mut creates = Vec::new();
    for spec in specs() {
        let sql = format!("SHOW CREATE TABLE {}", spec.name);
        let rows = link.query(&sql, audit).await?;
        let text = session::rows_texts(&rows)?
            .into_iter()
            .flatten()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n");
        let model = classify_model(&text);
        if model != spec.model {
            return Err(Fail::new("model", format!("{} {model}", spec.name)));
        }
        creates.push(json!({
            "table": spec.name,
            "model": model,
            "create": text,
            "constraint_trust": "absent",
            "physical_hint": model
        }));
    }
    let partitions = session::rows_texts(&link.query("SHOW PARTITIONS FROM part_day", audit).await?)?;
    let joined = partitions
        .iter()
        .flatten()
        .filter_map(|cell| cell.clone())
        .collect::<Vec<_>>()
        .join("\n");
    let half_open = joined.contains("2026-03-01")
        && joined.contains("2026-04-01")
        && joined.contains("2026-05-01")
        && joined.contains(')');
    if !half_open {
        let sample: String = joined.chars().take(400).collect();
        return Err(Fail::new("partition", sample));
    }
    Ok(json!({
        "protocol": "oo.doris.catalog/v0",
        "engine": "apache-doris",
        "driver": DRIVER,
        "select_version": select_version,
        "select_version_is_product_identity": false,
        "database": db,
        "frontends": frontends,
        "backends": backends,
        "tables": tables,
        "columns": columns,
        "creates": creates,
        "partitions": partitions,
        "partition_half_open": true,
        "final_keyword_required": false,
        "empty_string_is_null": false,
        "constraint_trust": "absent",
        "source_acceptance": false
    }))
}

fn classify_model(create_sql: &str) -> &'static str {
    let folded = create_sql.to_ascii_uppercase();
    if folded.contains("AGGREGATE KEY") {
        "AGGREGATE"
    } else if folded.contains("UNIQUE KEY") {
        "UNIQUE"
    } else if folded.contains("DUPLICATE KEY") {
        "DUPLICATE"
    } else {
        "UNKNOWN"
    }
}

async fn write_cap(
    evidence: &std::path::Path,
    link: &mut Link,
    audit: &Audit,
    name: &str,
    max_rows: Option<usize>,
    max_bytes: Option<usize>,
) -> Result<(), Fail> {
    let statement = business_statement("snapshot", None, DUP_SQL)?;
    let (rows, reason, bytes) = link.query_bound(&statement, audit, max_rows, max_bytes).await?;
    let arrow = evidence.join(format!("{name}.arrow"));
    let partial = evidence.join(format!("{name}.arrow.partial"));
    let _ = std::fs::remove_file(&arrow);
    let _ = std::fs::remove_file(&partial);
    if reason.is_none() {
        return Err(Fail::new("cap", format!("{name} did not stop the read")));
    }
    if arrow.exists() || partial.exists() {
        return Err(Fail::new("cap", "arrow published"));
    }
    session::write_json(
        &evidence.join(format!("{name}.json")),
        &json!({
            "ready": false,
            "reason": reason,
            "kept_rows": rows.len(),
            "bytes": bytes,
            "max_rows": max_rows,
            "max_bytes": max_bytes,
            "arrow": false
        }),
    )
}

async fn saturation(evidence: &std::path::Path, link: &mut Link, audit: &Audit) -> Result<(), Fail> {
    let statement = business_statement("snapshot", None, SAT_SQL)?;
    let read = link.query(&statement, audit).await;
    let mut stored = None;
    let mut literal = None;
    let mut error = None;
    match read {
        Ok(rows) => {
            if let Some(row) = rows.first() {
                stored = session::text_at(row, 0).ok();
                literal = session::text_at(row, 1).ok();
            }
        }
        Err(err) => error = Some(err.fact),
    }
    session::write_json(
        &evidence.join("saturation.json"),
        &json!({
            "lossless": false,
            "ready": false,
            "stored_text": stored,
            "literal_text": literal,
            "error": error,
            "note": "saturation is not precision-safe"
        }),
    )
}

async fn denied(evidence: &std::path::Path, link: &mut Link, audit: &Audit) -> Result<(), Fail> {
    let statements = [
        "INSERT INTO dup_events VALUES (99, 'x', NULL)",
        "UPDATE dup_events SET kind = 'z' WHERE event_id = 11",
        "DELETE FROM dup_events WHERE event_id = 11",
        "CREATE TABLE reader_denied (id INT) DISTRIBUTED BY HASH(id) BUCKETS 1 PROPERTIES (\"replication_num\" = \"1\")",
        "DROP TABLE dup_events",
        "ALTER TABLE dup_events ADD COLUMN extra INT NULL",
        "GRANT SELECT_PRIV ON internal.oo_dl01w1_43.* TO 'oo_reader'",
    ];
    let mut results = Vec::new();
    for sql in statements {
        match link.exec(sql, audit).await {
            Ok(()) => return Err(Fail::new("denied", format!("succeeded: {sql}"))),
            Err(err) => results.push(json!({"sql": sql, "ok": false, "error": err.fact})),
        }
    }
    session::write_json(&evidence.join("denied.json"), &json!({"statements": results}))
}

fn refuse_unapproved(evidence: &std::path::Path) -> Result<(), Fail> {
    let secret = std::env::var("OO_DORIS_SECRET_REF").unwrap_or_default();
    let approved_raw = std::env::var("OO_DORIS_APPROVED_OBJECTS").unwrap_or_default();
    let mut approved = BTreeSet::new();
    for part in approved_raw.split(',') {
        let name = part.trim();
        if !name.is_empty() {
            approved.insert(name.to_string());
        }
    }
    let secret_ok = secret.starts_with("secret://")
        && secret.len() > "secret://".len()
        && !secret.chars().any(char::is_whitespace);
    if !secret_ok || approved.is_empty() {
        session::write_json(
            &evidence.join("object-refusal.json"),
            &json!({
                "reason": "customer_input",
                "source_read": false
            }),
        )?;
        return Err(Fail::new("customer_input", "secret_ref or approved objects"));
    }
    let probe = std::env::var("OO_DORIS_PROBE_OBJECT").unwrap_or_default();
    if !probe.is_empty() && !approved.contains(&probe) {
        session::write_json(
            &evidence.join("object-refusal.json"),
            &json!({
                "exit": 1,
                "object": probe,
                "reason": "not_approved",
                "source_read": false
            }),
        )?;
        return Err(Fail::new("not_approved", probe));
    }
    Ok(())
}

async fn confirm_counts(link: &mut Link, audit: &Audit, evidence: &std::path::Path) -> Result<(), Fail> {
    let mut counts = Vec::new();
    for (table, expected) in [
        ("dec_exact", 3),
        ("dup_events", 5),
        ("agg_qty", 5),
        ("uniq_acct", 3),
        ("str_key", 7),
        ("nz_obs", 3),
        ("part_day", 3),
    ] {
        let sql = format!("SELECT COUNT(*) FROM {table}");
        let rows = link.query(&sql, audit).await?;
        let got = session::text_at(rows.first().ok_or_else(|| Fail::new("count", table))?, 0)?
            .parse::<i64>()
            .map_err(|_| Fail::new("count", table))?;
        counts.push(json!({"table": table, "count": got}));
        if got != expected {
            return Err(Fail::new("count", format!("{table} {got} != {expected}")));
        }
    }
    session::write_json(&evidence.join("confirm.json"), &json!({"counts": counts}))
}

async fn deadline(evidence: &std::path::Path, endpoint: &Endpoint, password: &str, audit: &Audit) -> Result<(), Fail> {
    let mut link = open(endpoint, password, audit, evidence).await?;
    link.exec("SET query_timeout = 1", audit).await?;
    let statement = business_statement("snapshot", None, DEADLINE_SQL)?;
    let outcome = timeout(Duration::from_secs(15), link.query(&statement, audit)).await;
    let (status, error) = match outcome {
        Ok(Err(err)) if timeout_text(&err.fact) => ("hit", Some(err.fact)),
        Ok(Err(err)) => ("other_error", Some(err.fact)),
        Ok(Ok(_)) => ("query_finished", None),
        Err(_) => ("client_timeout", Some("15s client timeout is not a server deadline".to_string())),
    };
    link.close(audit).await;
    session::write_json(
        &evidence.join("deadline.json"),
        &json!({"status": status, "error": error, "query_timeout": 1}),
    )?;
    if status == "hit" {
        Ok(())
    } else {
        Err(Fail::new("deadline", status))
    }
}

fn timeout_text(text: &str) -> bool {
    let folded = text.to_ascii_lowercase();
    folded.contains("timeout") || folded.contains("time out")
}

async fn cancel(evidence: &std::path::Path, endpoint: &Endpoint, password: &str, audit: &Arc<Audit>) -> Result<Json, Fail> {
    let endpoint_task = endpoint.clone();
    let password_task = password.to_string();
    let audit_task = Arc::clone(audit);
    let evidence_task = evidence.to_path_buf();
    let mut handle = tokio::spawn(async move {
        let mut link = open(&endpoint_task, &password_task, &audit_task, &evidence_task).await?;
        let result = link.query(CANCEL_SQL, &audit_task).await;
        link.close(&audit_task).await;
        result
    });
    sleep(Duration::from_secs(2)).await;
    let mut control = open(endpoint, password, audit, evidence).await?;
    let mut query_id = None;
    let mut process_error = None;
    for _ in 0..8 {
        match control.query("SHOW PROCESSLIST", audit).await {
            Ok(rows) => {
                query_id = find_query_id(&rows)?;
                if query_id.is_some() {
                    break;
                }
            }
            Err(err) => {
                process_error = Some(err.fact);
                break;
            }
        }
        sleep(Duration::from_secs(1)).await;
    }
    let kill_error = if let Some(id) = &query_id {
        let kill = format!("KILL QUERY \"{id}\"");
        match control.exec(&kill, audit).await {
            Ok(()) => None,
            Err(err) => Some(err.fact),
        }
    } else {
        None
    };
    control.close(audit).await;
    let sleeper = timeout(Duration::from_secs(12), &mut handle).await;
    if sleeper.is_err() {
        handle.abort();
    }
    let (status, error) = match sleeper {
        Ok(Ok(Err(err))) if err.fact.to_ascii_lowercase().contains("cancel query by user") => ("cancelled", Some(err.fact)),
        Ok(Ok(Err(err))) => ("pending", Some(err.fact)),
        Ok(Ok(Ok(_))) => ("failed", Some("sleep returned a row".to_string())),
        Ok(Err(err)) => ("pending", Some(err.to_string())),
        Err(_) => ("pending", Some("sleeper still running; client drop is not cancel proof".to_string())),
    };
    let body = json!({
        "status": status,
        "error": error,
        "query_id": query_id,
        "processlist_error": process_error,
        "kill_error": kill_error,
        "business_connections": 1,
        "control_connections": 1,
        "statement": "KILL QUERY \"<queryId>\"",
        "disconnect_is_not_success": true
    });
    session::write_json(&evidence.join("cancel.json"), &body)?;
    Ok(body)
}

fn find_query_id(rows: &[Row]) -> Result<Option<String>, Fail> {
    for row in rows {
        let info_at = session::column_index(row, &["info"]);
        let id_at = session::column_index(row, &["queryid", "query_id"]);
        let info = if let Some(index) = info_at {
            session::text_at(row, index).unwrap_or_default()
        } else if row.len() > 11 {
            session::text_at(row, 11).unwrap_or_default()
        } else {
            String::new()
        };
        if !info.contains("oo-dl01w1-43-cancel") {
            continue;
        }
        let raw = if let Some(index) = id_at {
            session::text_at(row, index)?
        } else if row.len() > 10 {
            session::text_at(row, 10)?
        } else {
            return Ok(None);
        };
        if raw.chars().all(|ch| ch.is_ascii_hexdigit() || ch == '-') && !raw.is_empty() {
            return Ok(Some(raw));
        }
        return Err(Fail::new("query_id", "unexpected characters"));
    }
    Ok(None)
}


