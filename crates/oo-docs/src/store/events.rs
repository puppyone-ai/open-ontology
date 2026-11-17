//! Event outbox (10 §8.5): `append_event` and cursor reads.
//!
//! `append_event` takes the `event_head` row lock and holds it until the
//! caller commits, so `seq` order is commit order and a `since` reader never
//! skips a visible event. Call it last in a transaction.

use sqlx::{PgConnection, Row};

use super::StoreError;

/// `LISTEN` channel; the payload is the new `seq`.
pub const NOTIFY_CHANNEL: &str = "oo_docs_events";

/// One stored event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    /// Gap-free sequence, starting at 1.
    pub seq: i64,
    /// `<domain>.<name>`, e.g. `file.state_changed`.
    pub kind: String,
    /// Drive id text.
    pub drive_id: Option<String>,
    /// File, share or review id.
    pub subject: Option<String>,
    /// JSON object text.
    pub payload: String,
}

/// Append an event; returns its `seq`. `payload_json` is a JSON object (at
/// most 16 KiB, no top-level credential keys, else `OO010`).
pub async fn append_event(
    conn: &mut PgConnection,
    kind: &str,
    drive_id: Option<&str>,
    subject: Option<&str>,
    payload_json: &str,
) -> Result<i64, StoreError> {
    let seq = sqlx::query_scalar("SELECT _oo_docs.append_event($1, $2::uuid, $3, $4::jsonb)")
        .bind(kind)
        .bind(drive_id)
        .bind(subject)
        .bind(payload_json)
        .fetch_one(&mut *conn)
        .await?;
    Ok(seq)
}

/// Events with `seq > since`, ascending, at most `limit`.
pub async fn read_since(
    conn: &mut PgConnection,
    since: i64,
    limit: i64,
) -> Result<Vec<Event>, StoreError> {
    let rows = sqlx::query(
        "SELECT seq, type, drive_id::text AS drive_id, subject, payload::text AS payload \
         FROM _oo_docs.event WHERE seq > $1 ORDER BY seq LIMIT $2",
    )
    .bind(since)
    .bind(limit)
    .fetch_all(&mut *conn)
    .await?;
    rows.iter()
        .map(|r| {
            Ok(Event {
                seq: r.try_get("seq")?,
                kind: r.try_get("type")?,
                drive_id: r.try_get("drive_id")?,
                subject: r.try_get("subject")?,
                payload: r.try_get("payload")?,
            })
        })
        .collect()
}

/// Highest committed `seq` (0 when empty).
pub async fn head(conn: &mut PgConnection) -> Result<i64, StoreError> {
    Ok(
        sqlx::query_scalar("SELECT COALESCE(max(seq), 0)::bigint FROM _oo_docs.event")
            .fetch_one(&mut *conn)
            .await?,
    )
}
