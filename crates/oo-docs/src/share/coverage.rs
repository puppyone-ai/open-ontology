//! Freshness coverage of a compile (D54, 10 §16.2): measure how far the latest
//! ontology version covers the Drive's changes and the files of the scope, wait
//! up to `wait_ms` for it to catch up, and refuse an incomplete pack when the
//! caller asked for `require_complete`. Never silent: what is missing is named
//! in the pack's first lines and in `coverage` (in-scope files only, D26).

use std::time::{Duration, Instant};

use sqlx::PgConnection;

use crate::api::error::{ApiError, code};
use crate::compile::coverage::Coverage;

/// Defaults of 10 §16.2.
pub const DEFAULT_WAIT_MS: u64 = 20_000;
pub const MAX_WAIT_MS: u64 = 120_000;

/// The freshness parameters of a compile request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Freshness {
    /// `None`: the Drive's `rev` when the request arrived.
    pub min_drive_rev: Option<i64>,
    pub wait_ms: u64,
    pub require_complete: bool,
}

impl Default for Freshness {
    fn default() -> Self {
        Self {
            min_drive_rev: None,
            wait_ms: DEFAULT_WAIT_MS,
            require_complete: false,
        }
    }
}

/// Not settled: something is left to do before the file is in a version.
const PENDING: [&str; 6] = [
    "stored",
    "queued",
    "parsing",
    "parsed",
    "extracting",
    "indexed",
];
/// Settled without being fully read.
const FAILED: [&str; 5] = [
    "parse_failed",
    "extract_partial",
    "unsupported",
    "too_large",
    "encrypted",
];

fn scope_sql(col: &str) -> String {
    format!(
        "(cardinality($2::text[]) = 0 OR EXISTS (SELECT 1 FROM unnest($2::text[]) sp(p) \
           WHERE {col} = sp.p OR {col} LIKE replace(replace(replace(sp.p, '\\', '\\\\'), '%', '\\%'), '_', '\\_') || '/%'))"
    )
}

/// Measure now.
pub async fn measure(
    conn: &mut PgConnection,
    drive_id: &str,
    prefixes: &[String],
) -> Result<Coverage, sqlx::Error> {
    let drive_rev: i64 = sqlx::query_scalar("SELECT rev FROM _oo_docs.drive WHERE id = $1::uuid")
        .bind(drive_id)
        .fetch_optional(&mut *conn)
        .await?
        .unwrap_or(0);
    // Live, not the number stored at the last publish: a file that became
    // `duplicate` or `superseded` after it settles without a publish.
    let covered_rev = crate::store::rev::covers_drive_rev_live(conn)
        .await
        .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    let sql = format!(
        "SELECT f.path, f.state, f.state_code FROM _oo_docs.file f \
          WHERE f.drive_id = $1::uuid AND f.deleted_at IS NULL \
            AND f.state NOT IN ('deleted', 'superseded', 'duplicate') AND {} ORDER BY f.path",
        scope_sql("f.path")
    );
    let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(drive_id)
        .bind(prefixes)
        .fetch_all(&mut *conn)
        .await?;
    let mut c = Coverage {
        drive_rev,
        covered_rev,
        in_scope: rows.len() as u64,
        ..Coverage::default()
    };
    for (path, state, state_code) in rows {
        if FAILED.contains(&state.as_str()) {
            c.failed.push((path, state_code.unwrap_or(state)));
        } else if PENDING.contains(&state.as_str()) {
            c.pending.push((path, state));
        } else {
            c.processed += 1;
        }
    }
    Ok(c)
}

/// Wait for `min_drive_rev`, measure, and enforce `require_complete`.
pub async fn prepare(
    pool: &sqlx::PgPool,
    drive_id: &str,
    prefixes: &[String],
    f: &Freshness,
) -> Result<Coverage, ApiError> {
    let mut conn = pool.acquire().await?;
    let want = match f.min_drive_rev {
        Some(r) => r,
        None => sqlx::query_scalar("SELECT rev FROM _oo_docs.drive WHERE id = $1::uuid")
            .bind(drive_id)
            .fetch_optional(&mut *conn)
            .await?
            .unwrap_or(0),
    };
    let deadline = Instant::now() + Duration::from_millis(f.wait_ms.min(MAX_WAIT_MS));
    let mut c = measure(&mut conn, drive_id, prefixes).await?;
    while c.covered_rev < want && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
        c = measure(&mut conn, drive_id, prefixes).await?;
    }
    // The floor the caller asked for, not the Drive's later moves.
    c.drive_rev = c.drive_rev.max(want);
    if f.require_complete && !c.complete() {
        return Err(ApiError::new(
            code::PACK_COVERAGE_INCOMPLETE,
            "the ontology does not cover every file of the scope yet",
        )
        .details(c.to_json()));
    }
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_the_spec() {
        let f = Freshness::default();
        assert_eq!((f.wait_ms, f.require_complete), (20_000, false));
        assert!(MAX_WAIT_MS >= f.wait_ms);
    }
}
