//! Per file latency (D49, G13), derived from the append-only state history.
//!
//! Every transition has a timestamp, so the time a file spent in each state of
//! its current cycle (from the last `stored` entry on) is the per stage
//! duration: no extra columns and nothing to keep in step. `llm_wait_ms` is a
//! separate item (D49: model queueing is shown apart); it is the sum of the
//! `llm_wait_ms` numbers stages record in the history `detail`, 0 without a
//! model.

use std::collections::HashMap;

use serde_json::{Map, Value, json};
use sqlx::{PgConnection, Row};

use crate::api::error::ApiError;

/// One history row, reduced to what latency needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// `to_state`.
    pub to_state: String,
    /// Commit-side timestamp in milliseconds since the epoch.
    pub at_ms: i64,
    /// `detail.llm_wait_ms`, 0 when absent.
    pub llm_wait_ms: i64,
}

/// Stage name for the time spent in a state, `None` for states that are not
/// part of the path to `published`.
pub fn stage_of(state: &str) -> Option<&'static str> {
    Some(match state {
        "stored" => "hash_ms",
        "queued" => "queue_ms",
        "parsing" => "parse_ms",
        "parsed" => "extract_queue_ms",
        "extracting" => "extract_ms",
        "indexed" => "publish_wait_ms",
        _ => return None,
    })
}

/// Every stage name, in path order.
pub const STAGES: [&str; 6] = [
    "hash_ms",
    "queue_ms",
    "parse_ms",
    "extract_queue_ms",
    "extract_ms",
    "publish_wait_ms",
];

/// Per stage durations of the current cycle of a file (history in `seq`
/// order). `None` when there is no `stored` entry. `total_ms` is present only
/// when the cycle ended in `published`. Retries add up in the stage they
/// repeat.
pub fn stages(history: &[Entry]) -> Option<Map<String, Value>> {
    let start = history.iter().rposition(|e| e.to_state == "stored")?;
    let cycle = &history[start..];
    let mut out: Map<String, Value> = Map::new();
    let mut llm = 0;
    for pair in cycle.windows(2) {
        let (cur, next) = (&pair[0], &pair[1]);
        llm += cur.llm_wait_ms.max(0);
        if let Some(stage) = stage_of(&cur.to_state) {
            let d = (next.at_ms - cur.at_ms).max(0);
            let prev = out.get(stage).and_then(Value::as_i64).unwrap_or(0);
            out.insert(stage.to_owned(), json!(prev + d));
        }
    }
    if let Some(last) = cycle.last() {
        llm += last.llm_wait_ms.max(0);
        if last.to_state == "published" {
            out.insert(
                "total_ms".to_owned(),
                json!((last.at_ms - cycle[0].at_ms).max(0)),
            );
        }
    }
    out.insert("llm_wait_ms".to_owned(), json!(llm));
    Some(out)
}

/// `latency_ms` objects for files, by id. Files without a history are absent.
pub async fn for_files(
    conn: &mut PgConnection,
    ids: &[String],
) -> Result<HashMap<String, Value>, ApiError> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = sqlx::query(
        "SELECT file_id::text AS id, to_state, \
                (extract(epoch FROM at) * 1000)::bigint AS ms, \
                COALESCE((detail->>'llm_wait_ms')::bigint, 0) AS llm \
           FROM _oo_docs.file_state_history WHERE file_id = ANY($1::text[]::uuid[]) \
          ORDER BY file_id, seq",
    )
    .bind(ids)
    .fetch_all(&mut *conn)
    .await?;
    let mut by_file: HashMap<String, Vec<Entry>> = HashMap::new();
    for r in rows {
        by_file.entry(r.try_get("id")?).or_default().push(Entry {
            to_state: r.try_get("to_state")?,
            at_ms: r.try_get("ms")?,
            llm_wait_ms: r.try_get("llm")?,
        });
    }
    Ok(by_file
        .into_iter()
        .filter_map(|(id, h)| stages(&h).map(|m| (id, Value::Object(m))))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(state: &str, at_ms: i64) -> Entry {
        Entry {
            to_state: state.into(),
            at_ms,
            llm_wait_ms: 0,
        }
    }

    #[test]
    fn a_published_cycle_has_every_stage_and_a_total() {
        let h = [
            e("stored", 1_000),
            e("queued", 1_040),
            e("parsing", 1_300),
            e("parsed", 1_360),
            e("extracting", 1_400),
            e("indexed", 1_500),
            e("published", 3_500),
        ];
        let m = stages(&h).unwrap();
        assert_eq!(m["hash_ms"], 40);
        assert_eq!(m["queue_ms"], 260);
        assert_eq!(m["parse_ms"], 60);
        assert_eq!(m["extract_queue_ms"], 40);
        assert_eq!(m["extract_ms"], 100);
        assert_eq!(m["publish_wait_ms"], 2_000);
        assert_eq!(m["total_ms"], 2_500);
        assert_eq!(m["llm_wait_ms"], 0);
        let sum: i64 = STAGES.iter().map(|s| m[*s].as_i64().unwrap()).sum();
        assert_eq!(sum, 2_500, "the stages add up to the total");
    }

    #[test]
    fn only_the_current_cycle_counts_and_retries_add_up() {
        let h = [
            e("stored", 0),
            e("queued", 10),
            e("published", 20),
            e("superseded", 100),
            e("stored", 200),
            e("queued", 210),
            e("parsing", 220),
            e("queued", 300),
            e("parsing", 1_300),
        ];
        let m = stages(&h).unwrap();
        assert_eq!(m["hash_ms"], 10);
        assert_eq!(m["queue_ms"], 10 + 1_000, "both waits in queued");
        assert_eq!(m["parse_ms"], 80);
        assert!(m.get("total_ms").is_none(), "not published yet");
        assert!(stages(&[e("queued", 1)]).is_none());
    }

    #[test]
    fn llm_wait_is_its_own_item() {
        let mut h = vec![
            e("stored", 0),
            e("queued", 1),
            e("parsing", 2),
            e("parsed", 3),
            e("extracting", 4),
        ];
        h[4].llm_wait_ms = 700;
        let m = stages(&h).unwrap();
        assert_eq!(m["llm_wait_ms"], 700);
    }
}
