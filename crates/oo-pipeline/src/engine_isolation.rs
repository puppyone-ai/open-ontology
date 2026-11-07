//! Closed table-failure classification and durable scheduled-cycle receipts.
use crate::engine_load::Result;
use oo_connectors::engine::TableRef;
use sha2::{Digest, Sha256};
use sqlx::{Connection, PgConnection};

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Unknown failures remain global. Never classify driver text by substring:
/// these are closed connector renderings, not SQL messages or row values.
pub fn stale_reason(error: &str) -> Option<&'static str> {
    match error {
        "c5_rebaseline_required_lineage"
        | "c5_rebaseline_required_plan"
        | "c5_rebaseline_required_active_sync" => return Some("rebaseline_required"),
        "schema_unproven" => return Some("schema_unproven"),
        "load_schema_drift" => return Some("schema_drift"),
        "c5_range_changed_retry" => return Some("source_changed"),
        _ => {}
    }
    let mut parts = error.strip_prefix("source error: ")?.split(' ');
    let kind = parts.next()?;
    let fact = parts.next()?.strip_prefix("fact=")?;
    if fact.is_empty()
        || !fact
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return None;
    }
    let state = parts.next();
    if state.is_some_and(|s| {
        s.strip_prefix("sqlstate=").is_none_or(|s| {
            s.len() != 5
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        })
    }) || parts.next().is_some()
    {
        return None;
    }
    match (kind, fact) {
        // These catalogs hide tables on permission denial as well as removal.
        // Isolate the table, but do not invent which of the two happened.
        ("catalog", "missing_table" | "table_not_visible") => Some("schema_unproven"),
        (
            "privilege",
            "statement_denied"
            | "insufficient_privilege"
            | "permission_denied"
            | "ora_00942_not_readable"
            | "ora_01031_insufficient_privileges",
        ) => Some("source_permission"),
        (
            "budget",
            "client_statement_timeout"
            | "server_statement_timeout"
            | "call_timeout"
            | "timeout_connection_closed"
            | "chunk_deadline"
            | "statement_timeout"
            | "client_timeout"
            | "lock_timeout"
            | "ora_01013_call_cancelled",
        ) => Some("source_timeout"),
        (
            "budget",
            "chunk_over_budget"
            | "chunk_ipc_over_budget"
            | "single_read_over_budget"
            | "chunk_over_budget_split"
            | "digest_over_budget_split"
            | "single_row_cannot_split"
            | "single_read_cannot_split"
            | "chunk_count"
            | "sink_rows"
            | "sink_bytes"
            | "max_message"
            | "max_value"
            | "max_row"
            | "max_columns",
        ) => Some("table_budget"),
        // PostgreSQL's sanitized driver mapping uses a generic fact.
        ("privilege", "database") if state == Some("sqlstate=42501") => Some("source_permission"),
        ("budget", "database") if state == Some("sqlstate=57014") => Some("source_timeout"),
        _ => None,
    }
}

pub fn exit_code(healthy: usize, stale: usize) -> u8 {
    match (healthy, stale) {
        (_, 0) => 0,
        (0, _) => 4,
        _ => 3,
    }
}

pub async fn begin(
    conn: &mut PgConnection,
    source: &str,
    run: &str,
    tables: &[TableRef],
) -> Result<bool> {
    if tables.is_empty() {
        return Err("sync_scope");
    }
    let mut scope = tables.to_vec();
    scope.sort();
    if scope.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err("sync_scope");
    }
    let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock(hashtextextended($1,0))")
        .bind(format!("oo-load-cycle:{}", hash(source.as_bytes())))
        .fetch_one(&mut *conn)
        .await
        .map_err(|_| "sync_target")?;
    if !locked {
        return Err("sync_cycle_busy");
    }
    sqlx::raw_sql(
        "CREATE SCHEMA IF NOT EXISTS _oo_load; REVOKE ALL ON SCHEMA _oo_load FROM PUBLIC;",
    )
    .execute(&mut *conn)
    .await
    .map_err(|_| "sync_target")?;
    sqlx::raw_sql(include_str!("../migrations/0127_table_isolation.sql"))
        .execute(&mut *conn)
        .await
        .map_err(|_| "sync_target")?;
    let mut tx = conn.begin().await.map_err(|_| "sync_target")?;
    let source = hash(source.as_bytes());
    let scope = serde_json::to_string(&scope).map_err(|_| "sync_scope")?;
    sqlx::query(
        "INSERT INTO _oo_load.sync_cycles VALUES($1,$2,$3::jsonb,'running')
         ON CONFLICT(source,run) DO NOTHING",
    )
    .bind(&source)
    .bind(run)
    .bind(&scope)
    .execute(&mut *tx)
    .await
    .map_err(|_| "sync_target")?;
    let (old, state): (String, String) = sqlx::query_as(
        "SELECT scope::text,state FROM _oo_load.sync_cycles WHERE source=$1 AND run=$2 FOR UPDATE",
    )
    .bind(&source)
    .bind(run)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| "sync_target")?;
    if serde_json::from_str::<Vec<TableRef>>(&old).map_err(|_| "sync_scope")?
        != tables_sorted(tables)
    {
        return Err("sync_scope_changed");
    }
    if state != "running" {
        tx.commit().await.map_err(|_| "sync_target")?;
        return Ok(false);
    }
    sqlx::query("UPDATE _oo_load.sync_cycles SET state='running' WHERE source=$1 AND run=$2")
        .bind(&source)
        .bind(run)
        .execute(&mut *tx)
        .await
        .map_err(|_| "sync_target")?;
    sqlx::query("DELETE FROM _oo_load.sync_table_results WHERE source=$1 AND run=$2")
        .bind(&source)
        .bind(run)
        .execute(&mut *tx)
        .await
        .map_err(|_| "sync_target")?;
    tx.commit().await.map_err(|_| "sync_target")?;
    Ok(true)
}

pub async fn replay(
    conn: &mut PgConnection,
    source: &str,
    run: &str,
) -> Result<Vec<serde_json::Value>> {
    let superseded: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM _oo_load.sync_table_results r
         LEFT JOIN _oo_load.current c USING(identity)
         WHERE r.source=$1 AND r.run=$2 AND c.run IS DISTINCT FROM r.retained_run)",
    )
    .bind(hash(source.as_bytes()))
    .bind(run)
    .fetch_one(&mut *conn)
    .await
    .map_err(|_| "sync_target")?;
    if superseded {
        return Err("sync_run_superseded");
    }
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT jsonb_build_object('table',table_ref,'retained_run',retained_run,
         'status',CASE WHEN state='stale' THEN 'stale' ELSE 'noop' END,
         'stale_reason',reason,'stale_detail',CASE WHEN state='stale'
            THEN 'Table failed twice; previous retained generation preserved.' END,
         'replay',true,'new_generations',0,'target_write_bytes',0,
         'generation',(SELECT count(*) FROM _oo_load.publications p WHERE p.identity=r.identity))::text
         FROM _oo_load.sync_table_results r WHERE source=$1 AND run=$2 ORDER BY table_ref::text",
    ).bind(hash(source.as_bytes())).bind(run).fetch_all(&mut *conn).await.map_err(|_| "sync_target")?;
    rows.iter()
        .map(|r| serde_json::from_str(r).map_err(|_| "sync_table_state"))
        .collect()
}

fn tables_sorted(tables: &[TableRef]) -> Vec<TableRef> {
    let mut tables = tables.to_vec();
    tables.sort();
    tables
}

/// Record only a completed table attempt. Reason/detail never contain the
/// underlying error. Preserve the start of an uninterrupted stale interval.
pub async fn record(
    conn: &mut PgConnection,
    source: &str,
    run: &str,
    table: &TableRef,
    state: &str,
    reason: Option<&str>,
) -> Result<()> {
    if !matches!(state, "fresh" | "noop" | "stale") || (state == "stale") != reason.is_some() {
        return Err("sync_table_state");
    }
    let identity = hash(&serde_json::to_vec(&(source, table)).map_err(|_| "sync_scope")?);
    let mut tx = conn.begin().await.map_err(|_| "sync_target")?;
    sqlx::query(
        "INSERT INTO _oo_load.table_health(identity,state,reason,since)
         VALUES($1,$2,$3,CASE WHEN $2='stale' THEN clock_timestamp() END)
         ON CONFLICT(identity) DO UPDATE SET state=EXCLUDED.state,reason=EXCLUDED.reason,
         since=CASE WHEN EXCLUDED.state='stale'
            THEN coalesce(_oo_load.table_health.since,EXCLUDED.since) END",
    )
    .bind(&identity)
    .bind(state)
    .bind(reason)
    .execute(&mut *tx)
    .await
    .map_err(|_| "sync_target")?;
    sqlx::query(
        "INSERT INTO _oo_load.sync_table_results
         SELECT $1,$2,h.identity,$3::jsonb,c.run,h.state,h.reason,h.since
         FROM _oo_load.table_health h LEFT JOIN _oo_load.current c USING(identity)
         WHERE h.identity=$4",
    )
    .bind(hash(source.as_bytes()))
    .bind(run)
    .bind(serde_json::to_string(table).map_err(|_| "sync_scope")?)
    .bind(&identity)
    .execute(&mut *tx)
    .await
    .map_err(|_| "sync_target")?;
    tx.commit().await.map_err(|_| "sync_target")
}

pub async fn finish(conn: &mut PgConnection, source: &str, run: &str) -> Result<u8> {
    let source = hash(source.as_bytes());
    let mut tx = conn.begin().await.map_err(|_| "sync_target")?;
    let count: i32 = sqlx::query_scalar(
        "SELECT jsonb_array_length(scope) FROM _oo_load.sync_cycles
         WHERE source=$1 AND run=$2 FOR UPDATE",
    )
    .bind(&source)
    .bind(run)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| "sync_target")?;
    let (healthy, stale): (i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER(WHERE state<>'stale'),count(*) FILTER(WHERE state='stale')
         FROM _oo_load.sync_table_results WHERE source=$1 AND run=$2",
    )
    .bind(&source)
    .bind(run)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| "sync_target")?;
    if healthy + stale != i64::from(count) {
        return Err("sync_incomplete");
    }
    let code = exit_code(healthy as usize, stale as usize);
    let state = match code {
        0 => "succeeded",
        3 => "partial",
        _ => "failed",
    };
    sqlx::query("UPDATE _oo_load.sync_cycles SET state=$3 WHERE source=$1 AND run=$2")
        .bind(&source)
        .bind(run)
        .bind(state)
        .execute(&mut *tx)
        .await
        .map_err(|_| "sync_target")?;
    tx.commit().await.map_err(|_| "sync_target")?;
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_exit_contract() {
        for (healthy, stale, expected) in [
            (0, 0, 0),
            (1, 0, 0),
            (5, 0, 0),
            (1, 1, 3),
            (1, 5, 3),
            (0, 1, 4),
            (0, 5, 4),
        ] {
            assert_eq!(exit_code(healthy, stale), expected);
        }
    }

    #[test]
    fn global_and_unknown_errors_are_never_swallowed() {
        for error in [
            "load_target",
            "sync_target",
            "c5_target_close",
            "sync_lineage_permission_required",
            "source error: auth fact=statement_denied",
            "source error: connect fact=statement_timeout",
            "source error: budget fact=session_setup_timeout",
            "source error: privilege fact=probe",
            "source error: budget fact=source_busy",
            "source error: privilege fact=statement_denied row=private",
            "source error: budget fact=statement_timeout\nprivate",
        ] {
            assert_eq!(stale_reason(error), None, "{error}");
        }
    }

    #[test]
    fn closed_table_codes_are_classified() {
        for (error, reason) in [
            ("c5_rebaseline_required_lineage", "rebaseline_required"),
            ("schema_unproven", "schema_unproven"),
            (
                "source error: catalog fact=missing_table",
                "schema_unproven",
            ),
            (
                "source error: catalog fact=table_not_visible",
                "schema_unproven",
            ),
            (
                "source error: privilege fact=statement_denied",
                "source_permission",
            ),
            (
                "source error: budget fact=server_statement_timeout",
                "source_timeout",
            ),
            ("source error: budget fact=call_timeout", "source_timeout"),
            (
                "source error: budget fact=chunk_ipc_over_budget",
                "table_budget",
            ),
        ] {
            assert_eq!(stale_reason(error), Some(reason));
        }
    }
}
