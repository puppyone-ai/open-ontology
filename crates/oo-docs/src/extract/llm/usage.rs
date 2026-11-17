//! Call reservation, caps and usage rows (10 §5.2.4, D19).
//!
//! A call is reserved before it is sent: one statement raises
//! `llm_usage.reserved` for (Drive, UTC day) only while it is below the
//! Drive's daily limit, so an interrupted request still counts. The per-file
//! cap is counted by the caller (the extraction checkpoint). `completed`,
//! `failed`, `cache_hits` and the token counts follow the outcome.

use serde_json::{Value, json};
use sqlx::{PgConnection, Row};

use crate::store::StoreError;

/// Result of asking for a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reserve {
    Granted {
        used: i32,
    },
    /// The Drive's daily limit is used up.
    DriveLimit {
        limit: i32,
    },
}

const DAY: &str = "(clock_timestamp() AT TIME ZONE 'UTC')::date";

/// Reserve one call for `drive_id` against `daily_limit`.
pub async fn reserve(
    conn: &mut PgConnection,
    drive_id: &str,
    daily_limit: i32,
) -> Result<Reserve, StoreError> {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO _oo_docs.llm_usage (drive_id, day) VALUES ($1::uuid, {DAY}) ON CONFLICT DO NOTHING"
    )))
    .bind(drive_id)
    .execute(&mut *conn)
    .await?;
    let used: Option<i32> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "UPDATE _oo_docs.llm_usage SET reserved = reserved + 1 \
          WHERE drive_id = $1::uuid AND day = {DAY} AND reserved < $2 RETURNING reserved"
    )))
    .bind(drive_id)
    .bind(daily_limit)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(match used {
        Some(used) => Reserve::Granted { used },
        None => Reserve::DriveLimit { limit: daily_limit },
    })
}

/// Which counter moves.
#[derive(Clone, Copy, Debug)]
pub enum Outcome {
    Completed {
        tokens_in: u64,
        tokens_out: u64,
    },
    Failed,
    CacheHit,
    /// The file hit its own cap (counted once per file).
    FileLimited,
}

/// Record the outcome of a call.
pub async fn record(conn: &mut PgConnection, drive_id: &str, o: Outcome) -> Result<(), StoreError> {
    let (col, ti, to): (&str, i64, i64) = match o {
        Outcome::Completed {
            tokens_in,
            tokens_out,
        } => (
            "completed",
            i64::try_from(tokens_in).unwrap_or(i64::MAX),
            i64::try_from(tokens_out).unwrap_or(i64::MAX),
        ),
        Outcome::Failed => ("failed", 0, 0),
        Outcome::CacheHit => ("cache_hits", 0, 0),
        Outcome::FileLimited => ("files_limited", 0, 0),
    };
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO _oo_docs.llm_usage (drive_id, day, {col}, tokens_in, tokens_out) \
         VALUES ($1::uuid, {DAY}, 1, $2, $3) \
         ON CONFLICT (drive_id, day) DO UPDATE SET {col} = _oo_docs.llm_usage.{col} + 1, \
            tokens_in = _oo_docs.llm_usage.tokens_in + $2, tokens_out = _oo_docs.llm_usage.tokens_out + $3"
    )))
    .bind(drive_id)
    .bind(ti)
    .bind(to)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// `GET /usage` body: per Drive and day for the last `days` days.
pub async fn read(
    conn: &mut PgConnection,
    drive_id: Option<&str>,
    days: i32,
    per_file_limit: u32,
) -> Result<Value, StoreError> {
    let rows = sqlx::query(
        "SELECT u.drive_id::text AS drive_id, u.day::text AS day, u.reserved, u.cache_hits, \
                u.tokens_in, u.tokens_out, u.files_limited, d.llm_daily_calls \
           FROM _oo_docs.llm_usage u JOIN _oo_docs.drive d ON d.id = u.drive_id \
          WHERE ($1::uuid IS NULL OR u.drive_id = $1::uuid) \
            AND u.day > (clock_timestamp() AT TIME ZONE 'UTC')::date - $2::int \
          ORDER BY u.day DESC, u.drive_id",
    )
    .bind(drive_id)
    .bind(days)
    .fetch_all(&mut *conn)
    .await?;
    let drives: Vec<Value> = rows
        .iter()
        .map(|r| {
            Ok::<_, sqlx::Error>(json!({
                "drive_id": r.try_get::<String, _>("drive_id")?,
                "day": r.try_get::<String, _>("day")?,
                "calls": r.try_get::<i32, _>("reserved")?,
                "limit": r.try_get::<i32, _>("llm_daily_calls")?,
                "cache_hits": r.try_get::<i32, _>("cache_hits")?,
                "tokens_in": r.try_get::<i64, _>("tokens_in")?,
                "tokens_out": r.try_get::<i64, _>("tokens_out")?,
                "files_limited": r.try_get::<i32, _>("files_limited")?,
            }))
        })
        .collect::<Result<_, _>>()?;
    Ok(json!({"drives": drives, "per_file_limit": per_file_limit}))
}
