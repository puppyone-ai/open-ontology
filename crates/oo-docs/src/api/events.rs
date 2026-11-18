//! `GET /v1/docs/events`: server sent events with `since` replay (10 §8.5).
//!
//! Formal: FM4. Events are appended under the `event_head` row lock, which is
//! held until commit, so the committed events always form a prefix
//! `1..=head`. A reader that asks for `seq > cursor` therefore never skips an
//! event, and a `since` replay followed by the live tail is gap free: the
//! tail resumes from the last delivered `seq`, not from the head.

use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Response, StatusCode, header};
use futures_core::Stream;
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool, Row};
use tokio::sync::{Notify, OwnedSemaphorePermit, mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::error::{ApiError, code};
use super::http::FMT;
use super::{AppState, Req};
use crate::store::events::NOTIFY_CHANNEL;

/// Events per read.
const BATCH: i64 = 500;
/// More than this behind the head while live: `stream.lagging`.
const LAG_LIMIT: i64 = 10_000;

/// One event as stored, with its `at` as RFC 3339 text.
#[derive(Clone, Debug)]
pub struct Row3 {
    /// Sequence.
    pub seq: i64,
    /// `type`.
    pub kind: String,
    /// Drive id.
    pub drive_id: Option<String>,
    /// Subject (file id for file events).
    pub subject: Option<String>,
    /// Time.
    pub at: String,
    /// Stored payload.
    pub payload: Value,
}

/// Events with `seq > since`, ascending.
pub async fn read(conn: &mut PgConnection, since: i64, limit: i64) -> Result<Vec<Row3>, ApiError> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT seq, type, drive_id::text AS drive_id, subject, \
         to_char(at AT TIME ZONE 'UTC', {FMT}) AS at, payload::text AS payload \
         FROM _oo_docs.event WHERE seq > $1 ORDER BY seq LIMIT $2"
    )))
    .bind(since)
    .bind(limit)
    .fetch_all(&mut *conn)
    .await?;
    rows.iter()
        .map(|r| {
            let payload: String = r.try_get("payload")?;
            Ok(Row3 {
                seq: r.try_get("seq")?,
                kind: r.try_get("type")?,
                drive_id: r.try_get("drive_id")?,
                subject: r.try_get("subject")?,
                at: r.try_get("at")?,
                payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
            })
        })
        .collect()
}

/// `(oldest seq, head seq)`; `(0, 0)` when the log is empty.
pub async fn bounds(conn: &mut PgConnection) -> Result<(i64, i64), ApiError> {
    let (min, max): (Option<i64>, Option<i64>) =
        sqlx::query_as("SELECT min(seq), max(seq) FROM _oo_docs.event")
            .fetch_one(&mut *conn)
            .await?;
    Ok((min.unwrap_or(0), max.unwrap_or(0)))
}

/// Turn stored events into `Event` objects (`{seq,type,at,drive_id,data}`);
/// `file.state_changed` carries the current `FileSummary` of its file and
/// `share.created` / `share.state_changed` the current `Share` (`{share}`),
/// as the contract says, so clients need no second request.
pub async fn envelopes(conn: &mut PgConnection, rows: &[Row3]) -> Result<Vec<Value>, ApiError> {
    let file_ids: Vec<String> = rows
        .iter()
        .filter(|r| r.kind == "file.state_changed")
        .filter_map(|r| r.subject.clone())
        .collect();
    let summaries = super::files::summaries(conn, &file_ids).await?;
    let mut shares: std::collections::HashMap<String, Value> = Default::default();
    for r in rows {
        if matches!(r.kind.as_str(), "share.created" | "share.state_changed")
            && let Some(id) = r.subject.as_ref()
            && !shares.contains_key(id)
            && let Some(v) = crate::share::lifecycle::share_value(conn, id).await?
        {
            shares.insert(id.clone(), v);
        }
    }
    Ok(rows
        .iter()
        .map(|r| {
            let share = r.subject.as_ref().and_then(|id| shares.get(id));
            let data = match (r.kind.as_str(), r.subject.as_ref()) {
                ("file.state_changed", Some(id)) => match summaries.get(id) {
                    Some(file) => json!({ "file": file }),
                    None => r.payload.clone(),
                },
                ("share.created" | "share.state_changed", _) if share.is_some() => {
                    json!({ "share": share })
                }
                _ => r.payload.clone(),
            };
            // A share event names the Drive of its share even when the writer did not.
            let drive_id = match (&r.drive_id, share) {
                (None, Some(s)) if r.kind.starts_with("share.") => {
                    s["drive_id"].as_str().map(str::to_owned)
                }
                (d, _) => d.clone(),
            };
            json!({
                "seq": r.seq,
                "type": r.kind,
                "at": r.at,
                "drive_id": drive_id,
                "data": data,
            })
        })
        .collect())
}

/// One SSE frame.
pub fn frame(event: &Value) -> String {
    format!(
        "id: {}\nevent: {}\ndata: {}\n\n",
        event["seq"],
        event["type"].as_str().unwrap_or("message"),
        event
    )
}

/// Parse `types=a,b` into a filter; empty means everything.
pub fn parse_types(text: Option<&str>) -> Result<Vec<String>, ApiError> {
    let Some(text) = text.filter(|t| !t.is_empty()) else {
        return Ok(Vec::new());
    };
    text.split(',')
        .map(|t| {
            if !t.is_empty()
                && t.bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'_' || b == b'.')
            {
                Ok(t.to_owned())
            } else {
                Err(ApiError::bad(
                    "/query/types",
                    "expected a comma separated list of event types",
                ))
            }
        })
        .collect()
}

/// Cursor of the request: `since` wins over `Last-Event-ID`.
pub fn parse_since(q: &super::http::Query, headers: &HeaderMap) -> Result<Option<i64>, ApiError> {
    let text = match q.get("since") {
        Some(s) => Some(s.to_owned()),
        None => headers
            .get("last-event-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned),
    };
    match text {
        None => Ok(None),
        Some(t) => t
            .trim()
            .parse::<i64>()
            .ok()
            .filter(|n| *n >= 0)
            .map(Some)
            .ok_or_else(|| ApiError::bad("/query/since", "expected a sequence number")),
    }
}

struct Rx(mpsc::Receiver<Bytes>);

impl Stream for Rx {
    type Item = Result<Bytes, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.0.poll_recv(cx).map(|o| o.map(Ok))
    }
}

/// Timing of the stream.
#[derive(Clone, Copy, Debug)]
pub struct SseConfig {
    /// Idle ping period (15 s).
    pub ping: Duration,
    /// Fallback poll when no notification arrives (NOTIFY is advisory).
    pub poll: Duration,
}

impl Default for SseConfig {
    fn default() -> Self {
        Self {
            ping: Duration::from_secs(15),
            poll: Duration::from_secs(1),
        }
    }
}

/// `GET /v1/docs/events`.
pub async fn stream(app: &Arc<AppState>, req: &Req) -> Result<Response<Body>, ApiError> {
    let since = parse_since(&req.query, &req.headers)?;
    let types = parse_types(req.query.get("types"))?;
    let permit = app.sse_slots.clone().try_acquire_owned().map_err(|_| {
        ApiError::new(code::RATE_LIMITED, "too many event streams")
            .details(json!({ "retry_after_ms": 1000 }))
            .header("retry-after", "1".into())
    })?;
    let mut conn = app.pool.acquire().await?;
    let (oldest, head) = bounds(&mut conn).await?;
    let start = match since {
        // No cursor: live events only.
        None => head,
        Some(s) => {
            // The next event the client needs is s + 1; it must still exist.
            let gone = head > 0 && s + 1 < oldest;
            if s > head || gone {
                return Err(ApiError::new(
                    code::CURSOR_EXPIRED,
                    "since is outside the retained log",
                )
                .details(json!({ "oldest_seq": oldest, "head_seq": head })));
            }
            s
        }
    };
    drop(conn);
    let (tx, rx) = mpsc::channel::<Bytes>(64);
    let app = app.clone();
    tokio::spawn(async move {
        run(app, tx, permit, start, types, since.is_some()).await;
    });
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("x-accel-buffering", "no")
        .body(Body::from_stream(Rx(rx)))
        .map_err(|_| ApiError::new(code::INTERNAL, "response"))
}

async fn run(
    app: Arc<AppState>,
    tx: mpsc::Sender<Bytes>,
    _permit: OwnedSemaphorePermit,
    mut cursor: i64,
    types: Vec<String>,
    replay: bool,
) {
    let send = |text: String| {
        let tx = tx.clone();
        async move { tx.send(Bytes::from(text)).await.is_ok() }
    };
    // Subscribe before the client can see `retry:`, so a transient event
    // emitted after that frame is never missed.
    let mut progress = app.progress.subscribe();
    if !send("retry: 3000\n\n".to_owned()).await {
        return;
    }
    let cfg = app.sse;
    let mut stop = app.stop_signal();
    let mut last_sent = Instant::now();
    let mut caught_up = !replay;
    let wants_progress = types.is_empty() || types.iter().any(|t| t == "file.progress");
    loop {
        if *stop.borrow() {
            return;
        }
        let mut conn = match app.pool.acquire().await {
            Ok(c) => c,
            Err(_) => {
                let _ = send("retry: 5000\n\n".to_owned()).await;
                return;
            }
        };
        let batch = match read(&mut conn, cursor, BATCH).await {
            Ok(b) => b,
            Err(_) => {
                let _ = send("retry: 5000\n\n".to_owned()).await;
                return;
            }
        };
        let full = batch.len() as i64 == BATCH;
        if !batch.is_empty() {
            let wanted: Vec<Row3> = batch
                .iter()
                .filter(|r| types.is_empty() || types.contains(&r.kind))
                .cloned()
                .collect();
            let events = match envelopes(&mut conn, &wanted).await {
                Ok(e) => e,
                Err(_) => {
                    let _ = send("retry: 5000\n\n".to_owned()).await;
                    return;
                }
            };
            // Filtered events still move the cursor (`types` only filters
            // delivery, 8.5).
            cursor = batch.last().map_or(cursor, |r| r.seq);
            for e in &events {
                if !send(frame(e)).await {
                    return;
                }
            }
            last_sent = Instant::now();
        }
        if !full {
            caught_up = true;
        }
        if caught_up
            && let Ok((_, head)) = bounds(&mut conn).await
            && head - cursor > LAG_LIMIT
        {
            let lag = json!({
                "type": "stream.lagging",
                "at": chrono_free_now(),
                "data": { "seq": cursor, "head": head },
            });
            let _ = send(format!("event: stream.lagging\ndata: {lag}\n\n")).await;
            return;
        }
        drop(conn);
        if full {
            continue;
        }
        let idle = cfg.ping.saturating_sub(last_sent.elapsed());
        tokio::select! {
            _ = app.events.notified() => {}
            _ = stop.changed() => return,
            r = progress.recv(), if wants_progress => {
                // Transient: no id, not stored, a missed one is not replayed.
                if let Ok(p) = r
                    && !send(super::progress::frame(&p.event(&chrono_free_now()))).await
                {
                    return;
                }
            }
            _ = tokio::time::sleep(cfg.poll.min(idle.max(Duration::from_millis(1)))) => {}
        }
        if last_sent.elapsed() >= cfg.ping {
            if !send(": ping\n\n".to_owned()).await {
                return;
            }
            last_sent = Instant::now();
        }
        if tx.is_closed() {
            return;
        }
    }
}

/// RFC 3339 now (seconds precision), see [`crate::obs::now_rfc3339`].
fn chrono_free_now() -> String {
    crate::obs::now_rfc3339()
}

/// Listens on the NOTIFY channel and wakes every stream; reconnects with a
/// backoff. Streams also poll, so a lost notification only costs latency.
pub fn spawn_hub(
    pool: PgPool,
    wake: Arc<Notify>,
    mut stop: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut backoff = Duration::from_millis(500);
        loop {
            if *stop.borrow() {
                return;
            }
            if let Ok(mut l) = sqlx::postgres::PgListener::connect_with(&pool).await
                && l.listen(NOTIFY_CHANNEL).await.is_ok()
            {
                backoff = Duration::from_millis(500);
                loop {
                    tokio::select! {
                        r = l.recv() => match r {
                            Ok(_) => wake.notify_waiters(),
                            Err(_) => break,
                        },
                        _ = stop.changed() => return,
                    }
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(backoff) => {}
                _ = stop.changed() => return,
            }
            backoff = (backoff * 2).min(Duration::from_secs(10));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_carry_id_event_and_data() {
        let e =
            json!({"seq": 7, "type": "drive.created", "at": "2026-11-17T00:00:00Z", "data": {}});
        let f = frame(&e);
        assert!(f.starts_with("id: 7\nevent: drive.created\ndata: {"));
        assert!(f.ends_with("\n\n"));
        assert_eq!(f.matches('\n').count(), 4, "data must stay on one line");
    }

    #[test]
    fn since_beats_last_event_id() {
        let mut h = HeaderMap::new();
        h.insert("last-event-id", "5".parse().unwrap());
        let q = super::super::http::Query::parse(Some("since=9")).unwrap();
        assert_eq!(parse_since(&q, &h).unwrap(), Some(9));
        let none = super::super::http::Query::parse(None).unwrap();
        assert_eq!(parse_since(&none, &h).unwrap(), Some(5));
        assert_eq!(parse_since(&none, &HeaderMap::new()).unwrap(), None);
        let bad = super::super::http::Query::parse(Some("since=x")).unwrap();
        assert!(parse_since(&bad, &h).is_err());
        let neg = super::super::http::Query::parse(Some("since=-1")).unwrap();
        assert!(parse_since(&neg, &h).is_err());
    }

    #[test]
    fn types_filter_syntax() {
        assert!(parse_types(None).unwrap().is_empty());
        assert_eq!(
            parse_types(Some("file.state_changed,drive.created")).unwrap(),
            ["file.state_changed", "drive.created"]
        );
        assert!(parse_types(Some("a,,b")).is_err());
        assert!(parse_types(Some("A")).is_err());
    }

    #[test]
    fn civil_date() {
        let s = chrono_free_now();
        assert_eq!(s.len(), 20);
        assert!(s.starts_with("20") && s.ends_with('Z'));
    }
}
